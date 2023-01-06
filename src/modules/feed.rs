use opml::Outline;

use super::Error;

/// Get the `title` field in the `outline` tag if it exists, otherwise it retrieve the `text` field that is
/// mandatory in the tag. If the String is empty, it returns a `NoTitleOrTextInOpmlOutline` error.
fn get_title_or_text(outline: &Outline) -> Result<String, Error> {
    let mut name = String::with_capacity(64);
    if let Some(title) = &outline.title {
        name.push_str(title.trim());
    } else {
        name.push_str(outline.text.trim());
    }
    if name.is_empty() {
        Err(Error::NoTitleOrTextInOpmlOutline)
    } else {
        Ok(name)
    }
}

pub(crate) fn import(body: opml::Body) -> Result<(), Error> {
    for o in body.outlines {
        if let Some(rss) = o.r#type {
            if rss.to_lowercase() != "rss" {
                return Err(Error::NoRssTypeInOutline);
            }
            // TODO: handle RSS line
        } else {
            // this is a folder
            let _name = get_title_or_text(&o);
        }
    }
    Ok(())
}
