pub(crate) mod atom;
pub(crate) mod epub;
pub(crate) mod json;
pub(crate) mod markdown;
pub(crate) mod rss;

pub(crate) use atom::AtomExporter;
pub(crate) use epub::EpubExporter;
pub(crate) use json::JsonExporter;
pub(crate) use markdown::MarkdownExporter;
pub(crate) use rss::RssExporter;

use std::path::Path;

use crate::{error::FrustError, model::Article};

pub(crate) trait Exporter {
    /// `articles`: items to export.
    /// `title`:    channel/document title (group or feed name).
    /// `link`:     canonical URL of the channel (base URL of the output site).
    /// `destination`: for Monolithic, path to the output file; for Individual/Daily, path to the output directory.
    fn generate(
        &self,
        articles: &[Article],
        title: &str,
        link: &str,
        destination: &Path,
    ) -> Result<(), FrustError>;
}
