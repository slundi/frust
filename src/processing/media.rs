// download_asset and rewrite_inline_images are WIP — not yet wired into the main pipeline
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use regex::Regex;
use reqwest::{Client, header};
use scraper::{Html, Selector};
use twox_hash::XxHash3_64;

/// Matches `src="..."` (group 1) or `src='...'` (group 2). The Rust `regex`
/// crate has no backreferences, so we spell both quote styles out as an
/// alternation instead of `src=(["'])([^"']+)\1`.
fn src_regex() -> &'static Regex {
    static SRC_RE: OnceLock<Regex> = OnceLock::new();
    SRC_RE.get_or_init(|| Regex::new(r#"src="([^"]+)"|src='([^']+)'"#).unwrap())
}

/// Map a MIME content-type string to a file extension.
fn mime_to_ext(content_type: &str) -> &'static str {
    match content_type.split(';').next().unwrap_or("").trim() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/avif" => "avif",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/ogg" => "ogg",
        "audio/flac" => "flac",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/aac" => "aac",
        "audio/mp4" => "m4a",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/ogg" => "ogv",
        _ => "bin",
    }
}

/// Try to extract a file extension from a URL path (ignores query string).
///
/// Extension must be non-empty, at most 5 characters, and consist only of
/// ASCII alphanumerics. Any URL-encoded or unusual character makes the URL
/// fall back to MIME-derived extensions — defense in depth against odd
/// filenames on disk and any theoretical path shenanigans.
fn ext_from_url(url: &str) -> Option<&str> {
    let path = url.split('?').next()?;
    let filename = path.rsplit('/').next()?;
    let dot = filename.rfind('.')?;
    let ext = &filename[dot + 1..];
    if ext.is_empty() || ext.len() > 5 {
        return None;
    }
    if !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext)
}

/// Download a single asset, deduplicate by XXH3 hash, and write to `media_dir/<hash>.<ext>`.
/// Returns the local path on success, `None` if skipped (size limit) or on error.
pub(crate) async fn download_asset(
    client: &Client,
    url: &str,
    media_dir: &Path,
    max_size: u64,
) -> Option<PathBuf> {
    let resp = client.get(url).send().await.ok()?;

    // Reject early based on Content-Length if available and a limit is set
    if max_size > 0
        && let Some(len) = resp.content_length()
        && len > max_size
    {
        tracing::warn!(
            "Skipping asset (declared {} bytes > limit {} bytes): {}",
            len,
            max_size,
            url
        );
        return None;
    }

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let bytes = resp.bytes().await.ok()?;

    // Reject after download if actual size exceeds limit (Content-Length may be absent)
    if max_size > 0 && bytes.len() as u64 > max_size {
        tracing::warn!(
            "Skipping asset (actual {} bytes > limit {} bytes): {}",
            bytes.len(),
            max_size,
            url
        );
        return None;
    }

    let hash = XxHash3_64::oneshot(&bytes);
    let ext = ext_from_url(url).unwrap_or_else(|| mime_to_ext(&content_type));
    let filename = format!("{:016x}.{}", hash, ext);
    let path = media_dir.join(&filename);

    // Skip write if already on disk (same hash = same content)
    if !tokio::fs::try_exists(&path).await.unwrap_or(false)
        && let Err(e) = tokio::fs::write(&path, &bytes).await
    {
        tracing::error!("Cannot write asset {}: {}", path.display(), e);
        return None;
    }

    Some(path)
}

