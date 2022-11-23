use actix_web::{error, Error};
use rusqlite::params;
use crate::{model::Account, utils::{encode_id, decode_id}};

use super::{Connection, get_datetime_utc};

const SQL_LOGIN: &str = "SELECT id, username, encrypted_password, config, created FROM account WHERE username = :u";
const SQL_AUTH_TOKEN: &str = "SELECT a.id, a.username, a.encrypted_password, a.created, a.config, t.value AS token, t.created AS token_created
                              FROM token t INNER JOIN account a ON t.account_id = a.id
                              WHERE t.value = $1";
const SQL_REGISTER: &str = "INSERT INTO account (username, encrypted_password, config) VALUES ($1, $2, '')";
const SQL_DELETE_ACCOUNT: &str = "DELETE FROM account WHERE id = $1";
/// Create a token, ignore it if it already exists
const SQL_CREATE_TOKEN: &str = "INSERT OR IGNORE INTO token (account_id, value) VALUES (:a, :v) RETURNING value";
const SQL_GET_ACCOUNT_TOKENS: &str = "SELECT id, created, name FROM token WHERE account_id = $1";
const SQL_DELETE_TOKEN: &str = "DELETE FROM token WHERE account_id = $1 AND value = $2";

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
            config: row.get(4)?,
            created: get_datetime_utc(row.get(3)?),
            token: row.get(5)?,
            token_created: get_datetime_utc(row.get(6)?),
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
pub async fn create_token(conn: &Connection, account_id: i32) -> Result<String, Error> {
    let mut stmt = conn.prepare(SQL_CREATE_TOKEN).expect("Wrong create token SQL");
    stmt.query_row(&[(":a", &account_id.to_string()), (":v", &uuid::Uuid::new_v4().to_string())], |row| {
        row.get(0)
    }).map_err(|e|{
        log::error!("{}: {}", crate::messages::ERROR_CREATE_TOKEN, e);
        error::ErrorInternalServerError("CANNOT_CREATE_TOKEN")
    })
}

pub async fn delete_token(conn: &Connection, account_hid: String, token: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_DELETE_TOKEN).expect("Wrong create token SQL");
    stmt.execute(params![decode_id(account_hid), token]).map(|_| ()).map_err(|e|{
        log::error!("{}: {}", crate::messages::ERROR_DELETE_TOKEN, e);
        error::ErrorInternalServerError("CANNOT_DELETE_TOKEN")
    })
}