use chrono::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Token {
    pub created: DateTime<Utc>,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Account {
    pub hash_id: String,
    pub username: String,
    pub encrypted_password: String,
    pub config: String,
    pub created: DateTime<Utc>,
    pub token: String,
    pub token_created: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Folder {
    pub hash_id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Filter {
    pub hash_id: String,
    pub account_id: i32,
    /// Subscription related to this filter, if None, the filter applies for all subscribed feeds.
    pub subscription_id: Option<i32>,
    /// Text to find or regex to match
    pub find: String,
    /// If the find string is a regex
    pub is_regex: bool,
    /// If we search in the title
    pub in_title: bool,
    /// If we search in the content
    pub in_content: bool,
    /// If we only want to include results that match this filter (or other include filters). If false, results from
    /// this filter are excluded.
    pub includes: bool,
}

/// Essential feed information to update data and display it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Feed {
    /// Name of the feed. Value is retrieved from the ATOM/RSS or from the user if he renames it.
    pub name: String,
    pub url: String,
    pub updated: DateTime<Utc>,
    /// Feed icon will be in <ASSETS_PATH>/f/<hash of the url>.png (convert it if needed)
    pub icon_filename: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountFeed {
    pub hash_id: String,
    /// If the link is accessible to anonymous user. If you follow a private tracker, you may need to set it to `false` to make it private.
    pub public: bool,
    /// When the user added this feed
    pub added: DateTime<Utc>,
    /// HashID and name of the folder
    pub folder: (String, String),
    pub name: String,
    pub feed: Feed,
}
