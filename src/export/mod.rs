pub(crate) mod atom;
pub(crate) mod epub;
pub(crate) mod json;
pub(crate) mod markdown;
pub(crate) mod rss;
pub(crate) mod zip;

pub(crate) use atom::AtomExporter;
pub(crate) use epub::EpubExporter;
pub(crate) use json::JsonExporter;
pub(crate) use markdown::MarkdownExporter;
pub(crate) use rss::RssExporter;

use std::{collections::HashMap, path::Path};

use crate::{
    error::FrustError,
    model::{Article, Enrichment},
};

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
