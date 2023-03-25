use std::collections::HashMap;

use chrono::Utc;
use feed_rs::parser;
use futures::{stream, StreamExt};
use reqwest::Client;
use xxhash_rust::xxh3::xxh3_64;

use crate::{model::{ArticleRecord, SCOPE_TITLE}, CONFIG, DB};

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
        for filter_id in &config.includes {
            let filter = config.filters.get(filter_id).unwrap();
            if filter.is_regex { // FIXME: use lazy_regex?
                for exp in &filter.expressions {
                    let re = regex::Regex::new(exp).unwrap();
                    if let Some(value) = &f.title {
                        if (filter.scopes & SCOPE_TITLE) == SCOPE_TITLE {}
                        re.is_match(&value.content);
                    }
                    if let Some(value) = &f.summary {
                        re.is_match(&value.content);
                    }
                    if let Some(value) = &f.content {
                        re.is_match(&value.body.clone().unwrap());
                    }
                }
            } else {

            }
        }
        // TODO: Generate feed file
        // TODO: insert into "DB"
        // fr.articles.insert(
        //     hash,
        //     ArticleRecord {
        //         date,
        //         flags: todo!(),
        //         slug: hash.to_string(),
        //     },
        // );
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
