//! Minimal EPUB 3 writer on top of the `zip` crate.
//!
//! Replaces `epub-builder`, whose only zip backend pinned a vulnerable
//! `zip 6`. The container holds exactly what EPUB 3.3 requires, plus an NCX
//! for older readers:
//!
//! ```text
//! mimetype                  stored, first entry, no extra field (OCF)
//! META-INF/container.xml    points to the package document
//! OEBPS/content.opf         metadata, manifest, spine
//! OEBPS/nav.xhtml           EPUB 3 navigation document
//! OEBPS/toc.ncx             EPUB 2 table of contents (older Kobo/Adobe readers)
//! OEBPS/chapterNNNN.xhtml   one chapter per article, oldest first
//! ```

use std::io::{Seek, Write};

use chrono::DateTime;
use tracing::info;
use twox_hash::XxHash3_64;
use zip::{CompressionMethod, ZipWriter, result::ZipError, write::SimpleFileOptions};

use crate::{error::FrustError, model::Article};

use super::{
    ExportJob, Exporter,
    html::{is_safe_link, markdown_to_safe_html},
    write_atomically,
};

pub(crate) struct EpubExporter;

const LANG: &str = "en";

const CONTAINER_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
<rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles>\
</container>";

impl Exporter for EpubExporter {
    fn generate(&self, job: &ExportJob<'_>) -> Result<(), FrustError> {
        let ExportJob {
            articles,
            title,
            link: _,
            destination,
            enrichments: _,
        } = *job;
        info!("Exporting to EPUB");

        // Oldest-first on purpose: EPUB is read start-to-end like a book, so the
        // natural order is chronological. RSS/Atom exports sort newest-first for
        // feed-reader conventions — that's a deliberate divergence.
        let mut sorted: Vec<&Article> = articles.iter().collect();
        sorted.sort_by_key(|a| a.timestamp);

        write_atomically(destination, |out| {
            write_epub(out, title, &sorted)?;
            Ok(())
        })
    }
}

fn zip_error(e: ZipError) -> FrustError {
    FrustError::Export(e.to_string())
}

fn add_entry<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    name: &str,
    content: &str,
) -> Result<(), FrustError> {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(name, options).map_err(zip_error)?;
    zip.write_all(content.as_bytes())?;
    Ok(())
}

/// Write the EPUB container for `articles` (already in reading order) to `out`.
fn write_epub<W: Write + Seek>(
    out: W,
    title: &str,
    articles: &[&Article],
) -> Result<W, FrustError> {
    let mut zip = ZipWriter::new(out);

    // OCF: `mimetype` must come first, uncompressed, so readers can sniff it.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("mimetype", stored).map_err(zip_error)?;
    zip.write_all(b"application/epub+zip")?;

    add_entry(&mut zip, "META-INF/container.xml", CONTAINER_XML)?;

    let chapters: Vec<(String, &str)> = articles
        .iter()
        .enumerate()
        .map(|(i, article)| (format!("chapter{:04}.xhtml", i + 1), article.title.as_str()))
        .collect();
    // Stable across runs, so readers update the same book instead of adding one.
    let identifier = format!("urn:xxh3:{:016x}", XxHash3_64::oneshot(title.as_bytes()));
    // Derived from the content, not the clock: identical input gives identical
    // bytes, so an unchanged group doesn't rewrite the file.
    let modified = articles
        .iter()
        .map(|a| a.timestamp.max(a.added_at))
        .max()
        .unwrap_or(0);

    add_entry(
        &mut zip,
        "OEBPS/content.opf",
        &package_opf(title, &identifier, modified, &chapters),
    )?;
    add_entry(&mut zip, "OEBPS/nav.xhtml", &nav_xhtml(title, &chapters))?;
    add_entry(
        &mut zip,
        "OEBPS/toc.ncx",
        &toc_ncx(title, &identifier, &chapters),
    )?;
    for ((file, _), article) in chapters.iter().zip(articles) {
        add_entry(
            &mut zip,
            &format!("OEBPS/{file}"),
            &article_to_xhtml(article),
        )?;
    }

    zip.finish().map_err(zip_error)
}

