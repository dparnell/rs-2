//! webcore: the engine shared by rs2's web browsers (WEBTEXT, the
//! text-mode one, and WEB, the Presentation Manager one).
//!
//! - [`http`]: HTTP/1.1 over TCP or TLS (rustls, ring, webpki-roots),
//!   `file:`, `data:` and `about:` URLs
//! - [`dom`]: the document tree html5ever builds
//! - [`text`]: a document as wrapped text with numbered links
//! - [`codepage`]: Unicode to the PC code pages of OS/2 text

pub mod about;
pub mod codepage;
pub mod dom;
pub mod http;
pub mod image;
pub mod page;
pub mod layout;
pub mod css;
pub mod select;
pub mod style;
pub mod text;

pub use url::Url;

/// What a URL turned out to hold.
pub enum Content {
    Html(dom::Dom),
    Text(String),
    Image(Vec<u8>),
    Other(String),
}

pub struct Loaded {
    pub url: Url,
    pub status: u16,
    pub content: Content,
}

/// Fetch a URL and parse what came back.
pub fn load(url: &Url) -> Result<Loaded, http::Error> {
    let r = http::get(url)?;
    let mime = r.mime();
    let content = if mime == "text/html" || mime == "application/xhtml+xml" || (mime.is_empty() && looks_like_html(&r.body)) {
        Content::Html(dom::parse(&http::decode_text(&r)))
    } else if mime.starts_with("text/") || mime.is_empty() || mime == "application/json" || mime == "application/javascript" {
        Content::Text(http::decode_text(&r))
    } else if mime.starts_with("image/") {
        Content::Image(r.body)
    } else {
        Content::Other(mime)
    };
    Ok(Loaded { url: r.url, status: r.status, content })
}

fn looks_like_html(b: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_ascii_lowercase();
    head.contains("<html") || head.contains("<!doctype html") || head.contains("<body")
}

/// Make a URL of what a user typed: "example.com" is http://example.com/,
/// "C:\dir\file.htm" is a file: URL.
pub fn parse_user_url(s: &str) -> Option<Url> {
    let s = s.trim();
    if s.len() >= 2 && s.as_bytes()[1] == b':' && s.as_bytes()[0].is_ascii_alphabetic() {
        let p = s.replace('\\', "/");
        return Url::parse(&format!("file:///{p}")).ok();
    }
    if let Ok(u) = Url::parse(s) {
        if u.has_host() || matches!(u.scheme(), "file" | "data" | "about") {
            return Some(u);
        }
    }
    Url::parse(&format!("http://{s}")).ok()
}
