use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use slug::slugify;
use tracing::info;

use crate::{
    error::FrustError,
    model::{Article, ExportStrategy},
};

use super::{
    Enrichment, ExportJob, Exporter, article_date, article_urn, enriched_html, write_atomically,
};

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
    /// Unique string identifier for the item, see [`article_urn`].
    id: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    url: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    title: &'a str,
    /// Article body rendered from Markdown to HTML — the correct JSON Feed
    /// 1.1 field for structured content (spec §content_html). Raw HTML and
    /// unsafe link schemes are neutralised, see [`super::html::markdown_to_safe_html`].
    #[serde(skip_serializing_if = "String::is_empty")]
    content_html: String,
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
    /// URL of this feed file (JSON Feed `feed_url`); `home_page_url` would be
    /// the website, which a group of feeds does not have.
    #[serde(skip_serializing_if = "str::is_empty")]
    feed_url: &'a str,
    items: Vec<ItemDto<'a>>,
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn to_item<'a>(article: &'a Article, enrichment: Option<&Enrichment>) -> ItemDto<'a> {
    let date_published =
        DateTime::<Utc>::from_timestamp(article_date(article), 0).map(|dt| dt.to_rfc3339());
    ItemDto {
        id: article_urn(article),
        url: &article.url,
        title: &article.title,
        content_html: enriched_html(article, enrichment),
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
    write_atomically(path, |out| {
        serde_json::to_writer_pretty(out, value).map_err(|e| FrustError::Export(e.to_string()))
    })
}

// ── strategies ────────────────────────────────────────────────────────────────

impl Exporter for JsonExporter {
    fn generate(&self, job: &ExportJob<'_>) -> Result<(), FrustError> {
        let ExportJob {
            articles,
            title,
            link,
            destination,
            enrichments,
        } = *job;
        info!("Exporting to JSON");
        match self.strategy {
            ExportStrategy::Monolithic => {
                monolithic(articles, title, link, destination, enrichments)
            }
            ExportStrategy::Individual => individual(articles, destination, enrichments),
            ExportStrategy::Daily => daily(articles, destination, enrichments),
        }
    }
}

/// Produces a complete JSON Feed 1.1 document.
fn monolithic(
    articles: &[Article],
    title: &str,
    link: &str,
    destination: &Path,
    enrichments: &HashMap<u64, Enrichment>,
) -> Result<(), FrustError> {
    let feed = FeedDto {
        version: "https://jsonfeed.org/version/1.1",
        title,
        feed_url: link,
        items: articles
            .iter()
            .map(|a| to_item(a, enrichments.get(&a.feed_id)))
            .collect(),
    };
    write_json(&feed, destination)
}

/// Produces one JSON file per article (a single JSON Feed item object each).
fn individual(
    articles: &[Article],
    destination: &Path,
    enrichments: &HashMap<u64, Enrichment>,
) -> Result<(), FrustError> {
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
        write_json(&to_item(article, enrichments.get(&article.feed_id)), &path)?;
    }
    Ok(())
}

