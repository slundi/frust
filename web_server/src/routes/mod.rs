pub(crate) mod account;
pub(crate) mod article;
pub(crate) mod feed;
pub(crate) mod folder;

#[actix_web::get("/")]
async fn index() -> impl actix_web::Responder {
    actix_files::NamedFile::open_async("pages/index.html").await
}