fn package_opf(
    title: &str,
    identifier: &str,
    modified: i64,
    chapters: &[(String, &str)],
) -> String {
    let modified = DateTime::from_timestamp(modified, 0)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ");
    let mut manifest = String::new();
    let mut spine = String::new();
    for (i, (file, _)) in chapters.iter().enumerate() {
        manifest.push_str(&format!(
            "<item id=\"c{:04}\" href=\"{file}\" media-type=\"application/xhtml+xml\"/>",
            i + 1
        ));
        spine.push_str(&format!("<itemref idref=\"c{:04}\"/>", i + 1));
    }
    let title = escape_xml(title);
    // The navigation document is also the first spine item: a table of
    // contents page, and a spine that is never empty (required by the spec).
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" \
         unique-identifier=\"pub-id\" xml:lang=\"{LANG}\">\
         <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
         <dc:identifier id=\"pub-id\">{identifier}</dc:identifier>\
         <dc:title>{title}</dc:title>\
         <dc:language>{LANG}</dc:language>\
         <meta property=\"dcterms:modified\">{modified}</meta>\
         </metadata>\
         <manifest>\
         <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\
         <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\
         {manifest}\
         </manifest>\
         <spine toc=\"ncx\"><itemref idref=\"nav\"/>{spine}</spine>\
         </package>"
    )
}

fn nav_xhtml(title: &str, chapters: &[(String, &str)]) -> String {
    let items: String = if chapters.is_empty() {
        // A navigation list must not be empty.
        "<li><span>No articles</span></li>".to_string()
    } else {
        chapters
            .iter()
            .map(|(file, chapter_title)| {
                format!(
                    "<li><a href=\"{file}\">{}</a></li>",
                    escape_xml(chapter_title)
                )
            })
            .collect()
    };
    let title = escape_xml(title);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <!DOCTYPE html>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" \
         xml:lang=\"{LANG}\" lang=\"{LANG}\">\
         <head><title>{title}</title></head>\
         <body><nav epub:type=\"toc\" id=\"toc\"><h1>{title}</h1><ol>{items}</ol></nav></body>\
         </html>"
    )
}

fn toc_ncx(title: &str, identifier: &str, chapters: &[(String, &str)]) -> String {
    let points: String = if chapters.is_empty() {
        // The NCX navMap needs at least one entry.
        "<navPoint id=\"np1\" playOrder=\"1\"><navLabel><text>Contents</text></navLabel>\
         <content src=\"nav.xhtml\"/></navPoint>"
            .to_string()
    } else {
        chapters
            .iter()
            .enumerate()
            .map(|(i, (file, chapter_title))| {
                format!(
                    "<navPoint id=\"np{order}\" playOrder=\"{order}\"><navLabel><text>{}</text>\
                     </navLabel><content src=\"{file}\"/></navPoint>",
                    escape_xml(chapter_title),
                    order = i + 1
                )
            })
            .collect()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\
         <head><meta name=\"dtb:uid\" content=\"{identifier}\"/></head>\
         <docTitle><text>{}</text></docTitle>\
         <navMap>{points}</navMap>\
         </ncx>",
        escape_xml(title)
    )
}

