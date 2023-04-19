use regex::RegexSet;
use std::collections::HashMap;

const DEFAULT_OUTPUT: &str = "/var/www/rss";
const DEFAULT_HTTP_TIMEOUT: u8 = 10;
const DEFAULT_MIN_REFRESH_INTERVAL: i64 = 600;
const DEFAULT_KEEP_TIME: i64 = 30;
const DEFAULT_RETRIEVE_SERVER_MEDIA: bool = false;
const DEFAULT_SORTING: &str = "-date";

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Number of maximum simultaneous tasks the app will be running
    pub(crate) workers: usize,
    /// Output folder like `/var/www/rss` where feeds are generated and assets are stored. Be sure to have permissions.
    pub(crate) output: String,
    /// All filters, the key is a xxh3 of the slug
    pub(crate) filters: HashMap<u64, Filter>,
    /// All groups, the key is a xxh3 of the slug
    pub(crate) groups: HashMap<u64, Group>,
    /// All feeds, the key is a xxh3 of the slug
    pub(crate) feeds: HashMap<u64, Feed>,
    /// Excludes filters are executed before include filters
    pub(crate) excludes: Vec<u64>,
    ///Include filters
    pub(crate) includes: Vec<u64>,
    // pub(crate) format: "atom"  // generated feed format (rss, atom or json)
    pub(crate) global_config: Config,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            workers: std::thread::available_parallelism().unwrap().get(), // https://stackoverflow.com/questions/22155130/determine-number-of-cores-using-rust
            output: String::from(DEFAULT_OUTPUT),
            // will be replaced with a filled One
            filters: HashMap::with_capacity(0),
            // will be replaced with a filled One
            groups: HashMap::with_capacity(0),
            // will be replaced with a filled One
            feeds: HashMap::with_capacity(0),
            // will be replaced with a filled One
            includes: Vec::with_capacity(0),
            // will be replaced with a filled One
            excludes: Vec::with_capacity(0),
            global_config: Config::default(),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Config {
    /// Timeout in seconds when performing HTTP queries, default 10 seconds
    pub(crate) timeout: u8,
    /// Minimal refresh time in seconds for feeds and new articles, default 600 seconds (10 minutes).
    pub(crate) min_refresh_time: i64,
    /// Keep time in days, default 30 days. After 30 days, it will remove it from the feed, and also from the output path (assets).  `i64` to be easier to use with chrono.
    pub(crate) article_keep_time: i64,
    /// Download images `<output>/[<folder>/]<feed>/assets`. Default is `false`.
    pub(crate) retrieve_server_media: bool,
    /// Default article sorting. Minus before the filed indicates a descending order. Available fields are: date, feed
    pub(crate) sort: String, //# OPTIONAL default sorting. Default is "-date".
}
impl Default for Config {
    fn default() -> Self {
        Config {
            timeout: DEFAULT_HTTP_TIMEOUT,
            min_refresh_time: DEFAULT_MIN_REFRESH_INTERVAL,
            article_keep_time: DEFAULT_KEEP_TIME,
            retrieve_server_media: DEFAULT_RETRIEVE_SERVER_MEDIA,
            sort: String::from(DEFAULT_SORTING),
        }
    }
}

/// A group allows you to produce a unique feed by aggregating the enumerated ones.
#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Group {
    pub(crate) title: String,
    pub(crate) slug: String,
    // pub(crate) sort: "-date"
    /// List of feed slugs
    pub(crate) feeds: Vec<String>,
    /// Excludes filters are executed before include filters
    pub(crate) excludes: Vec<u64>,
    ///Include filters
    pub(crate) includes: Vec<u64>,
    pub(crate) config: Config,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Feed {
    pub(crate) title: String,
    /// Unique and URL usable string to identify the feed
    pub(crate) slug: String,
    pub(crate) url: String,
    pub(crate) page_url: String,
    pub(crate) xpath: String, // Option<String>?
    // pub(crate) produces: ["HTML", "PDF"]
    /// Identify group by its hash
    pub(crate) group: Option<u64>,
    /// Excludes filters are executed before include filters
    pub(crate) excludes: Vec<u64>,
    ///Include filters
    pub(crate) includes: Vec<u64>,
    pub(crate) config: Config,
    /// Output file without extension
    pub(crate) output_file: String,
}

pub(crate) const SCOPE_TITLE: u8 = 1;
pub(crate) const SCOPE_SUMMARY: u8 = 2;
pub(crate) const SCOPE_BODY: u8 = 4;

/// Filter structure. The name is not kept because it is only used during filter loading in order to help the user to find errors quickly.
#[derive(Debug, Clone)]
pub(crate) struct Filter {
    /// Text or regex.
    ///
    /// If `expressions=["Elon Musk", "Tesla"]`, it will search the exact `Elon Musk` then `Tesla`. It will not be `Elon`, `Musk` and `Tesla`.
    pub(crate) sentences: Vec<String>,
    /// list of regex to match
    pub(crate) regexes: RegexSet,
    /// If the search is case sensitive, default false
    pub(crate) is_case_sensitive: bool,
    /// if all sentences and regexes must match, default `false
    pub(crate) must_match_all: bool,
    /// Scope of the search: combine with `SCOPE_TITLE`, `SCOPE_SUMMARY` and `SCOPE_BODY` constants
    pub(crate) scopes: u8,
}
