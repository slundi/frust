pub(crate) mod feed;
pub(crate) mod feeds_finder;

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Error {
    /// If we cannot parse the URL page for the feed
    InvalidPageUrl,
    /// If we cannot parse the URL feed
    InvalidFeedUrl,
    /// If the OPML outline element has a `type` attribute different from `rss`
    NoRssTypeInOutline,
    /// If the text field in the outline element is empty
    NoTextInOpmlOutline,
    /// In case the page URL or the feed URL is unreachable (404, timeout, ...)
    UnreachableUrl,
}
