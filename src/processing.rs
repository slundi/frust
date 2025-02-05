use std::collections::HashMap;

use chrono::prelude::*;
use feed_rs::parser;
use futures::{stream, StreamExt};
use reqwest::{Client, ClientBuilder};

use crate::model::{App, Feed, Filter};

fn is_time_elapsed(current_time: &DateTime<Utc>, time: DateTime<Utc>, delay: i64) -> bool {
    time.signed_duration_since(current_time).num_seconds() >= delay
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
        let content = if filter.is_case_sensitive {
            text
        } else {
            text.to_lowercase()
        };
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
        if filter.filter_in_title {
            if let Some(value) = &entry.title {
                if text_is_found(value.content.clone(), *filter_id, filters) {
                    return true;
                }
            }
        }
        if filter.filter_in_summary {
            if let Some(value) = &entry.summary {
                if text_is_found(value.content.clone(), *filter_id, filters) {
                    return true;
                }
            }
        }
        if filter.filter_in_content {
            if let Some(value) = &entry.content {
                if text_is_found(
                    value.body.clone().unwrap_or(String::with_capacity(0)),
                    *filter_id,
                    filters,
                ) {
                    return true;
                }
            }
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
        if let Some(date) = entry.updated {
            if is_time_elapsed(
                *crate::NOW,
                date,
                feeds.get(&feed_id).unwrap().config.article_keep_time * 86400,
            ) {
                should_add = false;
            }
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
        if !feeds.get(&feed_id).unwrap().selector.is_empty() && !entry.links.is_empty() {
            let rt = tokio::runtime::Runtime::new().unwrap();
            match rt.block_on(get_link_data(
                &client,
                &entry.links[0].href,
                &feeds.get(&feed_id).unwrap().selector,
            )) {
                Ok(_) => todo!(),
                Err(_) => todo!(),
            };
        }
        should_add
    });
    // apply filters
    // TODO: Generate feed file
}

pub(crate) async fn start(app: &App) {
    let client = Client::new();

    tracing::info!("Using {} workers", app.workers);
    tracing::info!("Feeds {}", app.feeds.len());
    let bodies = stream::iter(app.feeds.clone())
        .map(|feed| {
            let client = &client;
            tracing::info!("Processing feed {}", feed.1.title);
            async move {
                match client.get(&feed.1.url).send().await {
                    //perform the HTTP query
                    Ok(resp) => {
                        tracing::info!("Feed downloaded from: {}", feed.1.url);
                        //read the response
                        if let Some(result) = get_response_feed(resp, &feed.1.url).await {
                            tracing::info!("result: {:?}", result);
                            //read feed data
                            let stored = if std::path::Path::new(&feed.1.output).is_file() {
                                Some(
                                    feed_rs::parser::parse(
                                        std::fs::read_to_string(&feed.1.output).unwrap().as_bytes(),
                                    )
                                    .unwrap(),
                                )
                            } else {
                                None
                            };
                            add_new_articles(feed.0, stored, result, &app.feeds).await;
                        }
                    }
                    Err(e) => tracing::error!("Unable to get feed from: {}     {}", &feed.1.url, e),
                }
            }
        })
        .buffer_unordered(app.workers);
    bodies
        .for_each(|b| async move {
            tracing::info!("ok: {:?}", b);
            // match b {
            //     // Ok(b) => tracing::info!("Got {} bytes", b.len()),
            //     Ok(()) => tracing::info!("ok"),
            //     Err(e) => tracing::error!("Got an error: {}", e),
            // }
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time() {
        let now = chrono::offset::Utc::now();
        assert!(
            is_time_elapsed(&now, now, 0),
            "1/3 It is not exactly the same date"
        );
        let t = now
            .checked_add_signed(chrono::Duration::milliseconds(10500))
            .unwrap();
        assert!(
            is_time_elapsed(&now, t, 10),
            "2/3 Date 10.5s after the feed date with a delay of 10s"
        );
        assert!(
            !is_time_elapsed(&now, t, 20),
            "3/3 Date 10.5s after the feed date with a delay of 20s"
        );
    }
}
