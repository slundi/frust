use std::collections::HashMap;

use chrono::Utc;
use feed_rs::parser;
use futures::{stream, StreamExt};
use reqwest::Client;
use xxhash_rust::xxh3::xxh3_64;

use crate::{CONFIG, DB};

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

pub(crate) async fn start() {
    let client = Client::new();

    let _bodies = stream::iter(get_upgradable_feeds().await)
        .map(|feed| {
            let client = &client;
            async move {
                match client.get(&feed.1.url).send().await {
                    //perform the HTTP query
                    Ok(resp) => {
                        if let Some(result) = get_response_feed(resp, &feed.1.url).await {
                            // if same hash, do not process
                            // TODO: process retrieved data
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
