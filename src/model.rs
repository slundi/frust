use chrono::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Token {
    pub hash_id: String,
    pub account_id: i32,
    pub created: DateTime<Utc>,
    pub name: String,
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
    pub account_id: i32,
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
