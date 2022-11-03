use actix_web::{post, get, patch, delete, web, HttpResponse, HttpRequest};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct FolderForm {
    name: String,
    account_id: i32,
}

#[get("/folders/")]
pub(crate) async fn route_list_folers(pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    HttpResponse::Ok().body("LIST FOLDER")
}

#[post("/folders/")]
pub(crate) async fn route_create_folder(form: web::Form<FolderForm>, pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    if let Some(account) = crate::check_token(&pool, req).await {
        let result = crate::db::create_folder(&pool, form.account_id, form.name.clone()).await;
        if result.is_ok() {
            return HttpResponse::Ok().json(result.unwrap());
        }
    }
    HttpResponse::BadRequest().json("Cannot create folder")
}

/// Rename a folder (for now)
#[patch("/folders/{folder_hid}/")]
pub(crate) async fn route_edit_folder(path: web::Path<(String, String)>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    HttpResponse::Ok().body("EDIT FOLDER")
}

#[delete("/folders/{folder_hid}/")]
pub(crate) async fn route_delete_folder(path: web::Path<(String)>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::check_token(&pool, req).await {
        let result = crate::db::delete_folder(&pool, account.hash_id, path.into_inner()).await;
        if result.is_ok() {
            return HttpResponse::Ok().json(());
        }
    }
    HttpResponse::BadRequest().json("Cannot delete folder")
}
