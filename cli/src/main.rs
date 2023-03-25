#[macro_use]
extern crate lazy_static;
extern crate slug;
extern crate yaml_rust;

use std::collections::HashMap;
use std::path::Path;
use std::{env, process::ExitCode};

use model::{AppConfig, Storage};
use tokio::sync::RwLock;

use crate::model::DEFAULT_HISTORY_FILE;

pub(crate) mod config;
pub(crate) mod model;
pub(crate) mod processing;

lazy_static! {
    static ref CONFIG: RwLock<AppConfig> = RwLock::new(AppConfig::default());
    static ref DB: RwLock<Storage> = RwLock::new(Storage(HashMap::new()));
}

/// Create all feed folders following the scheme: `<output>/<feed slug>`
async fn create_output_structure() {
    let config = CONFIG.read().await;
    for f in &config.feeds {
        // does not require a folder if we do not save media, we will just keep an XML feed with combined old articles with new ones
        if !f.1.config.retrieve_server_media {
            continue;
        }
        let mut folder = String::with_capacity(config.output.len() + f.1.slug.len() + 1);
        folder.push_str(&config.output);
        folder.push('/');
        folder.push_str(&f.1.slug);
        std::fs::create_dir_all(folder)
            .unwrap_or_else(|e| panic!("Unable to create feed directory: {} {}", f.1.slug, e));
    }
}

/// Load feed history if the file exists. It contains an u64 key and information about last update datetime and last retrieved feed hash.
async fn load_database() {
    let mut db = DB.write().await;
    let config = CONFIG.read().await;
    let mut database_file =
        String::with_capacity(config.output.len() + DEFAULT_HISTORY_FILE.len() + 1);
    database_file.push_str(&config.output);
    database_file.push('/');
    database_file.push_str(DEFAULT_HISTORY_FILE);
    if Path::new(&database_file).exists() {
        let data = std::fs::read(&database_file).expect("Cannot open database file");
        db.0 = bincode::deserialize(&data).expect("Unable to read database");
    }
}

fn print_usage() {
    println!("Usage:    frust-cli path/to/config.yaml");
    println!("       If the config.yaml is in the working directory, the argument is not needed.");
}

#[tokio::main]
async fn main() -> ExitCode {
    // parse CLI
    println!("frust-CLI v0.1.0, more at https://github.com/slundi/frust");
    let args: Vec<String> = env::args().collect();
    if args.len() > 2 {
        println!("Too many arguments.");
        print_usage();
        return ExitCode::FAILURE;
    }
    // check config file exists
    let mut config_file = String::from("config.yaml");
    if args.len() == 2 {
        config_file = args[1].clone();
    }
    if !Path::new(&config_file).exists() {
        println!(
            "Config file not found: {} in {}",
            config_file,
            env::current_dir().unwrap().display()
        );
        print_usage();
        return ExitCode::FAILURE;
    }
    // make output directory if not exists, check for permissions
    config::load_config_file(config_file).await;
    std::fs::create_dir_all((CONFIG.read().await).output.clone())
        .unwrap_or_else(|e| panic!("Unable to create output directory: {}", e));
    load_database().await;
    create_output_structure().await;
    crate::processing::start().await;
    ExitCode::SUCCESS
}
