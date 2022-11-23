use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    /// Server `<IP or hostaname>:<port>`. Default is `127.0.0.1:8330`
    pub server_addr: String,
    /// Log level (available options are: INFO, WARN, ERROR, DEBUG, TRACE). Default is `INFO`.
    pub log_level: String,
    /// Where the SQLite database should be created/loaded. Default is `data/frust.sqlite3`
    pub sqlite_file: String,
    /// Delete old (and not save from any user) articles older than XX days. Default is 30 days.
    /// u16 max value is 65535 so it is more than 175 years
    pub article_keep_time: u16,
    /// Where do we store feed and article assets (images for now)? Default is `data/assets`.
    /// Some sub folders will be created:
    /// * `f` for feed icons (path will be:  `f/<feed UUID>.<ext>`)
    /// * `a` for article content such as images (path will be: `a/<article UUID>/<image name>.<ext>`)
    pub assets_path: String,
    /// Refresh all feed every XXX seconds. Default is 600 seconds (10 minutes)
    pub feed_refresh_time: u32,
    /// Secret key for hashing functions
    pub secret_key: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server_addr: "127.0.0.1:8330".to_owned(),
            log_level: "INFO".to_owned(),
            sqlite_file: "data/frust.sqlite3".to_owned(),
            article_keep_time: 30,
            assets_path: "data/assets".to_owned(),
            feed_refresh_time: 600,
            secret_key: "MY-T0P-S3CR3T-K3Y!".to_owned(),
        }
    }
}
