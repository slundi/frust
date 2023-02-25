//example [here](https://github.com/actix/examples/tree/master/databases/sqlite)
use chrono::prelude::*;
use serde::{Serialize, Deserialize};

pub(crate) mod account;
pub(crate) mod article;
pub(crate) mod feed;
pub(crate) mod folder;

//pub type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
//pub type Pool = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;
pub(crate) type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
pub(crate) type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Account {
    pub hash_id: String,
    pub username: String,
    pub encrypted_password: String,
    pub config: String,
    pub created: DateTime<Utc>,
    pub token: String,
    pub token_created: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Token {
    pub created: DateTime<Utc>,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Folder {
    /// Hash ID for POST/PATCH/DELETE
    pub hash_id: String,
    pub name: String,
}

/// Essential feed information to update data and display it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Feed {
    pub hash_id: String,
    /// Name of the feed. Value is retrieved from the ATOM/RSS or from the user if he renames it.
    pub name: String,
    pub page_url: String,
    pub feed_url: String,
    pub updated: DateTime<Utc>,
    /// Feed icon will be in <ASSETS_PATH>/f/<hash of the url>.png (convert it if needed)
    pub icon_filename: String,
    /// how many articles are unread for the user
    pub unread_count: u32,
    /// If we inject links (save, unsubscribe feed?) in articles in published feeds (when Frust generate RSS feed and
    /// the user uses another client instead of the Frust UI)
    pub inject: bool,
}

const DATETIME_UTC_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";
fn get_datetime_utc(data: String) -> DateTime<Utc> {
    chrono::DateTime::parse_from_str(&format!("{} +0000", data), DATETIME_UTC_FORMAT).expect("Cannot parse DB datetime").with_timezone(&Utc)
}

/// Create tables if not exists
pub(crate) fn create_schema(conn: Connection) {
    log::info!("Preparing DB schema import");
    let sql = std::fs::read_to_string(std::path::Path::new("schema.sqlite.sql"))
        .expect(crate::messages::ERROR_SCHEMA_FILE);
    let mut batch = rusqlite::Batch::new(&conn, &sql);
    while let Some(mut stmt) = batch.next().expect("Cannot execute next schema statement") {
        stmt.execute([]).expect("Cannot execute schema statement");
        log::info!("Table created!");
    }
}
