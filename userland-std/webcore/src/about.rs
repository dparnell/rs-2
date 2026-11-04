//! Built-in `about:` pages.

pub fn page(name: &str) -> String {
    match name {
        "blank" => "<html><head><title></title></head><body></body></html>".into(),
        _ => format!(
            "<html><head><title>WebRS2</title></head><body>\
             <h1>WebRS2</h1>\
             <p>The web browser of rs2, written in Rust: HTTP and HTTPS (rustls), \
             HTML parsed by html5ever, styles, layout and painting of its own.</p>\
             <p>Type an address and press Enter. Pages to try:</p>\
             <ul><li><a href=\"https://example.com/\">example.com</a></li>\
             <li><a href=\"http://info.cern.ch/hypertext/WWW/TheProject.html\">The first web site</a></li>\
             <li><a href=\"https://en.wikipedia.org/wiki/OS/2\">OS/2 on Wikipedia</a></li>\
             <li><a href=\"file:///C:/\">Drive C:</a></li></ul>\
             <p><small>{}</small></p></body></html>",
            crate::http::USER_AGENT
        ),
    }
}
