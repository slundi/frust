use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use twox_hash::XxHash3_64;

use crate::{
    START_TIME,
    model::{Feed, Filter},
};

use super::content::apply_content_mode;

/// Returns `true` when the article is older than the retention window.
/// A retention of `0` means "keep forever".
pub(super) fn is_article_expired(entry_date: DateTime<Utc>, retention_days: u16) -> bool {
    if retention_days == 0 {
        return false;
    }
    (*START_TIME.get().unwrap())
        .signed_duration_since(entry_date)
        .num_days()
        >= retention_days as i64
}

/// Returns `true` when `text` satisfies the filter's regex or plain-text rules.
pub(super) fn check_text_match(text: &str, filter: &Filter) -> bool {
    if filter.is_regex {
        let matches = filter.regexes.matches(text);
        if filter.must_match_all {
            // matches.len() is the total pattern count, not the matched count
            matches.iter().count() == filter.regexes.len()
        } else {
            matches.matched_any()
        }
    } else {
        let haystack = text.to_lowercase();
        let match_count = filter
            .expressions
            .iter()
            .filter(|e| haystack.contains(e.to_lowercase().as_str()))
            .count();
        if filter.must_match_all {
            match_count == filter.expressions.len()
        } else {
            match_count > 0
        }
    }
}

