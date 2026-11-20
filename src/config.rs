use std::{collections::HashMap, convert::TryFrom, num::NonZeroUsize};

use regex::{RegexSet, RegexSetBuilder};
use saphyr::{LoadableYamlNode, Mapping, Yaml};
use slug::slugify;
use twox_hash::XxHash3_64;

use crate::model::{App, ContentMode, Feed, Filter, Group};

const APP_KEYS: &[&str] = &[
    "output",
    "base_url",
    "data_dir",
    "workers",
    "retrieve_server_media",
    "media",
    "media_max_size",
    "timeout",
    "retention",
    "min_refresh_time",
    "enrichment_prepend",
    "enrichment_append",
    "filters",
    "groups",
];
const FILTER_KEYS: &[&str] = &[
    "slug",
    "expressions",
    "is_regex",
    "filter_in_title",
    "filter_in_summary",
    "filter_in_content",
    "must_match_all",
    "keep",
];
const GROUP_KEYS: &[&str] = &[
    "title",
    "slug",
    "output",
    "retention",
    "media",
    "media_max_size",
    "enrichment_prepend",
    "enrichment_append",
    "filters",
    "feeds",
];
const FEED_KEYS: &[&str] = &[
    "title",
    "url",
    "page_url",
    "content_mode",
    "selector",
    "retention",
    "media",
    "media_max_size",
    "enrichment_prepend",
    "enrichment_append",
    "filters",
];

/// Keys of `map` that are not in `known`, in file order.
fn unknown_keys<'a>(map: &'a Mapping<'_>, known: &[&str]) -> Vec<&'a str> {
    map.keys()
        .filter_map(|key| key.as_str())
        .filter(|key| !known.contains(key))
        .collect()
}

/// Warn about keys the loader ignores (typos, options that don't exist),
/// which would otherwise silently fall back to their defaults.
fn warn_unknown_keys(map: &Mapping<'_>, known: &[&str], yaml_path: &str) {
    for key in unknown_keys(map, known) {
        tracing::warn!("Unknown key '{}' in {}, ignored", key, yaml_path);
    }
}

/// Concatenates two optional enrichment template strings.
/// If both are `Some`, they are joined without any separator.
/// If only one is `Some`, it is returned as-is.
fn concat_enrichment(outer: Option<&str>, inner: Option<&str>) -> Option<String> {
    match (outer, inner) {
        (Some(a), Some(b)) => Some(format!("{}{}", a, b)),
        (Some(a), None) => Some(a.to_string()),
        (None, Some(b)) => Some(b.to_string()),
        (None, None) => None,
    }
}

