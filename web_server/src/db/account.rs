use actix_web::{error, Error};
use rusqlite::params;
use crate::{utils::{encode_id, decode_id}};

use super::{Connection, get_datetime_utc, Account, Token};

const SQL_LOGIN: &str = "SELECT id, username, encrypted_password, config, created FROM account WHERE username = :u";
const SQL_AUTH_TOKEN: &str = "SELECT a.id, a.username, a.encrypted_password, a.created, a.config, t.value AS token, t.created AS token_created
                              FROM token t INNER JOIN account a ON t.account_id = a.id
                              WHERE t.value = $1";
const SQL_REGISTER: &str = "INSERT INTO account (username, encrypted_password, config) VALUES ($1, $2, '') RETURNING id";
const SQL_DELETE_ACCOUNT: &str = "DELETE FROM account WHERE id = $1";
/// Create a token, ignore it if it already exists
const SQL_CREATE_TOKEN: &str = "INSERT OR IGNORE INTO token (account_id, value) VALUES (:a, :v) RETURNING value";
const SQL_GET_ACCOUNT_TOKENS: &str = "SELECT created, value FROM token WHERE account_id = $1 ORDER by created DESC";
const SQL_RENEW_TOKEN: &str = "UPDATE token SET value = :new, created = DATETIME('now') WHERE account_id = :a AND value = :old";
const SQL_DELETE_TOKEN: &str = "DELETE FROM token WHERE account_id = $1 AND value = $2";

pub async fn create_user(conn: &Connection, username: String, encrypted_password: String) -> Result<i32, Error> {
    let mut stmt = conn.prepare(SQL_REGISTER).expect("Wrong login SQL");
    stmt.query_row([&username, &encrypted_password], |row| row.get(0))
        .map_err(|_| error::ErrorInternalServerError(crate::messages::ERROR_USERNAME_EXISTS))
}

/// Get the account associated to the username and password.
/// It also returns a token on succes because auth is based on tokens
pub async fn get_user(conn: &Connection, username: String) -> Result<Account, Error> {
    let mut stmt = conn.prepare_cached(SQL_LOGIN).expect("Wrong login SQL");
    stmt.query_row(&[(":u", &username)], |row| {
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
        Ok(account)
    }).map_err(error::ErrorInternalServerError)
}

/// Get the account assiciated to the provided token
pub async fn get_user_from_token(conn: &Connection, token: String) -> Result<Account, Error> {
    // TODO: consider using a `HashMap<Token, &Account>` to avoid frequent queries to DB (account should be cached somewhere too)
    let mut stmt = conn.prepare_cached(SQL_AUTH_TOKEN).expect("Wrong login SQL");
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
        log::error!("{}: {:?}", crate::messages::ERROR_WRONG_TOKEN, e); //e == rusqlite::Error::QueryReturnedNoRows
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
    let mut stmt = conn.prepare_cached(SQL_CREATE_TOKEN).expect("Wrong create token SQL");
    stmt.query_row(&[(":a", &account_id.to_string()), (":v", &uuid::Uuid::new_v4().to_string())], |row| {
        row.get(0)
    }).map_err(|e|{
        log::error!("{}: {}", crate::messages::ERROR_CREATE_TOKEN, e);
        error::ErrorInternalServerError("CANNOT_CREATE_TOKEN")
    })
}

/// Renew the token by updating the one used. It also updates the crated date in the SQL statement.
pub async fn renew_token(conn: &Connection, account_hid: String, token: String) -> Result<String, Error> {
    let mut stmt = conn.prepare_cached(SQL_RENEW_TOKEN).expect("Wrong renew token SQL");
    let new_token = uuid::Uuid::new_v4().to_string();
    stmt.execute(&[(":a", &decode_id(account_hid).to_string()), (":new", &new_token.clone()), (":old", &token)])
    .map(|_| new_token)
    .map_err(|e|{
        log::error!("{}: {:?}", crate::messages::ERROR_RENEW_TOKEN, e);
        error::ErrorInternalServerError("RENEW_TOKEN")
    })
}

pub async fn delete_token(conn: &Connection, account_hid: String, token: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_DELETE_TOKEN).expect("Wrong delete token SQL");
    stmt.execute(params![decode_id(account_hid), token]).map(|_| ()).map_err(|e|{
        log::error!("{}: {}", crate::messages::ERROR_DELETE_TOKEN, e);
        error::ErrorInternalServerError("CANNOT_DELETE_TOKEN")
    })
}

pub async fn get_tokens(conn: &Connection, account_hid: String) -> Result<Vec<Token>, Error> {
    let mut stmt = conn.prepare_cached(SQL_GET_ACCOUNT_TOKENS).expect("Wrong delete token SQL");
    let result = stmt.query_map([decode_id(account_hid)], |r| {
        Ok(Token {
            value: r.get(1).unwrap(),
            created: get_datetime_utc(r.get(0).unwrap())
        })
    });
    if let Err(e) = result {
        log::error!("{}: {}", crate::messages::ERROR_LIST_TOKENS, e);
        return Err(error::ErrorInternalServerError("CANNOT_LIST_TOKENS"))
    }
    let rows = result.unwrap();
    let mut results: Vec<Token> = Vec::new();
    for t in rows {
        results.push(t.unwrap());
    }
    Ok(results)
}
