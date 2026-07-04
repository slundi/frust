extern crate slug;

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;

use anyhow::Context;
use chrono::{DateTime, Utc};
use tracing::info;

use crate::cli::{CliOptions, Command};
use crate::model::App;
use crate::storage::Storage;

pub(crate) mod cli;
pub(crate) mod command;
pub(crate) mod config;
pub(crate) mod error;
pub(crate) mod export;
pub(crate) mod model;
pub(crate) mod opml;
pub(crate) mod processing;
pub(crate) mod storage;
pub(crate) mod utils;

const DEFAULT_HTTP_TIMEOUT: u8 = 10;
const DEFAULT_RETRIEVE_SERVER_MEDIA: bool = false;
static START_TIME: OnceLock<DateTime<Utc>> = OnceLock::new();

/// When `retrieve_media_server` is enabled, create one subdirectory per feed
/// under `app.output` so per-article files can be written alongside their
/// media. No-op otherwise: the flat `media/<xxh3>.<ext>` layout doesn't need
/// per-feed folders.
fn create_output_structure(app: &App) -> anyhow::Result<()> {
    if !app.retrieve_media_server {
        return Ok(());
    }
    let base = Path::new(&app.output);
    for group in app.groups.values() {
        for feed in group.feeds.values() {
            let path = base.join(&feed.slug);
            std::fs::create_dir_all(&path)
                .with_context(|| format!("create directory {}", path.display()))?;
        }
    }
    Ok(())
}

