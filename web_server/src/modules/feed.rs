use opml::Outline;

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
fn read_outlines(data: &mut Vec<(String, String, String, String)>, folder: &mut String, outlines: &Vec<Outline>) -> Result<(), Error> {
    for o in outlines {
        match get_text(o) {
            Ok(name) => {
                if let Some(rss) = &o.r#type { // RSS line
                    if rss.to_lowercase() != "rss" {
                        return Err(Error::NoRssTypeInOutline);
                    }
                    if let Ok(xml_url) = url::Url::parse(&o.xml_url.clone().unwrap_or_else(|| String::from(""))) {
                        if let Ok(page_url) = url::Url::parse(&o.html_url.clone().unwrap_or_else(|| String::from(""))) {
                            data.push((folder.clone(), name, xml_url.to_string(), page_url.to_string()));
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

/// Process the uncompressed OPML file and return it in a Vec of strings: Vec<folder name if applicable else empty string, feed name, rss URL, page URL, valid URLs>
pub(crate) fn import(body: opml::Body) -> Result<Vec<(String, String, String, String)>, Error> {
    let mut data: Vec<(String, String, String, String)> = Vec::new();
    let mut folder = String::with_capacity(64);
    read_outlines(&mut data, &mut folder, &body.outlines)?;
    //TODO: insert into DB with feed status 'ADDED' and start scheduler
    Ok(data)
}

/// Search for feeds in a HTML page, it looks for the `link` tag (see [tag documentation](https://developer.mozilla.org/en-US/docs/Web/HTML/Attributes/rel#attr-alternate)).
/// Multiple feeds can be available in a page.
fn list_feeds_in_html(html: String) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    let document = scraper::Html::parse_document(&html);
    let selector = scraper::Selector::parse("link[rel=\"alternate\"").expect("Bad HTML selector format");
    for tag in document.select(&selector) {
        if let Some(r#type) = tag.value().attr("type") {
            if r#type.starts_with("application/atom+xml") || r#type.starts_with("application/rss+xml") {
                urls.push(tag.value().attr("href").unwrap().to_owned());
            }
        }
    }
    urls
}

/// Read the feed content to get the title
fn get_feed_title(content: String) -> Option<String> {
    match feed_rs::parser::parse(content.as_bytes()) {
        Ok(result) => {
            if let Some(title) = result.title {
                return Some(title.content);
            }
            None
        },
        Err(_) => None,
    }
}

/// From an URL, check if it is a feed, otherwise its reads the HTML in order to find feeds.
pub(crate) fn get_links(url: String) -> Vec<(String, String)> {
    let mut feeds: Vec<(String, String)> = Vec::new();
    match ureq::get(&url).call() {
        Ok(response) => {
            let content_type = response.content_type().trim().to_lowercase();
            // HTTP error, abort! Handle redirection (30x)?
            if response.status() != 200 {
                return feeds;
            }
            // Do not process unrelevant content
            if !content_type.starts_with("application/atom+xml") && !content_type.starts_with("application/rss+xml") && !content_type.starts_with("text/html") {
                return feeds;
            }
            if let Ok(html) = response.into_string() {
                if content_type.starts_with("application/atom+xml") || content_type.starts_with("application/rss+xml") {
                    if let Some(title) = get_feed_title(html) {
                        feeds.push((url, title));
                    } else {
                        feeds.push((url.clone(), url));
                    }
                } else if content_type.starts_with("text/html") {
                    list_feeds_in_html(html);
                    //TODO: find feed(s) in the page. Multiple feeds can be available!
                    todo!()
                }
            }
        },
        Err(e) => {
            log::error!("{}: {}", crate::messages::ERROR_OPEN_URL, e);
            //return HttpResponse::BadRequest().json("CANNOT_OPEN_URL");
        },
    };
    feeds
}
