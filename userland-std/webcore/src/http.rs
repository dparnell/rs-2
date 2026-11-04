//! HTTP/1.1 client over TCP and TLS (rustls with the ring provider and
//! Mozilla's root certificates from webpki-roots).
//!
//! One connection per request (`Connection: close`); chunked and
//! length-delimited bodies; gzip and deflate content encodings; redirects;
//! a cookie jar; `file:` URLs read local files and `data:` URLs decode
//! their contents.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use url::Url;

pub const USER_AGENT: &str = "Mozilla/5.0 (OS/2; rs2) WebRS2/0.1";

#[derive(Debug)]
pub struct Response {
    /// The URL the content came from (after redirects).
    pub url: Url,
    pub status: u16,
    pub reason: String,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The MIME type (lower case, without parameters).
    pub fn mime(&self) -> String {
        self.header("content-type").unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase()
    }

    /// The charset parameter of Content-Type, if any.
    pub fn charset(&self) -> Option<String> {
        let ct = self.header("content-type")?;
        ct.split(';').skip(1).find_map(|p| {
            let (k, v) = p.split_once('=')?;
            (k.trim().eq_ignore_ascii_case("charset")).then(|| v.trim().trim_matches('"').to_ascii_lowercase())
        })
    }
}

#[derive(Debug)]
pub enum Error {
    Url(String),
    Io(io::Error),
    Tls(String),
    Protocol(String),
    TooManyRedirects,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Url(s) => write!(f, "bad address: {s}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Tls(s) => write!(f, "secure connection failed: {s}"),
            Error::Protocol(s) => write!(f, "bad response: {s}"),
            Error::TooManyRedirects => write!(f, "too many redirections"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        // rustls reports certificate problems as io::Error(InvalidData).
        if let Some(inner) = e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>()) {
            return Error::Tls(inner.to_string());
        }
        Error::Io(e)
    }
}

fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let mut c = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("TLS versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            c.alpn_protocols = vec![b"http/1.1".to_vec()];
            Arc::new(c)
        })
        .clone()
}

/// A connection, plain or TLS.
pub enum Conn {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Conn {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(b),
            Conn::Tls(s) => match s.read(b) {
                // Servers that close without close_notify: end of data.
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(0),
                r => r,
            },
        }
    }
}

impl Write for Conn {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(b),
            Conn::Tls(s) => s.write(b),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            Conn::Tls(s) => s.flush(),
        }
    }
}

pub fn connect(url: &Url) -> Result<Conn, Error> {
    let host = url.host_str().ok_or_else(|| Error::Url(url.to_string()))?;
    let port = url.port_or_known_default().ok_or_else(|| Error::Url(url.to_string()))?;
    let tcp = TcpStream::connect((host, port))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30)))?;
    let _ = tcp.set_nodelay(true);
    match url.scheme() {
        "http" => Ok(Conn::Plain(tcp)),
        "https" => {
            let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| Error::Url(e.to_string()))?;
            let conn = rustls::ClientConnection::new(tls_config(), name).map_err(|e| Error::Tls(e.to_string()))?;
            let mut s = rustls::StreamOwned::new(conn, tcp);
            // Handshake now, so that certificate errors show as such.
            while s.conn.is_handshaking() {
                s.conn.complete_io(&mut s.sock)?;
            }
            Ok(Conn::Tls(Box::new(s)))
        }
        s => Err(Error::Url(format!("unsupported scheme {s}"))),
    }
}

/// Cookies by host: name -> value.
static COOKIES: Mutex<Option<HashMap<String, Vec<(String, String)>>>> = Mutex::new(None);

