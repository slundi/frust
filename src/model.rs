use chrono::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Token {
    pub id: i32,
    pub account_id: i32,
    pub created: DateTime::<Utc>,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: i32,
    pub slug: String,
    pub username: String,
    pub encrypted_password: String,
    pub config: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Folder {
    pub id: i32,
    pub slug: String,
    pub account_id: i32,
    pub name: String,
}
