use std::collections::HashMap;

use chrono::Duration;
use regex::RegexSet;
use rkyv::{Archive, Deserialize, Serialize};

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
    /// Download media assets (images, audio, video) to `media/<xxh3>.<ext>`
    pub(crate) media: bool,
    /// Maximum asset size in bytes to download (0 = no limit)
    pub(crate) media_max_size: u64,
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
            media: false,
            media_max_size: 0,
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
    /// Download media assets (images, audio, video) to `media/<xxh3>.<ext>`
    pub(crate) media: bool,
    /// Maximum asset size in bytes to download (0 = no limit)
    pub(crate) media_max_size: u64,
}

impl Group {
    pub fn should_refresh_feed(&self, feed_slug: u64, app: &App) -> bool {
        if let Some(feed) = self.feeds.get(&feed_slug)
            && let Some(last_check) = feed.last_check
        {
            return *START_TIME.get().unwrap() - last_check
                > Duration::seconds(app.min_refresh_time);
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
    /// Download media assets (images, audio, video) to `media/<xxh3>.<ext>`
    pub(crate) media: bool,
    /// Maximum asset size in bytes to download (0 = no limit)
    pub(crate) media_max_size: u64,
    /// Mustache-style template injected before each article's content at export time.
    /// Available variables: {{feed.title}}, {{feed.url}}, {{feed.slug}}, {{feed.page_url}},
    /// {{article.title}}, {{article.url}}, {{article.id}}
    pub(crate) enrichment_prepend: Option<String>,
    /// Mustache-style template injected after each article's content at export time.
    pub(crate) enrichment_append: Option<String>,
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
            must_match_all: false,
            filter_in_title: true,
            filter_in_summary: true,
            filter_in_content: true,
            regexes: RegexSet::empty(),
            keep: false,
        }
    }
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
pub(crate) struct Article {
    /// Hash XXH3 of the original URL or GUID to identify uniqueness
    pub(crate) id: u64,
    /// Hash of the feed it belongs to
    pub(crate) feed_id: u64,
    pub(crate) title: String,
    /// The link to the original article
    pub(crate) url: String,
    /// Content converted to Markdown
    pub(crate) content: String,
    /// Summary or snippet
    pub(crate) summary: Option<String>,
    /// Date from the feed (published or updated)
    pub(crate) timestamp: i64,
    /// Date when the article was first seen by frust
    pub(crate) added_at: i64,
    /// Useful for 'Force' mode: has the full content been fetched?
    pub(crate) is_full_content: bool,
    /// List of media/enclosures (images, podcasts)
    pub(crate) enclosures: Vec<Enclosure>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
pub(crate) struct Enclosure {
    pub(crate) url: String,
    pub(crate) mime_type: String,
    pub(crate) length: Option<u64>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
// rkyv needs this to handle byte alignment and validation
pub(crate) struct FeedState {
    pub(crate) last_etag: Option<String>,
    // rkyv works best with fixed-size types or its own primitives
    // We store timestamp as i64 (seconds) for easier serialization
    pub(crate) last_check_ts: Option<i64>,
    pub(crate) last_modified_ts: Option<i64>,
    pub(crate) last_http_status: Option<u16>,
}

#[derive(Debug, Clone)]
pub(crate) enum ExportStrategy {
    /// One file containing all articles of the group (Ideal for EPUB/RSS)
    Monolithic,
    /// One file per article (Ideal for Markdown/Knowledge bases)
    Individual,
    /// One file per day (Good compromise for Journaling)
    Daily,
}

/// Feed-level enrichment context carried to exporters at runtime (not stored).
/// Templates may reference: {{feed.title}}, {{feed.url}}, {{feed.slug}},
/// {{feed.page_url}}, {{article.title}}, {{article.url}}, {{article.id}}
pub(crate) struct Enrichment {
    pub(crate) feed_title: String,
    pub(crate) feed_url: String,
    pub(crate) feed_slug: String,
    pub(crate) feed_page_url: String,
    /// Template injected before the article body.
    pub(crate) prepend: Option<String>,
    /// Template injected after the article body.
    pub(crate) append: Option<String>,
}
