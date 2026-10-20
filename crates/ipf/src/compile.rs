//! A compiler for IPF source text: help libraries (`.HLP`) and books
//! (`.INF`) for rs2's own programs, written in the layout [`Doc`] reads
//! and IBM's help manager expects (see the crate documentation).
//!
//! It takes the subset of the IPF tag language rs2's help files use:
//!
//! - `:userdoc.` / `:euserdoc.`, `:title.` (rest of the line), `.*`
//!   comment lines;
//! - `:h1.`-`:h6.` with `res=`, `id=`, `hide`: a panel (title: the rest of
//!   the line); `:i1.` (rest of the line): an index entry for the panel;
//! - `:p.`, `:note.`, `:ul.`/`:ol.`/`:sl.` with `:li.` and their ends,
//!   `:dl.` (`tsize=`) with `:dt.`/`:dd.` and `:edl.`;
//! - `:hp1.`-`:hp7.` / `:ehp1.`-`:ehp7.` (italic, bold, underlined and
//!   their mixes; `:hp4.` is bold);
//! - `:link reftype=hd res=N.` or `refid=ID.` ... `:elink.`;
//! - symbols such as `&colon.`, `&amp.`, `&period.`.
//!
//! Text is in the file's code page (437); words are runs of letters and
//! digits, every other character is a word of its own, and words written
//! together in the source are joined with autospacing off, as IBM's
//! compiler does.
//!
//! [`Doc`]: crate::Doc

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// A compile error at a source line (1-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub line: usize,
    pub msg: String,
}

impl core::fmt::Display for CompileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

/// Local dictionary words a cell may have (header byte 0x52).
const LOCAL_WORDS: usize = 0xf5;

const PARA: u8 = 0xfa;
const SPACING: u8 = 0xfc;
const BREAK: u8 = 0xfd;

/// Where a link leads before panels are numbered.
#[derive(Debug, Clone)]
enum Ref {
    Res(u16),
    Id(Vec<u8>),
}

#[derive(Debug, Clone)]
enum Tok {
    /// A word; `glued`: never followed by a space (bullets, numbers).
    Word(Vec<u8>, bool),
    /// White space in the source.
    Ws,
    /// A paragraph end or line break (autospacing back on).
    Block(u8),
    /// An escape; `brk`: it ends a run of words (margins).
    Esc(Vec<u8>, bool),
    Link(Ref, usize),
}

fn esc(ty: u8, data: &[u8]) -> Vec<u8> {
    let mut v = vec![0xff, data.len() as u8 + 2, ty];
    v.extend_from_slice(data);
    v
}

struct Panel {
    level: u8,
    hidden: bool,
    res: Option<u16>,
    id: Option<Vec<u8>>,
    title: Vec<u8>,
    toks: Vec<Tok>,
}

enum List {
    /// `:ul.`/`:ol.`/`:sl.`: kind, item number, bullet and text margins,
    /// the margin to return to.
    Items { kind: u8, n: u32, bullet: u8, text: u8, base: u8 },
    Defs { first: bool, tsize: u8, base: u8 },
}

struct Parser {
    line: usize,
    title: Vec<u8>,
    panels: Vec<Panel>,
    index: Vec<(Vec<u8>, usize)>,
    lists: Vec<List>,
    /// Margin of the text outside the current list item.
    margin: u8,
    /// The margin a `:note.` returns to.
    note: Option<u8>,
}

fn err<T>(line: usize, msg: impl Into<String>) -> Result<T, CompileError> {
    Err(CompileError { line, msg: msg.into() })
}

