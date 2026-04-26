// download_asset and rewrite_inline_images are WIP — not yet wired into the main pipeline
#![allow(dead_code)]

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use reqwest::{Client, header};
use scraper::{Html, Selector};
use twox_hash::XxHash3_64;

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
fn ext_from_url(url: &str) -> Option<&str> {
    let path = url.split('?').next()?;
    let filename = path.rsplit('/').next()?;
    if !filename.contains('.') {
        return None;
    }
    let ext = filename.rsplit('.').next()?;
    if ext.is_empty() || ext.len() > 5 {
        None
    } else {
        Some(ext)
    }
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

    let mut result = html.to_string();
    for src in srcs {
        if let Some(path) = download_asset(client, &src, media_dir, max_size).await {
            let filename = path.file_name().unwrap().to_string_lossy();
            let local = format!("media/{}", filename);
            result = result.replace(&format!("src=\"{}\"", src), &format!("src=\"{}\"", local));
            result = result.replace(&format!("src='{}'", src), &format!("src='{}'", local));
        }
    }
    result
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
}
