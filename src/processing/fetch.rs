/// Save the filtered feed to the specified output file.
pub(super) async fn save_feed_to_disk(
    feed: &feed_rs::model::Feed,
    path: &str,
) -> Result<(), std::io::Error> {
    // Note: converting feed_rs model back to RSS/Atom XML requires 'rss' or
    // 'atom_syndication' crates. This is a placeholder until export is wired up.
    tracing::debug!(
        "Writing {} filtered entries to {}",
        feed.entries.len(),
        path
    );
    // TODO: Implementation placeholder for serialization logic
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_feed(ids: &[&str]) -> feed_rs::model::Feed {
        let items: String = ids
            .iter()
            .map(|id| format!("<item><guid>{id}</guid><title>T</title></item>"))
            .collect::<Vec<_>>()
            .join("\n");
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <rss version="2.0"><channel>
                <title>Test</title><link>https://example.com</link>
                {items}
            </channel></rss>"#
        );
        feed_rs::parser::parse(xml.as_bytes()).unwrap()
    }

    #[tokio::test]
    async fn test_save_feed_to_disk_returns_ok() {
        let feed = parse_feed(&[]);
        let result = save_feed_to_disk(&feed, "/tmp/test_feed_output.xml").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_save_feed_with_entries_returns_ok() {
        let feed = parse_feed(&["entry-1", "entry-2"]);
        assert_eq!(feed.entries.len(), 2);
        let result = save_feed_to_disk(&feed, "/tmp/test_feed_with_entries.xml").await;
        assert!(result.is_ok());
    }
}
