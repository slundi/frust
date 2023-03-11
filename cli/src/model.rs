use std::collections::HashMap;
use serde::{Serialize, Deserialize};

const DEFAULT_OUTPUT: &str = "/var/www/rss";
pub(crate) const DEFAULT_HISTORY_FILE: &str = "frust.dat";
const DEFAULT_HTTP_TIMEOUT: u8 = 10;
const DEFAULT_MIN_REFRESH_INTERVAL: u32 = 600;
const DEFAULT_KEEP_TIME: u16 = 30;
const DEFAULT_RETRIEVE_SERVER_MEDIA: bool = false;
const DEFAULT_SORTING: &str = "-date";

#[derive(Debug, PartialEq, Clone)]
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
    /// Minimal refresh time in seconds for feeds and new articles, default 600 seconds (10 minutes)
    pub(crate) min_refresh_time: u32,
    /// Keep time in days, default 30 days. After 30 days, it will remove it from the feed, and also from the output path (assets)
    pub(crate) article_keep_time: u16,
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
    pub(crate) name: String,
    /// Text or regex.
    /// 
    /// If `expressions=["Elon Musk", "Tesla"]`, it will search the exact `Elon Musk` then `Tesla`. It will not be `Elon`, `Musk` and `Tesla`.
    pub(crate) expressions: Vec<String>,
    /// If provided expressions are regular expresssions or not
    pub(crate) is_regex: bool,
    /// If the search is case sensitive, default false
    pub(crate) is_case_sensitive: bool,
}

/// Pseudo-database that containt feed metadata and article metadata. It is an HashMap where the key is the xxHash of the slug.
/// 
/// What should I use for storage?
/// * [nom](https://crates.io/crates/nom)?
/// * [pest](https://crates.io/crates/pest)?
/// * [bincode](https://crates.io/crates/bincode)? -> may be my choice, need to test
/// * [zerocopy](https://crates.io/crates/zerocopy)?
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub(crate) struct Storage (pub(crate) HashMap<u64, FeedRecord>);

/// Store feed information to be lightweight in memory (because everything is loaded during process) and small on the drive.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub(crate) struct FeedRecord {
    /// Encoded date and time of the last update by Frust (not the feed last modification date!)
    pub(crate) date: u64,
    /// xxHash of the last downloaded content
    pub(crate) hash: u64,
    pub(crate) slug: String,
    /// Article meta information. The map key is :
    /// * the xxHash of the RSS article link (because required for RSS) or RSS GUID link if applicable
    /// * the xxHash of the ATOM item ID field
    /// * the xxHash of the JSON item ID field
    pub(crate) articles: HashMap<u64, ArticleRecord>,
}

/// Store article information for each feed.
/// 
/// Article title and plushed date are not stored because the feed will be loaded in memory and be sorted before writing it to disk.
/// 
/// Article content is not stored here because it would duplicate it with generated feeds so we should just append content in the feed file.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub(crate) struct ArticleRecord {
    /// Article last modification date, in order to retrieve content if the article has been updated
    pub(crate) date: u64,
    /// Article flags (for now, just one):
    /// * 0x01: ignored (by filters)
    pub(crate) flags: u8,
    /// Article slug to find the output files if applicable?
    pub(crate) slug: String,
}
