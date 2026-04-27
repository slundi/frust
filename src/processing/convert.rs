use twox_hash::XxHash3_64;

use crate::model::{Article, Enclosure};

/// Convert a `feed_rs` entry into our internal [`Article`].
///
/// * `feed_id`  — XXH3 hash of the parent feed.
/// * `now_ts`   — Unix timestamp (seconds) recorded as `added_at`.
pub(super) fn entry_to_article(
    entry: &feed_rs::model::Entry,
    feed_id: u64,
    now_ts: i64,
) -> Article {
    let id = XxHash3_64::oneshot(entry.id.as_bytes());

    let title = entry
        .title
        .as_ref()
        .map(|t| t.content.clone())
        .unwrap_or_default();

    let url = entry
        .links
        .first()
        .map(|l| l.href.clone())
        .unwrap_or_else(|| entry.id.clone());

    let content = entry
        .content
        .as_ref()
        .and_then(|c| c.body.clone())
        .unwrap_or_default();

    let summary = entry.summary.as_ref().map(|s| s.content.clone());

    let timestamp = entry
        .updated
        .or(entry.published)
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    let enclosures: Vec<Enclosure> = entry
        .media
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|mc| {
            let url = mc.url.as_ref()?.to_string();
            Some(Enclosure {
                url,
                mime_type: mc
                    .content_type
                    .as_ref()
                    .map(|ct| ct.to_string())
                    .unwrap_or_default(),
                length: mc.size,
            })
        })
        .collect();

    Article {
        id,
        feed_id,
        title,
        url,
        content,
        summary,
        timestamp,
        added_at: now_ts,
        is_full_content: false,
        enclosures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rss(id: &str, title: &str, link: &str) -> feed_rs::model::Entry {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <rss version="2.0"><channel>
                <title>Test</title><link>https://example.com</link>
                <item>
                    <guid>{id}</guid>
                    <title>{title}</title>
                    <link>{link}</link>
                </item>
            </channel></rss>"#
        );
        feed_rs::parser::parse(xml.as_bytes())
            .unwrap()
            .entries
            .remove(0)
    }

    #[test]
    fn test_id_is_xxh3_of_entry_id() {
        let entry = parse_rss("my-unique-id", "Title", "https://example.com/1");
        let article = entry_to_article(&entry, 42, 1000);
        assert_eq!(article.id, XxHash3_64::oneshot(b"my-unique-id"));
    }

    #[test]
    fn test_title_extracted() {
        let entry = parse_rss("1", "Hello World", "https://example.com/1");
        let article = entry_to_article(&entry, 1, 0);
        assert_eq!(article.title, "Hello World");
    }

    #[test]
    fn test_url_from_first_link() {
        let entry = parse_rss("1", "T", "https://example.com/article");
        let article = entry_to_article(&entry, 1, 0);
        assert_eq!(article.url, "https://example.com/article");
    }

    #[test]
    fn test_feed_id_preserved() {
        let entry = parse_rss("1", "T", "https://example.com/");
        let article = entry_to_article(&entry, 999, 0);
        assert_eq!(article.feed_id, 999);
    }

    #[test]
    fn test_added_at_set_from_now_ts() {
        let entry = parse_rss("1", "T", "https://example.com/");
        let article = entry_to_article(&entry, 1, 12345);
        assert_eq!(article.added_at, 12345);
    }

    #[test]
    fn test_no_title_gives_empty_string() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <rss version="2.0"><channel>
                <title>T</title><link>https://example.com</link>
                <item><guid>no-title</guid><link>https://example.com/x</link></item>
            </channel></rss>"#;
        let entry = feed_rs::parser::parse(xml.as_bytes())
            .unwrap()
            .entries
            .remove(0);
        let article = entry_to_article(&entry, 1, 0);
        assert_eq!(article.title, "");
    }
}
