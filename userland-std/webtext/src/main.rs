//! WEBTEXT: rs2's text-mode web browser.
//!
//! ```text
//! WEBTEXT [-dump] [-width N] [URL]
//! ```
//!
//! Shows a page a screen at a time. At the prompt: Enter for the next
//! screen, a link number to follow it, `g URL` to go somewhere, `b` back,
//! `r` reload, `l` the list of links, `t` back to the top, `q` to quit.
//! `-dump` prints the whole page and its links and exits.

use std::io::{BufRead, Write};

use rs2libc as _;
use webcore::{Content, Url, codepage, text};

struct Page {
    url: Url,
    title: String,
    lines: Vec<String>,
    links: Vec<Url>,
}

fn load(url: &Url, width: usize) -> Page {
    match webcore::load(url) {
        Ok(l) => {
            let (title, lines, links) = match l.content {
                Content::Html(dom) => {
                    let t = text::render(&dom, &l.url, width);
                    (t.title.unwrap_or_else(|| l.url.to_string()), t.lines, t.links)
                }
                Content::Text(s) => (l.url.to_string(), s.lines().map(|l| l.replace('\t', "    ")).collect(), Vec::new()),
                Content::Image(b) => (l.url.to_string(), vec![format!("(an image, {} bytes)", b.len())], Vec::new()),
                Content::Other(m) => (l.url.to_string(), vec![format!("(a file of type {m}, not shown)")], Vec::new()),
            };
            let mut lines = lines;
            if l.status >= 400 {
                lines.insert(0, format!("(the server answered {})", l.status));
            }
            Page { url: l.url, title, lines, links }
        }
        Err(e) => Page { url: url.clone(), title: "Error".into(), lines: vec![format!("Could not load {url}: {e}")], links: Vec::new() },
    }
}

fn out(s: &str, cp: u32) {
    let mut b = codepage::encode(s, cp);
    b.push(b'\n');
    let _ = std::io::stdout().write_all(&b);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dump = false;
    // -retry: keep trying for a minute while the network comes up (tests).
    let mut retry = false;
    let mut width = 79;
    let mut start = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].to_ascii_lowercase().as_str() {
            "-dump" | "/dump" => dump = true,
            "-retry" => retry = true,
            "-width" | "/width" => {
                i += 1;
                width = args.get(i).and_then(|w| w.parse().ok()).unwrap_or(79);
            }
            _ => start = Some(args[i].clone()),
        }
        i += 1;
    }
    let cp = codepage::current();
    let url = start.as_deref().and_then(webcore::parse_user_url).unwrap_or_else(|| Url::parse("about:home").unwrap());
    let mut page = load(&url, width);
    let t0 = std::time::Instant::now();
    while retry && page.title == "Error" && t0.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        page = load(&url, width);
    }
    if dump {
        out(&format!("== {} ==", page.title), cp);
        out(&format!("   {}", page.url), cp);
        out("", cp);
        for l in &page.lines {
            out(l, cp);
        }
        if !page.links.is_empty() {
            out("", cp);
            out("Links:", cp);
            for (i, l) in page.links.iter().enumerate() {
                out(&format!("[{}] {}", i + 1, l), cp);
            }
        }
        return;
    }
    let mut history: Vec<Url> = Vec::new();
    let mut top = 0;
    let screen = 21;
    let stdin = std::io::stdin();
    loop {
        if top == 0 {
            out(&format!("== {} ==", page.title), cp);
            out(&format!("   {}", page.url), cp);
        }
        let end = (top + screen).min(page.lines.len());
        for l in &page.lines[top..end] {
            out(l, cp);
        }
        let more = end < page.lines.len();
        print!(
            "-- {}{} lines, {} links. Enter=more, number=follow, g URL, b=back, l=links, r=reload, q=quit: ",
            if more { format!("{}% of ", end * 100 / page.lines.len().max(1)) } else { String::new() },
            page.lines.len(),
            page.links.len()
        );
        let _ = std::io::stdout().flush();
        let mut cmd = String::new();
        if stdin.lock().read_line(&mut cmd).unwrap_or(0) == 0 {
            break;
        }
        let cmd = cmd.trim();
        let go = |to: Url, page: &mut Page, top: &mut usize, history: &mut Vec<Url>| {
            history.push(page.url.clone());
            *page = load(&to, width);
            *top = 0;
        };
        match cmd {
            "" => {
                if more {
                    top = end;
                } else {
                    top = page.lines.len();
                }
            }
            "q" | "Q" => break,
            "t" => top = 0,
            "r" => {
                page = load(&page.url.clone(), width);
                top = 0;
            }
            "b" => {
                if let Some(u) = history.pop() {
                    page = load(&u, width);
                    top = 0;
                }
            }
            "l" => {
                for (i, l) in page.links.iter().enumerate() {
                    out(&format!("[{}] {}", i + 1, l), cp);
                }
                top = top.min(page.lines.len());
                continue;
            }
            c if c.starts_with("g ") => match webcore::parse_user_url(&c[2..]) {
                Some(u) => go(u, &mut page, &mut top, &mut history),
                None => out("That is not an address.", cp),
            },
            c => match c.parse::<usize>() {
                Ok(n) if n >= 1 && n <= page.links.len() => {
                    let u = page.links[n - 1].clone();
                    go(u, &mut page, &mut top, &mut history);
                }
                _ => out("Unknown command.", cp),
            },
        }
        if top >= page.lines.len() && !page.lines.is_empty() && cmd.is_empty() {
            out("-- end of page --", cp);
            top = page.lines.len().saturating_sub(screen);
        }
    }
}
