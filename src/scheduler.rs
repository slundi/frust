use actix::prelude::*;

use crate::{messages::*, CONFIG};

const SQL_CLEAN_TOKENS: &str = "DELETE FROM token WHERE DATETIME(created, '+' || $1 || ' days') < DATETIME('now')";

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
        //TODO: clean old articles
    }

    fn clean_tokens(&self, _ctx: &mut Context<Self>) {
        let conn = self.pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let config = &*CONFIG.read().expect("Cannot read config");
        let mut stmt = conn.prepare(SQL_CLEAN_TOKENS).expect("Wrong clean tokens SQL");
        let result = stmt.execute([config.token_duration.to_string()]);
        if let Err(e) = result {
            log::error!("{}: {}", crate::messages::ERROR_DELETE_TOKEN, e);
        }
    }
}