/// Apply content-mode transformation, retention policy and include/exclude filters
/// to all entries in `fetched_feed`, mutating it in place.
///
/// `existing_ids` is a set of XXH3-hashed entry IDs already in storage; matching
/// entries are dropped before any expensive content enrichment takes place.
pub(super) async fn apply_filters_and_retention(
    fetched_feed: &mut feed_rs::model::Feed,
    feed_config: &Feed,
    global_filters: &HashMap<u64, Filter>,
    client: &reqwest::Client,
    selector: Option<String>,
    existing_ids: &HashSet<u64>,
) {
    // 0. Skip entries already stored — do this before content enrichment to avoid
    //    unnecessary HTTP requests (especially costly for ContentMode::Force).
    fetched_feed
        .entries
        .retain(|entry| !existing_ids.contains(&XxHash3_64::oneshot(entry.id.as_bytes())));

    // 1. Adjust content according to the configured mode
    for entry in &mut fetched_feed.entries {
        apply_content_mode(entry, &feed_config.content_mode, client, &selector).await;
    }

    // 2. Retention and filter pass
    fetched_feed.entries.retain(|entry| {
        // A. Retention check
        let entry_date = entry
            .updated
            .or(entry.published)
            .unwrap_or(*START_TIME.get().unwrap());
        if is_article_expired(entry_date, feed_config.retention) {
            return false;
        }

        // B. Filter check (inherited group + feed filters are already merged into feed_config.filters)
        for filter_id in &feed_config.filters {
            if let Some(filter) = global_filters.get(filter_id) {
                let mut is_match = false;

                if filter.filter_in_title
                    && let Some(title) = &entry.title
                    && check_text_match(&title.content, filter)
                {
                    is_match = true;
                }

                if !is_match
                    && filter.filter_in_summary
                    && let Some(summary) = &entry.summary
                    && check_text_match(&summary.content, filter)
                {
                    is_match = true;
                }

                // Skip content matching when a CSS selector is configured: the feed
                // content will be replaced by the scraped page, so it is not the
                // final text to filter on.
                if !is_match
                    && filter.filter_in_content
                    && feed_config.selector.is_none()
                    && let Some(content) = &entry.content
                    && let Some(body) = &content.body
                    && check_text_match(body, filter)
                {
                    is_match = true;
                }

                if filter.keep {
                    if !is_match {
                        return false;
                    }
                } else if is_match {
                    return false;
                }
            }
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use regex::RegexSet;

    use super::*;
    use crate::model::Filter;

    fn make_plain_filter(expressions: &[&str], must_match_all: bool, keep: bool) -> Filter {
        Filter {
            expressions: expressions.iter().map(|s| s.to_lowercase()).collect(),
            regexes: RegexSet::empty(),
            is_regex: false,
            must_match_all,
            filter_in_title: true,
            filter_in_summary: true,
            filter_in_content: true,
            keep,
        }
    }

    fn make_regex_filter(patterns: &[&str], must_match_all: bool) -> Filter {
        Filter {
            expressions: vec![],
            regexes: RegexSet::new(patterns).unwrap(),
            is_regex: true,
            must_match_all,
            filter_in_title: true,
            filter_in_summary: true,
            filter_in_content: true,
            keep: true,
        }
    }

    // ---- is_article_expired ----

    fn init_start_time() {
        // Safe to call multiple times: OnceLock ignores subsequent sets.
        let _ = crate::START_TIME.set(Utc::now());
    }

    #[test]
    fn test_expired_retention_zero_never_expires() {
        init_start_time();
        let ancient = Utc::now() - chrono::Duration::days(9999);
        assert!(!is_article_expired(ancient, 0));
    }

    #[test]
    fn test_expired_old_article_beyond_retention() {
        init_start_time();
        // Article 10 days old, retention 7 days → expired
        let old = Utc::now() - chrono::Duration::days(10);
        assert!(is_article_expired(old, 7));
    }

    #[test]
    fn test_expired_recent_article_within_retention() {
        init_start_time();
        // Article 3 days old, retention 7 days → not expired
        let recent = Utc::now() - chrono::Duration::days(3);
        assert!(!is_article_expired(recent, 7));
    }

    #[test]
    fn test_expired_exactly_at_boundary() {
        init_start_time();
        let start = *START_TIME.get().unwrap();
        // Article dated exactly `retention` days before start_time → expired (>=)
        let boundary = start - chrono::Duration::days(7);
        assert!(is_article_expired(boundary, 7));
    }

    // ---- check_text_match (plain text) ----

    #[test]
    fn test_plain_match_single_expression() {
        let f = make_plain_filter(&["rust"], false, true);
        assert!(check_text_match("I love Rust programming", &f));
    }

    #[test]
    fn test_plain_no_match() {
        let f = make_plain_filter(&["python"], false, true);
        assert!(!check_text_match("I love Rust programming", &f));
    }

    #[test]
    fn test_plain_case_insensitive() {
        let f = make_plain_filter(&["RUST"], false, true);
        assert!(check_text_match("I love rust programming", &f));
    }

    #[test]
    fn test_plain_must_match_all_success() {
        let f = make_plain_filter(&["rust", "programming"], true, true);
        assert!(check_text_match("Rust programming is great", &f));
    }

    #[test]
    fn test_plain_must_match_all_partial_fails() {
        let f = make_plain_filter(&["rust", "python"], true, true);
        assert!(!check_text_match("Rust programming is great", &f));
    }

    #[test]
    fn test_plain_match_any_one_of_two() {
        let f = make_plain_filter(&["rust", "python"], false, true);
        assert!(check_text_match("Python is popular", &f));
    }

    // ---- check_text_match (regex) ----

    #[test]
    fn test_regex_match() {
        let f = make_regex_filter(&[r"\bRust\b"], false);
        assert!(check_text_match("Rust is fast", &f));
    }

    #[test]
    fn test_regex_no_match() {
        let f = make_regex_filter(&[r"\bPython\b"], false);
        assert!(!check_text_match("Rust is fast", &f));
    }

    #[test]
    fn test_regex_must_match_all() {
        let f = make_regex_filter(&[r"\bRust\b", r"\bfast\b"], true);
        assert!(check_text_match("Rust is fast", &f));
        assert!(!check_text_match("Rust is slow", &f));
    }

    // ---- apply_filters_and_retention (keep semantics) ----

    /// Build a `feed_rs::model::Feed` with the given item titles. Each item's
    /// GUID is the title itself so `existing_ids` dedup is easy to control.
    fn feed_with_titles(titles: &[&str]) -> feed_rs::model::Feed {
        let items: String = titles
            .iter()
            .map(|t| format!("<item><guid>{t}</guid><title>{t}</title></item>"))
            .collect::<Vec<_>>()
            .join("\n");
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <rss version="2.0"><channel>
                <title>T</title><link>https://example.com</link>
                {items}
            </channel></rss>"#
        );
        feed_rs::parser::parse(xml.as_bytes()).unwrap()
    }

    fn feed_config_with_filter(filter_id: u64) -> crate::model::Feed {
        crate::model::Feed {
            title: "test".into(),
            slug: "test".into(),
            url: "https://example.com/feed.xml".into(),
            page_url: String::new(),
            content_mode: crate::model::ContentMode::Default,
            selector: None,
            filters: vec![filter_id],
            output: String::new(),
            retention: 0,
            media: false,
            media_max_size: 0,
            enrichment_prepend: None,
            enrichment_append: None,
        }
    }

    #[tokio::test]
    async fn test_apply_filters_keep_true_includes_only_matches() {
        init_start_time();
        let mut feed = feed_with_titles(&["Rust is great", "Python is fine", "Go is nice"]);
        let filter_id = 1u64;
        let mut filters = HashMap::new();
        filters.insert(filter_id, make_plain_filter(&["rust"], false, true));
        let feed_cfg = feed_config_with_filter(filter_id);

        apply_filters_and_retention(
            &mut feed,
            &feed_cfg,
            &filters,
            &reqwest::Client::new(),
            None,
            &HashSet::new(),
        )
        .await;

        // keep:true → only entries whose title matches "rust" survive.
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(
            feed.entries[0].title.as_ref().unwrap().content,
            "Rust is great"
        );
    }

    #[tokio::test]
    async fn test_apply_filters_keep_false_excludes_matches() {
        init_start_time();
        let mut feed = feed_with_titles(&["Rust is great", "Python is fine", "Go is nice"]);
        let filter_id = 2u64;
        let mut filters = HashMap::new();
        filters.insert(filter_id, make_plain_filter(&["rust"], false, false));
        let feed_cfg = feed_config_with_filter(filter_id);

        apply_filters_and_retention(
            &mut feed,
            &feed_cfg,
            &filters,
            &reqwest::Client::new(),
            None,
            &HashSet::new(),
        )
        .await;

        // keep:false → matching entries are dropped, others remain.
        assert_eq!(feed.entries.len(), 2);
        let titles: Vec<&str> = feed
            .entries
            .iter()
            .map(|e| e.title.as_ref().unwrap().content.as_str())
            .collect();
        assert!(titles.contains(&"Python is fine"));
        assert!(titles.contains(&"Go is nice"));
        assert!(!titles.contains(&"Rust is great"));
    }

    #[tokio::test]
    async fn test_apply_filters_keep_true_drops_all_when_no_match() {
        init_start_time();
        let mut feed = feed_with_titles(&["Python is fine", "Go is nice"]);
        let filter_id = 3u64;
        let mut filters = HashMap::new();
        filters.insert(filter_id, make_plain_filter(&["rust"], false, true));
        let feed_cfg = feed_config_with_filter(filter_id);

        apply_filters_and_retention(
            &mut feed,
            &feed_cfg,
            &filters,
            &reqwest::Client::new(),
            None,
            &HashSet::new(),
        )
        .await;

        // keep:true with no matches → everything is dropped.
        assert!(feed.entries.is_empty());
    }
}