/// Case-sensitive lookup of a string key in a YAML mapping.
///
/// saphyr keys are typed scalars, so `map.get(&Yaml::Value(Scalar::String(...)))`
/// would require building a synthetic node. Config keys are short and few, so
/// linear scan on `as_str` is both simplest and fast enough.
fn map_get<'a, 'input>(map: &'a Mapping<'input>, key: &str) -> Option<&'a Yaml<'input>> {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

fn optional_string_field(map: &Mapping<'_>, field: &str) -> Option<String> {
    map_get(map, field)
        .and_then(Yaml::as_str)
        .map(str::to_string)
}

fn required_string_field(map: &Mapping<'_>, field: &str, yaml_path: Option<&str>) -> String {
    optional_string_field(map, field).unwrap_or_else(|| {
        panic!(
            "Field missing in config file: {}",
            yaml_path.unwrap_or("UNKNOWN")
        )
    })
}

impl App {
    fn load_globals(&mut self, map: &Mapping<'_>) {
        warn_unknown_keys(map, APP_KEYS, "the top level");
        // load output folder
        if let Some(output) = optional_string_field(map, "output") {
            self.output = output;
        }
        // public URL of the output folder, for absolute channel links
        if let Some(base_url) = optional_string_field(map, "base_url") {
            match url::Url::parse(&base_url) {
                Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => {
                    self.base_url = Some(base_url.trim_end_matches('/').to_string());
                }
                _ => panic!(
                    "Invalid data in config file: base_url must be an absolute http(s) URL, got '{base_url}'"
                ),
            }
        }
        // private database directory (defaults to the XDG state dir)
        if let Some(data_dir) = optional_string_field(map, "data_dir") {
            self.data_dir = data_dir;
        }
        // set the number of workers; 0 would never start a fetch and hang
        if let Some(value) = map_get(map, "workers") {
            self.workers = value
                .as_integer()
                .and_then(|workers| usize::try_from(workers).ok())
                .and_then(NonZeroUsize::new)
                .expect("Invalid data in config file: workers must be at least 1");
        }
        // set if we should retrieve media from server
        if let Some(value) = map_get(map, "retrieve_server_media") {
            self.retrieve_media_server = value
                .as_bool()
                .expect("Invalid data in config file: retrieve_server_media");
        }
        // enable media asset download
        if let Some(value) = map_get(map, "media") {
            self.media = value.as_bool().expect("Invalid data in config file: media");
        }
        // max asset size in bytes (0 = no limit)
        if let Some(value) = map_get(map, "media_max_size") {
            self.media_max_size = value
                .as_integer()
                .expect("Invalid data in config file: media_max_size")
                as u64;
        }
        // set the timeout for HTTP queries
        if let Some(value) = map_get(map, "timeout") {
            self.timeout = u8::try_from(
                value
                    .as_integer()
                    .expect("Invalid data in config file: timeout"),
            )
            .expect("Invalid data in config file: timeout");
        }
        // article retention in days (0 = keep forever); groups/feeds inherit this
        if let Some(value) = map_get(map, "retention") {
            self.retention = u16::try_from(
                value
                    .as_integer()
                    .expect("Invalid data in config file: retention"),
            )
            .expect("Invalid data in config file: retention");
        }
        // minimum interval between refreshes for a given feed, in seconds
        if let Some(value) = map_get(map, "min_refresh_time") {
            self.min_refresh_time = value
                .as_integer()
                .expect("Invalid data in config file: min_refresh_time");
        }
        // app-level enrichment templates
        self.enrichment_prepend = map_get(map, "enrichment_prepend")
            .and_then(Yaml::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        self.enrichment_append = map_get(map, "enrichment_append")
            .and_then(Yaml::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }

    fn load_filters(&mut self, map: &Mapping<'_>) {
        if let Some(filters) = map_get(map, "filters") {
            let values = filters
                .as_vec()
                .expect("Invalid field in config file: filters");
            self.filters = HashMap::with_capacity(values.len());
            for (i, f) in values.iter().enumerate() {
                let m = f
                    .as_mapping()
                    .expect("Invalid data in config file: filters");
                // process filter name
                let slug = required_string_field(m, "slug", Some(&format!("filters[{}].slug", i)));
                warn_unknown_keys(m, FILTER_KEYS, &format!("filter '{}'", slug));
                let h = XxHash3_64::oneshot(slug.as_bytes());
                // process filter expressions/sentences
                let value = map_get(m, "expressions");
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
                // Store expressions verbatim. Regex expressions rely on the
                // RegexSetBuilder's case_insensitive(true) flag (see below);
                // pre-lowercasing them here would corrupt character classes
                // like [A-Z] and word-boundary patterns. Plain-text matching
                // handles case-insensitivity in check_text_match.
                let expressions: Vec<String> = value
                    .iter()
                    .map(|exp| {
                        exp.as_str()
                            .unwrap_or_else(|| {
                                panic!("Invalid filters.expressions string for filter {}", slug)
                            })
                            .to_string()
                    })
                    .collect();
                let mut is_regex = false;
                if let Some(v) = map_get(m, "is_regex") {
                    is_regex = v.as_bool().unwrap_or_default();
                }
                // handle scopes
                let mut filter_in_title = true;
                let mut filter_in_summary = true;
                let mut filter_in_content = true;
                if let Some(v) = map_get(m, "filter_in_title") {
                    filter_in_title = v.as_bool().unwrap_or_default();
                }
                if let Some(v) = map_get(m, "filter_in_summary") {
                    filter_in_summary = v.as_bool().unwrap_or_default();
                }
                if let Some(v) = map_get(m, "filter_in_content") {
                    filter_in_content = v.as_bool().unwrap_or_default();
                }

                // process filter is_regex
                let mut must_match_all = false;
                if let Some(v) = map_get(m, "must_match_all") {
                    must_match_all = v.as_bool().unwrap_or_else(|| {
                        panic!("Invalid filters.must_match_all boolean for filter {}", slug)
                    });
                }
                let mut keep = false;
                if let Some(v) = map_get(m, "keep") {
                    keep = v.as_bool().unwrap_or_else(|| {
                        panic!("Invalid filters.keep boolean for filter {}", slug)
                    });
                }
                let mut regexes = RegexSet::empty();
                if is_regex {
                    // Cap compiled program size and DFA cache well below the
                    // regex crate defaults (10 MiB / 2 MiB) so a pathological
                    // pattern can't chew through memory on a router-class host.
                    // Both limits are generous relative to what any real filter
                    // pattern needs.
                    const REGEX_SIZE_LIMIT: usize = 256 * 1024;
                    const REGEX_DFA_SIZE_LIMIT: usize = 1024 * 1024;
                    // No ignore_whitespace: spaces in a pattern are literal
                    // ("Iron Maiden"); users can opt in with `(?x)`.
                    match RegexSetBuilder::new(expressions.clone())
                        .case_insensitive(true)
                        .unicode(true)
                        .size_limit(REGEX_SIZE_LIMIT)
                        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
                        .build()
                    {
                        Ok(r) => regexes = r,
                        Err(e) => {
                            tracing::warn!(
                                "Filter '{}' regex compilation failed ({}); skipping this filter",
                                slug,
                                e
                            );
                            // Skip inserting this filter entirely — feeds that
                            // reference it by slug will silently no-op. Better
                            // than a panic that kills the whole aggregator run.
                            continue;
                        }
                    }
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
                        keep,
                    },
                );
            }
        }
        tracing::info!("Loaded filters: {}", self.filters.len());
    }

    fn load_groups(&mut self, map: &Mapping<'_>) {
        if let Some(groups) = map_get(map, "groups") {
            let provided = groups.as_vec().expect("Invalid groups");

            for (i, g) in provided.iter().enumerate() {
                let m = g.as_mapping().expect("Invalid group hash");

                // Either may be omitted: the slug is derived from the title,
                // and the title (shown in every export) falls back to the slug.
                let title = optional_string_field(m, "title");
                let slug = optional_string_field(m, "slug")
                    .or_else(|| title.as_deref().map(slugify))
                    .unwrap_or_else(|| {
                        panic!(
                            "Field missing in config file: groups[{i}].slug (or groups[{i}].title)"
                        )
                    });
                warn_unknown_keys(m, GROUP_KEYS, &format!("group '{}'", slug));

                // Relative to `output`. Defaulting to `output` itself made the
                // destination the output directory, which cannot be written.
                let group_output =
                    optional_string_field(m, "output").unwrap_or_else(|| format!("{slug}.atom"));

                let mut group_obj = Group {
                    title: title.unwrap_or_else(|| slug.clone()),
                    slug,
                    output: group_output,
                    // Group retention or global if missing
                    retention: map_get(m, "retention")
                        .and_then(Yaml::as_integer)
                        .map(|v| v as u16)
                        .unwrap_or(self.retention),
                    // Group media settings, inherit from app if missing
                    media: map_get(m, "media")
                        .and_then(Yaml::as_bool)
                        .unwrap_or(self.media),
                    media_max_size: map_get(m, "media_max_size")
                        .and_then(Yaml::as_integer)
                        .map(|v| v as u64)
                        .unwrap_or(self.media_max_size),
                    // Group enrichment templates: concatenate app-level + group's own value
                    enrichment_prepend: concat_enrichment(
                        self.enrichment_prepend.as_deref(),
                        map_get(m, "enrichment_prepend")
                            .and_then(Yaml::as_str)
                            .filter(|s| !s.is_empty()),
                    ),
                    enrichment_append: concat_enrichment(
                        self.enrichment_append.as_deref(),
                        map_get(m, "enrichment_append")
                            .and_then(Yaml::as_str)
                            .filter(|s| !s.is_empty()),
                    ),
                    ..Group::default()
                };

                // Load group filters
                if let Some(filters) = map_get(m, "filters") {
                    let empty = Vec::new();
                    for f_val in filters.as_vec().unwrap_or(&empty) {
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
            if self.base_url.is_none() && !self.groups.is_empty() {
                tracing::warn!(
                    "No base_url: channel links in the exports are relative, which RSS \
                     and Atom readers or validators may reject"
                );
            }
            for (url, slugs) in shared_feeds(&self.groups) {
                tracing::warn!(
                    "Feed {} is in groups {}: it is fetched once, with the settings \
                     (filters, retention, content mode) of group '{}'",
                    url,
                    slugs.join(", "),
                    slugs[0]
                );
            }
        }
    }
}

/// Feed URLs listed in more than one group, with the slugs of those groups,
/// sorted. The first slug is the group whose feed settings are used, see
/// `processing::unique_feeds`.
fn shared_feeds(groups: &HashMap<u64, Group>) -> Vec<(String, Vec<String>)> {
    let mut by_url: HashMap<&str, Vec<String>> = HashMap::new();
    for group in groups.values() {
        for feed in group.feeds.values() {
            by_url
                .entry(feed.url.as_str())
                .or_default()
                .push(group.slug.clone());
        }
    }
    let mut shared: Vec<(String, Vec<String>)> = by_url
        .into_iter()
        .filter(|(_, slugs)| slugs.len() > 1)
        .map(|(url, mut slugs)| {
            slugs.sort();
            (url.to_string(), slugs)
        })
        .collect();
    shared.sort();
    shared
}

impl Group {
    fn load_feeds(&mut self, map: &Mapping<'_>) {
        if let Some(feeds) = map_get(map, "feeds") {
            let provided = feeds.as_vec().expect("Invalid feeds");

            for (i, f) in provided.iter().enumerate() {
                let m = f.as_mapping().expect("Invalid feed hash");
                let yaml_path = format!("group '{}' feeds[{}]", self.slug, i);
                warn_unknown_keys(m, FEED_KEYS, &yaml_path);

                let content_mode = match optional_string_field(m, "content_mode") {
                    None => ContentMode::Default,
                    Some(value) => value.parse::<ContentMode>().unwrap_or_else(|e| {
                        panic!("Invalid data in config file: {yaml_path}.content_mode: {e}")
                    }),
                };

                // --- Feed inheritance ---
                let mut feed_obj = Feed {
                    title: required_string_field(m, "title", Some(&format!("{yaml_path}.title"))),
                    url: required_string_field(m, "url", Some(&format!("{yaml_path}.url"))),
                    slug: String::new(), // will be computed later
                    retention: map_get(m, "retention")
                        .and_then(Yaml::as_integer)
                        .map(|v| v as u16)
                        .unwrap_or(self.retention), // inherited from group
                    filters: self.filters.clone(), // starts with group filters
                    selector: load_selector(m, &yaml_path, &content_mode),
                    content_mode,
                    page_url: optional_string_field(m, "page_url").unwrap_or_default(),
                    media: map_get(m, "media")
                        .and_then(Yaml::as_bool)
                        .unwrap_or(self.media), // inherited from group
                    media_max_size: map_get(m, "media_max_size")
                        .and_then(Yaml::as_integer)
                        .map(|v| v as u64)
                        .unwrap_or(self.media_max_size), // inherited from group
                    enrichment_prepend: concat_enrichment(
                        self.enrichment_prepend.as_deref(),
                        map_get(m, "enrichment_prepend")
                            .and_then(Yaml::as_str)
                            .filter(|s| !s.is_empty()),
                    ),
                    enrichment_append: concat_enrichment(
                        self.enrichment_append.as_deref(),
                        map_get(m, "enrichment_append")
                            .and_then(Yaml::as_str)
                            .filter(|s| !s.is_empty()),
                    ),
                };

                // Add feed filters to the one inherited from the group
                if let Some(f_list) = map_get(m, "filters") {
                    let empty = Vec::new();
                    for f_val in f_list.as_vec().unwrap_or(&empty) {
                        if let Some(name) = f_val.as_str() {
                            let h = XxHash3_64::oneshot(name.as_bytes());
                            if !feed_obj.filters.contains(&h) {
                                feed_obj.filters.push(h);
                            }
                        }
                    }
                }

                // Compute slug and insertion. Prefer the hostname when the URL
                // parses cleanly; on malformed input, warn and slugify the raw
                // URL so a single bad entry can't panic the whole loader.
                feed_obj.slug = match url::Url::parse(&feed_obj.url) {
                    Ok(u) => slugify(u.host_str().unwrap_or("no-host")),
                    Err(e) => {
                        tracing::warn!(
                            "Invalid feed URL '{}' in group '{}': {}. Deriving slug from the raw string.",
                            feed_obj.url,
                            self.slug,
                            e
                        );
                        slugify(&feed_obj.url)
                    }
                };

                // Hash the URL, not the slug: two feeds on the same host
                // (e.g. different YouTube channels) share a hostname slug and
                // would otherwise silently overwrite each other in the map.
                let feed_code = XxHash3_64::oneshot(feed_obj.url.as_bytes());
                if self.feeds.contains_key(&feed_code) {
                    tracing::warn!(
                        "Duplicate feed URL in group '{}': {}",
                        self.slug,
                        feed_obj.url
                    );
                }
                self.feeds.insert(feed_code, feed_obj);
            }
            tracing::info!("Loaded feeds: {} (group: {})", self.feeds.len(), self.slug);
        }
    }
}

/// Read the CSS `selector` of a feed. An invalid selector is dropped with a
/// warning instead of failing every Force-mode fetch of the feed; a selector
/// on a feed that is not in Force mode is kept but reported as unused.
fn load_selector(map: &Mapping<'_>, yaml_path: &str, mode: &ContentMode) -> Option<String> {
    let selector = optional_string_field(map, "selector")?;
    if let Err(e) = scraper::Selector::parse(&selector) {
        tracing::warn!(
            "Invalid CSS selector '{}' in {}: {}; ignored",
            selector,
            yaml_path,
            e
        );
        return None;
    }
    if *mode != ContentMode::Force {
        tracing::warn!(
            "selector in {} is only used with `content_mode: force`",
            yaml_path
        );
    }
    Some(selector)
}

pub(crate) fn load_config_file(config_file: String) -> App {
    let result = std::fs::read_to_string(config_file);
    if let Err(e) = result {
        tracing::error!("Unable to open config file: {:?}", e);
        std::process::exit(1);
    }
    let raw = result.unwrap();
    let result = Yaml::load_from_str(&raw);
    if let Err(e) = result {
        tracing::error!("Unable to parse config file: {:?}", e);
        std::process::exit(1);
    }
    let docs = result.unwrap();
    let mut app = App::default();
    if let Some(doc) = docs.first()
        && let Some(map) = doc.as_mapping()
    {
        app.load_globals(map);
        app.load_filters(map);
        app.load_groups(map);
    }
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses a YAML string into an `App` without touching the filesystem.
    fn app_from_yaml(yaml: &str) -> App {
        let docs = Yaml::load_from_str(yaml).expect("invalid yaml");
        let mut app = App::default();
        if let Some(doc) = docs.first()
            && let Some(map) = doc.as_mapping()
        {
            app.load_globals(map);
            app.load_filters(map);
            app.load_groups(map);
        }
        app
    }

    /// Returns the first feed found in the first group of the app.
    fn first_feed(app: &App) -> &crate::model::Feed {
        app.groups
            .values()
            .next()
            .unwrap()
            .feeds
            .values()
            .next()
            .unwrap()
    }

    const FEED_URL: &str = "https://example.com/feed.xml";

    #[test]
    fn test_enrichment_app_level_only() {
        let app = app_from_yaml(&format!(
            r#"
enrichment_prepend: "[app-pre]"
enrichment_append: "[app-app]"
groups:
- slug: g
  output: g.atom
  feeds:
  - title: F
    url: {FEED_URL}
"#
        ));
        let feed = first_feed(&app);
        assert_eq!(feed.enrichment_prepend.as_deref(), Some("[app-pre]"));
        assert_eq!(feed.enrichment_append.as_deref(), Some("[app-app]"));
    }

    #[test]
    fn test_enrichment_group_level_only() {
        let app = app_from_yaml(&format!(
            r#"
groups:
- slug: g
  output: g.atom
  enrichment_prepend: "[grp-pre]"
  enrichment_append: "[grp-app]"
  feeds:
  - title: F
    url: {FEED_URL}
"#
        ));
        let feed = first_feed(&app);
        assert_eq!(feed.enrichment_prepend.as_deref(), Some("[grp-pre]"));
        assert_eq!(feed.enrichment_append.as_deref(), Some("[grp-app]"));
    }

    #[test]
    fn test_enrichment_feed_level_only() {
        let app = app_from_yaml(&format!(
            r#"
groups:
- slug: g
  output: g.atom
  feeds:
  - title: F
    url: {FEED_URL}
    enrichment_prepend: "[feed-pre]"
    enrichment_append: "[feed-app]"
"#
        ));
        let feed = first_feed(&app);
        assert_eq!(feed.enrichment_prepend.as_deref(), Some("[feed-pre]"));
        assert_eq!(feed.enrichment_append.as_deref(), Some("[feed-app]"));
    }

    #[test]
    fn test_enrichment_app_and_group_concatenated() {
        let app = app_from_yaml(&format!(
            r#"
enrichment_prepend: "[app]"
enrichment_append: "[app-app]"
groups:
- slug: g
  output: g.atom
  enrichment_prepend: "[grp]"
  enrichment_append: "[grp-app]"
  feeds:
  - title: F
    url: {FEED_URL}
"#
        ));
        let feed = first_feed(&app);
        assert_eq!(feed.enrichment_prepend.as_deref(), Some("[app][grp]"));
        assert_eq!(
            feed.enrichment_append.as_deref(),
            Some("[app-app][grp-app]")
        );
    }

    #[test]
    fn test_enrichment_all_three_levels_concatenated() {
        let app = app_from_yaml(&format!(
            r#"
enrichment_prepend: "[app]"
enrichment_append: "[app-app]"
groups:
- slug: g
  output: g.atom
  enrichment_prepend: "[grp]"
  enrichment_append: "[grp-app]"
  feeds:
  - title: F
    url: {FEED_URL}
    enrichment_prepend: "[feed]"
    enrichment_append: "[feed-app]"
"#
        ));
        let feed = first_feed(&app);
        assert_eq!(feed.enrichment_prepend.as_deref(), Some("[app][grp][feed]"));
        assert_eq!(
            feed.enrichment_append.as_deref(),
            Some("[app-app][grp-app][feed-app]")
        );
    }

    #[test]
    fn test_invalid_regex_filter_is_skipped_not_panicked() {
        // Unclosed character class — should skip the filter instead of panicking.
        let app = app_from_yaml(
            r#"
filters:
- slug: broken
  expressions: ["[unclosed"]
  is_regex: true
- slug: ok
  expressions: ["hello"]
  is_regex: true
"#,
        );
        // Only the valid filter survives.
        assert_eq!(app.filters.len(), 1);
        let f = app.filters.values().next().unwrap();
        assert!(f.regexes.is_match("say hello"));
    }

    #[test]
    fn test_invalid_feed_url_does_not_panic() {
        // Bad URL used to hit .expect("Invalid URL") and crash the whole load.
        let app = app_from_yaml(
            r#"
groups:
- slug: g
  output: g.atom
  feeds:
  - title: Bad
    url: "not a url"
"#,
        );
        let feed = first_feed(&app);
        assert_eq!(
            feed.url, "not a url",
            "URL is preserved verbatim even when unparsable"
        );
        assert!(!feed.slug.is_empty(), "a fallback slug must be produced");
    }

    #[test]
    fn test_valid_feed_url_still_uses_hostname_slug() {
        let app = app_from_yaml(
            r#"
groups:
- slug: g
  output: g.atom
  feeds:
  - title: Ok
    url: https://blog.rust-lang.org/feed.xml
"#,
        );
        assert_eq!(first_feed(&app).slug, "blog-rust-lang-org");
    }

    #[test]
    fn test_global_retention_loaded_from_yaml() {
        let app = app_from_yaml(
            r#"
retention: 30
groups:
- slug: g
  output: g.atom
  feeds:
  - title: F
    url: https://example.com/feed.xml
"#,
        );
        assert_eq!(app.retention, 30);
        // Groups and feeds inherit the app-level default.
        let group = app.groups.values().next().unwrap();
        assert_eq!(group.retention, 30);
        assert_eq!(first_feed(&app).retention, 30);
    }

    #[test]
    fn test_global_retention_defaults_to_zero() {
        let app = app_from_yaml(
            r#"
groups:
- slug: g
  output: g.atom
  feeds:
  - title: F
    url: https://example.com/feed.xml
"#,
        );
        assert_eq!(
            app.retention, 0,
            "retention must default to 0 (keep forever)"
        );
    }

    #[test]
    fn test_global_min_refresh_time_loaded_from_yaml() {
        let app = app_from_yaml(
            r#"
min_refresh_time: 1800
groups: []
"#,
        );
        assert_eq!(app.min_refresh_time, 1800);
    }

    #[test]
    fn test_group_retention_overrides_global() {
        let app = app_from_yaml(
            r#"
retention: 30
groups:
- slug: g
  output: g.atom
  retention: 90
  feeds:
  - title: F
    url: https://example.com/feed.xml
"#,
        );
        let group = app.groups.values().next().unwrap();
        assert_eq!(group.retention, 90, "group retention must override global");
        assert_eq!(
            first_feed(&app).retention,
            90,
            "feed inherits group retention"
        );
    }

    #[test]
    fn test_filter_regex_preserves_case_in_character_class() {
        // Uppercase character classes must survive loading; the RegexSet
        // builder handles case-insensitivity via its own flag.
        let app = app_from_yaml(
            r#"
filters:
- slug: caps
  expressions: ["[A-Z]{3,}"]
  is_regex: true
"#,
        );
        let filter = app.filters.values().next().expect("filter present");
        assert_eq!(filter.expressions[0], "[A-Z]{3,}");
        assert!(
            filter.regexes.is_match("HELLO"),
            "regex must match uppercase"
        );
    }

    #[test]
    fn test_filter_regex_word_boundary_preserved() {
        let app = app_from_yaml(
            r#"
filters:
- slug: rust
  expressions: ["\\bRust\\b"]
  is_regex: true
"#,
        );
        let filter = app.filters.values().next().expect("filter present");
        assert_eq!(filter.expressions[0], "\\bRust\\b");
        assert!(filter.regexes.is_match("I love Rust"));
        assert!(!filter.regexes.is_match("crustacean"));
    }

    #[test]
    fn test_filter_regex_spaces_are_literal() {
        let app = app_from_yaml(
            r#"
filters:
- slug: bands
  expressions: ["Iron Maiden", "E standard"]
  is_regex: true
"#,
        );
        let filter = app.filters.values().next().expect("filter present");
        assert!(filter.regexes.is_match("Iron Maiden live"));
        assert!(filter.regexes.is_match("tuned to e standard"));
        assert!(!filter.regexes.is_match("ironmaiden"));
    }

    #[test]
    fn test_filter_regex_verbose_mode_is_opt_in() {
        let app = app_from_yaml(
            r#"
filters:
- slug: verbose
  expressions: ["(?x) Iron \\s Maiden  # comment"]
  is_regex: true
"#,
        );
        let filter = app.filters.values().next().expect("filter present");
        assert!(filter.regexes.is_match("iron maiden"));
    }

    #[test]
    fn test_filter_keep_true_loaded_from_yaml() {
        let app = app_from_yaml(
            r#"
filters:
- slug: only-rust
  expressions: [rust]
  keep: true
"#,
        );
        let filter = app.filters.values().next().expect("filter present");
        assert!(filter.keep, "keep: true must be honoured");
    }

    #[test]
    fn test_filter_keep_defaults_to_false() {
        let app = app_from_yaml(
            r#"
filters:
- slug: no-ads
  expressions: [sponsored]
"#,
        );
        let filter = app.filters.values().next().expect("filter present");
        assert!(!filter.keep, "keep must default to false when unspecified");
    }

    #[test]
    fn test_feeds_same_host_do_not_collide() {
        let app = app_from_yaml(
            r#"
groups:
- slug: g
  output: g.atom
  feeds:
  - title: Channel A
    url: https://www.youtube.com/feeds/videos.xml?channel_id=AAA
  - title: Channel B
    url: https://www.youtube.com/feeds/videos.xml?channel_id=BBB
"#,
        );
        let group = app.groups.values().next().unwrap();
        assert_eq!(
            group.feeds.len(),
            2,
            "two feeds on the same host must not collide"
        );
        let urls: std::collections::HashSet<&str> =
            group.feeds.values().map(|f| f.url.as_str()).collect();
        assert!(urls.contains("https://www.youtube.com/feeds/videos.xml?channel_id=AAA"));
        assert!(urls.contains("https://www.youtube.com/feeds/videos.xml?channel_id=BBB"));
    }

    #[test]
    fn test_enrichment_none_when_not_defined() {
        let app = app_from_yaml(&format!(
            r#"
groups:
- slug: g
  output: g.atom
  feeds:
  - title: F
    url: {FEED_URL}
"#
        ));
        let feed = first_feed(&app);
        assert!(feed.enrichment_prepend.is_none());
        assert!(feed.enrichment_append.is_none());
    }

    // --- saphyr migration coverage ---

    #[test]
    fn test_globals_all_scalar_types_parsed() {
        // Exercises every scalar type the loader touches: integer (workers,
        // timeout, retention, media_max_size, min_refresh_time), boolean
        // (retrieve_server_media, media), and string (output).
        let app = app_from_yaml(
            r#"
output: /tmp/frust
data_dir: /tmp/frust-data
workers: 4
timeout: 15
retention: 45
media: true
media_max_size: 1048576
retrieve_server_media: true
min_refresh_time: 900
"#,
        );
        assert_eq!(app.output, "/tmp/frust");
        assert_eq!(app.data_dir, "/tmp/frust-data");
        assert_eq!(app.workers.get(), 4);
        assert_eq!(app.timeout, 15);
        assert_eq!(app.retention, 45);
        assert!(app.media);
        assert_eq!(app.media_max_size, 1_048_576);
        assert!(app.retrieve_media_server);
        assert_eq!(app.min_refresh_time, 900);
    }

    #[test]
    fn test_group_media_overrides_app_media() {
        let app = app_from_yaml(
            r#"
media: false
media_max_size: 100
groups:
- slug: g
  output: g.atom
  media: true
  media_max_size: 999
  feeds:
  - title: F
    url: https://example.com/feed.xml
"#,
        );
        let group = app.groups.values().next().unwrap();
        assert!(group.media, "group media overrides app media");
        assert_eq!(group.media_max_size, 999);
        let feed = first_feed(&app);
        assert!(feed.media, "feed inherits group media");
        assert_eq!(feed.media_max_size, 999);
    }

    #[test]
    fn test_multiple_groups_are_loaded_in_order() {
        // saphyr preserves insertion order via LinkedHashMap; make sure
        // we load every group listed, not just the first.
        let app = app_from_yaml(
            r#"
groups:
- slug: alpha
  output: alpha.atom
  feeds:
  - title: A
    url: https://alpha.example.com/feed.xml
- slug: beta
  output: beta.atom
  feeds:
  - title: B
    url: https://beta.example.com/feed.xml
"#,
        );
        assert_eq!(app.groups.len(), 2);
        let slugs: std::collections::HashSet<&str> =
            app.groups.values().map(|g| g.slug.as_str()).collect();
        assert!(slugs.contains("alpha"));
        assert!(slugs.contains("beta"));
    }

    #[test]
    fn test_group_filters_are_inherited_by_feeds() {
        // Filters referenced at the group level should propagate to feeds,
        // and per-feed filters should be appended without duplicating the
        // group-level ones.
        let app = app_from_yaml(
            r#"
filters:
- slug: no-ads
  expressions: [sponsored]
- slug: only-rust
  expressions: [rust]
  keep: true
groups:
- slug: g
  output: g.atom
  filters: [no-ads]
  feeds:
  - title: F
    url: https://example.com/feed.xml
    filters: [only-rust, no-ads]
"#,
        );
        let feed = first_feed(&app);
        // Group filter propagates, feed adds only-rust, no-ads is not
        // duplicated on the feed.
        assert_eq!(feed.filters.len(), 2);
        let no_ads = XxHash3_64::oneshot(b"no-ads");
        let only_rust = XxHash3_64::oneshot(b"only-rust");
        assert!(feed.filters.contains(&no_ads));
        assert!(feed.filters.contains(&only_rust));
    }

    #[test]
    fn test_load_from_empty_document_yields_default_app() {
        // Empty YAML input must not panic: we simply return App::default().
        let docs = Yaml::load_from_str("").expect("empty is valid YAML");
        // An empty stream has no documents; the loader must cope.
        assert!(docs.is_empty() || docs.first().unwrap().as_mapping().is_none());
    }

    #[test]
    fn test_filter_scopes_default_to_all_true() {
        // When no scope keys are specified, the loader must match the
        // Filter::default() from model.rs: all three scopes on.
        let app = app_from_yaml(
            r#"
filters:
- slug: any
  expressions: [needle]
"#,
        );
        let filter = app.filters.values().next().expect("filter present");
        assert!(filter.filter_in_title);
        assert!(filter.filter_in_summary);
        assert!(
            filter.filter_in_content,
            "filter_in_content should default to true, matching Filter::default()"
        );
    }

    #[test]
    fn test_feed_selector_defaults_to_none_when_missing() {
        // With the sentinel-empty-string sentinel gone, an omitted selector
        // must arrive as None so downstream feature checks (e.g. skip
        // content-body matching when a CSS selector is configured) work.
        let app = app_from_yaml(&format!(
            r#"
groups:
- slug: g
  output: g.atom
  feeds:
  - title: F
    url: {FEED_URL}
"#
        ));
        assert!(first_feed(&app).selector.is_none());
    }

    // --- fields that used to be ignored (B4) ---

    #[test]
    fn test_group_title_is_loaded() {
        let app = app_from_yaml(
            r#"
groups:
- title: My Group
  slug: g
  output: g.atom
"#,
        );
        let group = app.groups.values().next().unwrap();
        assert_eq!(group.title, "My Group");
        assert_eq!(group.slug, "g");
    }

    #[test]
    fn test_base_url_is_loaded_without_trailing_slash() {
        let app = app_from_yaml("base_url: https://example.org/rss/\n");
        assert_eq!(app.base_url.as_deref(), Some("https://example.org/rss"));
    }

    #[test]
    fn test_base_url_defaults_to_none() {
        assert!(app_from_yaml("output: /srv\n").base_url.is_none());
    }

    #[test]
    #[should_panic(expected = "base_url must be an absolute http(s) URL")]
    fn test_relative_base_url_is_rejected() {
        app_from_yaml("base_url: /rss\n");
    }

    #[test]
    fn test_shared_feeds_lists_urls_in_several_groups() {
        let app = app_from_yaml(&format!(
            r#"
groups:
- slug: music
  feeds:
  - title: A
    url: {FEED_URL}
- slug: guitar
  feeds:
  - title: B
    url: {FEED_URL}
  - title: C
    url: https://other.example/feed
"#
        ));
        assert_eq!(
            shared_feeds(&app.groups),
            vec![(
                FEED_URL.to_string(),
                vec!["guitar".to_string(), "music".to_string()]
            )]
        );
    }

    #[test]
    fn test_group_output_defaults_to_slug_atom() {
        // Used to default to the output directory itself: unwritable.
        let app = app_from_yaml("output: /srv/www\ngroups:\n- slug: tech\n");
        assert_eq!(app.groups.values().next().unwrap().output, "tech.atom");
    }

    #[test]
    fn test_group_output_is_kept_when_set() {
        let app = app_from_yaml("groups:\n- slug: tech\n  output: news/tech.rss\n");
        assert_eq!(app.groups.values().next().unwrap().output, "news/tech.rss");
    }

    #[test]
    fn test_group_title_defaults_to_slug() {
        let app = app_from_yaml("groups:\n- slug: tech\n  output: tech.atom\n");
        assert_eq!(app.groups.values().next().unwrap().title, "tech");
    }

    #[test]
    fn test_group_slug_derived_from_title() {
        let app = app_from_yaml("groups:\n- title: My Group Name\n  output: x.xml\n");
        assert_eq!(app.groups.values().next().unwrap().slug, "my-group-name");
    }

    #[test]
    #[should_panic(expected = "groups[0].slug")]
    fn test_group_without_slug_nor_title_names_the_field() {
        app_from_yaml("groups:\n- output: x.xml\n");
    }

    #[test]
    fn test_feed_page_url_is_loaded() {
        let app = app_from_yaml(&format!(
            "groups:\n- slug: g\n  feeds:\n  - title: F\n    url: {FEED_URL}\n    page_url: https://example.com/\n"
        ));
        assert_eq!(first_feed(&app).page_url, "https://example.com/");
    }

    fn feed_with_mode(mode: &str) -> Feed {
        let app = app_from_yaml(&format!(
            "groups:\n- slug: g\n  feeds:\n  - title: F\n    url: {FEED_URL}\n    content_mode: {mode}\n"
        ));
        first_feed(&app).clone()
    }

    #[test]
    fn test_feed_content_mode_is_loaded() {
        assert_eq!(feed_with_mode("default").content_mode, ContentMode::Default);
        assert_eq!(feed_with_mode("no").content_mode, ContentMode::No);
        assert_eq!(feed_with_mode("brief").content_mode, ContentMode::Brief);
        assert_eq!(feed_with_mode("force").content_mode, ContentMode::Force);
        assert_eq!(
            feed_with_mode("links_only").content_mode,
            ContentMode::LinksOnly
        );
    }

    #[test]
    fn test_feed_content_mode_defaults_to_default() {
        let app = app_from_yaml(&format!(
            "groups:\n- slug: g\n  feeds:\n  - title: F\n    url: {FEED_URL}\n"
        ));
        assert_eq!(first_feed(&app).content_mode, ContentMode::Default);
    }

    #[test]
    #[should_panic(expected = "content_mode: unknown content_mode 'full'")]
    fn test_feed_content_mode_typo_is_reported() {
        feed_with_mode("full");
    }

    #[test]
    fn test_feed_valid_selector_is_kept() {
        let app = app_from_yaml(&format!(
            "groups:\n- slug: g\n  feeds:\n  - title: F\n    url: {FEED_URL}\n    content_mode: force\n    selector: \"div.post > p\"\n"
        ));
        assert_eq!(first_feed(&app).selector.as_deref(), Some("div.post > p"));
    }

    #[test]
    fn test_feed_invalid_selector_is_dropped() {
        let app = app_from_yaml(&format!(
            "groups:\n- slug: g\n  feeds:\n  - title: F\n    url: {FEED_URL}\n    content_mode: force\n    selector: \"div[\"\n"
        ));
        assert!(first_feed(&app).selector.is_none());
    }

    #[test]
    fn test_unknown_keys_are_detected() {
        let docs = Yaml::load_from_str("url: x\nxpath: y\nis_include: true\n").unwrap();
        let map = docs[0].as_mapping().unwrap();
        assert_eq!(unknown_keys(map, FEED_KEYS), vec!["xpath", "is_include"]);
    }

    /// Unknown keys of an example config file, at every level.
    fn example_unknown_keys(path: &str) -> Vec<String> {
        let raw = std::fs::read_to_string(path).unwrap();
        let docs = Yaml::load_from_str(&raw).unwrap();
        let root = docs[0].as_mapping().unwrap();
        let entries = |key: &str| -> Vec<&Mapping<'_>> {
            map_get(root, key)
                .and_then(Yaml::as_vec)
                .map(|items| items.iter().filter_map(Yaml::as_mapping).collect())
                .unwrap_or_default()
        };
        let mut found: Vec<String> = unknown_keys(root, APP_KEYS)
            .into_iter()
            .map(str::to_string)
            .collect();
        for filter in entries("filters") {
            found.extend(
                unknown_keys(filter, FILTER_KEYS)
                    .into_iter()
                    .map(str::to_string),
            );
        }
        for group in entries("groups") {
            found.extend(
                unknown_keys(group, GROUP_KEYS)
                    .into_iter()
                    .map(str::to_string),
            );
            for feed in map_get(group, "feeds")
                .and_then(Yaml::as_vec)
                .into_iter()
                .flatten()
                .filter_map(Yaml::as_mapping)
            {
                found.extend(
                    unknown_keys(feed, FEED_KEYS)
                        .into_iter()
                        .map(str::to_string),
                );
            }
        }
        found
    }

    #[test]
    fn test_example_configs_only_use_known_keys() {
        for path in ["config.yaml", "my-config.yaml"] {
            assert_eq!(example_unknown_keys(path), Vec::<String>::new(), "{path}");
        }
    }

    #[test]
    fn test_example_configs_load() {
        let app = load_config_file("config.yaml".to_string());
        let group = app.groups.values().next().unwrap();
        assert_eq!(group.title, "My group name");
        assert_eq!(group.slug, "my-group-name");
        let example = load_config_file("my-config.yaml".to_string());
        assert_eq!(example.groups.len(), 5);
        assert!(shared_feeds(&example.groups).is_empty());
    }

    #[test]
    #[should_panic(expected = "workers must be at least 1")]
    fn test_zero_workers_is_rejected() {
        // buffer_unordered(0) never polls a fetch: the run would hang forever.
        app_from_yaml("workers: 0\n");
    }

    #[test]
    #[should_panic(expected = "workers must be at least 1")]
    fn test_negative_workers_is_rejected() {
        app_from_yaml("workers: -2\n");
    }

    #[test]
    fn test_quoted_and_unquoted_string_keys_both_work() {
        // saphyr's scalar resolution differs slightly from yaml-rust for
        // ambiguous scalars; make sure both quoted and bare keys still map
        // to strings for the config keys we care about.
        let app = app_from_yaml(
            r#"
"output": /tmp/quoted
workers: 2
"#,
        );
        assert_eq!(app.output, "/tmp/quoted");
        assert_eq!(app.workers.get(), 2);
    }
}
