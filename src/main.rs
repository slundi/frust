extern crate slug;
extern crate yaml_rust;

use std::path::Path;
use std::{env, process::ExitCode};

use std::collections::HashMap;

pub(crate) mod config;
pub(crate) mod model;
pub(crate) mod processing;

const DEFAULT_HTTP_TIMEOUT: u8 = 10;
const DEFAULT_RETRIEVE_SERVER_MEDIA: bool = false;

/// Create all feed folders following the scheme: `<output>/<feed slug>`
fn create_output_structure(
    output: String,
    retrieve: bool,
    feeds: &HashMap<u64, crate::model::Feed>,
) {
    for f in feeds.iter() {
        // does not require a folder if we do not save media, we will just keep an XML feed with combined old articles with new ones
        if !retrieve {
            continue;
        }
        let mut folder = String::with_capacity(output.len() + f.1.slug.len() + 1);
        folder.push_str(&output);
        folder.push('/');
        folder.push_str(&f.1.slug);
        std::fs::create_dir_all(folder).unwrap();
    }
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
    let pwd = env::current_dir().unwrap().display().to_string();
    tracing::info!("Working directory: {}", pwd);
    tracing::info!("Config file: {}", config_file);
    if !Path::new(&config_file).exists() {
        tracing::error!(
            "Config file not found: {} in {}",
            config_file,
            pwd
        );
        print_usage();
        return ExitCode::FAILURE;
    }

    // make output directory if not exists, check for permissions
    let mut exit_code = ExitCode::SUCCESS;
    // load globals
    let app = crate::config::load_config_file(config_file);
    std::fs::create_dir_all(app.output.clone()).unwrap_or_else(|e| {
        tracing::error!("Unable to create output directory: {}", e);
        exit_code = ExitCode::FAILURE;
    });
    create_output_structure(app.output.clone(), app.retrieve_media_server, &app.feeds);
    crate::processing::start(&app).await;
    exit_code
}
