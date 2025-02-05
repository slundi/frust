use std::{collections::HashMap, convert::TryFrom};

use linked_hash_map::LinkedHashMap;
use regex::{RegexSet, RegexSetBuilder};
use slug::slugify;
use xxhash_rust::xxh3::xxh3_64;
use yaml_rust::Yaml;

use crate::model::{App, Feed, Filter, Group};

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
        panic!(
            "Field missing in config file: {}",
            yaml_path.unwrap_or_else(|| "UNKNOWN".to_string())
        );
    }
    String::with_capacity(0)
}

impl App {
    fn load_globals(&mut self, map: &LinkedHashMap<Yaml, Yaml>) {
        // load output folder
        let output =
            get_string_field_from_map(map, "output".to_string(), false, Some("output".to_string()));
        if !output.is_empty() {
            self.output = output;
        }
        // set the number of workers
        if let Some(value) = map.get(&Yaml::String("workers".to_string())) {
            self.workers = usize::try_from(value.as_i64().unwrap())
                .expect("Invalid data in config file: workers");
        }
        // set if we should retrieve media from server
        if let Some(value) = map.get(&Yaml::String("retrieve_server_media".to_string())) {
            self.retrieve_media_server = value
                .as_bool()
                .expect("Invalid data in config file: retrieve_server_media");
        }
        // set the timeout for HTTP queries
        if let Some(value) = map.get(&Yaml::String("timeout".to_string())) {
            self.timeout = u8::try_from(value.as_i64().unwrap())
                .expect("Invalid data in config file: timeout");
        }
    }

    fn load_filters(&mut self, map: &LinkedHashMap<Yaml, Yaml>) {
        if let Some(filters) = map.get(&Yaml::String("filters".to_string())) {
            let values = filters
                .as_vec()
                .expect("Invalid field in config file: filters");
            self.filters = HashMap::with_capacity(values.len());
            for (i, f) in values.iter().enumerate() {
                let m = f.as_hash().expect("Invalid data in config file: filters");
                // process filter name
                let slug = get_string_field_from_map(
                    m,
                    "slug".to_string(),
                    true,
                    Some(format!("filters[{}].slug", i)),
                );
                let h = xxh3_64(slug.as_bytes());
                // process is_case_sensitive
                let mut is_case_sensitive = false;
                if let Some(v) = m.get(&Yaml::String("is_case_sensitive".to_string())) {
                    is_case_sensitive = v.as_bool().unwrap_or_else(|| {
                        panic!(
                            "Invalid filters.is_case_sensitive boolean for filter {}",
                            slug
                        )
                    });
                }
                // process filter expressions/sentences
                let value = m.get(&Yaml::String("expressions".to_string()));
                if value.is_none() {
                    panic!(
                        "Field missing in config file: filters[{}].expressions in filter {}",
                        i, slug
                    );
                }
                let value = value.unwrap().as_vec();
                if value.is_none() {
                    panic!(
                        "Invalid data in config file: filters[{}].expressions in filter {}",
                        i, slug
                    );
                }
                let value = value.unwrap();
                let expressions: Vec<String> = value
                    .iter()
                    .map(|exp| {
                        let sentence = exp
                            .as_str()
                            .unwrap_or_else(|| {
                                panic!("Invalid filters.expressions string for filter {}", slug)
                            })
                            .to_string();
                        if is_case_sensitive {
                            sentence
                        } else {
                            sentence.to_lowercase()
                        }
                    })
                    .collect();
                let value = m.get(&Yaml::String("is_regex".to_string()));
                let mut is_regex = false;
                if let Some(v) = value {
                    is_regex = v.as_bool().unwrap_or_default();
                }
                // handle scopes
                let mut filter_in_title = true;
                let mut filter_in_summary = true;
                let mut filter_in_content = false;
                let value = m.get(&Yaml::String("filter_in_title".to_string()));
                if let Some(v) = value {
                    filter_in_title = v.as_bool().unwrap_or_default();
                }
                let value = m.get(&Yaml::String("filter_in_summary".to_string()));
                if let Some(v) = value {
                    filter_in_summary = v.as_bool().unwrap_or_default();
                }
                let value = m.get(&Yaml::String("filter_in_content".to_string()));
                if let Some(v) = value {
                    filter_in_content = v.as_bool().unwrap_or_default();
                }

                // process filter is_regex
                let mut must_match_all = false;
                if let Some(v) = m.get(&Yaml::String("must_match_all".to_string())) {
                    must_match_all = v.as_bool().unwrap_or_else(|| {
                        panic!("Invalid filters.is_regex boolean for filter {}", slug)
                    });
                }
                // process filter regexes: will be generated from expressions and is_regex flag
                // let value = m.get(&Yaml::String("regexes".to_string()));
                // if value.is_none() {
                //     panic!(
                //         "Field missing in config file: filters[{}].regexes in filter {}",
                //         i, slug
                //     );
                // }
                // let value = value.unwrap().as_vec();
                // if value.is_none() {
                //     panic!(
                //         "Invalid data in config file: filters[{}].regexes in filter {}",
                //         i, slug
                //     );
                // }
                // let value = value.unwrap();
                // let expressions: Vec<String> = value
                //     .iter()
                //     .map(|exp| {
                //         exp.as_str()
                //             .unwrap_or_else(|| {
                //                 panic!("Invalid filters.regexes string for filter {}", slug)
                //             })
                //             .to_string()
                //     })
                //     .collect();
                let mut regexes = RegexSet::empty();
                if is_regex {
                    regexes = RegexSetBuilder::new(expressions.clone())
                        .case_insensitive(!is_case_sensitive)
                        .ignore_whitespace(true)
                        .unicode(true)
                        .build()
                        .unwrap_or_else(|e| {
                            panic!("Cannot build one regex for filter {}: {:?}", slug, e)
                        });
                }
                self.filters.insert(
                    h,
                    Filter {
                        slug,
                        expressions,
                        regexes,
                        is_regex,
                        is_case_sensitive,
                        must_match_all,
                        filter_in_title,
                        filter_in_summary,
                        filter_in_content,
                    },
                );
            }
        }
        tracing::info!("Loaded filters: {}", self.filters.len());
    }

