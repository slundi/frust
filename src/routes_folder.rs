use actix_web::{post, get, patch, delete, web, HttpResponse, HttpRequest};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct FolderForm {
    name: String,
    account_id: i32,
}

#[get("/folders/")]
pub(crate) async fn route_list_folers(path: web::Path<(String,)>)  ->  HttpResponse {
    HttpResponse::Ok().body("LIST FOLDER")
}

#[post("/folders/")]
pub(crate) async fn route_create_folder(path: web::Path<(String,)>, form: web::Form<FolderForm>, pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    let result = crate::db::create_folder(&pool, form.account_id, form.name.clone()).await;
    if result.is_err() {
        return HttpResponse::BadRequest().json("Cannot create folder");
    }
    HttpResponse::Ok().json(result.unwrap())
}

#[patch("/folders/{folder_hid}/")]
pub(crate) async fn route_edit_folder(path: web::Path<(String, String)>) ->  HttpResponse {
    HttpResponse::Ok().body("EDIT FOLDER")
}

#[delete("/folders/{folder_hid}/")]
pub(crate) async fn route_delete_folder(path: web::Path<(String, String)>) ->  HttpResponse {
    HttpResponse::Ok().body("DELETE FOLDER")
}
