use argh::FromArgs;

/// Frust — RSS/Atom feed aggregator.
///
/// - `frust [-c config.yaml]`
/// - `frust import OUTPUT OPML_FILE [OPML_FILE…]`
/// - `frust export OUTPUT [CONFIG_FILE]`
#[derive(FromArgs, Debug)]
pub struct CliOptions {
    /// print version information
    #[argh(switch, short = 'V')]
    pub version: bool,

    /// path to the YAML config file (default: config.yaml)
    #[argh(option, short = 'c')]
    pub config: Option<String>,

    #[argh(subcommand)]
    pub command: Option<Command>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
pub enum Command {
    Import(ImportOpts),
    Export(ExportOpts),
}

/// Generate a base YAML configuration from one or more OPML files.
///
/// Usage: `frust import OUTPUT OPML_FILE [OPML_FILE…]`
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "import")]
pub struct ImportOpts {
    #[argh(positional)]
    pub args: Vec<String>,
}

impl ImportOpts {
    pub fn output(&self) -> Option<&str> {
        self.args.first().map(String::as_str)
    }

    pub fn opml_files(&self) -> &[String] {
        self.args.get(1..).unwrap_or(&[])
    }
}

/// Export OPML from a config, or a zip archive of feeds+media from the DB.
///
/// - `OUTPUT` only        → zip archive of all feeds + media from redb
/// - `OUTPUT CONFIG_FILE` → OPML generated from the given YAML config
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "export")]
pub struct ExportOpts {
    #[argh(positional)]
    pub args: Vec<String>,
}

impl ExportOpts {
    pub fn output(&self) -> Option<&str> {
        self.args.first().map(String::as_str)
    }

    /// Present → generate OPML; absent → build zip archive.
    pub fn config_file(&self) -> Option<&str> {
        self.args.get(1).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<CliOptions, argh::EarlyExit> {
        CliOptions::from_args(&["frust"], args)
    }

    #[test]
    fn parses_config_short_flag() {
        let opts = parse(&["-c", "custom.yaml"]).unwrap();
        assert_eq!(opts.config.as_deref(), Some("custom.yaml"));
        assert!(!opts.version);
        assert!(opts.command.is_none());
    }

    #[test]
    fn parses_config_long_flag() {
        let opts = parse(&["--config", "custom.yaml"]).unwrap();
        assert_eq!(opts.config.as_deref(), Some("custom.yaml"));
    }

    #[test]
    fn parses_version_flag() {
        let opts = parse(&["-V"]).unwrap();
        assert!(opts.version);
    }

    #[test]
    fn no_args_leaves_defaults() {
        let opts = parse(&[]).unwrap();
        assert!(opts.config.is_none());
        assert!(!opts.version);
        assert!(opts.command.is_none());
    }

    #[test]
    fn parses_import_subcommand() {
        let opts = parse(&["import", "out.yaml", "a.opml", "b.opml"]).unwrap();
        match opts.command {
            Some(Command::Import(ref o)) => {
                assert_eq!(o.output(), Some("out.yaml"));
                assert_eq!(
                    o.opml_files(),
                    &["a.opml".to_string(), "b.opml".to_string()]
                );
            }
            _ => panic!("expected Import subcommand"),
        }
    }

    #[test]
    fn parses_export_subcommand_zip_form() {
        let opts = parse(&["export", "archive.zip"]).unwrap();
        match opts.command {
            Some(Command::Export(ref o)) => {
                assert_eq!(o.output(), Some("archive.zip"));
                assert!(o.config_file().is_none());
            }
            _ => panic!("expected Export subcommand"),
        }
    }

    #[test]
    fn parses_export_subcommand_opml_form() {
        let opts = parse(&["export", "out.opml", "config.yaml"]).unwrap();
        match opts.command {
            Some(Command::Export(ref o)) => {
                assert_eq!(o.output(), Some("out.opml"));
                assert_eq!(o.config_file(), Some("config.yaml"));
            }
            _ => panic!("expected Export subcommand"),
        }
    }

    #[test]
    fn config_flag_before_subcommand_is_captured() {
        let opts = parse(&["-c", "alt.yaml", "export", "archive.zip"]).unwrap();
        assert_eq!(opts.config.as_deref(), Some("alt.yaml"));
        match opts.command {
            Some(Command::Export(ref o)) => assert_eq!(o.output(), Some("archive.zip")),
            _ => panic!("expected Export subcommand"),
        }
    }
}
