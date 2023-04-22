use actix_web::{post, get, patch, delete, web::{self, Buf}, HttpResponse, HttpRequest, http::header};

use crate::{messages::ERROR_CANNOT_GET_CONNEXION, model::FeedData};



/// List feed with name
#[get("/")]
pub(crate) async fn list(pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::feed::get_feeds(&conn, account.hash_id).await;
        if let Ok(feeds) = result {
            return HttpResponse::Ok().json(feeds);
        }
    }
    HttpResponse::BadRequest().json("CANNOT_LIST_FEEDS")
}

#[get("/{feed_hid}")]
pub(crate) async fn get(path: web::Path<String>, pool: web::Data<crate::db::Pool>, req: HttpRequest) -> HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::feed::get_feed(&conn, account.hash_id, path.into_inner()).await;
        if let Ok(feeds) = result {
            return HttpResponse::Ok().json(feeds);
        }
    }
    HttpResponse::BadRequest().json("CANNOT_GET_FEED")
}

/// Create a feed for the user.
/// If the feed already exists, add a subscription, otherwise:
/// - perform a GET request to retrieve feed (lok for feeds: https://perishablepress.com/what-is-my-wordpress-feed-url/), if it fails the feed is not added and it returns an error
/// - read feed information: title, icon URL (and download it), last updated, ...
/// - save articles as unread (in DB, download images and rewrite image src, ...)
#[post("/")]
pub(crate) async fn post(info: web::Json<FeedData>, pool: web::Data<crate::db::Pool>, req: HttpRequest)  ->  HttpResponse {
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let mut conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let data: FeedData = info.into_inner();
        let result = crate::db::feed::create_feed(&mut conn, account.hash_id.clone(), &data).await;
        if let Ok(hash_id) = result {
            let result = crate::db::feed::subscribe(&conn, account.hash_id, hash_id.clone(), data.folder.clone(), data.selector.clone()).await;
            if result.is_ok() {
                crate::modules::feed::get_links(data.url.clone());
                //TODO: get feed and articles (if selector), returns hash ID, found feed URL, found page URL, found page name if applicable, selector checked if applicable
                return HttpResponse::Created().json(hash_id);
            }
        }
    }
    HttpResponse::BadRequest().json("CANNOT_CREATE_FEED")
}

/// Edit auser subscription:
/// - update `subscription.name` and `subscription.selector`
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
    HttpResponse::BadRequest().json("CANNOT_RENAME_FEED")
}

/// Delete a user's subscription:
#[delete("/{feed_hid}/{what}")]
pub(crate) async fn delete(path: web::Path<(String, String)>, pool: web::Data<crate::db::Pool>, req: HttpRequest) ->  HttpResponse {
    //let delete_articles: bool = matches!(path.1.as_str(), "with_articles");
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        let result = crate::db::feed::unsubscribe_feed(&conn, account.hash_id, path.0.clone()).await;
        if result.is_err() {
            return HttpResponse::BadRequest().json("CANNOT_UNSUBSCRIBE_FEED")
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
    if let Some(account) = crate::auth::check_token(&pool, req).await {
        let mut conn = pool.get().expect(ERROR_CANNOT_GET_CONNEXION);
        // process OPML
        let input = opml::OPML::from_reader(&mut bytes.reader());
        match input {
            Ok(opml) => {
                let folders_and_feeds = crate::modules::feed::import(opml.body);
                if let Ok(data) = folders_and_feeds {
                    if let Ok(folders) = crate::db::folder::get_raw_folders(&conn, account.hash_id.clone()).await {
                        if let Err(e) = crate::db::feed::import(&mut conn, account.hash_id, data, folders) {
                            log::error!("{}: {}", crate::messages::ERROR_IMPORT_FEEDS, e);
                            return HttpResponse::BadRequest().json("CANNOT_IMPORT_OPML_FILE")
                        }
                    }
                }
                // get folders
                // get feeds
                // check URLs
                // sqlite transaction for each folder
                // start scheduler to check feed URL and retrieve them
            },
            Err(e) => {
                log::error!("{}: {}", crate::messages::ERROR_IMPORT_FEEDS, e);
                return HttpResponse::BadRequest().json("CANNOT_READ_OPML_FILE")
            }
        }
        // TODO: add feeds to DB if OK (create folders then add feeds)
        /*let result = crate::db::feed::import(&conn, account).await;
        if result.is_err() {
            return HttpResponse::BadRequest().json("CANNOT_IMPORT_FEED_OPML")
        }*/
        return HttpResponse::NoContent().finish();
    }
    HttpResponse::Forbidden().json("ACCESS_DENIED_FEED_IMPORT_OPML")
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
