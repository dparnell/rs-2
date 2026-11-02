//! Tokens and clauses. The source is split into clauses at semicolons and
//! line ends (a comma ending a line continues the clause); comments are
//! dropped. A clause whose characters cannot be tokenized keeps its error,
//! which is raised when the clause is reached (as IBM's REXX does).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::Err;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// A literal string's value; `kind` is 0 plain, b'X' hex, b'B' binary.
    Str { val: Vec<u8>, raw: Vec<u8>, kind: u8 },
    /// A symbol as written.
    Sym(Vec<u8>),
    /// An operator (canonical text).
    Op(&'static [u8]),
    LParen,
    RParen,
    Comma,
    Colon,
    /// Object REXX: `[` and `]`.
    LBrack,
    RBrack,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    /// Whitespace (or a comment) came before the token.
    pub blank: bool,
    pub line: u32,
}

impl Token {
    pub fn is_sym(&self) -> bool {
        matches!(self.tok, Tok::Sym(_))
    }
    pub fn sym(&self) -> Option<&[u8]> {
        match &self.tok {
            Tok::Sym(s) => Some(s),
            _ => None,
        }
    }
    /// The symbol upper-cased, if the token is a symbol.
    pub fn upper(&self) -> Option<Vec<u8>> {
        self.sym().map(upper)
    }
    pub fn is_op(&self, o: &[u8]) -> bool {
        matches!(&self.tok, Tok::Op(x) if *x == o)
    }
    pub fn is_kw(&self, kw: &[u8]) -> bool {
        match &self.tok {
            Tok::Sym(s) => s.len() == kw.len() && s.iter().zip(kw).all(|(a, b)| a.to_ascii_uppercase() == *b),
            _ => false,
        }
    }
    pub fn is_term_start(&self) -> bool {
        matches!(self.tok, Tok::Str { .. } | Tok::Sym(_) | Tok::LParen)
    }
    /// A message operator (`~` or `~~`).
    pub fn is_twiddle(&self) -> bool {
        matches!(&self.tok, Tok::Op(o) if o.starts_with(b"~"))
    }
}

pub fn upper(s: &[u8]) -> Vec<u8> {
    s.iter().map(|c| c.to_ascii_uppercase()).collect()
}

pub struct RawClause {
    pub toks: Vec<Token>,
    pub line: u32,
    /// Tokenizing error in this clause.
    pub err: Option<Err>,
}

pub fn is_sym_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'.' | b'!' | b'?' | b'_' | b'@' | b'#' | b'$')
}

const OPS: &[&[u8]] = &[
    b"\\==", b"\\>>", b"\\<<", b">>=", b"<<=", b"**", b"//", b"||", b"&&", b"==", b"\\=", b"<>", b"><", b">=", b"<=", b">>",
    b"<<", b"\\>", b"\\<", b"+", b"-", b"*", b"/", b"%", b"&", b"|", b"=", b">", b"<", b"\\",
];

/// The canonical operator for a `¬` (0xAA) spelling.
fn op_static(s: &[u8]) -> Option<&'static [u8]> {
    OPS.iter().copied().find(|o| *o == s)
}

/// Hex/binary string validation and conversion.
fn conv_hex(raw: &[u8]) -> Result<Vec<u8>, Err> {
    // blanks allowed only between pairs (at byte boundaries), not leading/trailing
    let mut groups: Vec<&[u8]> = Vec::new();
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    if raw[0] == b' ' || raw[raw.len() - 1] == b' ' {
        return Err(Err::new(15));
    }
    for g in raw.split(|&c| c == b' ' || c == b'\t').filter(|g| !g.is_empty()) {
        groups.push(g);
    }
    let mut digits: Vec<u8> = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        if i > 0 && g.len() % 2 != 0 {
            return Err(Err::new(15));
        }
        for &c in g.iter() {
            if !c.is_ascii_hexdigit() {
                return Err(Err::new(15));
            }
            digits.push(c);
        }
    }
    // the first group may be odd: pad with a zero on the left
    if digits.len() % 2 == 1 {
        digits.insert(0, b'0');
    }
    let hv = |c: u8| (c as char).to_digit(16).unwrap() as u8;
    Ok(digits.chunks(2).map(|p| hv(p[0]) << 4 | hv(p[1])).collect())
}

