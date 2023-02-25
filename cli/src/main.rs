use async_std::{eprint, eprintln};
use std::{env, process::ExitCode};
use std::path::Path;

struct Config {
    /// Number of maximum simultaneous tasks the app will be running, https://stackoverflow.com/questions/22155130/determine-number-of-cores-using-rust
    workers: u8,
    /// Output folder like `/var/www/rss` where feeds are generated and assets are stored. Be sure to have permissions.
    output: String,
    // format: "atom"  // generated feed format (rss, atom or json)
    /// Timeout in seconds when performing HTTP queries, default 10 seconds
    timeout: u8,
    /// Minimal refresh time in seconds for feeds and new articles, default 600 seconds (10 minutes)
    min_refresh_time: u32,
    /// Keep time in days, default 30 days. After 30 days, it will remove it from the feed, and also from the output path (assets)
    article_keep_time: u16,
    /// Download images `<output>/[<folder>/]<feed>/assets`. Default is `false`.
    retrieve_server_media:  bool,
    /// Default article sorting. Minus before the filed indicates a descending order. Available fields are: date, feed
    // sort: "-date"  # OPTIONAL default sorting. Default is "-date". 
}

/// A group allows you to produce a unique feed by aggregating the enumerated ones.
struct Group {
    title: String,
    slug: String,
    // sort: "-date"
    /// List of feed slugs
    feeds: Vec<String>,
}

struct Feed {
    title: String,
    /// Unique and URL usable string to identify the feed
    slug: String,
    url: String,
    page_url: String,
    xpath: String, // Option<String>?
    /// Minimal refresh time in seconds
    min_refresh_time: u32,
    /// Keep time in days
    article_keep_time: u16,
    /// If we retrive article assets (images, ...)
    retrieve_server_media: bool,
    // sort: "-date"
    // filters:  # see bellow
    // produces: ["HTML", "PDF"]
    // group:
}

struct Article {
    /// 32 bits xxHash used to identify articles
    hash: u32,
    /// Date and time information, use chrono: https://stackoverflow.com/questions/72884445/chrono-datetime-from-u64-unix-timestamp-in-rust
    date: i64,
    flags: u8,
    slug: String,
    title: String,
    //content: String, //?in this struct?
}

fn print_usage() {
    println!("Usage:    frust-cli path/to/config.yaml");
    println!(
        "       If the config.yaml is in the working directory, the argument is not needed."
    );
}

#[async_std::main]
async fn main() -> ExitCode {
    // parse CLI
    println!("frust-CLI v0.1.0, more at https://github.com/slundi/frust")
    let args: Vec<String> = env::args().collect();
    if args.len() > 2 {
        println!("Too many arguments.");
        print_usage();
        return ExitCode::FAILURE;
    }
    // check config file exists
    let mut config_file = String::from("config.yaml");
    if args.len() == 2 {
        config_file = args[1];
    }
    if !Path::new(&config_file).exists() {
        println!("Config file not found: {} in {:?}", config_file, env::current_dir());
        print_usage();
        return ExitCode::FAILURE;
    }
    // make output directory if not exists, check for permissions
    ExitCode::SUCCESS
}
