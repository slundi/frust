use actix_web::{post, get, patch, delete, web, HttpResponse, HttpRequest, http::header};
use serde::Deserialize;

use crate::messages::ERROR_CANNOT_GET_CONNEXION;

/// Struct to create and edit form
#[derive(Debug, Deserialize)]
pub struct FeedData {
    /// URL of the RSS or Atom feed
    pub url: String,
    /// Optional name of the feed if the user renamed it. Otherwise it is provided in the feed data
    pub name: String,
    /// Folder hash ID
    pub folder: String,
    pub xpath: String
}

/// List feed with name
#[get("/")]
pub(crate) async fn list(pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::feed::get_feeds(&conn, account.hash_id, None).await;
        if let Ok(feeds) = result {
            return HttpResponse::Ok().json(feeds);
        }
    }
    HttpResponse::BadRequest().json("BAD_REQUEST_LIST_FEEDS")
}

/// Create a feed for the user.
/// If the feed already exists, add a subscription, otherwise:
/// - perform a GET request to retrieve feed, if it fails the feed is not added and it returns an error
/// - read feed information: title, icon URL (and download it), last updated, ...
/// - save articles as unread (in DB, download images and rewrite image src, ...)
#[post("/")]
pub(crate) async fn post(info: web::Json<FeedData>, pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::feed::create_feed(&conn, account.hash_id.clone(), info.url.clone(), info.name.clone()).await;
        if let Ok(hash_id) = result {
            let result = crate::db::feed::subscribe(&conn, account.hash_id, hash_id.clone(), info.folder.clone(), info.xpath.clone()).await;
            if result.is_ok() {
                //TODO: get feed and articles
                return HttpResponse::Created().json(hash_id);
            }
        }
    }
    HttpResponse::BadRequest().json("BAD_REQUEST_CREATE_FEED")
}

/// Edit auser subscription:
/// - update `subscription.name` and `subscription.xpath`
/// - update URL using `subscription.feed_id` with:
///   - existing feed.id if the URL already exists
///   - new feed.id if the URL is not found in the DB -> get feed informations and articles
#[patch("/{feed_hid}")]
pub(crate) async fn patch(info: web::Json<FeedData>, path: web::Path<String>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::feed::edit_feed(&conn, account.hash_id, path.into_inner(), info.url.clone(), info.name.clone()).await;
        if result.is_ok() {
            return HttpResponse::Ok().finish();
        }
    }
    HttpResponse::BadRequest().json("BAD_REQUEST_RENAME_FEED")
}

/// Delete a user's subscription:
#[delete("/{feed_hid}/{what}")]
pub(crate) async fn delete(path: web::Path<(String, String)>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    //let delete_articles: bool = matches!(path.1.as_str(), "with_articles");
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::feed::unsubscribe_feed(&conn, account.hash_id, path.0.clone()).await;
        if result.is_err() {
            return HttpResponse::BadRequest().json("Cannot unsubscribe feed")
        }
        //TODO: delete articles if delete_articles
        let result = crate::db::feed::clear_unused_feed(&conn, path.0.clone()).await;
        if result.is_err() {
            return HttpResponse::BadRequest().json("BAD_REQUEST_DELETE_FEED")
        }
    }
    HttpResponse::NoContent().finish()
}

/// Import OPML file for the user
#[post("/opml")]
pub(crate) async fn import_opml(bytes: web::Bytes, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    HttpResponse::NoContent().finish()
}

/// Export user's OPML file
#[get("/opml")]
pub(crate) async fn export_opml(pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::feed::export(&conn, account).await;
        if let Ok(xml) = result {
            return HttpResponse::Ok().insert_header(header::ContentType::xml()).body(xml);
        }
    }
    HttpResponse::Forbidden().json("ACCESS_DENIED_FEED_EXPORT_OPML")
}
