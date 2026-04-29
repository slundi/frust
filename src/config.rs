use std::{collections::HashMap, convert::TryFrom};

use linked_hash_map::LinkedHashMap;
use regex::{RegexSet, RegexSetBuilder};
use slug::slugify;
use twox_hash::XxHash3_64;
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
        // enable media asset download
        if let Some(value) = map.get(&Yaml::String("media".to_string())) {
            self.media = value.as_bool().expect("Invalid data in config file: media");
        }
        // max asset size in bytes (0 = no limit)
        if let Some(value) = map.get(&Yaml::String("media_max_size".to_string())) {
            self.media_max_size = value
                .as_i64()
                .expect("Invalid data in config file: media_max_size")
                as u64;
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
                let h = XxHash3_64::oneshot(slug.as_bytes());
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
                        sentence.to_lowercase()
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
                        .case_insensitive(true)
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
                        expressions,
                        regexes,
                        is_regex,
                        must_match_all,
                        filter_in_title,
                        filter_in_summary,
                        filter_in_content,
                        keep: false,
                    },
                );
            }
        }
        tracing::info!("Loaded filters: {}", self.filters.len());
    }

    fn load_groups(&mut self, map: &LinkedHashMap<Yaml, Yaml>) {
        if let Some(groups) = map.get(&Yaml::String("groups".to_string())) {
            let provided = groups.as_vec().expect("Invalid groups");

            for g in provided.iter() {
                let m = g.as_hash().expect("Invalid group hash");

                let mut group_obj = Group::default();
                group_obj.slug = get_string_field_from_map(m, "slug".to_string(), true, None);

                // --- Group inheritance ---
                // if group does not have output, it takes it from the App (global
                group_obj.output = get_string_field_from_map(m, "output".to_string(), false, None);
                if group_obj.output.is_empty() {
                    group_obj.output = self.output.clone();
                }

                // Group retention or global if missing
                group_obj.retention = m
                    .get(&Yaml::String("retention".to_string()))
                    .and_then(|v| v.as_i64())
                    .map(|v| v as u16)
                    .unwrap_or(self.retention);
                // Group media settings, inherit from app if missing
                group_obj.media = m
                    .get(&Yaml::String("media".to_string()))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(self.media);
                group_obj.media_max_size = m
                    .get(&Yaml::String("media_max_size".to_string()))
                    .and_then(|v| v.as_i64())
                    .map(|v| v as u64)
                    .unwrap_or(self.media_max_size);

                // Load group filters
                if let Some(filters) = m.get(&Yaml::String("filters".to_string())) {
                    for f_val in filters.as_vec().unwrap_or(&vec![]) {
                        if let Some(name) = f_val.as_str() {
                            group_obj.filters.push(XxHash3_64::oneshot(name.as_bytes()));
                        }
                    }
                }

                // Give group object for feeds that are inheriting it
                group_obj.load_feeds(m);

                let group_code = XxHash3_64::oneshot(slugify(&group_obj.slug).as_bytes());
                self.groups.insert(group_code, group_obj);
            }
            tracing::info!("Loaded groups: {}", self.groups.len());
        }
    }
}

impl Group {
    fn load_feeds(&mut self, map: &LinkedHashMap<Yaml, Yaml>) {
        if let Some(feeds) = map.get(&Yaml::String("feeds".to_string())) {
            let provided = feeds.as_vec().expect("Invalid feeds");

            for f in provided.iter() {
                let m = f.as_hash().expect("Invalid feed hash");

                // --- Feed inheritance ---
                let mut feed_obj = Feed {
                    title: get_string_field_from_map(m, "title".to_string(), true, None),
                    url: get_string_field_from_map(m, "url".to_string(), true, None),
                    slug: String::new(), // will be computed later
                    output: get_string_field_from_map(m, "output".to_string(), false, None),
                    retention: m
                        .get(&Yaml::String("retention".to_string()))
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u16)
                        .unwrap_or(self.retention), // inherited from group
                    filters: self.filters.clone(), // starts with group filters
                    content_mode: crate::model::ContentMode::Default,
                    selector: Some(get_string_field_from_map(
                        m,
                        "selector".to_string(),
                        false,
                        None,
                    )),
                    page_url: String::new(),
                    last_etag: None,
                    last_modified: None,
                    last_check: None,
                    media: m
                        .get(&Yaml::String("media".to_string()))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(self.media), // inherited from group
                    media_max_size: m
                        .get(&Yaml::String("media_max_size".to_string()))
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u64)
                        .unwrap_or(self.media_max_size), // inherited from group
                    enrichment_prepend: m
                        .get(&Yaml::String("enrichment_prepend".to_string()))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()),
                    enrichment_append: m
                        .get(&Yaml::String("enrichment_append".to_string()))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string()),
                };

                // If feed does not have output, use the one from the group
                // that may have taken it from global
                if feed_obj.output.is_empty() {
                    feed_obj.output = self.output.clone();
                }

                // Add feed filters to the one inherited from the group
                if let Some(f_list) = m.get(&Yaml::String("filters".to_string())) {
                    for f_val in f_list.as_vec().unwrap_or(&vec![]) {
                        if let Some(name) = f_val.as_str() {
                            let h = XxHash3_64::oneshot(name.as_bytes());
                            if !feed_obj.filters.contains(&h) {
                                feed_obj.filters.push(h);
                            }
                        }
                    }
                }

                // Compute slug and insertion
                let parsed_url = url::Url::parse(&feed_obj.url).expect("Invalid URL");
                feed_obj.slug = slugify(parsed_url.host_str().unwrap_or("no-host"));

                let feed_code = XxHash3_64::oneshot(feed_obj.slug.as_bytes());
                self.feeds.insert(feed_code, feed_obj);
            }
            tracing::info!("Loaded feeds: {} (group: {})", self.feeds.len(), self.slug);
        }
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
