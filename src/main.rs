use actix_files::Files;
use std::io::prelude::*;
use std::path::Path;
use futures_util::future::join_all;
use r2d2_sqlite::{self, SqliteConnectionManager};

mod routes;
mod db;
use db::{Pool, Queries};

//use sailfish;
//use futures::future::join_all;

use serde::Deserialize;
#[derive(Debug, Deserialize)]
pub struct Config {
    /// Server `<IP or hostaname>:<port>`. Default is `127.0.0.1:8330`
    pub server_addr: String,
    /// Log level (available options are: INFO, WARN, ERROR, DEBUG, TRACE). Default is `INFO`.
    pub log_level: String,
    /// Where the SQLite database should be created/loaded. Default is `data/frust.sqlite3`
    pub sqlite_file: String,
    /// Delete old (and not save from any user) articles older than XX days. Default is 30 days.
    /// u16 max value is 65535 so it is more than 175 years
    pub article_keep_time: u16,
    /// Where do we store feed and article assets (images for now)? Default is `data/assets`.
    /// Some sub folders will be created:
    /// * `f` for feed icons (path will be:  `f/<feed UUID>.<ext>`)
    /// * `a` for article content such as images (path will be: `a/<article UUID>/<image name>.<ext>`)
    pub assets_path: String,
    /// Refresh all feed every XXX seconds. Default is 600 seconds (10 minutes)
    pub feed_refresh_time: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server_addr: "127.0.0.1:8330".to_owned(),
            log_level: "INFO".to_owned(),
            sqlite_file: "data/frust.sqlite3".to_owned(),
            article_keep_time: 30,
            assets_path: "data/assets".to_owned(),
            feed_refresh_time: 600 }
    }
}

//use ::config::Config;
use actix_web::{web, App, HttpServer};
use dotenv::dotenv;

//use crate::config::ExampleConfig;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    dotenv().ok();

    let config_ = config::Config::builder()
        .add_source(::config::Environment::default())
        .build()
        .expect("Cannot build config");
    let config: Config = config_.try_deserialize().expect("Cannot get config");

    //create folders for assets
    let mut root = config.assets_path.clone();
    root.push_str("/f");
    let feed_assets_path = Path::new(&root);
    if !feed_assets_path.is_dir() {
        std::fs::create_dir_all(feed_assets_path).expect("Cannot create feed assets folder");
    }
    let mut root = config.assets_path.clone();
    root.push_str("/a");
    let article_assets_path = Path::new(&root);
    if !article_assets_path.is_dir() {
        std::fs::create_dir_all(article_assets_path).expect("Cannot create article assets folder");
    }

    let pool = Pool::new(SqliteConnectionManager::file("frust.sqlite3")).expect("Cannot create database pool");

    let server = HttpServer::new(move || {
        /*let csrf = Csrf:: <rand::prelude::StdRng> ::new()
            .set_cookie(actix_web::http::Method::GET, "/login");*/
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .service(Files::new("/s", "static/"))
            //.service(web::resource("/users").route(web::post().to(add_user)))
    })
    .bind(config.server_addr.clone())?
    .run();
    log::info!("starting HTTP server at http://{}/", config.server_addr);

    server.await
}
