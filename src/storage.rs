use crate::error::FrustError;
use crate::model::{Article, FeedState};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use regex::Regex;
use std::collections::{HashMap, HashSet};

const ARTICLES_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("articles");
const STATE_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("states");

pub struct Storage {
    articles_db: Database,
    states_db: Database,
}

impl Storage {
    pub fn new(articles_path: &str, states_path: &str) -> Result<Self, FrustError> {
        tracing::info!("Creating database files");
        let articles_db = Database::builder().create(articles_path)?;
        let states_db = Database::builder().create(states_path)?;
        Ok(Self {
            articles_db,
            states_db,
        })
    }

    /// Save a FeedState using rkyv 0.8
    pub fn save_feed_state(&self, feed_id: u64, state: &FeedState) -> Result<(), FrustError> {
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
    pub fn load_all_states(&self) -> Result<HashMap<u64, FeedState>, FrustError> {
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

    /// Return the set of all article IDs currently stored. Used to skip already-seen entries.
    pub fn load_article_ids(&self) -> Result<HashSet<u64>, FrustError> {
        let read_txn = self.articles_db.begin_read()?;
        match read_txn.open_table(ARTICLES_TABLE) {
            Ok(table) => {
                let ids = table
                    .iter()?
                    .map(|item| item.map(|(k, _)| k.value()))
                    .collect::<Result<_, _>>()?;
                Ok(ids)
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(HashSet::new()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn upsert_articles(&self, articles: Vec<Article>) -> Result<(), FrustError> {
        let write_txn = self.articles_db.begin_write()?;
        {
            let mut table = write_txn.open_table(ARTICLES_TABLE)?;
            for article in articles {
                // Serialize -> Compress -> Store
                let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&article)?;
                let compressed = lz4_flex::compress_prepend_size(bytes.as_slice());
                table.insert(article.id, compressed.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Delete articles that have exceeded their retention window.
    ///
    /// Returns the number of deleted articles.
    /// `now_ts` is the current UNIX timestamp in seconds.
    /// `feed_retentions` maps feed_id → retention in days (0 = keep forever).
    /// `default_retention` is used for articles whose feed_id is not in the map.
    pub fn delete_expired_articles(
        &self,
        now_ts: i64,
        feed_retentions: &HashMap<u64, u16>,
        default_retention: u16,
    ) -> Result<usize, FrustError> {
        let read_txn = self.articles_db.begin_read()?;
        let ids_to_delete: Vec<u64> = match read_txn.open_table(ARTICLES_TABLE) {
            Ok(table) => {
                let mut ids = Vec::new();
                for item in table.iter()? {
                    let (key, bytes) = item?;
                    let decompressed = lz4_flex::decompress_size_prepended(bytes.value())
                        .map_err(|e| FrustError::Serialization(e.to_string()))?;
                    let archived = rkyv::access::<rkyv::Archived<Article>, rkyv::rancor::Error>(
                        &decompressed,
                    )?;
                    let article: Article =
                        rkyv::deserialize::<Article, rkyv::rancor::Error>(archived)?;
                    let retention = feed_retentions
                        .get(&article.feed_id)
                        .copied()
                        .unwrap_or(default_retention);
                    if retention == 0 {
                        continue;
                    }
                    let cutoff = now_ts - retention as i64 * 86_400;
                    if article.timestamp <= cutoff {
                        ids.push(key.value());
                    }
                }
                ids
            }
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        drop(read_txn);

        if ids_to_delete.is_empty() {
            return Ok(0);
        }

        let write_txn = self.articles_db.begin_write()?;
        {
            let mut table = write_txn.open_table(ARTICLES_TABLE)?;
            for id in &ids_to_delete {
                table.remove(id)?;
            }
        }
        write_txn.commit()?;
        Ok(ids_to_delete.len())
    }

    /// Collect bare filenames (e.g. `"abc123def456789a.jpg"`) of every media asset
    /// referenced by stored articles — either in enclosure URLs or inline in content.
    pub fn collect_media_refs(&self) -> Result<HashSet<String>, FrustError> {
        let re = Regex::new(r"media/([0-9a-f]{16}\.[a-zA-Z0-9]{1,5})").unwrap();
        let read_txn = self.articles_db.begin_read()?;
        let mut refs = HashSet::new();
        match read_txn.open_table(ARTICLES_TABLE) {
            Ok(table) => {
                for item in table.iter()? {
                    let (_, bytes) = item?;
                    let decompressed = lz4_flex::decompress_size_prepended(bytes.value())
                        .map_err(|e| FrustError::Serialization(e.to_string()))?;
                    let archived = rkyv::access::<rkyv::Archived<Article>, rkyv::rancor::Error>(
                        &decompressed,
                    )?;
                    let article: Article =
                        rkyv::deserialize::<Article, rkyv::rancor::Error>(archived)?;
                    for enc in &article.enclosures {
                        for cap in re.captures_iter(&enc.url) {
                            refs.insert(cap[1].to_string());
                        }
                    }
                    for cap in re.captures_iter(&article.content) {
                        refs.insert(cap[1].to_string());
                    }
                }
            }
            Err(redb::TableError::TableDoesNotExist(_)) => {}
            Err(e) => return Err(e.into()),
        }
        Ok(refs)
    }

    /// Delete files in `media_dir` that are not referenced by any stored article.
    /// Returns the number of deleted files.
    pub fn purge_orphaned_media(&self, media_dir: &str) -> Result<usize, FrustError> {
        let referenced = self.collect_media_refs()?;
        let media_path = std::path::Path::new(media_dir);
        if !media_path.exists() {
            return Ok(0);
        }
        let mut deleted = 0;
        for entry in std::fs::read_dir(media_path)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = entry.file_name().to_string_lossy().into_owned();
            if !referenced.contains(&filename) {
                match std::fs::remove_file(&path) {
                    Ok(()) => deleted += 1,
                    Err(e) => tracing::warn!("Cannot delete orphaned media {}: {}", filename, e),
                }
            }
        }
        Ok(deleted)
    }

    /// Load all articles for a specific feed (e.g., to regenerate the RSS XML)
    pub fn load_articles_for_feed(&self, feed_id: u64) -> Result<Vec<Article>, FrustError> {
        tracing::info!("Loading articles for feed");
        let read_txn = self.articles_db.begin_read()?;
        let table = match read_txn.open_table(ARTICLES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut articles = Vec::new();

        for item in table.iter()? {
            let (_, bytes) = item?;

            // 1. Decompress (since articles ARE compressed)
            let decompressed = lz4_flex::decompress_size_prepended(bytes.value())
                .map_err(|e| FrustError::Serialization(e.to_string()))?;

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
        articles.sort_by_key(|a| std::cmp::Reverse(a.timestamp));
        Ok(articles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Article, Enclosure};

    fn unique_path(prefix: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("/tmp/frust_test_{}_{}.redb", prefix, nanos)
    }

    fn make_storage() -> Storage {
        Storage::new(&unique_path("articles"), &unique_path("states")).unwrap()
    }

    fn make_article(id: u64, feed_id: u64, timestamp: i64) -> Article {
        Article {
            id,
            feed_id,
            title: String::from("Test"),
            url: String::from("http://example.com"),
            content: String::from("Content"),
            summary: None,
            timestamp,
            added_at: timestamp,
            is_full_content: false,
            enclosures: Vec::<Enclosure>::new(),
        }
    }

    #[test]
    fn test_cleanup_empty_db_returns_zero() {
        let storage = make_storage();
        let deleted = storage
            .delete_expired_articles(1_000_000, &HashMap::new(), 7)
            .unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_cleanup_fresh_articles_kept() {
        let storage = make_storage();
        let now = 1_000_000_i64;
        storage
            .upsert_articles(vec![make_article(1, 42, now - 3 * 86_400)])
            .unwrap();

        let mut retentions = HashMap::new();
        retentions.insert(42u64, 7u16);
        let deleted = storage
            .delete_expired_articles(now, &retentions, 0)
            .unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(storage.load_article_ids().unwrap().len(), 1);
    }

    #[test]
    fn test_cleanup_expired_articles_removed() {
        let storage = make_storage();
        let now = 1_000_000_i64;
        // Article 1: 10 days old → expired (retention 7)
        // Article 2: 3 days old → kept
        storage
            .upsert_articles(vec![
                make_article(1, 42, now - 10 * 86_400),
                make_article(2, 42, now - 3 * 86_400),
            ])
            .unwrap();

        let mut retentions = HashMap::new();
        retentions.insert(42u64, 7u16);
        let deleted = storage
            .delete_expired_articles(now, &retentions, 0)
            .unwrap();
        assert_eq!(deleted, 1);
        let remaining = storage.load_article_ids().unwrap();
        assert!(remaining.contains(&2));
        assert!(!remaining.contains(&1));
    }

    #[test]
    fn test_cleanup_retention_zero_keeps_all() {
        let storage = make_storage();
        let now = 1_000_000_i64;
        storage
            .upsert_articles(vec![make_article(1, 42, now - 9_999 * 86_400)])
            .unwrap();

        let mut retentions = HashMap::new();
        retentions.insert(42u64, 0u16); // 0 = keep forever
        let deleted = storage
            .delete_expired_articles(now, &retentions, 0)
            .unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(storage.load_article_ids().unwrap().len(), 1);
    }

    #[test]
    fn test_cleanup_uses_default_retention_for_unknown_feed() {
        let storage = make_storage();
        let now = 1_000_000_i64;
        // feed_id=99 is not in the map; default_retention=7 → 10-day-old article expires
        storage
            .upsert_articles(vec![make_article(1, 99, now - 10 * 86_400)])
            .unwrap();

        let deleted = storage
            .delete_expired_articles(now, &HashMap::new(), 7)
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(storage.load_article_ids().unwrap().is_empty());
    }

    #[test]
    fn test_cleanup_at_exact_boundary_is_expired() {
        let storage = make_storage();
        let now = 1_000_000_i64;
        // Article exactly at cutoff (7 * 86400 seconds old) → expired (<=)
        storage
            .upsert_articles(vec![make_article(1, 42, now - 7 * 86_400)])
            .unwrap();

        let mut retentions = HashMap::new();
        retentions.insert(42u64, 7u16);
        let deleted = storage
            .delete_expired_articles(now, &retentions, 0)
            .unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn test_cleanup_one_second_before_boundary_is_kept() {
        let storage = make_storage();
        let now = 1_000_000_i64;
        // One second before cutoff → not expired
        storage
            .upsert_articles(vec![make_article(1, 42, now - 7 * 86_400 + 1)])
            .unwrap();

        let mut retentions = HashMap::new();
        retentions.insert(42u64, 7u16);
        let deleted = storage
            .delete_expired_articles(now, &retentions, 0)
            .unwrap();
        assert_eq!(deleted, 0);
    }

    // ---- collect_media_refs ----

    #[test]
    fn test_collect_media_refs_empty_db() {
        let storage = make_storage();
        assert!(storage.collect_media_refs().unwrap().is_empty());
    }

    #[test]
    fn test_collect_media_refs_from_enclosure_url() {
        let storage = make_storage();
        let mut article = make_article(1, 42, 1_000_000);
        article.enclosures = vec![Enclosure {
            url: "media/abcd1234abcd1234.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            length: None,
        }];
        storage.upsert_articles(vec![article]).unwrap();

        let refs = storage.collect_media_refs().unwrap();
        assert_eq!(refs.len(), 1);
        assert!(refs.contains("abcd1234abcd1234.jpg"));
    }

    #[test]
    fn test_collect_media_refs_from_content() {
        let storage = make_storage();
        let mut article = make_article(1, 42, 1_000_000);
        article.content = r#"<img src="media/deadbeefdeadbeef.png">"#.to_string();
        storage.upsert_articles(vec![article]).unwrap();

        let refs = storage.collect_media_refs().unwrap();
        assert!(refs.contains("deadbeefdeadbeef.png"));
    }

    #[test]
    fn test_collect_media_refs_deduplicates() {
        let storage = make_storage();
        let mut a1 = make_article(1, 42, 1_000_000);
        let mut a2 = make_article(2, 42, 1_000_000);
        a1.content = r#"media/abcd1234abcd1234.jpg"#.to_string();
        a2.enclosures = vec![Enclosure {
            url: "media/abcd1234abcd1234.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            length: None,
        }];
        storage.upsert_articles(vec![a1, a2]).unwrap();

        let refs = storage.collect_media_refs().unwrap();
        assert_eq!(refs.len(), 1);
    }

    // ---- purge_orphaned_media ----

    fn tmp_media_dir() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("/tmp/frust_media_{}", nanos)
    }

    #[test]
    fn test_purge_nonexistent_dir_returns_zero() {
        let storage = make_storage();
        let deleted = storage
            .purge_orphaned_media("/tmp/frust_no_such_dir_xyz")
            .unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_purge_deletes_unreferenced_keeps_referenced() {
        let storage = make_storage();
        let dir = tmp_media_dir();
        std::fs::create_dir_all(&dir).unwrap();

        let kept = "abcd1234abcd1234.jpg";
        let orphan = "deadbeefdeadbeef.png";
        std::fs::write(format!("{}/{}", dir, kept), b"data").unwrap();
        std::fs::write(format!("{}/{}", dir, orphan), b"data").unwrap();

        let mut article = make_article(1, 42, 1_000_000);
        article.enclosures = vec![Enclosure {
            url: format!("media/{}", kept),
            mime_type: "image/jpeg".to_string(),
            length: None,
        }];
        storage.upsert_articles(vec![article]).unwrap();

        let deleted = storage.purge_orphaned_media(&dir).unwrap();
        assert_eq!(deleted, 1);
        assert!(std::path::Path::new(&format!("{}/{}", dir, kept)).exists());
        assert!(!std::path::Path::new(&format!("{}/{}", dir, orphan)).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_purge_all_files_orphaned() {
        let storage = make_storage();
        let dir = tmp_media_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{}/abcd1234abcd1234.jpg", dir), b"data").unwrap();
        std::fs::write(format!("{}/deadbeefdeadbeef.png", dir), b"data").unwrap();

        // No articles → all files are orphans
        let deleted = storage.purge_orphaned_media(&dir).unwrap();
        assert_eq!(deleted, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_purge_all_files_referenced() {
        let storage = make_storage();
        let dir = tmp_media_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let filename = "abcd1234abcd1234.jpg";
        std::fs::write(format!("{}/{}", dir, filename), b"data").unwrap();

        let mut article = make_article(1, 42, 1_000_000);
        article.enclosures = vec![Enclosure {
            url: format!("media/{}", filename),
            mime_type: "image/jpeg".to_string(),
            length: None,
        }];
        storage.upsert_articles(vec![article]).unwrap();

        let deleted = storage.purge_orphaned_media(&dir).unwrap();
        assert_eq!(deleted, 0);
        assert!(std::path::Path::new(&format!("{}/{}", dir, filename)).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