/// Find all external `<img src="...">` in an HTML fragment, download them, and rewrite
/// their `src` to the local `media/<hash>.<ext>` path. Returns the rewritten HTML.
pub(crate) async fn rewrite_inline_images(
    client: &Client,
    html: &str,
    media_dir: &Path,
    max_size: u64,
) -> String {
    if let Err(e) = tokio::fs::create_dir_all(media_dir).await {
        tracing::error!("Cannot create media directory: {}", e);
        return html.to_string();
    }

    let document = Html::parse_fragment(html);
    let img_sel = Selector::parse("img").unwrap();

    // Collect unique external image URLs (HashSet deduplicates)
    let srcs: HashSet<String> = document
        .select(&img_sel)
        .filter_map(|img| img.value().attr("src"))
        .filter(|src| src.starts_with("http://") || src.starts_with("https://"))
        .map(|s| s.to_string())
        .collect();

    // Download once per unique src and build a URL → local path map so the
    // rewrite step below can run in a single pass over the HTML instead of
    // scanning the entire string twice per image (String::replace is
    // O(html_len × 2 × images) — that quadratic factor bit us on large
    // articles).
    let mut mapping: HashMap<String, String> = HashMap::new();
    for src in srcs {
        if let Some(path) = download_asset(client, &src, media_dir, max_size).await {
            let filename = path.file_name().unwrap().to_string_lossy().into_owned();
            mapping.insert(src, format!("media/{}", filename));
        }
    }

    if mapping.is_empty() {
        return html.to_string();
    }

    src_regex()
        .replace_all(html, |caps: &regex::Captures| {
            // Exactly one of the two alternation branches matched — pick the
            // right quote style so the rewrite round-trips faithfully.
            let (url, quote) = match (caps.get(1), caps.get(2)) {
                (Some(m), _) => (m.as_str(), '"'),
                (_, Some(m)) => (m.as_str(), '\''),
                _ => return caps[0].to_string(),
            };
            match mapping.get(url) {
                Some(local) => format!("src={}{}{}", quote, local, quote),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- mime_to_ext ---

    #[test]
    fn test_mime_to_ext_known_types() {
        assert_eq!(mime_to_ext("image/jpeg"), "jpg");
        assert_eq!(mime_to_ext("image/jpg"), "jpg");
        assert_eq!(mime_to_ext("image/png"), "png");
        assert_eq!(mime_to_ext("image/gif"), "gif");
        assert_eq!(mime_to_ext("image/webp"), "webp");
        assert_eq!(mime_to_ext("image/svg+xml"), "svg");
        assert_eq!(mime_to_ext("image/avif"), "avif");
        assert_eq!(mime_to_ext("audio/mpeg"), "mp3");
        assert_eq!(mime_to_ext("audio/mp3"), "mp3");
        assert_eq!(mime_to_ext("audio/ogg"), "ogg");
        assert_eq!(mime_to_ext("audio/flac"), "flac");
        assert_eq!(mime_to_ext("audio/wav"), "wav");
        assert_eq!(mime_to_ext("audio/x-wav"), "wav");
        assert_eq!(mime_to_ext("audio/aac"), "aac");
        assert_eq!(mime_to_ext("audio/mp4"), "m4a");
        assert_eq!(mime_to_ext("video/mp4"), "mp4");
        assert_eq!(mime_to_ext("video/webm"), "webm");
        assert_eq!(mime_to_ext("video/ogg"), "ogv");
    }

    #[test]
    fn test_mime_to_ext_unknown_falls_back_to_bin() {
        assert_eq!(mime_to_ext("application/octet-stream"), "bin");
        assert_eq!(mime_to_ext("text/html"), "bin");
        assert_eq!(mime_to_ext(""), "bin");
    }

    #[test]
    fn test_mime_to_ext_strips_params() {
        // Content-Type headers often carry charset or boundary params
        assert_eq!(mime_to_ext("image/jpeg; charset=utf-8"), "jpg");
        assert_eq!(mime_to_ext("image/png;q=0.9"), "png");
    }

    // --- ext_from_url ---

    #[test]
    fn test_ext_from_url_simple() {
        assert_eq!(ext_from_url("https://example.com/photo.jpg"), Some("jpg"));
        assert_eq!(ext_from_url("https://example.com/audio.mp3"), Some("mp3"));
    }

    #[test]
    fn test_ext_from_url_with_query_string() {
        // Extension should come from the path, not the query string
        assert_eq!(
            ext_from_url("https://cdn.example.com/image.png?v=123&size=large"),
            Some("png")
        );
    }

    #[test]
    fn test_ext_from_url_no_extension() {
        assert_eq!(ext_from_url("https://example.com/resource"), None);
        assert_eq!(ext_from_url("https://example.com/"), None);
    }

    #[test]
    fn test_ext_from_url_extension_too_long() {
        // Extensions longer than 5 chars are rejected (not real extensions)
        assert_eq!(ext_from_url("https://example.com/file.toolongext"), None);
    }

    #[test]
    fn test_ext_from_url_empty_extension() {
        assert_eq!(ext_from_url("https://example.com/file."), None);
    }

    #[test]
    fn test_ext_from_url_rejects_url_encoded_chars() {
        // %20 is a space, %2f is a slash — neither should ever end up in the
        // filename we write to disk.
        assert_eq!(ext_from_url("https://example.com/f.j%20g"), None);
        assert_eq!(ext_from_url("https://example.com/f.j%2fg"), None);
    }

    #[test]
    fn test_ext_from_url_rejects_non_ascii_alphanumeric() {
        // Spaces, punctuation, non-ASCII — all rejected to keep filenames sane.
        assert_eq!(ext_from_url("https://example.com/f.j g"), None);
        assert_eq!(ext_from_url("https://example.com/f.j-g"), None);
        assert_eq!(ext_from_url("https://example.com/f.jpé"), None);
        assert_eq!(ext_from_url("https://example.com/f.j_g"), None);
    }

    #[test]
    fn test_ext_from_url_accepts_mixed_case_alphanumeric() {
        // Some CDNs use uppercase or digit-only extensions.
        assert_eq!(ext_from_url("https://example.com/f.JPG"), Some("JPG"));
        assert_eq!(ext_from_url("https://example.com/f.mp4"), Some("mp4"));
        assert_eq!(ext_from_url("https://example.com/f.m4a"), Some("m4a"));
    }

    // --- src_regex (single-pass rewrite driver) ---

    /// Simulate what `rewrite_inline_images` does after downloads: rewrite
    /// every `src="..."` / `src='...'` occurrence in one pass using the
    /// URL → local-path lookup map.
    fn rewrite(html: &str, mapping: &HashMap<String, String>) -> String {
        src_regex()
            .replace_all(html, |caps: &regex::Captures| {
                let (url, quote) = match (caps.get(1), caps.get(2)) {
                    (Some(m), _) => (m.as_str(), '"'),
                    (_, Some(m)) => (m.as_str(), '\''),
                    _ => return caps[0].to_string(),
                };
                match mapping.get(url) {
                    Some(local) => format!("src={}{}{}", quote, local, quote),
                    None => caps[0].to_string(),
                }
            })
            .into_owned()
    }

    #[test]
    fn test_src_regex_rewrites_double_quoted_src() {
        let mut m = HashMap::new();
        m.insert(
            "https://example.com/a.jpg".to_string(),
            "media/abc.jpg".to_string(),
        );
        let out = rewrite(r#"<img src="https://example.com/a.jpg">"#, &m);
        assert_eq!(out, r#"<img src="media/abc.jpg">"#);
    }

    #[test]
    fn test_src_regex_rewrites_single_quoted_src() {
        let mut m = HashMap::new();
        m.insert(
            "https://example.com/a.jpg".to_string(),
            "media/abc.jpg".to_string(),
        );
        let out = rewrite(r#"<img src='https://example.com/a.jpg'>"#, &m);
        assert_eq!(out, r#"<img src='media/abc.jpg'>"#);
    }

    #[test]
    fn test_src_regex_leaves_unmapped_srcs_untouched() {
        let m = HashMap::new();
        let input = r#"<img src="https://example.com/never-downloaded.jpg">"#;
        assert_eq!(rewrite(input, &m), input);
    }

    #[test]
    fn test_src_regex_handles_many_images_in_single_pass() {
        // Regression test for the O(html_len × images) blowup: build a
        // moderately large document and check the pass still substitutes
        // every occurrence correctly.
        let mut m = HashMap::new();
        m.insert("https://cdn/1.jpg".to_string(), "media/1.jpg".to_string());
        m.insert("https://cdn/2.jpg".to_string(), "media/2.jpg".to_string());

        // Interleave both URLs across many <img> tags, plus filler text.
        let mut html = String::with_capacity(4096);
        for i in 0..100 {
            html.push_str(&format!(
                r#"<p>filler {i} <img src="https://cdn/1.jpg" /> …</p>"#
            ));
            html.push_str(r#"<p><img src="https://cdn/2.jpg" /></p>"#);
        }
        let out = rewrite(&html, &m);
        // Every original URL must be replaced.
        assert!(!out.contains("https://cdn/1.jpg"));
        assert!(!out.contains("https://cdn/2.jpg"));
        assert_eq!(out.matches(r#"src="media/1.jpg""#).count(), 100);
        assert_eq!(out.matches(r#"src="media/2.jpg""#).count(), 100);
    }

    #[test]
    fn test_src_regex_preserves_html_around_rewrite() {
        // Verify the rest of the tag (attributes, self-close) is preserved
        // through the single-pass rewrite.
        let mut m = HashMap::new();
        m.insert(
            "https://example.com/a.jpg".to_string(),
            "media/abc.jpg".to_string(),
        );
        let input =
            r#"<img alt="cat" src="https://example.com/a.jpg" width="300" loading="lazy" />"#;
        let out = rewrite(input, &m);
        assert_eq!(
            out,
            r#"<img alt="cat" src="media/abc.jpg" width="300" loading="lazy" />"#
        );
    }
}
