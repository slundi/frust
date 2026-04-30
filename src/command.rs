use std::fs;
use std::io::BufWriter;

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::cli::{ExportOpts, ImportOpts};
use crate::error::FrustError;
use crate::model::{App, Group};

fn write_text_elem<W: std::io::Write>(
    writer: &mut Writer<W>,
    tag: &str,
    text: &str,
) -> Result<(), FrustError> {
    writer.write_event(Event::Start(BytesStart::new(tag)))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

fn write_opml<W: std::io::Write>(writer: &mut Writer<W>, app: &App) -> Result<(), FrustError> {
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut opml_tag = BytesStart::new("opml");
    opml_tag.push_attribute(("version", "2.0"));
    writer.write_event(Event::Start(opml_tag))?;

    writer.write_event(Event::Start(BytesStart::new("head")))?;
    write_text_elem(writer, "title", "frust feeds")?;
    writer.write_event(Event::End(BytesEnd::new("head")))?;

    writer.write_event(Event::Start(BytesStart::new("body")))?;

    let mut groups: Vec<&Group> = app.groups.values().collect();
    groups.sort_by(|a, b| a.slug.cmp(&b.slug));

    for group in groups {
        let label = if group.title.is_empty() { &group.slug } else { &group.title };

        let mut group_outline = BytesStart::new("outline");
        group_outline.push_attribute(("text", label.as_str()));
        group_outline.push_attribute(("title", label.as_str()));
        writer.write_event(Event::Start(group_outline))?;

        let mut feeds: Vec<_> = group.feeds.values().collect();
        feeds.sort_by(|a, b| a.title.cmp(&b.title));

        for feed in feeds {
            let mut feed_outline = BytesStart::new("outline");
            feed_outline.push_attribute(("type", "rss"));
            feed_outline.push_attribute(("text", feed.title.as_str()));
            feed_outline.push_attribute(("title", feed.title.as_str()));
            feed_outline.push_attribute(("xmlUrl", feed.url.as_str()));
            if !feed.page_url.is_empty() {
                feed_outline.push_attribute(("htmlUrl", feed.page_url.as_str()));
            }
            writer.write_event(Event::Empty(feed_outline))?;
        }

        writer.write_event(Event::End(BytesEnd::new("outline")))?;
    }

    writer.write_event(Event::End(BytesEnd::new("body")))?;
    writer.write_event(Event::End(BytesEnd::new("opml")))?;
    Ok(())
}

/// `frust import OUTPUT OPML_FILE [OPML_FILE…]`
///
/// Parses one or more OPML files and writes a base YAML configuration to OUTPUT.
pub fn import(opts: &ImportOpts) -> Result<(), FrustError> {
    let output = opts.output().ok_or_else(|| {
        FrustError::Config(
            "usage: frust import OUTPUT OPML_FILE [OPML_FILE…]".to_string(),
        )
    })?;
    let opml_files = opts.opml_files();
    if opml_files.is_empty() {
        return Err(FrustError::Config(
            "usage: frust import OUTPUT OPML_FILE [OPML_FILE…]".to_string(),
        ));
    }
    tracing::info!("Importing {} OPML file(s) → {}", opml_files.len(), output);
    // TODO: parse each OPML file, collect feed entries, emit a YAML config skeleton
    todo!("OPML → YAML config import is not yet implemented")
}

/// `frust export OUTPUT CONFIG_FILE`
///
/// Loads the YAML configuration from CONFIG_FILE and writes an OPML 2.0 file to OUTPUT.
/// Groups become container outlines; feeds become `type="rss"` leaf outlines.
pub fn export_opml(opts: &ExportOpts) -> Result<(), FrustError> {
    let output = opts.output().ok_or_else(|| {
        FrustError::Config("usage: frust export OUTPUT CONFIG_FILE".to_string())
    })?;
    let config_file = opts.config_file().ok_or_else(|| {
        FrustError::Config("usage: frust export OUTPUT CONFIG_FILE".to_string())
    })?;

    tracing::info!("Loading config from {}", config_file);
    let app = crate::config::load_config_file(config_file.to_string());

    if let Some(parent) = std::path::Path::new(output).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let file = fs::File::create(output)?;
    let mut writer = Writer::new_with_indent(BufWriter::new(file), b' ', 2);
    write_opml(&mut writer, &app)?;

    tracing::info!("OPML written to {}", output);
    Ok(())
}

/// `frust export OUTPUT`
///
/// Reads all articles and media references from the redb database (no cleanup)
/// and writes them into a zip archive at OUTPUT.
pub fn archive(opts: &ExportOpts) -> Result<(), FrustError> {
    let output = opts.output().ok_or_else(|| {
        FrustError::Config("usage: frust export OUTPUT".to_string())
    })?;
    tracing::info!("Building zip archive → {}", output);
    // TODO: open redb, iterate all articles + media, write zip
    todo!("Zip archive export is not yet implemented")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use quick_xml::Writer;

    use super::write_opml;
    use crate::model::{App, ContentMode, Feed, Group};

    fn make_feed(title: &str, url: &str, page_url: &str) -> Feed {
        Feed {
            title: title.to_string(),
            slug: slug::slugify(title),
            url: url.to_string(),
            page_url: page_url.to_string(),
            content_mode: ContentMode::Default,
            selector: None,
            filters: Vec::new(),
            output: String::new(),
            retention: 0,
            media: false,
            media_max_size: 0,
            enrichment_prepend: None,
            enrichment_append: None,
            last_etag: None,
            last_modified: None,
            last_check: None,
        }
    }

    fn make_group(slug: &str, feeds: Vec<Feed>) -> Group {
        let mut map = HashMap::new();
        for f in feeds {
            let key = twox_hash::XxHash3_64::oneshot(f.slug.as_bytes());
            map.insert(key, f);
        }
        Group {
            slug: slug.to_string(),
            feeds: map,
            ..Default::default()
        }
    }

    fn opml_from_app(app: &App) -> String {
        let mut buf = Vec::new();
        let mut writer = Writer::new_with_indent(&mut buf, b' ', 2);
        write_opml(&mut writer, app).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn build_app(groups: Vec<Group>) -> App {
        let mut map = HashMap::new();
        for g in groups {
            let key = twox_hash::XxHash3_64::oneshot(g.slug.as_bytes());
            map.insert(key, g);
        }
        App {
            groups: map,
            ..App::default()
        }
    }

    #[test]
    fn test_opml_document_structure() {
        let app = build_app(vec![]);
        let xml = opml_from_app(&app);
        assert!(xml.contains(r#"<opml version="2.0">"#));
        assert!(xml.contains("<head>"));
        assert!(xml.contains("<title>frust feeds</title>"));
        assert!(xml.contains("</head>"));
        assert!(xml.contains("<body>"));
        assert!(xml.contains("</body>"));
        assert!(xml.contains("</opml>"));
    }

    #[test]
    fn test_groups_sorted_by_slug() {
        let app = build_app(vec![
            make_group("zebra", vec![make_feed("Z Feed", "https://z.example/feed", "")]),
            make_group("alpha", vec![make_feed("A Feed", "https://a.example/feed", "")]),
        ]);
        let xml = opml_from_app(&app);
        let pos_alpha = xml.find("alpha").unwrap();
        let pos_zebra = xml.find("zebra").unwrap();
        assert!(pos_alpha < pos_zebra, "groups must be sorted alphabetically by slug");
    }

    #[test]
    fn test_feeds_sorted_by_title_within_group() {
        let app = build_app(vec![make_group(
            "news",
            vec![
                make_feed("Zebra News", "https://z.example/feed", ""),
                make_feed("Alpha News", "https://a.example/feed", ""),
            ],
        )]);
        let xml = opml_from_app(&app);
        let pos_alpha = xml.find("Alpha News").unwrap();
        let pos_zebra = xml.find("Zebra News").unwrap();
        assert!(pos_alpha < pos_zebra, "feeds must be sorted alphabetically by title");
    }

    #[test]
    fn test_feed_outline_attributes() {
        let app = build_app(vec![make_group(
            "tech",
            vec![make_feed("My Feed", "https://example.com/feed.xml", "https://example.com/")],
        )]);
        let xml = opml_from_app(&app);
        assert!(xml.contains(r#"type="rss""#));
        assert!(xml.contains(r#"xmlUrl="https://example.com/feed.xml""#));
        assert!(xml.contains(r#"htmlUrl="https://example.com/""#));
    }

    #[test]
    fn test_html_url_omitted_when_page_url_empty() {
        let app = build_app(vec![make_group(
            "tech",
            vec![make_feed("No Homepage", "https://example.com/feed.xml", "")],
        )]);
        let xml = opml_from_app(&app);
        assert!(!xml.contains("htmlUrl"), "htmlUrl must not appear when page_url is empty");
    }

    #[test]
    fn test_group_uses_slug_as_label_when_title_absent() {
        let app = build_app(vec![make_group("my-group", vec![])]);
        let xml = opml_from_app(&app);
        assert!(xml.contains(r#"text="my-group""#));
        assert!(xml.contains(r#"title="my-group""#));
    }
}
