use super::{get_datetime_utc, Account, Connection, Feed};
use crate::{
    model::FeedData,
    utils::{decode_id, encode_id, sha256},
};
use actix_web::{error, Error};
use rusqlite::{named_params, params, Error::QueryReturnedNoRows};
use std::collections::HashMap;

const SQL_CREATE_FEED: &str ="INSERT INTO feed (feed_link, title) VALUES ($1, $2) ON CONFLICT (feed_link) DO NOTHING RETURNING id, title";
const SQL_SUBSCRIBE: &str = "INSERT INTO subscription (account_id, feed_id, folder_id, selector, inject) VALUES(:account, :feed, :folder, :selector, :inject) ON CONFLICT DO UPDATE SET selector = :selector, folder_id = :folder, inject = :inject";
const SQL_EDIT_FEED: &str =
    "UPDATE subscription SET feed_link = $1, name =$2 WHERE feed_id = $3 AND account_id = $4";
const SQL_GET_FEED: &str = "SELECT s.id AS subscription_id, folder_id, d.name as folder,
        feed_id, CASE WHEN s.name IS NULL THEN f.title ELSE s.name END as name, selector, f.page_link, f.feed_link, description, language, added, updated, inject,
        sum(saved) AS read, COUNT(*) AS total
    FROM subscription s 
    INNER JOIN feed f    ON s.feed_id    = f.id
    LEFT  JOIN folder d  ON s.folder_id  = d.id
    LEFT  join article a ON f.id = s.feed_id
    LEFT  JOIN read r    ON a.id = r.article_id AND s.account_id = r.account_id
    WHERE s.account_id = $1 AND s.feed_id = $2 AND saved = FALSE
    GROUP BY 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13";
const SQL_GET_FEEDS: &str ="SELECT s.id AS subscription_id, folder_id, d.name as folder,
        feed_id, CASE WHEN s.name IS NULL THEN f.title ELSE s.name END as name, selector, f.page_link, f.feed_link, description, language, added, updated, inject,
        sum(saved) AS read, COUNT(*) AS total
    FROM subscription s
    INNER JOIN feed f    ON s.feed_id    = f.id
    LEFT  JOIN folder d  ON s.folder_id  = d.id
    LEFT  join article a ON f.id = s.feed_id
    LEFT  JOIN read r    ON a.id = r.article_id AND s.account_id = r.account_id
    WHERE s.account_id = $1 AND saved = FALSE
    GROUP BY 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13
    ORDER BY d.name, name";
const SQL_DELETE_USER_FEED: &str =
    "DELETE FROM subscription WHERE feed_id = $1 AND account_id = $2";
/// Remove feeds without subscriptions. Tou need to pass the feed ID you want to delete as SQL parameter
const SQL_REMOVE_UNUSED_FEEDS: &str = "DELETE FROM feed WHERE id = $1 AND id NOT IN (SELECT feed_id FROM subscription WHERE feed_id = $1)";

/// Create a feed and returns its HashID
pub async fn create_feed(
    conn: &mut Connection,
    account_hid: String,
    data: &FeedData,
) -> Result<String, Error> {
    //TODO: transaction: create feed if feed_link does not exists, insert subscription
    let mut feed_id: i32 = -1;
    if let Ok(tx) = conn.transaction() {
        let feed_result: rusqlite::Result<i32, rusqlite::Error> =
            tx.query_row(SQL_CREATE_FEED, params![data.url, data.name], |row| {
                row.get(0)
            });
        match feed_result {
            Ok(result) => {
                feed_id = result;
                let _ = tx.execute(
                    SQL_SUBSCRIBE,
                    named_params! {
                        "account": decode_id(account_hid),
                        "feed": feed_id,
                        "folder": decode_id(data.folder.clone()),
                        "selector": data.selector,
                        "inject": data.inject,
                    },
                )
                .map_err(|e| {
                    log::error!("{}: {}", crate::messages::ERROR_CREATE_FEED, e);
                });
            }
            Err(e) => {
                log::error!("{}: {}", crate::messages::ERROR_CREATE_FEED, e);
            }
        };
        if let Err(e) = tx.commit() {
            log::error!("{}: {}", crate::messages::ERROR_CREATE_FEED, e);
            return Err(error::ErrorInternalServerError("CANNOT_CREATE_FEED"));
        }
    }
    Ok(encode_id(feed_id))
}