/// `&name.` symbols.
fn symbol(name: &[u8]) -> Option<u8> {
    Some(match name {
        b"amp" => b'&',
        b"colon" => b':',
        b"period" | b"per" => b'.',
        b"comma" => b',',
        b"semi" => b';',
        b"lpar" => b'(',
        b"rpar" => b')',
        b"lbrk" => b'[',
        b"rbrk" => b']',
        b"lbrc" => b'{',
        b"rbrc" => b'}',
        b"apos" => b'\'',
        b"cdq" | b"odq" | b"dq" => b'"',
        b"slash" => b'/',
        b"bsl" => b'\\',
        b"us" => b'_',
        b"minus" | b"dash" => b'-',
        b"plus" => b'+',
        b"eq" | b"equals" => b'=',
        b"lt" => b'<',
        b"gt" => b'>',
        b"vbar" => b'|',
        b"tilde" => b'~',
        b"caret" => b'^',
        b"asterisk" => b'*',
        b"percent" => b'%',
        b"darrow" => 0x19,
        b"uarrow" => 0x18,
        b"rarrow" => 0x1a,
        b"larrow" => 0x1b,
        b"bullet" => 0x07,
        _ => return None,
    })
}

fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c >= 0x80
}

/// Attributes of a tag: `name=value`, `name='value'` or `name`.
fn attrs(s: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut v = Vec::new();
    let mut p = 0;
    while p < s.len() {
        while p < s.len() && s[p] == b' ' {
            p += 1;
        }
        let start = p;
        while p < s.len() && s[p] != b'=' && s[p] != b' ' {
            p += 1;
        }
        let name = s[start..p].to_ascii_lowercase();
        let mut value = Vec::new();
        if p < s.len() && s[p] == b'=' {
            p += 1;
            if p < s.len() && (s[p] == b'\'' || s[p] == b'"') {
                let q = s[p];
                p += 1;
                let vs = p;
                while p < s.len() && s[p] != q {
                    p += 1;
                }
                value = s[vs..p].to_vec();
                p += 1;
            } else {
                let vs = p;
                while p < s.len() && s[p] != b' ' {
                    p += 1;
                }
                value = s[vs..p].to_vec();
            }
        }
        if !name.is_empty() {
            v.push((name, value));
        }
    }
    v
}

fn number(line: usize, v: &[u8]) -> Result<u16, CompileError> {
    let s = core::str::from_utf8(v).unwrap_or("");
    let n = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(h, 16)
    } else {
        s.parse()
    };
    n.or_else(|_| err(line, format!("bad number {s:?}")))
}

impl Parser {
    fn panel(&mut self) -> Result<&mut Panel, CompileError> {
        let line = self.line;
        match self.panels.last_mut() {
            Some(p) => Ok(p),
            None => err(line, "text before the first heading"),
        }
    }

    fn push(&mut self, t: Tok) -> Result<(), CompileError> {
        self.panel()?.toks.push(t);
        Ok(())
    }

    fn margin_esc(&mut self, n: u8) -> Result<(), CompileError> {
        self.push(Tok::Esc(esc(0x02, &[n]), true))
    }

    fn text(&mut self, s: &[u8]) -> Result<(), CompileError> {
        let mut p = 0;
        while p < s.len() {
            let c = s[p];
            if c == b' ' || c == b'\t' {
                self.push(Tok::Ws)?;
                p += 1;
            } else if is_word_char(c) {
                let start = p;
                while p < s.len() && is_word_char(s[p]) {
                    p += 1;
                }
                self.push(Tok::Word(s[start..p].to_vec(), false))?;
            } else {
                self.push(Tok::Word(vec![c], false))?;
                p += 1;
            }
        }
        Ok(())
    }

    /// End a `:note.`'s hanging indent.
    fn end_note(&mut self) -> Result<(), CompileError> {
        if let Some(m) = self.note.take() {
            self.margin_esc(m)?;
        }
        Ok(())
    }

    fn para(&mut self) -> Result<(), CompileError> {
        self.end_note()?;
        self.push(Tok::Block(PARA))
    }

