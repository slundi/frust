use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufWriter, Write},
    path::Path,
};

use chrono::{DateTime, Utc};
use slug::slugify;

use crate::{
    error::FrustError,
    model::{Article, ExportStrategy},
};

use super::Exporter;

pub(crate) struct MarkdownExporter {
    pub(crate) strategy: ExportStrategy,
}

impl Exporter for MarkdownExporter {
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

fn article_to_md(article: &Article) -> String {
    let date = DateTime::<Utc>::from_timestamp(article.timestamp, 0)
        .unwrap_or_default()
        .to_rfc3339();
    // Escape double-quotes inside the title for valid YAML inline strings
    let escaped_title = article.title.replace('"', "\\\"");
    let mut s = format!(
        "---\ntitle: \"{escaped_title}\"\nurl: {}\ndate: {date}\n---\n\n",
        article.url
    );
    if !article.content.is_empty() {
        s.push_str(&article.content);
        s.push('\n');
    }
    s
}

fn article_filename(article: &Article, ext: &str) -> String {
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
    format!("{date}-{title_slug}.{ext}")
}

fn monolithic(
    articles: &[Article],
    title: &str,
    link: &str,
    destination: &Path,
) -> Result<(), FrustError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(destination)?;
    let mut w = BufWriter::new(file);
    writeln!(w, "# {title}")?;
    writeln!(w, "\nSource: {link}\n")?;
    for article in articles {
        writeln!(w, "{}", article_to_md(article))?;
    }
    Ok(())
}

fn individual(articles: &[Article], destination: &Path) -> Result<(), FrustError> {
    fs::create_dir_all(destination)?;
    let mut used: HashMap<String, u32> = HashMap::new();
    for article in articles {
        let base = article_filename(article, "md");
        // Strip .md to get the stem for dedup, then re-append
        let stem = base.trim_end_matches(".md").to_string();
        let idx = used.entry(stem.clone()).or_insert(0);
        let filename = if *idx == 0 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{idx}.md")
        };
        *idx += 1;
        fs::write(destination.join(filename), article_to_md(article))?;
    }
    Ok(())
}

fn daily(articles: &[Article], destination: &Path) -> Result<(), FrustError> {
    fs::create_dir_all(destination)?;
    // BTreeMap gives deterministic (chronological) order over ISO date keys
    let mut by_day: BTreeMap<String, Vec<&Article>> = BTreeMap::new();
    for article in articles {
        let day = DateTime::<Utc>::from_timestamp(article.timestamp, 0)
            .unwrap_or_default()
            .format("%Y-%m-%d")
            .to_string();
        by_day.entry(day).or_default().push(article);
    }
    for (day, day_articles) in &by_day {
        let path = destination.join(format!("{day}.md"));
        let file = fs::File::create(&path)?;
        let mut w = BufWriter::new(file);
        writeln!(w, "# {day}\n")?;
        for article in day_articles {
            writeln!(w, "{}", article_to_md(article))?;
        }
    }
    Ok(())
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_article(id: u64, title: &str, url: &str, ts: i64, content: &str) -> Article {
        Article {
            id,
            feed_id: 1,
            title: title.to_string(),
            url: url.to_string(),
            content: content.to_string(),
            summary: None,
            timestamp: ts,
            added_at: ts,
            is_full_content: false,
            enclosures: vec![],
        }
    }

    #[test]
    fn test_monolithic_empty_articles() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.md");
        MarkdownExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&[], "My Feed", "https://example.com", &dest)
        .unwrap();
        let content = fs::read_to_string(&dest).unwrap();
        assert!(content.contains("# My Feed"));
        assert!(content.contains("https://example.com"));
    }

    #[test]
    fn test_monolithic_single_article() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.md");
        let articles = vec![make_article(
            1,
            "Hello World",
            "https://example.com/1",
            1_705_276_800,
            "Some content here.",
        )];
        MarkdownExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&articles, "My Feed", "https://example.com", &dest)
        .unwrap();
        let content = fs::read_to_string(&dest).unwrap();
        assert!(content.contains("title: \"Hello World\""));
        assert!(content.contains("url: https://example.com/1"));
        assert!(content.contains("Some content here."));
    }

    #[test]
    fn test_monolithic_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("sub").join("dir").join("feed.md");
        MarkdownExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&[], "Feed", "https://example.com", &dest)
        .unwrap();
        assert!(dest.exists());
    }

    #[test]
    fn test_monolithic_title_quote_escaping() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("feed.md");
        let articles = vec![make_article(
            1,
            r#"Say "Hello""#,
            "https://example.com/1",
            0,
            "",
        )];
        MarkdownExporter {
            strategy: ExportStrategy::Monolithic,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let content = fs::read_to_string(&dest).unwrap();
        assert!(content.contains(r#"title: "Say \"Hello\"""#));
    }

    #[test]
    fn test_individual_creates_files() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("articles");
        let articles = vec![
            make_article(
                1,
                "Article One",
                "https://example.com/1",
                1_705_276_800,
                "A",
            ),
            make_article(
                2,
                "Article Two",
                "https://example.com/2",
                1_705_276_800,
                "B",
            ),
        ];
        MarkdownExporter {
            strategy: ExportStrategy::Individual,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let count = fs::read_dir(&dest).unwrap().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_individual_file_content() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("articles");
        let articles = vec![make_article(
            1,
            "My Article",
            "https://example.com/my-article",
            1_705_276_800,
            "Body text.",
        )];
        MarkdownExporter {
            strategy: ExportStrategy::Individual,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let path = dest.join("2024-01-15-my-article.md");
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("title: \"My Article\""));
        assert!(content.contains("url: https://example.com/my-article"));
        assert!(content.contains("Body text."));
    }

    #[test]
    fn test_individual_dedup_slugs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("articles");
        let articles = vec![
            make_article(1, "Same Title", "https://example.com/1", 1_705_276_800, "A"),
            make_article(2, "Same Title", "https://example.com/2", 1_705_276_800, "B"),
        ];
        MarkdownExporter {
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
            // 2024-01-15
            make_article(1, "Morning", "https://example.com/1", 1_705_276_800, "AM"),
            // 2024-01-16
            make_article(2, "Evening", "https://example.com/2", 1_705_363_200, "PM"),
        ];
        MarkdownExporter {
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
        assert_eq!(files[0], "2024-01-15.md");
        assert_eq!(files[1], "2024-01-16.md");
    }

    #[test]
    fn test_daily_file_contains_all_articles_for_day() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("daily");
        let articles = vec![
            make_article(1, "First", "https://example.com/1", 1_705_276_800, "C1"),
            make_article(2, "Second", "https://example.com/2", 1_705_276_800, "C2"),
        ];
        MarkdownExporter {
            strategy: ExportStrategy::Daily,
        }
        .generate(&articles, "Feed", "https://example.com", &dest)
        .unwrap();
        let content = fs::read_to_string(dest.join("2024-01-15.md")).unwrap();
        assert!(content.contains("First"));
        assert!(content.contains("Second"));
    }
}
