use std::{
    collections::HashMap,
    fs,
    io::{BufWriter, Write},
    path::Path,
};

use quick_xml::Writer;
use tracing::info;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    error::FrustError,
    export::atom::write_atom_to,
    model::{Enrichment, Group},
    storage::Storage,
};

/// Derive the Atom filename that will appear inside the ZIP for a given group.
///
/// Uses the basename of the configured `output` path (e.g. `divers.atom`) when
/// set, otherwise falls back to `{slug}.atom`.
fn group_atom_name(group: &Group) -> String {
    if !group.output.is_empty() {
        Path::new(&group.output)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.atom", group.slug))
    } else {
        format!("{}.atom", group.slug)
    }
}

/// Build a ZIP archive at `output_path` containing:
/// - one Atom 1.0 file per group (articles from all feeds in the group, newest-first)
/// - every file from `{app.output}/media/` under a `media/` prefix
///
/// The group/feed structure and the redb paths are derived from the YAML config
/// at `config_path` (defaults to `"config.yaml"` when called from the CLI).
pub(crate) fn build_zip_archive(output_path: &str, config_path: &str) -> Result<(), FrustError> {
    let app = crate::config::load_config_file(config_path.to_string());

    let articles_path = format!("{}/articles.redb", app.output);
    let states_path = format!("{}/states.redb", app.output);
    let storage = Storage::new(&articles_path, &states_path)?;

    if let Some(parent) = Path::new(output_path).parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let zip_file = fs::File::create(output_path)?;
    let mut zip = ZipWriter::new(BufWriter::new(zip_file));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // ── one Atom file per group ───────────────────────────────────────────────
    for group in app.groups.values() {
        let mut articles = Vec::new();
        let mut enrichments: HashMap<u64, Enrichment> = HashMap::new();

        for (feed_id, feed) in &group.feeds {
            let mut feed_articles = storage.load_articles_for_feed(*feed_id)?;
            enrichments.insert(
                *feed_id,
                Enrichment {
                    feed_title: feed.title.clone(),
                    feed_url: feed.url.clone(),
                    feed_slug: feed.slug.clone(),
                    feed_page_url: feed.page_url.clone(),
                    prepend: feed.enrichment_prepend.clone(),
                    append: feed.enrichment_append.clone(),
                },
            );
            articles.append(&mut feed_articles);
        }

        // newest first
        articles.sort_by_key(|a| std::cmp::Reverse(a.timestamp));

        // generate Atom XML in memory
        let buf = Vec::<u8>::new();
        let mut writer = Writer::new_with_indent(buf, b' ', 2);
        write_atom_to(
            &mut writer,
            &articles,
            &group.title,
            &group.output,
            &enrichments,
        )?;
        let xml_bytes = writer.into_inner();

        let atom_name = group_atom_name(group);
        info!("zip: adding {} ({} articles)", atom_name, articles.len());
        zip.start_file(&atom_name, opts)
            .map_err(|e| FrustError::Export(e.to_string()))?;
        zip.write_all(&xml_bytes)?;
    }

    // ── media assets ─────────────────────────────────────────────────────────
    let media_dir = format!("{}/media", app.output);
    if Path::new(&media_dir).is_dir() {
        for entry in fs::read_dir(&media_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = format!("media/{}", entry.file_name().to_string_lossy());
            info!("zip: adding {}", name);
            zip.start_file(&name, opts)
                .map_err(|e| FrustError::Export(e.to_string()))?;
            zip.write_all(&fs::read(&path)?)?;
        }
    }

    zip.finish()
        .map_err(|e| FrustError::Export(e.to_string()))?;
    info!("zip archive written to {}", output_path);
    Ok(())
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, io::Read};

    use slug::slugify;
    use twox_hash::XxHash3_64;
    use zip::ZipArchive;

    use crate::{model::Article, storage::Storage};

    use super::*;

    // ── group_atom_name ───────────────────────────────────────────────────────

    fn make_group(slug: &str, output: &str) -> Group {
        Group {
            slug: slug.to_string(),
            output: output.to_string(),
            ..Group::default()
        }
    }

    #[test]
    fn test_group_atom_name_uses_output_field() {
        assert_eq!(
            group_atom_name(&make_group("tech", "tech.atom")),
            "tech.atom"
        );
    }

    #[test]
    fn test_group_atom_name_strips_directory_components() {
        assert_eq!(
            group_atom_name(&make_group("tech", "/some/path/tech.atom")),
            "tech.atom"
        );
    }

    #[test]
    fn test_group_atom_name_falls_back_to_slug() {
        assert_eq!(group_atom_name(&make_group("my-feed", "")), "my-feed.atom");
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn unique_dir(prefix: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("/tmp/frust_zip_{}_{}", prefix, nanos)
    }

    /// Write a minimal YAML config to `{out_dir}/config.yaml` and return its path.
    /// `groups` is a slice of `(slug, title, feed_url)`.
    fn write_config(out_dir: &str, groups: &[(&str, &str, &str)]) -> String {
        let mut lines = vec![format!("output: {}", out_dir), "groups:".to_string()];
        for (slug, title, url) in groups {
            lines.push(format!("- title: {}", title));
            lines.push(format!("  slug: {}", slug));
            lines.push(format!("  output: {}.atom", slug));
            lines.push("  feeds:".to_string());
            lines.push("  - title: Feed".to_string());
            lines.push(format!("    url: {}", url));
            lines.push("    page_url: https://example.com".to_string());
        }
        fs::create_dir_all(out_dir).unwrap();
        let path = format!("{}/config.yaml", out_dir);
        fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    /// Compute the feed_id the same way config loading does:
    /// slugify the URL hostname, then XXH3-hash it.
    fn feed_id_for_url(url: &str) -> u64 {
        let parsed = url::Url::parse(url).unwrap();
        let slug = slugify(parsed.host_str().unwrap_or("no-host"));
        XxHash3_64::oneshot(slug.as_bytes())
    }

    fn make_article(id: u64, feed_id: u64, title: &str, ts: i64) -> Article {
        Article {
            id,
            feed_id,
            title: title.to_string(),
            url: format!("https://example.com/{}", id),
            content: "Content".to_string(),
            summary: None,
            timestamp: ts,
            added_at: ts,
            is_full_content: false,
            enclosures: vec![],
        }
    }

    /// Return the set of entry names stored in a ZIP file.
    fn zip_names(zip_path: &str) -> HashSet<String> {
        let mut archive = ZipArchive::new(fs::File::open(zip_path).unwrap()).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect()
    }

    /// Read a named entry from a ZIP as UTF-8 text.
    fn zip_entry(zip_path: &str, name: &str) -> String {
        let mut archive = ZipArchive::new(fs::File::open(zip_path).unwrap()).unwrap();
        let mut entry = archive.by_name(name).unwrap();
        let mut buf = String::new();
        entry.read_to_string(&mut buf).unwrap();
        buf
    }

    // ── build_zip_archive ─────────────────────────────────────────────────────

    #[test]
    fn test_build_zip_creates_output_file() {
        let dir = unique_dir("basic");
        let cfg = write_config(&dir, &[("tech", "Tech", "https://linuxfr.org/news.atom")]);
        let out = format!("{}/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();
        assert!(Path::new(&out).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_zip_creates_parent_dirs() {
        let dir = unique_dir("parents");
        let cfg = write_config(&dir, &[("tech", "Tech", "https://linuxfr.org/news.atom")]);
        let out = format!("{}/sub/nested/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();
        assert!(Path::new(&out).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_zip_contains_one_atom_per_group() {
        let dir = unique_dir("groups");
        let cfg = write_config(
            &dir,
            &[
                ("tech", "Tech", "https://linuxfr.org/news.atom"),
                ("music", "Music", "https://music.example.com/feed"),
            ],
        );
        let out = format!("{}/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();
        let names = zip_names(&out);
        assert!(
            names.contains("tech.atom"),
            "missing tech.atom: {:?}",
            names
        );
        assert!(
            names.contains("music.atom"),
            "missing music.atom: {:?}",
            names
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_zip_atom_is_valid_atom_xml() {
        let dir = unique_dir("xml");
        let cfg = write_config(&dir, &[("news", "News", "https://linuxfr.org/news.atom")]);
        let out = format!("{}/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();
        let xml = zip_entry(&out, "news.atom");
        // Well-formed Atom 1.0 document
        assert!(xml.starts_with("<?xml"), "missing xml declaration");
        assert!(
            xml.contains("xmlns=\"http://www.w3.org/2005/Atom\""),
            "missing Atom namespace"
        );
        assert!(xml.contains("<title>"), "missing title element");
        assert!(xml.contains("</feed>"), "missing closing feed tag");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_zip_articles_appear_in_atom() {
        let feed_url = "https://linuxfr.org/news.atom";
        let dir = unique_dir("articles");
        let cfg = write_config(&dir, &[("news", "News", feed_url)]);

        let fid = feed_id_for_url(feed_url);
        let storage = Storage::new(
            &format!("{}/articles.redb", dir),
            &format!("{}/states.redb", dir),
        )
        .unwrap();
        storage
            .upsert_articles(vec![make_article(1, fid, "Hello ZIP World", 1_705_276_800)])
            .unwrap();
        drop(storage);

        let out = format!("{}/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();
        let xml = zip_entry(&out, "news.atom");
        assert!(
            xml.contains("<title>Hello ZIP World</title>"),
            "article missing:\n{}",
            xml
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_zip_articles_sorted_newest_first() {
        let feed_url = "https://linuxfr.org/news.atom";
        let dir = unique_dir("sort");
        let cfg = write_config(&dir, &[("news", "News", feed_url)]);

        let fid = feed_id_for_url(feed_url);
        let storage = Storage::new(
            &format!("{}/articles.redb", dir),
            &format!("{}/states.redb", dir),
        )
        .unwrap();
        storage
            .upsert_articles(vec![
                make_article(1, fid, "Older Article", 1_700_000_000),
                make_article(2, fid, "Newer Article", 1_705_276_800),
            ])
            .unwrap();
        drop(storage);

        let out = format!("{}/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();
        let xml = zip_entry(&out, "news.atom");
        let pos_newer = xml.find("Newer Article").unwrap();
        let pos_older = xml.find("Older Article").unwrap();
        assert!(
            pos_newer < pos_older,
            "newest should appear before oldest in atom output"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_zip_no_media_dir_produces_no_media_entries() {
        let dir = unique_dir("nomedia");
        let cfg = write_config(&dir, &[("tech", "Tech", "https://linuxfr.org/news.atom")]);
        let out = format!("{}/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();
        let names = zip_names(&out);
        assert!(
            !names.iter().any(|n| n.starts_with("media/")),
            "unexpected media entries: {:?}",
            names
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_zip_media_files_included_under_media_prefix() {
        let dir = unique_dir("media");
        let cfg = write_config(&dir, &[("tech", "Tech", "https://linuxfr.org/news.atom")]);

        let media_dir = format!("{}/media", dir);
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(format!("{}/abcd1234abcd1234.jpg", media_dir), b"fake-image").unwrap();
        fs::write(format!("{}/deadbeefdeadbeef.png", media_dir), b"fake-png").unwrap();

        let out = format!("{}/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();
        let names = zip_names(&out);
        assert!(
            names.contains("media/abcd1234abcd1234.jpg"),
            "jpg missing: {:?}",
            names
        );
        assert!(
            names.contains("media/deadbeefdeadbeef.png"),
            "png missing: {:?}",
            names
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_zip_media_content_preserved() {
        let dir = unique_dir("mediacontent");
        let cfg = write_config(&dir, &[("tech", "Tech", "https://linuxfr.org/news.atom")]);

        let media_dir = format!("{}/media", dir);
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(
            format!("{}/abcd1234abcd1234.jpg", media_dir),
            b"image-bytes",
        )
        .unwrap();

        let out = format!("{}/archive.zip", dir);
        build_zip_archive(&out, &cfg).unwrap();

        let mut archive = ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
        let mut entry = archive.by_name("media/abcd1234abcd1234.jpg").unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"image-bytes");
        let _ = fs::remove_dir_all(&dir);
    }
}
