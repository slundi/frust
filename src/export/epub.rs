use std::{collections::HashMap, fs, path::Path};

use chrono::DateTime;
use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ZipLibrary};
use pulldown_cmark::{Parser, html};
use tracing::info;

use crate::{error::FrustError, model::Article};

use super::{Enrichment, Exporter};

pub(crate) struct EpubExporter;

impl Exporter for EpubExporter {
    fn generate(
        &self,
        articles: &[Article],
        title: &str,
        _link: &str,
        destination: &Path,
        _enrichments: &HashMap<u64, Enrichment>,
    ) -> Result<(), FrustError> {
        info!("Exporting to EPUB");
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        let out = fs::File::create(destination)?;

        let mut builder =
            EpubBuilder::new(ZipLibrary::new().map_err(|e| FrustError::Export(e.to_string()))?)
                .map_err(|e| FrustError::Export(e.to_string()))?;

        builder.epub_version(EpubVersion::V30);
        builder
            .metadata("title", title)
            .map_err(|e| FrustError::Export(e.to_string()))?
            .metadata("lang", "en")
            .map_err(|e| FrustError::Export(e.to_string()))?;

        let mut sorted: Vec<&Article> = articles.iter().collect();
        sorted.sort_by_key(|a| a.timestamp);

        for (i, article) in sorted.iter().enumerate() {
            let xhtml = article_to_xhtml(article);
            let filename = format!("chapter{:04}.xhtml", i + 1);
            builder
                .add_content(EpubContent::new(&filename, xhtml.as_bytes()).title(&article.title))
                .map_err(|e| FrustError::Export(e.to_string()))?;
        }

        builder
            .generate(out)
            .map_err(|e| FrustError::Export(e.to_string()))?;

        Ok(())
    }
}

fn article_to_xhtml(article: &Article) -> String {
    let mut body_html = String::new();
    html::push_html(&mut body_html, Parser::new(&article.content));

    let title = escape_xml(&article.title);
    let date = if article.timestamp == 0 {
        None
    } else {
        DateTime::from_timestamp(article.timestamp, 0).map(|dt| dt.format("%Y-%m-%d").to_string())
    };
    let source_line = match date {
        Some(d) => format!("<p><a href=\"{}\">{}</a></p>", escape_xml(&article.url), d),
        None => format!("<p><a href=\"{}\">source</a></p>", escape_xml(&article.url)),
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd\">\
         <html xmlns=\"http://www.w3.org/1999/xhtml\">\
         <head><title>{title}</title></head>\
         <body><h1>{title}</h1>{source_line}{body_html}</body>\
         </html>"
    )
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::Enrichment;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn no_enrichment() -> HashMap<u64, Enrichment> {
        HashMap::new()
    }

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

    fn output_path(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    #[test]
    fn test_epub_empty_articles() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.epub");
        EpubExporter
            .generate(
                &[],
                "Empty Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        assert!(dest.exists());
        let bytes = fs::read(&dest).unwrap();
        // EPUB is a ZIP archive
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn test_epub_single_article() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.epub");
        let articles = vec![make_article(1, "Hello World", "https://example.com/1", 0)];
        EpubExporter
            .generate(
                &articles,
                "My Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        assert!(dest.exists());
        let bytes = fs::read(&dest).unwrap();
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn test_epub_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("sub").join("dir").join("feed.epub");
        EpubExporter
            .generate(&[], "Feed", "https://example.com", &dest, &no_enrichment())
            .unwrap();
        assert!(dest.exists());
    }

    #[test]
    fn test_epub_multiple_articles() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "multi.epub");
        let articles = vec![
            make_article(1, "First Article", "https://example.com/1", 1_700_000_000),
            make_article(2, "Second Article", "https://example.com/2", 1_705_276_800),
        ];
        EpubExporter
            .generate(
                &articles,
                "Multi Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        assert!(dest.exists());
        let bytes = fs::read(&dest).unwrap();
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn test_epub_article_with_markdown_content() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "content.epub");
        let mut article = make_article(1, "Rich Article", "https://example.com/r", 1_705_276_800);
        article.content = "## Subtitle\n\nThis is **bold** text.".to_string();
        article.summary = Some("A summary".to_string());
        EpubExporter
            .generate(
                &[article],
                "Content Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        assert!(dest.exists());
        let bytes = fs::read(&dest).unwrap();
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn test_epub_xhtml_escaping() {
        let article = make_article(
            1,
            "AT&T <News> \"Quoted\"",
            "https://example.com/1",
            1_705_276_800,
        );
        let xhtml = article_to_xhtml(&article);
        assert!(xhtml.contains("AT&amp;T"));
        assert!(xhtml.contains("&lt;News&gt;"));
        assert!(xhtml.contains("&quot;Quoted&quot;"));
    }

    #[test]
    fn test_epub_xhtml_date_in_source_line() {
        let article = make_article(1, "Dated", "https://example.com/1", 1_705_276_800);
        let xhtml = article_to_xhtml(&article);
        assert!(xhtml.contains("2024-01-15"));
    }

    #[test]
    fn test_epub_xhtml_zero_timestamp() {
        let article = make_article(1, "No Date", "https://example.com/1", 0);
        let xhtml = article_to_xhtml(&article);
        assert!(xhtml.contains("source</a>"));
    }
}