    fn load_groups(&mut self, map: &LinkedHashMap<Yaml, Yaml>) {
        if let Some(groups) = map.get(&Yaml::String("groups".to_string())) {
            let provided = groups
                .as_vec()
                .expect("Invalid field in config file: groups");
            self.groups = HashMap::with_capacity(provided.len());
            for (i, g) in provided.iter().enumerate() {
                let mut obj: Group = Group::default();
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
                obj.output = get_string_field_from_map(
                    m,
                    "output".to_string(),
                    false,
                    Some(format!("groups[{}].output", i)),
                );
                // get filters if applicable
                if let Some(filters) = m.get(&Yaml::String("filters".to_string())) {
                    let values = filters
                        .as_vec()
                        .unwrap_or_else(|| panic!("Invalid item in groups[{}].filters", i));
                    obj.filters = Vec::with_capacity(values.len());
                    for v in values {
                        let name = v
                            .as_str()
                            .unwrap_or_else(|| panic!("Invalid data in groups[{}].filters", i));
                        let h: u64 = xxh3_64(name.as_bytes());
                        if self.filters.contains_key(&h) {
                            obj.filters.push(h);
                        }
                    }
                }
                let code = xxh3_64(slugify(obj.slug.clone()).as_bytes());
                self.groups.insert(code, obj);
                self.load_feeds(m, code);
            }
        }
        tracing::info!("Loaded groups: {}", self.groups.len());
    }

    fn load_feeds(&mut self, map: &LinkedHashMap<Yaml, Yaml>, group_code: u64) {
        if let Some(feeds) = map.get(&Yaml::String("feeds".to_string())) {
            let provided = feeds
                .as_vec()
                .expect("Invalid field in config file: groups");
            // self.feeds = HashMap::with_capacity(provided.len());
            // tracing::debug!("{} feeds to load", provided.len());
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
                let output = get_string_field_from_map(
                    m,
                    "output".to_string(),
                    false,
                    Some(format!("feeds[{}].output", i)),
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
                    scraper::Selector::parse(&selector).unwrap_or_else(|e| {
                        panic!("Invalid selector in feeds[{}].selector: {:?}", i, e)
                    });
                }
                // get the group if applicable and load
                // let mut group: Option<u64> = None;
                // let g = get_string_field_from_map(m, "group".to_string(), false, None);
                // let mut c = AppConfig::default();
                // if !g.is_empty() {
                //     let gh = xxh3_64(g.as_bytes());
                //     if let Some(g) = config.groups.get(&gh) {
                //         c = g.config.clone();
                //         group = Some(gh);
                //     } else {
                //         panic!("Invalid group slung in: feeds[{}].group", i);
                //     }
                // }
                // TODO: produces: ["HTML", "PDF"]  # OPTIONAL if we want article to be in various format instead of only be in the RSS feed file
                let mut obj = Feed {
                    title,
                    group_code,
                    url,
                    slug,
                    selector,
                    page_url: String::with_capacity(128),
                    // group,
                    filters: Vec::with_capacity(0),
                    output,
                };
                // get filters if applicable
                if let Some(filters) = m.get(&Yaml::String("filters".to_string())) {
                    let values = filters
                        .as_vec()
                        .unwrap_or_else(|| panic!("Invalid item in feeds[{}].filters", i));
                    obj.filters = Vec::with_capacity(values.len());
                    for v in values {
                        let name = v
                            .as_str()
                            .unwrap_or_else(|| panic!("Invalid data in feeds[{}].filters", i));
                        let h: u64 = xxh3_64(name.as_bytes());
                        if self.filters.contains_key(&h) {
                            obj.filters.push(h);
                        }
                    }
                }
                let code = xxh3_64(slugify(obj.slug.clone()).as_bytes());
                if self.has_output(group_code, code) {
                    self.feeds.insert(code, obj);
                } else {
                    tracing::warn!(
                        "Feed {} is never saved (no output in group or in the feed)",
                        obj.title
                    );
                }
            }
        }
        tracing::info!(
            "Loaded feeds: {} (group: {})",
            self.feeds.len(),
            self.groups.get(&group_code).unwrap().slug
        );
    }
}

pub(crate) fn load_config_file(config_file: String) -> App {
    let result = std::fs::read_to_string(config_file);
    if let Err(e) = result {
        tracing::error!("Unable to open config file: {:?}", e);
        std::process::exit(1);
    }
    let result = yaml_rust::YamlLoader::load_from_str(&result.unwrap());
    if let Err(e) = result {
        tracing::error!("Unable to parse config file: {:?}", e);
        std::process::exit(1);
    }
    let loader = result.unwrap();
    let mut app = App::default();
    if let Some(map) = loader[0].as_hash() {
        app.load_globals(map);
        app.load_filters(map);
        app.load_groups(map);
    }
    app.clone()
}