fn cookie_header(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    let jar = COOKIES.lock().unwrap();
    let jar = jar.as_ref()?;
    let mut parts = Vec::new();
    for (domain, list) in jar.iter() {
        if host == *domain || host.ends_with(&format!(".{domain}")) {
            for (n, v) in list {
                parts.push(format!("{n}={v}"));
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn store_cookie(url: &Url, set: &str) {
    let Some(host) = url.host_str() else { return };
    let mut fields = set.split(';');
    let Some((name, value)) = fields.next().and_then(|nv| nv.split_once('=')) else { return };
    let mut domain = host.to_ascii_lowercase();
    for f in fields {
        if let Some((k, v)) = f.split_once('=') {
            if k.trim().eq_ignore_ascii_case("domain") {
                let d = v.trim().trim_start_matches('.').to_ascii_lowercase();
                if domain == d || domain.ends_with(&format!(".{d}")) {
                    domain = d;
                }
            }
        }
    }
    let mut jar = COOKIES.lock().unwrap();
    let list = jar.get_or_insert_with(HashMap::new).entry(domain).or_default();
    let name = name.trim().to_string();
    list.retain(|(n, _)| *n != name);
    list.push((name, value.trim().to_string()));
}

/// A request body and its type.
pub struct Body<'a> {
    pub content_type: &'a str,
    pub data: &'a [u8],
}

/// Fetch `url`, following redirects.
pub fn get(url: &Url) -> Result<Response, Error> {
    fetch(url, None)
}

pub fn fetch(url: &Url, mut body: Option<Body>) -> Result<Response, Error> {
    let mut url = url.clone();
    for _ in 0..10 {
        let r = fetch_once(&url, body.as_ref())?;
        if matches!(r.status, 301 | 302 | 303 | 307 | 308) {
            if let Some(loc) = r.header("location") {
                url = url.join(loc).map_err(|e| Error::Url(e.to_string()))?;
                if r.status != 307 && r.status != 308 {
                    body = None;
                }
                continue;
            }
        }
        return Ok(r);
    }
    Err(Error::TooManyRedirects)
}

/// One request, no redirects followed.
pub fn fetch_once(url: &Url, body: Option<&Body>) -> Result<Response, Error> {
    match url.scheme() {
        "file" => return file(url),
        "data" => return data(url),
        "about" => {
            return Ok(Response {
                url: url.clone(),
                status: 200,
                reason: "OK".into(),
                headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
                body: crate::about::page(url.path()).into_bytes(),
            });
        }
        _ => {}
    }
    let mut conn = connect(url)?;
    let mut path = url.path().to_string();
    if let Some(q) = url.query() {
        path.push('?');
        path.push_str(q);
    }
    let host = match url.port() {
        Some(p) => format!("{}:{p}", url.host_str().unwrap_or("")),
        None => url.host_str().unwrap_or("").to_string(),
    };
    let method = if body.is_some() { "POST" } else { "GET" };
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\nAccept: text/html,application/xhtml+xml,*/*;q=0.8\r\nAccept-Encoding: gzip, deflate\r\nAccept-Language: en\r\nConnection: close\r\n"
    );
    if let Some(c) = cookie_header(url) {
        req.push_str(&format!("Cookie: {c}\r\n"));
    }
    if let Some(b) = body {
        req.push_str(&format!("Content-Type: {}\r\nContent-Length: {}\r\n", b.content_type, b.data.len()));
    }
    req.push_str("\r\n");
    let mut out = req.into_bytes();
    if let Some(b) = body {
        out.extend_from_slice(b.data);
    }
    conn.write_all(&out)?;
    conn.flush()?;
    let mut r = BufReader::with_capacity(16384, conn);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut it = line.trim_end().splitn(3, ' ');
    let proto = it.next().unwrap_or("");
    if !proto.starts_with("HTTP/") {
        return Err(Error::Protocol(format!("status line {:?}", line.trim_end())));
    }
    let status: u16 = it.next().and_then(|s| s.parse().ok()).ok_or_else(|| Error::Protocol(line.clone()))?;
    let reason = it.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            break;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((n, v)) = l.split_once(':') {
            headers.push((n.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    for (n, v) in &headers {
        if n == "set-cookie" {
            store_cookie(url, v);
        }
    }
    let hdr = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    let mut raw = Vec::new();
    if !(status == 204 || status == 304 || (100..200).contains(&status)) {
        if hdr("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked")) {
            loop {
                line.clear();
                r.read_line(&mut line)?;
                let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("0").trim(), 16).unwrap_or(0);
                if size == 0 {
                    break;
                }
                let start = raw.len();
                raw.resize(start + size, 0);
                r.read_exact(&mut raw[start..])?;
                line.clear();
                r.read_line(&mut line)?;
            }
        } else if let Some(len) = hdr("content-length").and_then(|l| l.parse::<usize>().ok()) {
            raw.resize(len, 0);
            let mut got = 0;
            while got < len {
                let n = r.read(&mut raw[got..])?;
                if n == 0 {
                    break;
                }
                got += n;
            }
            raw.truncate(got);
        } else {
            r.read_to_end(&mut raw)?;
        }
    }
    let body = match hdr("content-encoding").map(|e| e.to_ascii_lowercase()) {
        Some(e) if e.contains("gzip") => {
            let mut v = Vec::new();
            flate2::read::GzDecoder::new(&raw[..]).read_to_end(&mut v).map(|_| v).unwrap_or(raw)
        }
        Some(e) if e.contains("deflate") => {
            let mut v = Vec::new();
            match flate2::read::ZlibDecoder::new(&raw[..]).read_to_end(&mut v) {
                Ok(_) => v,
                Err(_) => {
                    let mut v = Vec::new();
                    flate2::read::DeflateDecoder::new(&raw[..]).read_to_end(&mut v).map(|_| v).unwrap_or(raw)
                }
            }
        }
        _ => raw,
    };
    Ok(Response { url: url.clone(), status, reason, headers, body })
}

fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "htm" | "html" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "txt" | "cmd" | "sys" | "ini" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// A file: URL: file:///C:/dir/page.html.
fn file(url: &Url) -> Result<Response, Error> {
    let mut path = url.path().to_string();
    let decoded = percent_decode(&path);
    path = decoded.trim_start_matches('/').to_string();
    if path.len() < 2 || path.as_bytes()[1] != b':' {
        path = format!("/{path}");
    }
    let meta = std::fs::metadata(&path)?;
    if meta.is_dir() {
        let mut names: Vec<(bool, String)> = std::fs::read_dir(&path)?
            .filter_map(|e| e.ok())
            .map(|e| (e.file_type().map(|t| t.is_dir()).unwrap_or(false), e.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.to_ascii_lowercase().cmp(&b.1.to_ascii_lowercase())));
        let mut html = format!("<html><head><title>{}</title></head><body><h1>Index of {}</h1><ul>", escape(&path), escape(&path));
        let base = url.as_str().trim_end_matches('/');
        html.push_str(&format!("<li><a href=\"{base}/..\">..</a></li>"));
        for (dir, n) in names {
            html.push_str(&format!("<li><a href=\"{base}/{}\">{}{}</a></li>", escape(&n), escape(&n), if dir { "/" } else { "" }));
        }
        html.push_str("</ul></body></html>");
        return Ok(Response {
            url: url.clone(),
            status: 200,
            reason: "OK".into(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: html.into_bytes(),
        });
    }
    let body = std::fs::read(&path)?;
    Ok(Response {
        url: url.clone(),
        status: 200,
        reason: "OK".into(),
        headers: vec![("content-type".into(), mime_for(&path).into())],
        body,
    })
}

fn data(url: &Url) -> Result<Response, Error> {
    let s = &url.as_str()[5..];
    let (meta, payload) = s.split_once(',').ok_or_else(|| Error::Url("data: without a comma".into()))?;
    let base64 = meta.ends_with(";base64");
    let mime = meta.trim_end_matches(";base64");
    let body = if base64 { base64_decode(payload) } else { percent_decode(payload).into_bytes() };
    Ok(Response {
        url: url.clone(),
        status: 200,
        reason: "OK".into(),
        headers: vec![("content-type".into(), if mime.is_empty() { "text/plain".into() } else { mime.to_string() })],
        body,
    })
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Text of a response body: the charset from the header, else a
/// `<meta charset>` in the first KB, else UTF-8 if it is valid, else
/// windows-1252.
pub fn decode_text(r: &Response) -> String {
    let label = r.charset().or_else(|| sniff_meta_charset(&r.body));
    let enc = label
        .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
        .unwrap_or_else(|| if std::str::from_utf8(&r.body).is_ok() { encoding_rs::UTF_8 } else { encoding_rs::WINDOWS_1252 });
    enc.decode(&r.body).0.into_owned()
}

fn sniff_meta_charset(b: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&b[..b.len().min(2048)]).to_ascii_lowercase();
    let i = head.find("charset=")?;
    let rest = head[i + 8..].trim_start_matches(['"', '\'']);
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_string())
}
