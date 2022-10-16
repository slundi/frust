use actix_web::{post, get, patch, delete, web, HttpResponse};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct FolderForm {
    name: String,
    account_id: i32,
    //for security purpose
    auth_token: String
}

#[get("/{account_uuid}/folders/")]
pub(crate) async fn route_list_folers(path: web::Path<(String,)>)  ->  HttpResponse {
    HttpResponse::Ok().body("LIST FOLDER")
}

#[post("/{account_uuid}/folders/")]
pub(crate) async fn route_create_folder(path: web::Path<(String,)>, form: web::Form<FolderForm>)  ->  HttpResponse {
    HttpResponse::Ok().body("CREATE FOLDER")
}

#[patch("/{account_uuid}/folders/{folder_uuid}/")]
pub(crate) async fn route_edit_folder(path: web::Path<(String, String)>) ->  HttpResponse {
    HttpResponse::Ok().body("EDIT FOLDER")
}

#[delete("/{account_uuid}/folders/{folder_uuid}/")]
pub(crate) async fn route_delete_folder(path: web::Path<(String, String)>) ->  HttpResponse {
    HttpResponse::Ok().body("DELETE FOLDER")
}
