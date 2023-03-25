use std::collections::HashMap;

use chrono::Utc;
use feed_rs::parser;
use futures::{stream, StreamExt};
use reqwest::Client;

use crate::model::{AppConfig, Storage};

/// Get upgradable feeds: when the delay between the last updated time and now is elapsed
fn get_upgradable_feeds(
    feeds: &HashMap<u64, crate::model::Feed>,
    db: &Storage,
) -> HashMap<u64, crate::model::Feed> {
    let now = Utc::now().timestamp();
    // filter feeds that does not need to be updated with the min_refresh_time
    let mut result: HashMap<u64, crate::model::Feed> = HashMap::with_capacity(feeds.len());
    for f in feeds {
        if let Some(feed) = db.0.get(f.0) {
            if !feed.is_delay_elapsed(now, f.1.config.min_refresh_time) {
                continue;
            }
        }
        result.insert(*f.0, f.1.clone());
    }
    result
}

pub(crate) async fn start(config: &AppConfig, db: &Storage) {
    let client = Client::new();

    let _bodies = stream::iter(get_upgradable_feeds(&config.feeds, db))
        .map(|feed| {
            let client = &client;
            async move {
                match client.get(&feed.1.url).send().await {
                    //perform the HTTP query
                    Ok(resp) => {
                        match resp.bytes().await {
                            // read the response
                            Ok(content) => {
                                match parser::parse(content.as_ref()) {
                                    // load feed data
                                    Ok(_) => todo!(),
                                    Err(e) => println!(
                                        "Unable to parse feed from: {}     {}",
                                        &feed.1.url, e
                                    ),
                                }
                            }
                            Err(e) => println!(
                                "Unable to read response from feed: {}     {}",
                                &feed.1.url, e
                            ),
                        }
                    }
                    Err(e) => println!("Unable to get feed from: {}     {}", &feed.1.url, e),
                }
                // TODO: process retrieved data
            }
        })
        .buffer_unordered(config.workers);
    // bodies
    //     .for_each(|b| async {
    //         match b {
    //             Ok(b) => println!("Got {} bytes", b.len()),
    //             Err(e) => eprintln!("Got an error: {}", e),
    //         }
    //     })
    //     .await;
}
