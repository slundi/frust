use std::collections::HashMap;

use chrono::Utc;
use feed_rs::parser;
use futures::{stream, StreamExt};
use reqwest::Client;
use xxhash_rust::xxh3::xxh3_64;

use crate::{
    model::{ArticleRecord, SCOPE_BODY, SCOPE_SUMMARY, SCOPE_TITLE},
    CONFIG, DB,
};

/// Get upgradable feeds: when the delay between the last updated time and now is elapsed
async fn get_upgradable_feeds() -> HashMap<u64, crate::model::Feed> {
    let config = CONFIG.read().await;
    let db = DB.read().await;
    let now = Utc::now().timestamp();
    // filter feeds that does not need to be updated with the min_refresh_time
    let mut result: HashMap<u64, crate::model::Feed> = HashMap::with_capacity(config.feeds.len());
    for f in &config.feeds {
        if let Some(feed) = db.0.get(f.0) {
            if !feed.is_delay_elapsed(now, f.1.config.min_refresh_time) {
                continue;
            }
        }
        result.insert(*f.0, f.1.clone());
    }
    result
}

async fn get_response_feed(
    response: reqwest::Response,
    url: &String,
) -> Option<(u64, feed_rs::model::Feed)> {
    match response.bytes().await {
        // read the response
        Ok(content) => {
            let hash = xxh3_64(content.as_ref());
            match parser::parse(content.as_ref()) {
                // load feed data
                Ok(feed) => return Some((hash, feed)),
                Err(e) => println!("Unable to parse feed from: {}     {}", url, e),
            }
        }
        Err(e) => println!("Unable to read response from feed: {}     {}", url, e),
    };
    None
}

fn text_is_found(
    text: String,
    filter_id: u64,
    config: &crate::model::AppConfig,
) -> bool {
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
    let sentence_found = if filter.sentences.is_empty() { true } else {
        // if case insensitive, sentences are in lower case (loaded in [config.rs](config.rs))
        let content = if filter.is_case_sensitive {text} else {text.to_lowercase()};
        let mut count_found = 0usize;
        for exp in &filter.sentences {
            if content.contains(exp) {
                count_found+=1;
            }
        }
        (filter.must_match_all && filter.sentences.len() == count_found)
            || (!filter.must_match_all && count_found > 0)
    };
    regex_found && sentence_found
}

async fn add_new_articles(feed_id: u64, feed: feed_rs::model::Feed) {
    // TODO: process retrieved data:
    // - If applicable, retrieve articles (multiple per source) and its assets if applicable
    let mut db = DB.write().await;
    let config = CONFIG.read().await;
    for f in feed.entries {
        // Check if articles are in the data file or not
        let hash = xxh3_64(f.id.as_bytes());
        let mut fr = db.0.get(&feed_id).unwrap();
        if fr.articles.contains_key(&hash) {
            continue;
        }
        //retrieve data for DB
        let mut date = Utc::now().timestamp();
        if let Some(d) = f.updated {
            date = d.timestamp();
        }
        if let Some(d) = f.published {
            date = d.timestamp();
        }
        // TODO: Apply filters (do not match content if xpath is specified)
        // TODO: handle blanks (\n, \r, ...)
        let mut flags = crate::model::FLAG_ARTICLE_IGNORED;
        for filter_id in &config.includes {
            let filter = config.filters.get(filter_id).unwrap();
            if (filter.scopes & SCOPE_TITLE) == SCOPE_TITLE {
                if let Some(value) = &f.title {
                    if text_is_found(value.content.clone(), *filter_id, &config) {
                        flags = 0;
                        break;
                    }
                }
            }
            if (filter.scopes & SCOPE_SUMMARY) == SCOPE_SUMMARY {
                if let Some(value) = &f.summary {
                    if text_is_found(value.content.clone(), *filter_id, &config) {
                        flags = 0;
                        break;
                    }
                }
            }
            if (filter.scopes & SCOPE_BODY) == SCOPE_BODY {
                if let Some(value) = &f.content {
                    if text_is_found(value.body.clone().unwrap_or(String::with_capacity(0)), *filter_id, &config) {
                        flags = 0;
                        break;
                    }
                }
            }
        }
        // TODO: Generate feed file
        // TODO: insert into "DB"
        fr.articles.insert(
            hash,
            ArticleRecord {
                date,
                flags,
                slug: hash.to_string(),
            },
        );
    }
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
                            // if same hash from last update, do not process
                            let mut updated = true;
                            for fr in (DB.read().await).0.values() {
                                if fr.hash == result.0 {
                                    print!("No new article in: {}", &feed.1.url);
                                    updated = true;
                                    break;
                                }
                            }
                            if updated {
                                add_new_articles(feed.0, result.1).await;
                            }
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
