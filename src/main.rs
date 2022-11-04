#[macro_use]
extern crate lazy_static;
extern crate bcrypt;

use actix_files::Files;
use log::{info, error};
use r2d2_sqlite::{self, SqliteConnectionManager};
use serde::Deserialize;
use std::path::Path;

mod db;
mod messages;
mod model;
mod routes;
mod routes_account;
mod routes_folder;
use db::{Pool, Queries};

//use futures::future::join_all;

lazy_static! {
    //static TOKEN_CACHE: std::sync::RwLock<std::collections::HashMap<String, &model::Account>> = std::sync::RwLock::new(std::collections::HashMap::with_capacity(1024));
    static ref HASH_ID: std::sync::RwLock<harsh::Harsh> = std::sync::RwLock::new(harsh::Harsh::builder()
        .salt(
            chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
                .as_bytes(),
        )
        .length(8)
        .build()
        .unwrap());
}

pub fn encode_id(id: i32) -> String {
    let hasher = HASH_ID.read().expect("Cannot get ID hasher");
    hasher.encode(&[id.try_into().unwrap()])
}

/// Decode a hash ID, if wrong it return -1
pub fn decode_id(hash: String) -> i32 {
    let hasher = HASH_ID.read().expect("Cannot get ID hasher");
    let result = hasher.decode(hash);
    if let Ok(ids) = result {
        return ids[0].try_into().unwrap();
    }
    error!("Cannot decode hash ID");
    -1
}

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
    /// Secret key for hashing functions
    pub secret_key: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server_addr: "127.0.0.1:8330".to_owned(),
            log_level: "INFO".to_owned(),
            sqlite_file: "data/frust.sqlite3".to_owned(),
            article_keep_time: 30,
            assets_path: "data/assets".to_owned(),
            feed_refresh_time: 600,
            secret_key: "MY-T0P-S3CR3T-K3Y!".to_owned(),
        }
    }
}

//use ::config::Config;
use actix_web::{web, App, HttpServer};
use dotenv::dotenv;

use crate::routes_account::*;
use crate::routes_folder::*;

async fn check_token(pool: &web::Data<crate::db::Pool>, req: actix_web::HttpRequest) -> Option<model::Account> {
    let value = req.headers().get(actix_web::http::header::AUTHORIZATION);
    if let Some(token) = value {
        let raw_token = token.to_str();
        if let Ok(token) = raw_token {
            let result = crate::db::get_user_from_token(&pool, token.to_owned()).await;
            if let Ok(account) = result {
                return Some(account);
            }
        }
    }
    None
}

#[actix_web::get("/")]
async fn index() -> impl actix_web::Responder {
    actix_files::NamedFile::open_async("pages/index.html").await
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let config_ = config::Config::builder()
        .add_source(::config::Environment::default())
        .build()
        .expect("Cannot build config");
    let config: Config = config_.try_deserialize().expect("Cannot get config");

    //configure logger
    simple_logger::init_with_level(match &config.log_level as &str {
        "WARN" => log::Level::Warn,
        "ERROR" => log::Level::Error,
        "DEBUG" => log::Level::Debug,
        "TRACE" => log::Level::Trace,
        _ => log::Level::Info,
    })
    .unwrap();

    info!(
        "Working directory: {}",
        std::env::current_dir()
            .expect("Cannot get working directory")
            .display()
    );

    //create folders for assets
    create_assets_directories(config.assets_path.clone());
    let mut feed_assets_path = config.assets_path.clone();
    feed_assets_path.push_str("/f/");
    let mut article_assets_path = config.assets_path.clone();
    article_assets_path.push_str("/a/");

    /*let file = std::path::Path::new(&config.sqlite_file);
    if file.exists() {
        std::fs::create_dir_all(file).expect("Cannot delete database file");
    }*/
    let pool = Pool::new(SqliteConnectionManager::file("frust.sqlite3"))
        .expect("Cannot create database pool");
    db::create_schema(pool.get().expect("Cannot get connection"));

    let server = HttpServer::new(move || {
        /*let csrf = Csrf:: <rand::prelude::StdRng> ::new()
        .set_cookie(actix_web::http::Method::GET, "/login");*/
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(actix_web::middleware::Logger::default())
            .service(index)
            .service(Files::new("/s", "static/"))
            .service(Files::new("/a", &article_assets_path))
            .service(Files::new("/f", &feed_assets_path))
            //user management
            .service(route_register)
            .service(route_login)
            .service(route_edit_account)
            .service(route_delete_account)
            .service(route_delete_token)
            .service(
                web::scope("/folders")
                    //folder management
                    .service(route_list_folers)
                    .service(route_create_folder)
                    .service(route_edit_folder)
                    .service(route_delete_folder),
            )
    })
    .bind(config.server_addr.clone())?
    .run();
    info!("starting HTTP server at http://{}/", config.server_addr);

    server.await
}

fn create_assets_directories(path: String) {
    let mut root = path.clone();
    root.push_str("/f");
    let feed_assets_path = Path::new(&root);
    if !feed_assets_path.is_dir() {
        std::fs::create_dir_all(feed_assets_path).expect("Cannot create feed assets folder");
    }
    let mut root = path;
    root.push_str("/a");
    let article_assets_path = Path::new(&root);
    if !article_assets_path.is_dir() {
        std::fs::create_dir_all(article_assets_path).expect("Cannot create article assets folder");
    }
}

#[cfg(test)]
mod tests {
    /*use actix_web::{
        http::{header::ContentType},
        test, get,
    };

    #[actix_web::test]
    async fn test_index_ok() {
        let app = test::init_service(actix_web::App::new().route("/", crate::web::get().to(crate::index))).await;
        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }*/
}