fn conv_bin(raw: &[u8]) -> Result<Vec<u8>, Err> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    if raw[0] == b' ' || raw[raw.len() - 1] == b' ' {
        return Err(Err::new(15));
    }
    let groups: Vec<&[u8]> = raw.split(|&c| c == b' ' || c == b'\t').filter(|g| !g.is_empty()).collect();
    let mut bits: Vec<u8> = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        if i > 0 && g.len() % 4 != 0 {
            return Err(Err::new(15));
        }
        for &c in g.iter() {
            if c != b'0' && c != b'1' {
                return Err(Err::new(15));
            }
            bits.push(c - b'0');
        }
    }
    while bits.len() % 8 != 0 {
        bits.insert(0, 0);
    }
    Ok(bits.chunks(8).map(|b| b.iter().fold(0u8, |a, &x| a << 1 | x)).collect())
}

/// Split the program into clauses. Fails only on an unterminated comment
/// (the whole program is unusable then).
pub fn clauses(src: &[u8], spell: &mut BTreeMap<Vec<u8>, Vec<u8>>) -> Result<Vec<RawClause>, (Err, u32)> {
    clauses_mode(src, spell, false)
}

/// Split into clauses; `oo`: Object REXX's tokens too (`~`, `~~`, `[`,
/// `]`).
pub fn clauses_mode(src: &[u8], spell: &mut BTreeMap<Vec<u8>, Vec<u8>>, oo: bool) -> Result<Vec<RawClause>, (Err, u32)> {
    let mut out = Vec::new();
    let mut i = 0;
    let n = src.len();
    let mut line: u32 = 1;
    let mut cur = RawClause { toks: Vec::new(), line: 1, err: None };
    let mut blank = false;
    // A Ctrl+Z ends the program text.
    let n = src.iter().position(|&c| c == 0x1a).unwrap_or(n);
    let finish = |cur: &mut RawClause, out: &mut Vec<RawClause>, line: u32| {
        let c = core::mem::replace(cur, RawClause { toks: Vec::new(), line, err: None });
        if !c.toks.is_empty() || c.err.is_some() {
            out.push(c);
        }
    };
    while i < n {
        let c = src[i];
        // comments
        if c == b'/' && i + 1 < n && src[i + 1] == b'*' {
            let start_line = line;
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if src[i] == b'/' && i + 1 < n && src[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if src[i] == b'*' && i + 1 < n && src[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    if src[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            if depth > 0 {
                return Err((Err::new(6), start_line));
            }
            blank = true;
            continue;
        }
        match c {
            b' ' | b'\t' | b'\r' | 0x0c | 0x0b => {
                blank = true;
                i += 1;
            }
            b'\n' => {
                // a comma ending the line continues the clause
                if cur.toks.last().is_some_and(|t| t.tok == Tok::Comma) && cur.err.is_none() {
                    cur.toks.pop();
                    blank = true;
                } else {
                    finish(&mut cur, &mut out, line + 1);
                    blank = false;
                }
                line += 1;
                i += 1;
                if cur.toks.is_empty() {
                    cur.line = line;
                }
            }
            b';' => {
                finish(&mut cur, &mut out, line);
                blank = false;
                i += 1;
            }
            b'\'' | b'"' => {
                let q = c;
                let mut val = Vec::new();
                i += 1;
                let mut closed = false;
                while i < n {
                    if src[i] == q {
                        if i + 1 < n && src[i + 1] == q {
                            val.push(q);
                            i += 2;
                            continue;
                        }
                        i += 1;
                        closed = true;
                        break;
                    }
                    if src[i] == b'\n' {
                        break;
                    }
                    val.push(src[i]);
                    i += 1;
                }
                if !closed {
                    if cur.err.is_none() {
                        cur.err = Some(Err::new(6));
                    }
                    continue;
                }
                // hex/binary suffix: x or b not followed by a symbol character
                let mut kind = 0u8;
                if i < n && matches!(src[i], b'x' | b'X' | b'b' | b'B') && !(i + 1 < n && is_sym_char(src[i + 1])) {
                    kind = src[i].to_ascii_uppercase();
                    i += 1;
                }
                let raw = val.clone();
                let v = match kind {
                    b'X' => conv_hex(&val),
                    b'B' => conv_bin(&val),
                    _ => Ok(val),
                };
                match v {
                    Ok(v) => cur.toks.push(Token { tok: Tok::Str { val: v, raw, kind }, blank, line }),
                    Err(e) => {
                        if cur.err.is_none() {
                            cur.err = Some(e);
                        }
                    }
                }
                blank = false;
            }
            b'(' => {
                cur.toks.push(Token { tok: Tok::LParen, blank, line });
                blank = false;
                i += 1;
            }
            b')' => {
                cur.toks.push(Token { tok: Tok::RParen, blank, line });
                blank = false;
                i += 1;
            }
            b',' => {
                cur.toks.push(Token { tok: Tok::Comma, blank, line });
                blank = false;
                i += 1;
            }
            b':' => {
                cur.toks.push(Token { tok: Tok::Colon, blank, line });
                blank = false;
                i += 1;
            }
            b'[' | b']' if oo => {
                cur.toks.push(Token { tok: if c == b'[' { Tok::LBrack } else { Tok::RBrack }, blank, line });
                blank = false;
                i += 1;
            }
            b'~' if oo => {
                let two = i + 1 < n && src[i + 1] == b'~';
                cur.toks.push(Token { tok: Tok::Op(if two { b"~~" } else { b"~" }), blank, line });
                blank = false;
                i += if two { 2 } else { 1 };
            }
            _ if is_sym_char(c) => {
                let start = i;
                while i < n && is_sym_char(src[i]) {
                    i += 1;
                }
                // a number with an exponent sign: 1E+5, 1.5e-3
                let s = &src[start..i];
                if i + 1 < n
                    && (src[i] == b'+' || src[i] == b'-')
                    && src[i + 1].is_ascii_digit()
                    && (s[0].is_ascii_digit() || s[0] == b'.')
                    && matches!(s[s.len() - 1], b'e' | b'E')
                    && s[..s.len() - 1].iter().all(|&c| c.is_ascii_digit() || c == b'.')
                    && s[..s.len() - 1].iter().filter(|&&c| c == b'.').count() <= 1
                    && s[..s.len() - 1].iter().any(|c| c.is_ascii_digit())
                {
                    i += 1;
                    while i < n && src[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let s = src[start..i].to_vec();
                spell.entry(upper(&s)).or_insert_with(|| s.clone());
                // a compound symbol's stem and variable tails are symbols too
                if !(s[0].is_ascii_digit() || s[0] == b'.') {
                    if let Some(p) = s.iter().position(|&c| c == b'.') {
                        let stem = s[..=p].to_vec();
                        spell.entry(upper(&stem)).or_insert(stem);
                        for part in s[p + 1..].split(|&c| c == b'.') {
                            if !part.is_empty() && !part[0].is_ascii_digit() {
                                spell.entry(upper(part)).or_insert_with(|| part.to_vec());
                            }
                        }
                    }
                }
                cur.toks.push(Token { tok: Tok::Sym(s), blank, line });
                blank = false;
            }
            _ => {
                // operators (¬ = 0xAA is "not")
                let mut buf: Vec<u8> = Vec::new();
                let mut j = i;
                let mut best: Option<(&'static [u8], usize)> = None;
                while j < n && buf.len() < 3 {
                    let ch = if src[j] == 0xAA { b'\\' } else { src[j] };
                    if !b"+-*/%&|=<>\\".contains(&ch) {
                        break;
                    }
                    buf.push(ch);
                    j += 1;
                    if let Some(o) = op_static(&buf) {
                        best = Some((o, j));
                    }
                }
                match best {
                    Some((o, end)) => {
                        cur.toks.push(Token { tok: Tok::Op(o), blank, line });
                        i = end;
                    }
                    None => {
                        if cur.err.is_none() {
                            cur.err = Some(Err::new(13));
                        }
                        i += 1;
                    }
                }
                blank = false;
            }
        }
    }
    finish(&mut cur, &mut out, line);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex() {
        assert_eq!(conv_hex(b"41 42").unwrap(), b"AB");
        assert_eq!(conv_hex(b"1").unwrap(), b"\x01");
        assert!(conv_hex(b"4 1").is_err());
        assert_eq!(conv_bin(b"0100 0001").unwrap(), b"A");
        assert_eq!(conv_bin(b"1").unwrap(), b"\x01");
    }

    #[test]
    fn split() {
        let mut sp = BTreeMap::new();
        let c = clauses(b"/* x */\r\nsay 'a',\r\n  1e+5; x=1\r\nif a then b", &mut sp).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].toks.len(), 3);
        assert_eq!(c[0].toks[2].tok, Tok::Sym(b"1e+5".to_vec()));
        assert_eq!(c[1].line, 3);
    }
}
