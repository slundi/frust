pub(crate) mod atom;
pub(crate) mod epub;
pub(crate) mod html;
pub(crate) mod json;
pub(crate) mod markdown;
pub(crate) mod rss;
pub(crate) mod zip;

pub(crate) use atom::AtomExporter;
pub(crate) use epub::EpubExporter;
pub(crate) use json::JsonExporter;
pub(crate) use markdown::MarkdownExporter;
pub(crate) use rss::RssExporter;

use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File},
    io::BufWriter,
    path::Path,
};

use crate::{
    error::FrustError,
    model::{Article, Enrichment},
};

/// Write `destination` atomically: `write` fills a hidden temporary file in
/// the same directory (`.<name>.tmp`), which is flushed, synced, then renamed
/// over `destination`.
///
/// The web server never serves a half-written file, a failed export leaves
/// the previous one in place, and write errors (full SD card) are reported
/// instead of being lost in `BufWriter`'s drop.
pub(crate) fn write_atomically<T>(
    destination: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<T, FrustError>,
) -> Result<T, FrustError> {
    let file_name = destination.file_name().ok_or_else(|| {
        FrustError::Export(format!("{} is not a file path", destination.display()))
    })?;
    let parent = destination
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut tmp_name = OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(".tmp");
    let tmp_path = parent.join(tmp_name);

    let result = (|| {
        let mut writer = BufWriter::new(File::create(&tmp_path)?);
        let value = write(&mut writer)?;
        writer
            .into_inner()
            .map_err(|e| e.into_error())?
            .sync_all()?;
        fs::rename(&tmp_path, destination)?;
        Ok(value)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

/// Substitute `{{key}}` placeholders in `template` using feed + article data.
pub(crate) fn render_template(template: &str, e: &Enrichment, article: &Article) -> String {
    template
        .replace("{{feed.title}}", &e.feed_title)
        .replace("{{feed.url}}", &e.feed_url)
        .replace("{{feed.slug}}", &e.feed_slug)
        .replace("{{feed.page_url}}", &e.feed_page_url)
        .replace("{{article.title}}", &article.title)
        .replace("{{article.url}}", &article.url)
        .replace("{{article.id}}", &article.id.to_string())
}

pub(crate) trait Exporter {
    /// `articles`:     items to export.
    /// `title`:        channel/document title (group or feed name).
    /// `link`:         canonical URL of the channel (base URL of the output site).
    /// `destination`:  for Monolithic, path to the output file; for Individual/Daily, path to the output directory.
    /// `enrichments`:  per-feed enrichment config keyed by `Article::feed_id`.
    ///                 RSS, Atom and JSON exporters inject the rendered prepend/append;
    ///                 other exporters may ignore it.
    fn generate(
        &self,
        articles: &[Article],
        title: &str,
        link: &str,
        destination: &Path,
        enrichments: &HashMap<u64, Enrichment>,
    ) -> Result<(), FrustError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Article;
    use std::io::Write;

    // ---- write_atomically (B9) ----

    fn dir_entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn test_write_atomically_writes_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("feed.atom");
        let value = write_atomically(&dest, |out| {
            out.write_all(b"<feed/>")?;
            Ok(42)
        })
        .unwrap();
        assert_eq!(value, 42);
        assert_eq!(fs::read(&dest).unwrap(), b"<feed/>");
        assert_eq!(dir_entries(dir.path()), vec!["feed.atom"]);
    }

    #[test]
    fn test_write_atomically_keeps_previous_file_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("feed.atom");
        fs::write(&dest, b"previous").unwrap();

        let result: Result<(), FrustError> = write_atomically(&dest, |out| {
            out.write_all(b"<feed><entry>half")?;
            Err(FrustError::Export("boom".into()))
        });

        assert!(result.is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"previous", "never truncated");
        assert_eq!(dir_entries(dir.path()), vec!["feed.atom"], "temp removed");
    }

    #[test]
    fn test_write_atomically_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("feed.rss");
        fs::write(&dest, b"old content that is longer").unwrap();
        write_atomically(&dest, |out| Ok(out.write_all(b"new")?)).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
    }

    #[test]
    fn test_write_atomically_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a/b/feed.json");
        write_atomically(&dest, |out| Ok(out.write_all(b"{}")?)).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"{}");
    }

    #[test]
    fn test_write_atomically_rejects_path_without_file_name() {
        let result = write_atomically(Path::new("/"), |out| Ok(out.write_all(b"x")?));
        assert!(matches!(result, Err(FrustError::Export(_))));
    }

    fn make_enrichment(prepend: Option<&str>, append: Option<&str>) -> Enrichment {
        Enrichment {
            feed_title: "My Feed".to_string(),
            feed_url: "https://example.com/feed.xml".to_string(),
            feed_slug: "example-com".to_string(),
            feed_page_url: "https://example.com".to_string(),
            prepend: prepend.map(str::to_string),
            append: append.map(str::to_string),
        }
    }

    fn make_article(id: u64) -> Article {
        Article {
            id,
            feed_id: 0,
            title: "Article Title".to_string(),
            url: "https://example.com/a/1".to_string(),
            content: String::new(),
            summary: None,
            timestamp: 0,
            added_at: 0,
            enclosures: vec![],
        }
    }

    #[test]
    fn test_render_template_all_placeholders() {
        let e = make_enrichment(None, None);
        let a = make_article(42);
        let out = render_template(
            "F={{feed.title}} U={{feed.url}} S={{feed.slug}} P={{feed.page_url}} \
             T={{article.title}} A={{article.url}} I={{article.id}}",
            &e,
            &a,
        );
        assert_eq!(
            out,
            "F=My Feed U=https://example.com/feed.xml S=example-com \
             P=https://example.com T=Article Title A=https://example.com/a/1 I=42"
        );
    }

    #[test]
    fn test_render_template_no_placeholders_is_verbatim() {
        let e = make_enrichment(None, None);
        let a = make_article(1);
        assert_eq!(render_template("static text", &e, &a), "static text");
    }

    #[test]
    fn test_render_template_unknown_placeholder_is_untouched() {
        let e = make_enrichment(None, None);
        let a = make_article(1);
        // {{unknown}} isn't in the substitution table — must be left as-is.
        assert_eq!(
            render_template("{{unknown}} vs {{feed.title}}", &e, &a),
            "{{unknown}} vs My Feed"
        );
    }

    #[test]
    fn test_render_template_repeated_placeholder_all_substituted() {
        let e = make_enrichment(None, None);
        let a = make_article(1);
        assert_eq!(
            render_template("{{feed.title}}-{{feed.title}}", &e, &a),
            "My Feed-My Feed"
        );
    }

    #[test]
    fn test_render_template_empty_template_yields_empty() {
        let e = make_enrichment(None, None);
        let a = make_article(1);
        assert_eq!(render_template("", &e, &a), "");
    }

    #[test]
    fn test_render_template_article_id_uses_decimal() {
        let e = make_enrichment(None, None);
        let mut a = make_article(0);
        a.id = 0xdead_beef;
        // XXH3 IDs are 64-bit unsigned — rendered as decimal, not hex.
        assert_eq!(render_template("{{article.id}}", &e, &a), "3735928559");
    }
}
