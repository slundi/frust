use std::{collections::HashMap, fs, io::BufWriter, path::Path};

use chrono::{DateTime, Utc};
use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use tracing::info;

use crate::{
    error::FrustError,
    model::{Article, Enrichment},
};

use super::{Exporter, render_template};

pub(crate) struct AtomExporter;

impl Exporter for AtomExporter {
    fn generate(
        &self,
        articles: &[Article],
        title: &str,
        link: &str,
        destination: &Path,
        enrichments: &HashMap<u64, Enrichment>,
    ) -> Result<(), FrustError> {
        info!("Exporting to Atom");
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::File::create(destination)?;
        let mut writer = Writer::new_with_indent(BufWriter::new(file), b' ', 2);
        write_atom_to(&mut writer, articles, title, link, enrichments)
    }
}

/// Write a complete Atom 1.0 feed to any `Write` sink.
///
/// This is the shared core used by both [`AtomExporter`] (file output) and the
/// ZIP exporter (in-memory `Vec<u8>` output).
pub(crate) fn write_atom_to<W: std::io::Write>(
    writer: &mut Writer<W>,
    articles: &[Article],
    title: &str,
    link: &str,
    enrichments: &HashMap<u64, Enrichment>,
) -> Result<(), FrustError> {
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| FrustError::Export(e.to_string()))?;

    // <feed xmlns="http://www.w3.org/2005/Atom">
    let mut feed_tag = BytesStart::new("feed");
    feed_tag.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));
    writer
        .write_event(Event::Start(feed_tag))
        .map_err(|e| FrustError::Export(e.to_string()))?;

    write_text_element(writer, "title", title)?;
    write_text_element(writer, "id", link)?;

    // <link rel="alternate" href="..."/>
    {
        let mut link_tag = BytesStart::new("link");
        link_tag.push_attribute(("rel", "alternate"));
        link_tag.push_attribute(("href", link));
        writer
            .write_event(Event::Empty(link_tag))
            .map_err(|e| FrustError::Export(e.to_string()))?;
    }

    // <updated> — most recent article timestamp, or now
    let updated = articles
        .iter()
        .filter(|a| a.timestamp != 0)
        .map(|a| a.timestamp)
        .max()
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0))
        .unwrap_or_else(Utc::now);
    write_text_element(writer, "updated", &updated.to_rfc3339())?;

    for article in articles {
        write_entry(writer, article, enrichments.get(&article.feed_id))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("feed")))
        .map_err(|e| FrustError::Export(e.to_string()))?;

    Ok(())
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

