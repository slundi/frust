#[macro_use]
extern crate lazy_static;
extern crate bcrypt;

extern crate r2d2;
extern crate r2d2_sqlite;
extern crate rusqlite;

use actix_files::Files;
use actix_web::{web, App, HttpServer};
use dotenv::dotenv;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

mod auth;
mod db;
mod config;
mod messages;
mod model;
mod routes;
mod utils;

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

#[actix_web::get("/")]
async fn index() -> impl actix_web::Responder {
    actix_files::NamedFile::open_async("pages/index.html").await
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let config_ = ::config::Config::builder()
        .add_source(::config::Environment::default())
        .build()
        .expect("Cannot build config");
    let config: crate::config::Config = config_.try_deserialize().expect("Cannot get config");

    //configure logger
    simple_logger::init_with_level(match &config.log_level as &str {
        "WARN" => log::Level::Warn,
        "ERROR" => log::Level::Error,
        "DEBUG" => log::Level::Debug,
        "TRACE" => log::Level::Trace,
        _ => log::Level::Info,
    })
    .unwrap();

    log::info!("Working directory: {}", std::env::current_dir().expect("Cannot get working directory").display());

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
    let pool = db::Pool::new(SqliteConnectionManager::file("frust.sqlite3")).expect("Cannot create database pool");
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
            .service(routes::account::route_register)
            .service(routes::account::route_login)
            .service(routes::account::route_edit_account)
            .service(routes::account::route_delete_account)
            .service(routes::account::route_delete_token)
            .service(
                web::scope("/folders")
                    //folder management
                    .service(routes::folder::route_list_folers)
                    .service(routes::folder::route_create_folder)
                    .service(routes::folder::route_edit_folder)
                    .service(routes::folder::route_delete_folder),
            )
    })
    .bind(config.server_addr.clone())?
    .workers(2)
    .run();
    log::info!("starting HTTP server at http://{}/", config.server_addr);

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
