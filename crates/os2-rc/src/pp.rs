//! The preprocessor: `#define` (object- and function-like macros with `#`
//! and `##`), `#undef`, `#include`, `#if`/`#ifdef`/`#ifndef`/`#elif`/
//! `#else`/`#endif` with `defined`, and RC's own `RCINCLUDE`.
//!
//! As IBM's RC, a file brought in by `#include` contributes only its
//! directives (a C header's declarations are skipped); `RCINCLUDE` takes a
//! whole script. The output is the script's lines after macro
//! expansion, each with the file and line it came from.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::{Diag, Files};

pub struct Line {
    pub text: Vec<u8>,
    pub file: u16,
    pub line: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    Id,
    Num,
    Str,
    Ws,
    Punct,
}

#[derive(Clone, Debug)]
struct T {
    k: K,
    s: Vec<u8>,
    /// Macros this token may no longer expand (its hide set).
    hs: Vec<Vec<u8>>,
}

impl T {
    fn new(k: K, s: &[u8]) -> T {
        T { k, s: s.to_vec(), hs: Vec::new() }
    }
    fn is(&self, p: &[u8]) -> bool {
        self.k == K::Punct && self.s == p
    }
}

struct Macro {
    params: Option<Vec<Vec<u8>>>,
    body: Vec<T>,
}

struct Cond {
    /// The enclosing region is active.
    outer: bool,
    /// A branch of this #if has been taken.
    taken: bool,
    /// The current branch is active.
    active: bool,
    else_seen: bool,
}

pub struct Pp<'a, F: Files> {
    files: &'a mut F,
    include: Vec<String>,
    macros: BTreeMap<Vec<u8>, Macro>,
    pub out: Vec<Line>,
    pub names: Vec<String>,
    pub diags: Vec<Diag>,
    stack: Vec<String>,
}

pub fn is_id_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

pub fn is_id(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Splits a line into preprocessing tokens. An unterminated string runs to
/// the end of the line (RC continues it on the next).
fn tokenize(s: &[u8]) -> Vec<T> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let start = i;
        let k = if c == b' ' || c == b'\t' || c == 0x0c || c == 0x0b || c == b'\r' {
            while i < s.len() && matches!(s[i], b' ' | b'\t' | 0x0c | 0x0b | b'\r') {
                i += 1;
            }
            K::Ws
        } else if is_id_start(c) {
            while i < s.len() && is_id(s[i]) {
                i += 1;
            }
            K::Id
        } else if c.is_ascii_digit() || (c == b'.' && s.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            i += 1;
            while i < s.len() {
                let d = s[i];
                if (d == b'+' || d == b'-') && matches!(s[i - 1], b'e' | b'E') && !s[start..i].iter().any(|&x| x == b'x' || x == b'X') {
                    i += 1;
                } else if is_id(d) || d == b'.' {
                    i += 1;
                } else {
                    break;
                }
            }
            K::Num
        } else if c == b'"' {
            i += 1;
            while i < s.len() {
                if s[i] == b'\\' && i + 1 < s.len() {
                    i += 2;
                } else if s[i] == b'"' {
                    i += 1;
                    // "" inside an RC string is a quote.
                    if s.get(i) == Some(&b'"') {
                        i += 1;
                        continue;
                    }
                    break;
                } else {
                    i += 1;
                }
            }
            K::Str
        } else if c == b'\'' && char_end(s, i).is_some() {
            i = char_end(s, i).unwrap();
            K::Str
        } else {
            i += 1;
            let two = s.get(start..start + 2);
            if matches!(two, Some(b"##" | b"<<" | b">>" | b"<=" | b">=" | b"==" | b"!=" | b"&&" | b"||")) {
                i += 1;
            }
            K::Punct
        };
        v.push(T::new(k, &s[start..i]));
    }
    v
}

/// The end of the character constant starting at `i`, if it closes.
fn char_end(s: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while j < s.len() {
        match s[j] {
            b'\\' => j += 2,
            b'\'' => return (j > i + 1).then_some(j + 1),
            _ => j += 1,
        }
    }
    None
}

fn text_of(ts: &[T]) -> Vec<u8> {
    let mut v = Vec::new();
    for t in ts {
        v.extend_from_slice(&t.s);
    }
    v
}

