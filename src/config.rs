use std::{collections::HashMap, convert::TryFrom};

use linked_hash_map::LinkedHashMap;
use regex::RegexSetBuilder;
use slug::slugify;
use xxhash_rust::xxh3::xxh3_64;
use yaml_rust::Yaml;

use crate::model::{
    AppConfig, Config, Feed, Filter, Group, SCOPE_BODY, SCOPE_SUMMARY, SCOPE_TITLE,
};

fn get_string_field_from_map(
    map: &LinkedHashMap<Yaml, Yaml>,
    field: String,
    required: bool,
    yaml_path: Option<String>,
) -> String {
    if let Some(value) = map.get(&Yaml::String(field)) {
        return value.as_str().unwrap().to_string();
    }
    if required {
        print!(
            "Field missing in config file: {}",
            yaml_path.unwrap_or_else(|| "UNKNOWN".to_string())
        );
        std::process::exit(1);
    }
    String::with_capacity(0)
}

fn load_globals(config: &mut AppConfig, map: &LinkedHashMap<Yaml, Yaml>) {
    // check for mandatory fields
    config.output =
        get_string_field_from_map(map, "output".to_string(), true, Some("output".to_string()));
    if let Some(value) = map.get(&Yaml::String("workers".to_string())) {
        config.workers =
            usize::try_from(value.as_i64().unwrap()).expect("Invalid data in config file: workers");
    }
    load_config(&mut config.global_config, map)
}

fn load_config(config: &mut Config, map: &LinkedHashMap<Yaml, Yaml>) {
    if let Some(value) = map.get(&Yaml::String("retrieve_server_media".to_string())) {
        config.retrieve_server_media = value
            .as_bool()
            .expect("Invalid data in config file: retrieve_server_media");
    }
    if let Some(value) = map.get(&Yaml::String("article_keep_time".to_string())) {
        config.article_keep_time = value.as_i64().unwrap();
    }
    if let Some(value) = map.get(&Yaml::String("min_refresh_time".to_string())) {
        config.min_refresh_time = value.as_i64().unwrap();
    }
    if let Some(value) = map.get(&Yaml::String("timeout".to_string())) {
        config.timeout =
            u8::try_from(value.as_i64().unwrap()).expect("Invalid data in config file: timeout");
    }
}

fn load_groups(config: &mut AppConfig, map: &LinkedHashMap<Yaml, Yaml>) {
    if let Some(groups) = map.get(&Yaml::String("groups".to_string())) {
        let provided = groups
            .as_vec()
            .expect("Invalid field in config file: groups");
        config.groups = HashMap::with_capacity(provided.len());
        for (i, g) in provided.iter().enumerate() {
            let mut obj = Group {
                title: String::with_capacity(32),
                slug: String::with_capacity(8),
                feeds: Vec::new(),
                excludes: Vec::new(),
                includes: Vec::new(),
                config: config.global_config.clone(),
            };
            let m = g
                .as_hash()
                .unwrap_or_else(|| panic!("Invalid item in groups[{}]", i));
            obj.title = get_string_field_from_map(
                m,
                "title".to_string(),
                true,
                Some(format!("groups[{}].title", i)),
            );
            obj.slug = get_string_field_from_map(
                m,
                "slug".to_string(),
                true,
                Some(format!("groups[{}].slug", i)),
            );
            load_config(&mut obj.config, m);
            // get excludes filters if applicable
            if let Some(excludes) = m.get(&Yaml::String("excludes".to_string())) {
                let values = excludes
                    .as_vec()
                    .unwrap_or_else(|| panic!("Invalid item in groups[{}].excludes", i));
                obj.excludes = Vec::with_capacity(values.len());
                for v in values {
                    let name = v
                        .as_str()
                        .unwrap_or_else(|| panic!("Invalid data in groups[{}].excludes", i));
                    let h: u64 = xxh3_64(name.as_bytes());
                    if config.filters.contains_key(&h) {
                        obj.excludes.push(h);
                    }
                }
            }
            // get includes filters if applicable
            if let Some(includes) = m.get(&Yaml::String("includes".to_string())) {
                let values = includes
                    .as_vec()
                    .unwrap_or_else(|| panic!("Invalid item in groups[{}].includes", i));
                obj.includes = Vec::with_capacity(values.len());
                for v in values {
                    let name = v
                        .as_str()
                        .unwrap_or_else(|| panic!("Invalid data in groups[{}].includes", i));
                    let h: u64 = xxh3_64(name.as_bytes());
                    if config.filters.contains_key(&h) {
                        obj.includes.push(h);
                    }
                }
            }
            config
                .groups
                .insert(xxh3_64(slugify(obj.slug.clone()).as_bytes()), obj);
        }
    }
}