fn article_to_xhtml(article: &Article) -> String {
    // Raw HTML would also make the chapter invalid XHTML for strict readers.
    let body_html = markdown_to_safe_html(&article.content);

    let title = escape_xml(&article.title);
    let date = if article.timestamp == 0 {
        None
    } else {
        DateTime::from_timestamp(article.timestamp, 0).map(|dt| dt.format("%Y-%m-%d").to_string())
    };
    let label = date.as_deref().unwrap_or("source");
    // The URL comes from the feed: no link at all rather than a `javascript:` one.
    let source_line = if is_safe_link(&article.url) {
        format!(
            "<p><a href=\"{}\">{label}</a></p>",
            escape_xml(&article.url)
        )
    } else {
        format!("<p>{label}</p>")
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <!DOCTYPE html>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{LANG}\" lang=\"{LANG}\">\
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
    use std::fs;

    use super::*;
    use crate::export::Enrichment;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
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
            .generate(&ExportJob {
                articles: &[],
                title: "Empty Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
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
            .generate(&ExportJob {
                articles: &articles,
                title: "My Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
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
    fn test_epub_multiple_articles() {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "multi.epub");
        let articles = vec![
            make_article(1, "First Article", "https://example.com/1", 1_700_000_000),
            make_article(2, "Second Article", "https://example.com/2", 1_705_276_800),
        ];
        EpubExporter
            .generate(&ExportJob {
                articles: &articles,
                title: "Multi Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
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
            .generate(&ExportJob {
                articles: &[article],
                title: "Content Feed",
                link: "https://example.com",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
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

    #[test]
    fn test_epub_xhtml_drops_javascript_source_link() {
        let article = make_article(1, "Bad", "javascript:alert(1)", 1_705_276_800);
        let xhtml = article_to_xhtml(&article);
        assert!(!xhtml.contains("javascript:"));
        assert!(xhtml.contains("<p>2024-01-15</p>"));
    }

    #[test]
    fn test_epub_xhtml_escapes_raw_html_and_is_well_formed() {
        let mut article = make_article(1, "Raw", "https://example.com/1", 1_705_276_800);
        article.content =
            "Line<br>break\n\n<script>alert(1)</script>\n\n[x](javascript:alert(2))".into();
        let xhtml = article_to_xhtml(&article);

        assert!(!xhtml.contains("<script"));
        assert!(!xhtml.contains("javascript:"));
        assert!(xhtml.contains("&lt;br&gt;"));
        assert_well_formed("chapter", &xhtml);
    }

    // ── EPUB container (own writer) ───────────────────────────────────────────

    fn assert_well_formed(name: &str, xml: &str) {
        let mut reader = quick_xml::Reader::from_str(xml);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("{name} is not well-formed XML: {e}\n{xml}"),
            }
        }
    }

    fn export(articles: &[Article], title: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let dest = output_path(&dir, "book.epub");
        EpubExporter
            .generate(&ExportJob {
                articles,
                title,
                link: "",
                destination: &dest,
                enrichments: &no_enrichment(),
            })
            .unwrap();
        (dir, dest)
    }

    fn entry(path: &Path, name: &str) -> String {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut content = String::new();
        std::io::Read::read_to_string(&mut archive.by_name(name).unwrap(), &mut content).unwrap();
        content
    }

    fn entry_names(path: &Path) -> Vec<String> {
        let archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        archive
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect()
    }

    fn two_articles() -> Vec<Article> {
        vec![
            make_article(2, "Newer & <b>", "https://example.com/2", 1_705_276_800),
            make_article(1, "Older", "https://example.com/1", 1_700_000_000),
        ]
    }

    #[test]
    fn test_epub_mimetype_is_first_stored_without_extra_field() {
        // OCF: readers sniff "PK..mimetypeapplication/epub+zip" at fixed offsets.
        let (_dir, dest) = export(&two_articles(), "Book");
        let bytes = fs::read(&dest).unwrap();
        let u16_at = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        assert_eq!(&bytes[..4], b"PK\x03\x04", "local file header");
        assert_eq!(u16_at(8), 0, "mimetype must be stored, not deflated");
        assert_eq!(u16_at(26), 8, "file name length");
        assert_eq!(u16_at(28), 0, "mimetype must have no extra field");
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..58], b"application/epub+zip");
    }

    #[test]
    fn test_epub_contains_required_entries() {
        let (_dir, dest) = export(&two_articles(), "Book");
        let names = entry_names(&dest);
        for required in [
            "mimetype",
            "META-INF/container.xml",
            "OEBPS/content.opf",
            "OEBPS/nav.xhtml",
            "OEBPS/toc.ncx",
            "OEBPS/chapter0001.xhtml",
            "OEBPS/chapter0002.xhtml",
        ] {
            assert!(
                names.iter().any(|n| n == required),
                "missing {required}: {names:?}"
            );
        }
        assert!(entry(&dest, "META-INF/container.xml").contains("full-path=\"OEBPS/content.opf\""));
    }

    #[test]
    fn test_epub_every_xml_entry_is_well_formed() {
        let (_dir, dest) = export(&two_articles(), "Tom & Jerry <News>");
        for name in entry_names(&dest).iter().filter(|n| *n != "mimetype") {
            assert_well_formed(name, &entry(&dest, name));
        }
    }

    #[test]
    fn test_epub_package_lists_chapters_oldest_first() {
        let (_dir, dest) = export(&two_articles(), "Book");
        let opf = entry(&dest, "OEBPS/content.opf");
        assert!(opf.contains("properties=\"nav\""));
        assert!(opf.contains(
            "<itemref idref=\"nav\"/><itemref idref=\"c0001\"/><itemref idref=\"c0002\"/>"
        ));
        // Chapter 1 is the oldest article.
        assert!(entry(&dest, "OEBPS/chapter0001.xhtml").contains("<h1>Older</h1>"));
        let nav = entry(&dest, "OEBPS/nav.xhtml");
        let older = nav.find("Older").unwrap();
        let newer = nav.find("Newer &amp; &lt;b&gt;").unwrap();
        assert!(older < newer, "nav must follow reading order: {nav}");
    }

    #[test]
    fn test_epub_metadata() {
        let (_dir, dest) = export(&two_articles(), "Tom & Jerry");
        let opf = entry(&dest, "OEBPS/content.opf");
        assert!(opf.contains("<dc:title>Tom &amp; Jerry</dc:title>"));
        assert!(opf.contains("<dc:language>en</dc:language>"));
        // Newest article date, in the exact format EPUB requires.
        assert!(opf.contains("<meta property=\"dcterms:modified\">2024-01-15T00:00:00Z</meta>"));
        let expected_id = format!("urn:xxh3:{:016x}", XxHash3_64::oneshot(b"Tom & Jerry"));
        assert!(opf.contains(&format!(">{expected_id}</dc:identifier>")));
        assert!(entry(&dest, "OEBPS/toc.ncx").contains(&expected_id));
    }

    #[test]
    fn test_epub_without_articles_is_still_valid() {
        let (_dir, dest) = export(&[], "Empty");
        let opf = entry(&dest, "OEBPS/content.opf");
        // Spine and navigation lists must never be empty.
        assert!(opf.contains("<spine toc=\"ncx\"><itemref idref=\"nav\"/></spine>"));
        assert!(entry(&dest, "OEBPS/nav.xhtml").contains("<li>"));
        assert!(entry(&dest, "OEBPS/toc.ncx").contains("<navPoint"));
        for name in entry_names(&dest).iter().filter(|n| *n != "mimetype") {
            assert_well_formed(name, &entry(&dest, name));
        }
    }

    #[test]
    fn test_epub_output_is_deterministic() {
        // Same input → same bytes, so unchanged groups don't churn the SD card.
        let (_first_dir, first) = export(&two_articles(), "Book");
        let (_second_dir, second) = export(&two_articles(), "Book");
        assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
    }

    #[test]
    fn test_epub_chapters_use_html5_doctype() {
        // EPUB 3 content documents are XHTML5; the XHTML 1.1 DOCTYPE is an error.
        let xhtml = article_to_xhtml(&make_article(1, "T", "https://example.com/1", 0));
        assert!(xhtml.contains("<!DOCTYPE html>"));
        assert!(!xhtml.contains("XHTML 1.1"));
    }
}
