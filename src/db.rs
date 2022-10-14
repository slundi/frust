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
