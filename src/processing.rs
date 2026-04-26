use std::collections::HashMap;

use chrono::prelude::*;
use feed_rs::parser;
use futures::{StreamExt, stream};
use htmd::HtmlToMarkdown;
use reqwest::{Client, header};
use scraper::{Html, Selector};

use crate::{
    START_TIME,
    model::{App, ContentMode, Feed, Filter},
    utils::is_refresh_required,
};

/// Check if an article date is older than the retention policy
fn is_article_expired(entry_date: DateTime<Utc>, retention_days: u16) -> bool {
    if retention_days == 0 {
        return false;
    }
    (*START_TIME.get().unwrap())
        .signed_duration_since(entry_date)
        .num_days()
        >= retention_days as i64
}

async fn get_response_feed(
    response: reqwest::Response,
    url: &String,
) -> Option<feed_rs::model::Feed> {
    let parser = parser::Builder::new().sanitize_content(true).build();
    let mut result: Option<feed_rs::model::Feed> = None;
    match response.bytes().await {
        // read the response
        Ok(content) => {
            match parser.parse(content.as_ref()) {
                // load feed data
                Ok(feed) => result = Some(feed),
                Err(e) => tracing::error!("Unable to parse feed from: {}     {}", url, e),
            }
        }
        Err(e) => tracing::error!("Unable to read response from feed: {}     {}", url, e),
    };
    result
}

fn text_is_found(text: String, filter_id: u64, filters: &HashMap<u64, Filter>) -> bool {
    let filter = filters.get(&filter_id).unwrap();
    // regex search
    let regex_found = if filter.regexes.is_empty() {
        true
    } else {
        let matches = filter.regexes.matches(&text);
        (filter.must_match_all && filter.regexes.len() == matches.len())
            || (!filter.must_match_all && matches.matched_any())
    };
    // sentence/word search
    let sentence_found = if filter.expressions.is_empty() {
        true
    } else {
        // if case insensitive, expressions are in lower case (loaded in [config.rs](config.rs))
        let content = text.to_lowercase();
        let mut count_found = 0usize;
        for exp in &filter.expressions {
            if content.contains(exp) {
                count_found += 1;
            }
        }
        (filter.must_match_all && filter.expressions.len() == count_found)
            || (!filter.must_match_all && count_found > 0)
    };
    regex_found && sentence_found
}

/// Apply filters to article. It returns `true` when a filter has matched or if the filter list is empty.
fn apply_filters_to_entry(
    entry: &feed_rs::model::Entry,
    applied_filters: &Vec<u64>,
    filters: &HashMap<u64, Filter>,
) -> bool {
    for filter_id in applied_filters {
        let filter = filters.get(filter_id).unwrap();
        if filter.filter_in_title
            && let Some(value) = &entry.title
            && text_is_found(value.content.clone(), *filter_id, filters)
        {
            return true;
        }
        if filter.filter_in_summary
            && let Some(value) = &entry.summary
            && text_is_found(value.content.clone(), *filter_id, filters)
        {
            return true;
        }
        if filter.filter_in_content
            && let Some(value) = &entry.content
            && text_is_found(
                value.body.clone().unwrap_or(String::with_capacity(0)),
                *filter_id,
                filters,
            )
        {
            return true;
        }
    }
    applied_filters.is_empty()
}

/// Merge Feeds in `base` with the given `entries`. If the ID are the same, the entry is skipped.
fn merge_feeds_by_id(base: &mut feed_rs::model::Feed, entries: Vec<feed_rs::model::Entry>) {
    let mut ids: Vec<String> = Vec::with_capacity(base.entries.len());
    for entry in base.entries.iter() {
        ids.push(entry.id.clone());
    }
    for entry in entries.iter() {
        if !ids.contains(&entry.id) {
            base.entries.push(entry.clone());
        }
    }
}

async fn get_link_data(
    client: &Client,
    url: &str,
    selector: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // TODO: replace println with a better error handling mechanism
    match client.get(url).send().await {
        Ok(response) => match response.text().await {
            Ok(data) => {
                let document = scraper::Html::parse_document(&data);
                let css_selector = scraper::Selector::parse(selector).unwrap();
                match document.select(&css_selector).next() {
                    Some(element) => return Ok(element.html()),
                    _ => tracing::error!("No content found for selector: {}", selector),
                }
            }
            Err(e) => tracing::error!("Cannot get response text for selector: {} \t {:?}", url, e),
        },
        Err(e) => tracing::warn!("Cannot open link for selector: {} \t {:?}", url, e),
    };
    Ok(String::with_capacity(0))
}

