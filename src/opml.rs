use std::fs;

use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::error::FrustError;
use crate::model::{App, Group};

// ---------------------------------------------------------------------------
// OPML import helpers
// ---------------------------------------------------------------------------

pub(crate) struct ParsedFeed {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) page_url: String,
}

pub(crate) struct ParsedGroup {
    pub(crate) title: String,
    pub(crate) slug: String,
    pub(crate) feeds: Vec<ParsedFeed>,
}

/// Extract the four attributes we care about from an `<outline>` element.
/// Returns `(label, type, xmlUrl, htmlUrl)`.
fn extract_outline_attrs(
    e: &quick_xml::events::BytesStart<'_>,
) -> (String, String, String, String) {
    let mut text = String::new();
    let mut title = String::new();
    let mut outline_type = String::new();
    let mut xml_url = String::new();
    let mut html_url = String::new();

    for attr in e.attributes().flatten() {
        let key = attr.key.as_ref().to_lowercase();
        let val = attr.value.into_owned();
        match key.as_str() {
            "type" => outline_type = val,
            "text" => text = val,
            "title" => title = val,
            "xmlurl" => xml_url = val,
            "htmlurl" => html_url = val,
            _ => {}
        }
    }

    let label = if !title.is_empty() { title } else { text };
    (label, outline_type, xml_url, html_url)
}

/// Attach a parsed feed to `current_group` if one is open, otherwise route it
/// into the shared "Uncategorized" group (creating it if needed).
fn push_feed(
    feed: ParsedFeed,
    current_group: &mut Option<ParsedGroup>,
    groups: &mut Vec<ParsedGroup>,
) {
    if let Some(g) = current_group.as_mut() {
        g.feeds.push(feed);
        return;
    }
    match groups.iter_mut().find(|g| g.slug == "uncategorized") {
        Some(g) => g.feeds.push(feed),
        None => groups.push(ParsedGroup {
            title: "Uncategorized".to_string(),
            slug: "uncategorized".to_string(),
            feeds: vec![feed],
        }),
    }
}

