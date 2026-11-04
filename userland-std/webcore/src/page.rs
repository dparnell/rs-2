//! A page: its document, style sheets and styles, ready for layout; and
//! the images it refers to.

use std::collections::HashMap;

use url::Url;

use crate::css::{Media, Origin, Rules};
use crate::dom::{Dom, NodeId};
use crate::http;
use crate::style::{self, Styles};

pub struct Page {
    pub url: Url,
    pub dom: Dom,
    pub title: String,
    pub sheets: Vec<(String, Url)>,
    pub rules: Rules,
    pub styles: Styles,
    /// The width the styles were computed for.
    pub style_width: f32,
    /// <img> elements and their sources.
    pub images: Vec<(NodeId, Url)>,
    pub status: u16,
}

fn ua_rules(media: Media) -> Rules {
    let mut r = Rules::default();
    r.add_sheet(style::UA_CSS, Origin::UserAgent, media);
    r
}

fn wrap(kind: &str, body: &str) -> String {
    format!("<html><head><title>{kind}</title></head><body>{body}</body></html>")
}

/// Fetch and prepare a page for a viewport of `size` pels. `progress` is
/// told what is being fetched.
pub fn load(url: &Url, size: (f32, f32), progress: &dyn Fn(&str)) -> Result<Page, http::Error> {
    load_with(url, None, size, progress)
}

/// A form sent with POST.
pub fn load_post(url: &Url, content_type: &str, data: &[u8], size: (f32, f32), progress: &dyn Fn(&str)) -> Result<Page, http::Error> {
    load_with(url, Some(http::Body { content_type, data }), size, progress)
}

fn load_with(url: &Url, body: Option<http::Body>, size: (f32, f32), progress: &dyn Fn(&str)) -> Result<Page, http::Error> {
    let (u, dom, status) = fetch_dom(url, body, progress)?;
    let mut page = from_dom(u, dom, size, progress);
    page.status = status;
    Ok(page)
}

/// Fetch a URL and parse it (non-HTML content is wrapped in a document):
/// the URL it came from, the document and the HTTP status. Scripts can
/// then change the document before [`from_dom`] styles it.
pub fn fetch_dom(url: &Url, body: Option<http::Body>, progress: &dyn Fn(&str)) -> Result<(Url, Dom, u16), http::Error> {
    progress(&format!("Connecting to {}...", url.host_str().unwrap_or(url.as_str())));
    let r = http::fetch(url, body)?;
    let mime = r.mime();
    let final_url = r.url.clone();
    let html = if mime == "text/html" || mime == "application/xhtml+xml" || mime.is_empty() && !r.body.is_empty() && r.body.iter().take(512).any(|&b| b == b'<') {
        http::decode_text(&r)
    } else if mime.starts_with("image/") {
        wrap(final_url.as_str(), &format!("<img src=\"{}\">", http::escape(final_url.as_str())))
    } else if mime.starts_with("text/") || mime.contains("json") || mime.contains("javascript") || mime.contains("xml") {
        wrap(final_url.as_str(), &format!("<pre>{}</pre>", http::escape(&http::decode_text(&r))))
    } else {
        wrap("Download", &format!("<h1>{}</h1><p>This is a file of type {} ({} bytes); WebRS2 cannot show it.</p>", http::escape(final_url.as_str()), http::escape(&mime), r.body.len()))
    };
    progress("Reading the page...");
    let dom = crate::dom::parse(&html);
    Ok((final_url, dom, r.status))
}

/// Prepare a parsed document (fetching its style sheets).
pub fn from_dom(url: Url, dom: Dom, size: (f32, f32), progress: &dyn Fn(&str)) -> Page {
    let media = Media { width: size.0, height: size.1 };
    let mut base = url.clone();
    if let Some(h) = dom.find("base").and_then(|n| dom.element(n)).and_then(|e| e.attr("href")) {
        if let Ok(u) = url.join(h) {
            base = u;
        }
    }
    // Style sheets in document order.
    let mut sheets: Vec<(String, Url)> = Vec::new();
    let mut fetched: HashMap<Url, ()> = HashMap::new();
    for n in dom.descendants(Dom::DOCUMENT) {
        let Some(e) = dom.element(n) else { continue };
        if e.is("style") {
            if e.attr("media").is_none_or(|m| crate::css::media_applies(m, media)) {
                sheets.push((dom.text_content(n), base.clone()));
            }
        } else if e.is("link") {
            let rel = e.attr("rel").unwrap_or("").to_ascii_lowercase();
            if !rel.split_ascii_whitespace().any(|r| r == "stylesheet") || rel.contains("alternate") {
                continue;
            }
            if e.attr("media").is_some_and(|m| !crate::css::media_applies(m, media)) {
                continue;
            }
            let Some(u) = e.attr("href").and_then(|h| base.join(h).ok()) else { continue };
            fetch_sheet(&u, &mut sheets, &mut fetched, media, progress, 0);
        }
    }
    let mut rules = ua_rules(media);
    for (css, _) in &sheets {
        rules.add_sheet(css, Origin::Author, media);
    }
    progress("Applying styles...");
    let styles = style::compute(&dom, &rules, size);
    let images = dom
        .descendants(Dom::DOCUMENT)
        .filter_map(|n| {
            let e = dom.element(n)?;
            let src = if e.is("img") {
                e.attr("src").or(e.attr("data-src"))?
            } else if e.is("input") && e.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("image")) {
                e.attr("src")?
            } else {
                return None;
            };
            let u = base.join(src.trim()).ok()?;
            matches!(u.scheme(), "http" | "https" | "file" | "data").then_some((n, u))
        })
        .collect();
    let title = dom.title().unwrap_or_else(|| url.to_string());
    Page { url, dom, title, sheets, rules, styles, style_width: size.0, images, status: 200 }
}

fn fetch_sheet(u: &Url, sheets: &mut Vec<(String, Url)>, fetched: &mut HashMap<Url, ()>, media: Media, progress: &dyn Fn(&str), depth: u32) {
    if fetched.contains_key(u) || depth > 3 {
        return;
    }
    fetched.insert(u.clone(), ());
    progress(&format!("Fetching style sheet {u}..."));
    let Ok(r) = http::get(u) else { return };
    if r.status >= 400 {
        return;
    }
    let css = http::decode_text(&r);
    // @imports come first.
    for imp in crate::css::imports_of(&css) {
        if let Ok(iu) = r.url.join(&imp) {
            fetch_sheet(&iu, sheets, fetched, media, progress, depth + 1);
        }
    }
    sheets.push((css, r.url.clone()));
}

impl Page {
    /// Styles again for another viewport width (media queries).
    pub fn restyle(&mut self, size: (f32, f32)) {
        let media = Media { width: size.0, height: size.1 };
        let mut rules = ua_rules(media);
        for (css, _) in &self.sheets {
            rules.add_sheet(css, Origin::Author, media);
        }
        self.styles = style::compute(&self.dom, &rules, size);
        self.rules = rules;
        self.style_width = size.0;
    }
}
