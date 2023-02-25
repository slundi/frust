/// Check if the token in the Authorization HTTP header is OK and return the account object
pub(crate) async fn check_token(pool: &actix_web::web::Data<crate::db::Pool>, req: actix_web::HttpRequest) -> Option<crate::db::Account> {
    let value = req.headers().get(actix_web::http::header::AUTHORIZATION);
    if let Some(token_h) = value {
        let raw_token = token_h.to_str().unwrap_or("").to_lowercase();
        if !raw_token.starts_with("token ") {
            return None
        }
        let token_cleaned = raw_token.strip_prefix("token ");
        if let Some(token) = token_cleaned {
            log::info!("Token 6: {:?}", token);
            let conn = pool.get().expect(crate::messages::ERROR_CANNOT_GET_CONNEXION);
            let result = std::future::IntoFuture::into_future(crate::db::account::get_user_from_token(&conn, token.to_owned())).await;
            if let Ok(account) = result {
                return Some(account);
            }
        }
    }
    None
}
