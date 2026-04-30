use gumdrop::Options;

/// `frust [--config config.yaml]`
/// `frust import OUTPUT OPML_FILE [OPML_FILE…]`
/// `frust export OUTPUT [CONFIG_FILE]`
#[derive(Debug, Options)]
pub struct CliOptions {
    #[options(help = "print help message")]
    pub help: bool,

    #[options(short = "V", help = "print version information")]
    pub version: bool,

    /// Path to the YAML config file used when no subcommand is given (default: config.yaml).
    #[options(
        short = "c",
        meta = "FILE",
        help = "path to the YAML config file (default: config.yaml)"
    )]
    pub config: Option<String>,

    #[options(command)]
    pub command: Option<Command>,
}

#[derive(Debug, Options)]
pub enum Command {
    #[options(help = "generate a base YAML configuration from one or more OPML files")]
    Import(ImportOpts),
    /// Without CONFIG_FILE → zip archive of all feeds + media from redb.
    /// With CONFIG_FILE    → OPML generated from the given YAML configuration.
    #[options(help = "export OPML from a config, or a zip archive of feeds+media from the DB")]
    Export(ExportOpts),
}

/// `frust import OUTPUT OPML_FILE [OPML_FILE…]`
#[derive(Debug, Options)]
pub struct ImportOpts {
    #[options(help = "print help message")]
    pub help: bool,

    #[options(free)]
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

/// `frust export OUTPUT [CONFIG_FILE]`
///
/// - `OUTPUT` only       → zip archive of all feeds + media from redb
/// - `OUTPUT CONFIG_FILE` → OPML generated from the given YAML config
#[derive(Debug, Options)]
pub struct ExportOpts {
    #[options(help = "print help message")]
    pub help: bool,

    #[options(free)]
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