async fn add_new_articles(
    feed_id: u64,
    file_feed: Option<feed_rs::model::Feed>,
    retrieved_feed: feed_rs::model::Feed,
    feeds: &HashMap<u64, Feed>,
) {
    // TODO: process retrieved data:
    // - If applicable, retrieve articles (multiple per source) and its assets if applicable
    let client = Client::new();
    let mut rf = retrieved_feed.clone();
    if let Some(ff) = file_feed {
        merge_feeds_by_id(&mut rf, ff.entries);
    }
    // check if feed is present in the file and keep it if yes (already filtered)
    rf.entries.retain(|entry| {
        let mut should_add = true;
        // check if entry should be kept (storage time)
        if is_refresh_required(
            entry.updated,
            *START_TIME.get().unwrap(),
            feeds.get(&feed_id).unwrap().retention as i64 * 86400,
        ) {
            should_add = false;
        }
        // Apply filters (do not match content if CSS selector is specified)
        // TODO: handle blanks (\n, \r, ...)
        // if apply_filters_to_entry(entry, &config.excludes, &config) {
        //     should_add |= FLAG_EXCLUDED;
        // }
        // if should_add != (FLAG_ELAPSED | FLAG_EXCLUDED)
        //     && !config.includes.is_empty()
        //     && apply_filters_to_entry(entry, &config.includes, &config)
        // {
        //     should_add |= FLAG_INCLUDED;
        // }
        // TODO: selector
        // if !feeds.get(&feed_id).unwrap().selector.is_empty() && !entry.links.is_empty() {
        //     let rt = tokio::runtime::Runtime::new().unwrap();
        //     match rt.block_on(get_link_data(
        //         &client,
        //         &entry.links[0].href,
        //         &feeds.get(&feed_id).unwrap().selector,
        //     )) {
        //         Ok(_) => todo!(),
        //         Err(_) => todo!(),
        //     };
        // }
        should_add
    });
    // apply filters
    // TODO: Generate feed file
}

/// Main processing entry point
pub(crate) async fn start(app: &App) {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(app.timeout as u64))
        .user_agent("frust/0.1.0")
        .build()
        .expect("Failed to build HTTP client");

    let now = *START_TIME.get().expect("START_TIME not initialized");

    // Create a flat list of feeds to process across all groups
    let mut feeds_to_process = Vec::new();
    for group in app.groups.values() {
        for (feed_id, feed) in &group.feeds {
            feeds_to_process.push((*feed_id, feed.clone()));
        }
    }

    tracing::info!(
        "Starting processing {} feeds with {} workers",
        feeds_to_process.len(),
        app.workers
    );

    let bodies = stream::iter(feeds_to_process)
        .map(|(feed_id, feed)| {
            let client = client.clone();
            let filters = &app.filters;
            let min_refresh = app.min_refresh_time;

            async move {
                // 1. Smart Polling check (Interval)
                if !is_refresh_required(feed.last_check, now, min_refresh) {
                    return Ok(());
                }

                // 2. HTTP Request with Conditional Headers
                let mut req = client.get(&feed.url);
                if let Some(ref etag) = feed.last_etag {
                    req = req.header(header::IF_NONE_MATCH, etag.to_rfc2822());
                }
                if let Some(last_mod) = feed.last_modified {
                    req = req.header(header::IF_MODIFIED_SINCE, last_mod.to_rfc2822());
                }

                let response = req.send().await?;

                // 3. Handle 304 Not Modified
                if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                    tracing::info!("Feed {} not modified (304)", feed.title);
                    return Ok(());
                }

                // 4. Extract new Cache Metadata
                let new_etag = response
                    .headers()
                    .get(header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());

                let new_last_mod = response
                    .headers()
                    .get(header::LAST_MODIFIED)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| DateTime::parse_from_rfc2822(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                // 5. Parse Content
                let bytes = response.bytes().await?;
                let mut fetched_feed = parser::Builder::new()
                    .sanitize_content(true)
                    .build()
                    .parse(bytes.as_ref())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

                // 6. Apply Filters and Retention
                apply_filters_and_retention(
                    &mut fetched_feed,
                    &feed,
                    filters,
                    &client,
                    feed.selector.clone(),
                )
                .await;

                // 7. Save output
                save_feed_to_disk(&fetched_feed, &feed.output).await?;

                // TODO: Here you should persist new_etag, new_last_mod and 'now' as last_check
                // back to your database/config state.

                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        })
        .buffer_unordered(app.workers);

    bodies
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::error!("Worker error: {}", e);
            }
        })
        .await;
}

