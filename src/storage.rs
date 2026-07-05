use crate::error::FrustError;
use crate::model::{Article, FeedState};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use regex::Regex;
use std::collections::{HashMap, HashSet};

const ARTICLES_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("articles");
const STATE_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("states");
/// Secondary index keyed by `(feed_id, article_id)` with the article's
/// `timestamp` as the value. Lets `delete_expired_articles` and
/// `load_articles_for_feed` skip the decompress+deserialize step for the
/// article payload — only the meta bytes (24 bytes/entry) are read.
const META_TABLE: TableDefinition<(u64, u64), i64> = TableDefinition::new("article_meta");

pub struct Storage {
    articles_db: Database,
    states_db: Database,
}

/// Read only the `added_at` field of a stored article by id, if present.
/// Currently pays for a full decompress+deserialize; a future secondary
/// index could avoid the payload read.
fn read_added_at<T>(table: &T, id: u64) -> Result<Option<i64>, FrustError>
where
    T: redb::ReadableTable<u64, &'static [u8]>,
{
    let Some(guard) = table.get(id)? else {
        return Ok(None);
    };
    let decompressed = lz4_flex::decompress_size_prepended(guard.value())
        .map_err(|e| FrustError::Serialization(e.to_string()))?;
    let archived = rkyv::access::<rkyv::Archived<Article>, rkyv::rancor::Error>(&decompressed)?;
    let existing: Article = rkyv::deserialize::<Article, rkyv::rancor::Error>(archived)?;
    Ok(Some(existing.added_at))
}