/// Physical lines spliced (backslash-newline) and stripped of comments:
/// (first physical line number, text).
fn logical_lines(data: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let data = match data.iter().position(|&c| c == 0x1a) {
        Some(n) => &data[..n],
        None => data,
    };
    let mut spliced: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut cont = false;
    for (n, raw) in data.split(|&c| c == b'\n').enumerate() {
        let mut l = raw;
        while let [rest @ .., b'\r'] = l {
            l = rest;
        }
        let (body, more) = match l {
            [rest @ .., b'\\'] => (rest, true),
            _ => (l, false),
        };
        if cont {
            spliced.last_mut().unwrap().1.extend_from_slice(body);
        } else {
            spliced.push((n as u32 + 1, body.to_vec()));
        }
        cont = more;
    }
    // Comments.
    let mut out = Vec::with_capacity(spliced.len());
    let mut in_block = false;
    for (n, l) in spliced {
        let mut o = Vec::with_capacity(l.len());
        let mut i = 0;
        while i < l.len() {
            if in_block {
                if l[i] == b'*' && l.get(i + 1) == Some(&b'/') {
                    in_block = false;
                    i += 2;
                    o.push(b' ');
                } else {
                    i += 1;
                }
                continue;
            }
            let c = l[i];
            if c == b'/' && l.get(i + 1) == Some(&b'*') {
                in_block = true;
                i += 2;
            } else if c == b'/' && l.get(i + 1) == Some(&b'/') {
                break;
            } else if c == b'"' {
                let start = i;
                i += 1;
                while i < l.len() {
                    if l[i] == b'\\' && i + 1 < l.len() {
                        i += 2;
                    } else if l[i] == b'"' {
                        i += 1;
                        if l.get(i) == Some(&b'"') {
                            i += 1;
                            continue;
                        }
                        break;
                    } else {
                        i += 1;
                    }
                }
                o.extend_from_slice(&l[start..i]);
            } else if c == b'\'' {
                if let Some(e) = char_end(&l, i) {
                    o.extend_from_slice(&l[i..e]);
                    i = e;
                } else {
                    o.push(c);
                    i += 1;
                }
            } else {
                o.push(c);
                i += 1;
            }
        }
        out.push((n, o));
    }
    out
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(s.len());
    let b = s.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(a, |p| p + 1);
    &s[a..b.max(a)]
}

fn latin1(s: &[u8]) -> String {
    s.iter().map(|&b| b as char).collect()
}

impl<'a, F: Files> Pp<'a, F> {
    pub fn new(files: &'a mut F, include: Vec<String>, defines: &[(String, String)]) -> Self {
        let mut p = Pp { files, include, macros: BTreeMap::new(), out: Vec::new(), names: Vec::new(), diags: Vec::new(), stack: Vec::new() };
        p.define(b"RC_INVOKED 1");
        for (n, v) in defines {
            let mut d = n.as_bytes().to_vec();
            d.push(b' ');
            d.extend_from_slice(if v.is_empty() { b"1" } else { v.as_bytes() });
            p.define(&d);
        }
        p
    }

    fn diag(&mut self, num: u16, file: u16, line: u32, arg: &str) {
        let f = self.names.get(file as usize).cloned().unwrap_or_default();
        self.diags.push(Diag { num, file: f, line, arg: String::from(arg) });
    }

    /// Finds and reads an included file: the current directory, the
    /// including file's directory (`"name"` only), then the include path.
    fn find(&mut self, name: &str, quoted: bool) -> Option<(String, Vec<u8>)> {
        let absolute = name.starts_with(['\\', '/']) || name.as_bytes().get(1) == Some(&b':');
        if absolute {
            return self.files.read(name).map(|d| (String::from(name), d));
        }
        let mut tries: Vec<String> = Vec::new();
        if quoted {
            tries.push(String::from(name));
            if let Some(cur) = self.stack.last() {
                if let Some(p) = cur.rfind(['\\', '/', ':']) {
                    tries.push(alloc::format!("{}{}", &cur[..=p], name));
                }
            }
        }
        for dir in &self.include {
            if dir.is_empty() {
                continue;
            }
            if dir.ends_with(['\\', '/', ':']) {
                tries.push(alloc::format!("{dir}{name}"));
            } else {
                tries.push(alloc::format!("{dir}\\{name}"));
            }
        }
        if !quoted {
            tries.push(String::from(name));
        }
        for t in tries {
            if let Some(d) = self.files.read(&t) {
                return Some((t, d));
            }
        }
        None
    }

