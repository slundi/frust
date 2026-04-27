use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum FrustError {
    /// YAML config is missing a required field or contains an invalid value.
    #[error("Config error: {0}")]
    Config(String),

    /// Filesystem operation failed (create dir, read dir, remove file…).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP request or response failed.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// redb: opening / creating the database file.
    #[error("Database error: {0}")]
    DbOpen(#[from] redb::DatabaseError),

    /// redb: begin_read / begin_write.
    #[error("Transaction error: {0}")]
    DbTransaction(#[from] redb::TransactionError),

    /// redb: open_table.
    #[error("Table error: {0}")]
    DbTable(#[from] redb::TableError),

    /// redb: iter, insert, remove and other low-level ops.
    #[error("Storage I/O error: {0}")]
    DbStorage(#[from] redb::StorageError),

    /// redb: commit.
    #[error("Commit error: {0}")]
    DbCommit(#[from] redb::CommitError),

    /// rkyv serialization / deserialization or zstd compression failed.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// feed-rs XML / Atom parsing failed.
    #[error("Feed parse error: {0}")]
    FeedParse(String),

    /// RSS/XML generation failed.
    #[error("Export error: {0}")]
    Export(String),

    /// A process-global (START_TIME, HTTP client) was accessed before it was set.
    #[error("{0} not initialized")]
    NotInitialized(&'static str),
}

// rkyv::rancor::Error wraps a `Box<dyn Display>` and does not implement
// `std::error::Error`, so we cannot use `#[from]` and convert via Display.
impl From<rkyv::rancor::Error> for FrustError {
    fn from(e: rkyv::rancor::Error) -> Self {
        Self::Serialization(e.to_string())
    }
}
