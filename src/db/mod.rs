//example [here](https://github.com/actix/examples/tree/master/databases/sqlite)
use chrono::prelude::*;

pub(crate) mod account;
pub(crate) mod folder;

//pub type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
//pub type Pool = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;
pub(crate) type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
pub(crate) type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

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
