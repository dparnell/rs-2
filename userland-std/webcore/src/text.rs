//! A document as plain text for a character screen (the text-mode
//! browser): paragraphs wrapped to a width, headings underlined, lists with
//! bullets or numbers, preformatted text kept, links numbered `[n]`, images
//! by their alternative text, form fields as brackets.

use url::Url;

use crate::dom::{Dom, NodeData, NodeId};

pub struct TextPage {
    pub lines: Vec<String>,
    /// Link targets: `[n]` in the text is `links[n - 1]`.
    pub links: Vec<Url>,
    pub title: Option<String>,
}

struct R<'a> {
    dom: &'a Dom,
    base: Url,
    width: usize,
    lines: Vec<String>,
    links: Vec<Url>,
    /// Words of the paragraph being built (with their joining space).
    para: String,
    indent: usize,
    /// First-line prefix (list bullets) for the paragraph being built.
    bullet: String,
    pre: usize,
    blank_pending: bool,
}

pub fn render(dom: &Dom, base: &Url, width: usize) -> TextPage {
    let mut r = R {
        dom,
        base: base.clone(),
        width: width.max(20),
        lines: Vec::new(),
        links: Vec::new(),
        para: String::new(),
        indent: 0,
        bullet: String::new(),
        pre: 0,
        blank_pending: false,
    };
    // <base href>.
    if let Some(b) = dom.find("base").and_then(|b| dom.element(b)).and_then(|e| e.attr("href")) {
        if let Ok(u) = base.join(b) {
            r.base = u;
        }
    }
    let body = dom.find("body").unwrap_or(Dom::DOCUMENT);
    r.node(body, 0);
    r.flush();
    while r.lines.last().is_some_and(|l| l.trim().is_empty()) {
        r.lines.pop();
    }
    TextPage { lines: r.lines, links: r.links, title: dom.title() }
}

const BLOCKS: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "dd", "details", "dialog", "dir", "div", "dl", "dt", "fieldset",
    "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "li", "main",
    "menu", "nav", "ol", "p", "pre", "section", "summary", "table", "tr", "ul", "caption", "tbody", "thead", "tfoot",
    "body", "html", "legend", "option",
];
const SKIP: &[&str] = &["head", "script", "style", "template", "svg", "math", "iframe", "object", "noembed", "canvas", "datalist"];

