use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
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
    model::{App, Article, Enrichment, ExportStrategy, Feed, FeedState, Group},
    storage::Storage,
    utils::is_refresh_required,
};

type StatesMap = Arc<HashMap<u64, FeedState>>;

/// Safety cap on a single feed response body. Real RSS/Atom feeds are rarely
/// more than a few hundred KB; anything past this is either misconfigured or
/// hostile. Reading unbounded would OOM the router-class targets this tool is
/// meant to run on.
const MAX_FEED_BYTES: u64 = 32 * 1024 * 1024; // 32 MiB

pub(crate) mod content;
pub(crate) mod convert;
pub(crate) mod filter;
pub(crate) mod media;

/// Append a chunk to `buf`, returning `false` if doing so would exceed `max`.
/// Extracted from `read_bounded_body` so the size-limit invariant is testable
/// without a live HTTP response.
fn append_within_cap(buf: &mut Vec<u8>, chunk: &[u8], max: u64) -> bool {
    if buf.len() as u64 + chunk.len() as u64 > max {
        return false;
    }
    buf.extend_from_slice(chunk);
    true
}

/// Read a response body into memory, aborting if it exceeds `max` bytes.
/// Rejects early via `Content-Length` when present; otherwise counts as it
/// streams. Returns `None` when the cap is hit — the caller should treat that
/// as "skip this feed but still update its state".
async fn read_bounded_body(
    mut response: reqwest::Response,
    max: u64,
) -> Result<Option<Vec<u8>>, FrustError> {
    if let Some(len) = response.content_length()
        && len > max
    {
        return Ok(None);
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if !append_within_cap(&mut buf, &chunk, max) {
            return Ok(None);
        }
    }
    Ok(Some(buf))
}

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

    let states: StatesMap = Arc::new(match storage.load_all_states() {
        Ok(s) => {
            tracing::info!("Loaded {} feed state(s) from storage", s.len());
            s
        }
        Err(e) => {
            tracing::warn!("Could not load feed states, cache headers disabled: {}", e);
            HashMap::new()
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
            let states = Arc::clone(&states);

            async move {
                let stored_state = states.get(&feed_id);

                let last_check = stored_state
                    .and_then(|s| s.last_check_ts)
                    .and_then(|ts| DateTime::from_timestamp(ts, 0));
                if !is_refresh_required(last_check, now, min_refresh) {
                    info!("Refresh not needed for {}", feed.title);
                    return Ok(None);
                }

                let mut req = client.get(&feed.url);
                if let Some(etag) = stored_state.and_then(|s| s.last_etag.as_deref()) {
                    req = req.header(header::IF_NONE_MATCH, etag);
                }
                if let Some(last_mod) = stored_state
                    .and_then(|s| s.last_modified_ts)
                    .and_then(|ts| DateTime::from_timestamp(ts, 0))
                {
                    req = req.header(header::IF_MODIFIED_SINCE, last_mod.to_rfc2822());
                }

                debug!("Sending request for {} to {}", feed.title, feed.url);
                let response = req.send().await?;
                let http_status = response.status().as_u16();

                if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                    tracing::info!("Feed '{}' not modified (304)", feed.title);
                    return Ok(Some(FeedResult {
                        feed_id,
                        articles: vec![],
                        state: FeedState {
                            last_etag: stored_state.and_then(|s| s.last_etag.clone()),
                            last_check_ts: Some(now_ts),
                            last_modified_ts: stored_state.and_then(|s| s.last_modified_ts),
                            last_http_status: Some(http_status),
                        },
                    }));
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

                let bytes = match read_bounded_body(response, MAX_FEED_BYTES).await? {
                    Some(b) => b,
                    None => {
                        tracing::warn!(
                            "Feed '{}' body exceeds {} bytes cap; skipping this refresh but recording state",
                            feed.title,
                            MAX_FEED_BYTES
                        );
                        return Ok(Some(FeedResult {
                            feed_id,
                            articles: vec![],
                            state: FeedState {
                                last_etag: new_etag,
                                last_check_ts: Some(now_ts),
                                last_modified_ts: new_last_mod.map(|dt| dt.timestamp()),
                                last_http_status: Some(http_status),
                            },
                        }));
                    }
                };
                let mut fetched_feed = parser::Builder::new()
                    .sanitize_content(true)
                    .build()
                    .parse(bytes.as_slice())
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
                    last_http_status: Some(http_status),
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

    // Phase 2: persist articles and feed states.
    // Save states first (only reads state, doesn't touch articles) so we can
    // then move-consume `results` into a single article vec without cloning.
    for result in &results {
        if let Err(e) = storage.save_feed_state(result.feed_id, &result.state) {
            tracing::warn!(
                "Could not save feed state for feed {}: {}",
                result.feed_id,
                e
            );
        }
    }

    let all_articles: Vec<Article> = results.into_iter().flat_map(|r| r.articles).collect();
    let new_count = all_articles.len();
    if !all_articles.is_empty() {
        storage.upsert_articles(all_articles)?;
        tracing::info!("Persisted {} new article(s)", new_count);
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

/// Build a per-feed enrichment map for a group (keyed by `feed_id`).
fn build_enrichment_map(group: &Group) -> HashMap<u64, Enrichment> {
    group
        .feeds
        .iter()
        .map(|(feed_id, feed)| (*feed_id, feed_to_enrichment(feed)))
        .collect()
}

fn feed_to_enrichment(feed: &Feed) -> Enrichment {
    Enrichment {
        feed_title: feed.title.clone(),
        feed_url: feed.url.clone(),
        feed_slug: feed.slug.clone(),
        feed_page_url: feed.page_url.clone(),
        prepend: feed.enrichment_prepend.clone(),
        append: feed.enrichment_append.clone(),
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
        let enrichments = build_enrichment_map(group);

        tracing::info!(
            "Exporting {} article(s) for group '{}' → {}",
            articles.len(),
            group.slug,
            dest.display()
        );

        if let Err(e) = exporter.generate(&articles, &group.title, &link, &dest, &enrichments) {
            tracing::error!("Export failed for group '{}': {}", group.slug, e);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::append_within_cap;

    #[test]
    fn test_append_within_cap_accepts_chunk_below_limit() {
        let mut buf = vec![0u8; 5];
        assert!(append_within_cap(&mut buf, &[1, 2, 3], 100));
        assert_eq!(buf.len(), 8);
    }

    #[test]
    fn test_append_within_cap_rejects_chunk_that_would_overflow() {
        let mut buf = vec![0u8; 90];
        // Chunk of 20 bytes would push us to 110 > cap of 100
        assert!(!append_within_cap(&mut buf, &vec![0u8; 20], 100));
        // Buffer is unchanged — no partial writes past the cap
        assert_eq!(buf.len(), 90);
    }

    #[test]
    fn test_append_within_cap_exact_boundary_is_accepted() {
        let mut buf = vec![0u8; 90];
        // Exactly reaching the cap must succeed (limit is inclusive)
        assert!(append_within_cap(&mut buf, &vec![0u8; 10], 100));
        assert_eq!(buf.len(), 100);
    }

    #[test]
    fn test_append_within_cap_one_over_boundary_is_rejected() {
        let mut buf = vec![0u8; 100];
        assert!(!append_within_cap(&mut buf, &[42], 100));
        assert_eq!(buf.len(), 100);
    }

    #[test]
    fn test_append_within_cap_empty_chunk_is_a_noop() {
        let mut buf = vec![0u8; 50];
        assert!(append_within_cap(&mut buf, &[], 100));
        assert_eq!(buf.len(), 50);
    }
}
