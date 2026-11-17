//! Tokens of the preprocessed script.

use alloc::string::String;
use alloc::vec::Vec;

use crate::pp::{is_id, is_id_start, Line};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    /// A number and whether it had an L suffix.
    Num(u32, bool),
    /// A string literal (escapes done; adjacent literals are separate tokens).
    Str(Vec<u8>),
    Id(Vec<u8>),
    /// An operator or punctuation: `+`, `<<`, `(`, `{`...
    Op(&'static str),
    Eof,
}

impl Tok {
    pub fn is_kw(&self, kw: &str) -> bool {
        matches!(self, Tok::Id(s) if s.eq_ignore_ascii_case(kw.as_bytes()))
    }
    pub fn is_op(&self, op: &str) -> bool {
        matches!(self, Tok::Op(o) if *o == op)
    }
    pub fn describe(&self) -> String {
        match self {
            Tok::Num(n, _) => alloc::format!("{n}"),
            Tok::Str(s) => {
                let mut o = String::from("\"");
                o.extend(s.iter().map(|&b| b as char));
                o.push('"');
                o
            }
            Tok::Id(s) => s.iter().map(|&b| b as char).collect(),
            Tok::Op(o) => String::from(*o),
            Tok::Eof => String::from("end of file"),
        }
    }
}

const OPS2: [&str; 9] = ["<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "##"];
const OPS1: &str = "+-*/%&|^~!<>(),{}=?:;#.[]@$\\`";

fn op1(c: u8) -> Option<&'static str> {
    let i = OPS1.bytes().position(|o| o == c)?;
    Some(&OPS1[i..i + 1])
}

pub struct Lexer<'a> {
    lines: &'a [Line],
    li: usize,
    pos: usize,
    peeked: Option<(Tok, usize)>,
    /// Line index of the last token returned.
    pub tok_line: usize,
    /// Set when a string had a bad escape.
    pub bad_escape: bool,
    /// The last file name was in quotes.
    pub quoted: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(lines: &'a [Line]) -> Self {
        Lexer { lines, li: 0, pos: 0, peeked: None, tok_line: 0, bad_escape: false, quoted: false }
    }

    /// (file index, line number) of the last token.
    pub fn place(&self) -> (u16, u32) {
        match self.lines.get(self.tok_line).or(self.lines.last()) {
            Some(l) => (l.file, l.line),
            None => (0, 0),
        }
    }

    fn cur(&self) -> &'a [u8] {
        self.lines.get(self.li).map_or(&[], |l| &l.text[..])
    }

    fn skip_ws(&mut self) -> bool {
        loop {
            if self.li >= self.lines.len() {
                return false;
            }
            let l = self.cur();
            while self.pos < l.len() && (l[self.pos].is_ascii_whitespace() || l[self.pos] == 0 || l[self.pos] == b';') {
                self.pos += 1;
            }
            if self.pos < l.len() {
                return true;
            }
            self.li += 1;
            self.pos = 0;
        }
    }

    pub fn peek(&mut self) -> &Tok {
        if self.peeked.is_none() {
            let t = self.lex();
            self.peeked = Some((t, self.tok_line));
        }
        &self.peeked.as_ref().unwrap().0
    }

    pub fn next(&mut self) -> Tok {
        if let Some((t, l)) = self.peeked.take() {
            self.tok_line = l;
            return t;
        }
        self.lex()
    }

    /// True if the next token is on the same line as the last one.
    pub fn same_line(&mut self) -> bool {
        let last = self.tok_line;
        self.peek();
        self.peeked.as_ref().is_some_and(|p| p.1 == last)
    }

    /// A file name: a quoted string (taken as it is, `\\` read as `\`) or
    /// everything up to the next blank.
    pub fn filename(&mut self) -> Option<String> {
        self.quoted = matches!(self.peeked, Some((Tok::Str(_), _)));
        if let Some((t, l)) = self.peeked.take() {
            // Already lexed as a token: only a string can be a whole name.
            self.tok_line = l;
            return match t {
                Tok::Str(s) => Some(s.iter().map(|&b| b as char).collect()),
                Tok::Eof => None,
                other => {
                    // Rebuild from the token and the rest of the word.
                    let mut s = other.describe();
                    let l = self.cur();
                    while self.pos < l.len() && !l[self.pos].is_ascii_whitespace() {
                        s.push(l[self.pos] as char);
                        self.pos += 1;
                    }
                    Some(s)
                }
            };
        }
        if !self.skip_ws() {
            return None;
        }
        self.tok_line = self.li;
        let l = self.cur();
        let mut s = String::new();
        if l[self.pos] == b'"' {
            self.quoted = true;
            self.pos += 1;
            while self.pos < l.len() && l[self.pos] != b'"' {
                if l[self.pos] == b'\\' && l.get(self.pos + 1) == Some(&b'\\') {
                    self.pos += 1;
                }
                s.push(l[self.pos] as char);
                self.pos += 1;
            }
            self.pos += 1;
        } else {
            while self.pos < l.len() && !l[self.pos].is_ascii_whitespace() {
                s.push(l[self.pos] as char);
                self.pos += 1;
            }
        }
        Some(s)
    }

    fn lex(&mut self) -> Tok {
        if !self.skip_ws() {
            self.tok_line = self.lines.len().saturating_sub(1);
            return Tok::Eof;
        }
        self.tok_line = self.li;
        let l = self.cur();
        let c = l[self.pos];
        if is_id_start(c) {
            let s = self.pos;
            while self.pos < l.len() && is_id(l[self.pos]) {
                self.pos += 1;
            }
            return Tok::Id(l[s..self.pos].to_vec());
        }
        if c.is_ascii_digit() {
            return self.number();
        }
        if c == b'"' {
            return Tok::Str(self.string());
        }
        if c == b'\'' {
            // A character constant.
            let s = self.pos;
            self.pos += 1;
            let mut v: u32 = 0;
            while self.pos < l.len() && l[self.pos] != b'\'' {
                let (b, n) = escape(&l[self.pos..], &mut self.bad_escape);
                v = v << 8 | b as u32;
                self.pos += n;
            }
            if self.pos < l.len() {
                self.pos += 1;
                return Tok::Num(v, false);
            }
            self.pos = s + 1;
            return Tok::Op("'");
        }
        if let Some(two) = l.get(self.pos..self.pos + 2) {
            if let Some(o) = OPS2.iter().find(|o| o.as_bytes() == two) {
                self.pos += 2;
                return Tok::Op(o);
            }
        }
        self.pos += 1;
        match op1(c) {
            Some(o) => Tok::Op(o),
            None => Tok::Op("?"),
        }
    }

    fn number(&mut self) -> Tok {
        let l = self.cur();
        let s = self.pos;
        while self.pos < l.len() && is_id(l[self.pos]) {
            self.pos += 1;
        }
        let (v, long) = parse_number(&l[s..self.pos]);
        Tok::Num(v, long)
    }

    /// A string literal starting at the quote: escapes, `""` for a quote,
    /// and a line end inside the string as a newline (it goes on).
    fn string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        self.pos += 1;
        loop {
            let l = self.cur();
            if self.pos >= l.len() {
                // No closing quote on this line: the line end is part of it.
                if self.li + 1 >= self.lines.len() {
                    return out;
                }
                out.push(b'\n');
                self.li += 1;
                self.pos = 0;
                continue;
            }
            let c = l[self.pos];
            if c == b'"' {
                if l.get(self.pos + 1) == Some(&b'"') {
                    out.push(b'"');
                    self.pos += 2;
                    continue;
                }
                self.pos += 1;
                return out;
            }
            if c == b'\\' {
                let (b, n) = escape(&l[self.pos..], &mut self.bad_escape);
                out.push(b);
                self.pos += n;
                continue;
            }
            out.push(c);
            self.pos += 1;
        }
    }
}

