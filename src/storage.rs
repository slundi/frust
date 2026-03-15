use crate::model::{Article, FeedState};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::{collections::HashMap, io::Cursor};

const ARTICLES_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("articles");
const STATE_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("states");
const MEDIA_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("media");

pub struct Storage {
    articles_db: Database,
    states_db: Database,
    media_db: Database,
}

impl Storage {
    pub fn new(
        articles_path: &str,
        states_path: &str,
        media_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        tracing::info!("Creating database files");
        let articles_db = Database::builder().create(articles_path)?;
        let states_db = Database::builder().create(states_path)?;
        let media_db = Database::builder().create(media_path)?;
        Ok(Self {
            articles_db,
            states_db,
            media_db,
        })
    }

    /// Save a FeedState using rkyv 0.8
    pub fn save_feed_state(
        &self,
        feed_id: u64,
        state: &FeedState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("Saving feed state");
        let write_txn = self.states_db.begin_write()?;
        {
            let mut table = write_txn.open_table(STATE_TABLE)?;

            // rkyv 0.8: to_bytes returns a Pooled<AlignedVec>
            // We use the default API which requires specifying an error type (rancor::Error)
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(state)?;

            table.insert(feed_id, bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load all states using rkyv 0.8 access API
    pub fn load_all_states(&self) -> Result<HashMap<u64, FeedState>, Box<dyn std::error::Error>> {
        tracing::info!("Loading feed state");
        let read_txn = self.states_db.begin_read()?;
        let table = read_txn.open_table(STATE_TABLE)?;
        let mut states = HashMap::new();

        for item in table.iter()? {
            let (id, bytes) = item?;

            // rkyv 0.8: access provides a zero-copy view of the bytes
            // It requires the Archived version of the struct
            let bytes_slice = bytes.value();
            let archived =
                rkyv::access::<rkyv::Archived<FeedState>, rkyv::rancor::Error>(bytes_slice)?;

            // To get an owned FeedState back from the archived view
            let state: FeedState = rkyv::deserialize::<FeedState, rkyv::rancor::Error>(archived)?;

            states.insert(id.value(), state);
        }
        Ok(states)
    }

    pub fn upsert_articles(
        &self,
        articles: Vec<Article>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let write_txn = self.articles_db.begin_write()?;
        {
            let mut table = write_txn.open_table(ARTICLES_TABLE)?;
            for article in articles {
                // Serialize -> Compress -> Store
                let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&article)?;
                let compressed = zstd::encode_all(Cursor::new(bytes.as_slice()), 3)?;
                table.insert(article.id, compressed.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load all articles for a specific feed (e.g., to regenerate the RSS XML)
    pub fn load_articles_for_feed(
        &self,
        feed_id: u64,
    ) -> Result<Vec<Article>, Box<dyn std::error::Error>> {
        tracing::info!("Loading articles for feed");
        let read_txn = self.articles_db.begin_read()?;
        let table = read_txn.open_table(ARTICLES_TABLE)?;
        let mut articles = Vec::new();

        for item in table.iter()? {
            let (_, bytes) = item?;

            // 1. Decompress (since articles ARE compressed)
            let decompressed = zstd::decode_all(std::io::Cursor::new(bytes.value()))?;

            // 2. Deserialize
            let archived =
                rkyv::access::<rkyv::Archived<Article>, rkyv::rancor::Error>(&decompressed)?;
            let article: Article = rkyv::deserialize::<Article, rkyv::rancor::Error>(archived)?;

            // 3. Filter by feed_id
            if article.feed_id == feed_id {
                articles.push(article);
            }
        }

        // Sort by date (descending) to have newest articles first in the RSS
        articles.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(articles)
    }

    pub fn save_media(&self, data: &[u8]) -> Result<u64, Box<dyn std::error::Error>> {
        tracing::info!("Saving media");
        let hash = xxhash_rust::xxh3::xxh3_64(data);

        let write_txn = self.articles_db.begin_write()?;
        {
            let mut table = write_txn.open_table(MEDIA_TABLE)?;
            // insert only if not existing (avoid duplicates)
            if table.get(hash)?.is_none() {
                table.insert(hash, data)?;
            }
        }
        write_txn.commit()?;
        Ok(hash)
    }

    /// Load media from its hash
    pub fn load_media(&self, hash: u64) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        tracing::info!("Loading media {}", hash);
        let read_txn = self.articles_db.begin_read()?;
        let table = read_txn.open_table(MEDIA_TABLE)?;

        if let Some(access) = table.get(hash)? {
            // return a copy of bytes
            Ok(Some(access.value().to_vec()))
        } else {
            Ok(None)
        }
    }

    /// Load all media from a hash list (to export or for the cache)
    pub fn load_multiple_media(
        &self,
        hashes: &[u64],
    ) -> Result<HashMap<u64, Vec<u8>>, Box<dyn std::error::Error>> {
        tracing::info!("Loading multiple media");
        let read_txn = self.articles_db.begin_read()?;
        let table = read_txn.open_table(MEDIA_TABLE)?;
        let mut results = HashMap::new();

        for &hash in hashes {
            if let Some(access) = table.get(hash)? {
                results.insert(hash, access.value().to_vec());
            }
        }
        Ok(results)
    }
}
