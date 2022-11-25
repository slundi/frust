use actix_web::{post, get, patch, delete, web, HttpResponse, HttpRequest};

use crate::messages::ERROR_CANNOT_GET_CONNEXION;

/// List folder with name
#[get("/")]
pub(crate) async fn list(pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::folder::get_folders(&conn, account.hash_id).await;
        if let Ok(folders) = result {
            return HttpResponse::Ok().json(folders);
        }
    }
    HttpResponse::BadRequest().json("Cannot get folders")
}

/// Create a folder for the user
#[post("/")]
pub(crate) async fn post(form: web::Form<String>, pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::folder::create_folder(&conn, account.hash_id, form.0).await;
        if result.is_ok() {
            return HttpResponse::Ok().finish();
        }
    }
    HttpResponse::BadRequest().json("Cannot create folder")
}

/// Rename a folder (for now)
#[patch("/{folder_hid}/")]
pub(crate) async fn patch(form: web::Form<String>, path: web::Path<String>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::folder::edit_folder(&conn, account.hash_id, path.into_inner(), form.0).await;
        if result.is_ok() {
            return HttpResponse::Ok().json(()); //TODO: return folder
        }
    }
    HttpResponse::BadRequest().json("Cannot rename folder")
}

#[delete("/{folder_hid}/")]
pub(crate) async fn delete(path: web::Path<String>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::folder::delete_folder(&conn, account.hash_id, path.into_inner()).await;
        if result.is_ok() {
            return HttpResponse::Ok().json(());
        }
    }
    HttpResponse::BadRequest().json("Cannot delete folder")
}
