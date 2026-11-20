use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
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
    processing::safe_http::SafeClient,
    storage::Storage,
    utils::is_refresh_required,
};

type StatesMap = Arc<HashMap<u64, FeedState>>;

/// Safety cap on a single feed response body. Real RSS/Atom feeds are rarely
/// more than a few hundred KB; anything past this is either misconfigured or
/// hostile. Reading unbounded would OOM the router-class targets this tool is
/// meant to run on.
const MAX_FEED_BYTES: u64 = 32 * 1024 * 1024; // 32 MiB

/// Upper bound on a whole run. Every request already has its own timeout,
/// but a stuck run would hold the exclusive redb lock and make every
/// following cron run fail.
pub(crate) const MAX_RUN_DURATION: Duration = Duration::from_secs(60 * 60);

pub(crate) mod content;
pub(crate) mod convert;
pub(crate) mod filter;
pub(crate) mod media;
pub(crate) mod safe_http;

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

/// Every feed of every group, once per feed id (hash of the URL).
///
/// A feed listed in several groups is fetched once, with the settings of the
/// first group by slug (deterministic); fetching it once per group ran the
/// same download, state save and insert in parallel. Its articles are stored
/// once and exported in each of its groups.
fn unique_feeds(app: &App) -> Vec<(u64, Feed)> {
    let mut groups: Vec<&Group> = app.groups.values().collect();
    groups.sort_by(|left, right| left.slug.cmp(&right.slug));
    let mut feeds: HashMap<u64, Feed> = HashMap::new();
    for group in groups {
        for (feed_id, feed) in &group.feeds {
            feeds.entry(*feed_id).or_insert_with(|| feed.clone());
        }
    }
    feeds.into_iter().collect()
}

/// Run `work`, aborting it (and dropping what it holds, such as the storage
/// lock) once `deadline` has passed.
pub(crate) async fn with_deadline<T>(
    deadline: Duration,
    work: impl Future<Output = Result<T, FrustError>>,
) -> Result<T, FrustError> {
    tokio::time::timeout(deadline, work)
        .await
        .map_err(|_| FrustError::RunTimeout(deadline.as_secs()))?
}

/// Main processing entry point: fetches all feeds concurrently, applies
/// filters/retention, persists new articles, then exports per-group output files.
pub(crate) async fn start(app: &App) -> Result<(), FrustError> {
    debug!("Creating HTTP clients");
    let timeout = Duration::from_secs(app.timeout as u64);
    let client = Client::builder()
        .timeout(timeout)
        .user_agent(concat!("frust/", env!("CARGO_PKG_VERSION")))
        .build()?;
    // Feed URLs come from the config and are trusted; page URLs come from the
    // feeds themselves and go through the SSRF-guarded client.
    let scrape_client = SafeClient::new(timeout)?;

    let now = *START_TIME
        .get()
        .ok_or(FrustError::NotInitialized("START_TIME"))?;
    let now_ts = now.timestamp();

    let storage = Storage::open_in(&app.data_dir)?;

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

    let feeds_to_process = unique_feeds(app);

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
            let scrape_client = scrape_client.clone();
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
                    feed_id,
                    &feed,
                    filters,
                    &scrape_client,
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
        .buffer_unordered(app.workers.get())
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
    persist_results(&storage, results)?;

    // Phase 3: export per-group output files
    run_group_exports(app, &storage)?;

    Ok(())
}

