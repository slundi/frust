extern crate slug;
extern crate yaml_rust;

use std::path::Path;
use std::{env, process::ExitCode};

use model::AppConfig;

pub(crate) mod model;
pub(crate) mod config;

/// Create all feed folders following the scheme: `<output>/<feed slug>`
fn create_output_structure(config: &AppConfig) {
    for f in &config.feeds {
        // does not require a folder if we do not save media, we will just keep an XML feed with combined old articles with new ones
        if !f.1.config.retrieve_server_media {
            continue;
        }
        let mut folder = String::with_capacity(config.output.len()+f.1.slug.len()+1);
        folder.push_str(&config.output);
        folder.push('/');
        folder.push_str(&f.1.slug);
        std::fs::create_dir_all(folder).unwrap_or_else(|e| panic!("Unable to create feed directory: {} {}", f.1.slug, e));
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
    let config = config::load_config_file(config_file);
    std::fs::create_dir_all(config.output.clone()).unwrap_or_else(|e| panic!("Unable to create output directory: {}", e));
    //load database if exists
    if Path::new(&config.database_file).exists() {
        todo!();
    }
    create_output_structure(&config);
    ExitCode::SUCCESS
}
