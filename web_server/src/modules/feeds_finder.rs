// Auto-discovering RSS, Atom, JSON feeds from the content of a page, based on [feedfinder](https://github.com/wezm/feedfinder/).
//
// You supply the primary function [detect_feeds](fn.detect_feeds.html) with the content of
// a HTML page and the URL of that page and it returns a list of possible feeds.
//
// ## About
//
// Find feeds from these sources:
//
// * Linked via the `<link>` tag in the HTML
// * Linked via `<a>` tag in the HTML
// * By guessing from the software used to generate the page:
//     * Tumblr
//     * WordPress
//     * Hugo
//     * Jekyll
//     * Ghost
// * From YouTube:
//     * channels
//     * playlists
//     * users

use std::fmt;

use url::Url;

use scraper::{Html, Selector};

const MIGHT_BE_FEED: [&str; 4] = ["feed", "xml", "rss", "atom"];

#[derive(Debug, PartialEq)]
pub enum FeedFinderError {
    Url(url::ParseError),
    Select,
}
impl fmt::Display for FeedFinderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedFinderError::Url(err) => err.fmt(f),
            FeedFinderError::Select => f.write_str("unable to select elements in doc"),
        }
    }
}
impl std::error::Error for FeedFinderError {}

#[derive(Debug, PartialEq)]
pub enum FeedType {
    Rss,
    Atom,
    Json,
    Link,
    Guess,
}

#[derive(Debug, PartialEq)]
pub struct FeedEntry {
    url: Url,
    type_: FeedType,
    title: Option<String>,
}

struct FeedFinder {
    url: Url,
    raw: String,
    html: Html,
}

type FeedResult = Result<Vec<FeedEntry>, FeedFinderError>;

fn nth_path_segment(url: &Url, nth: usize) -> Option<&str> {
    url.path_segments()
        .and_then(|mut segments| segments.nth(nth))
}
pub(crate) fn detect_feeds(url: String, html: String) -> FeedResult {
    let sources = [FeedFinder::meta_links, FeedFinder::youtube, FeedFinder::body_links, FeedFinder::guess];
    let finder = FeedFinder {
        url: Url::parse(&url).expect("Invalid URL"),
        raw: html.clone(),
        html: Html::parse_document(&html)
    };
    for source in &sources {
        let candidates = source(&finder)?;
        if !candidates.is_empty() {
            return Ok(candidates);
        }
    }
    Ok(Vec::new())
}

impl FeedFinder {
    /// Find RSS links in the `link` HTML tag (ex: `<link rel="alternate" type="application/rss+xml" title="Korben &raquo; Flux" href="https://korben.info/feed" />`)
    fn meta_links(&self) -> FeedResult {
        let mut feeds = vec![];
        let selector = Selector::parse("head.link").unwrap();
        for link in self.html.select(&selector) {
            let title = link.value().attr("title").map(|title| title.to_owned());
            match (link.value().attr("href"), link.value().attr("type")) {
                (Some("application/rss+xml"), Some(href)) => feeds.push(FeedEntry {
                    url: self.url.join(href).map_err(FeedFinderError::Url)?,
                    type_: FeedType::Rss,
                    title,
                }),
                (Some("application/atom+xml"), Some(href)) => feeds.push(FeedEntry {
                    url: self.url.join(href).map_err(FeedFinderError::Url)?,
                    type_: FeedType::Atom,
                    title,
                }),
                (Some("application/json"), Some(href)) => feeds.push(FeedEntry {
                    url: self.url.join(href).map_err(FeedFinderError::Url)?,
                    type_: FeedType::Json,
                    title,
                }),
                _ => (),
            }
        }
        Ok(feeds)
    }

    /// Searches the body for links to things that might be feeds
    fn body_links(&self) -> FeedResult {
        let mut feeds = vec![];
        let selector = Selector::parse("a").unwrap();
        for a in self.html.select(&selector) {
            if let Some(href) = a.value().attr("href") {
                if MIGHT_BE_FEED.iter().any(|hint| href.contains(hint)) {
                    feeds.push(FeedEntry {
                        url: self.url.join(href).map_err(FeedFinderError::Url)?,
                        type_: FeedType::Link,
                        title: None,
                    })
                }
            }
        }
        Ok(feeds)
    }

