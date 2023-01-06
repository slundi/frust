pub(crate) mod feed;

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Error {
    NoTitleOrTextInOpmlOutline,
    NoRssTypeInOutline,
}
