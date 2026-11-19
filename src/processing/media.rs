// download_asset and rewrite_inline_images are WIP — not yet wired into the main pipeline
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    hash::Hasher,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use regex::Regex;
use reqwest::{Response, header};
use scraper::{Html, Selector};
use tokio::io::AsyncWriteExt;
use twox_hash::XxHash3_64;

use super::safe_http::{SafeClient, parse_content_type};
use crate::error::FrustError;

/// Hard ceiling applied when `media_max_size` is 0 ("no limit"), so a hostile
/// server cannot fill the SD card with an endless body.
const MAX_ASSET_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

/// Content types that say nothing about the format; only then is the URL
/// extension consulted.
const GENERIC_CONTENT_TYPES: &[&str] = &["", "application/octet-stream", "binary/octet-stream"];

/// Media extensions accepted from the URL when the content type is generic.
/// Never `svg`, `html`, `js`…: assets are served from the same origin as the
/// feeds, so anything a browser would execute is a stored XSS.
const URL_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "avif", "mp3", "ogg", "oga", "opus", "flac", "wav", "aac",
    "m4a", "mp4", "m4v", "webm", "ogv",
];

/// Matches `src="..."` (group 1) or `src='...'` (group 2). The Rust `regex`
/// crate has no backreferences, so we spell both quote styles out as an
/// alternation instead of `src=(["'])([^"']+)\1`.
fn src_regex() -> &'static Regex {
    static SRC_RE: OnceLock<Regex> = OnceLock::new();
    SRC_RE.get_or_init(|| Regex::new(r#"src="([^"]+)"|src='([^']+)'"#).unwrap())
}

/// Map a MIME content-type string to a file extension, for the media types
/// we are willing to serve. SVG is deliberately absent (scriptable).
fn mime_to_ext(content_type: &str) -> Option<&'static str> {
    let ext = match parse_content_type(content_type).0.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
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
        _ => return None,
    };
    Some(ext)
}

/// Extension to store an asset under, or `None` to refuse it.
///
/// A known media content type wins. A generic one (`application/octet-stream`
/// or missing) falls back to the URL extension if it is a known media one.
/// Anything else (`image/svg+xml`, `text/html`, …) is refused.
fn asset_extension(url: &str, content_type: &str) -> Option<String> {
    if let Some(ext) = mime_to_ext(content_type) {
        return Some(ext.to_string());
    }
    let essence = parse_content_type(content_type).0;
    if !GENERIC_CONTENT_TYPES.contains(&essence.as_str()) {
        return None;
    }
    let ext = ext_from_url(url)?.to_ascii_lowercase();
    URL_EXTENSIONS.contains(&ext.as_str()).then_some(ext)
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

/// Unique hidden name for an in-progress download inside `media_dir`. It never
/// matches the `<16 hex>.<ext>` pattern, so `purge_orphaned_media` removes
/// leftovers from a crash.
fn temp_name(url: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut hasher = XxHash3_64::new();
    hasher.write(url.as_bytes());
    hasher.write(&nanos.to_le_bytes());
    hasher.write(&std::process::id().to_le_bytes());
    hasher.write(&COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    format!(".{:016x}.part", hasher.finish())
}

/// Stream the body to `path`, hashing on the fly. Fails past `limit` bytes.
/// Returns the XXH3 of the content (same value as `XxHash3_64::oneshot`).
async fn stream_to_file(
    response: &mut Response,
    path: &Path,
    limit: u64,
) -> Result<u64, FrustError> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut hasher = XxHash3_64::new();
    let mut written: u64 = 0;
    while let Some(chunk) = response.chunk().await? {
        written += chunk.len() as u64;
        if written > limit {
            return Err(FrustError::Media(format!("body exceeds {limit} bytes")));
        }
        hasher.write(&chunk);
        file.write_all(&chunk).await?;
    }
    // Durable before the rename, so a crash never leaves a truncated asset
    // under its final name.
    file.sync_all().await?;
    Ok(hasher.finish())
}

async fn try_download_asset(
    client: &SafeClient,
    url: &str,
    media_dir: &Path,
    max_size: u64,
) -> Result<PathBuf, FrustError> {
    let limit = if max_size == 0 {
        MAX_ASSET_BYTES
    } else {
        max_size.min(MAX_ASSET_BYTES)
    };
    // SafeClient refuses non-public targets (including via redirects and
    // DNS) and non-2xx responses, so error pages are never stored.
    let mut response = client
        .get(url, "image/*, audio/*, video/*, */*;q=0.1")
        .await?;

    if let Some(len) = response.content_length()
        && len > limit
    {
        return Err(FrustError::Media(format!(
            "declared {len} bytes > limit {limit} bytes"
        )));
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let ext = asset_extension(url, content_type)
        .ok_or_else(|| FrustError::Media(format!("refusing content type '{content_type}'")))?;

    let tmp_path = media_dir.join(temp_name(url));
    let hash = match stream_to_file(&mut response, &tmp_path, limit).await {
        Ok(hash) => hash,
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(e);
        }
    };

    // Always rename, even if the target exists: same hash means same content,
    // and it replaces any truncated file left by older non-atomic writes.
    let path = media_dir.join(format!("{hash:016x}.{ext}"));
    if let Err(e) = tokio::fs::rename(&tmp_path, &path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(e.into());
    }
    Ok(path)
}

/// Download a single asset, deduplicate by XXH3 hash, and write to `media_dir/<hash>.<ext>`.
/// Returns the local path on success, `None` if refused (unsafe target or type,
/// size limit, error status) or on error.
pub(crate) async fn download_asset(
    client: &SafeClient,
    url: &str,
    media_dir: &Path,
    max_size: u64,
) -> Option<PathBuf> {
    match try_download_asset(client, url, media_dir, max_size).await {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::warn!("Skipping asset {}: {}", url, e);
            None
        }
    }
}

