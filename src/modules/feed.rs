use opml::Outline;
use url::{Url, ParseError};

use super::Error;

/// Get the `title` field in the `outline` tag if it exists, otherwise it retrieve the `text` field that is
/// mandatory in the tag. If the String is empty, it returns a `NoTitleOrTextInOpmlOutline` error.
fn get_text(outline: &Outline) -> Result<String, Error> {
    let mut name = String::with_capacity(64);
    name.push_str(outline.text.trim());
    if name.is_empty() {
        Err(Error::NoTextInOpmlOutline)
    } else {
        Ok(name)
    }
}

/// Read OPML outline element. It can be a RSS or a folder. If it is a folder, this function is called recursively.
fn read_outlines(data: &mut Vec<(String, String, String, String, bool)>, folder: &mut String, outlines: &Vec<Outline>) -> Result<(), Error> {
    for o in outlines {
        match get_text(o) {
            Ok(name) => {
                if let Some(rss) = &o.r#type { // RSS line
                    if rss.to_lowercase() != "rss" {
                        return Err(Error::NoRssTypeInOutline);
                    }
                    if let Ok(xml_url) = Url::parse(&o.xml_url.clone().unwrap_or_else(|| String::from(""))) {
                        if let Ok(page_url) = Url::parse(&o.html_url.clone().unwrap_or_else(|| String::from(""))) {
                            data.push((folder.clone(), name, xml_url.to_string(), page_url.to_string(), false));
                        } else {
                            return Err(Error::InvalidPageUrl);
                        }
                    } else {
                        return Err(Error::InvalidFeedUrl);
                    }
                } else { // folder line
                    folder.push_str(" / ");
                    folder.push_str(&name);
                    read_outlines(data, folder, outlines)?
                }
            }
            Err(_) => return Err(Error::NoTextInOpmlOutline),
        }
    }
    Ok(())
}

/// Check the URL and return the URL, if it is redirected it returns a new URL.
fn check_url(url: String) -> Result<String, Error> {
    match ureq::get(&url).call() {
        Ok(response) => {
            if (300..308).contains(&response.status()) {
                if let Some(location) = response.header("Location") {
                    return Ok(String::from(location));
                } else {
                    return Err(Error::UnreachableUrl);
                }
            }
        },
        Err(_e) => return Err(Error::UnreachableUrl),
    }
    Ok(url)
}

pub(crate) fn import(body: opml::Body) -> Result<(), Error> {
    // Vec<folder name if applicable else empty string, feed name, rss URL, page URL, valid URLs>
    let mut data: Vec<(String, String, String, String, bool)> = Vec::new();
    let mut folder = String::with_capacity(64);
    read_outlines(&mut data, &mut folder, &body.outlines)?;
    //TODO: check URLs then insert into DB
    for l in data {
        //let (feed_url, page_url) = futures::future::join(check_url(l.2), check_url(l.3)).await;
    }
    Ok(())
}
