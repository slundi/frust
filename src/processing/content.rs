use htmd::HtmlToMarkdown;
use mediatype::MediaTypeBuf;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::model::ContentMode;

/// Merge entries from `entries` into `base`, skipping any whose ID already exists.
#[allow(dead_code)]
pub(super) fn merge_feeds_by_id(
    base: &mut feed_rs::model::Feed,
    entries: Vec<feed_rs::model::Entry>,
) {
    let existing_ids: std::collections::HashSet<String> =
        base.entries.iter().map(|e| e.id.clone()).collect();
    for entry in entries {
        if !existing_ids.contains(&entry.id) {
            base.entries.push(entry);
        }
    }
}

/// Fetch a URL and return the inner HTML of the first element matching `selector`.
#[allow(dead_code)]
pub(super) async fn get_link_data(
    client: &Client,
    url: &str,
    selector: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match client.get(url).send().await {
        Ok(response) => match response.text().await {
            Ok(data) => {
                let document = Html::parse_document(&data);
                let css_selector = Selector::parse(selector).unwrap();
                match document.select(&css_selector).next() {
                    Some(element) => return Ok(element.html()),
                    None => tracing::error!("No content found for selector: {}", selector),
                }
            }
            Err(e) => tracing::error!("Cannot get response text for selector: {} \t {:?}", url, e),
        },
        Err(e) => tracing::warn!("Cannot open link for selector: {} \t {:?}", url, e),
    };
    Ok(String::new())
}

/// Adjust entry content based on the configured `ContentMode`.
pub(super) async fn apply_content_mode(
    entry: &mut feed_rs::model::Entry,
    mode: &ContentMode,
    client: &Client,
    selector_str: &Option<String>,
) {
    let converter = HtmlToMarkdown::new();

    match mode {
        ContentMode::No | ContentMode::LinksOnly => {
            entry.content = None;
            entry.summary = None;
        }
        ContentMode::Default => {
            // Convert existing HTML content to Markdown in place
            if let Some(content) = &mut entry.content
                && let Some(body) = &content.body
            {
                content.body = Some(converter.convert(body).unwrap_or_else(|_| body.clone()));
            }
        }
        ContentMode::Brief => {
            // Keep title + summary only, drop full content
            entry.content = None;
        }
        ContentMode::Force => {
            // Clear feed-provided summary; the scraped page becomes the content
            entry.summary = None;

            if let Some(link) = entry.links.first()
                && let Ok(resp) = client.get(&link.href).send().await
                && let Ok(html_content) = resp.text().await
            {
                let document = Html::parse_document(&html_content);
                let selector = selector_str.as_deref().unwrap_or("article, main, .content");
                if let Ok(sel) = Selector::parse(selector)
                    && let Some(element) = document.select(&sel).next()
                {
                    let inner_html = element.inner_html();
                    let markdown = converter
                        .convert(&inner_html)
                        .unwrap_or_else(|_| inner_html.clone());
                    match entry.content {
                        Some(ref mut c) => c.body = Some(markdown),
                        None => {
                            entry.content = Some(feed_rs::model::Content {
                                body: Some(markdown),
                                content_type: "text/plain".parse::<MediaTypeBuf>().unwrap(),
                                length: None,
                                src: None,
                            });
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a minimal RSS document with the given entry GUIDs.
    fn parse_feed(ids: &[&str]) -> feed_rs::model::Feed {
        let items: String = ids
            .iter()
            .map(|id| format!("<item><guid>{id}</guid><title>T</title></item>"))
            .collect::<Vec<_>>()
            .join("\n");
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <rss version="2.0"><channel>
                <title>Test</title><link>https://example.com</link>
                {items}
            </channel></rss>"#
        );
        feed_rs::parser::parse(xml.as_bytes()).unwrap()
    }

    fn make_text(content: &str) -> feed_rs::model::Text {
        feed_rs::model::Text {
            content_type: "text/plain".parse::<MediaTypeBuf>().unwrap(),
            src: None,
            content: content.to_string(),
        }
    }

    fn make_content(body: &str) -> feed_rs::model::Content {
        feed_rs::model::Content {
            body: Some(body.to_string()),
            content_type: "text/plain".parse::<MediaTypeBuf>().unwrap(),
            length: None,
            src: None,
        }
    }

    // --- merge_feeds_by_id ---

    #[test]
    fn test_merge_adds_new_entries() {
        let mut base = parse_feed(&["a", "b"]);
        let incoming = parse_feed(&["c", "d"]).entries;
        merge_feeds_by_id(&mut base, incoming);
        assert_eq!(base.entries.len(), 4);
        let ids: Vec<&str> = base.entries.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"c") && ids.contains(&"d"));
    }

    #[test]
    fn test_merge_skips_duplicate_ids() {
        let mut base = parse_feed(&["a", "b"]);
        let incoming = parse_feed(&["b", "c"]).entries;
        merge_feeds_by_id(&mut base, incoming);
        // "b" must not be duplicated
        assert_eq!(base.entries.len(), 3);
        assert_eq!(base.entries.iter().filter(|e| e.id == "b").count(), 1);
    }

    #[test]
    fn test_merge_into_empty_base() {
        let mut base = parse_feed(&[]);
        let incoming = parse_feed(&["x", "y"]).entries;
        merge_feeds_by_id(&mut base, incoming);
        assert_eq!(base.entries.len(), 2);
    }

    #[test]
    fn test_merge_with_empty_incoming() {
        let mut base = parse_feed(&["a"]);
        merge_feeds_by_id(&mut base, vec![]);
        assert_eq!(base.entries.len(), 1);
    }

    // --- apply_content_mode ---

    #[tokio::test]
    async fn test_content_mode_no_clears_content_and_summary() {
        let client = Client::new();
        let mut entry = parse_feed(&["1"]).entries.remove(0);
        entry.summary = Some(make_text("summary"));
        entry.content = Some(make_content("body"));

        apply_content_mode(&mut entry, &ContentMode::No, &client, &None).await;

        assert!(entry.content.is_none());
        assert!(entry.summary.is_none());
    }

    #[tokio::test]
    async fn test_content_mode_links_only_clears_content_and_summary() {
        let client = Client::new();
        let mut entry = parse_feed(&["1"]).entries.remove(0);
        entry.summary = Some(make_text("summary"));
        entry.content = Some(make_content("body"));

        apply_content_mode(&mut entry, &ContentMode::LinksOnly, &client, &None).await;

        assert!(entry.content.is_none());
        assert!(entry.summary.is_none());
    }

    #[tokio::test]
    async fn test_content_mode_brief_keeps_summary_drops_content() {
        let client = Client::new();
        let mut entry = parse_feed(&["1"]).entries.remove(0);
        entry.summary = Some(make_text("my summary"));
        entry.content = Some(make_content("full body"));

        apply_content_mode(&mut entry, &ContentMode::Brief, &client, &None).await;

        assert!(entry.content.is_none());
        assert!(entry.summary.is_some());
    }

    #[tokio::test]
    async fn test_content_mode_default_preserves_content() {
        let client = Client::new();
        let mut entry = parse_feed(&["1"]).entries.remove(0);
        entry.content = Some(feed_rs::model::Content {
            body: Some("<p>Hello</p>".into()),
            content_type: "text/html".parse::<MediaTypeBuf>().unwrap(),
            length: None,
            src: None,
        });

        apply_content_mode(&mut entry, &ContentMode::Default, &client, &None).await;

        // Content should still be present (converted to MD)
        assert!(entry.content.is_some());
    }
}
