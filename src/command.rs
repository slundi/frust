use std::fs;
use std::io::BufWriter;

use quick_xml::Writer;

use crate::cli::{ExportOpts, ImportOpts};
use crate::error::FrustError;
use crate::opml::{ParsedGroup, build_yaml, parse_opml, write_opml};

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

/// `frust import OUTPUT OPML_FILE [OPML_FILE…]`
///
/// Parses one or more OPML files and writes a base YAML configuration to OUTPUT.
/// Groups with the same slug found across multiple files are merged; duplicate
/// feed URLs within a group are silently deduplicated.
pub fn import_opml(opts: &ImportOpts) -> Result<(), FrustError> {
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

    // Parse every OPML file and merge groups by slug.
    let mut all_groups: Vec<ParsedGroup> = Vec::new();
    for path in opml_files {
        tracing::debug!("Parsing {}", path);
        for g in parse_opml(path)? {
            match all_groups.iter_mut().find(|e| e.slug == g.slug) {
                Some(existing) => {
                    for feed in g.feeds {
                        if !existing.feeds.iter().any(|f| f.url == feed.url) {
                            existing.feeds.push(feed);
                        }
                    }
                }
                None => all_groups.push(g),
            }
        }
    }

    // Deterministic output: groups and feeds sorted alphabetically.
    all_groups.sort_by(|a, b| a.slug.cmp(&b.slug));
    for g in &mut all_groups {
        g.feeds.sort_by(|a, b| a.title.cmp(&b.title));
    }

    let yaml = build_yaml(&all_groups);

    if let Some(parent) = std::path::Path::new(output).parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, yaml)?;
    tracing::info!(
        "Config written to {} ({} group(s))",
        output,
        all_groups.len()
    );
    Ok(())
}

/// `frust export OUTPUT CONFIG_FILE`
///
/// Loads the YAML configuration from CONFIG_FILE and writes an OPML 2.0 file to OUTPUT.
/// Groups become container outlines; feeds become `type="rss"` leaf outlines.
pub fn export_opml(opts: &ExportOpts) -> Result<(), FrustError> {
    let output = opts
        .output()
        .ok_or_else(|| FrustError::Config("usage: frust export OUTPUT CONFIG_FILE".to_string()))?;
    let config_file = opts
        .config_file()
        .ok_or_else(|| FrustError::Config("usage: frust export OUTPUT CONFIG_FILE".to_string()))?;

    tracing::info!("Loading config from {}", config_file);
    let app = crate::config::load_config_file(config_file.to_string());

    if let Some(parent) = std::path::Path::new(output).parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let file = fs::File::create(output)?;
    let mut writer = Writer::new_with_indent(BufWriter::new(file), b' ', 2);
    write_opml(&mut writer, &app)?;

    tracing::info!("OPML written to {}", output);
    Ok(())
}
