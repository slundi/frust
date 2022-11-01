use actix_web::{post, patch, delete, web, HttpResponse, HttpRequest};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct LoginForm {
    username: String,
    clear_password: String
}

#[derive(Debug, Deserialize)]
pub(crate) struct RegisterForm {
    username: String,
    clear_password: String,
    config: String,
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
            crate::db::create_token(&pool, account.id, client).await;
            //TODO: return token used in Authorization HTTP header
            HttpResponse::Ok().json(account)
        },
        Err(_) => {
            log::warn!("Failed login attempt (wrong username). IP={:?}\tusername={}", req.peer_addr(), form.username);
            HttpResponse::Unauthorized().json("Wrong credentials")
        }
    }
}

#[post("/")]
/// Register a new user
pub(crate) async fn route_register(form: web::Form<RegisterForm>)  ->  HttpResponse {
    HttpResponse::Ok().body("CREATE ACCOUNT")
}

#[patch("/{account_hid}/")]
pub(crate) async fn route_edit_account(path: web::Path<(String,)>, req: HttpRequest) ->  HttpResponse {
    let auth = req.headers().get(actix_web::http::header::AUTHORIZATION);
    if let Some(token) = auth {
        return HttpResponse::Ok().body("EDIT ACCOUNT");
    }
    HttpResponse::Unauthorized().json("Wrong credentials")
}

#[delete("/{account_hid}/")]
pub(crate) async fn route_delete_account(path: web::Path<(String,)>, req: HttpRequest) ->  HttpResponse {
    //HttpResponse::Ok().body(format!("User detail: {}", path.into_inner().0))
    HttpResponse::Ok().body("DELETE ACCOUNT")
}

#[delete("/{account_hid}/tokens/{token_hid}/")]
pub(crate) async fn route_delete_token(path: web::Path<(String,)>, req: HttpRequest) ->  HttpResponse {
    HttpResponse::Ok().body("DELETE ACCOUNT TOKEN")
}
