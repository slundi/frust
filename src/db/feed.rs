use crate::{utils::{encode_id, decode_id, sha256}};
use super::{Connection, get_datetime_utc, Feed, Account};
use actix_web::{error, Error};
use rusqlite::{params, Error::QueryReturnedNoRows};

const SQL_CREATE_FEED: &str ="INSERT INTO feed (url, name, account_id) VALUES ($1, $2, $3) ON CONFLICT (url) DO NOTHING RETURNING id, name";
const SQL_SUBSCRIBE: &str = "INSERT INTO subscription (account_id, feed_id, folder_id, xpath) VALUES(:account, :feed, :folder) ON CONFLICT DO UPDATE SET xpath = :xpath, folder_id = :folder";
const SQL_EDIT_FEED: &str = "UPDATE subscription SET url = $1, name =$2 WHERE feed_id = $3 AND account_id = $4";
const SQL_GET_FEEDS: &str ="SELECT s.id AS subscription_id, folder_id, d.name as folder,
    feed_id, CASE WHEN s.name IS NULL THEN f.title ELSE s.name END as name, xpath, f.page_link, f.feed_link, description, language, added, updated,
    sum(saved) AS read, COUNT(*) AS total
    FROM subscription s
    INNER JOIN feed f    ON s.feed_id    = f.id
    LEFT  JOIN folder d  ON s.folder_id  = d.id
    LEFT  join article a ON f.id = s.feed_id
    LEFT  JOIN read r    ON a.id = r.article_id AND s.account_id = r.account_id
    WHERE s.account_id = $1 AND saved = FALSE
    GROUP BY 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12
    ORDER BY d.name, name";
const SQL_DELETE_USER_FEED: &str = "DELETE FROM subscription WHERE feed_id = $1 AND account_id = $2";
/// Remove feeds without subscriptions. Tou need to pass the feed ID you want to delete as SQL parameter
const SQL_REMOVE_UNUSED_FEEDS: &str = "DELETE FROM feed WHERE id = $1 AND id NOT IN (SELECT feed_id FROM subscription WHERE feed_id = $1)";

/// Create a feed and returns its HashID
pub async fn create_feed(conn: &Connection, account_hid: String, url: String, name: String) -> Result<String, Error> {
    let mut stmt = conn.prepare(SQL_CREATE_FEED).expect("Wrong create feed SQL");
    stmt.query_row(params![&url, &name, decode_id(account_hid)], |row| {
        Ok(encode_id(row.get(0)?))
    }).map_err({
        log::error!("{}: {}", crate::messages::ERROR_CREATE_FEED, name);
        error::ErrorInternalServerError
    })
}

pub async fn subscribe(conn: &Connection, account_hid: String, feed_hid: String, folder_hid: String, xpath: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_SUBSCRIBE).expect("Wrong subscribe SQL");
    let result = stmt.execute(params![decode_id(account_hid), decode_id(feed_hid), decode_id(folder_hid), &xpath]);
    if let Ok(count) = result {
        if count == 1 {return Ok(());}
    }
    log::error!("{}", crate::messages::ERROR_SUBSCRIBE_FEED);
    Err(error::ErrorInternalServerError("Cannot subscribe to feed"))
}

pub async fn edit_feed(conn: &Connection, account_hid: String, feed_hid: String, url: String, name: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_EDIT_FEED).expect("Wrong edit feed SQL");
    let id = decode_id(feed_hid);
    stmt.execute(params![&url, &name, id, decode_id(account_hid)]).map(|_| ())
    .map_err(|_e|{
        log::error!("{}: {}", crate::messages::ERROR_EDIT_FEED, id);
        error::ErrorInternalServerError("Cannot edit feed")
    })
}

/// Delete a feed from the feed and account hash IDs (double check)
pub async fn unsubscribe_feed(conn: &Connection, account_hid: String, feed_hid: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_DELETE_USER_FEED).expect("Wrong unsubscribe feed SQL");
    let id = decode_id(feed_hid);
    stmt.execute([id, decode_id(account_hid)]).map(|_| ()).map_err(|_e|{
        log::error!("{}: {}", crate::messages::ERROR_UNSUBSCRIBE_FEED, id);
        error::ErrorInternalServerError("Cannot unsubscribe feed")
    })
}

/// After unsubscribing a feed, this func
pub async fn clear_unused_feed(conn: &Connection, feed_hid: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_REMOVE_UNUSED_FEEDS).expect("Wrong unsubscribe feed SQL");
    let id = decode_id(feed_hid);
    stmt.execute([id]).map(|_| ()).map_err(|_e|{
        log::error!("{}: {}", crate::messages::ERROR_DELETE_FEED, id);
        error::ErrorInternalServerError("Cannot delete feed")
    })
}

/// Get user's feeds
pub async fn get_feeds(conn: &Connection, account_hid: String, feed_hid: Option<String>) -> Result<Vec<Feed>, Error> {
    let mut stmt = conn.prepare_cached(SQL_GET_FEEDS).expect("Wrong delete token SQL");
    let result = stmt.query_map([decode_id(account_hid)], |r| {
        Ok(Feed {
            hash_id: encode_id(r.get(0).unwrap()),
            name: r.get(5).unwrap(),
            page_url: r.get(6).unwrap(),
            feed_url: r.get(7).unwrap(),
            updated: get_datetime_utc(r.get(3).unwrap()),
            icon_filename: sha256(r.get(2).unwrap()),
            unread_count: 0,
        })
    });
    if let Err(e) = result {
        log::error!("{}: {}", crate::messages::ERROR_LIST_FEEDS, e);
        return Err(error::ErrorInternalServerError("CANNOT_LIST_FEEDS"))
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
    let mut stmt = conn.prepare_cached(SQL_GET_FEEDS).expect("Wrong delete token SQL");
    let result = stmt.query([decode_id(account.hash_id)]);
    let mut out = String::with_capacity(2097152); // allocate 2 MB
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"1.0\">\n<head>\n\t<title>");
    out.push_str(&account.username);
    out.push_str("subscriptions in Frust</title>\n</head>\n<body>\n");
    return match result {
        Ok(mut rows) => {
            let mut current_folder = String::with_capacity(64);
            let mut first = true;
            while let Ok(Some(row)) = rows.next() {
                //handle folders
                let folder: String = row.get(2).expect("Cannot get folder name");
                if folder != current_folder {
                    if first { // if not the first element
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
        },
        Err(e) => {
            if e == QueryReturnedNoRows {
                out.push_str("\n</body>\n</opml>");
                return Ok(out);
            }
            log::error!("{}: {}", crate::messages::ERROR_LIST_FEEDS, e);
            Err(error::ErrorInternalServerError("CANNOT_LIST_FEEDS"))
        },
    }
}