    /// A file named in the script (ICON, BITMAP, RCINCLUDE...): the current
    /// directory, then the script's directory and the include path.
    pub fn find_resource_file(&mut self, name: &str) -> Option<(String, Vec<u8>)> {
        if let Some(d) = self.files.read(name) {
            return Some((String::from(name), d));
        }
        let absolute = name.starts_with(['\\', '/']) || name.as_bytes().get(1) == Some(&b':');
        if absolute {
            return None;
        }
        if let Some(main) = self.names.first().cloned() {
            if let Some(p) = main.rfind(['\\', '/', ':']) {
                let t = alloc::format!("{}{}", &main[..=p], name);
                if let Some(d) = self.files.read(&t) {
                    return Some((t, d));
                }
            }
        }
        self.find(name, false)
    }

    /// Preprocesses `name` (already read: `data`). `text`: the file's lines
    /// go to the output (the main script, RCINCLUDE files); otherwise only
    /// its directives count (#include).
    pub fn run(&mut self, name: &str, data: &[u8], text: bool) {
        if self.stack.len() > 32 || self.stack.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            let (f, l) = self.here();
            self.diag(2180, f, l, name);
            return;
        }
        let fi = self.names.len() as u16;
        self.names.push(String::from(name));
        self.stack.push(String::from(name));
        let mut conds: Vec<Cond> = Vec::new();
        let lines = logical_lines(data);
        for (n, l) in lines {
            let active = conds.last().map_or(true, |c| c.active);
            let t = trim(&l);
            if t.first() == Some(&b'#') {
                self.directive(&t[1..], fi, n, &mut conds, active);
                continue;
            }
            if !active || !text {
                continue;
            }
            // RCINCLUDE file
            let word_end = t.iter().position(|c| !is_id(*c)).unwrap_or(t.len());
            if t[..word_end].eq_ignore_ascii_case(b"RCINCLUDE") && word_end > 0 {
                let mut f = trim(&t[word_end..]);
                if f.first() == Some(&b'"') {
                    f = &f[1..f.len() - (f.len() > 1 && f.ends_with(b"\"")) as usize];
                }
                let fname = latin1(f).replace("\\\\", "\\");
                if fname.is_empty() {
                    self.diag(2058, fi, n, "");
                    continue;
                }
                match self.find_resource_file(&fname) {
                    Some((path, d)) => {
                        let _ = path;
                        self.run(&fname, &d, true);
                    }
                    None => self.diag(1002, fi, n, &fname),
                }
                continue;
            }
            let toks = tokenize(&l);
            let e = self.expand(toks);
            self.out.push(Line { text: text_of(&e), file: fi, line: n });
        }
        if !conds.is_empty() {
            let last = self.out.last().map_or(0, |l| l.line);
            self.diag(2051, fi, last, "");
        }
        self.stack.pop();
    }

    fn here(&self) -> (u16, u32) {
        (self.names.len().saturating_sub(1) as u16, 0)
    }

    fn define(&mut self, s: &[u8]) -> Option<()> {
        let s = trim(s);
        let n = s.iter().position(|c| !is_id(*c)).unwrap_or(s.len());
        if n == 0 || !is_id_start(s[0]) {
            return None;
        }
        let name = s[..n].to_vec();
        let mut rest = &s[n..];
        let mut params = None;
        if rest.first() == Some(&b'(') {
            let close = rest.iter().position(|&c| c == b')')?;
            let mut ps = Vec::new();
            for p in rest[1..close].split(|&c| c == b',') {
                let p = trim(p);
                if !p.is_empty() {
                    ps.push(p.to_vec());
                }
            }
            params = Some(ps);
            rest = &rest[close + 1..];
        }
        let mut body: Vec<T> = tokenize(trim(rest));
        // Whitespace in a body counts as one blank.
        for t in body.iter_mut() {
            if t.k == K::Ws {
                t.s = vec![b' '];
            }
        }
        self.macros.insert(name, Macro { params, body });
        Some(())
    }

    fn directive(&mut self, d: &[u8], fi: u16, n: u32, conds: &mut Vec<Cond>, active: bool) {
        let d = trim(d);
        let w = d.iter().position(|c| !is_id(*c)).unwrap_or(d.len());
        let (word, rest) = (&d[..w], trim(&d[w..]));
        let lw: Vec<u8> = word.to_ascii_lowercase();
        match &lw[..] {
            b"if" | b"ifdef" | b"ifndef" => {
                let v = if !active {
                    false
                } else if lw == b"if" {
                    self.cond_value(rest, fi, n)
                } else {
                    let m = rest.iter().position(|c| !is_id(*c)).unwrap_or(rest.len());
                    if m == 0 {
                        self.diag(2047, fi, n, &latin1(word));
                    }
                    let def = self.macros.contains_key(&rest[..m]);
                    if lw == b"ifdef" { def } else { !def }
                };
                conds.push(Cond { outer: active, taken: v, active: v, else_seen: false });
            }
            b"elif" => match conds.last_mut() {
                Some(c) if !c.else_seen => {
                    if c.outer && !c.taken {
                        let (outer, _) = (c.outer, ());
                        let _ = outer;
                        let v = self.cond_value(rest, fi, n);
                        let c = conds.last_mut().unwrap();
                        c.active = v;
                        c.taken = v;
                    } else {
                        c.active = false;
                    }
                }
                _ => self.diag(2050, fi, n, ""),
            },
            b"else" => match conds.last_mut() {
                Some(c) if !c.else_seen => {
                    c.else_seen = true;
                    c.active = c.outer && !c.taken;
                    c.taken = true;
                }
                _ => self.diag(2050, fi, n, ""),
            },
            b"endif" => {
                if conds.pop().is_none() {
                    self.diag(2050, fi, n, "");
                }
            }
            _ if !active => {}
            b"" => {}
            b"define" => {
                if self.define(rest).is_none() {
                    self.diag(2047, fi, n, "define");
                }
            }
            b"undef" => {
                let m = rest.iter().position(|c| !is_id(*c)).unwrap_or(rest.len());
                self.macros.remove(&rest[..m]);
            }
            b"include" => {
                let mut r = rest.to_vec();
                if !matches!(r.first(), Some(b'"' | b'<')) {
                    r = text_of(&self.expand(tokenize(&r)));
                }
                let r = trim(&r);
                let (quoted, close) = match r.first() {
                    Some(b'"') => (true, b'"'),
                    Some(b'<') => (false, b'>'),
                    _ => {
                        self.diag(2176, fi, n, "");
                        return;
                    }
                };
                let Some(end) = r[1..].iter().position(|&c| c == close) else {
                    self.diag(2057, fi, n, "");
                    return;
                };
                let name = latin1(&r[1..1 + end]).replace("\\\\", "\\");
                if trim(name.as_bytes()).is_empty() {
                    self.diag(2177, fi, n, "");
                    return;
                }
                match self.find(&name, quoted) {
                    Some((path, data)) => self.run(&path, &data, false),
                    None => self.diag(2178, fi, n, &name),
                }
            }
            b"pragma" | b"line" | b"ident" | b"warning" => {}
            b"error" => self.diag(2003, fi, n, &latin1(rest)),
            _ => self.diag(2175, fi, n, &latin1(word)),
        }
    }

    fn cond_value(&mut self, e: &[u8], fi: u16, n: u32) -> bool {
        // `defined NAME` and `defined(NAME)` before expansion.
        let ts = tokenize(e);
        let mut v: Vec<T> = Vec::new();
        let mut i = 0;
        while i < ts.len() {
            if ts[i].k == K::Id && ts[i].s == b"defined" {
                let mut j = i + 1;
                while j < ts.len() && ts[j].k == K::Ws {
                    j += 1;
                }
                let paren = j < ts.len() && ts[j].is(b"(");
                if paren {
                    j += 1;
                    while j < ts.len() && ts[j].k == K::Ws {
                        j += 1;
                    }
                }
                if j < ts.len() && ts[j].k == K::Id {
                    let d = self.macros.contains_key(&ts[j].s);
                    j += 1;
                    if paren {
                        while j < ts.len() && ts[j].k == K::Ws {
                            j += 1;
                        }
                        if j < ts.len() && ts[j].is(b")") {
                            j += 1;
                        }
                    }
                    v.push(T::new(K::Num, if d { b"1" } else { b"0" }));
                    i = j;
                    continue;
                }
            }
            v.push(ts[i].clone());
            i += 1;
        }
        let x = self.expand(v);
        let text = text_of(&x);
        match crate::expr::eval_pp(&text) {
            Ok(v) => v != 0,
            Err(div0) => {
                self.diag(if div0 { 2049 } else { 2048 }, fi, n, "");
                false
            }
        }
    }

    /// Macro expansion of a token list (hide sets, as C's rescanning).
    fn expand(&self, ts: Vec<T>) -> Vec<T> {
        let mut q: VecDeque<T> = ts.into();
        let mut out = Vec::new();
        let mut steps = 0usize;
        while let Some(t) = q.pop_front() {
            steps += 1;
            if steps > 100_000 {
                out.push(t);
                out.extend(q);
                break;
            }
            if t.k != K::Id || t.hs.iter().any(|h| *h == t.s) {
                out.push(t);
                continue;
            }
            let Some(m) = self.macros.get(&t.s) else {
                out.push(t);
                continue;
            };
            match &m.params {
                None => {
                    let mut hs = t.hs.clone();
                    hs.push(t.s.clone());
                    let body = self.subst(m, &[], &hs);
                    for b in body.into_iter().rev() {
                        q.push_front(b);
                    }
                }
                Some(params) => {
                    // Needs an argument list.
                    let mut j = 0;
                    while j < q.len() && q[j].k == K::Ws {
                        j += 1;
                    }
                    if j >= q.len() || !q[j].is(b"(") {
                        out.push(t);
                        continue;
                    }
                    let mut depth = 0;
                    let mut args: Vec<Vec<T>> = vec![Vec::new()];
                    let mut k = j + 1;
                    let mut closed = None;
                    while k < q.len() {
                        let a = &q[k];
                        if a.is(b"(") {
                            depth += 1;
                        } else if a.is(b")") {
                            if depth == 0 {
                                closed = Some(k);
                                break;
                            }
                            depth -= 1;
                        } else if a.is(b",") && depth == 0 {
                            args.push(Vec::new());
                            k += 1;
                            continue;
                        }
                        args.last_mut().unwrap().push(a.clone());
                        k += 1;
                    }
                    let Some(close) = closed else {
                        out.push(t);
                        continue;
                    };
                    let rparen = q[close].clone();
                    for _ in 0..=close {
                        q.pop_front();
                    }
                    if params.is_empty() && args.len() == 1 && args[0].iter().all(|a| a.k == K::Ws) {
                        args.clear();
                    }
                    let mut hs: Vec<Vec<u8>> = t.hs.iter().filter(|h| rparen.hs.contains(h)).cloned().collect();
                    hs.push(t.s.clone());
                    let body = self.subst(m, &args, &hs);
                    for b in body.into_iter().rev() {
                        q.push_front(b);
                    }
                }
            }
        }
        out
    }

    fn subst(&self, m: &Macro, args: &[Vec<T>], hs: &[Vec<u8>]) -> Vec<T> {
        let params: &[Vec<u8>] = m.params.as_deref().unwrap_or(&[]);
        let param = |t: &T| if t.k == K::Id { params.iter().position(|p| *p == t.s) } else { None };
        let body = &m.body;
        let mut out: Vec<T> = Vec::new();
        let mut i = 0;
        let next_nonws = |from: usize| (from..body.len()).find(|&x| body[x].k != K::Ws);
        while i < body.len() {
            let t = &body[i];
            if t.is(b"#") && params.len() + m.params.is_some() as usize > 0 {
                if let Some(j) = next_nonws(i + 1) {
                    if let Some(p) = param(&body[j]) {
                        let raw = args.get(p).map(|a| trim(&text_of(a)).to_vec()).unwrap_or_default();
                        let mut s = vec![b'"'];
                        for &c in &raw {
                            if c == b'"' || c == b'\\' {
                                s.push(b'\\');
                            }
                            s.push(c);
                        }
                        s.push(b'"');
                        out.push(T::new(K::Str, &s));
                        i = j + 1;
                        continue;
                    }
                }
            }
            if t.is(b"##") {
                // Paste the previous token with the next.
                while out.last().is_some_and(|x| x.k == K::Ws) {
                    out.pop();
                }
                let j = next_nonws(i + 1);
                let rhs: Vec<T> = match j {
                    Some(j) => match param(&body[j]) {
                        Some(p) => args.get(p).cloned().unwrap_or_default(),
                        None => vec![body[j].clone()],
                    },
                    None => Vec::new(),
                };
                let mut left = out.pop().map(|x| x.s).unwrap_or_default();
                let rhs_trim: Vec<&T> = rhs.iter().skip_while(|x| x.k == K::Ws).collect();
                if let Some(first) = rhs_trim.first() {
                    left.extend_from_slice(&first.s);
                }
                out.extend(tokenize(&left));
                for r in rhs_trim.iter().skip(1) {
                    out.push((*r).clone());
                }
                i = j.map_or(body.len(), |j| j + 1);
                continue;
            }
            if let Some(p) = param(t) {
                let next_is_paste = next_nonws(i + 1).is_some_and(|j| body[j].is(b"##"));
                let a = args.get(p).cloned().unwrap_or_default();
                if next_is_paste {
                    out.extend(a);
                } else {
                    out.extend(self.expand(a));
                }
                i += 1;
                continue;
            }
            out.push(t.clone());
            i += 1;
        }
        for t in out.iter_mut() {
            for h in hs {
                if !t.hs.contains(h) {
                    t.hs.push(h.clone());
                }
            }
        }
        out
    }
}
