use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::BufWriter,
    path::Path,
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use slug::slugify;

use crate::{
    error::FrustError,
    model::{Article, ExportStrategy},
};

use super::Exporter;

pub(crate) struct JsonExporter {
    pub(crate) strategy: ExportStrategy,
}

// ── serialization DTOs (JSON Feed 1.1) ────────────────────────────────────────

/// https://www.jsonfeed.org/version/1.1/ — attachment object
#[derive(Serialize)]
struct AttachmentDto<'a> {
    url: &'a str,
    mime_type: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_in_bytes: Option<u64>,
}

/// JSON Feed 1.1 item object
#[derive(Serialize)]
struct ItemDto<'a> {
    /// Unique string identifier for the item; we use the article URL.
    id: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    url: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    title: &'a str,
    /// Markdown content maps to `content_text` (plain text per spec).
    #[serde(skip_serializing_if = "str::is_empty")]
    content_text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<&'a str>,
    /// RFC 3339 publication date derived from the feed timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    date_published: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<AttachmentDto<'a>>,
}

/// JSON Feed 1.1 top-level feed object (used for Monolithic strategy)
#[derive(Serialize)]
struct FeedDto<'a> {
    version: &'static str,
    title: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    home_page_url: &'a str,
    items: Vec<ItemDto<'a>>,
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn to_item(article: &Article) -> ItemDto<'_> {
    let date_published = if article.timestamp != 0 {
        DateTime::<Utc>::from_timestamp(article.timestamp, 0).map(|dt| dt.to_rfc3339())
    } else {
        None
    };
    ItemDto {
        id: &article.url,
        url: &article.url,
        title: &article.title,
        content_text: &article.content,
        summary: article.summary.as_deref(),
        date_published,
        attachments: article
            .enclosures
            .iter()
            .map(|e| AttachmentDto {
                url: &e.url,
                mime_type: &e.mime_type,
                size_in_bytes: e.length,
            })
            .collect(),
    }
}

fn article_filename(article: &Article) -> String {
    let date = DateTime::<Utc>::from_timestamp(article.timestamp, 0)
        .unwrap_or_default()
        .format("%Y-%m-%d")
        .to_string();
    let title_slug = slugify(&article.title);
    let title_slug = if title_slug.is_empty() {
        article.id.to_string()
    } else {
        title_slug
    };
    format!("{date}-{title_slug}.json")
}

fn write_json<T: Serialize>(value: &T, path: &Path) -> Result<(), FrustError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(BufWriter::new(file), value)
        .map_err(|e| FrustError::Export(e.to_string()))
}

// ── strategies ────────────────────────────────────────────────────────────────

impl Exporter for JsonExporter {
    fn generate(
        &self,
        articles: &[Article],
        title: &str,
        link: &str,
        destination: &Path,
    ) -> Result<(), FrustError> {
        match self.strategy {
            ExportStrategy::Monolithic => monolithic(articles, title, link, destination),
            ExportStrategy::Individual => individual(articles, destination),
            ExportStrategy::Daily => daily(articles, destination),
        }
    }
}

/// Produces a complete JSON Feed 1.1 document.
fn monolithic(
    articles: &[Article],
    title: &str,
    link: &str,
    destination: &Path,
) -> Result<(), FrustError> {
    let feed = FeedDto {
        version: "https://jsonfeed.org/version/1.1",
        title,
        home_page_url: link,
        items: articles.iter().map(to_item).collect(),
    };
    write_json(&feed, destination)
}

/// Produces one JSON file per article (a single JSON Feed item object each).
fn individual(articles: &[Article], destination: &Path) -> Result<(), FrustError> {
    fs::create_dir_all(destination)?;
    let mut used: HashMap<String, u32> = HashMap::new();
    for article in articles {
        let base = article_filename(article);
        let stem = base.trim_end_matches(".json").to_string();
        let idx = used.entry(stem.clone()).or_insert(0);
        let filename = if *idx == 0 {
            format!("{stem}.json")
        } else {
            format!("{stem}-{idx}.json")
        };
        *idx += 1;
        let path = destination.join(filename);
        let file = fs::File::create(&path)?;
        serde_json::to_writer_pretty(BufWriter::new(file), &to_item(article))
            .map_err(|e| FrustError::Export(e.to_string()))?;
    }
    Ok(())
}