fn load_filters(config: &mut AppConfig, map: &LinkedHashMap<Yaml, Yaml>) {
    if let Some(filters) = map.get(&Yaml::String("filters".to_string())) {
        let values = filters
            .as_vec()
            .expect("Invalid field in config file: filters");
        config.filters = HashMap::with_capacity(values.len());
        for (i, f) in values.iter().enumerate() {
            let m = f.as_hash().expect("Invalid data in config file: filters");
            // process filter name
            let name = get_string_field_from_map(
                m,
                "name".to_string(),
                true,
                Some(format!("filters[{}].name", i)),
            );
            let h = xxh3_64(name.as_bytes());
            // process is_case_sensitive
            let mut is_case_sensitive = false;
            if let Some(v) = m.get(&Yaml::String("is_case_sensitive".to_string())) {
                is_case_sensitive = v
                    .as_bool()
                    .unwrap_or_else(|| panic!("Invalid filters.is_case_sensitive boolean for filter {}", name));
            }
            // process filter sentences
            let value = m.get(&Yaml::String("sentences".to_string()));
            if value.is_none() {
                panic!("Field missing in config file: filters[{}].sentences in filter {}", i, name);
            }
            let value = value.unwrap().as_vec();
            if value.is_none() {
                panic!("Invalid data in config file: filters[{}].sentences in filter {}", i, name);
            }
            let value = value.unwrap();
            let sentences: Vec<String> = value
                .iter()
                .map(|exp| {
                    let sentence = exp.as_str()
                        .unwrap_or_else(|| panic!("Invalid filters.sentences string for filter {}", name))
                        .to_string();
                    if is_case_sensitive {sentence} else { sentence.to_lowercase() }
                })
                .collect();
            // handle scopes
            let value = m.get(&Yaml::String("scopes".to_string()));
            let mut scopes = 0u8;
            match value {
                Some(v) => {
                    let value = v.as_vec();
                    if value.is_none() {
                        panic!("Invalid data in config file: filters[{}].scopes in filter {}", i, name);
                    }
                    let value = value.unwrap();
                    let data: Vec<String> = value
                        .iter()
                        .map(|exp| {
                            exp.as_str()
                            .unwrap_or_else(|| panic!("Invalid filters.scopes string for filter {}", name))
                                .to_string()
                        })
                        .collect();
                    if data.contains(&String::from("title")) {
                        scopes = SCOPE_TITLE;
                    }
                    if data.contains(&String::from("summary")) {
                        scopes += SCOPE_SUMMARY;
                    }
                    if data.contains(&String::from("content")) {
                        scopes += SCOPE_BODY;
                    }
                }
                None => scopes = SCOPE_TITLE,
            }
            // process filter is_regex
            let mut must_match_all = false;
            if let Some(v) = m.get(&Yaml::String("must_match_all".to_string())) {
                must_match_all = v.as_bool().unwrap_or_else(|| panic!("Invalid filters.is_regex boolean for filter {}", name));
            }
            // process filter regexes
            let value = m.get(&Yaml::String("regexes".to_string()));
            if value.is_none() {
                panic!("Field missing in config file: filters[{}].regexes in filter {}", i, name);
            }
            let value = value.unwrap().as_vec();
            if value.is_none() {
                panic!("Invalid data in config file: filters[{}].regexes in filter {}", i, name);
            }
            let value = value.unwrap();
            let expressions: Vec<String> = value
                .iter()
                .map(|exp| {
                    exp.as_str()
                        .unwrap_or_else(|| panic!("Invalid filters.regexes string for filter {}", name))
                        .to_string()
                })
                .collect();
            let rs = RegexSetBuilder::new(expressions)
                .case_insensitive(!is_case_sensitive)
                .ignore_whitespace(true)
                .unicode(true)
                .build()
                .unwrap_or_else(|e| panic!("Cannot build one regex for filter {}: {:?}", name, e));
            config.filters.insert(
                h,
                Filter {
                    sentences,
                    regexes: rs,
                    must_match_all,
                    is_case_sensitive,
                    scopes,
                },
            );
        }
    }
    //load global filters
    // get excludes filters if applicable
    if let Some(excludes) = map.get(&Yaml::String("excludes".to_string())) {
        let values = excludes
            .as_vec()
            .unwrap_or_else(|| panic!("Invalid item in .excludes"));
        config.excludes = Vec::with_capacity(values.len());
        for v in values {
            let name = v
                .as_str()
                .unwrap_or_else(|| panic!("Invalid data in .excludes"));
            let h: u64 = xxh3_64(name.as_bytes());
            if config.filters.contains_key(&h) {
                config.excludes.push(h);
            }
        }
    }
    // get includes filters if applicable
    if let Some(includes) = map.get(&Yaml::String("includes".to_string())) {
        let values = includes
            .as_vec()
            .unwrap_or_else(|| panic!("Invalid item in includes"));
        config.includes = Vec::with_capacity(values.len());
        for v in values {
            let name = v
                .as_str()
                .unwrap_or_else(|| panic!("Invalid data in includes"));
            let h: u64 = xxh3_64(name.as_bytes());
            if config.filters.contains_key(&h) {
                config.includes.push(h);
            }
        }
    }
}

