extern crate slug;
extern crate yaml_rust;

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use gumdrop::Options;
use tracing::info;

use crate::cli::{CliOptions, Command};
use crate::error::FrustError;
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

fn create_output_structure(app: &App) -> Result<(), FrustError> {
    for g in app.groups.iter() {
        if !app.retrieve_media_server {
            continue;
        }
        for f in g.1.feeds.iter() {
            let mut folder =
                String::with_capacity(app.output.len() + g.1.slug.len() + f.1.slug.len() + 2);
            folder.push_str(&app.output);
            folder.push('/');
            folder.push_str(&f.1.slug);
            std::fs::create_dir_all(folder)?;
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
        tracing::error!("Failed to create output directories: {}", e);
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

#[tokio::main]
async fn main() -> ExitCode {
    let subscriber = tracing_subscriber::fmt()
        .with_level(true)
        .with_max_level(tracing::level_filters::LevelFilter::INFO)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let opts = CliOptions::parse_args_default_or_exit();

    if opts.version {
        println!("frust-feed {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

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
                command::archive(o)
            };
            if let Err(e) = result {
                tracing::error!("{}", e);
                return ExitCode::FAILURE;
            }
        }
        None => {
            let config_path = opts.config.as_deref().unwrap_or("config.yaml");
            return run_aggregator(config_path).await;
        }
    }

    ExitCode::SUCCESS
}
