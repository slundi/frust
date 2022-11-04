use actix_web::{error, Error};

use crate::{
    encode_id,
    model::{Account, Folder},
};

pub type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
pub type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

//example [here](https://github.com/actix/examples/tree/master/databases/sqlite)

#[allow(clippy::enum_variant_names)]
pub enum Queries {
    // user queries
    Login,
    Register,
    GetTokens,
    RevokeToken,
    DeleteAccount,

    //folder queries
    AddFolder,
    DeleteFolder,

    //feed queries
    AddFeed,
    DeleteFeed,
    DeleteUnusedFeeds,

    //article queries
    AddArticle,
    ReadArticle,
    SaveArticle,
    DeleteArticle,
    DeleteUnsavedOldArticles,
}

const SQL_LOGIN: &str = "SELECT id, username, encrypted_password, config, created FROM account WHERE username = $1";
const SQL_AUTH_TOKEN: &str = "SELECT a.*, t.value AS token, t.created AS token_created
                              FROM token t LEFT JOIN account a ON t.account_id = a.id
                              WHERE value = $1";
const SQL_REGISTER: &str = "INSERT INTO account (username, encrypted_password, config) VALUES ($1, $2, $3) RETURNING *";
const SQL_DELETE_ACCOUNT: &str = "DELETE FROM account WHERE id = $1";
/// Create a token, ignore it if it already exists
const SQL_CREATE_TOKEN: &str =
    "INSERT OR IGNORE INTO token (account_id, created, name) VALUES ($1, $2, $3) RETURNING *";
const SQL_GET_ACCOUNT_TOKENS: &str = "SELECT id, created, name FROM token WHERE account_id = $1";
const SQL_DELETE_TOKEN: &str = "DELETE FROM token WHERE id = $1";
const SQL_CREATE_FOLDER: &str ="INSERT INTO folder (name, account_id) VALUES ($1, $2) RETURNING id";
const SQL_EDIT_FOLDER: &str = "UPDATE folder SET name = $1 WHERE id = $2 AND account_id = $3";
const SQL_GET_MY_FOLDER: &str ="SELECT id, name FROM folder WHERE account_id = $1 ORDER BY name";
const SQL_DELETE_FOLDER: &str = "DELETE FROM folder WHERE id = $1 AND account_id = $2";

/// Create tables if not exists
pub(crate) fn create_schema(conn: Connection) {
    log::info!("Preparing DB schema import");
    let sql = std::fs::read_to_string(std::path::Path::new("sql/schema.sql"))
        .expect(crate::messages::ERROR_SCHEMA_FILE);
    let mut batch = rusqlite::Batch::new(&conn, &sql);
    while let Some(mut stmt) = batch.next().expect("Cannot execute next schema statement") {
        stmt.execute([]).expect("Cannot execute schema statement");
        log::info!("Table created!");
    }
}

/// Get the account associated to the username and password.
/// It also returns a token on succes because auth is based on tokens
pub async fn get_user(pool: &Pool, username: String) -> Result<Account, Error> {
    let conn = pool.get().map_err(error::ErrorInternalServerError)?;
    let mut stmt = conn.prepare(SQL_LOGIN).expect("Wrong login SQL");
    if stmt.execute([&username]).is_err() {
        log::error!("{}", crate::messages::ERROR_WRONG_USERNAME);
        return Err(error::ErrorInternalServerError(""));
    }
    stmt.query_row([], |row| {
        let mut account = Account {
            hash_id: encode_id(row.get(0)?),
            username: row.get(1)?,
            encrypted_password: row.get(2)?,
            config: row.get(3)?,
            created: row.get(4)?,
            token: String::with_capacity(64),
            token_created: chrono::Utc::now(),
        };
        //TODO: generate and add token
        account.token.push_str("Token ");
        Ok(account)
    })
    .map_err(error::ErrorInternalServerError)
}

/// Get the account assiciated to the provided token
pub async fn get_user_from_token(pool: &Pool, token: String) -> Result<Account, Error> {
    // TODO: consider using a `HashMap<Token, &Account>` to avoid frequent queries to DB (account should be cached somewhere too)
    let conn = pool.get().map_err(error::ErrorInternalServerError)?;
    let mut stmt = conn.prepare(SQL_AUTH_TOKEN).expect("Wrong login SQL");
    if stmt.execute([&token]).is_err() {
        log::error!("{}", crate::messages::ERROR_WRONG_TOKEN);
        return Err(error::ErrorInternalServerError(""));
    }
    stmt.query_row([], |row| {
        Ok(Account {
            hash_id: encode_id(row.get(0)?),
            username: row.get(1)?,
            encrypted_password: row.get(2)?,
            config: row.get(3)?,
            created: row.get(4)?,
            token: row.get(5)?,
            token_created: row.get(6)?,
        })
    })
    .map_err(error::ErrorInternalServerError)
}

/// Create the token for the given account.
/// It also saves the client requesting it (like `Mozilla/5.0 (Windows NT 6.1; Win64; x64; rv:47.0) Gecko/20100101 Firefox/47.0`)
pub async fn create_token(pool: &Pool, account_id: i32, client: String) {
    let conn = pool.get().expect("Cannot get connection pool");
    let mut stmt = conn
        .prepare(SQL_CREATE_TOKEN)
        .expect("Wrong create token SQL");
    let result = stmt.execute((account_id, &client));
    if let Err(e) = result {
        log::error!("{}: {}", crate::messages::ERROR_CREATE_TOKEN, e);
    }
}

pub async fn create_folder(pool: &Pool, account_id: i32, name: String) -> Result<Folder, Error> {
    let conn = pool.get().expect("Cannot get connection pool");
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

pub async fn edit_folder(pool: &Pool, account_hid: String, folder_hid: String, name: String) -> Result<(), Error> {
    let conn = pool.get().expect("Cannot get connection pool");
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
pub async fn delete_folder(pool: &Pool, account_hid: String, folder_hid: String) -> Result<(), Error> {
    let conn = pool.get().expect("Cannot get connection pool");
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