fn load_feeds(config: &mut AppConfig, map: &LinkedHashMap<Yaml, Yaml>) {
    if let Some(feeds) = map.get(&Yaml::String("feeds".to_string())) {
        let provided = feeds
            .as_vec()
            .expect("Invalid field in config file: groups");
        config.feeds = HashMap::with_capacity(provided.len());
        for (i, f) in provided.iter().enumerate() {
            let m = f
                .as_hash()
                .unwrap_or_else(|| panic!("Invalid item in groups[{}]", i));
            let title = get_string_field_from_map(
                m,
                "title".to_string(),
                true,
                Some(format!("feeds[{}].title", i)),
            );
            let url = get_string_field_from_map(
                m,
                "url".to_string(),
                true,
                Some(format!("feeds[{}].url", i)),
            );
            // parse URL (so check it) and get slug
            let parsed_url = url::Url::parse(&url)
                .unwrap_or_else(|e| panic!("Invalid data in feeds[{}].url: {}", i, e));
            let slug = parsed_url
                .host_str()
                .unwrap_or_else(|| panic!("Invalid host in feeds[{}].url", i))
                .to_string();
            let selector = get_string_field_from_map(m, "selector".to_string(), false, None);
            if !selector.is_empty() {
                scraper::Selector::parse(&selector).unwrap_or_else(|e| panic!("Invalid selector in feeds[{}].selector: {:?}", i, e));
            }
            // get the group if applicable and load
            let mut group: Option<u64> = None;
            let g = get_string_field_from_map(m, "group".to_string(), false, None);
            let mut c = Config::default();
            if !g.is_empty() {
                let gh = xxh3_64(g.as_bytes());
                if let Some(g) = config.groups.get(&gh) {
                    c = g.config.clone();
                    group = Some(gh);
                } else {
                    panic!("Invalid group slung in: feeds[{}].group", i);
                }
            } else {
                load_config(&mut c, m);
            }
            // TODO: produces: ["HTML", "PDF"]  # OPTIONAL if we want article to be in various format instead of only be in the RSS feed file
            let mut obj = Feed {
                title,
                url,
                slug,
                selector,
                page_url: String::with_capacity(128),
                group,
                excludes: Vec::with_capacity(0),
                includes: Vec::with_capacity(0),
                config: c,
                output_file: String::with_capacity(256),
            };
            // get excludes filters if applicable
            if let Some(excludes) = m.get(&Yaml::String("excludes".to_string())) {
                let values = excludes
                    .as_vec()
                    .unwrap_or_else(|| panic!("Invalid item in feeds[{}].excludes", i));
                obj.excludes = Vec::with_capacity(values.len());
                for v in values {
                    let name = v
                        .as_str()
                        .unwrap_or_else(|| panic!("Invalid data in feeds[{}].excludes", i));
                    let h: u64 = xxh3_64(name.as_bytes());
                    if config.filters.contains_key(&h) {
                        obj.excludes.push(h);
                    }
                }
            }
            // get includes filters if applicable
            if let Some(includes) = m.get(&Yaml::String("includes".to_string())) {
                let values = includes
                    .as_vec()
                    .unwrap_or_else(|| panic!("Invalid item in feeds[{}].includes", i));
                obj.includes = Vec::with_capacity(values.len());
                for v in values {
                    let name = v
                        .as_str()
                        .unwrap_or_else(|| panic!("Invalid data in feeds[{}].includes", i));
                    let h: u64 = xxh3_64(name.as_bytes());
                    if config.filters.contains_key(&h) {
                        obj.includes.push(h);
                    }
                }
            }
            config
                .feeds
                .insert(xxh3_64(slugify(obj.slug.clone()).as_bytes()), obj);
        }
    }
}

pub(crate) async fn load_config_file(config_file: String) {
    let result = std::fs::read_to_string(config_file);
    if let Err(e) = result {
        print!("Unable to open config file: {:?}", e);
        std::process::exit(1);
    }
    let result = yaml_rust::YamlLoader::load_from_str(&result.unwrap());
    if let Err(e) = result {
        print!("Unable to parse config file: {:?}", e);
        std::process::exit(1);
    }
    let loader = result.unwrap();
    let mut config = crate::CONFIG.write().await;
    if let Some(map) = loader[0].as_hash() {
        load_globals(&mut config, map);
        load_filters(&mut config, map);
        load_groups(&mut config, map);
        load_feeds(&mut config, map);
    }
}
