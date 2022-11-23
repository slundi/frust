use crate::{model::Folder, utils::encode_id};
use super::Connection;
use actix_web::{error, Error};

const SQL_CREATE_FOLDER: &str ="INSERT INTO folder (name, account_id) VALUES ($1, $2) RETURNING id";
const SQL_EDIT_FOLDER: &str = "UPDATE folder SET name = $1 WHERE id = $2 AND account_id = $3";
const SQL_GET_MY_FOLDER: &str ="SELECT id, name FROM folder WHERE account_id = $1 ORDER BY name";
const SQL_DELETE_FOLDER: &str = "DELETE FROM folder WHERE id = $1 AND account_id = $2";

pub async fn create_folder(conn: &Connection, account_id: i32, name: String) -> Result<Folder, Error> {
    let mut stmt = conn
        .prepare(SQL_CREATE_FOLDER)
        .expect("Wrong create folder SQL");
    if stmt.execute((account_id, &name)).is_err() {
        log::error!("{}: {}", crate::messages::ERROR_CREATE_FOLDER, name);
    }
    stmt.query_row([], |row| {
        Ok(Folder {
            hash_id: encode_id(row.get(0)?),
            account_id: row.get(1)?,
            name: row.get(2)?,
        })
    })
    .map_err(error::ErrorInternalServerError)
}

pub async fn edit_folder(conn: &Connection, account_hid: String, folder_hid: String, name: String) -> Result<(), Error> {
    let mut stmt = conn
        .prepare(SQL_EDIT_FOLDER)
        .expect("Wrong edit folder SQL");
    let id = crate::utils::decode_id(folder_hid);
    if stmt.execute((&name, id, crate::utils::decode_id(account_hid))).is_err() {
        log::error!("{}: {}", crate::messages::ERROR_EDIT_FOLDER, id);
        return Err(error::ErrorInternalServerError("Cannot edit folder"));
    }
    Ok(())
}

/// Delete a folder from the folder and account hash IDs (double check)
pub async fn delete_folder(conn: &Connection, account_hid: String, folder_hid: String) -> Result<(), Error> {
    let mut stmt = conn
        .prepare(SQL_DELETE_FOLDER)
        .expect("Wrong delete folder SQL");
    let id = crate::utils::decode_id(folder_hid);
    if stmt.execute((id, crate::utils::decode_id(account_hid))).is_err() {
        log::error!("{}: {}", crate::messages::ERROR_DELETE_FOLDER, id);
        return Err(error::ErrorInternalServerError("Cannot delete folder"));
    }
    Ok(())
}
