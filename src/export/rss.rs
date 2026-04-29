use std::{collections::HashMap, fs, io::BufWriter, path::Path};

use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use tracing::info;

use crate::{error::FrustError, model::Article};

use super::{Enrichment, Exporter, render_template};

pub(crate) struct RssExporter;

impl Exporter for RssExporter {
    fn generate(
        &self,
        articles: &[Article],
        title: &str,
        link: &str,
        destination: &Path,
        enrichments: &HashMap<u64, Enrichment>,
    ) -> Result<(), FrustError> {
        info!("Exporting to RSS");
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = fs::File::create(destination)?;
        let mut writer = Writer::new_with_indent(BufWriter::new(file), b' ', 2);

        writer
            .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
            .map_err(|e| FrustError::Export(e.to_string()))?;

        // <rss version="2.0">
        let mut rss_tag = BytesStart::new("rss");
        rss_tag.push_attribute(("version", "2.0"));
        writer
            .write_event(Event::Start(rss_tag))
            .map_err(|e| FrustError::Export(e.to_string()))?;

        // <channel>
        writer
            .write_event(Event::Start(BytesStart::new("channel")))
            .map_err(|e| FrustError::Export(e.to_string()))?;

        write_text_element(&mut writer, "title", title)?;
        write_text_element(&mut writer, "link", link)?;
        write_text_element(&mut writer, "description", title)?;

        for article in articles {
            write_item(&mut writer, article, enrichments.get(&article.feed_id))?;
        }

        // </channel>
        writer
            .write_event(Event::End(BytesEnd::new("channel")))
            .map_err(|e| FrustError::Export(e.to_string()))?;

        // </rss>
        writer
            .write_event(Event::End(BytesEnd::new("rss")))
            .map_err(|e| FrustError::Export(e.to_string()))?;

        Ok(())
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn write_text_element<W: std::io::Write>(
    writer: &mut Writer<W>,
    tag: &str,
    text: &str,
) -> Result<(), FrustError> {
    writer
        .write_event(Event::Start(BytesStart::new(tag)))
        .map_err(|e| FrustError::Export(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(text)))
        .map_err(|e| FrustError::Export(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new(tag)))
        .map_err(|e| FrustError::Export(e.to_string()))?;
    Ok(())
}

fn write_item<W: std::io::Write>(
    writer: &mut Writer<W>,
    article: &Article,
    enrichment: Option<&Enrichment>,
) -> Result<(), FrustError> {
    writer
        .write_event(Event::Start(BytesStart::new("item")))
        .map_err(|e| FrustError::Export(e.to_string()))?;

    write_text_element(writer, "title", &article.title)?;
    write_text_element(writer, "link", &article.url)?;
    write_text_element(writer, "guid", &article.url)?;

    let base = article.summary.as_deref().unwrap_or(&article.content);
    if !base.is_empty() || enrichment.is_some() {
        let description = match enrichment {
            Some(e) => {
                let pre = e
                    .prepend
                    .as_deref()
                    .map(|t| render_template(t, e, article))
                    .unwrap_or_default();
                let app = e
                    .append
                    .as_deref()
                    .map(|t| render_template(t, e, article))
                    .unwrap_or_default();
                format!("{pre}{base}{app}")
            }
            None => base.to_string(),
        };
        write_text_element(writer, "description", &description)?;
    }

    // pubDate in RFC 2822 format
    if article.timestamp != 0 {
        use chrono::{DateTime, Utc};
        let dt = DateTime::<Utc>::from_timestamp(article.timestamp, 0)
            .unwrap_or_default()
            .to_rfc2822();
        write_text_element(writer, "pubDate", &dt)?;
    }

    // enclosures
    for enc in &article.enclosures {
        let mut tag = BytesStart::new("enclosure");
        tag.push_attribute(("url", enc.url.as_str()));
        tag.push_attribute(("type", enc.mime_type.as_str()));
        if let Some(len) = enc.length {
            tag.push_attribute(("length", len.to_string().as_str()));
        } else {
            tag.push_attribute(("length", "0"));
        }
        writer
            .write_event(Event::Empty(tag))
            .map_err(|e| FrustError::Export(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("item")))
        .map_err(|e| FrustError::Export(e.to_string()))?;

    Ok(())
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
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

    fn no_enrichment() -> HashMap<u64, Enrichment> {
        HashMap::new()
    }

    fn output_path(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    fn read_xml(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn test_rss_empty_articles() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.xml");
        RssExporter
            .generate(
                &[],
                "Empty Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("<title>Empty Feed</title>"));
        assert!(xml.contains("<link>https://example.com</link>"));
        assert!(!xml.contains("<item>"));
    }

    #[test]
    fn test_rss_single_article() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.xml");
        let articles = vec![make_article(1, "Hello World", "https://example.com/1", 0)];
        RssExporter
            .generate(
                &articles,
                "My Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("<title>Hello World</title>"));
        assert!(xml.contains("<link>https://example.com/1</link>"));
    }

    #[test]
    fn test_rss_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("sub").join("dir").join("feed.xml");
        RssExporter
            .generate(&[], "Feed", "https://example.com", &dest, &no_enrichment())
            .unwrap();
        assert!(dest.exists());
    }

    #[test]
    fn test_rss_enclosure() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.xml");
        let mut article = make_article(1, "Podcast", "https://example.com/ep1", 0);
        article.enclosures.push(crate::model::Enclosure {
            url: "https://example.com/ep1.mp3".to_string(),
            mime_type: "audio/mpeg".to_string(),
            length: Some(4096),
        });
        RssExporter
            .generate(
                &[article],
                "Podcast Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("url=\"https://example.com/ep1.mp3\""));
        assert!(xml.contains("type=\"audio/mpeg\""));
        assert!(xml.contains("length=\"4096\""));
    }

    #[test]
    fn test_rss_pubdate_present_when_nonzero() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.xml");
        // 2024-01-15 00:00:00 UTC
        let articles = vec![make_article(
            1,
            "Dated",
            "https://example.com/d",
            1_705_276_800,
        )];
        RssExporter
            .generate(
                &articles,
                "Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("<pubDate>"), "pubDate element missing");
    }

    #[test]
    fn test_rss_enrichment_prepend_append() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.xml");
        let mut article = make_article(1, "My Article", "https://example.com/1", 0);
        article.feed_id = 42;
        article.content = "Body text".to_string();
        let mut enrichments = HashMap::new();
        enrichments.insert(
            42u64,
            Enrichment {
                feed_title: "Test Feed".to_string(),
                feed_url: "https://example.com".to_string(),
                feed_slug: "test-feed".to_string(),
                feed_page_url: "https://example.com/page".to_string(),
                prepend: Some("[Read on {{feed.title}}]({{article.url}}) — ".to_string()),
                append: Some(
                    " — [Save](https://getpocket.com/save?url={{article.url}})".to_string(),
                ),
            },
        );
        RssExporter
            .generate(
                &[article],
                "Feed",
                "https://example.com",
                &dest,
                &enrichments,
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("Read on Test Feed"), "prepend missing");
        assert!(xml.contains("getpocket.com"), "append missing");
        assert!(xml.contains("Body text"), "original content missing");
    }
}
