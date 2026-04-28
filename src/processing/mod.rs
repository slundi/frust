use std::{
    cmp::Reverse,
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use feed_rs::parser;
use futures::{StreamExt, stream};
use reqwest::{Client, header};
use tracing::{debug, info};

use crate::{
    START_TIME,
    error::FrustError,
    export::{AtomExporter, EpubExporter, Exporter, JsonExporter, MarkdownExporter, RssExporter},
    model::{App, Article, ExportStrategy, FeedState},
    storage::Storage,
    utils::is_refresh_required,
};

pub(crate) mod content;
pub(crate) mod convert;
pub(crate) mod fetch;
pub(crate) mod filter;
pub(crate) mod media;

struct FeedResult {
    feed_id: u64,
    articles: Vec<Article>,
    state: FeedState,
}

/// Main processing entry point: fetches all feeds concurrently, applies
/// filters/retention, persists new articles, then exports per-group output files.
pub(crate) async fn start(app: &App) -> Result<(), FrustError> {
    debug!("Creating HTTP client");
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(app.timeout as u64))
        .user_agent("frust/0.1.0")
        .build()?;

    let now = *START_TIME
        .get()
        .ok_or(FrustError::NotInitialized("START_TIME"))?;
    let now_ts = now.timestamp();

    let articles_path = format!("{}/articles.redb", app.output);
    let states_path = format!("{}/states.redb", app.output);
    let storage = Storage::new(&articles_path, &states_path)?;

    let existing_ids: Arc<HashSet<u64>> = Arc::new(match storage.load_article_ids() {
        Ok(ids) => {
            tracing::info!("Loaded {} known article IDs", ids.len());
            ids
        }
        Err(e) => {
            tracing::warn!(
                "Could not load article IDs, proceeding without dedup: {}",
                e
            );
            HashSet::new()
        }
    });

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

    // Phase 1: fetch → filter → convert to Articles (runs concurrently)
    let results: Vec<FeedResult> = stream::iter(feeds_to_process)
        .map(|(feed_id, feed)| {
            let client = client.clone();
            let min_refresh = app.min_refresh_time;
            let existing_ids = Arc::clone(&existing_ids);

            async move {
                if !is_refresh_required(feed.last_check, now, min_refresh) {
                    info!("Refresh not needed for {}", feed.title);
                    return Ok(None);
                }

                let mut req = client.get(&feed.url);
                if let Some(ref etag) = feed.last_etag {
                    req = req.header(header::IF_NONE_MATCH, etag.to_rfc2822());
                }
                if let Some(last_mod) = feed.last_modified {
                    req = req.header(header::IF_MODIFIED_SINCE, last_mod.to_rfc2822());
                }

                debug!("Sending request for {} to {}", feed.title, feed.url);
                let response = req.send().await?;

                if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                    tracing::info!("Feed '{}' not modified (304)", feed.title);
                    return Ok(None);
                }

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

                let bytes = response.bytes().await?;
                let mut fetched_feed = parser::Builder::new()
                    .sanitize_content(true)
                    .build()
                    .parse(bytes.as_ref())
                    .map_err(|e| FrustError::FeedParse(e.to_string()))?;

                filter::apply_filters_and_retention(
                    &mut fetched_feed,
                    &feed,
                    filters,
                    &client,
                    feed.selector.clone(),
                    &existing_ids,
                )
                .await;

                let articles: Vec<Article> = fetched_feed
                    .entries
                    .iter()
                    .map(|entry| convert::entry_to_article(entry, feed_id, now_ts))
                    .collect();

                tracing::info!(
                    "Feed '{}': {} new article(s) after filtering",
                    feed.title,
                    articles.len()
                );

                let state = FeedState {
                    last_etag: new_etag,
                    last_check_ts: Some(now_ts),
                    last_modified_ts: new_last_mod.map(|dt| dt.timestamp()),
                };

                Ok::<_, FrustError>(Some(FeedResult {
                    feed_id,
                    articles,
                    state,
                }))
            }
        })
        .buffer_unordered(app.workers)
        .filter_map(|res| async move {
            match res {
                Ok(Some(r)) => Some(r),
                Ok(None) => None,
                Err(e) => {
                    tracing::error!("Worker error: {}", e);
                    None
                }
            }
        })
        .collect()
        .await;

    // Phase 2: persist articles and feed states
    let all_articles: Vec<Article> = results.iter().flat_map(|r| r.articles.clone()).collect();
    let new_count = all_articles.len();
    if !all_articles.is_empty() {
        storage.upsert_articles(all_articles)?;
        tracing::info!("Persisted {} new article(s)", new_count);
    }

    for result in &results {
        if let Err(e) = storage.save_feed_state(result.feed_id, &result.state) {
            tracing::warn!(
                "Could not save feed state for feed {}: {}",
                result.feed_id,
                e
            );
        }
    }

    // Phase 3: export per-group output files
    run_group_exports(app, &storage)?;

    Ok(())
}

/// Pick an exporter based on the destination file extension.
/// Defaults to RSS for unknown or `.xml` extensions.
fn select_exporter(dest: &Path) -> Box<dyn Exporter> {
    match dest.extension().and_then(|e| e.to_str()) {
        Some("atom") => Box::new(AtomExporter),
        Some("json") => Box::new(JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }),
        Some("epub") => Box::new(EpubExporter),
        Some("md") => Box::new(MarkdownExporter {
            strategy: ExportStrategy::Monolithic,
        }),
        _ => Box::new(RssExporter),
    }
}

/// For each group, load its articles from storage and write the output file.
fn run_group_exports(app: &App, storage: &Storage) -> Result<(), FrustError> {
    for group in app.groups.values() {
        let mut articles: Vec<Article> = Vec::new();
        for feed_id in group.feeds.keys() {
            match storage.load_articles_for_feed(*feed_id) {
                Ok(mut feed_articles) => articles.append(&mut feed_articles),
                Err(e) => tracing::warn!(
                    "Could not load articles for feed {} in group '{}': {}",
                    feed_id,
                    group.slug,
                    e
                ),
            }
        }
        articles.sort_unstable_by_key(|a| Reverse(a.timestamp));

        if articles.is_empty() {
            tracing::debug!("Group '{}' has no articles, skipping export", group.slug);
            continue;
        }

        let dest = if Path::new(&group.output).is_absolute() {
            PathBuf::from(&group.output)
        } else {
            Path::new(&app.output).join(&group.output)
        };

        let exporter = select_exporter(&dest);
        let link = format!("/{}", group.slug);

        tracing::info!(
            "Exporting {} article(s) for group '{}' → {}",
            articles.len(),
            group.slug,
            dest.display()
        );

        if let Err(e) = exporter.generate(&articles, &group.title, &link, &dest) {
            tracing::error!("Export failed for group '{}': {}", group.slug, e);
        }
    }
    Ok(())
}