pub async fn subscribe(
    conn: &Connection,
    account_hid: String,
    feed_hid: String,
    folder_hid: String,
    selector: String,
) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_SUBSCRIBE).expect("Wrong subscribe SQL");
    let result = stmt.execute(params![
        decode_id(account_hid),
        decode_id(feed_hid),
        decode_id(folder_hid),
        &selector
    ]);
    if let Ok(count) = result {
        if count == 1 {
            return Ok(());
        }
    }
    log::error!("{}", crate::messages::ERROR_SUBSCRIBE_FEED);
    Err(error::ErrorInternalServerError("Cannot subscribe to feed"))
}

pub async fn edit_feed(
    conn: &Connection,
    account_hid: String,
    feed_hid: String,
    url: String,
    name: String,
) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_EDIT_FEED).expect("Wrong edit feed SQL");
    let id = decode_id(feed_hid);
    stmt.execute(params![&url, &name, id, decode_id(account_hid)])
        .map(|_| ())
        .map_err(|_e| {
            log::error!("{}: {}", crate::messages::ERROR_EDIT_FEED, id);
            error::ErrorInternalServerError("Cannot edit feed")
        })
}

/// Delete a feed from the feed and account hash IDs (double check)
pub async fn unsubscribe_feed(
    conn: &Connection,
    account_hid: String,
    feed_hid: String,
) -> Result<(), Error> {
    let mut stmt = conn
        .prepare(SQL_DELETE_USER_FEED)
        .expect("Wrong unsubscribe feed SQL");
    let id = decode_id(feed_hid);
    stmt.execute([id, decode_id(account_hid)])
        .map(|_| ())
        .map_err(|_e| {
            log::error!("{}: {}", crate::messages::ERROR_UNSUBSCRIBE_FEED, id);
            error::ErrorInternalServerError("Cannot unsubscribe feed")
        })
}

/// After unsubscribing a feed, this func
pub async fn clear_unused_feed(conn: &Connection, feed_hid: String) -> Result<(), Error> {
    let mut stmt = conn
        .prepare(SQL_REMOVE_UNUSED_FEEDS)
        .expect("Wrong unsubscribe feed SQL");
    let id = decode_id(feed_hid);
    stmt.execute([id]).map(|_| ()).map_err(|_e| {
        log::error!("{}: {}", crate::messages::ERROR_DELETE_FEED, id);
        error::ErrorInternalServerError("Cannot delete feed")
    })
}

pub async fn get_feed(
    conn: &Connection,
    account_hid: String,
    feed_hid: String,
) -> Result<Feed, Error> {
    let result = conn.query_row(
        SQL_GET_FEED,
        (decode_id(account_hid), decode_id(feed_hid)),
        |r| {
            Ok(Feed {
                hash_id: encode_id(r.get(0).unwrap()),
                name: r.get(5).unwrap(),
                page_url: r.get(6).unwrap(),
                feed_url: r.get(7).unwrap(),
                updated: get_datetime_utc(r.get(3).unwrap()),
                icon_filename: sha256(r.get(2).unwrap()),
                unread_count: 0,
                inject: r.get(12).unwrap(),
            })
        },
    );
    match result {
        Ok(feed) => Ok(feed),
        Err(e) => {
            log::error!("{}: {}", crate::messages::ERROR_LIST_FEEDS, e);
            Err(error::ErrorInternalServerError("CANNOT_LIST_FEEDS"))
        },
    }
}

