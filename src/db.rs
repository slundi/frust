
use std::{thread::sleep, time::Duration};

use actix_web::{error, web, Error};
use rusqlite::Statement;
use serde::{Deserialize, Serialize};

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