    fn tag(&mut self, name: &[u8], args: &[u8], rest: &[u8]) -> Result<bool, CompileError> {
        let line = self.line;
        let a = attrs(args);
        let get = |k: &[u8]| a.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        match name {
            b"userdoc" | b"euserdoc" | b"docprof" => {}
            b"title" => {
                self.title = rest.to_vec();
                return Ok(true);
            }
            b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                if !self.lists.is_empty() {
                    return err(line, "heading inside a list");
                }
                self.note = None;
                self.margin = 1;
                let res = get(b"res").map(|v| number(line, &v)).transpose()?;
                if let Some(r) = res {
                    if self.panels.iter().any(|p| p.res == Some(r)) {
                        return err(line, format!("res={r} used twice"));
                    }
                }
                self.panels.push(Panel {
                    level: name[1] - b'0',
                    hidden: a.iter().any(|(n, _)| n == b"hide"),
                    res,
                    id: get(b"id").map(|v| v.to_ascii_lowercase()),
                    title: rest.to_vec(),
                    toks: Vec::new(),
                });
                return Ok(true);
            }
            b"i1" => {
                let n = self.panels.len();
                if n == 0 {
                    return err(line, ":i1. before the first heading");
                }
                self.index.push((rest.to_vec(), n - 1));
                return Ok(true);
            }
            b"p" => self.para()?,
            b"note" => {
                self.para()?;
                self.push(Tok::Esc(esc(0x04, &[2]), false))?;
                self.push(Tok::Word(b"Note".to_vec(), false))?;
                self.push(Tok::Word(b":".to_vec(), false))?;
                self.push(Tok::Esc(esc(0x04, &[0]), false))?;
                self.push(Tok::Ws)?;
                self.push(Tok::Esc(esc(0x1c, &[]), false))?;
                self.note = Some(self.margin);
            }
            b"ul" | b"ol" | b"sl" => {
                self.end_note()?;
                let base = self.margin;
                let (bullet, text) = match name {
                    b"ul" => (base + 2, base + 5),
                    b"ol" => (base + 1, base + 5),
                    _ => (base + 2, base + 2),
                };
                self.lists.push(List::Items { kind: name[0], n: 0, bullet, text, base });
                self.margin_esc(bullet)?;
                self.push(Tok::Block(PARA))?;
            }
            b"li" => {
                let Some(List::Items { kind, n, bullet, text, .. }) = self.lists.last_mut() else {
                    return err(line, ":li. outside a list");
                };
                let (kind, bullet, text, first) = (*kind, *bullet, *text, *n == 0);
                *n += 1;
                let num = *n;
                self.end_note()?;
                if !first {
                    self.margin_esc(bullet)?;
                    self.push(Tok::Block(BREAK))?;
                }
                match kind {
                    b'u' => self.push(Tok::Word(vec![0x07], true))?,
                    b'o' => {
                        self.push(Tok::Word(format!("{num}").into_bytes(), true))?;
                        self.push(Tok::Word(b".".to_vec(), true))?;
                    }
                    _ => {}
                }
                self.margin_esc(text)?;
                self.margin = text;
            }
            b"eul" | b"eol" | b"esl" => {
                let Some(List::Items { kind, base, .. }) = self.lists.pop() else {
                    return err(line, "list end without a list");
                };
                if kind != name[1] {
                    return err(line, "list end does not match its list");
                }
                self.end_note()?;
                self.margin = base;
                self.margin_esc(base)?;
                self.push(Tok::Block(BREAK))?;
            }
            b"dl" => {
                self.end_note()?;
                let tsize = get(b"tsize").map(|v| number(line, &v)).transpose()?.unwrap_or(10) as u8;
                self.lists.push(List::Defs { first: true, tsize, base: self.margin });
                self.push(Tok::Block(PARA))?;
            }
            b"dt" => {
                let Some(List::Defs { first, base, .. }) = self.lists.last_mut() else {
                    return err(line, ":dt. outside :dl.");
                };
                let (was_first, base) = (*first, *base);
                *first = false;
                self.end_note()?;
                self.margin = base;
                self.margin_esc(base)?;
                if !was_first {
                    self.push(Tok::Block(BREAK))?;
                }
            }
            b"dd" => {
                let Some(&List::Defs { tsize, base, .. }) = self.lists.last() else {
                    return err(line, ":dd. outside :dl.");
                };
                self.margin = base + tsize;
                self.push(Tok::Esc(esc(0x12, &[base + tsize]), true))?;
            }
            b"edl" => {
                let Some(List::Defs { base, .. }) = self.lists.pop() else {
                    return err(line, ":edl. without :dl.");
                };
                self.end_note()?;
                self.margin = base;
                self.margin_esc(base)?;
                self.push(Tok::Block(BREAK))?;
            }
            b"hp1" | b"hp2" | b"hp3" | b"hp4" | b"hp5" | b"hp6" | b"hp7" => {
                let style = [1, 2, 3, 2, 4, 5, 6][(name[2] - b'1') as usize];
                self.push(Tok::Esc(esc(0x04, &[style]), false))?;
            }
            b"ehp1" | b"ehp2" | b"ehp3" | b"ehp4" | b"ehp5" | b"ehp6" | b"ehp7" => {
                self.push(Tok::Esc(esc(0x04, &[0]), false))?;
            }
            b"link" => {
                if get(b"reftype").as_deref() != Some(b"hd") {
                    return err(line, ":link. needs reftype=hd");
                }
                let r = match (get(b"res"), get(b"refid")) {
                    (Some(r), _) => Ref::Res(number(line, &r)?),
                    (None, Some(id)) => Ref::Id(id.to_ascii_lowercase()),
                    _ => return err(line, ":link. needs res= or refid="),
                };
                self.push(Tok::Link(r, line))?;
            }
            b"elink" => self.push(Tok::Esc(esc(0x08, &[]), false))?,
            _ => return err(line, format!("unknown tag :{}.", crate::latin1(name))),
        }
        Ok(false)
    }

    fn line(&mut self, l: &[u8]) -> Result<(), CompileError> {
        if l.starts_with(b".*") {
            return Ok(());
        }
        if l.starts_with(b".") {
            return err(self.line, "control words are not supported");
        }
        let mut text = Vec::new();
        let mut p = 0;
        while p < l.len() {
            let c = l[p];
            if c == b':' && l.get(p + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
                let ns = p + 1;
                let mut q = ns;
                while q < l.len() && l[q].is_ascii_alphanumeric() {
                    q += 1;
                }
                let name = l[ns..q].to_ascii_lowercase();
                // Attributes up to the '.' that ends the tag (not inside quotes).
                let mut e = q;
                let mut quote = None;
                while e < l.len() {
                    match (quote, l[e]) {
                        (None, b'.') => break,
                        (None, b'\'' | b'"') => quote = Some(l[e]),
                        (Some(qc), c) if c == qc => quote = None,
                        _ => {}
                    }
                    e += 1;
                }
                if e >= l.len() {
                    return err(self.line, "tag without its closing '.'");
                }
                self.text(&core::mem::take(&mut text))?;
                let rest = l[e + 1..].trim_ascii();
                if self.tag(&name, &l[q..e], rest)? {
                    return Ok(());
                }
                p = e + 1;
            } else if c == b'&' {
                let e = l[p + 1..].iter().position(|&c| c == b'.').map(|e| p + 1 + e);
                match e.and_then(|e| symbol(&l[p + 1..e]).map(|s| (e, s))) {
                    Some((e, s)) => {
                        text.push(s);
                        p = e + 1;
                    }
                    None => return err(self.line, "unknown symbol (write & as &amp.)"),
                }
            } else {
                text.push(c);
                p += 1;
            }
        }
        self.text(&text)?;
        if !self.panels.is_empty() {
            self.push(Tok::Ws)?;
        }
        Ok(())
    }
}

