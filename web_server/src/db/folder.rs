use std::collections::HashMap;

use crate::{utils::{encode_id, decode_id}};
use super::{Connection, Folder};
use actix_web::{error, Error};
use rusqlite::params;

pub const SQL_CREATE_FOLDER: &str ="INSERT INTO folder (name, account_id) VALUES ($1, $2) RETURNING id";
const SQL_EDIT_FOLDER: &str = "UPDATE folder SET name = $1 WHERE id = $2 AND account_id = $3";
const SQL_GET_MY_FOLDER: &str ="SELECT id, name FROM folder WHERE account_id = $1 ORDER BY name";
const SQL_DELETE_FOLDER: &str = "DELETE FROM folder WHERE id = $1 AND account_id = $2";

/// Create a folder and returns its HashID
pub async fn create_folder(conn: &Connection, account_hid: String, name: String) -> Result<String, Error> {
    let mut stmt = conn.prepare(SQL_CREATE_FOLDER).expect("Wrong create folder SQL");
    stmt.query_row(params![&name, decode_id(account_hid)], |row| {
        Ok(encode_id(row.get(0)?))
    }).map_err({
        log::error!("{}: {}", crate::messages::ERROR_CREATE_FOLDER, name);
        error::ErrorInternalServerError
    })
}

pub async fn edit_folder(conn: &Connection, account_hid: String, folder_hid: String, name: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_EDIT_FOLDER).expect("Wrong edit folder SQL");
    let id = decode_id(folder_hid);
    stmt.execute(params![&name, id, decode_id(account_hid)]).map(|_| ())
    .map_err(|_e|{
        log::error!("{}: {}", crate::messages::ERROR_EDIT_FOLDER, id);
        error::ErrorInternalServerError("Cannot edit folder")
    })
}

/// Delete a folder from the folder and account hash IDs (double check)
pub async fn delete_folder(conn: &Connection, account_hid: String, folder_hid: String) -> Result<(), Error> {
    let mut stmt = conn.prepare(SQL_DELETE_FOLDER).expect("Wrong delete folder SQL");
    let id = decode_id(folder_hid);
    stmt.execute((id, decode_id(account_hid))).map(|_| ()).map_err(|_e|{
        log::error!("{}: {}", crate::messages::ERROR_DELETE_FOLDER, id);
        error::ErrorInternalServerError("Cannot delete folder")
    })
}

/// Get user's folders
pub async fn get_folders(conn: &Connection, account_hid: String) -> Result<Vec<Folder>, Error> {
    let mut stmt = conn.prepare_cached(SQL_GET_MY_FOLDER).expect("Wrong get folder SQL");
    let result = stmt.query_map([decode_id(account_hid)], |r| {
        Ok(Folder {
            hash_id: encode_id(r.get(0).unwrap()),
            name: r.get(1).unwrap(),
        })
    });
    if let Err(e) = result {
        log::error!("{}: {}", crate::messages::ERROR_LIST_FOLDERS, e);
        return Err(error::ErrorInternalServerError("CANNOT_LIST_FOLDERS"))
    }
    let rows = result.unwrap();
    let mut results: Vec<Folder> = Vec::new();
    for t in rows {
        results.push(t.unwrap());
    }
    Ok(results)
}

/// Get user's folders
pub async fn get_raw_folders(conn: &Connection, account_hid: String) -> Result<HashMap<String, i32>, Error> {
    let mut stmt = conn.prepare_cached(SQL_GET_MY_FOLDER).expect("Wrong get folder SQL");
    let result = stmt.query([decode_id(account_hid)]);
    match result {
        Ok(mut rows) => {
            let mut folders: HashMap<String, i32> = HashMap::new();
            while let Ok(Some(value)) = rows.next() {
                folders.insert(value.get(1).unwrap(), value.get(0).unwrap());
            }
            Ok(folders)
        },
        Err(e) => {
            log::error!("{}: {}", crate::messages::ERROR_LIST_FOLDERS, e);
            Err(error::ErrorInternalServerError("CANNOT_LIST_FOLDERS"))
        }
    }
}
