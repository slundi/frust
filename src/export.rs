use std::{error::Error, path::Path};

use crate::model::Article;

pub(crate) trait Exporter {
    /// metadata: contains the group name or feed name for titles/filenames
    fn generate(
        &self,
        articles: &[Article],
        title: &str,
        destination: &Path,
    ) -> Result<(), Box<dyn Error>>;
}

pub struct EpubExporter;
pub struct MarkdownExporter;
pub struct JsonExporter;
