use std::collections::HashMap;

use chrono::prelude::*;
use feed_rs::parser;
use futures::{stream, StreamExt};
use reqwest::Client;

use crate::{
    model::{AppConfig, SCOPE_BODY, SCOPE_SUMMARY, SCOPE_TITLE},
    CONFIG,
};

fn is_time_elapsed(
    current_time: DateTime<Utc>,
    time: DateTime<Utc>,
    delay: i64,
) -> bool {
    time.signed_duration_since(current_time).num_seconds() >= delay
}

/// Get upgradable feeds: when the delay between the last updated time and now is elapsed
async fn get_upgradable_feeds() -> HashMap<u64, crate::model::Feed> {
    let config = CONFIG.read().await;
    // TODO: get date on the feed file
    // filter feeds that does not need to be updated with the min_refresh_time
    let mut result: HashMap<u64, crate::model::Feed> = HashMap::with_capacity(config.feeds.len());
    for f in &config.feeds {
        let mut feed = f.1.clone();
        // build output file path
        feed.output_file.push_str(&config.output);
        feed.output_file.push(std::path::MAIN_SEPARATOR);
        feed.output_file.push_str(&f.1.slug);
        feed.output_file.push_str(".json");
        // check if the file exists and if we can get the modification date
        if std::path::Path::new(&f.1.output_file).is_file() {
            if let Ok(date) = std::fs::metadata(&f.1.output_file)
                .expect("Cannot get feed metadata")
                .modified()
            {
                let local: DateTime<Local> = date.into();
                let dt = Utc.from_local_datetime(&local.naive_local()).single().unwrap();
                if !is_time_elapsed(*crate::NOW, dt, f.1.config.min_refresh_time) {
                    continue;
                }
            }
        }
        result.insert(*f.0, feed);
    }
    result
}

async fn get_response_feed(
    response: reqwest::Response,
    url: &String,
) -> Option<feed_rs::model::Feed> {
    match response.bytes().await {
        // read the response
        Ok(content) => {
            match parser::parse(content.as_ref()) {
                // load feed data
                Ok(feed) => return Some(feed),
                Err(e) => println!("Unable to parse feed from: {}     {}", url, e),
            }
        }
        Err(e) => println!("Unable to read response from feed: {}     {}", url, e),
    };
    None
}

fn text_is_found(text: String, filter_id: u64, config: &crate::model::AppConfig) -> bool {
    let filter = config.filters.get(&filter_id).unwrap();
    // regex search
    let regex_found = if filter.regexes.is_empty() {
        true
    } else {
        let matches = filter.regexes.matches(&text);
        (filter.must_match_all && filter.regexes.len() == matches.len())
            || (!filter.must_match_all && matches.matched_any())
    };
    // sentence/word search
    let sentence_found = if filter.sentences.is_empty() {
        true
    } else {
        // if case insensitive, sentences are in lower case (loaded in [config.rs](config.rs))
        let content = if filter.is_case_sensitive {
            text
        } else {
            text.to_lowercase()
        };
        let mut count_found = 0usize;
        for exp in &filter.sentences {
            if content.contains(exp) {
                count_found += 1;
            }
        }
        (filter.must_match_all && filter.sentences.len() == count_found)
            || (!filter.must_match_all && count_found > 0)
    };
    regex_found && sentence_found
}

/// Apply filters to article. It returns `true` when a filter has matched or if the filter list is empty.
fn apply_filters_to_entry(
    entry: &feed_rs::model::Entry,
    filters: &Vec<u64>,
    config: &AppConfig,
) -> bool {
    for filter_id in filters {
        let filter = config.filters.get(filter_id).unwrap();
        if (filter.scopes & SCOPE_TITLE) == SCOPE_TITLE {
            if let Some(value) = &entry.title {
                if text_is_found(value.content.clone(), *filter_id, config) {
                    return true;
                }
            }
        }
        if (filter.scopes & SCOPE_SUMMARY) == SCOPE_SUMMARY {
            if let Some(value) = &entry.summary {
                if text_is_found(value.content.clone(), *filter_id, config) {
                    return true;
                }
            }
        }
        if (filter.scopes & SCOPE_BODY) == SCOPE_BODY {
            if let Some(value) = &entry.content {
                if text_is_found(
                    value.body.clone().unwrap_or(String::with_capacity(0)),
                    *filter_id,
                    config,
                ) {
                    return true;
                }
            }
        }
    }
    filters.is_empty()
}

async fn add_new_articles(
    feed_id: u64,
    file_feed: Option<feed_rs::model::Feed>,
    retrieved_feed: feed_rs::model::Feed,
) {
    // TODO: process retrieved data:
    // - If applicable, retrieve articles (multiple per source) and its assets if applicable
    let config = CONFIG.read().await;
    let ff = if let Some(ff) = file_feed {
        ff.entries
    } else {
        Vec::with_capacity(0)
    };
    let mut rf = retrieved_feed.clone();
    rf.entries.retain(|entry| {
        let mut should_add = false;
        // check if entry should be kept (storage time)
        if let Some(date) = entry.updated {
            if is_time_elapsed(
                *crate::NOW,
                date,
                config.feeds.get(&feed_id).unwrap().config.article_keep_time * 86400,
            ) {}
        }
        // check if feed is present in the file and keep it if yes (already filtered)
        for f in ff.iter() {
            if entry.id == f.id {
                should_add = true;
                break;
            }
        }
        // Apply filters (do not match content if xpath is specified)
        // TODO: handle blanks (\n, \r, ...)
        if apply_filters_to_entry(entry, &config.excludes, &config) {
            should_add = false;
        }
        if !should_add
            && !config.includes.is_empty()
            && apply_filters_to_entry(entry, &config.includes, &config)
        {
            should_add = true;
        }
        // TODO: xpath
        should_add
    });
    // apply filters
    // TODO: Generate feed file
}

pub(crate) async fn start() {
    let client = Client::new();

    let _bodies = stream::iter(get_upgradable_feeds().await)
        .map(|feed| {
            let client = &client;
            async move {
                match client.get(&feed.1.url).send().await {
                    //perform the HTTP query
                    Ok(resp) => {
                        //read the response
                        if let Some(result) = get_response_feed(resp, &feed.1.url).await {
                            //read feed data
                            let stored = if std::path::Path::new(&feed.1.output_file).is_file() {
                                Some(
                                    feed_rs::parser::parse(
                                        std::fs::read_to_string(&feed.1.output_file)
                                            .unwrap()
                                            .as_bytes(),
                                    )
                                    .unwrap(),
                                )
                            } else {
                                None
                            };
                            add_new_articles(feed.0, stored, result).await;
                        }
                    }
                    Err(e) => println!("Unable to get feed from: {}     {}", &feed.1.url, e),
                }
            }
        })
        .buffer_unordered((CONFIG.read().await).workers);
    // bodies
    //     .for_each(|b| async {
    //         match b {
    //             Ok(b) => println!("Got {} bytes", b.len()),
    //             Err(e) => eprintln!("Got an error: {}", e),
    //         }
    //     })
    //     .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time() {
        let now = chrono::offset::Utc::now();
        assert!(
            is_time_elapsed(now, now, 0),
            "1/3 It is not exactly the same date"
        );
        let t = now
            .checked_add_signed(chrono::Duration::milliseconds(10500))
            .unwrap();
        assert!(
            is_time_elapsed(now, t, 10),
            "2/3 Date 10.5s after the feed date with a delay of 10s"
        );
        assert!(
            !is_time_elapsed(now, t, 20),
            "3/3 Date 10.5s after the feed date with a delay of 20s"
        );
    }
}
