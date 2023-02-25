use actix::prelude::*;

use crate::{messages::*, CONFIG};

const SQL_CLEAN_TOKENS: &str = "DELETE FROM token WHERE DATETIME(created, '+' || $1 || ' days') < DATETIME('now')";
/// Remove unsaved articles that are older than `Config.article_keep_time` (in days)
const SQL_CLEAN_ARTICLES: &str = "DELETE FROM article
    WHERE DATETIME(published, '+' || $1 || ' days') < DATETIME('now')
      AND id NOT IN (SELECT article_id FROM read WHERE saved = TRUE)
    RETURNING id";

pub(crate) struct Scheduler {
    pub(crate) pool: crate::db::Pool,
}

impl Actor for Scheduler {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Context<Self>) {
        log::info!("Scheduler started");
        self.clean(ctx);
        ctx.run_interval(
            std::time::Duration::from_secs(86400), //refresh every 24 hours
            move |this, ctx| this.clean(ctx),
        );
    }
}

impl Scheduler {
    fn clean(&self, ctx: &mut Context<Self>) {
        self.clean_tokens(ctx);
        self.clean_articles(ctx);
        self.refresh_feeds(ctx);
    }

    fn clean_tokens(&self, _ctx: &mut Context<Self>) {
        let conn = self.pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let config = &*CONFIG.read().expect("Cannot read config");
        let mut stmt = conn.prepare(SQL_CLEAN_TOKENS).expect("Wrong clean tokens SQL");
        let result = stmt.execute([config.token_duration.to_string()]);
        if let Err(e) = result {
            log::error!("{}: {}", crate::messages::ERROR_CLEAN_TOKENS, e);
        }
    }

    /// Clean old articles:
    /// - delete entries from DB if not saved for any user
    /// - delete article assets
    fn clean_articles(&self, _ctx: &mut Context<Self>) {
        let conn = self.pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let config = &*CONFIG.read().expect("Cannot read config");
        let mut stmt = conn.prepare(SQL_CLEAN_ARTICLES).expect("Wrong clean tokens SQL");
        let result = stmt.query([config.article_keep_time.to_string()]);
        match result {
            Ok(mut ids) => {
                while let Ok(Some(value)) = ids.next() {
                    let mut path = config.assets_path.clone();
                    path.push('/');
                    let id: i32 = value.get(0).unwrap();
                    path.push_str(&id.to_string());
                    if let Err(e) = std::fs::remove_dir_all(&path) {
                        log::error!("{}: {}", crate::messages::ERROR_CLEAN_ARTICLES_ASSETS, e);
                    }
                }
            }
            Err(e) => log::error!("{}: {}", crate::messages::ERROR_CLEAN_ARTICLES, e),
        }
    }

    fn refresh_feeds(&self, _ctx: &mut Context<Self>) {
        // TODO: get feed list from DB, query each RSS, add new articles, update status
        let _ = check_url("https://127.0.0.1/rss.xml".to_owned());
    }
}

/// Check the URL and return the URL, if it is redirected it returns a new URL.
fn check_url(url: String) -> Result<String, crate::modules::Error> {
    match ureq::get(&url).call() {
        Ok(response) => {
            if (300..308).contains(&response.status()) {
                if let Some(location) = response.header("Location") {
                    return Ok(String::from(location));
                } else {
                    return Err(crate::modules::Error::UnreachableUrl);
                }
            }
        },
        Err(_e) => return Err(crate::modules::Error::UnreachableUrl),
    }
    Ok(url)
}