async fn run_aggregator(config_path: &str) -> ExitCode {
    let pwd = match std::env::current_dir() {
        Ok(p) => p.display().to_string(),
        Err(e) => {
            tracing::error!("Cannot determine working directory: {}", e);
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("Working directory: {}", pwd);
    tracing::info!("Config file: {}", config_path);
    if !Path::new(config_path).exists() {
        tracing::error!("Config file not found: {} in {}", config_path, pwd);
        return ExitCode::FAILURE;
    }

    let mut exit_code = ExitCode::SUCCESS;
    let app = crate::config::load_config_file(config_path.to_string());
    START_TIME.set(Utc::now()).unwrap();
    std::fs::create_dir_all(app.output.clone()).unwrap_or_else(|e| {
        tracing::error!("Unable to create output directory: {}", e);
        exit_code = ExitCode::FAILURE;
    });
    if let Err(e) = create_output_structure(&app) {
        tracing::error!("Failed to create output directories: {:#}", e);
        return ExitCode::FAILURE;
    }

    {
        info!("Cleaning up old articles");
        let articles_path = format!("{}/articles.redb", app.output);
        let states_path = format!("{}/states.redb", app.output);
        if let Ok(storage) = Storage::new(&articles_path, &states_path) {
            let feed_retentions: HashMap<u64, u16> = app
                .groups
                .values()
                .flat_map(|g| g.feeds.iter().map(|(id, f)| (*id, f.retention)))
                .collect();
            let now_ts = START_TIME.get().unwrap().timestamp();
            match storage.delete_expired_articles(now_ts, &feed_retentions, app.retention) {
                Ok(0) => info!("No article to delete"),
                Ok(n) => tracing::info!("Cleaned {} expired article(s)", n),
                Err(e) => tracing::warn!("Article cleanup failed: {}", e),
            }
            let media_dir = format!("{}/media", app.output);
            match storage.purge_orphaned_media(&media_dir) {
                Ok(0) => info!("No media to delete"),
                Ok(n) => tracing::info!("Purged {} orphaned media file(s)", n),
                Err(e) => tracing::warn!("Media purge failed: {}", e),
            }
        }
    }

    if let Err(e) = crate::processing::start(&app).await {
        tracing::error!("Processing failed: {}", e);
        exit_code = ExitCode::FAILURE;
    }
    exit_code
}

/// Build a `tracing` filter from a `RUST_LOG`-style directive.
///
/// Returns `INFO` when the directive is missing or fails to parse — cron
/// setups shouldn't crash on a typo in an env var.
fn build_log_filter(directive: Option<&str>) -> tracing_subscriber::EnvFilter {
    directive
        .and_then(|d| tracing_subscriber::EnvFilter::try_new(d).ok())
        .unwrap_or_else(|| tracing_subscriber::EnvFilter::new("info"))
}

#[tokio::main]
async fn main() -> ExitCode {
    let filter = build_log_filter(std::env::var("RUST_LOG").ok().as_deref());
    let subscriber = tracing_subscriber::fmt()
        .with_level(true)
        .with_env_filter(filter)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let opts: CliOptions = argh::from_env();

    if opts.version {
        println!("frust-feed {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let config_path = opts.config.as_deref().unwrap_or("config.yaml");
    match opts.command {
        Some(Command::Import(ref o)) => {
            if let Err(e) = command::import_opml(o) {
                tracing::error!("{}", e);
                return ExitCode::FAILURE;
            }
        }
        Some(Command::Export(ref o)) => {
            let result = if o.config_file().is_some() {
                command::export_opml(o)
            } else {
                command::archive(o, config_path)
            };
            if let Err(e) = result {
                tracing::error!("{}", e);
                return ExitCode::FAILURE;
            }
        }
        None => {
            return run_aggregator(config_path).await;
        }
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::model::{App, Feed, Group};

    use super::{build_log_filter, create_output_structure};

    fn unique_dir(prefix: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("/tmp/frust_out_{}_{}", prefix, nanos)
    }

    fn feed_with_slug(slug: &str) -> Feed {
        Feed {
            title: slug.to_string(),
            slug: slug.to_string(),
            url: format!("https://{}.example/feed", slug),
            page_url: String::new(),
            content_mode: crate::model::ContentMode::Default,
            selector: None,
            filters: Vec::new(),
            output: String::new(),
            retention: 0,
            media: false,
            media_max_size: 0,
            enrichment_prepend: None,
            enrichment_append: None,
        }
    }

    fn group_with_feeds(feeds: &[&str]) -> Group {
        let mut map = HashMap::new();
        for (i, slug) in feeds.iter().enumerate() {
            map.insert(i as u64, feed_with_slug(slug));
        }
        Group {
            feeds: map,
            ..Group::default()
        }
    }

    fn app_with(output: &str, retrieve: bool, groups: Vec<Group>) -> App {
        let mut gmap = HashMap::new();
        for (i, g) in groups.into_iter().enumerate() {
            gmap.insert(i as u64, g);
        }
        App {
            output: output.to_string(),
            retrieve_media_server: retrieve,
            groups: gmap,
            ..App::default()
        }
    }

    #[test]
    fn test_create_output_structure_disabled_creates_nothing() {
        let dir = unique_dir("disabled");
        std::fs::create_dir_all(&dir).unwrap();
        let app = app_with(&dir, false, vec![group_with_feeds(&["a", "b"])]);
        create_output_structure(&app).unwrap();
        assert!(!std::path::Path::new(&format!("{}/a", dir)).exists());
        assert!(!std::path::Path::new(&format!("{}/b", dir)).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_log_filter_defaults_to_info_when_none() {
        let filter = build_log_filter(None);
        // EnvFilter's Display echoes the effective directives; "info" is enough
        // for a smoke check.
        assert_eq!(format!("{}", filter), "info");
    }

    #[test]
    fn test_build_log_filter_parses_valid_directive() {
        let filter = build_log_filter(Some("frust=debug,warn"));
        let rendered = format!("{}", filter);
        assert!(
            rendered.contains("frust=debug"),
            "unexpected filter directives: {}",
            rendered
        );
        assert!(
            rendered.contains("warn"),
            "unexpected filter directives: {}",
            rendered
        );
    }

    #[test]
    fn test_build_log_filter_falls_back_on_garbage() {
        // A malformed directive must not crash the CLI in a cron job.
        let filter = build_log_filter(Some("!!not-a-real-directive!!"));
        assert_eq!(format!("{}", filter), "info");
    }

    #[test]
    fn test_create_output_structure_error_includes_failing_path() {
        // Point output at a path that cannot be created (parent is a file).
        let dir = unique_dir("badoutput");
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = format!("{}/blocker", dir);
        std::fs::write(&blocker, b"").unwrap();
        // <blocker>/<feed_slug> — parent is a regular file, so create_dir_all fails.
        let app = app_with(&blocker, true, vec![group_with_feeds(&["only-feed"])]);
        let err = create_output_structure(&app).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("only-feed"),
            "error context should name the failing directory, got {}",
            msg
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_create_output_structure_enabled_creates_one_folder_per_feed() {
        let dir = unique_dir("enabled");
        let app = app_with(
            &dir,
            true,
            vec![
                group_with_feeds(&["alpha", "beta"]),
                group_with_feeds(&["gamma"]),
            ],
        );
        create_output_structure(&app).unwrap();
        // Folders live at <output>/<feed_slug>, not <output>/<group>/<feed>.
        for slug in ["alpha", "beta", "gamma"] {
            let p = std::path::Path::new(&dir).join(slug);
            assert!(p.is_dir(), "missing {}", p.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
