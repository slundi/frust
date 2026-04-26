use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use chrono::prelude::*;
use feed_rs::parser;
use futures::{StreamExt, stream};
use htmd::HtmlToMarkdown;
use mediatype::MediaTypeBuf;
use reqwest::{Client, header};
use scraper::{Html, Selector};
use twox_hash::XxHash3_64;

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
    workers: usize,
    output_dir: &Path,
) {
    let feed_config = feeds.get(&feed_id).unwrap();
    let client = Client::new();
    let mut rf = retrieved_feed;
    if let Some(ff) = file_feed {
        merge_feeds_by_id(&mut rf, ff.entries);
    }

    // 1. Drop entries that exceed the retention window
    rf.entries.retain(|entry| {
        !is_refresh_required(
            entry.updated,
            *START_TIME.get().unwrap(),
            feed_config.retention as i64 * 86400,
        )
    });

    // 2. In Force mode, replace the feed's brief content with the full article body.
    //    Each entry's first link is fetched, the configured CSS selector extracts the
    //    article element, and the result is converted to Markdown.
    if feed_config.content_mode == ContentMode::Force {
        let selector = feed_config
            .selector
            .as_deref()
            .unwrap_or("article, main, .content")
            .to_string();

        // Collect (index, url) pairs so the async tasks own their data
        let indexed_urls: Vec<(usize, String)> = rf
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| entry.links.first().map(|link| (i, link.href.clone())))
            .collect();

        // Fetch all article pages concurrently, bounded by `workers`
        let fetched: Vec<(usize, String)> = stream::iter(indexed_urls)
            .map(|(i, url)| {
                let client = client.clone();
                let selector = selector.clone();
                async move {
                    match get_link_data(&client, &url, &selector).await {
                        Ok(html) if !html.is_empty() => (i, html),
                        Ok(_) => {
                            tracing::warn!("No content found with selector at {}", url);
                            (i, String::new())
                        }
                        Err(e) => {
                            tracing::error!("Failed to fetch article {}: {}", url, e);
                            (i, String::new())
                        }
                    }
                }
            })
            .buffer_unordered(workers)
            .collect()
            .await;

        // Convert HTML → Markdown and update entries (sequential: HtmlToMarkdown is not Send)
        let media_dir = output_dir.join("media");
        let converter = HtmlToMarkdown::new();
        for (i, mut html) in fetched {
            if html.is_empty() {
                continue;
            }
            // Rewrite inline <img> src to local paths before MD conversion
            if feed_config.media {
                html =
                    rewrite_inline_images(&client, &html, &media_dir, feed_config.media_max_size)
                        .await;
            }
            let markdown = converter.convert(&html).unwrap_or(html);
            if let Some(entry) = rf.entries.get_mut(i) {
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
                entry.summary = None;
            }
        }
    }

    // 3. Download enclosures (audio, video, images declared in <enclosure> / media:content)
    if feed_config.media {
        let media_dir = output_dir.join("media");
        if let Err(e) = tokio::fs::create_dir_all(&media_dir).await {
            tracing::error!("Cannot create media directory: {}", e);
        } else {
            let enclosure_urls: Vec<String> = rf
                .entries
                .iter()
                .flat_map(|e| e.media.iter())
                .flat_map(|m| m.content.iter())
                .filter_map(|c| c.url.as_ref().map(|u| u.to_string()))
                .collect();

            stream::iter(enclosure_urls)
                .map(|url| {
                    let client = client.clone();
                    let media_dir = media_dir.clone();
                    let max_size = feed_config.media_max_size;
                    async move {
                        download_asset(&client, &url, &media_dir, max_size).await;
                    }
                })
                .buffer_unordered(workers)
                .for_each(|_| async {})
                .await;
        }
    }

    // TODO: apply filters
    // TODO: generate output
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

/// Map a MIME content-type string to a file extension.
fn mime_to_ext(content_type: &str) -> &'static str {
    match content_type.split(';').next().unwrap_or("").trim() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/avif" => "avif",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/ogg" => "ogg",
        "audio/flac" => "flac",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/aac" => "aac",
        "audio/mp4" => "m4a",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/ogg" => "ogv",
        _ => "bin",
    }
}

/// Try to extract a file extension from a URL path (ignores query string).
fn ext_from_url(url: &str) -> Option<&str> {
    let path = url.split('?').next()?;
    let filename = path.rsplit('/').next()?;
    if !filename.contains('.') {
        return None;
    }
    let ext = filename.rsplit('.').next()?;
    if ext.is_empty() || ext.len() > 5 {
        None
    } else {
        Some(ext)
    }
}

/// Download a single asset, deduplicate by XXH3 hash, and write to `media_dir/<hash>.<ext>`.
/// Returns the local path on success, `None` if skipped (size limit) or on error.
async fn download_asset(
    client: &Client,
    url: &str,
    media_dir: &Path,
    max_size: u64,
) -> Option<PathBuf> {
    let resp = client.get(url).send().await.ok()?;

    // Reject early based on Content-Length if available and a limit is set
    if max_size > 0 {
        if let Some(len) = resp.content_length() {
            if len > max_size {
                tracing::warn!(
                    "Skipping asset (declared {} bytes > limit {} bytes): {}",
                    len,
                    max_size,
                    url
                );
                return None;
            }
        }
    }

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let bytes = resp.bytes().await.ok()?;

    // Reject after download if actual size exceeds limit (Content-Length may be absent)
    if max_size > 0 && bytes.len() as u64 > max_size {
        tracing::warn!(
            "Skipping asset (actual {} bytes > limit {} bytes): {}",
            bytes.len(),
            max_size,
            url
        );
        return None;
    }

    let hash = XxHash3_64::oneshot(&bytes);
    let ext = ext_from_url(url).unwrap_or_else(|| mime_to_ext(&content_type));
    let filename = format!("{:016x}.{}", hash, ext);
    let path = media_dir.join(&filename);

    // Skip write if already on disk (same hash = same content)
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        if let Err(e) = tokio::fs::write(&path, &bytes).await {
            tracing::error!("Cannot write asset {}: {}", path.display(), e);
            return None;
        }
    }

    Some(path)
}

/// Find all external `<img src="...">` in an HTML fragment, download them, and rewrite
/// their `src` to the local `media/<hash>.<ext>` path. Returns the rewritten HTML.
async fn rewrite_inline_images(
    client: &Client,
    html: &str,
    media_dir: &Path,
    max_size: u64,
) -> String {
    if let Err(e) = tokio::fs::create_dir_all(media_dir).await {
        tracing::error!("Cannot create media directory: {}", e);
        return html.to_string();
    }

    let document = Html::parse_fragment(html);
    let img_sel = Selector::parse("img").unwrap();

    // Collect unique external image URLs (HashSet deduplicates)
    let srcs: HashSet<String> = document
        .select(&img_sel)
        .filter_map(|img| img.value().attr("src"))
        .filter(|src| src.starts_with("http://") || src.starts_with("https://"))
        .map(|s| s.to_string())
        .collect();

    let mut result = html.to_string();
    for src in srcs {
        if let Some(path) = download_asset(client, &src, media_dir, max_size).await {
            let filename = path.file_name().unwrap().to_string_lossy();
            let local = format!("media/{}", filename);
            // Replace both single and double-quoted attribute values
            result = result.replace(&format!("src=\"{}\"", src), &format!("src=\"{}\"", local));
            result = result.replace(&format!("src='{}'", src), &format!("src='{}'", local));
        }
    }
    result
}

#[cfg(test)]
mod tests {}