/// Persist fetched articles, then the feed states. Returns the number of
/// articles written.
///
/// Order matters: a state carries the new ETag / Last-Modified. Committing it
/// before the articles would make the next run receive `304 Not Modified` and
/// lose this fetch's articles for good if the article write failed. So if the
/// article commit fails, no state is saved and the next run re-fetches.
/// The reverse failure (articles saved, states not) is harmless: the next run
/// re-fetches with the old validators and dedups against the stored ids.
fn persist_results(storage: &Storage, results: Vec<FeedResult>) -> Result<usize, FrustError> {
    let mut states: Vec<(u64, FeedState)> = Vec::with_capacity(results.len());
    let mut all_articles: Vec<Article> = Vec::new();
    for result in results {
        states.push((result.feed_id, result.state));
        all_articles.extend(result.articles);
    }

    let new_count = all_articles.len();
    if !all_articles.is_empty() {
        storage.upsert_articles(all_articles)?;
        tracing::info!("Persisted {} new article(s)", new_count);
    }

    if !states.is_empty()
        && let Err(e) = storage.save_feed_states(&states)
    {
        tracing::warn!("Could not save {} feed state(s): {}", states.len(), e);
    }

    Ok(new_count)
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
///
/// A failing group does not stop the others, but the run fails at the end so
/// cron/monitoring sees it.
fn run_group_exports(app: &App, storage: &Storage) -> Result<(), FrustError> {
    let mut failed_groups = 0;
    for group in app.groups.values() {
        let mut articles: Vec<Article> = Vec::new();
        let mut load_failed = false;
        for feed_id in group.feeds.keys() {
            match storage.load_articles_for_feed(*feed_id) {
                Ok(mut feed_articles) => articles.append(&mut feed_articles),
                Err(e) => {
                    load_failed = true;
                    tracing::error!(
                        "Could not load articles for feed {} in group '{}': {}",
                        feed_id,
                        group.slug,
                        e
                    )
                }
            }
        }
        if load_failed {
            failed_groups += 1;
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
            if !load_failed {
                failed_groups += 1;
            }
        }
    }
    if failed_groups > 0 {
        return Err(FrustError::Export(format!(
            "{failed_groups} of {} group(s) failed to export",
            app.groups.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        FeedResult, append_within_cap, persist_results, run_group_exports, unique_feeds,
        with_deadline,
    };
    use crate::error::FrustError;
    use crate::model::{App, Article, FeedState, Group};
    use crate::storage::{Storage, test_support::FailingBackend};
    use redb::{Database, backends::InMemoryBackend};
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;

    fn make_article(id: u64, feed_id: u64) -> Article {
        Article {
            id,
            feed_id,
            title: String::from("Title"),
            url: String::from("http://example.com/a"),
            content: String::from("Content"),
            summary: None,
            timestamp: 1_000,
            added_at: 1_000,
            enclosures: Vec::new(),
        }
    }

    fn make_result(feed_id: u64, etag: &str, articles: Vec<Article>) -> FeedResult {
        FeedResult {
            feed_id,
            articles,
            state: FeedState {
                last_etag: Some(etag.to_string()),
                last_check_ts: Some(1_000),
                last_modified_ts: None,
                last_http_status: Some(200),
            },
        }
    }

    fn in_memory_db() -> Database {
        Database::builder()
            .create_with_backend(InMemoryBackend::new())
            .unwrap()
    }

    #[test]
    fn test_persist_results_saves_articles_and_states() {
        let storage = Storage::from_databases(in_memory_db(), in_memory_db());
        let results = vec![
            make_result(1, "\"v1\"", vec![make_article(10, 1), make_article(11, 1)]),
            // 304-style result: no articles, state must still be saved
            make_result(2, "\"v2\"", vec![]),
        ];

        let written = persist_results(&storage, results).unwrap();

        assert_eq!(written, 2);
        assert_eq!(storage.load_articles_for_feed(1).unwrap().len(), 2);
        let states = storage.load_all_states().unwrap();
        assert_eq!(states[&1].last_etag.as_deref(), Some("\"v1\""));
        assert_eq!(states[&2].last_etag.as_deref(), Some("\"v2\""));
    }

    #[test]
    fn test_persist_results_keeps_old_etag_when_article_write_fails() {
        let (backend, fail_writes) = FailingBackend::new();
        let articles_db = Database::builder().create_with_backend(backend).unwrap();
        let storage = Storage::from_databases(articles_db, in_memory_db());
        storage
            .save_feed_states(&[(1, make_result(1, "\"old\"", vec![]).state)])
            .unwrap();

        // Simulate a full SD card on the articles database.
        fail_writes.store(true, Ordering::SeqCst);
        let results = vec![make_result(1, "\"new\"", vec![make_article(10, 1)])];

        assert!(persist_results(&storage, results).is_err());

        // The new ETag must not be committed, otherwise the next run gets a
        // 304 and the article above is lost for good.
        let states = storage.load_all_states().unwrap();
        assert_eq!(states[&1].last_etag.as_deref(), Some("\"old\""));
    }

    #[test]
    fn test_persist_results_succeeds_when_state_write_fails() {
        let (backend, fail_writes) = FailingBackend::new();
        let states_db = Database::builder().create_with_backend(backend).unwrap();
        let storage = Storage::from_databases(in_memory_db(), states_db);

        fail_writes.store(true, Ordering::SeqCst);
        let results = vec![make_result(1, "\"new\"", vec![make_article(10, 1)])];

        // Articles are safe; losing the state only costs a re-fetch.
        assert_eq!(persist_results(&storage, results).unwrap(), 1);
        assert!(storage.load_article_ids().unwrap().contains(&10));
    }

    // ---- run_group_exports (B11) ----

    fn feed(url: &str) -> crate::model::Feed {
        crate::model::Feed {
            title: url.to_string(),
            slug: url.to_string(),
            url: url.to_string(),
            page_url: String::new(),
            content_mode: crate::model::ContentMode::Default,
            selector: None,
            filters: Vec::new(),
            retention: 0,
            media: false,
            media_max_size: 0,
            enrichment_prepend: None,
            enrichment_append: None,
        }
    }

    /// App with one group per `(slug, output, feed_id)`.
    fn app_with_groups(output_dir: &std::path::Path, groups: &[(&str, &str, u64)]) -> App {
        let groups = groups
            .iter()
            .enumerate()
            .map(|(index, (slug, output, feed_id))| {
                let group = Group {
                    title: slug.to_string(),
                    slug: slug.to_string(),
                    output: output.to_string(),
                    feeds: HashMap::from([(*feed_id, feed(slug))]),
                    ..Group::default()
                };
                (index as u64, group)
            })
            .collect();
        App {
            output: output_dir.display().to_string(),
            groups,
            ..App::default()
        }
    }

    #[test]
    fn test_run_group_exports_fails_when_a_group_fails() {
        let dir = tempfile::tempdir().unwrap();
        // "blocked" is a file, so blocked/b.atom cannot be created.
        std::fs::write(dir.path().join("blocked"), b"").unwrap();
        let app = app_with_groups(
            dir.path(),
            &[("a", "a.atom", 1), ("b", "blocked/b.atom", 2)],
        );
        let storage = Storage::from_databases(in_memory_db(), in_memory_db());
        storage
            .upsert_articles(vec![make_article(10, 1), make_article(20, 2)])
            .unwrap();

        let result = run_group_exports(&app, &storage);

        let message = result
            .expect_err("a failed export must fail the run")
            .to_string();
        assert!(message.contains("1 of 2"), "{message}");
        // The healthy group is still exported.
        assert!(dir.path().join("a.atom").is_file());
    }

    #[test]
    fn test_run_group_exports_succeeds_when_all_groups_export() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_with_groups(dir.path(), &[("a", "a.atom", 1)]);
        let storage = Storage::from_databases(in_memory_db(), in_memory_db());
        storage.upsert_articles(vec![make_article(10, 1)]).unwrap();

        run_group_exports(&app, &storage).unwrap();
        assert!(dir.path().join("a.atom").is_file());
    }

    // ---- unique_feeds (B12) ----

    #[test]
    fn test_unique_feeds_fetches_shared_feed_once_with_first_group_settings() {
        let dir = tempfile::tempdir().unwrap();
        // Same feed id 1 in both groups, with different retention.
        let mut app = app_with_groups(
            dir.path(),
            &[("music", "m.atom", 1), ("guitar", "g.atom", 1)],
        );
        for group in app.groups.values_mut() {
            let retention = if group.slug == "guitar" { 30 } else { 90 };
            group.feeds.get_mut(&1).unwrap().retention = retention;
        }
        app.groups
            .values_mut()
            .find(|group| group.slug == "music")
            .unwrap()
            .feeds
            .insert(2, feed("other"));

        let mut feeds = unique_feeds(&app);
        feeds.sort_by_key(|(feed_id, _)| *feed_id);

        let ids: Vec<u64> = feeds.iter().map(|(feed_id, _)| *feed_id).collect();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(
            feeds[0].1.retention, 30,
            "settings of 'guitar', first by slug"
        );
    }

    #[tokio::test]
    async fn test_with_deadline_aborts_stuck_work() {
        let stuck = std::future::pending::<Result<(), FrustError>>();
        let result = with_deadline(std::time::Duration::from_millis(10), stuck).await;
        assert!(matches!(result, Err(FrustError::RunTimeout(_))));
    }

    #[tokio::test]
    async fn test_with_deadline_passes_through_result() {
        let done = async { Ok::<_, FrustError>(7) };
        let result = with_deadline(std::time::Duration::from_secs(5), done).await;
        assert_eq!(result.unwrap(), 7);
    }

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
        assert!(!append_within_cap(&mut buf, &[0u8; 20], 100));
        // Buffer is unchanged — no partial writes past the cap
        assert_eq!(buf.len(), 90);
    }

    #[test]
    fn test_append_within_cap_exact_boundary_is_accepted() {
        let mut buf = vec![0u8; 90];
        // Exactly reaching the cap must succeed (limit is inclusive)
        assert!(append_within_cap(&mut buf, &[0u8; 10], 100));
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
