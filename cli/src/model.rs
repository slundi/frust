use std::collections::HashMap;

const DEFAULT_OUTPUT: &str = "/var/www/rss";
const DEFAULT_HTTP_TIMEOUT: u8 = 10;
const DEFAULT_MIN_REFRESH_INTERVAL: u32 = 600;
const DEFAULT_KEEP_TIME: u16 = 30;
const DEFAULT_RETRIEVE_SERVER_MEDIA: bool = false;
const DEFAULT_SORTING: &str = "-date";

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Config {
    /// Number of maximum simultaneous tasks the app will be running
    pub(crate) workers: usize,
    /// Output folder like `/var/www/rss` where feeds are generated and assets are stored. Be sure to have permissions.
    pub(crate) output: String,
    // pub(crate) format: "atom"  // generated feed format (rss, atom or json)
    /// Timeout in seconds when performing HTTP queries, default 10 seconds
    pub(crate) timeout: u8,
    /// Minimal refresh time in seconds for feeds and new articles, default 600 seconds (10 minutes)
    pub(crate) min_refresh_time: u32,
    /// Keep time in days, default 30 days. After 30 days, it will remove it from the feed, and also from the output path (assets)
    pub(crate) article_keep_time: u16,
    /// Download images `<output>/[<folder>/]<feed>/assets`. Default is `false`.
    pub(crate) retrieve_server_media: bool,
    /// Default article sorting. Minus before the filed indicates a descending order. Available fields are: date, feed
    pub(crate) sort: String, //# OPTIONAL default sorting. Default is "-date".
    /// All filters, the key is a xxh3 of the slug
    pub(crate) filters: HashMap<u64, Filter>,
    /// All filters, the key is a xxh3 of the slug
    pub(crate) groups: HashMap<u64, Group>,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            workers: std::thread::available_parallelism().unwrap().get(), // https://stackoverflow.com/questions/22155130/determine-number-of-cores-using-rust
            output: String::from(DEFAULT_OUTPUT),
            timeout: DEFAULT_HTTP_TIMEOUT,
            min_refresh_time: DEFAULT_MIN_REFRESH_INTERVAL,
            article_keep_time: DEFAULT_KEEP_TIME,
            retrieve_server_media: DEFAULT_RETRIEVE_SERVER_MEDIA,
            sort: String::from(DEFAULT_SORTING),
            // will be replaced with a filled One
            filters: HashMap::with_capacity(0),
            // will be replaced with a filled One
            groups: HashMap::with_capacity(0),
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
    // pub(crate) group:
    /// Excludes filters are executed before include filters
    pub(crate) excludes: Vec<u64>,
    ///Include filters
    pub(crate) includes: Vec<u64>,
    pub(crate) config: Config,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate)struct Article {
    /// 32 bits xxHash used to identify articles
    pub(crate) hash: u32,
    /// Date and time information, use chrono: https://stackoverflow.com/questions/72884445/chrono-datetime-from-u64-unix-timestamp-in-rust
    pub(crate) date: i64,
    pub(crate) flags: u8,
    pub(crate) slug: String,
    pub(crate) title: String,
    // pub(crate) content: String, //?in this struct?
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Filter {
    /// Filter name
    name: String,
    /// Text or regex.
    /// 
    /// If `expressions=["Elon Musk", "Tesla"]`, it will search the exact `Elon Musk` then `Tesla`. It will not be `Elon`, `Musk` and `Tesla`.
    pub(crate) expressions: Vec<String>,
    /// If provided expressions are regular expresssions or not
    pub(crate) is_regex: bool,
    /// If the search is case sensitive, default false
    pub(crate) is_case_sensitive: bool,
}
