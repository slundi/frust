pub(crate) const ERROR_SCHEMA_FILE: &str = "Cannot read schema file";
pub(crate) const ERROR_CANNOT_GET_CONNEXION: &str = "couldn't get db connection from pool";

pub(crate) const ERROR_GET_CONFIG: &str = "[CONFIG .env] read";

//account related
pub(crate) const ERROR_USERNAME_EXISTS: &str = "[ACCOUNT] username already exists";
pub(crate) const ERROR_DELETE_ACCOUNT: &str = "[ACCOUNT] delete";
pub(crate) const ERROR_WRONG_TOKEN: &str = "[TOKEN] no matching token";
pub(crate) const ERROR_CREATE_TOKEN: &str = "[TOKEN] create";
pub(crate) const ERROR_RENEW_TOKEN: &str = "[TOKEN] renew";
pub(crate) const ERROR_DELETE_TOKEN: &str = "[TOKEN] delete";
pub(crate) const ERROR_CLEAN_TOKENS: &str = "[TOKEN] clean";
pub(crate) const ERROR_LIST_TOKENS: &str = "[TOKEN] list";

// folders
pub(crate) const ERROR_CREATE_FOLDER: &str = "[FOLDER] create";
pub(crate) const ERROR_EDIT_FOLDER: &str = "[FOLDER] edit";
pub(crate) const ERROR_DELETE_FOLDER: &str = "[FOLDER] delete";
pub(crate) const ERROR_LIST_FOLDERS: &str = "[FOLDER] list";

// feeds
pub(crate) const ERROR_CREATE_FEED: &str = "[FEED] create";
pub(crate) const ERROR_EDIT_FEED: &str = "[FEED] edit";
pub(crate) const ERROR_SUBSCRIBE_FEED: &str = "[FEED] subscribe";
pub(crate) const ERROR_UNSUBSCRIBE_FEED: &str = "[FEED] unsubscribe";
pub(crate) const ERROR_DELETE_FEED: &str = "[FEED] delete";
pub(crate) const ERROR_LIST_FEEDS: &str = "[FEED] list";
pub(crate) const ERROR_IMPORT_FEEDS: &str = "[FEED] import";

pub(crate) const ERROR_OPEN_URL: &str = "[URL] open";

// articles
pub(crate) const ERROR_CLEAN_ARTICLES: &str = "[ARTICLE] clean";
pub(crate) const ERROR_CLEAN_ARTICLES_ASSETS: &str = "[ARTICLE] clean assets";
