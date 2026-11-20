use std::collections::HashMap;

use chrono::{DateTime, Utc};
use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use tracing::info;
use twox_hash::XxHash3_64;

use crate::{
    error::FrustError,
    model::{Article, Enrichment},
};

use super::{
    ExportJob, Exporter, article_date, article_urn, enriched_html, is_absolute_url,
    write_atomically,
};

pub(crate) struct AtomExporter;

impl Exporter for AtomExporter {
    fn generate(&self, job: &ExportJob<'_>) -> Result<(), FrustError> {
        let ExportJob {
            articles,
            title,
            link,
            destination,
            enrichments,
        } = *job;
        info!("Exporting to Atom");
        write_atomically(destination, |out| {
            let mut writer = Writer::new_with_indent(out, b' ', 2);
            write_atom_to(&mut writer, articles, title, link, enrichments)
        })
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
    // <id> must be an absolute IRI (RFC 4287 §4.2.6): the feed URL when
    // base_url is set, else a stable URN derived from the relative link.
    if is_absolute_url(link) {
        write_text_element(writer, "id", link)?;
        // <link rel="self" type="application/atom+xml" href="..."/>
        let mut link_tag = BytesStart::new("link");
        link_tag.push_attribute(("rel", "self"));
        link_tag.push_attribute(("type", "application/atom+xml"));
        link_tag.push_attribute(("href", link));
        writer
            .write_event(Event::Empty(link_tag))
            .map_err(|e| FrustError::Export(e.to_string()))?;
    } else {
        let urn = format!("urn:xxh3:{:016x}", XxHash3_64::oneshot(link.as_bytes()));
        write_text_element(writer, "id", &urn)?;
    }

    // <updated> — most recent article timestamp, or now
    let updated = articles
        .iter()
        .map(article_date)
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

/// `<tag type="html">escaped HTML</tag>`
fn write_html_element<W: std::io::Write>(
    writer: &mut Writer<W>,
    tag: &str,
    html: &str,
) -> Result<(), FrustError> {
    let mut start = BytesStart::new(tag);
    start.push_attribute(("type", "html"));
    writer
        .write_event(Event::Start(start))
        .map_err(|e| FrustError::Export(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(html)))
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
    // Atom entry <id> must be a permanent IRI (RFC 4287 §4.2.6).
    write_text_element(writer, "id", &article_urn(article))?;

    // <link rel="alternate" href="..."/>
    {
        let mut link_tag = BytesStart::new("link");
        link_tag.push_attribute(("rel", "alternate"));
        link_tag.push_attribute(("href", article.url.as_str()));
        writer
            .write_event(Event::Empty(link_tag))
            .map_err(|e| FrustError::Export(e.to_string()))?;
    }

    // <updated> (required, RFC 4287 §4.1.2) and <published> in RFC 3339
    let date = DateTime::<Utc>::from_timestamp(article_date(article), 0)
        .unwrap_or_default()
        .to_rfc3339();
    write_text_element(writer, "published", &date)?;
    write_text_element(writer, "updated", &date)?;

    // <summary type="html">: the feed's own summary, HTML sanitized by feed-rs
    if let Some(ref summary) = article.summary {
        write_html_element(writer, "summary", summary)?;
    }

    // <content type="html">: the Markdown content rendered to sanitized HTML,
    // optionally enriched; as type="text", readers showed raw Markdown.
    let content = enriched_html(article, enrichment);
    if !content.is_empty() {
        write_html_element(writer, "content", &content)?;
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
    use std::fs;

    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
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
            .generate(&ExportJob {
                articles: &[],
                title: "Empty Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("xmlns=\"http://www.w3.org/2005/Atom\""));
        assert!(xml.contains("<title>Empty Feed</title>"));
        assert!(xml.contains("<id>https://example.com</id>"));
        assert!(!xml.contains("<entry>"));
    }

    // ---- entry compliance (B14) ----

    #[test]
    fn test_atom_entry_always_has_updated() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        // Stored dateless by an older version: dated by added_at.
        let mut article = make_article(1, "T", "https://example.com/1", 0);
        article.added_at = 1_705_276_800;
        AtomExporter
            .generate(&ExportJob {
                articles: &[article],
                title: "F",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        let xml = read_xml(&dest);
        let entry = &xml[xml.find("<entry>").unwrap()..];
        assert!(
            entry.contains("<updated>2024-01-15T00:00:00+00:00</updated>"),
            "{entry}"
        );
    }

    #[test]
    fn test_atom_content_is_sanitized_html() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        let mut article = make_article(1, "T", "https://example.com/1", 0);
        article.content = "[x](javascript:alert(1)) <script>bad()</script>".to_string();
        AtomExporter
            .generate(&ExportJob {
                articles: &[article],
                title: "F",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        let xml = read_xml(&dest);
        assert!(!xml.contains("javascript:"), "{xml}");
        assert!(!xml.contains("&lt;script"), "{xml}");
    }

    // ---- channel links (B13) ----

    fn atom_with_link(link: &str) -> String {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        AtomExporter
            .generate(&ExportJob {
                articles: &[],
                title: "T",
                link,
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        read_xml(&dest)
    }

    #[test]
    fn test_atom_absolute_link_is_id_and_self_link() {
        let xml = atom_with_link("https://example.org/rss/tech.atom");
        assert!(xml.contains("<id>https://example.org/rss/tech.atom</id>"));
        assert!(xml.contains(
            "<link rel=\"self\" type=\"application/atom+xml\" href=\"https://example.org/rss/tech.atom\"/>"
        ));
        assert!(!xml.contains("rel=\"alternate\""));
    }

    #[test]
    fn test_atom_relative_link_gives_urn_id_and_no_self_link() {
        let xml = atom_with_link("/tech.atom");
        assert!(xml.contains("<id>urn:xxh3:"), "{xml}");
        assert!(!xml.contains("<id>/tech.atom</id>"));
        assert!(!xml.contains("<link"));
        // Stable across runs.
        let id_line = |xml: &str| {
            xml.lines()
                .find(|line| line.contains("<id>"))
                .map(str::to_string)
        };
        assert_eq!(id_line(&xml), id_line(&atom_with_link("/tech.atom")));
    }

    #[test]
    fn test_atom_single_article() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        let articles = vec![make_article(1, "Hello World", "https://example.com/1", 0)];
        AtomExporter
            .generate(&ExportJob {
                articles: &articles,
                title: "My Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("<title>Hello World</title>"));
        // Entry <id> is a stable URN derived from article.id, not the URL.
        assert!(
            xml.contains("<id>urn:xxh3:0000000000000001</id>"),
            "expected stable URN id, got:\n{}",
            xml
        );
        // The URL still appears as the alternate link.
        assert!(xml.contains("href=\"https://example.com/1\""));
    }

    #[test]
    fn test_atom_entry_id_is_stable_across_url_changes() {
        // Two articles with the same id but different URLs must produce the
        // same <id> URN — otherwise feed readers would see them as new items
        // every time the source URL is rewritten (redirects, tracking params).
        let dir = TempDir::new().unwrap();
        let a = output_path(&dir, "a.atom");
        let b = output_path(&dir, "b.atom");
        let mut art_a = make_article(42, "T", "https://example.com/old", 0);
        let mut art_b = make_article(42, "T", "https://example.com/new?utm=1", 0);
        art_a.content = "body".into();
        art_b.content = "body".into();
        AtomExporter
            .generate(&ExportJob {
                articles: &[art_a],
                title: "F",
                link: "https://example.com",
                destination: &a,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        AtomExporter
            .generate(&ExportJob {
                articles: &[art_b],
                title: "F",
                link: "https://example.com",
                destination: &b,
                enrichments: &no_enrichment(),
            })
            .unwrap();

        let extract_id = |p: &Path| {
            let xml = read_xml(p);
            let start = xml.find("<entry>").unwrap();
            let idx = xml[start..].find("<id>").unwrap() + start;
            let end = xml[idx..].find("</id>").unwrap() + idx;
            xml[idx + 4..end].to_string()
        };
        assert_eq!(
            extract_id(&a),
            extract_id(&b),
            "entry <id> must be stable when article.id is the same"
        );
        assert!(
            extract_id(&a).starts_with("urn:xxh3:"),
            "id must be a urn:xxh3 URN"
        );
    }

    #[test]
    fn test_atom_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("sub").join("dir").join("feed.atom");
        AtomExporter
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
            .generate(&ExportJob {
                articles: &[article],
                title: "Podcast Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
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
            .generate(&ExportJob {
                articles: &articles,
                title: "Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
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
            .generate(&ExportJob {
                articles: &[article],
                title: "Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("<summary type=\"html\">Short summary</summary>"));
        // Markdown rendered to HTML, escaped once in the XML.
        assert!(xml.contains(
            "<content type=\"html\">&lt;p&gt;Full &lt;strong&gt;markdown&lt;/strong&gt; content"
        ));
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
            .generate(&ExportJob {
                articles: &articles,
                title: "Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        let xml = read_xml(&dest);
        // The feed-level <updated> should be the newer timestamp
        assert!(xml.contains("<updated>2024-01-15T00:00:00+00:00</updated>"));
    }

    #[test]
    fn test_atom_escapes_xml_special_chars() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "feed.atom");
        let mut article =
            make_article(1, "AT&T <News> \"Q&A\"", "https://example.com/1?a=1&b=2", 0);
        article.content = "5 < 10 & ok".to_string();
        article.summary = Some("Sum & <sub>".to_string());
        AtomExporter
            .generate(&ExportJob {
                articles: &[article],
                title: "News & Media",
                link: "https://example.com/?x=1&y=2",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        let xml = read_xml(&dest);
        // Text nodes escaped
        assert!(xml.contains("AT&amp;T"), "title amp not escaped");
        assert!(xml.contains("&lt;News&gt;"), "title angle not escaped");
        assert!(xml.contains("News &amp; Media"), "feed title not escaped");
        assert!(xml.contains("Sum &amp; &lt;sub&gt;"), "summary not escaped");
        // HTML-rendered content, itself escaped in the XML text node.
        assert!(
            xml.contains("5 &amp;lt; 10 &amp;amp; ok"),
            "content not escaped"
        );
        // Attribute values (href) escaped
        assert!(
            xml.contains("a=1&amp;b=2"),
            "attribute amp not escaped: {}",
            xml
        );
        assert!(!xml.contains("AT&T"), "raw '&' leaked into output: {}", xml);
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
            .generate(&ExportJob {
                articles: &[article],
                title: "Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &enrichments,
            })
            .unwrap();
        let xml = read_xml(&dest);
        assert!(xml.contains("SOURCE: My Feed"), "prepend missing");
        assert!(xml.contains("getpocket.com"), "append missing");
        assert!(xml.contains("Article body"), "content missing");
    }
}