/// A cell being filled.
#[derive(Default)]
struct CellBuf {
    text: Vec<u8>,
    local: Vec<u16>,
    map: BTreeMap<u16, u8>,
}

/// Case-insensitive dictionary order (then byte order, so equal words
/// that differ in case are kept apart).
fn dict_key(w: &[u8]) -> (Vec<u8>, Vec<u8>) {
    (w.to_ascii_uppercase(), w.to_vec())
}

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn set16(v: &mut [u8], o: usize, x: u16) {
    v[o..o + 2].copy_from_slice(&x.to_le_bytes());
}

fn set32(v: &mut [u8], o: usize, x: u32) {
    v[o..o + 4].copy_from_slice(&x.to_le_bytes());
}

/// Country information: the country and code page, then the characters
/// that make up words (letters and digits) and those that are graphics
/// (control characters and the upper half), as 256-bit sets.
fn nls(codepage: u16) -> Vec<u8> {
    let mut v = vec![0x0c, 0, 0, 0, 0, 1, 1, 0];
    put16(&mut v, codepage);
    put16(&mut v, 0);
    let set = |v: &mut Vec<u8>, ty: u16, f: &dyn Fn(u8) -> bool| {
        put16(v, 0x24);
        put16(v, ty);
        for byte in 0..32u8 {
            let mut b = 0u8;
            for bit in 0..8u8 {
                if f(byte * 8 + bit) {
                    b |= 0x80 >> bit;
                }
            }
            v.push(b);
        }
    };
    set(&mut v, 1, &|c| c.is_ascii_alphanumeric());
    set(&mut v, 2, &|c| (1..0x20).contains(&c) || c >= 0x80);
    v
}