/// Parse an OPML document from a string and return a flat list of groups with
/// their feeds. Feeds that appear directly under `<body>` (no enclosing group
/// outline) are placed in an "Uncategorized" group.
///
/// Handles both feed forms: `<outline xmlUrl="..."/>` (Empty event) and
/// `<outline xmlUrl="...">...</outline>` (Start event with children — emitted
/// by some readers such as older Feedly/Reeder exports).
fn parse_opml_str(content: &str) -> Result<Vec<ParsedGroup>, FrustError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut groups: Vec<ParsedGroup> = Vec::new();
    let mut current_group: Option<ParsedGroup> = None;
    let mut in_body = false;
    let mut outline_depth: i32 = 0;

    loop {
        match reader
            .read_event()
            .map_err(|e| FrustError::Config(e.to_string()))?
        {
            Event::Start(ref e) => {
                let local = e.local_name();
                let tag = local.as_ref().to_lowercase();
                match tag.as_str() {
                    "body" => in_body = true,
                    "outline" if in_body => {
                        outline_depth += 1;
                        let (label, _, xml_url, html_url) = extract_outline_attrs(e);
                        if xml_url.is_empty() {
                            // Group container — only the outermost creates a new group.
                            if outline_depth == 1 {
                                if let Some(g) = current_group.take() {
                                    groups.push(g);
                                }
                                current_group = Some(ParsedGroup {
                                    slug: slug::slugify(&label),
                                    title: label,
                                    feeds: Vec::new(),
                                });
                            }
                        } else {
                            // Feed leaf that carries children/whitespace — capture it
                            // just like the Empty-tag form.
                            push_feed(
                                ParsedFeed {
                                    title: label,
                                    url: xml_url,
                                    page_url: html_url,
                                },
                                &mut current_group,
                                &mut groups,
                            );
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(ref e) => {
                let local = e.local_name();
                let tag = local.as_ref().to_lowercase();
                if tag == "outline" && in_body {
                    let (label, _, xml_url, html_url) = extract_outline_attrs(e);
                    if !xml_url.is_empty() {
                        push_feed(
                            ParsedFeed {
                                title: label,
                                url: xml_url,
                                page_url: html_url,
                            },
                            &mut current_group,
                            &mut groups,
                        );
                    }
                }
            }
            Event::End(ref e) => {
                let local = e.local_name();
                let tag = local.as_ref().to_lowercase();
                match tag.as_str() {
                    "body" => {
                        in_body = false;
                        if let Some(g) = current_group.take() {
                            groups.push(g);
                        }
                    }
                    "outline" if in_body => {
                        outline_depth -= 1;
                        if outline_depth == 0
                            && let Some(g) = current_group.take()
                        {
                            groups.push(g);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(groups)
}

/// Read an OPML file from disk and delegate to [`parse_opml_str`].
pub(crate) fn parse_opml(path: &str) -> Result<Vec<ParsedGroup>, FrustError> {
    let content = fs::read_to_string(path)?;
    parse_opml_str(&content)
}

/// Double-quote a YAML scalar, escaping backslashes and double-quotes.
fn yaml_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Serialise the parsed groups into a minimal YAML config skeleton.
pub(crate) fn build_yaml(groups: &[ParsedGroup]) -> String {
    let mut out = String::from("groups:\n");
    for g in groups {
        out.push_str(&format!("- title: {}\n", yaml_quote(&g.title)));
        out.push_str(&format!("  slug: {}\n", g.slug));
        out.push_str(&format!("  output: {}.atom\n", g.slug));
        out.push_str("  feeds:\n");
        for f in &g.feeds {
            out.push_str(&format!("  - title: {}\n", yaml_quote(&f.title)));
            out.push_str(&format!("    url: {}\n", f.url));
            if !f.page_url.is_empty() {
                out.push_str(&format!("    page_url: {}\n", f.page_url));
            }
        }
    }
    out
}

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

pub(crate) fn write_opml<W: std::io::Write>(
    writer: &mut Writer<W>,
    app: &App,
) -> Result<(), FrustError> {
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
        let label = if group.title.is_empty() {
            &group.slug
        } else {
            &group.title
        };

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use quick_xml::Writer;

    use super::{build_yaml, parse_opml_str, write_opml};
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
            make_group(
                "zebra",
                vec![make_feed("Z Feed", "https://z.example/feed", "")],
            ),
            make_group(
                "alpha",
                vec![make_feed("A Feed", "https://a.example/feed", "")],
            ),
        ]);
        let xml = opml_from_app(&app);
        let pos_alpha = xml.find("alpha").unwrap();
        let pos_zebra = xml.find("zebra").unwrap();
        assert!(
            pos_alpha < pos_zebra,
            "groups must be sorted alphabetically by slug"
        );
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
        assert!(
            pos_alpha < pos_zebra,
            "feeds must be sorted alphabetically by title"
        );
    }

    #[test]
    fn test_feed_outline_attributes() {
        let app = build_app(vec![make_group(
            "tech",
            vec![make_feed(
                "My Feed",
                "https://example.com/feed.xml",
                "https://example.com/",
            )],
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
        assert!(
            !xml.contains("htmlUrl"),
            "htmlUrl must not appear when page_url is empty"
        );
    }

    #[test]
    fn test_group_uses_slug_as_label_when_title_absent() {
        let app = build_app(vec![make_group("my-group", vec![])]);
        let xml = opml_from_app(&app);
        assert!(xml.contains(r#"text="my-group""#));
        assert!(xml.contains(r#"title="my-group""#));
    }

    #[test]
    fn test_opml_write_escapes_xml_special_chars() {
        let app = build_app(vec![make_group(
            "news",
            vec![make_feed(
                "AT&T <News>",
                "https://example.com/feed?a=1&b=2",
                "https://example.com/?x=1&y=2",
            )],
        )]);
        let xml = opml_from_app(&app);
        // Title attribute must escape &, <, >
        assert!(
            xml.contains("AT&amp;T &lt;News&gt;"),
            "feed title not escaped: {}",
            xml
        );
        // xmlUrl / htmlUrl attributes must escape &
        assert!(
            xml.contains("a=1&amp;b=2"),
            "xmlUrl amp not escaped: {}",
            xml
        );
        assert!(
            xml.contains("x=1&amp;y=2"),
            "htmlUrl amp not escaped: {}",
            xml
        );
        // No raw '&' outside entity references
        assert!(!xml.contains("AT&T"), "raw '&' leaked: {}", xml);
    }

    // -----------------------------------------------------------------------
    // import: parse_opml_str
    // -----------------------------------------------------------------------

    const SAMPLE_OPML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head><title>Test</title></head>
  <body>
    <outline text="Tech" title="Tech">
      <outline type="rss" text="Rust Blog" title="Rust Blog"
               xmlUrl="https://blog.rust-lang.org/feed.xml"
               htmlUrl="https://blog.rust-lang.org/"/>
      <outline type="rss" text="LWN"
               xmlUrl="https://lwn.net/headlines/rss"/>
    </outline>
    <outline text="Music" title="Music">
      <outline type="rss" text="Pitchfork"
               xmlUrl="https://pitchfork.com/rss/news/"/>
    </outline>
  </body>
</opml>"#;

    #[test]
    fn test_parse_opml_groups() {
        let groups = parse_opml_str(SAMPLE_OPML).unwrap();
        assert_eq!(groups.len(), 2);
        let tech = groups
            .iter()
            .find(|g| g.slug == "tech")
            .expect("tech group");
        assert_eq!(tech.feeds.len(), 2);
        let music = groups
            .iter()
            .find(|g| g.slug == "music")
            .expect("music group");
        assert_eq!(music.feeds.len(), 1);
    }

    #[test]
    fn test_parse_opml_feed_fields() {
        let groups = parse_opml_str(SAMPLE_OPML).unwrap();
        let tech = groups.iter().find(|g| g.slug == "tech").unwrap();
        let rust = tech
            .feeds
            .iter()
            .find(|f| f.title == "Rust Blog")
            .expect("Rust Blog feed");
        assert_eq!(rust.url, "https://blog.rust-lang.org/feed.xml");
        assert_eq!(rust.page_url, "https://blog.rust-lang.org/");
    }

    #[test]
    fn test_parse_opml_missing_html_url() {
        let groups = parse_opml_str(SAMPLE_OPML).unwrap();
        let tech = groups.iter().find(|g| g.slug == "tech").unwrap();
        let lwn = tech
            .feeds
            .iter()
            .find(|f| f.title == "LWN")
            .expect("LWN feed");
        assert!(lwn.page_url.is_empty());
    }

    #[test]
    fn test_parse_opml_flat_feeds_go_to_uncategorized() {
        let opml = r#"<?xml version="1.0"?>
<opml version="2.0">
  <head><title>flat</title></head>
  <body>
    <outline type="rss" text="Flat Feed" xmlUrl="https://example.com/feed"/>
  </body>
</opml>"#;
        let groups = parse_opml_str(opml).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].slug, "uncategorized");
        assert_eq!(groups[0].feeds.len(), 1);
    }

    #[test]
    fn test_parse_opml_feed_as_start_form_inside_group() {
        // Some readers (older Feedly/Reeder) emit feeds as <outline …>…</outline>
        // (Start form with children/whitespace) instead of self-closing.
        let opml = r#"<?xml version="1.0"?>
<opml version="2.0">
  <head><title>t</title></head>
  <body>
    <outline text="Tech" title="Tech">
      <outline type="rss" text="Rust Blog" title="Rust Blog"
               xmlUrl="https://blog.rust-lang.org/feed.xml"
               htmlUrl="https://blog.rust-lang.org/">
      </outline>
    </outline>
  </body>
</opml>"#;
        let groups = parse_opml_str(opml).unwrap();
        let tech = groups.iter().find(|g| g.slug == "tech").expect("tech");
        assert_eq!(tech.feeds.len(), 1, "start-form feed must be captured");
        assert_eq!(tech.feeds[0].url, "https://blog.rust-lang.org/feed.xml");
        assert_eq!(tech.feeds[0].page_url, "https://blog.rust-lang.org/");
    }

    #[test]
    fn test_parse_opml_flat_feed_as_start_form() {
        // Start-form feed directly under <body> (no enclosing group container)
        // should land in Uncategorized, matching the Empty-form behavior.
        let opml = r#"<?xml version="1.0"?>
<opml version="2.0">
  <head><title>t</title></head>
  <body>
    <outline type="rss" text="LWN" xmlUrl="https://lwn.net/headlines/rss">
    </outline>
  </body>
</opml>"#;
        let groups = parse_opml_str(opml).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].slug, "uncategorized");
        assert_eq!(groups[0].feeds.len(), 1);
        assert_eq!(groups[0].feeds[0].url, "https://lwn.net/headlines/rss");
    }

    #[test]
    fn test_parse_opml_mixed_start_and_empty_forms() {
        let opml = r#"<?xml version="1.0"?>
<opml version="2.0">
  <head><title>t</title></head>
  <body>
    <outline text="Mixed" title="Mixed">
      <outline type="rss" text="Empty Form" xmlUrl="https://example.com/a"/>
      <outline type="rss" text="Start Form" xmlUrl="https://example.com/b">
      </outline>
    </outline>
  </body>
</opml>"#;
        let groups = parse_opml_str(opml).unwrap();
        let mixed = groups.iter().find(|g| g.slug == "mixed").unwrap();
        assert_eq!(mixed.feeds.len(), 2, "both forms must be captured");
        let urls: Vec<&str> = mixed.feeds.iter().map(|f| f.url.as_str()).collect();
        assert!(urls.contains(&"https://example.com/a"));
        assert!(urls.contains(&"https://example.com/b"));
    }

    #[test]
    fn test_parse_opml_empty_body() {
        let opml = r#"<?xml version="1.0"?>
<opml version="2.0"><head/><body/></opml>"#;
        let groups = parse_opml_str(opml).unwrap();
        assert!(groups.is_empty());
    }

    // -----------------------------------------------------------------------
    // import: build_yaml
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_yaml_structure() {
        let groups = parse_opml_str(SAMPLE_OPML).unwrap();
        let yaml = build_yaml(&groups);
        assert!(yaml.starts_with("groups:\n"));
        assert!(yaml.contains("slug: tech"));
        assert!(yaml.contains("output: tech.atom"));
        assert!(yaml.contains("slug: music"));
        assert!(yaml.contains("url: https://blog.rust-lang.org/feed.xml"));
        assert!(yaml.contains("page_url: https://blog.rust-lang.org/"));
    }

    #[test]
    fn test_build_yaml_omits_page_url_when_empty() {
        let groups = parse_opml_str(SAMPLE_OPML).unwrap();
        let yaml = build_yaml(&groups);
        // LWN has no htmlUrl — its entry must not emit a page_url line
        let lwn_pos = yaml.find("LWN").expect("LWN in yaml");
        let next_title_pos = yaml[lwn_pos..].find("\n  - title:").map(|p| lwn_pos + p);
        let lwn_block = match next_title_pos {
            Some(end) => &yaml[lwn_pos..end],
            None => &yaml[lwn_pos..],
        };
        assert!(
            !lwn_block.contains("page_url"),
            "page_url must be absent for LWN"
        );
    }

    #[test]
    fn test_build_yaml_quotes_special_chars() {
        let opml = r#"<?xml version="1.0"?>
<opml version="2.0">
  <head><title>t</title></head>
  <body>
    <outline text="Say &quot;Hello&quot;" title="Say &quot;Hello&quot;">
      <outline type="rss" text="Feed" xmlUrl="https://example.com/f"/>
    </outline>
  </body>
</opml>"#;
        let groups = parse_opml_str(opml).unwrap();
        let yaml = build_yaml(&groups);
        // The title must be wrapped in double-quotes in the YAML output.
        assert!(yaml.contains("title: \""), "title should be quoted");
    }
}