/// Adjust entry content based on the configured ContentMode
async fn apply_content_mode(
    entry: &mut feed_rs::model::Entry,
    mode: &ContentMode,
    client: &reqwest::Client,
    selector_str: &Option<String>,
) {
    let converter = HtmlToMarkdown::new();

    if *mode == ContentMode::LinksOnly || *mode == ContentMode::No {
        entry.content = None;
        entry.summary = None;
    }
    if *mode == ContentMode::Default {
        // Keep everything as is (Title + Summary + Content)
        // Convert existing content to Markdown to save space if needed
        if let Some(content) = &mut entry.content {
            if let Some(body) = &content.body {
                content.body = Some(converter.convert(body).unwrap_or(body.clone()));
            }
        }
    }
    if *mode == ContentMode::Force {
        // Use summary as content and convert to MD
        if let Some(summary) = &entry.summary {
            // entry.content = Some(feed_rs::model::Content {
            //     body: Some(
            //         converter
            //             .convert(&summary.content)
            //             .unwrap_or(summary.content.clone()),
            //     ),
            //     content_type: mime::TEXT_MARKDOWN,
            //     length: None,
            //     src: None,
            // });
            entry.summary = None;
        }
    }
    if *mode == ContentMode::Force {
        // 1. Get the article URL
        if let Some(link) = entry.links.first() {
            // 2. Download the full HTML page
            if let Ok(resp) = client.get(&link.href).send().await
                && let Ok(html_content) = resp.text().await
            {
                let document = Html::parse_document(&html_content);

                // 3. Use CSS selector to find the main article body
                let selector = selector_str.as_deref().unwrap_or("article, main, .content");
                if let Ok(sel) = Selector::parse(selector)
                    && let Some(element) = document.select(&sel).next()
                {
                    let inner_html = element.inner_html();
                    // 4. Convert targeted HTML to Markdown
                    // entry.content = Some(feed_rs::model::Content {
                    //     body: Some(converter.convert(&inner_html).unwrap_or(inner_html)),
                    //     content_type: mime::TEXT_MARKDOWN,
                    //     length: None,
                    //     src: None,
                    // });
                    entry.summary = None;
                }
            }
        }
    }
}

/// Filter entries based on retention policy and RegexSet filters
async fn apply_filters_and_retention(
    fetched_feed: &mut feed_rs::model::Feed,
    feed_config: &Feed,
    global_filters: &HashMap<u64, Filter>,
    client: &reqwest::Client,
    selector: Option<String>,
) {
    // 1. First, adjust content according to the mode
    for entry in &mut fetched_feed.entries {
        apply_content_mode(entry, &feed_config.content_mode, client, &selector).await;
    }

    // 2. Then, proceed with retention and filtering
    fetched_feed.entries.retain(|entry| {
        // A. Retention Check
        let entry_date = entry
            .updated
            .or(entry.published)
            .unwrap_or(*START_TIME.get().unwrap());
        if is_article_expired(entry_date, feed_config.retention) {
            return false;
        }

        // B. Filter Check
        // Inherited filters (Group + Feed) are already in feed_config.filters hashes
        for filter_id in &feed_config.filters {
            if let Some(filter) = global_filters.get(filter_id) {
                let mut is_match = false;

                // Check title scope
                if filter.filter_in_title
                    && let Some(title) = &entry.title
                    && check_text_match(&title.content, filter)
                {
                    is_match = true;
                }

                // Check summary scope
                if !is_match
                    && filter.filter_in_summary
                    && let Some(summary) = &entry.summary
                    && check_text_match(&summary.content, filter)
                {
                    is_match = true;
                }

                // Check content scope (skip if selector is set — content will be fetched
                // separately via scraping, so feed content is not the final text to filter on)
                if !is_match
                    && filter.filter_in_content
                    && feed_config.selector.is_none()
                    && let Some(content) = &entry.content
                    && let Some(body) = &content.body
                    && check_text_match(body, filter)
                {
                    is_match = true;
                }

                // Filter logic: 'keep' means only keep if it matches, otherwise exclude if it matches
                if filter.keep {
                    if !is_match {
                        return false;
                    } // Must match to be kept
                } else if is_match {
                    return false;
                } // Must not match to be kept
            }
        }
        true
    });
}

/// Logic to check if a string matches the filter (RegexSet or plain text)
fn check_text_match(text: &str, filter: &Filter) -> bool {
    if filter.is_regex {
        let matches = filter.regexes.matches(text);
        if filter.must_match_all {
            matches.len() == filter.regexes.len()
        } else {
            matches.matched_any()
        }
    } else {
        // Case insensitive search (if not specified otherwise)
        let (haystack, needles): (String, Vec<String>) = (
            text.to_lowercase(),
            filter
                .expressions
                .iter()
                .map(|e| e.to_lowercase())
                .collect(),
        );

        let match_count = needles.iter().filter(|&e| haystack.contains(e)).count();
        if filter.must_match_all {
            match_count == needles.len()
        } else {
            match_count > 0
        }
    }
}

/// Save the filtered feed to the specified output file
async fn save_feed_to_disk(feed: &feed_rs::model::Feed, path: &str) -> Result<(), std::io::Error> {
    // Note: To save as XML, I'll need to convert feed_rs model back to RSS/Atom
    // using crates like 'rss' or 'atom_syndication'.
    // For now, we trace the action.
    tracing::debug!(
        "Writing {} filtered entries to {}",
        feed.entries.len(),
        path
    );

    // TODO: Implementation placeholder for serialization logic
    Ok(())
}

#[cfg(test)]
mod tests {}