/// Compile IPF source (code page 437 text) into a help library
/// (`help`) or a book.
pub fn compile(src: &[u8], help: bool) -> Result<Vec<u8>, CompileError> {
    let mut ps = Parser { line: 0, title: Vec::new(), panels: Vec::new(), index: Vec::new(), lists: Vec::new(), margin: 1, note: None };
    for (i, l) in src.split(|&c| c == b'\n').enumerate() {
        ps.line = i + 1;
        let l = l.strip_suffix(b"\r").unwrap_or(l);
        if l.first() == Some(&0x1a) {
            break;
        }
        ps.line(l)?;
    }
    if !ps.lists.is_empty() {
        return err(ps.line, "a list is not ended");
    }
    let panels = ps.panels;
    if panels.is_empty() {
        return err(ps.line, "no panels");
    }

    // Link targets.
    let find = |r: &Ref, line: usize| -> Result<u16, CompileError> {
        let i = match r {
            Ref::Res(n) => panels.iter().position(|p| p.res == Some(*n)),
            Ref::Id(id) => panels.iter().position(|p| p.id.as_ref() == Some(id)),
        };
        match i {
            Some(i) => Ok(i as u16),
            None => err(line, format!("link to a panel that is not there: {r:?}")),
        }
    };

    // The dictionary: every word of the text.
    let mut words = BTreeSet::new();
    for p in &panels {
        for t in &p.toks {
            if let Tok::Word(w, _) = t {
                if w.len() > 254 {
                    return err(0, "word too long");
                }
                words.insert(dict_key(w));
            }
        }
    }
    let dict: Vec<Vec<u8>> = words.into_iter().map(|(_, w)| w).collect();
    if dict.len() > 0xffff {
        return err(0, "too many words");
    }
    let word_no: BTreeMap<&[u8], u16> = dict.iter().enumerate().map(|(i, w)| (w.as_slice(), i as u16)).collect();

    // Cells: each panel's text, split when a cell's local dictionary
    // is full. `panel_cells[i]`: the cells of panel i.
    let mut cells: Vec<CellBuf> = Vec::new();
    let mut panel_cells: Vec<Vec<u16>> = Vec::new();
    let mut search: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); dict.len()];
    for (pi, p) in panels.iter().enumerate() {
        let mut mine = vec![cells.len() as u16];
        cells.push(CellBuf::default());
        let mut spacing = true;
        for (ti, t) in p.toks.iter().enumerate() {
            match t {
                Tok::Word(w, glued) => {
                    let n = word_no[w.as_slice()];
                    search[n as usize].insert(pi as u16);
                    let cur = cells.last_mut().unwrap();
                    if !cur.map.contains_key(&n) && cur.local.len() == LOCAL_WORDS {
                        mine.push(cells.len() as u16);
                        cells.push(CellBuf::default());
                        spacing = true;
                    }
                    let cur = cells.last_mut().unwrap();
                    let local = match cur.map.get(&n) {
                        Some(&l) => l,
                        None => {
                            let l = cur.local.len() as u8;
                            cur.local.push(n);
                            cur.map.insert(n, l);
                            l
                        }
                    };
                    // A space follows unless the next word comes straight after.
                    let next = p.toks[ti + 1..].iter().find(|t| !matches!(t, Tok::Esc(_, false) | Tok::Link(..)));
                    let space = !glued && !matches!(next, Some(Tok::Word(..)));
                    if space != spacing {
                        cur.text.push(SPACING);
                        spacing = space;
                    }
                    cur.text.push(local);
                }
                Tok::Ws => {}
                Tok::Block(b) => {
                    cells.last_mut().unwrap().text.push(*b);
                    spacing = true;
                }
                Tok::Esc(e, _) => cells.last_mut().unwrap().text.extend_from_slice(e),
                Tok::Link(r, line) => {
                    let toc = find(r, *line)?;
                    cells.last_mut().unwrap().text.extend_from_slice(&esc(0x06, &toc.to_le_bytes()));
                }
            }
        }
        panel_cells.push(mine);
    }
    if panels.len() > 0x7fff || cells.len() > 0xffff {
        return err(0, "too many panels");
    }

    // ---- The file ----
    let mut f = vec![0u8; 0x9b];
    f[..3].copy_from_slice(b"HSP");
    f[3] = if help { crate::KIND_HELP } else { crate::KIND_BOOK };
    set16(&mut f, 0x04, 0x9b);
    f[6] = 2;
    f[7] = 2;
    f[0x52] = LOCAL_WORDS as u8;
    let title = &ps.title[..ps.title.len().min(47)];
    f[0x6b..0x6b + title.len()].copy_from_slice(title);

    // Resource ids, sorted, then their panels.
    let mut res: Vec<(u16, u16)> = panels.iter().enumerate().filter_map(|(i, p)| p.res.map(|r| (r, i as u16))).collect();
    res.sort();
    set16(&mut f, 0x16, res.len() as u16);
    let x = f.len() as u32;
    set32(&mut f, 0x18, x);
    for &(r, _) in &res {
        put16(&mut f, r);
    }
    for &(_, i) in &res {
        put16(&mut f, i);
    }

    // The table of contents.
    let toc_start = f.len();
    let mut toc_offsets = Vec::new();
    for (i, p) in panels.iter().enumerate() {
        toc_offsets.push(f.len() as u32);
        let kids = panels.get(i + 1).is_some_and(|n| n.level > p.level);
        let mut flags = p.level;
        if kids {
            flags |= 0x80;
        }
        if p.hidden {
            flags |= 0x40;
        }
        let cl = &panel_cells[i];
        let t = &p.title[..p.title.len().min(255 - 3 - cl.len() * 2)];
        f.push((3 + cl.len() * 2 + t.len()) as u8);
        f.push(flags);
        f.push(cl.len() as u8);
        for &c in cl {
            put16(&mut f, c);
        }
        f.extend_from_slice(t);
    }
    set16(&mut f, 0x08, panels.len() as u16);
    set32(&mut f, 0x0a, toc_start as u32);
    let x = (f.len() - toc_start) as u32;
    set32(&mut f, 0x0e, x);
    let x = f.len() as u32;
    set32(&mut f, 0x12, x);
    for o in toc_offsets {
        put32(&mut f, o);
    }

    // The index, sorted.
    let mut index = ps.index;
    index.sort_by(|a, b| dict_key(&a.0).cmp(&dict_key(&b.0)));
    let index_start = f.len();
    for (text, panel) in &index {
        let t = &text[..text.len().min(255)];
        f.push(t.len() as u8);
        f.push(0x01);
        f.push(0);
        put16(&mut f, *panel as u16);
        f.extend_from_slice(t);
    }
    set16(&mut f, 0x22, index.len() as u16);
    set32(&mut f, 0x24, index_start as u32);
    let x = (f.len() - index_start) as u32;
    set32(&mut f, 0x28, x);

    // Country information.
    let n = nls(437);
    let x = f.len() as u32;
    set32(&mut f, 0x53, x);
    set32(&mut f, 0x57, n.len() as u32);
    f.extend_from_slice(&n);

    // Extended header parts: no databases, the default font, empty
    // control data.
    let db_start = f.len();
    let font_start = f.len();
    let mut face = [0u8; 33];
    face[..19].copy_from_slice(b"System Proportional");
    f.extend_from_slice(&face);
    put16(&mut f, 0);
    put16(&mut f, 0);
    put16(&mut f, 437);
    let ctrl_start = f.len();
    f.extend_from_slice(&[0; 8]);

    // The dictionary.
    let dict_start = f.len();
    for w in &dict {
        f.push(w.len() as u8 + 1);
        f.extend_from_slice(w);
    }
    let x = (f.len() - dict_start) as u32;
    set32(&mut f, 0x44, x);
    set16(&mut f, 0x48, dict.len() as u16);
    set32(&mut f, 0x4a, dict_start as u32);

    // The cells.
    let mut cell_offsets = Vec::new();
    for c in &cells {
        let o = f.len();
        cell_offsets.push(o as u32);
        f.push(0);
        put32(&mut f, (o + 8 + c.text.len()) as u32);
        f.push(c.local.len() as u8);
        put16(&mut f, c.text.len() as u16);
        f.extend_from_slice(&c.text);
        for &w in &c.local {
            put16(&mut f, w);
        }
    }
    set16(&mut f, 0x3e, cells.len() as u16);
    let x = f.len() as u32;
    set32(&mut f, 0x40, x);
    for o in cell_offsets {
        put32(&mut f, o);
    }

    // No panel names.
    let x = f.len() as u32;
    set32(&mut f, 0x1e, x);

    // Full-text search: for each word, the panels it is in.
    let records: Vec<Vec<u8>> = search
        .iter()
        .map(|s| {
            let mut r = Vec::new();
            if s.is_empty() {
                r.push(0);
            } else {
                r.push(3);
                for &t in s {
                    put16(&mut r, t);
                }
            }
            r
        })
        .collect();
    let wide = records.iter().any(|r| r.len() + 1 > 0xff);
    let search_start = f.len();
    for r in &records {
        if wide {
            put16(&mut f, r.len() as u16 + 2);
        } else {
            f.push(r.len() as u8 + 1);
        }
        f.extend_from_slice(r);
    }
    set32(&mut f, 0x36, search_start as u32 | if wide { 0x8000_0000 } else { 0 });
    let x = (f.len() - search_start) as u32;
    set32(&mut f, 0x3a, x);

    // The extended header.
    let ext = f.len();
    f.extend_from_slice(&[0; 0x40]);
    set16(&mut f, ext, 1);
    set32(&mut f, ext + 2, font_start as u32);
    set32(&mut f, ext + 8, db_start as u32);
    set32(&mut f, ext + 0x28, ctrl_start as u32);
    set32(&mut f, ext + 0x2c, 8);
    set32(&mut f, 0x5b, ext as u32);
    Ok(f)
}
