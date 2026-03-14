use std::collections::HashMap;

use chrono::Duration;
use regex::RegexSet;

use crate::{DEFAULT_HTTP_TIMEOUT, DEFAULT_RETRIEVE_SERVER_MEDIA, START_TIME};

#[derive(Debug, Clone)]
pub(crate) struct App {
    pub(crate) output: String,
    /// Timeout in seconds
    pub(crate) timeout: u8,
    pub(crate) retrieve_media_server: bool,
    /// min refresh time in second
    pub(crate) min_refresh_time: i64,
    // https://stackoverflow.com/questions/22155130/
    pub(crate) workers: usize,
    /// List of filters, the u64 key is a XXH3 of the slug
    pub(crate) filters: HashMap<u64, Filter>,
    /// All groups, the key is a xxh3 of the slug
    pub(crate) groups: HashMap<u64, Group>,
    /// Article retention in days, when defined retention, they are not kept
    pub(crate) retention: u16,
}

impl Default for App {
    fn default() -> Self {
        Self {
            output: std::env::current_dir().unwrap().display().to_string(),
            retrieve_media_server: DEFAULT_RETRIEVE_SERVER_MEDIA,
            timeout: DEFAULT_HTTP_TIMEOUT,
            min_refresh_time: 600,
            workers: std::thread::available_parallelism().unwrap().get(),
            filters: HashMap::with_capacity(0),
            groups: HashMap::with_capacity(0),
            retention: 0,
        }
    }
}

impl App {
    pub fn has_output(&self, group_code: u64, feed_code: u64) -> bool {
        let group = self.groups.get(&group_code).unwrap();
        let feed = group.feeds.get(&feed_code).unwrap();
        !feed.output.is_empty()
    }

    pub fn get_output(&self, group_code: u64, feed_code: u64) -> String {
        let group = self.groups.get(&group_code).unwrap();
        let feed = group.feeds.get(&feed_code).unwrap();
        if !feed.output.is_empty() {
            return feed.output.clone();
        }
        group.output.clone()
    }

    pub fn is_article_too_old(
        &self,
        article_date: chrono::DateTime<chrono::Utc>,
        retention_days: u16,
    ) -> bool {
        let expiration = *START_TIME.get().unwrap() - Duration::days(retention_days as i64);
        article_date < expiration
    }
}

/// A group allows you to produce a unique feed by aggregating the enumerated ones.
#[derive(Debug, Default, PartialEq, Clone)]
pub(crate) struct Group {
    pub(crate) title: String,
    pub(crate) slug: String,
    /// All feeds, the key is a xxh3 of the slug
    pub(crate) feeds: HashMap<u64, Feed>,
    /// Applied filter, from the first in the list to the last
    pub(crate) filters: Vec<u64>,
    /// Set this output file path if you want to aggregate the feeds in the group
    pub(crate) output: String,
    /// Article retention in days
    pub(crate) retention: u16,
}

impl Group {
    pub fn should_refresh_feed(&self, feed_slug: u64, app: &App) -> bool {
        if let Some(feed) = self.feeds.get(&feed_slug) {
            if let Some(last_check) = feed.last_check {
                // Vérifie si le délai minimal (ex: 600s) est passé
                return *START_TIME.get().unwrap() - last_check
                    > Duration::seconds(app.min_refresh_time);
            }
        }
        true // new feed or never checked
    }
}

#[repr(u8)]
#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ContentMode {
    /// Default content in the field
    Default,
    /// No content, only keep the title
    No,
    /// Keep title and summary
    Brief,
    /// Title and try to get the article content on the page because some sites
    /// are forcing you to click on the feed to visit their website
    Force,
    /// Title and only keep the links on the page, may be usefull for
    /// downloadable stuffs
    LinksOnly,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Feed {
    pub(crate) title: String,
    /// Unique and URL usable string to identify the feed
    pub(crate) slug: String,
    pub(crate) url: String,
    pub(crate) page_url: String,
    pub(crate) content_mode: ContentMode,
    /// CSS selector
    pub(crate) selector: Option<String>,
    // pub(crate) produces: ["HTML", "PDF"]
    // /// Identify group by its hash
    // pub(crate) group: Option<u64>,
    /// Applied filter, from the first in the list to the last
    pub(crate) filters: Vec<u64>,
    /// Output file
    pub(crate) output: String,
    /// Article retention in days
    pub(crate) retention: u16,
    /// Entity tag timestamp in order to optimize cache.
    /// We need to send the header `If-None-Match` with our timestamp, then the
    /// server should return a 304 Not Modified with no content se we do not
    /// need to process anything for this field
    pub(crate) last_etag: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_modified: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_check: Option<chrono::DateTime<chrono::Utc>>,
}

/// Filter structure. The name is not kept because it is only used during filter loading in order to help the user to find errors quickly.
#[derive(Debug, Clone)]
pub(crate) struct Filter {
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
    /// `true` to only keep article matching otherwise `false` to exclude
    pub(crate) keep: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            expressions: Vec::new(),
            is_regex: false,
            is_case_sensitive: false,
            must_match_all: false,
            filter_in_title: true,
            filter_in_summary: true,
            filter_in_content: true,
            regexes: RegexSet::empty(),
            keep: false,
        }
    }
}
