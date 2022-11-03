use actix_web::{post, patch, delete, web, HttpResponse, HttpRequest};
use serde::Deserialize;

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

/// Log the user in:
/// 1. Get the user from the nickname
/// 2. Verify bcrypt encoded password
/// 3. Create or update token if client is unknown
/// 
/// It returns the JSON formatted account
#[post("/login")]
pub(crate) async fn route_login(form: web::Form<LoginForm>, pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    let result = crate::db::get_user(&pool, form.username.clone()).await;
    match result {
        Ok(account) => {
            let valid = bcrypt::verify(&form.clear_password, &account.encrypted_password);
            if let Err(e) = valid {
                log::error!("Bcrypt error during login: {}", e);
                return HttpResponse::InternalServerError().json("Internal server error");
            }
            let client: String = String::new();
            crate::db::create_token(&pool, decode_id(account.hash_id.clone()), client).await;
            //TODO: return token used in Authorization HTTP header
            HttpResponse::Ok().json(account)
        },
        Err(_) => {
            log::warn!("Failed login attempt (wrong username). IP={:?}\tusername={}", req.peer_addr(), form.username);
            HttpResponse::Unauthorized().json("Wrong credentials")
        }
    }
}

#[post("/account")]
/// Register a new user
pub(crate) async fn route_register(form: web::Form<RegisterForm>, pool: web::Data<crate::db::Pool>)  ->  HttpResponse {
    if form.clear_password != form.clear_password_2 {
        return HttpResponse::BadRequest().json("Passwords are differents");
    }
    let result = crate::db::get_user(&pool, form.username.clone()).await;
    if result.is_ok() {
        return HttpResponse::BadRequest().json("Username already exists");
    }
    HttpResponse::Ok().body("CREATE ACCOUNT")
}

#[patch("/account")]
pub(crate) async fn route_edit_account(pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    let value = req.headers().get(actix_web::http::header::AUTHORIZATION);
    if let Some(token) = value {
        let raw_token = token.to_str();
        if let Ok(token) = raw_token {
            let result = crate::db::get_user_from_token(&pool, token.to_owned()).await;
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
    HttpResponse::NoContent().body("")
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
