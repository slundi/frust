use actix_web::{error, Error};
use chrono::prelude::*;

use crate::{
    encode_id,
    model::{Account, Folder}, decode_id,
};

//pub type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
//pub type Pool = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;
pub(crate) type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
pub(crate) type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

//example [here](https://github.com/actix/examples/tree/master/databases/sqlite)

const SQL_LOGIN: &str = "SELECT id, username, encrypted_password, config, created FROM account WHERE username = :u";
const SQL_AUTH_TOKEN: &str = "SELECT a.*, t.value AS token, t.created AS token_created
                              FROM token t LEFT JOIN account a ON t.account_id = a.id
                              WHERE value = $1";
const SQL_REGISTER: &str = "INSERT INTO account (username, encrypted_password, config) VALUES ($1, $2, '')";
const SQL_DELETE_ACCOUNT: &str = "DELETE FROM account WHERE id = $1";
/// Create a token, ignore it if it already exists
const SQL_CREATE_TOKEN: &str = "INSERT OR IGNORE INTO token (account_id, value) VALUES (:a, :v) RETURNING value";
const SQL_GET_ACCOUNT_TOKENS: &str = "SELECT id, created, name FROM token WHERE account_id = $1";
const SQL_DELETE_TOKEN: &str = "DELETE FROM token WHERE id = $1";
const SQL_CREATE_FOLDER: &str ="INSERT INTO folder (name, account_id) VALUES ($1, $2) RETURNING id";
const SQL_EDIT_FOLDER: &str = "UPDATE folder SET name = $1 WHERE id = $2 AND account_id = $3";
const SQL_GET_MY_FOLDER: &str ="SELECT id, name FROM folder WHERE account_id = $1 ORDER BY name";
const SQL_DELETE_FOLDER: &str = "DELETE FROM folder WHERE id = $1 AND account_id = $2";

const DATETIME_UTC_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";
fn get_datetime_utc(data: String) -> DateTime<Utc> {
    chrono::DateTime::parse_from_str(&format!("{} +0000", data), DATETIME_UTC_FORMAT).unwrap().with_timezone(&Utc)
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

pub async fn create_user(conn: &Connection, username: String, encrypted_password: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_REGISTER).expect("Wrong login SQL");
    stmt.insert((&username, &encrypted_password))
        .map(|_| Ok(()))
        .unwrap_or_else(|_| Err(error::ErrorInternalServerError(crate::messages::ERROR_USERNAME_EXISTS)))
}

/// Get the account associated to the username and password.
/// It also returns a token on succes because auth is based on tokens
pub async fn get_user(conn: &Connection, username: String) -> Result<Account, Error> {
    log::info!("get_user: {}", username);
    let mut stmt = conn.prepare(SQL_LOGIN).expect("Wrong login SQL");
    stmt.query_row(&[(":u", &username)], |row| {
        log::info!("row");
        let mut account = Account {
            hash_id: encode_id(row.get(0)?),
            username: row.get(1)?,
            encrypted_password: row.get(2)?,
            config: row.get(3)?,
            created: get_datetime_utc(row.get(4)?), //chrono::Utc::now(), //created: row.get(4)?, //FIXME:
            token: String::with_capacity(64),
            token_created: chrono::Utc::now(),
        };
        //TODO: generate and add token
        account.token.push_str("Token ");
        log::info!("account: {:?}", account);
        Ok(account)
    }).map_err(error::ErrorInternalServerError)
}

/// Get the account assiciated to the provided token
pub async fn get_user_from_token(conn: &Connection, token: String) -> Result<Account, Error> {
    // TODO: consider using a `HashMap<Token, &Account>` to avoid frequent queries to DB (account should be cached somewhere too)
    let mut stmt = conn.prepare(SQL_AUTH_TOKEN).expect("Wrong login SQL");
    stmt.query_row([&token], |row| {
        Ok(Account {
            hash_id: encode_id(row.get(0)?),
            username: row.get(1)?,
            encrypted_password: row.get(2)?,
            config: row.get(3)?,
            created: get_datetime_utc(row.get(4)?),  //created: row.get(4)?,
            token: row.get(5)?,
            token_created: chrono::Utc::now(), //token_created: row.get(6)?,
        })
    }).map_err(|e|{
        log::error!("{}: {:?}", crate::messages::ERROR_WRONG_TOKEN, e);
        error::ErrorInternalServerError("WRONG_TOKEN")
    })
}

pub async fn delete_account(conn: &Connection, account_hid: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_DELETE_ACCOUNT).expect("Wrong create token SQL");
    stmt.execute([(decode_id(account_hid))]).map(|_| ()).map_err(|e|{
        log::error!("{}: {}", crate::messages::ERROR_DELETE_ACCOUNT, e);
        error::ErrorInternalServerError("CANNOT_DELETE_TOKEN")
    })
}

/// Create the token for the given account.
/// It also saves the client requesting it (like `Mozilla/5.0 (Windows NT 6.1; Win64; x64; rv:47.0) Gecko/20100101 Firefox/47.0`)
pub async fn create_token(conn: &Connection, account_id: i32, client: String) -> Result<String, Error> {
    let mut stmt = conn.prepare(SQL_CREATE_TOKEN).expect("Wrong create token SQL");
    stmt.query_row(&[(":a", &account_id.to_string()), (":v", &uuid::Uuid::new_v4().to_string())], |row| {
        row.get(0)
    }).map_err(|e|{
        log::error!("{}: {}", crate::messages::ERROR_CREATE_TOKEN, e);
        error::ErrorInternalServerError("CANNOT_CREATE_TOKEN")
    })
}

pub async fn create_folder(conn: &Connection, account_id: i32, name: String) -> Result<Folder, Error> {
    let mut stmt = conn
        .prepare(SQL_CREATE_FOLDER)
        .expect("Wrong create folder SQL");
    if stmt.execute((account_id, &name)).is_err() {
        log::error!("{}: {}", crate::messages::ERROR_CREATE_FOLDER, name);
    }
    stmt.query_row([], |row| {
        Ok(Folder {
            hash_id: encode_id(row.get(0)?),
            account_id: row.get(1)?,
            name: row.get(2)?,
        })
    })
    .map_err(error::ErrorInternalServerError)
}

pub async fn edit_folder(conn: &Connection, account_hid: String, folder_hid: String, name: String) -> Result<(), Error> {
    let mut stmt = conn
        .prepare(SQL_EDIT_FOLDER)
        .expect("Wrong edit folder SQL");
    let id = crate::decode_id(folder_hid);
    if stmt.execute((&name, id, crate::decode_id(account_hid))).is_err() {
        log::error!("{}: {}", crate::messages::ERROR_EDIT_FOLDER, id);
        return Err(error::ErrorInternalServerError("Cannot edit folder"));
    }
    Ok(())
}

/// Delete a folder from the folder and account hash IDs (double check)
pub async fn delete_folder(conn: &Connection, account_hid: String, folder_hid: String) -> Result<(), Error> {
    let mut stmt = conn
        .prepare(SQL_DELETE_FOLDER)
        .expect("Wrong delete folder SQL");
    let id = crate::decode_id(folder_hid);
    if stmt.execute((id, crate::decode_id(account_hid))).is_err() {
        log::error!("{}: {}", crate::messages::ERROR_DELETE_FOLDER, id);
        return Err(error::ErrorInternalServerError("Cannot delete folder"));
    }
    Ok(())
}
