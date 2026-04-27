use std::sync::Arc;

use chrono::{DateTime, Utc};
use feed_rs::parser;
use futures::{StreamExt, stream};
use reqwest::{Client, header};

use crate::{
    START_TIME, error::FrustError, model::App, storage::Storage, utils::is_refresh_required,
};

pub(crate) mod content;
pub(crate) mod fetch;
pub(crate) mod filter;
pub(crate) mod media;

/// Main processing entry point: fetches all feeds concurrently and applies
/// filters, content enrichment, and output generation.
pub(crate) async fn start(app: &App) -> Result<(), FrustError> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(app.timeout as u64))
        .user_agent("frust/0.1.0")
        .build()?;

    let now = *START_TIME
        .get()
        .ok_or(FrustError::NotInitialized("START_TIME"))?;

    // Load existing article IDs from storage so we can skip already-seen entries.
    let existing_ids = Arc::new({
        let articles_path = format!("{}/articles.redb", app.output);
        let states_path = format!("{}/states.redb", app.output);
        match Storage::new(&articles_path, &states_path) {
            Ok(storage) => match storage.load_article_ids() {
                Ok(ids) => {
                    tracing::info!("Loaded {} known article IDs", ids.len());
                    ids
                }
                Err(e) => {
                    tracing::warn!(
                        "Could not load article IDs, proceeding without dedup: {}",
                        e
                    );
                    std::collections::HashSet::new()
                }
            },
            Err(e) => {
                tracing::warn!("Could not open storage, proceeding without dedup: {}", e);
                std::collections::HashSet::new()
            }
        }
    });

    // Flatten groups → feeds into a single work list
    let feeds_to_process: Vec<_> = app
        .groups
        .values()
        .flat_map(|group| group.feeds.iter().map(|(id, feed)| (*id, feed.clone())))
        .collect();

    tracing::info!(
        "Starting processing {} feeds with {} workers",
        feeds_to_process.len(),
        app.workers
    );

    let filters = &app.filters;

    stream::iter(feeds_to_process)
        .map(|(_feed_id, feed)| {
            let client = client.clone();
            let min_refresh = app.min_refresh_time;
            let existing_ids = Arc::clone(&existing_ids);

            async move {
                // 1. Smart polling: skip if the refresh interval has not elapsed
                if !is_refresh_required(feed.last_check, now, min_refresh) {
                    return Ok(());
                }

                // 2. HTTP request with conditional cache headers
                let mut req = client.get(&feed.url);
                if let Some(ref etag) = feed.last_etag {
                    req = req.header(header::IF_NONE_MATCH, etag.to_rfc2822());
                }
                if let Some(last_mod) = feed.last_modified {
                    req = req.header(header::IF_MODIFIED_SINCE, last_mod.to_rfc2822());
                }

                let response = req.send().await?;

                // 3. 304 Not Modified → nothing to do
                if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                    tracing::info!("Feed {} not modified (304)", feed.title);
                    return Ok(());
                }

                // 4. Extract cache metadata for persistence
                let _new_etag = response
                    .headers()
                    .get(header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());

                let _new_last_mod = response
                    .headers()
                    .get(header::LAST_MODIFIED)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| DateTime::parse_from_rfc2822(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                // 5. Parse feed content
                let bytes = response.bytes().await?;
                let mut fetched_feed = parser::Builder::new()
                    .sanitize_content(true)
                    .build()
                    .parse(bytes.as_ref())
                    .map_err(|e| FrustError::FeedParse(e.to_string()))?;

                // 6. Apply content mode, retention and filters
                filter::apply_filters_and_retention(
                    &mut fetched_feed,
                    &feed,
                    filters,
                    &client,
                    feed.selector.clone(),
                    &existing_ids,
                )
                .await;

                // 7. Persist to disk
                fetch::save_feed_to_disk(&fetched_feed, &feed.output).await?;

                // TODO: persist _new_etag, _new_last_mod and `now` as last_check to storage

                Ok::<(), FrustError>(())
            }
        })
        .buffer_unordered(app.workers)
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::error!("Worker error: {}", e);
            }
        })
        .await;
    Ok(())
}
