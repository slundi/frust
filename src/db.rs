use actix_web::{web, error, Error};

use crate::model::Account;

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

pub(crate) fn create_schema(conn: Connection) {
    log::info!("Preparing DB schema import");
    let sql = std::fs::read_to_string(std::path::Path::new("sql/schema.sql")).expect("Cannot read schema file");
    let mut batch = rusqlite::Batch::new(&conn, &sql);
    while let Some(mut stmt) = batch.next().expect("Cannot execute next schema statement") {
        stmt.execute([]).expect("Cannot execute schema statement");
        log::info!("Table created!");
    }
}

pub async fn login(pool: &Pool, username: String, password: String) -> Result<Account, Error> {
    let conn = pool.get()
        .map_err(error::ErrorInternalServerError)?;
        let mut stmt = conn.prepare("SELECT id, slug, username, password, config FROM account WHERE username = ?1").expect("Wrong login SQL");
        stmt.execute([&username]);
        stmt.query_row([], |row| {
            Ok(Account {
                id: row.get(0).expect("msg"),
                slug: row.get(1)?,
                username: row.get(2)?,
                encrypted_password: row.get(3)?,
                config: row.get(4)?,
            })
        })
        .map_err(error::ErrorInternalServerError)
}
