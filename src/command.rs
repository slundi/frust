use std::fs;
use std::io::BufWriter;

use quick_xml::Writer;

use crate::cli::{ExportOpts, ImportOpts};
use crate::error::FrustError;
use crate::opml::{ParsedGroup, build_yaml, parse_opml, write_opml};

/// `frust export OUTPUT`
///
/// Loads the YAML config at `config_path` to locate the redb database and feed
/// structure, then writes a ZIP archive to OUTPUT containing:
/// - one Atom 1.0 file per group
/// - all media assets from `{app.output}/media/`
pub fn archive(opts: &ExportOpts, config_path: &str) -> Result<(), FrustError> {
    let output = opts
        .output()
        .ok_or_else(|| FrustError::Config("usage: frust export OUTPUT".to_string()))?;
    tracing::info!(
        "Building zip archive → {} (config: {})",
        output,
        config_path
    );
    crate::export::zip::build_zip_archive(output, config_path)
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

#[cfg(test)]
mod tests {
    use std::{io::Read, path::Path};

    use zip::ZipArchive;

    use super::*;

    fn unique_dir(prefix: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("/tmp/frust_cmd_{}_{}", prefix, nanos)
    }

    fn write_config_at(path: &str, output_dir: &str, group_slug: &str) {
        let yaml = format!(
            "output: {out}\n\
             groups:\n\
             - title: {slug}\n  slug: {slug}\n  output: {slug}.atom\n  \
             feeds:\n  - title: F\n    url: https://example.com/feed.xml\n",
            out = output_dir,
            slug = group_slug,
        );
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, yaml).unwrap();
    }

    fn zip_names(zip_path: &str) -> Vec<String> {
        let mut archive = ZipArchive::new(fs::File::open(zip_path).unwrap()).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect()
    }

    #[test]
    fn test_archive_uses_supplied_config_path() {
        // A non-default config path (not "config.yaml") must be honoured.
        let dir = unique_dir("cfgpath");
        fs::create_dir_all(&dir).unwrap();
        let cfg = format!("{}/custom-name.yaml", dir);
        write_config_at(&cfg, &dir, "customgroup");

        let out = format!("{}/archive.zip", dir);
        let opts = ExportOpts {
            args: vec![out.clone()],
        };
        archive(&opts, &cfg).unwrap();

        let names = zip_names(&out);
        assert!(
            names.iter().any(|n| n == "customgroup.atom"),
            "group from custom config must appear in zip, got {:?}",
            names
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_archive_config_path_content_used() {
        // Verify the config we supplied — not "config.yaml" — is actually read
        // by checking the atom <id> reflects the group's configured output name.
        let dir = unique_dir("cfgused");
        fs::create_dir_all(&dir).unwrap();
        let cfg = format!("{}/alt.yaml", dir);
        write_config_at(&cfg, &dir, "sentinelgroup");

        let out = format!("{}/archive.zip", dir);
        let opts = ExportOpts {
            args: vec![out.clone()],
        };
        archive(&opts, &cfg).unwrap();

        let mut archive_file = ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
        let mut entry = archive_file.by_name("sentinelgroup.atom").unwrap();
        let mut xml = String::new();
        entry.read_to_string(&mut xml).unwrap();
        assert!(
            xml.contains("<id>sentinelgroup.atom</id>"),
            "atom <id> must reflect the sentinel group's output: {}",
            xml
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_archive_missing_output_arg_errors() {
        let opts = ExportOpts { args: vec![] };
        let err = archive(&opts, "does-not-matter.yaml").unwrap_err();
        matches!(err, FrustError::Config(_));
    }
}