impl R<'_> {
    fn word(&mut self, s: &str) {
        self.para.push_str(s);
    }

    fn text(&mut self, t: &str) {
        if self.pre > 0 {
            for (i, l) in t.split('\n').enumerate() {
                if i > 0 {
                    let p = std::mem::take(&mut self.para);
                    self.push_line(format!("{}{}", " ".repeat(self.indent), p));
                }
                self.para.push_str(&l.replace('\t', "    "));
            }
            return;
        }
        let mut first = true;
        for w in t.split(|c: char| c.is_ascii_whitespace()) {
            if !first || t.starts_with(|c: char| c.is_ascii_whitespace()) {
                if !self.para.is_empty() && !self.para.ends_with(' ') {
                    self.para.push(' ');
                }
            }
            first = false;
            self.para.push_str(w);
        }
        if t.ends_with(|c: char| c.is_ascii_whitespace()) && !self.para.is_empty() && !self.para.ends_with(' ') {
            self.para.push(' ');
        }
    }

    fn push_line(&mut self, l: String) {
        if self.blank_pending {
            if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                self.lines.push(String::new());
            }
            self.blank_pending = false;
        }
        self.lines.push(l.trim_end().to_string());
    }

    /// End the paragraph: wrap it into lines.
    fn flush(&mut self) {
        let para = std::mem::take(&mut self.para);
        let bullet = std::mem::take(&mut self.bullet);
        if self.pre > 0 {
            if !para.is_empty() {
                self.push_line(format!("{}{}", " ".repeat(self.indent), para));
            }
            return;
        }
        let words: Vec<&str> = para.split(' ').filter(|w| !w.is_empty()).collect();
        if words.is_empty() {
            self.bullet = bullet;
            return;
        }
        let ind = self.indent.min(self.width / 2);
        let avail = self.width.saturating_sub(ind).max(10);
        let mut line = String::new();
        let mut first = true;
        let lead = |first: bool| if first && !bullet.is_empty() { format!("{}{}", " ".repeat(ind.saturating_sub(bullet.chars().count())), bullet) } else { " ".repeat(ind) };
        for w in words {
            let wl = w.chars().count();
            let ll = line.chars().count();
            if ll > 0 && ll + 1 + wl > avail {
                let l = format!("{}{}", lead(first), line);
                self.push_line(l);
                first = false;
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            // Words longer than a line (URLs) are cut.
            let mut w = w.to_string();
            while w.chars().count() > avail {
                let head: String = w.chars().take(avail).collect();
                let tail: String = w.chars().skip(avail).collect();
                if !line.is_empty() {
                    let l = format!("{}{}", lead(first), line.trim_end());
                    self.push_line(l);
                    first = false;
                    line.clear();
                }
                let l = format!("{}{}", lead(first), head);
                self.push_line(l);
                first = false;
                w = tail;
            }
            line.push_str(&w);
        }
        if !line.is_empty() {
            let l = format!("{}{}", lead(first), line);
            self.push_line(l);
        }
    }

    fn blank(&mut self) {
        self.flush();
        self.blank_pending = true;
    }

    fn node(&mut self, id: NodeId, depth: usize) {
        if depth > 200 {
            return;
        }
        let dom = self.dom;
        match &dom.nodes[id].data {
            NodeData::Text(t) => self.text(t),
            NodeData::Element(e) => {
                let name: &str = e.local();
                if SKIP.contains(&name) {
                    return;
                }
                if e.attr("hidden").is_some() || e.attr("aria-hidden") == Some("true") && name != "body" {
                    return;
                }
                if let Some(style) = e.attr("style") {
                    let s = style.replace(' ', "").to_ascii_lowercase();
                    if s.contains("display:none") || s.contains("visibility:hidden") {
                        return;
                    }
                }
                let block = BLOCKS.contains(&name);
                match name {
                    "br" => {
                        let ind = self.indent;
                        let p = std::mem::take(&mut self.para);
                        if self.pre > 0 {
                            self.push_line(format!("{}{}", " ".repeat(ind), p));
                        } else if p.trim().is_empty() {
                            // <br><br>: an empty line.
                            self.push_line(String::new());
                        } else {
                            self.para = p;
                            self.flush();
                        }
                        return;
                    }
                    "hr" => {
                        self.flush();
                        let w = self.width.saturating_sub(self.indent);
                        self.push_line(format!("{}{}", " ".repeat(self.indent), "-".repeat(w)));
                        return;
                    }
                    "img" => {
                        let alt = e.attr("alt").map(str::trim).unwrap_or("");
                        if !alt.is_empty() {
                            self.word(&format!("[{alt}]"));
                        }
                        return;
                    }
                    "input" => {
                        let ty = e.attr("type").unwrap_or("text").to_ascii_lowercase();
                        let v = e.attr("value").unwrap_or("");
                        let s = match ty.as_str() {
                            "hidden" => String::new(),
                            "submit" | "button" | "reset" => format!("[{}]", if v.is_empty() { "Submit" } else { v }),
                            "checkbox" => (if e.attr("checked").is_some() { "[x]" } else { "[ ]" }).to_string(),
                            "radio" => (if e.attr("checked").is_some() { "(*)" } else { "( )" }).to_string(),
                            _ => {
                                let ph = e.attr("placeholder").unwrap_or("");
                                let t = if v.is_empty() { ph } else { v };
                                format!("[{:_<12}]", t.chars().take(30).collect::<String>())
                            }
                        };
                        if !s.is_empty() {
                            if !self.para.is_empty() && !self.para.ends_with(' ') {
                                self.para.push(' ');
                            }
                            self.word(&s);
                            self.para.push(' ');
                        }
                        return;
                    }
                    "textarea" => {
                        self.word("[");
                        self.text(&dom.text_content(id));
                        self.word("]");
                        return;
                    }
                    "select" => {
                        let opts: Vec<NodeId> = dom.descendants(id).filter(|&n| dom.element(n).is_some_and(|e| e.is("option"))).collect();
                        let sel = opts.iter().find(|&&o| dom.element(o).unwrap().attr("selected").is_some()).or(opts.first());
                        let t = sel.map(|&o| dom.text_content(o)).unwrap_or_default();
                        self.word(&format!("[{} v]", t.trim()));
                        return;
                    }
                    "button" => {
                        self.word("[");
                        for &c in &dom.nodes[id].children {
                            self.node(c, depth + 1);
                        }
                        self.word("]");
                        return;
                    }
                    _ => {}
                }
                let heading = matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
                if heading || matches!(name, "p" | "ul" | "ol" | "dl" | "table" | "pre" | "blockquote" | "figure" | "form" | "fieldset")
                {
                    self.blank();
                } else if block {
                    self.flush();
                }
                let old_indent = self.indent;
                match name {
                    "blockquote" | "dd" | "figure" => self.indent += 4,
                    "ul" | "ol" | "dir" | "menu" => self.indent += 3,
                    "pre" => self.pre += 1,
                    "li" => {
                        let parent = dom.parent_element(id).and_then(|p| dom.element(p));
                        if parent.is_some_and(|p| p.is("ol")) {
                            let p = dom.nodes[id].parent.unwrap();
                            let start: usize = parent.and_then(|p| p.attr("start")).and_then(|s| s.parse().ok()).unwrap_or(1);
                            let n = dom.nodes[p].children.iter().take_while(|&&c| c != id).filter(|&&c| dom.element(c).is_some_and(|e| e.is("li"))).count();
                            self.bullet = format!("{}. ", start + n);
                        } else {
                            let level = (self.indent / 3).max(1);
                            self.bullet = match level % 3 {
                                1 => "* ",
                                2 => "o ",
                                _ => "+ ",
                            }
                            .to_string();
                        }
                    }
                    "td" | "th" => {
                        if !self.para.is_empty() && !self.para.ends_with(' ') {
                            self.para.push(' ');
                        }
                    }
                    _ => {}
                }
                let start_line = self.lines.len();
                let start_para = self.para.len();
                for &c in &dom.nodes[id].children {
                    self.node(c, depth + 1);
                }
                match name {
                    "a" => {
                        if let Some(href) = e.attr("href") {
                            let href = href.trim();
                            if !href.starts_with("javascript:") && !href.is_empty() {
                                if let Ok(u) = self.base.join(href) {
                                    let n = match self.links.iter().position(|l| *l == u) {
                                        Some(i) => i + 1,
                                        None => {
                                            self.links.push(u);
                                            self.links.len()
                                        }
                                    };
                                    let trimmed = self.para.trim_end().len();
                                    let had_space = trimmed < self.para.len();
                                    self.para.truncate(trimmed);
                                    self.para.push_str(&format!("[{n}]"));
                                    if had_space {
                                        self.para.push(' ');
                                    }
                                }
                            }
                        }
                    }
                    "td" | "th" => {
                        if !self.para.ends_with(' ') {
                            self.para.push(' ');
                        }
                        self.para.push_str("  ");
                    }
                    "q" => {}
                    _ => {}
                }
                if heading {
                    if name == "h1" || name == "h2" {
                        let text = std::mem::take(&mut self.para);
                        let text = if name == "h1" { text.to_uppercase() } else { text };
                        self.para = text;
                        self.flush();
                        if let Some(last) = self.lines.last() {
                            let n = last.trim().chars().count();
                            let ind = last.len() - last.trim_start().len();
                            if n > 0 && self.lines.len() > start_line {
                                let u = if name == "h1" { "=" } else { "-" };
                                self.lines.push(format!("{}{}", " ".repeat(ind), u.repeat(n.min(self.width))));
                            }
                        }
                    }
                    self.blank();
                } else if matches!(name, "p" | "ul" | "ol" | "dl" | "table" | "pre" | "blockquote" | "figure" | "form" | "fieldset") {
                    self.blank();
                } else if block {
                    self.flush();
                }
                let _ = start_para;
                match name {
                    "pre" => self.pre -= 1,
                    _ => {}
                }
                self.indent = old_indent;
            }
            NodeData::Document => {
                for &c in &dom.nodes[id].children {
                    self.node(c, depth + 1);
                }
            }
            _ => {}
        }
    }
}
