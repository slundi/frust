use std::collections::HashMap;

use regex::RegexSet;

use crate::{DEFAULT_HTTP_TIMEOUT, DEFAULT_RETRIEVE_SERVER_MEDIA};

#[derive(Debug, Clone)]
pub(crate) struct App {
    pub(crate) output: String,
    pub(crate) timeout: u8,
    pub(crate) retrieve_media_server: bool,
    /// min refresh time in second
    pub(crate) min_refresh_time: i64,
    // https://stackoverflow.com/questions/22155130/
    pub(crate) workers: usize,
    pub(crate) now: chrono::DateTime<chrono::Utc>,
    /// List of filters, the u64 key is a XXH3 of the slug
    pub(crate) filters: HashMap<u64, Filter>,
    /// All groups, the key is a xxh3 of the slug
    pub(crate) groups: HashMap<u64, Group>,
    /// All feeds, the key is a xxh3 of the slug
    pub(crate) feeds: HashMap<u64, Feed>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            output: std::env::current_dir().unwrap().display().to_string(),
            retrieve_media_server: DEFAULT_RETRIEVE_SERVER_MEDIA,
            timeout: DEFAULT_HTTP_TIMEOUT,
            min_refresh_time: 600,
            workers: std::thread::available_parallelism().unwrap().get(),
            now: chrono::offset::Utc::now(),
            filters: HashMap::with_capacity(0),
            groups: HashMap::with_capacity(0),
            feeds: HashMap::new(),
        }
    }
}

impl App {
    pub fn has_output(&self, group_code: u64, feed_code: u64) -> bool {
        self.groups.get(&group_code).unwrap().output.is_some()
            || !self.feeds.get(&feed_code).unwrap().output_file.is_empty()
    }
}

/// A group allows you to produce a unique feed by aggregating the enumerated ones.
#[derive(Debug, Default, PartialEq, Clone)]
pub(crate) struct Group {
    pub(crate) title: String,
    pub(crate) slug: String,
    /// List of feed slugs
    pub(crate) feeds: Vec<String>,
    /// Applied filter, from the first in the list to the last
    pub(crate) filters: Vec<u64>,
    /// Set it if you want to aggregate the feeds in the group
    pub(crate) output: Option<String>,
    /// Article retention in days
    pub(crate) retention: Option<u16>,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Feed {
    pub(crate) title: String,
    pub(crate) group_code: u64,
    /// Unique and URL usable string to identify the feed
    pub(crate) slug: String,
    pub(crate) url: String,
    pub(crate) page_url: String,
    pub(crate) selector: String, // Option<String>?
    // pub(crate) produces: ["HTML", "PDF"]
    // /// Identify group by its hash
    // pub(crate) group: Option<u64>,
    /// Applied filter, from the first in the list to the last
    pub(crate) filters: Vec<u64>,
    /// Output file without extension
    pub(crate) output_file: String,
}

/// Filter structure. The name is not kept because it is only used during filter loading in order to help the user to find errors quickly.
#[derive(Debug, Clone)]
pub(crate) struct Filter {
    pub(crate) slug: String,
    /// Text or regex.
    ///
    /// If `expressions=["Elon Musk", "Tesla"]`, it will search the exact `Elon Musk` then `Tesla`. It will not be `Elon`, `Musk` and `Tesla`.
    pub(crate) expressions: Vec<String>,
    pub(crate) regexes: RegexSet,
    /// list of regex to match
    pub(crate) is_regex: bool,
    /// If the search is case sensitive, default false
    pub(crate) is_case_sensitive: bool,
    /// if all sentences and regexes must match, default `false
    pub(crate) must_match_all: bool,
    // scopes
    pub(crate) filter_in_title: bool,
    pub(crate) filter_in_summary: bool,
    pub(crate) filter_in_content: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            slug: String::new(),
            expressions: Vec::new(),
            is_regex: false,
            is_case_sensitive: false,
            must_match_all: false,
            filter_in_title: true,
            filter_in_summary: true,
            filter_in_content: true,
            regexes: RegexSet::empty(),
        }
    }
}