/// An escape sequence at `s` (starting with the backslash, or a plain
/// character): its value and length.
fn escape(s: &[u8], bad: &mut bool) -> (u8, usize) {
    if s[0] != b'\\' || s.len() < 2 {
        return (s[0], 1);
    }
    let c = s[1];
    let v = match c {
        b'a' => 7,
        b'b' => 8,
        b'f' => 12,
        b'n' => 10,
        b'r' => 13,
        b't' => 9,
        b'v' => 11,
        b'\'' | b'"' | b'?' | b'\\' => c,
        b'0'..=b'7' => {
            let mut v: u32 = 0;
            let mut n = 1;
            while n < 4 && s.get(n).is_some_and(|d| (b'0'..=b'7').contains(d)) {
                v = v * 8 + (s[n] - b'0') as u32;
                n += 1;
            }
            return (v as u8, n);
        }
        b'x' | b'X' => {
            let mut v: u32 = 0;
            let mut n = 2;
            while n < 4 && s.get(n).is_some_and(|d| d.is_ascii_hexdigit()) {
                v = v * 16 + (s[n] as char).to_digit(16).unwrap();
                n += 1;
            }
            if n == 2 {
                *bad = true;
            }
            return (v as u8, n);
        }
        _ => {
            *bad = true;
            return (b'\\', 1);
        }
    };
    (v, 2)
}

/// A C integer literal (decimal, 0x hex, 0 octal; U and L suffixes).
pub fn parse_number(s: &[u8]) -> (u32, bool) {
    let mut t = s;
    let mut long = false;
    while let [rest @ .., c] = t {
        match c {
            b'l' | b'L' => long = true,
            b'u' | b'U' => {}
            _ => break,
        }
        t = rest;
    }
    let (digits, radix) = if t.len() > 2 && (t.starts_with(b"0x") || t.starts_with(b"0X")) {
        (&t[2..], 16)
    } else if t.len() > 1 && t[0] == b'0' && t[1..].iter().all(|c| (b'0'..=b'7').contains(c)) {
        (&t[1..], 8)
    } else {
        (t, 10)
    };
    let mut v: u32 = 0;
    for &d in digits {
        let Some(x) = (d as char).to_digit(radix) else { break };
        v = v.wrapping_mul(radix).wrapping_add(x);
    }
    (v, long)
}
