use actix_web::{post, patch, delete, web, HttpResponse};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct LoginForm {
    username: String,
    clear_password: String
}

#[derive(Debug, Deserialize)]
pub(crate) struct RegisterForm {
    username: String,
    encrypted_password: String,
    config: String,
}

#[post("/login")]
pub(crate) async fn route_login(form: web::Form<LoginForm>)  ->  HttpResponse {
    HttpResponse::Ok().body("LOGIN")
}

#[post("/")]
/// Register a new user
pub(crate) async fn route_register(form: web::Form<RegisterForm>)  ->  HttpResponse {
    HttpResponse::Ok().body("CREATE ACCOUNT")
}

#[patch("/{account_uuid}/")]
pub(crate) async fn route_edit_account(path: web::Path<(String,)>) ->  HttpResponse {
    HttpResponse::Ok().body("EDIT ACCOUNT")
}

#[delete("/{account_uuid}/")]
pub(crate) async fn route_delete_account(path: web::Path<(String,)>) ->  HttpResponse {
    //HttpResponse::Ok().body(format!("User detail: {}", path.into_inner().0))
    HttpResponse::Ok().body("DELETE ACCOUNT")
}

#[delete("/{account_uuid}/tokens/{token_uuid}/")]
pub(crate) async fn route_delete_token(path: web::Path<(String,)>) ->  HttpResponse {
    HttpResponse::Ok().body("DELETE ACCOUNT TOKEN")
}
