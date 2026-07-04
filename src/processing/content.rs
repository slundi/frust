use std::net::{Ipv4Addr, Ipv6Addr};

use htmd::HtmlToMarkdown;
use mediatype::MediaTypeBuf;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::model::ContentMode;

/// Guard against feed-driven SSRF into private/loopback targets.
///
/// Returns `true` only for `http`/`https` URLs whose host is either
/// * a domain name that isn't a well-known loopback alias, or
/// * an IP literal that is neither loopback/private/link-local/unspecified.
///
/// This is a best-effort check on the URL as it appears in the feed; a full
/// defence would also resolve DNS and re-check the answer (DNS rebinding),
/// which is out of scope here.
pub(super) fn is_safe_force_target(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = u.host() else {
        return false;
    };
    match host {
        url::Host::Domain(name) => !is_forbidden_domain(name),
        url::Host::Ipv4(addr) => is_public_ipv4(addr),
        url::Host::Ipv6(addr) => is_public_ipv6(addr),
    }
}

fn is_forbidden_domain(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let l = lower.as_str();
    // Common loopback aliases and RFC 6762/6761 local-scope suffixes.
    l == "localhost"
        || l.ends_with(".localhost")
        || l.ends_with(".local")
        || l.ends_with(".internal")
}

fn is_public_ipv4(addr: Ipv4Addr) -> bool {
    !addr.is_loopback()
        && !addr.is_private()
        && !addr.is_link_local()
        && !addr.is_broadcast()
        && !addr.is_multicast()
        && !addr.is_unspecified()
}

fn is_public_ipv6(addr: Ipv6Addr) -> bool {
    if addr.is_loopback() || addr.is_unspecified() || addr.is_multicast() {
        return false;
    }
    let seg0 = addr.segments()[0];
    // fc00::/7 — Unique Local Addresses
    if (seg0 & 0xfe00) == 0xfc00 {
        return false;
    }
    // fe80::/10 — link-local
    if (seg0 & 0xffc0) == 0xfe80 {
        return false;
    }
    // IPv4-mapped: check the mapped v4 is public too.
    if let Some(v4) = addr.to_ipv4_mapped()
        && !is_public_ipv4(v4)
    {
        return false;
    }
    true
}

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

            let Some(link) = entry.links.first() else {
                return;
            };
            if !is_safe_force_target(&link.href) {
                tracing::warn!(
                    "Refusing Force-mode fetch of '{}' (non-http scheme or private/loopback host)",
                    link.href
                );
                return;
            }

            if let Ok(resp) = client.get(&link.href).send().await
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

    // --- is_safe_force_target ---

    #[test]
    fn test_ssrf_guard_accepts_public_https_domain() {
        assert!(is_safe_force_target("https://example.com/article"));
        assert!(is_safe_force_target("http://blog.rust-lang.org/2024/x"));
    }

    #[test]
    fn test_ssrf_guard_rejects_non_http_schemes() {
        assert!(!is_safe_force_target("file:///etc/passwd"));
        assert!(!is_safe_force_target("ftp://ftp.example.com/x"));
        assert!(!is_safe_force_target("gopher://example.com/"));
        assert!(!is_safe_force_target("javascript:alert(1)"));
    }

    #[test]
    fn test_ssrf_guard_rejects_localhost_aliases() {
        assert!(!is_safe_force_target("http://localhost/x"));
        assert!(!is_safe_force_target("http://LOCALHOST/x"));
        assert!(!is_safe_force_target("http://foo.localhost/x"));
        assert!(!is_safe_force_target("http://router.local/x"));
        assert!(!is_safe_force_target("http://admin.internal/x"));
    }

    #[test]
    fn test_ssrf_guard_rejects_private_ipv4_literals() {
        for host in [
            "127.0.0.1",
            "127.1.2.3",
            "10.0.0.1",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "192.168.255.255",
            "169.254.169.254", // AWS/GCP metadata
            "0.0.0.0",
            "255.255.255.255",
        ] {
            let url = format!("http://{}/x", host);
            assert!(
                !is_safe_force_target(&url),
                "should reject private/reserved IPv4: {}",
                url
            );
        }
    }

    #[test]
    fn test_ssrf_guard_accepts_public_ipv4_literals() {
        assert!(is_safe_force_target("http://8.8.8.8/x"));
        assert!(is_safe_force_target("http://1.1.1.1/x"));
    }

    #[test]
    fn test_ssrf_guard_rejects_private_ipv6() {
        for host in ["[::1]", "[::]", "[fe80::1]", "[fc00::1]", "[fd12:3456::1]"] {
            let url = format!("http://{}/x", host);
            assert!(
                !is_safe_force_target(&url),
                "should reject private IPv6: {}",
                url
            );
        }
    }

    #[test]
    fn test_ssrf_guard_rejects_ipv4_mapped_private_ipv6() {
        // ::ffff:127.0.0.1 wraps loopback in IPv6 syntax
        assert!(!is_safe_force_target("http://[::ffff:127.0.0.1]/x"));
        assert!(!is_safe_force_target("http://[::ffff:192.168.1.1]/x"));
    }

    #[test]
    fn test_ssrf_guard_rejects_malformed_urls() {
        assert!(!is_safe_force_target(""));
        assert!(!is_safe_force_target("not a url"));
        assert!(!is_safe_force_target("http://"));
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
