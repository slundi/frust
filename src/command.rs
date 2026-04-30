use crate::cli::{ExportOpts, ImportOpts};
use crate::error::FrustError;

/// `frust import OUTPUT OPML_FILE [OPML_FILE…]`
///
/// Parses one or more OPML files and writes a base YAML configuration to OUTPUT.
pub fn import(opts: &ImportOpts) -> Result<(), FrustError> {
    let output = opts.output().ok_or_else(|| {
        FrustError::Config("usage: frust import OUTPUT OPML_FILE [OPML_FILE…]".to_string())
    })?;
    let opml_files = opts.opml_files();
    if opml_files.is_empty() {
        return Err(FrustError::Config(
            "usage: frust import OUTPUT OPML_FILE [OPML_FILE…]".to_string(),
        ));
    }
    tracing::info!("Importing {} OPML file(s) → {}", opml_files.len(), output);
    // TODO: parse each OPML file, collect feed entries, emit a YAML config skeleton
    todo!("OPML → YAML config import is not yet implemented")
}

/// `frust export OUTPUT CONFIG_FILE`
///
/// Loads the YAML configuration from CONFIG_FILE and writes an OPML file to OUTPUT.
pub fn export_opml(opts: &ExportOpts) -> Result<(), FrustError> {
    let output = opts
        .output()
        .ok_or_else(|| FrustError::Config("usage: frust export OUTPUT CONFIG_FILE".to_string()))?;
    let config_file = opts
        .config_file()
        .ok_or_else(|| FrustError::Config("usage: frust export OUTPUT CONFIG_FILE".to_string()))?;
    tracing::info!("Exporting OPML: {} → {}", config_file, output);
    // TODO: load YAML config, iterate groups/feeds, write OPML XML
    todo!("YAML config → OPML export is not yet implemented")
}

/// `frust export OUTPUT`
///
/// Reads all articles and media references from the redb database (no cleanup)
/// and writes them into a zip archive at OUTPUT.
pub fn archive(opts: &ExportOpts) -> Result<(), FrustError> {
    let output = opts
        .output()
        .ok_or_else(|| FrustError::Config("usage: frust export OUTPUT".to_string()))?;
    tracing::info!("Building zip archive → {}", output);
    // TODO: open redb, iterate all articles + media, write zip
    todo!("Zip archive export is not yet implemented")
}
