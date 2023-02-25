use std::{collections::HashMap, convert::TryFrom};

use linked_hash_map::LinkedHashMap;
use regex::Regex;
use slug::slugify;
use xxhash_rust::xxh3::xxh3_64;
use yaml_rust::Yaml;

use crate::model::{Config, Feed, Filter, Group};

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

fn load_globals(config: &mut Config, map: &LinkedHashMap<Yaml, Yaml>) {
    // check for mandatory fields
    config.output =
        get_string_field_from_map(map, "output".to_string(), true, Some("output".to_string()));
    if let Some(value) = map.get(&Yaml::String("article_keep_time".to_string())) {
        config.article_keep_time = u16::try_from(value.as_i64().unwrap())
            .expect("Invalid data in config file: article_keep_time");
    }
    if let Some(value) = map.get(&Yaml::String("min_refresh_time".to_string())) {
        config.min_refresh_time = u32::try_from(value.as_i64().unwrap())
            .expect("Invalid data in config file: min_refresh_time");
    }
    if let Some(value) = map.get(&Yaml::String("timeout".to_string())) {
        config.timeout =
            u8::try_from(value.as_i64().unwrap()).expect("Invalid field in config file: timeout");
    }
    if let Some(value) = map.get(&Yaml::String("workers".to_string())) {
        config.workers =
            usize::try_from(value.as_i64().unwrap()).expect("Invalid data in config file: workers");
    }
    if let Some(value) = map.get(&Yaml::String("retrieve_server_media".to_string())) {
        config.retrieve_server_media = value
            .as_bool()
            .expect("Invalid data in config file: retrieve_server_media");
    }
    // if let Some(value)=map.get(&Yaml::String("sort".to_string())) {
    //     config.workers = usize::try_from(value.as_i64().unwrap()).expect("Invalid field in config file: workers");
    // }
}

fn load_groups(config: &mut Config, map: &LinkedHashMap<Yaml, Yaml>) -> HashMap<u64, Group> {
    if let Some(groups) = map.get(&Yaml::String("groups".to_string())) {
        let provided = groups
            .as_hash()
            .expect("Invalid field in config file: groups");
        let mut result: HashMap<u64, Group> = HashMap::with_capacity(provided.len());
        for (i, g) in provided.iter().enumerate() {
            let mut obj = Group {
                title: String::with_capacity(32),
                slug: String::with_capacity(8),
                feeds: Vec::new(),
                excludes: Vec::new(),
                includes: Vec::new(),
                config: config.clone(),
            };
            if g.0 == &Yaml::String("title".to_string()) {
                obj.title =
                    g.1.as_str()
                        .expect("Invalid data in config file: groups.title")
                        .to_string();
            }
            if g.0 == &Yaml::String("slug".to_string()) {
                obj.slug =
                    g.1.as_str()
                        .expect("Invalid data in config file: groups.slug")
                        .to_string();
            }
            result.insert(xxh3_64(slugify(obj.slug.clone()).as_bytes()), obj);
            // get filter slugs and compare if defined in filters
            todo!();
        }
        return result;
    }
    HashMap::with_capacity(0)
}

fn load_filters(config: &mut Config, map: &LinkedHashMap<Yaml, Yaml>) {
    if let Some(filters) = map.get(&Yaml::String("filters".to_string())) {
        for (i, f) in filters
            .as_vec()
            .expect("Invalid field in config file: filters")
            .iter()
            .enumerate()
        {
            let m = f.as_hash().expect("Invalid data in config file: filters");
            // process filter name
            let name = get_string_field_from_map(
                m,
                "name".to_string(),
                true,
                Some(format!("filters[{}].name", i)),
            );
            // process filter expressions
            let value = m.get(&Yaml::String("expressions".to_string()));
            if value.is_none() {
                print!("Field missing in config file: filters[{}].expressions", i);
                std::process::exit(1);
            }
            let value = value.unwrap().as_vec();
            if value.is_none() {
                print!("Invalid data in config file: filters[{}].expressions", i);
                std::process::exit(1);
            }
            let value = value.unwrap();
            let expressions: Vec<String> = value
                .iter()
                .map(|exp| {
                    exp.as_str()
                        .expect("Invalid filters.expressions string")
                        .to_string()
                })
                .collect();
            // process filter is_regex
            let mut is_regex = false;
            if let Some(v) = m.get(&Yaml::String("is_regex".to_string())) {
                is_regex = v.as_bool().expect("Invalid filters.is_regex boolean");
            }
            // process is_case_sensitive
            let mut is_case_sensitive = false;
            if let Some(v) = m.get(&Yaml::String("is_case_sensitive".to_string())) {
                is_case_sensitive = v
                    .as_bool()
                    .expect("Invalid filters.is_case_sensitive boolean");
            }
            if is_regex {
                for exp in expressions {
                    if let Err(e) = Regex::new(&exp) {
                        print!("Invalid regular expression in filter in config file: filters[{}].expressions: {}\t{}", i, exp, e);
                        std::process::exit(1);
                    }
                }
            }
            todo!();
            //config.filters = HashMap::with_capacity(capacity)
        }
    }
}

fn load_feeds(config: &mut Config, map: &LinkedHashMap<Yaml, Yaml>) -> HashMap<u64, Feed> {
    todo!()
}

pub(crate) fn load_config_file(config_file: String) {
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
    let mut config: Config = Config::default();
    if let Some(map) = loader[0].as_hash() {
        load_globals(&mut config, map);
        load_filters(&mut config, map);
        load_groups(&mut config, map);
        load_feeds(&mut config, map);
    }
}
