extern crate slug;
extern crate yaml_rust;

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::{env, process::ExitCode};

use chrono::{DateTime, Utc};

use crate::error::FrustError;
use crate::model::App;
use crate::storage::Storage;

pub(crate) mod config;
pub(crate) mod error;
pub(crate) mod export;
pub(crate) mod model;
pub(crate) mod processing;
pub(crate) mod storage;
pub(crate) mod utils;

const DEFAULT_HTTP_TIMEOUT: u8 = 10;
const DEFAULT_RETRIEVE_SERVER_MEDIA: bool = false;
static START_TIME: OnceLock<DateTime<Utc>> = OnceLock::new();

/// Create all feed folders following the scheme: `<output>/<feed slug>`
fn create_output_structure(app: &App) -> Result<(), FrustError> {
    for g in app.groups.iter() {
        // does not require a folder if we do not save media, we will just keep an XML feed with combined old articles with new ones
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

fn print_usage() {
    println!("Usage:    frust path/to/config.yaml");
    println!("       If the config.yaml is in the working directory, the argument is not needed.");
}

#[tokio::main]
async fn main() -> ExitCode {
    let subscriber = tracing_subscriber::fmt()
        .with_level(true)
        .with_max_level(tracing::level_filters::LevelFilter::INFO)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    // parse CLI
    // tracing::info!("frust-CLI v0.1.0, more at https://codeberg.org/slundi/frust");
    // TODO: use a lib to parse cli
    let args: Vec<String> = env::args().collect();
    if args.len() > 2 {
        tracing::error!("Too many arguments.");
        print_usage();
        return ExitCode::FAILURE;
    }
    // check config file exists
    let mut config_file = String::from("config.yaml");
    if args.len() == 2 {
        config_file = args[1].clone();
    }
    let pwd = match env::current_dir() {
        Ok(p) => p.display().to_string(),
        Err(e) => {
            tracing::error!("Cannot determine working directory: {}", e);
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("Working directory: {}", pwd);
    tracing::info!("Config file: {}", config_file);
    if !Path::new(&config_file).exists() {
        tracing::error!("Config file not found: {} in {}", config_file, pwd);
        print_usage();
        return ExitCode::FAILURE;
    }

    // make output directory if not exists, check for permissions
    let mut exit_code = ExitCode::SUCCESS;
    // load globals
    let app = crate::config::load_config_file(config_file);
    START_TIME.set(Utc::now()).unwrap();
    std::fs::create_dir_all(app.output.clone()).unwrap_or_else(|e| {
        tracing::error!("Unable to create output directory: {}", e);
        exit_code = ExitCode::FAILURE;
    });
    if let Err(e) = create_output_structure(&app) {
        tracing::error!("Failed to create output directories: {}", e);
        return ExitCode::FAILURE;
    }

    // Clean up articles that have exceeded their retention window
    {
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
                Ok(0) => {}
                Ok(n) => tracing::info!("Cleaned {} expired article(s)", n),
                Err(e) => tracing::warn!("Article cleanup failed: {}", e),
            }
            let media_dir = format!("{}/media", app.output);
            match storage.purge_orphaned_media(&media_dir) {
                Ok(0) => {}
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
