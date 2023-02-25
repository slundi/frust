extern crate slug;
extern crate yaml_rust;

use std::path::Path;
use std::{env, process::ExitCode};

pub(crate) mod model;
pub(crate) mod config;

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
    std::fs::create_dir_all(config.output).unwrap_or_else(|e| panic!("Unable to create output directory: {}", e));
    ExitCode::SUCCESS
}