/// Get user's feeds
pub async fn get_feeds(conn: &Connection, account_hid: String) -> Result<Vec<Feed>, Error> {
    let mut stmt = conn
        .prepare_cached(SQL_GET_FEEDS)
        .expect("Wrong delete token SQL");
    let result = stmt.query_map([decode_id(account_hid)], |r| {
        Ok(Feed {
            hash_id: encode_id(r.get(0).unwrap()),
            name: r.get(5).unwrap(),
            page_url: r.get(6).unwrap(),
            feed_url: r.get(7).unwrap(),
            updated: get_datetime_utc(r.get(3).unwrap()),
            icon_filename: sha256(r.get(2).unwrap()),
            unread_count: 0,
            inject: r.get(12).unwrap(),
        })
    });
    if let Err(e) = result {
        log::error!("{}: {}", crate::messages::ERROR_LIST_FEEDS, e);
        return Err(error::ErrorInternalServerError("CANNOT_LIST_FEEDS"));
    }
    let rows = result.unwrap();
    let mut results: Vec<Feed> = Vec::new();
    for t in rows {
        results.push(t.unwrap());
    }
    Ok(results)
}

/// Get user's feed with folders and generate OPML string
pub async fn export(conn: &Connection, account: Account) -> Result<String, Error> {
    let mut stmt = conn
        .prepare_cached(SQL_GET_FEEDS)
        .expect("Wrong delete token SQL");
    let result = stmt.query([decode_id(account.hash_id)]);
    let mut out = String::with_capacity(2097152); // allocate 2 MB
    out.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"1.0\">\n<head>\n\t<title>",
    );
    out.push_str(&account.username);
    out.push_str("subscriptions in Frust</title>\n</head>\n<body>\n");
    match result {
        Ok(mut rows) => {
            let mut current_folder = String::with_capacity(64);
            let mut first = true;
            while let Ok(Some(row)) = rows.next() {
                //handle folders
                let folder: String = row.get(2).expect("Cannot get folder name");
                if folder != current_folder {
                    if first {
                        // if not the first element
                        out.push_str("\t</outline>");
                        first = false;
                        current_folder = folder;
                    }
                    out.push_str("\t<outline title=\"");
                    out.push_str(&current_folder);
                    out.push_str("\">\n");
                }
                //feed line
                let tmp: String = row.get(4).expect("Cannot get feed title");
                out.push_str("\t\t<outline type=\"rss\" title=\"");
                out.push_str(&tmp);
                out.push_str("\" xmlUrl=\"");
                let tmp: String = row.get(7).expect("Cannot get feed URL");
                out.push_str(&tmp);
                out.push_str("\" htmlUrl=\"");
                let tmp: String = row.get(6).expect("Cannot get page URL");
                out.push_str(&tmp);
                out.push_str("\">\n");
            }
            out.push_str("\t</outline>\n</body>\n</opml>");
            Ok(out)
        }
        Err(e) => {
            if e == QueryReturnedNoRows {
                out.push_str("\n</body>\n</opml>");
                return Ok(out);
            }
            log::error!("{}: {}", crate::messages::ERROR_LIST_FEEDS, e);
            Err(error::ErrorInternalServerError("CANNOT_LIST_FEEDS"))
        }
    }
}

pub fn import(
    conn: &mut Connection,
    account_hid: String,
    data: Vec<(String, String, String, String)>,
    mut folders: HashMap<String, i32>,
) -> Result<(), Error> {
    //let folders = super::folder::get_raw_folders(conn, account.hash_id).await;
    if let Ok(tx) = conn.transaction() {
        for v in data {
            folders.entry(v.0.clone()).or_insert_with(|| {
                let result: i32 = tx
                    .query_row(
                        super::folder::SQL_CREATE_FOLDER,
                        params![&v.0, decode_id(account_hid.clone())],
                        |row| row.get(0),
                    )
                    .unwrap();
                result
            });
            if let Err(e) = tx.execute(SQL_CREATE_FEED, params![account_hid, v.1, v.2, v.3]) {
                log::error!("{}: {}", crate::messages::ERROR_IMPORT_FEEDS, e);
                if let Err(e) = tx.rollback() {
                    log::error!("{}: {}", crate::messages::ERROR_IMPORT_FEEDS, e);
                }
                return Err(error::ErrorInternalServerError("CANNOT_IMPORT_OPML"));
            }
        }
        return match tx.commit() {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("{}: {}", crate::messages::ERROR_IMPORT_FEEDS, e);
                Err(error::ErrorInternalServerError("CANNOT_IMPORT_OPML"))
            }
        };
    }
    Err(error::ErrorInternalServerError("CANNOT_IMPORT_OPML"))
}