/// Produces one JSON file per day (an array of JSON Feed item objects each).
fn daily(articles: &[Article], destination: &Path) -> Result<(), FrustError> {
    fs::create_dir_all(destination)?;
    let mut by_day: BTreeMap<String, Vec<&Article>> = BTreeMap::new();
    for article in articles {
        let day = DateTime::<Utc>::from_timestamp(article.timestamp, 0)
            .unwrap_or_default()
            .format("%Y-%m-%d")
            .to_string();
        by_day.entry(day).or_default().push(article);
    }
    for (day, day_articles) in &by_day {
        let path = destination.join(format!("{day}.json"));
        let file = fs::File::create(&path)?;
        let items: Vec<ItemDto> = day_articles.iter().map(|a| to_item(a)).collect();
        serde_json::to_writer_pretty(BufWriter::new(file), &items)
            .map_err(|e| FrustError::Export(e.to_string()))?;
    }
    Ok(())
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::TempDir;

    fn make_article(id: u64, title: &str, url: &str, ts: i64) -> Article {
        Article {
            id,
            feed_id: 1,
            title: title.to_string(),
            url: url.to_string(),
            content: String::new(),
            summary: None,
            timestamp: ts,
            added_at: ts,
            is_full_content: false,
            enclosures: vec![],
        }
    }

    fn parse(path: &Path) -> Value {
        let s = fs::read_to_string(path).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn test_monolithic_empty() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.json");
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&[], "My Feed", "https://example.com", &dest)
        .unwrap();
        let v = parse(&dest);
        assert_eq!(
            v["version"].as_str().unwrap(),
            "https://jsonfeed.org/version/1.1"
        );
        assert_eq!(v["title"].as_str().unwrap(), "My Feed");
        assert_eq!(v["home_page_url"].as_str().unwrap(), "https://example.com");
        assert!(v["items"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_monolithic_article_fields() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.json");
        let articles = vec![make_article(
            42,
            "Test Article",
            "https://example.com/42",
            1_705_276_800,
        )];
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let v = parse(&dest);
        let item = &v["items"][0];
        assert_eq!(item["id"].as_str().unwrap(), "https://example.com/42");
        assert_eq!(item["url"].as_str().unwrap(), "https://example.com/42");
        assert_eq!(item["title"].as_str().unwrap(), "Test Article");
        assert_eq!(
            item["date_published"].as_str().unwrap(),
            "2024-01-15T00:00:00+00:00"
        );
        // internal fields must not be present
        assert!(item["timestamp"].is_null());
        assert!(item["added_at"].is_null());
        assert!(item["is_full_content"].is_null());
    }

    #[test]
    fn test_monolithic_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("sub").join("dir").join("feed.json");
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&[], "Feed", "https://example.com", &dest)
        .unwrap();
        assert!(dest.exists());
    }

    #[test]
    fn test_monolithic_summary_optional() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.json");
        let mut article = make_article(1, "A", "https://example.com/a", 0);
        article.summary = Some("A summary".to_string());
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&[article], "Feed", "https://example.com", &dest)
        .unwrap();
        let v = parse(&dest);
        assert_eq!(v["items"][0]["summary"].as_str().unwrap(), "A summary");
    }

    #[test]
    fn test_monolithic_enclosure_fields() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.json");
        let mut article = make_article(1, "Podcast", "https://example.com/ep1", 0);
        article.enclosures.push(crate::model::Enclosure {
            url: "https://example.com/ep1.mp3".to_string(),
            mime_type: "audio/mpeg".to_string(),
            length: Some(4096),
        });
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&[article], "Podcast Feed", "https://example.com", &dest)
        .unwrap();
        let v = parse(&dest);
        let att = &v["items"][0]["attachments"][0];
        assert_eq!(att["url"].as_str().unwrap(), "https://example.com/ep1.mp3");
        assert_eq!(att["mime_type"].as_str().unwrap(), "audio/mpeg");
        assert_eq!(att["size_in_bytes"].as_u64().unwrap(), 4096);
    }

    #[test]
    fn test_individual_creates_files() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("articles");
        let articles = vec![
            make_article(1, "Article One", "https://example.com/1", 1_705_276_800),
            make_article(2, "Article Two", "https://example.com/2", 1_705_276_800),
        ];
        JsonExporter {
            strategy: ExportStrategy::Individual,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 2);
    }

    #[test]
    fn test_individual_file_is_single_object() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("articles");
        let articles = vec![make_article(
            7,
            "My Article",
            "https://example.com/my-article",
            1_705_276_800,
        )];
        JsonExporter {
            strategy: ExportStrategy::Individual,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let path = dest.join("2024-01-15-my-article.json");
        let v = parse(&path);
        assert_eq!(v["id"].as_str().unwrap(), "https://example.com/my-article");
        assert_eq!(v["title"].as_str().unwrap(), "My Article");
    }

    #[test]
    fn test_individual_dedup_slugs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("articles");
        let articles = vec![
            make_article(1, "Same Title", "https://example.com/1", 1_705_276_800),
            make_article(2, "Same Title", "https://example.com/2", 1_705_276_800),
        ];
        JsonExporter {
            strategy: ExportStrategy::Individual,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let mut files: Vec<String> = fs::read_dir(&dest)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files.len(), 2);
        assert_ne!(files[0], files[1], "duplicate filenames: {:?}", files);
    }

    #[test]
    fn test_daily_groups_by_date() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("daily");
        let articles = vec![
            make_article(1, "Morning", "https://example.com/1", 1_705_276_800), // 2024-01-15
            make_article(2, "Evening", "https://example.com/2", 1_705_363_200), // 2024-01-16
        ];
        JsonExporter {
            strategy: ExportStrategy::Daily,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let mut files: Vec<String> = fs::read_dir(&dest)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "2024-01-15.json");
        assert_eq!(files[1], "2024-01-16.json");
    }

    #[test]
    fn test_daily_file_is_array() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("daily");
        let articles = vec![
            make_article(1, "First", "https://example.com/1", 1_705_276_800),
            make_article(2, "Second", "https://example.com/2", 1_705_276_800),
        ];
        JsonExporter {
            strategy: ExportStrategy::Daily,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let v = parse(&dest.join("2024-01-15.json"));
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }
}
