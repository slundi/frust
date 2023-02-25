#[macro_use]
extern crate lazy_static;
extern crate bcrypt;
extern crate r2d2;
extern crate r2d2_sqlite;
extern crate rusqlite;
extern crate uuid;

use actix::Actor;
use actix_files::Files;
use actix_web::{web, App, HttpServer};
use dotenv::dotenv;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

mod auth;
mod db;
mod config;
mod messages;
mod modules;
pub(crate) mod model;
mod routes;
mod scheduler;
mod utils;

lazy_static! {
    //static TOKEN_CACHE: std::sync::RwLock<std::collections::HashMap<String, &model::Account>> = std::sync::RwLock::new(std::collections::HashMap::with_capacity(1024));
    /// Hash ID data for encode and decode functions. It is initilized here because it is easier than loading a key from the `.env`.
    static ref HASH_ID: std::sync::RwLock<harsh::Harsh> = std::sync::RwLock::new(harsh::Harsh::builder()
        .length(8).salt(load_config().secret_key.as_bytes())
        .build().unwrap());
    static ref CONFIG: std::sync::RwLock<crate::config::Config> = std::sync::RwLock::new(crate::config::Config::default());
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = load_config();
    //configure logger
    simple_logger::init_with_level(match &config.log_level as &str {
        "WARN" => log::Level::Warn,
        "ERROR" => log::Level::Error,
        "DEBUG" => log::Level::Debug,
        "TRACE" => log::Level::Trace,
        _ => log::Level::Info,
    }).unwrap();

    //create folders for assets
    create_assets_directories(config.assets_path.clone());
    let mut feed_assets_path = config.assets_path.clone();
    feed_assets_path.push_str("/f/");
    let mut article_assets_path = config.assets_path.clone();
    article_assets_path.push_str("/a/");

    log::info!("Working directory: {}", std::env::current_dir().expect("Cannot get working directory").display());

    let pool = db::Pool::new(SqliteConnectionManager::file("frust.sqlite3")).expect("Cannot create database pool");
    db::create_schema(pool.get().expect("Cannot get connection"));

    scheduler::Scheduler{pool: pool.clone()}.start();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(actix_web::middleware::Logger::default())
            .service(routes::index)
            .service(Files::new("/s", "static/"))
            .service(Files::new("/a", &article_assets_path))
            .service(Files::new("/f", &feed_assets_path))
            //user management
            .service(routes::account::register)
            .service(routes::account::login)
            .service(routes::account::patch)
            .service(routes::account::delete)
            .service(routes::account::delete_token)
            .service(routes::account::renew_token)
            .service(routes::account::list_tokens)
            .service(
                web::scope("/folders")
                    //folder management
                    .service(routes::folder::list)
                    .service(routes::folder::post)
                    .service(routes::folder::patch)
                    .service(routes::folder::delete),
            )
            .service(
                web::scope("/feeds")
                    //folder management
                    .service(routes::feed::list)
                    .service(routes::feed::post)
                    .service(routes::feed::patch)
                    .service(routes::feed::delete)
                    .service(routes::feed::import_opml)
                    .service(routes::feed::export_opml)
            )
    })
    .bind(config.server_addr.clone())?
    .workers(2)
    .run();
    log::info!("starting HTTP server at http://{}/", config.server_addr);

    server.await
}

fn load_config() -> config::Config {
    dotenv().ok();
    let config_ = ::config::Config::builder()
        .add_source(::config::Environment::default())
        .build().expect("Cannot build config");
    let mut config = CONFIG.write().expect("Cannot load config");
    *config = config_.try_deserialize().expect("Cannot get config");
    config.clone()
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