impl Storage {
    pub fn new(
        articles_path: impl AsRef<std::path::Path>,
        states_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, FrustError> {
        tracing::info!("Creating database files");
        let articles_db = Database::builder().create(articles_path.as_ref())?;
        let states_db = Database::builder().create(states_path.as_ref())?;
        let storage = Self {
            articles_db,
            states_db,
        };
        storage.backfill_meta_if_needed()?;
        Ok(storage)
    }

    /// Rebuild META_TABLE from ARTICLES_TABLE when the two are out of sync.
    /// This makes the schema upgrade transparent for databases created before
    /// the secondary index existed — the meta table only gets fully populated
    /// once, then upsert_articles keeps it in sync.
    fn backfill_meta_if_needed(&self) -> Result<(), FrustError> {
        let read_txn = self.articles_db.begin_read()?;
        let articles_len = match read_txn.open_table(ARTICLES_TABLE) {
            Ok(t) => t.len()?,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let meta_len = match read_txn.open_table(META_TABLE) {
            Ok(t) => t.len()?,
            Err(redb::TableError::TableDoesNotExist(_)) => 0,
            Err(e) => return Err(e.into()),
        };
        if articles_len == meta_len {
            return Ok(());
        }

        tracing::info!(
            "Rebuilding article meta index ({} articles, {} meta entries)",
            articles_len,
            meta_len
        );
        let articles_table = read_txn.open_table(ARTICLES_TABLE)?;
        let mut entries: Vec<(u64, u64, i64)> = Vec::with_capacity(articles_len as usize);
        for item in articles_table.iter()? {
            let (_key, bytes) = item?;
            let decompressed = lz4_flex::decompress_size_prepended(bytes.value())
                .map_err(|e| FrustError::Serialization(e.to_string()))?;
            let archived =
                rkyv::access::<rkyv::Archived<Article>, rkyv::rancor::Error>(&decompressed)?;
            let article: Article = rkyv::deserialize::<Article, rkyv::rancor::Error>(archived)?;
            entries.push((article.feed_id, article.id, article.timestamp));
        }
        drop(read_txn);

        let write_txn = self.articles_db.begin_write()?;
        {
            let mut meta = write_txn.open_table(META_TABLE)?;
            for (feed_id, article_id, timestamp) in entries {
                meta.insert((feed_id, article_id), timestamp)?;
            }
        }
        write_txn.commit()?;
        Ok(())
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
            let mut meta = write_txn.open_table(META_TABLE)?;
            for mut article in articles {
                // If the id already exists, preserve its original added_at so
                // "first seen" isn't rewritten to now on every re-fetch. The
                // filter layer normally dedups upstream, but this makes the
                // storage self-healing if dedup ever fails (e.g. the id-set
                // couldn't be loaded, or the source rewrote its GUIDs).
                if let Some(existing_added_at) = read_added_at(&table, article.id)? {
                    article.added_at = existing_added_at;
                }
                let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&article)?;
                let compressed = lz4_flex::compress_prepend_size(bytes.as_slice());
                table.insert(article.id, compressed.as_slice())?;
                meta.insert((article.feed_id, article.id), article.timestamp)?;
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
        // Walk the meta index instead of the articles table so we never
        // decompress a payload just to read its timestamp/feed_id.
        let to_delete: Vec<(u64, u64)> = match read_txn.open_table(META_TABLE) {
            Ok(table) => {
                let mut victims = Vec::new();
                for item in table.iter()? {
                    let (key, value) = item?;
                    let (feed_id, article_id) = key.value();
                    let timestamp = value.value();
                    let retention = feed_retentions
                        .get(&feed_id)
                        .copied()
                        .unwrap_or(default_retention);
                    if retention == 0 {
                        continue;
                    }
                    let cutoff = now_ts - retention as i64 * 86_400;
                    if timestamp <= cutoff {
                        victims.push((feed_id, article_id));
                    }
                }
                victims
            }
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        drop(read_txn);

        if to_delete.is_empty() {
            return Ok(0);
        }

        let write_txn = self.articles_db.begin_write()?;
        {
            let mut articles = write_txn.open_table(ARTICLES_TABLE)?;
            let mut meta = write_txn.open_table(META_TABLE)?;
            for (feed_id, article_id) in &to_delete {
                articles.remove(article_id)?;
                meta.remove((*feed_id, *article_id))?;
            }
        }
        write_txn.commit()?;
        Ok(to_delete.len())
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
    pub fn purge_orphaned_media(
        &self,
        media_dir: impl AsRef<std::path::Path>,
    ) -> Result<usize, FrustError> {
        let referenced = self.collect_media_refs()?;
        let media_path = media_dir.as_ref();
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
        let meta = match read_txn.open_table(META_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let articles_table = match read_txn.open_table(ARTICLES_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        // Range-query the composite (feed_id, article_id) key so we only touch
        // meta entries for this feed. Then load and decompress just the matching
        // article payloads — no scan of every article in every feed.
        let range = meta.range((feed_id, u64::MIN)..=(feed_id, u64::MAX))?;
        let mut articles = Vec::new();
        for item in range {
            let (key, _) = item?;
            let (_, article_id) = key.value();
            let Some(bytes) = articles_table.get(article_id)? else {
                continue;
            };
            let decompressed = lz4_flex::decompress_size_prepended(bytes.value())
                .map_err(|e| FrustError::Serialization(e.to_string()))?;
            let archived =
                rkyv::access::<rkyv::Archived<Article>, rkyv::rancor::Error>(&decompressed)?;
            let article: Article = rkyv::deserialize::<Article, rkyv::rancor::Error>(archived)?;
            articles.push(article);
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

    // ---- upsert_articles / added_at preservation ----

    fn load_added_at(storage: &Storage, id: u64) -> Option<i64> {
        let arts = storage
            .load_articles_for_feed(42)
            .unwrap()
            .into_iter()
            .chain(storage.load_articles_for_feed(0).unwrap())
            .collect::<Vec<_>>();
        arts.into_iter().find(|a| a.id == id).map(|a| a.added_at)
    }

    #[test]
    fn test_upsert_preserves_added_at_on_reinsert() {
        let storage = make_storage();
        // First insert: added_at = 100.
        let mut art = make_article(1, 42, 500);
        art.added_at = 100;
        storage.upsert_articles(vec![art]).unwrap();
        assert_eq!(load_added_at(&storage, 1), Some(100));

        // Second insert of the same id, later added_at — must NOT overwrite.
        let mut art2 = make_article(1, 42, 900);
        art2.added_at = 999;
        storage.upsert_articles(vec![art2]).unwrap();
        assert_eq!(
            load_added_at(&storage, 1),
            Some(100),
            "added_at must be preserved across re-inserts"
        );
    }

    #[test]
    fn test_upsert_new_id_uses_supplied_added_at() {
        let storage = make_storage();
        let mut art = make_article(7, 42, 500);
        art.added_at = 123;
        storage.upsert_articles(vec![art]).unwrap();
        assert_eq!(
            load_added_at(&storage, 7),
            Some(123),
            "brand-new id must keep the supplied added_at"
        );
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

    // ---- meta secondary-index behavior ----

    #[test]
    fn test_load_articles_for_feed_only_returns_matching_feed() {
        let storage = make_storage();
        // Two feeds, one article each. The meta range query must isolate
        // articles by feed_id — a naive full scan would return both.
        storage
            .upsert_articles(vec![
                make_article(1, 42, 1_000),
                make_article(2, 99, 2_000),
                make_article(3, 42, 3_000),
            ])
            .unwrap();
        let articles = storage.load_articles_for_feed(42).unwrap();
        let ids: Vec<u64> = articles.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![3, 1], "newest-first order, only feed 42");
    }

    #[test]
    fn test_load_articles_for_feed_empty_when_no_match() {
        let storage = make_storage();
        storage
            .upsert_articles(vec![make_article(1, 42, 1_000)])
            .unwrap();
        assert!(storage.load_articles_for_feed(777).unwrap().is_empty());
    }

    #[test]
    fn test_delete_expired_removes_meta_entries_too() {
        let storage = make_storage();
        let now = 1_000_000_i64;
        storage
            .upsert_articles(vec![
                make_article(1, 42, now - 10 * 86_400), // expired
                make_article(2, 42, now - 3 * 86_400),  // kept
            ])
            .unwrap();

        let mut retentions = HashMap::new();
        retentions.insert(42u64, 7u16);
        let deleted = storage
            .delete_expired_articles(now, &retentions, 0)
            .unwrap();
        assert_eq!(deleted, 1);

        // After deletion, load_articles_for_feed must not resurrect the
        // expired article — this catches a bug where the meta table is not
        // cleaned up in sync with ARTICLES_TABLE.
        let remaining = storage.load_articles_for_feed(42).unwrap();
        let ids: Vec<u64> = remaining.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![2]);
    }

    #[test]
    fn test_backfill_meta_from_legacy_articles_table() {
        // Simulate a legacy database where META_TABLE is empty but
        // ARTICLES_TABLE has entries — writing directly to the articles table
        // and then reopening Storage should trigger the backfill and let
        // load_articles_for_feed work without any explicit upgrade step.
        let articles_path = unique_path("articles");
        let states_path = unique_path("states");

        {
            let storage = Storage::new(&articles_path, &states_path).unwrap();
            storage
                .upsert_articles(vec![make_article(1, 42, 1_000)])
                .unwrap();
        }

        // Wipe the meta table to simulate the pre-index state.
        {
            let db = Database::builder().open(&articles_path).unwrap();
            let tx = db.begin_write().unwrap();
            let _ = tx.delete_table(META_TABLE);
            tx.commit().unwrap();
        }

        // Reopening triggers backfill_meta_if_needed → meta is rebuilt.
        let storage = Storage::new(&articles_path, &states_path).unwrap();
        let arts = storage.load_articles_for_feed(42).unwrap();
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0].id, 1);
    }
}