    fn youtube(&self) -> FeedResult {
        let mut feeds = vec![];
        let url = self.url.as_str();

        if url.starts_with("https://www.youtube.com/channel/") {
            // Get the path segment after /channel/
            if let Some(id) = nth_path_segment(&self.url, 1) {
                let feed = Url::parse(&format!(
                    "https://www.youtube.com/feeds/videos.xml?channel_id={}",
                    id
                ))
                .map_err(FeedFinderError::Url)?;
                feeds.push(FeedEntry {
                    url: feed,
                    type_: FeedType::Atom,
                    title: None,
                });
            }
        } else if url.starts_with("https://www.youtube.com/user/") {
            // Get the path segment after /user/
            if let Some(id) = nth_path_segment(&self.url, 1) {
                let feed = Url::parse(&format!(
                    "https://www.youtube.com/feeds/videos.xml?user={}",
                    id
                ))
                .map_err(FeedFinderError::Url)?;
                feeds.push(FeedEntry {
                    url: feed,
                    type_: FeedType::Atom,
                    title: None,
                });
            }
        } else if url.starts_with("https://www.youtube.com/playlist?list=")
            || url.starts_with("https://www.youtube.com/watch")
        {
            // get the value of the list query param
            for (key, value) in self.url.query_pairs() {
                if key == "list" {
                    let feed = Url::parse(&format!(
                        "https://www.youtube.com/feeds/videos.xml?playlist_id={}",
                        value
                    ))
                    .map_err(FeedFinderError::Url)?;
                    feeds.push(FeedEntry {
                        url: feed,
                        type_: FeedType::Atom,
                        title: None,
                    });
                    break;
                }
            }
        }

        Ok(feeds)
    }

    /// Guesses the feed for some well known locations:
    /// * Tumblr
    /// * Wordpress
    /// * Ghost
    /// * Jekyll
    /// * Hugo
    fn guess(&self) -> FeedResult {
        let markup = self.raw.to_lowercase();

        let url = if markup.contains("tumblr.com") {
            Some(self.url.join("/rss").map_err(FeedFinderError::Url)?)
        } else if markup.contains("wordpress") {
            Some(self.url.join("/feed").map_err(FeedFinderError::Url)?)
        } else if markup.contains("hugo") {
            return self.guess_segments("index.xml".to_owned());
        } else if markup.contains("jekyll")
            || self
                .url
                .host_str()
                .map(|host| host.ends_with("github.io"))
                .unwrap_or(false)
        {
            return self.guess_segments("atom.xml".to_owned());
        } else if markup.contains("ghost") {
            Some(self.url.join("/rss/").map_err(FeedFinderError::Url)?)
        } else {
            None
        };

        Ok(url
            .map(|url| {
                vec![FeedEntry {
                    url,
                    type_: FeedType::Guess,
                    title: None,
                }]
            })
            .unwrap_or_else(Vec::new))
    }

    // Well this sure isn't pretty. TODO: Clean up
    fn guess_segments(&self, feed_file: String) -> FeedResult {
        let mut feeds = Vec::new();

        if let Some(segments) = self.url.path_segments() {
            let mut remaining_segments = segments.collect::<Vec<_>>();
            let mut segments = vec!["", &feed_file];

            loop {
                let url = self
                    .url
                    .join(&segments.join("/"))
                    .map_err(FeedFinderError::Url)?;
                feeds.push(FeedEntry {
                    url,
                    type_: FeedType::Guess,
                    title: None,
                });
                if remaining_segments.is_empty() {
                    break;
                }

                let index = segments.len() - 1;
                let segment = remaining_segments.remove(0);
                if segment.is_empty() {
                    // Skip empty strings, which should only occur as the last element
                    break;
                }

                segments.insert(index, segment);
            }
        }

        Ok(feeds)
    }
}
