use actix_web::{get, post, patch, delete, web, HttpResponse, HttpRequest};
use std::future::IntoFuture;

use crate::{utils::decode_id, messages::ERROR_CANNOT_GET_CONNEXION, model::{LoginForm, RegisterForm}};

/// Log the user in:
/// 1. Get the user from the nickname
/// 2. Verify bcrypt encoded password
/// 3. Create or update token if client is unknown
/// 
/// It returns the JSON formatted account
#[post("/login")]
pub(crate) async fn login(info: web::Json<LoginForm>, pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    log::debug!("Login");
    let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
    let result = crate::db::account::get_user(&conn, info.username.clone()).into_future().await;
    match result {
        Ok(account) => {
            let valid = bcrypt::verify(&info.clear_password, &account.encrypted_password);
            if let Ok( ok) = valid {
                if !ok {
                    log::warn!("Failed login attempt (wrong username). IP={:?}\tusername={}", req.peer_addr(), info.username);
                    return HttpResponse::Unauthorized().json("WRONG_CREDENTIALS");
                }
            }
            let record = crate::db::account::create_token(&conn, decode_id(account.hash_id.clone())).into_future().await;
            if let Ok(token) = record {
                //TODO: get feeds with folder and unread article count/feed
                return HttpResponse::Ok().json(token);
            }
            log::error!("{:?}", record);
            HttpResponse::InternalServerError().finish()
        },
        Err(_) => {
            log::warn!("Failed login attempt (wrong username). IP={:?}\tusername={}", req.peer_addr(), info.username);
            HttpResponse::Unauthorized().json("WRONG_CREDENTIALS")
        }
    }
}

/// Register a new user and create the default folder
#[post("/account")]
pub(crate) async fn register(info: web::Json<RegisterForm>, pool: web::Data<crate::db::Pool>, _req: HttpRequest)  ->  HttpResponse {
    log::debug!("Register");
    if info.clear_password != info.clear_password_2 {
        return HttpResponse::BadRequest().json("DIFFERENT_PASSWORDS");
    }
    let analyzed = passwords::analyzer::analyze(info.clear_password.clone());
    if analyzed.length() < 8 ||  analyzed.numbers_count() < 1 || analyzed.lowercase_letters_count() < 1 || analyzed.uppercase_letters_count() < 1 || analyzed.symbols_count() < 1 ||
       analyzed.consecutive_count() > 2 || analyzed.progressive_count() > 3 {
        let mut msg = String::with_capacity(22);
        msg.push_str("PASSWORD_STRENGTH:");
        msg.push_str(passwords::scorer::score(&analyzed).round().to_string().trim_end_matches(".0"));
        return HttpResponse::BadRequest().json(msg);
    }
    let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
    let result = crate::db::account::create_user(&conn, info.username.clone(), bcrypt::hash(info.clear_password.clone(), 10).unwrap()).into_future().await;
    if let Ok(account_id) = result {
        let folder = {
            let result = crate::CONFIG.read();
            if result.is_err() {
                log::warn!("{}", crate::messages::ERROR_GET_CONFIG);
                return HttpResponse::BadRequest().json("CANNOT_GET_DEFAULT_FOLDER_NAME");
            }
            result.unwrap().default_folder.clone()
        };
        let result = crate::db::folder::create_folder(&conn, crate::utils::encode_id(account_id), folder).into_future().await;
        if result.is_err() {
            log::warn!("{}", crate::messages::ERROR_CREATE_FOLDER);
            return HttpResponse::BadRequest().json("CANNOT_CREATE_DEFAULT_FOLDER");
        }
    } else {
        log::warn!("{}", crate::messages::ERROR_USERNAME_EXISTS);
        return HttpResponse::BadRequest().json("USERNAME_ALREADY_EXISTS");
    }
    HttpResponse::Created().finish()
}

#[patch("/account")]
pub(crate) async fn patch(pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    let value = req.headers().get(actix_web::http::header::AUTHORIZATION);
    if let Some(token) = value {
        let raw_token = token.to_str();
        if let Ok(token) = raw_token {
            let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
            let result = crate::db::account::get_user_from_token(&conn, token.to_owned()).await;
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
pub(crate) async fn delete(pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        log::info!("Deleting account: {:?}", account);
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::account::delete_account(&conn, account.hash_id).await;
        log::info!("Deleting account result: {:?}", result);
        if result.is_ok() {
            return  HttpResponse::NoContent().finish();
        }
    }
    HttpResponse::BadRequest().json("CANNOT_DELETE_ACCOUNT")
}

/// Renew the current token: it does not take data but it returns a new UUID as a response
#[patch("/tokens/{token}")]
pub(crate) async fn renew_token(path: web::Path<(String,)>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        log::info!("RENEW TOKEN for account: {}", account.username);
        let result = crate::db::account::renew_token(&conn, account.hash_id, path.0.clone()).await;
        if let Ok(token) = result {
            return  HttpResponse::Ok().json(token);
        }
    }
    HttpResponse::InternalServerError().body("CANNOT_RENEW_TOKEN")
}

/// Allow a user to delete a token in case of problem (laptop or phone stolen) while logged in, it also log the
/// user out if he deletes its current authorization token
#[delete("/tokens/{token}")]
pub(crate) async fn delete_token(path: web::Path<(String,)>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        log::info!("DELETE TOKEN for account: {}", account.username);
        let result = crate::db::account::delete_token(&conn, account.hash_id, path.0.clone()).await;
        if result.is_ok() {
            return  HttpResponse::NoContent().finish();
        }
    }
    HttpResponse::InternalServerError().body("CANNOT_DELETE_TOKEN")
}

/// Get all user tokens, in case he wants to revoke/delete som
#[get("/tokens")]
pub(crate) async fn list_tokens(pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::account::get_tokens(&conn, account.hash_id).await;
        if let Ok(tokens) = result{
            return  HttpResponse::Ok().json(tokens);
        }
    }
    HttpResponse::InternalServerError().body("CANNOT_LIST_TOKENS")
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
