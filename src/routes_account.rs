use actix_web::{post, patch, delete, web, HttpResponse, HttpRequest};
use serde::Deserialize;
use std::future::IntoFuture;

use crate::decode_id;

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    username: String,
    clear_password: String
}

#[derive(Debug, Deserialize)]
pub struct RegisterForm {
    username: String,
    clear_password: String,
    /// To check against clear_password
    clear_password_2: String,
}

/// Check if the token in the Authorization HTTP header is OK and return the account object
pub(crate) async fn check_token(pool: &actix_web::web::Data<crate::db::Pool>, req: actix_web::HttpRequest) -> Option<crate::model::Account> {
    let value = req.headers().get(actix_web::http::header::AUTHORIZATION);
    if let Some(token_h) = value {
        let raw_token = token_h.to_str().unwrap_or("").to_lowercase();
        if !raw_token.starts_with("token ") {
            return None
        }
        let token_cleaned = raw_token.strip_prefix("token ");
        if let Some(token) = token_cleaned {
            log::info!("Token 6: {:?}", token);
            let conn = pool.get().expect("couldn't get db connection from pool");
            let result = std::future::IntoFuture::into_future(crate::db::get_user_from_token(&conn, token.to_owned())).await;
            if let Ok(account) = result {
                return Some(account);
            }
        }
    }
    None
}

/// Log the user in:
/// 1. Get the user from the nickname
/// 2. Verify bcrypt encoded password
/// 3. Create or update token if client is unknown
/// 
/// It returns the JSON formatted account
#[post("/login")]
pub(crate) async fn route_login(form: web::Form<LoginForm>, pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    log::debug!("Login");
    let conn = pool.get().expect("couldn't get db connection from pool");
    let result = crate::db::get_user(&conn, form.username.clone()).into_future().await;
    log::info!("login: {:?}", result);
    match result {
        Ok(account) => {
            let valid = bcrypt::verify(&form.clear_password, &account.encrypted_password);
            if let Err(e) = valid {
                log::error!("Bcrypt error during login: {}", e);
                return HttpResponse::InternalServerError().json("Internal server error");
            }
            let record = crate::db::create_token(&conn, decode_id(account.hash_id.clone())).into_future().await;
            if let Ok(token) = record {
                return HttpResponse::Ok().json(token);
            }
            log::error!("{:?}", record);
            HttpResponse::InternalServerError().finish()
        },
        Err(_) => {
            log::warn!("Failed login attempt (wrong username). IP={:?}\tusername={}", req.peer_addr(), form.username);
            HttpResponse::Unauthorized().json("Wrong credentials")
        }
    }
}

/// Register a new user
#[post("/account")]
pub(crate) async fn route_register(form: web::Form<RegisterForm>, pool: web::Data<crate::db::Pool>, _req: HttpRequest)  ->  HttpResponse {
    log::debug!("Register");
    if form.clear_password != form.clear_password_2 {
        return HttpResponse::BadRequest().json("Passwords are differents");
    }
    let conn = pool.get().expect("couldn't get db connection from pool");
    let result = crate::db::create_user(&conn, form.username.clone(), bcrypt::hash(form.clear_password.clone(), 10).unwrap()).into_future().await;
    result.map(|_| HttpResponse::Created().finish()).unwrap_or_else(|_| {
            log::warn!("{}", crate::messages::ERROR_USERNAME_EXISTS);
            HttpResponse::BadRequest().json("USERNAME_ALREADY_EXISTS")
        })
}

#[patch("/account")]
pub(crate) async fn route_edit_account(pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    let value = req.headers().get(actix_web::http::header::AUTHORIZATION);
    if let Some(token) = value {
        let raw_token = token.to_str();
        if let Ok(token) = raw_token {
            let conn = pool.get().expect("couldn't get db connection from pool");
            let result = crate::db::get_user_from_token(&conn, token.to_owned()).await;
            if let Ok(account) = result {
                // TODO: upgrade fields
                return HttpResponse::Ok().json(account);
            }
        }
    }
    HttpResponse::Unauthorized().json("Wrong credentials")
}

/// Delete the account. We check the token first so we don't need form data
#[delete("/account")]
pub(crate) async fn route_delete_account(pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::check_token(&pool, req).await {
        log::info!("Deleting account: {:?}", account);
        let conn = pool.get().expect("couldn't get db connection from pool");
        let result = crate::db::delete_account(&conn, account.hash_id).await;
        log::info!("Deleting account result: {:?}", result);
        if result.is_ok() {
            return  HttpResponse::NoContent().finish();
        }
    }
    HttpResponse::BadRequest().json("CANNOT_DELETE_ACCOUNT")
}

/// Allow a user to delete a token in case of problem (laptop or phone stolen) while logged in, it also log the
/// user out if he deletes its current authorization token
#[delete("/tokens/{token}")]
pub(crate) async fn route_delete_token(path: web::Path<(String,)>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::check_token(&pool, req).await {
        let conn = pool.get().expect("couldn't get db connection from pool");
        log::info!("DELETE TOKEN for account: {:?}", account);
        let result = crate::db::delete_token(&conn, account.hash_id, path.0.clone()).await;
        if result.is_ok() {
            return  HttpResponse::NoContent().finish();
        }
    }
    HttpResponse::Ok().body("CANNOT_TOKEN_DELETED")
}

#[cfg(test)]
mod tests {
    /*use actix_web::{
        http::{self, header::ContentType},
        test,
    };

    #[actix_web::test]
    async fn test_register() {
        let username = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
        let req = test::TestRequest::default()
            .insert_header(ContentType::plaintext())
            .to_http_request();
        let resp = crate::routes_account::route_register(req).await;
        assert_eq!(resp.status(), http::StatusCode::OK);
    }*/
}
