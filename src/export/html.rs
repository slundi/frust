//! Markdown → HTML for exports, safe to embed in a web page or an EPUB.
//!
//! Article Markdown is rebuilt from feed content and enrichment templates
//! (`{{article.url}}` is feed-controlled), and pulldown-cmark copies raw HTML
//! and every link scheme verbatim. A static page consuming the JSON feed would
//! then run `<script>` or `javascript:` links from any feed. So:
//!
//! * raw HTML, block or inline, is emitted as escaped text;
//! * links and images whose URL is neither relative nor `http`/`https`/`mailto`
//!   are unwrapped: the link text / alt text is kept as plain text.
//!
//! pulldown-cmark writes self-closing void tags (`<br />`, `<img … />`), so
//! without raw HTML the output is also well-formed XHTML for strict e-readers.

use pulldown_cmark::{Event, Parser, Tag, TagEnd, html};

const SAFE_SCHEMES: &[&str] = &["http", "https", "mailto"];

/// `true` for relative references and `http`/`https`/`mailto` URLs.
pub(crate) fn is_safe_link(url: &str) -> bool {
    // Browsers drop ASCII whitespace and control characters inside the scheme
    // (`java\tscript:`), so judge the URL without them.
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_ascii_control())
        .collect();
    match cleaned.find([':', '/', '?', '#']) {
        Some(pos) if cleaned.as_bytes()[pos] == b':' => SAFE_SCHEMES
            .iter()
            .any(|scheme| cleaned[..pos].eq_ignore_ascii_case(scheme)),
        // No scheme before the first `/`, `?` or `#`: a relative reference.
        _ => true,
    }
}

/// Render Markdown to an HTML fragment without raw HTML or unsafe links.
pub(crate) fn markdown_to_safe_html(md: &str) -> String {
    // One entry per open link/image: whether its tags were dropped, so the
    // matching end tag is dropped too.
    let mut dropped: Vec<bool> = Vec::new();
    let events = Parser::new(md).filter_map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Some(Event::Text(raw)),
        Event::Start(Tag::Link { ref dest_url, .. } | Tag::Image { ref dest_url, .. }) => {
            let drop_tag = !is_safe_link(dest_url);
            dropped.push(drop_tag);
            (!drop_tag).then_some(event)
        }
        Event::End(TagEnd::Link | TagEnd::Image) => {
            let drop_tag = dropped.pop().unwrap_or(false);
            (!drop_tag).then_some(event)
        }
        other => Some(other),
    });
    let mut out = String::with_capacity(md.len());
    html::push_html(&mut out, events);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_safe_link ---

    #[test]
    fn test_safe_link_accepts_web_and_mail_schemes() {
        for url in [
            "https://example.com/a",
            "http://example.com",
            "HTTPS://EXAMPLE.COM",
            "mailto:me@example.com",
        ] {
            assert!(is_safe_link(url), "{url}");
        }
    }

    #[test]
    fn test_safe_link_accepts_relative_references() {
        for url in [
            "media/abcd1234abcd1234.png",
            "/divers",
            "#section",
            "?page=2",
            "../up",
            "path/with:colon",
            "",
        ] {
            assert!(is_safe_link(url), "{url}");
        }
    }

    #[test]
    fn test_safe_link_rejects_script_schemes() {
        for url in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " javascript:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            "\u{1}javascript:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            ":nothing",
        ] {
            assert!(!is_safe_link(url), "{url:?}");
        }
    }

    // --- markdown_to_safe_html ---

    #[test]
    fn test_renders_regular_markdown() {
        let out = markdown_to_safe_html("## Title\n\nSome **bold** [link](https://x.org/a).");
        assert!(out.contains("<h2>Title</h2>"));
        assert!(out.contains("<strong>bold</strong>"));
        assert!(out.contains(r#"<a href="https://x.org/a">link</a>"#));
    }

    #[test]
    fn test_escapes_block_html() {
        let out = markdown_to_safe_html("<script>alert(1)</script>\n\ntext");
        assert!(!out.contains("<script>"));
        assert!(out.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn test_escapes_inline_html() {
        let out = markdown_to_safe_html(r#"hello <img src=x onerror="alert(1)"> world"#);
        assert!(!out.contains("<img"));
        assert!(out.contains("&lt;img src=x onerror="));
    }

    #[test]
    fn test_unwraps_javascript_link_keeping_text() {
        let out = markdown_to_safe_html("[read more](javascript:alert(1))");
        assert!(!out.contains("javascript:"));
        assert!(!out.contains("<a"));
        assert!(out.contains("read more"));
    }

    #[test]
    fn test_unwraps_entity_encoded_javascript_link() {
        // pulldown-cmark decodes entities in link destinations.
        let out = markdown_to_safe_html("[x](javascript&#58;alert(1))");
        assert!(!out.contains("<a"));
    }

    #[test]
    fn test_unwraps_javascript_autolink() {
        let out = markdown_to_safe_html("<javascript:alert(1)>");
        assert!(!out.contains("<a"));
    }

    #[test]
    fn test_unwraps_unsafe_image_keeping_alt_text() {
        let out = markdown_to_safe_html("![a cat](javascript:alert(1))");
        assert!(!out.contains("<img"));
        assert!(out.contains("a cat"));
    }

    #[test]
    fn test_keeps_relative_image() {
        let out = markdown_to_safe_html("![cat](media/abcd1234abcd1234.png)");
        assert!(out.contains(r#"<img src="media/abcd1234abcd1234.png" alt="cat" />"#));
    }

    #[test]
    fn test_safe_link_after_unsafe_one_is_kept() {
        // The dropped-tag stack must stay aligned across siblings and nesting.
        let out = markdown_to_safe_html(
            "[bad](javascript:x) then [good](https://x.org) and \
             [![img](https://x.org/i.png)](javascript:y)",
        );
        assert!(out.contains(r#"<a href="https://x.org">good</a>"#));
        assert!(out.contains(r#"<img src="https://x.org/i.png" alt="img" />"#));
        assert!(!out.contains("javascript:"));
    }

    #[test]
    fn test_output_is_well_formed_xml() {
        let md = "line one  \nline two\n\n<br>\n\n---\n\n![i](https://x.org/i.png)";
        let html = markdown_to_safe_html(md);
        let wrapped = format!("<div>{html}</div>");
        let mut reader = quick_xml::Reader::from_str(&wrapped);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("not well-formed XHTML: {e}\n{wrapped}"),
            }
        }
    }
}
