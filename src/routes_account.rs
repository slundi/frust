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

pub(crate) async fn check_token(pool: &actix_web::web::Data<crate::db::Pool>, req: actix_web::HttpRequest) -> Option<crate::model::Account> {
    let value = req.headers().get(actix_web::http::header::AUTHORIZATION);
    if let Some(token) = value {
        let raw_token = token.to_str();
        if let Ok(token) = raw_token {
            let conn = pool.get().expect("couldn't get db connection from pool");
            let fut = std::future::IntoFuture::into_future(crate::db::get_user_from_token(&conn, token.to_owned()));
            let result = fut.await;
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
    match result {
        Ok(account) => {
            let valid = bcrypt::verify(&form.clear_password, &account.encrypted_password);
            if let Err(e) = valid {
                log::error!("Bcrypt error during login: {}", e);
                return HttpResponse::InternalServerError().json("Internal server error");
            }
            let client: String = String::new();
            crate::db::create_token(&conn, decode_id(account.hash_id.clone()), client).await;
            //TODO: return token used in Authorization HTTP header
            HttpResponse::Ok().json(account)
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
    let result = crate::db::create_user(&conn, form.username.clone(), form.clear_password.clone()).into_future().await;
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

#[delete("/account")]
pub(crate) async fn route_delete_account(pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    HttpResponse::NoContent().finish()
}

/// Allow a user to delete a token in case of problem (laptop or phone stolen) while logged in
#[delete("/tokens/{token_hid}/")]
pub(crate) async fn route_delete_token(path: web::Path<(String,)>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    HttpResponse::Ok().body("DELETE ACCOUNT TOKEN")
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