/// Find all external `<img src="...">` in an HTML fragment, download them, and rewrite
/// their `src` to the local `media/<hash>.<ext>` path. Returns the rewritten HTML.
pub(crate) async fn rewrite_inline_images(
    client: &SafeClient,
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
        for (mime, ext) in [
            ("image/jpeg", "jpg"),
            ("image/jpg", "jpg"),
            ("image/png", "png"),
            ("image/gif", "gif"),
            ("image/webp", "webp"),
            ("image/avif", "avif"),
            ("audio/mpeg", "mp3"),
            ("audio/mp3", "mp3"),
            ("audio/ogg", "ogg"),
            ("audio/flac", "flac"),
            ("audio/wav", "wav"),
            ("audio/x-wav", "wav"),
            ("audio/aac", "aac"),
            ("audio/mp4", "m4a"),
            ("video/mp4", "mp4"),
            ("video/webm", "webm"),
            ("video/ogg", "ogv"),
        ] {
            assert_eq!(mime_to_ext(mime), Some(ext), "{mime}");
        }
    }

    #[test]
    fn test_mime_to_ext_unknown_is_none() {
        assert_eq!(mime_to_ext("application/octet-stream"), None);
        assert_eq!(mime_to_ext("text/html"), None);
        assert_eq!(mime_to_ext(""), None);
    }

    #[test]
    fn test_mime_to_ext_refuses_svg() {
        assert_eq!(mime_to_ext("image/svg+xml"), None);
    }

    #[test]
    fn test_mime_to_ext_strips_params_and_case() {
        // Content-Type headers often carry charset or boundary params
        assert_eq!(mime_to_ext("image/jpeg; charset=utf-8"), Some("jpg"));
        assert_eq!(mime_to_ext("image/png;q=0.9"), Some("png"));
        assert_eq!(mime_to_ext("Image/PNG"), Some("png"));
    }

    // --- asset_extension ---

    #[test]
    fn test_asset_extension_prefers_content_type() {
        // Served as PNG, so stored as .png whatever the URL claims.
        assert_eq!(
            asset_extension("https://x.org/a.svg", "image/png").as_deref(),
            Some("png")
        );
    }

    #[test]
    fn test_asset_extension_refuses_scriptable_content_types() {
        for content_type in [
            "image/svg+xml",
            "text/html",
            "application/xhtml+xml",
            "text/javascript",
            "application/xml",
        ] {
            assert_eq!(
                asset_extension("https://x.org/a.jpg", content_type),
                None,
                "{content_type} with a .jpg URL must be refused"
            );
        }
    }

    #[test]
    fn test_asset_extension_generic_type_uses_known_url_extension() {
        assert_eq!(
            asset_extension("https://x.org/ep.MP3?dl=1", "application/octet-stream").as_deref(),
            Some("mp3")
        );
        assert_eq!(
            asset_extension("https://x.org/a.jpeg", "").as_deref(),
            Some("jpeg")
        );
    }

    #[test]
    fn test_asset_extension_generic_type_refuses_unknown_url_extension() {
        for url in [
            "https://x.org/a.svg",
            "https://x.org/a.svgz",
            "https://x.org/a.html",
            "https://x.org/a.js",
            "https://x.org/a.exe",
            "https://x.org/noext",
        ] {
            assert_eq!(
                asset_extension(url, "application/octet-stream"),
                None,
                "{url}"
            );
        }
    }

    #[test]
    fn test_temp_name_is_hidden_and_unique() {
        let first = temp_name("https://x.org/a.png");
        let second = temp_name("https://x.org/a.png");
        assert_ne!(first, second);
        assert!(first.starts_with('.') && first.ends_with(".part"));
    }

    // --- download_asset against a local server ---

    use crate::processing::safe_http::test_server::{http_response, spawn_server, test_client};

    const PNG_BODY: &[u8] = b"\x89PNG\r\n\x1a\nnot-really-a-png";

    fn png(_: &str, _: u16) -> Vec<u8> {
        http_response("200 OK", &[("Content-Type", "image/png")], PNG_BODY)
    }

    fn dir_entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn test_download_writes_file_named_by_content_hash() {
        let port = spawn_server(png).await;
        let dir = tempfile::tempdir().unwrap();

        let path = download_asset(
            &test_client(),
            &format!("http://127.0.0.1:{port}/img"),
            dir.path(),
            0,
        )
        .await
        .unwrap();

        // Streaming hash must match the one-shot hash used everywhere else.
        let expected = format!("{:016x}.png", XxHash3_64::oneshot(PNG_BODY));
        assert_eq!(path, dir.path().join(&expected));
        assert_eq!(std::fs::read(&path).unwrap(), PNG_BODY);
        assert_eq!(dir_entries(dir.path()), vec![expected], "no temp file left");
    }

    #[tokio::test]
    async fn test_download_replaces_truncated_file_at_final_name() {
        let port = spawn_server(png).await;
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir
            .path()
            .join(format!("{:016x}.png", XxHash3_64::oneshot(PNG_BODY)));
        std::fs::write(&final_path, &PNG_BODY[..4]).unwrap();

        download_asset(
            &test_client(),
            &format!("http://127.0.0.1:{port}/img"),
            dir.path(),
            0,
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&final_path).unwrap(), PNG_BODY);
    }

    async fn assert_refused(respond: fn(&str, u16) -> Vec<u8>, max_size: u64) {
        let port = spawn_server(respond).await;
        let dir = tempfile::tempdir().unwrap();
        let result = download_asset(
            &test_client(),
            &format!("http://127.0.0.1:{port}/a.jpg"),
            dir.path(),
            max_size,
        )
        .await;
        assert!(result.is_none());
        assert!(
            dir_entries(dir.path()).is_empty(),
            "nothing must be written"
        );
    }

    #[tokio::test]
    async fn test_download_refuses_error_status() {
        assert_refused(
            |_, _| {
                http_response(
                    "404 Not Found",
                    &[("Content-Type", "text/html")],
                    b"<h1>404</h1>",
                )
            },
            0,
        )
        .await;
    }

    #[tokio::test]
    async fn test_download_refuses_svg() {
        assert_refused(
            |_, _| {
                http_response(
                    "200 OK",
                    &[("Content-Type", "image/svg+xml")],
                    b"<svg onload=\"alert(1)\"/>",
                )
            },
            0,
        )
        .await;
    }

    #[tokio::test]
    async fn test_download_refuses_html_behind_image_url() {
        assert_refused(
            |_, _| {
                http_response(
                    "200 OK",
                    &[("Content-Type", "text/html")],
                    b"<script>x</script>",
                )
            },
            0,
        )
        .await;
    }

    #[tokio::test]
    async fn test_download_refuses_declared_size_over_limit() {
        assert_refused(png, 4).await;
    }

    #[tokio::test]
    async fn test_download_refuses_streamed_size_over_limit() {
        // No Content-Length: the limit must be enforced while streaming, and
        // the partial temp file removed.
        assert_refused(
            |_, _| {
                let mut out =
                    b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nConnection: close\r\n\r\n"
                        .to_vec();
                out.extend_from_slice(&[0u8; 4096]);
                out
            },
            1024,
        )
        .await;
    }

    #[tokio::test]
    async fn test_download_production_client_refuses_loopback() {
        let port = spawn_server(png).await;
        let dir = tempfile::tempdir().unwrap();
        let client = SafeClient::new(std::time::Duration::from_secs(5)).unwrap();
        let result = download_asset(
            &client,
            &format!("http://127.0.0.1:{port}/img"),
            dir.path(),
            0,
        )
        .await;
        assert!(result.is_none());
        assert!(dir_entries(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn test_rewrite_inline_images_uses_local_paths() {
        let port = spawn_server(png).await;
        let dir = tempfile::tempdir().unwrap();
        let html = format!(r#"<p><img src="http://127.0.0.1:{port}/a"></p>"#);

        let out = rewrite_inline_images(&test_client(), &html, dir.path(), 0).await;

        let expected = format!(
            r#"<p><img src="media/{:016x}.png"></p>"#,
            XxHash3_64::oneshot(PNG_BODY)
        );
        assert_eq!(out, expected);
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