/// Produces one JSON file per day (an array of JSON Feed item objects each).
fn daily(
    articles: &[Article],
    destination: &Path,
    enrichments: &HashMap<u64, Enrichment>,
) -> Result<(), FrustError> {
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
        let items: Vec<ItemDto> = day_articles
            .iter()
            .map(|a| to_item(a, enrichments.get(&a.feed_id)))
            .collect();
        write_json(&items, &path)?;
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
            enclosures: vec![],
        }
    }

    fn no_enrichment() -> HashMap<u64, Enrichment> {
        HashMap::new()
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
        .generate(&ExportJob {
            articles: &[],
            title: "My Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
        .unwrap();
        let v = parse(&dest);
        assert_eq!(
            v["version"].as_str().unwrap(),
            "https://jsonfeed.org/version/1.1"
        );
        assert_eq!(v["title"].as_str().unwrap(), "My Feed");
        assert_eq!(v["feed_url"].as_str().unwrap(), "https://example.com");
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
        .generate(&ExportJob {
            articles: &articles,
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
        .unwrap();
        let v = parse(&dest);
        let item = &v["items"][0];
        assert_eq!(item["id"].as_str().unwrap(), "urn:xxh3:000000000000002a");
        assert_eq!(item["url"].as_str().unwrap(), "https://example.com/42");
        assert_eq!(item["title"].as_str().unwrap(), "Test Article");
        assert_eq!(
            item["date_published"].as_str().unwrap(),
            "2024-01-15T00:00:00+00:00"
        );
        // internal fields must not be present
        assert!(item["timestamp"].is_null());
        assert!(item["added_at"].is_null());
    }

    #[test]
    fn test_monolithic_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("sub").join("dir").join("feed.json");
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&ExportJob {
            articles: &[],
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
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
        .generate(&ExportJob {
            articles: &[article],
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
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
        .generate(&ExportJob {
            articles: &[article],
            title: "Podcast Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
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
        .generate(&ExportJob {
            articles: &articles,
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
        .unwrap();
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 2);
    }

    #[test]
    fn test_item_id_does_not_change_with_url() {
        let tracked = make_article(9, "T", "https://example.com/a?utm=x", 1);
        let clean = make_article(9, "T", "https://example.com/a", 1);
        let first = to_item(&tracked, None);
        let moved = to_item(&clean, None);
        assert_eq!(first.id, "urn:xxh3:0000000000000009");
        assert_eq!(first.id, moved.id);
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
        .generate(&ExportJob {
            articles: &articles,
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
        .unwrap();
        let path = dest.join("2024-01-15-my-article.json");
        let v = parse(&path);
        assert_eq!(v["id"].as_str().unwrap(), "urn:xxh3:0000000000000007");
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
        .generate(&ExportJob {
            articles: &articles,
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
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
        .generate(&ExportJob {
            articles: &articles,
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
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
        .generate(&ExportJob {
            articles: &articles,
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
        .unwrap();
        let v = parse(&dest.join("2024-01-15.json"));
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn test_json_enrichment_prepend_append() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.json");
        let mut article = make_article(1, "Test", "https://example.com/1", 1_705_276_800);
        article.feed_id = 7;
        article.content = "The content".to_string();
        let mut enrichments = HashMap::new();
        enrichments.insert(
            7u64,
            Enrichment {
                feed_title: "My Source".to_string(),
                feed_url: "https://example.com".to_string(),
                feed_slug: "my-source".to_string(),
                feed_page_url: "https://example.com/page".to_string(),
                prepend: Some("Via {{feed.title}}: ".to_string()),
                append: Some(" [Save](https://getpocket.com/save?url={{article.url}})".to_string()),
            },
        );
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&ExportJob {
            articles: &[article],
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &enrichments,
        })
        .unwrap();
        let v = parse(&dest);
        // Enrichment templates are concatenated with the article body and the
        // whole thing is rendered from Markdown to HTML.
        let ch = v["items"][0]["content_html"].as_str().unwrap();
        assert!(ch.contains("Via My Source:"), "prepend missing");
        assert!(ch.contains("getpocket.com"), "append missing");
        assert!(ch.contains("The content"), "original content missing");
    }

    #[test]
    fn test_json_content_html_renders_markdown() {
        // JSON Feed 1.1: article body must be exposed as content_html when it
        // has any HTML structure. Markdown is rendered via pulldown-cmark.
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.json");
        let mut article = make_article(1, "MD", "https://example.com/1", 0);
        article.content = "## Heading\n\nThis is **bold**.".to_string();
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&ExportJob {
            articles: &[article],
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
        .unwrap();
        let v = parse(&dest);
        let ch = v["items"][0]["content_html"].as_str().unwrap();
        assert!(
            ch.contains("<h2>Heading</h2>"),
            "heading not rendered: {}",
            ch
        );
        assert!(
            ch.contains("<strong>bold</strong>"),
            "bold not rendered: {}",
            ch
        );
        // Legacy field must not exist anymore
        assert!(
            v["items"][0]["content_text"].is_null(),
            "content_text must be dropped in favour of content_html"
        );
    }

    #[test]
    fn test_json_content_html_omitted_when_body_empty() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.json");
        // Article with no content should not produce a content_html key at all.
        let article = make_article(1, "Nothing", "https://example.com/n", 0);
        JsonExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&ExportJob {
            articles: &[article],
            title: "Feed",
            link: "https://example.com",
            destination: &dest,
            enrichments: &no_enrichment(),
        })
        .unwrap();
        let v = parse(&dest);
        assert!(
            v["items"][0]["content_html"].is_null(),
            "content_html should be omitted when the body is empty"
        );
    }

    #[test]
    fn test_json_content_html_neutralises_raw_html_and_template_links() {
        // A hostile feed controls both the content and `{{article.url}}`,
        // which the append template turns into a Markdown link.
        let mut article = make_article(1, "XSS", "javascript:alert(document.cookie)", 0);
        article.content = "<script>alert(1)</script>\n\nBody <img src=x onerror=alert(2)>".into();
        let enrichment = Enrichment {
            feed_title: "Feed".into(),
            feed_url: "https://example.com/feed".into(),
            feed_slug: "feed".into(),
            feed_page_url: String::new(),
            prepend: None,
            append: Some("\n\n[Read more]({{article.url}})".into()),
        };

        let item = to_item(&article, Some(&enrichment));

        let html = item.content_html;
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(html.contains("Read more"), "{html}");
    }
}