fn write_entry<W: std::io::Write>(
    writer: &mut Writer<W>,
    article: &Article,
    enrichment: Option<&Enrichment>,
) -> Result<(), FrustError> {
    writer
        .write_event(Event::Start(BytesStart::new("entry")))
        .map_err(|e| FrustError::Export(e.to_string()))?;

    write_text_element(writer, "title", &article.title)?;
    write_text_element(writer, "id", &article.url)?;

    // <link rel="alternate" href="..."/>
    {
        let mut link_tag = BytesStart::new("link");
        link_tag.push_attribute(("rel", "alternate"));
        link_tag.push_attribute(("href", article.url.as_str()));
        writer
            .write_event(Event::Empty(link_tag))
            .map_err(|e| FrustError::Export(e.to_string()))?;
    }

    // <updated> and <published> in RFC 3339
    if article.timestamp != 0
        && let Some(dt) = DateTime::<Utc>::from_timestamp(article.timestamp, 0)
    {
        let ts = dt.to_rfc3339();
        write_text_element(writer, "published", &ts)?;
        write_text_element(writer, "updated", &ts)?;
    }

    // <summary>
    if let Some(ref summary) = article.summary {
        write_text_element(writer, "summary", summary)?;
    }

    // <content type="text"> — full markdown content, optionally enriched
    let enriched;
    let content: &str = if let Some(e) = enrichment {
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
        enriched = format!("{pre}{}{app}", article.content);
        &enriched
    } else {
        &article.content
    };

    if !content.is_empty() {
        let mut content_tag = BytesStart::new("content");
        content_tag.push_attribute(("type", "text"));
        writer
            .write_event(Event::Start(content_tag))
            .map_err(|e| FrustError::Export(e.to_string()))?;
        writer
            .write_event(Event::Text(BytesText::new(content)))
            .map_err(|e| FrustError::Export(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("content")))
            .map_err(|e| FrustError::Export(e.to_string()))?;
    }

    // enclosures as <link rel="enclosure" .../>
    for enc in &article.enclosures {
        let mut enc_tag = BytesStart::new("link");
        enc_tag.push_attribute(("rel", "enclosure"));
        enc_tag.push_attribute(("href", enc.url.as_str()));
        enc_tag.push_attribute(("type", enc.mime_type.as_str()));
        if let Some(len) = enc.length {
            enc_tag.push_attribute(("length", len.to_string().as_str()));
        }
        writer
            .write_event(Event::Empty(enc_tag))
            .map_err(|e| FrustError::Export(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("entry")))
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
    fn test_atom_empty_articles() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        AtomExporter
            .generate(
                &[],
                "Empty Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("xmlns=\"http://www.w3.org/2005/Atom\""));
        assert!(xml.contains("<title>Empty Feed</title>"));
        assert!(xml.contains("<id>https://example.com</id>"));
        assert!(!xml.contains("<entry>"));
    }

    #[test]
    fn test_atom_single_article() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        let articles = vec![make_article(1, "Hello World", "https://example.com/1", 0)];
        AtomExporter
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
        assert!(xml.contains("<id>https://example.com/1</id>"));
        assert!(xml.contains("href=\"https://example.com/1\""));
    }

    #[test]
    fn test_atom_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("sub").join("dir").join("feed.atom");
        AtomExporter
            .generate(&[], "Feed", "https://example.com", &dest, &no_enrichment())
            .unwrap();
        assert!(dest.exists());
    }

    #[test]
    fn test_atom_enclosure() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        let mut article = make_article(1, "Podcast", "https://example.com/ep1", 0);
        article.enclosures.push(crate::model::Enclosure {
            url: "https://example.com/ep1.mp3".to_string(),
            mime_type: "audio/mpeg".to_string(),
            length: Some(4096),
        });
        AtomExporter
            .generate(
                &[article],
                "Podcast Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("rel=\"enclosure\""));
        assert!(xml.contains("href=\"https://example.com/ep1.mp3\""));
        assert!(xml.contains("type=\"audio/mpeg\""));
        assert!(xml.contains("length=\"4096\""));
    }

    #[test]
    fn test_atom_timestamps() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        // 2024-01-15 00:00:00 UTC
        let articles = vec![make_article(
            1,
            "Dated",
            "https://example.com/d",
            1_705_276_800,
        )];
        AtomExporter
            .generate(
                &articles,
                "Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("<published>2024-01-15T00:00:00+00:00</published>"));
        assert!(xml.contains("<updated>2024-01-15T00:00:00+00:00</updated>"));
    }

    #[test]
    fn test_atom_summary_and_content() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        let mut article = make_article(1, "Rich", "https://example.com/r", 0);
        article.summary = Some("Short summary".to_string());
        article.content = "Full **markdown** content".to_string();
        AtomExporter
            .generate(
                &[article],
                "Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("<summary>Short summary</summary>"));
        assert!(xml.contains("type=\"text\""));
        assert!(xml.contains("Full **markdown** content"));
    }

    #[test]
    fn test_atom_updated_uses_latest_timestamp() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        let articles = vec![
            make_article(1, "Older", "https://example.com/1", 1_700_000_000),
            make_article(2, "Newer", "https://example.com/2", 1_705_276_800),
        ];
        AtomExporter
            .generate(
                &articles,
                "Feed",
                "https://example.com",
                &dest,
                &no_enrichment(),
            )
            .unwrap();
        let xml = read_xml(&dest);
        // The feed-level <updated> should be the newer timestamp
        assert!(xml.contains("<updated>2024-01-15T00:00:00+00:00</updated>"));
    }

    #[test]
    fn test_atom_enrichment_prepend_append() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        let mut article = make_article(1, "My Article", "https://example.com/1", 0);
        article.feed_id = 99;
        article.content = "Article body".to_string();
        let mut enrichments = HashMap::new();
        enrichments.insert(
            99u64,
            Enrichment {
                feed_title: "My Feed".to_string(),
                feed_url: "https://example.com".to_string(),
                feed_slug: "my-feed".to_string(),
                feed_page_url: "https://example.com/page".to_string(),
                prepend: Some("SOURCE: {{feed.title}} | ".to_string()),
                append: Some(
                    " | [Pocket](https://getpocket.com/save?url={{article.url}})".to_string(),
                ),
            },
        );
        AtomExporter
            .generate(
                &[article],
                "Feed",
                "https://example.com",
                &dest,
                &enrichments,
            )
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("SOURCE: My Feed"), "prepend missing");
        assert!(xml.contains("getpocket.com"), "append missing");
        assert!(xml.contains("Article body"), "content missing");
    }
}
