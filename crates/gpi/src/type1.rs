//! Type 1 fonts (Adobe's, as OS/2's PSFONTS\*.PFB hold them): the PFB
//! segments, eexec decryption, the private dictionary's Subrs and the
//! CharStrings, and a charstring interpreter (hsbw/sbw, the moves, lines
//! and curves, closepath, callsubr, seac accents, flex and hint
//! replacement through callothersubr) that makes `ttf::Op` outlines for
//! the shared rasteriser. Hints are ignored.

use alloc::vec::Vec;

use crate::math::floor;
use crate::ttf::{self, Op, Outline, Pf, Rendered};

pub struct Type1 {
    /// (glyph name, charstring decrypted), sorted by name.
    glyphs: Vec<(Vec<u8>, Vec<u8>)>,
    subrs: Vec<Vec<u8>>,
    /// Font units per em (from the FontMatrix; 1000 as a rule).
    pub units: f64,
    pub ascender: i32,
    pub descender: i32,
    pub x_height: i32,
    pub avg_width: i32,
    pub name: Vec<u8>,
}

fn decrypt(data: &[u8], mut r: u16, skip: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for &c in data {
        out.push(c ^ (r >> 8) as u8);
        r = (c as u16).wrapping_add(r).wrapping_mul(52845).wrapping_add(22719);
    }
    out.split_off(skip.min(out.len()))
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// A cursor over the decrypted private part: PostScript tokens, and the
/// binary strings that follow `n RD `.
struct Lex<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Lex<'a> {
    fn space(&mut self) {
        while self.p < self.b.len() && matches!(self.b[self.p], b' ' | b'\t' | b'\r' | b'\n') {
            self.p += 1;
        }
    }

    fn token(&mut self) -> Option<&'a [u8]> {
        self.space();
        let s = self.p;
        if s >= self.b.len() {
            return None;
        }
        if matches!(self.b[s], b'[' | b']' | b'{' | b'}') {
            self.p += 1;
            return Some(&self.b[s..s + 1]);
        }
        self.p += 1;
        while self.p < self.b.len() && !matches!(self.b[self.p], b' ' | b'\t' | b'\r' | b'\n' | b'[' | b']' | b'{' | b'}' | b'/') {
            self.p += 1;
        }
        Some(&self.b[s..self.p])
    }

    fn number(&mut self) -> Option<usize> {
        let t = self.token()?;
        core::str::from_utf8(t).ok()?.parse().ok()
    }

    /// `n RD <n bytes>` (or `-|`): the bytes.
    fn binary(&mut self) -> Option<&'a [u8]> {
        let n = self.number()?;
        self.token()?;
        // One space separates the token from the data.
        self.p += 1;
        let d = self.b.get(self.p..self.p + n)?;
        self.p += n;
        Some(d)
    }
}

impl Type1 {
    /// A .PFB file (segments 0x80 0x01 text, 0x80 0x02 binary).
    pub fn parse(pfb: &[u8]) -> Option<Type1> {
        let (mut text, mut bin) = (Vec::new(), Vec::new());
        let mut p = 0;
        while p + 6 <= pfb.len() && pfb[p] == 0x80 {
            let kind = pfb[p + 1];
            if kind == 3 {
                break;
            }
            let n = u32::from_le_bytes([pfb[p + 2], pfb[p + 3], pfb[p + 4], pfb[p + 5]]) as usize;
            let d = pfb.get(p + 6..p + 6 + n)?;
            if kind == 1 && bin.is_empty() {
                text.extend_from_slice(d);
            } else if kind == 2 {
                bin.extend_from_slice(d);
            }
            p += 6 + n;
        }
        if bin.is_empty() {
            return None;
        }
        let units = find(&text, b"/FontMatrix")
            .and_then(|i| {
                let s = &text[i..];
                let a = find(s, b"[")? + 1;
                let first = s[a..].split(|&c| c == b' ').find(|t| !t.is_empty())?;
                let v: f64 = core::str::from_utf8(first).ok()?.trim().parse().ok()?;
                (v > 0.0).then(|| 1.0 / v)
            })
            .unwrap_or(1000.0);
        let name = find(&text, b"/FontName")
            .and_then(|i| {
                let s = &text[i + 9..];
                let a = find(s, b"/")? + 1;
                let e = s[a..].iter().position(|&c| matches!(c, b' ' | b'\r' | b'\n' | b'\t'))?;
                Some(s[a..a + e].to_vec())
            })
            .unwrap_or_default();
        let private = decrypt(&bin, 55665, 4);
        let len_iv = find(&private, b"/lenIV").and_then(|i| Lex { b: &private, p: i + 6 }.number()).unwrap_or(4);
        let mut subrs = Vec::new();
        if let Some(i) = find(&private, b"/Subrs") {
            let mut l = Lex { b: &private, p: i + 6 };
            let n = l.number()?;
            subrs = alloc::vec![Vec::new(); n];
            // `array`, then: dup i n RD <bytes> NP
            let mut seen = 0;
            while seen < n {
                let Some(t) = l.token() else { break };
                if t != b"dup" {
                    if t == b"ND" || t == b"end" || t == b"readonly" && seen > 0 {
                        break;
                    }
                    continue;
                }
                seen += 1;
                let k = l.number()?;
                let d = l.binary()?;
                l.token()?;
                if let Some(s) = subrs.get_mut(k) {
                    *s = decrypt(d, 4330, len_iv);
                }
            }
        }
        let mut glyphs = Vec::new();
        let at = find(&private, b"/CharStrings")?;
        let mut l = Lex { b: &private, p: at + 12 };
        let n = l.number()?;
        // To the first glyph name.
        while l.p < private.len() && private[l.p] != b'/' {
            l.p += 1;
        }
        for _ in 0..n {
            let Some(t) = l.token() else { break };
            if t.first() != Some(&b'/') {
                break;
            }
            let name = t[1..].to_vec();
            let Some(d) = l.binary() else { break };
            l.token();
            glyphs.push((name, decrypt(d, 4330, len_iv)));
        }
        glyphs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut t = Type1 { glyphs, subrs, units, ascender: 0, descender: 0, x_height: 0, avg_width: 0, name };
        // Metrics from the glyphs themselves.
        let top = |t: &Type1, s: &[u8], hi: bool| -> i32 {
            let mut best: Option<f64> = None;
            for &c in s {
                let (o, _) = t.outline_of(STD[c as usize - 32]);
                for op in &o {
                    let y = match op {
                        Op::M(p) | Op::L(p) | Op::Q(_, p) | Op::C(_, _, p) => p.1,
                    };
                    best = Some(match best {
                        Some(b) if hi => b.max(y),
                        Some(b) => b.min(y),
                        None => y,
                    });
                }
            }
            best.unwrap_or(0.0) as i32
        };
        t.ascender = top(&t, b"bdfhklABCDEFGHIJKLMNOPQRSTUVWXYZ", true).max((0.7 * units) as i32);
        t.descender = (-top(&t, b"gjpqy", false)).max((0.2 * units) as i32);
        t.x_height = top(&t, b"xvwz", true);
        t.avg_width = t.outline_of(b"x").1 as i32;
        Some(t)
    }

    fn charstring(&self, name: &[u8]) -> Option<&[u8]> {
        self.glyphs.binary_search_by(|g| g.0.as_slice().cmp(name)).ok().map(|i| self.glyphs[i].1.as_slice())
    }

    /// A glyph's outline (font units, y up) and advance width.
    pub fn outline_of(&self, name: &[u8]) -> (Outline, f64) {
        let mut r = Run { t: self, ops: Vec::new(), stack: Vec::new(), ps: Vec::new(), x: 0.0, y: 0.0, origin: (0.0, 0.0), width: 0.0, flex: None, open: false, done: false, depth: 0 };
        if let Some(cs) = self.charstring(name) {
            r.exec(cs);
        }
        (r.ops, r.width)
    }

    /// The pels per unit for an em of `px` pels, nudged so the
    /// x-height is whole pels (as `Ttf::scale_for`).
    pub fn scale_for(&self, px: f64) -> f64 {
        let s = px / self.units;
        if self.x_height > 0 && px < 40.0 {
            let xh = self.x_height as f64 * s;
            let k = floor(xh + 0.5).max(1.0) / xh;
            if (0.92..=1.08).contains(&k) {
                return s * k;
            }
        }
        s
    }

    /// Render a character (by its Unicode name; nothing for a glyph the
    /// font lacks).
    pub fn render(&self, ch: char, scale: f64, bold: f64) -> Rendered {
        let name = glyph_name(ch);
        let (o, w) = match name {
            Some(n) if self.charstring(n).is_some() => self.outline_of(n),
            _ => self.outline_of(b"space"),
        };
        let adv = floor(w * scale + bold + 0.5) as i32;
        ttf::render(&o, scale, adv, bold)
    }
}

impl crate::font::Outlines for Type1 {
    fn scale_for(&self, px: f64) -> f64 {
        Type1::scale_for(self, px)
    }
    fn render(&self, ch: char, scale: f64, bold: f64) -> Rendered {
        Type1::render(self, ch, scale, bold)
    }
    fn metrics(&self) -> (i32, i32, i32) {
        (self.ascender, self.descender, self.avg_width)
    }
    fn outline(&self, ch: char) -> (Outline, f64) {
        match glyph_name(ch) {
            Some(n) if self.charstring(n).is_some() => self.outline_of(n),
            _ => self.outline_of(b"space"),
        }
    }
}

/// A charstring being run.
struct Run<'a> {
    t: &'a Type1,
    ops: Outline,
    stack: Vec<f64>,
    /// The PostScript operand stack callothersubr hands values back on.
    ps: Vec<f64>,
    x: f64,
    y: f64,
    /// Where the glyph's origin is (seac's accent sits elsewhere).
    origin: Pf,
    width: f64,
    /// The flex points being collected.
    flex: Option<Vec<Pf>>,
    open: bool,
    done: bool,
    depth: u32,
}

impl Run<'_> {
    fn move_to(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        if let Some(f) = &mut self.flex {
            f.push((self.x, self.y));
            return;
        }
        self.ops.push(Op::M((self.x, self.y)));
        self.open = true;
    }

    fn line_to(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.ops.push(Op::L((self.x, self.y)));
    }

    fn curve(&mut self, d: [f64; 6]) {
        let a = (self.x + d[0], self.y + d[1]);
        let b = (a.0 + d[2], a.1 + d[3]);
        let c = (b.0 + d[4], b.1 + d[5]);
        (self.x, self.y) = c;
        self.ops.push(Op::C(a, b, c));
    }

    fn exec(&mut self, cs: &[u8]) {
        self.depth += 1;
        if self.depth > 16 {
            return;
        }
        let mut i = 0;
        while i < cs.len() && !self.done {
            let v = cs[i];
            i += 1;
            match v {
                32..=246 => self.stack.push(v as f64 - 139.0),
                247..=250 => {
                    let w = *cs.get(i).unwrap_or(&0) as f64;
                    i += 1;
                    self.stack.push((v as f64 - 247.0) * 256.0 + w + 108.0);
                }
                251..=254 => {
                    let w = *cs.get(i).unwrap_or(&0) as f64;
                    i += 1;
                    self.stack.push(-(v as f64 - 251.0) * 256.0 - w - 108.0);
                }
                255 => {
                    let b = cs.get(i..i + 4).unwrap_or(&[0; 4]);
                    i += 4;
                    self.stack.push(i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64);
                }
                1 | 3 => self.stack.clear(),
                4 => {
                    let dy = self.arg(0);
                    self.move_to(0.0, dy);
                }
                5 => {
                    let (dx, dy) = (self.arg(0), self.arg(1));
                    self.line_to(dx, dy);
                }
                6 => {
                    let dx = self.arg(0);
                    self.line_to(dx, 0.0);
                }
                7 => {
                    let dy = self.arg(0);
                    self.line_to(0.0, dy);
                }
                8 => {
                    let d = [self.arg(0), self.arg(1), self.arg(2), self.arg(3), self.arg(4), self.arg(5)];
                    self.curve(d);
                }
                9 => {
                    // closepath: the next move starts a new figure.
                    self.open = false;
                }
                10 => {
                    let k = self.stack.pop().unwrap_or(-1.0);
                    let sub = self.t.subrs.get(k as usize).cloned();
                    if let (true, Some(sub)) = (k >= 0.0, sub) {
                        self.exec(&sub);
                    }
                    continue;
                }
                11 => break,
                13 => {
                    // hsbw: side bearing x, width.
                    self.x = self.origin.0 + self.arg(0);
                    self.y = self.origin.1;
                    if self.origin == (0.0, 0.0) {
                        self.width = self.arg(1);
                    }
                }
                14 => self.done = true,
                21 => {
                    let (dx, dy) = (self.arg(0), self.arg(1));
                    self.move_to(dx, dy);
                }
                22 => {
                    let dx = self.arg(0);
                    self.move_to(dx, 0.0);
                }
                30 => {
                    let d = [0.0, self.arg(0), self.arg(1), self.arg(2), self.arg(3), 0.0];
                    self.curve(d);
                }
                31 => {
                    let d = [self.arg(0), 0.0, self.arg(1), self.arg(2), 0.0, self.arg(3)];
                    self.curve(d);
                }
                12 => {
                    let e = *cs.get(i).unwrap_or(&0);
                    i += 1;
                    self.escape(e);
                    if e == 16 || e == 17 || e == 12 {
                        continue;
                    }
                }
                _ => {}
            }
            // Operators consume the whole stack.
            if v < 32 {
                self.stack.clear();
            }
        }
        self.depth -= 1;
    }

    fn arg(&self, i: usize) -> f64 {
        self.stack.get(i).copied().unwrap_or(0.0)
    }

    fn escape(&mut self, e: u8) {
        match e {
            // seac: asb adx ady bchar achar.
            6 => {
                let (asb, adx, ady, b, a) = (self.arg(0), self.arg(1), self.arg(2), self.arg(3) as usize, self.arg(4) as usize);
                self.stack.clear();
                let (Some(bn), Some(an)) = (std_name(b), std_name(a)) else { return };
                let (bcs, acs) = (self.t.charstring(bn).map(|c| c.to_vec()), self.t.charstring(an).map(|c| c.to_vec()));
                if let Some(c) = bcs {
                    self.exec(&c);
                    self.done = false;
                }
                if let Some(c) = acs {
                    let saved = self.origin;
                    self.origin = (adx - asb, ady);
                    self.exec(&c);
                    self.origin = saved;
                }
                self.done = true;
            }
            // sbw: side bearing x, y, width x, y.
            7 => {
                self.x = self.origin.0 + self.arg(0);
                self.y = self.origin.1 + self.arg(1);
                if self.origin == (0.0, 0.0) {
                    self.width = self.arg(2);
                }
            }
            // div
            12 => {
                let b = self.stack.pop().unwrap_or(1.0);
                let a = self.stack.pop().unwrap_or(0.0);
                self.stack.push(if b != 0.0 { a / b } else { 0.0 });
            }
            // callothersubr: args... n othersubr#.
            16 => {
                let k = self.stack.pop().unwrap_or(-1.0) as i32;
                let n = self.stack.pop().unwrap_or(0.0).max(0.0) as usize;
                let at = self.stack.len().saturating_sub(n);
                let args: Vec<f64> = self.stack.split_off(at);
                match k {
                    // Flex starts: collect the rmoveto points.
                    1 => self.flex = Some(Vec::new()),
                    2 => {}
                    // Flex ends: two curves through the points collected
                    // (the first is the reference point).
                    0 => {
                        if let Some(p) = self.flex.take()
                            && p.len() >= 7
                        {
                            self.ops.push(Op::C(p[1], p[2], p[3]));
                            self.ops.push(Op::C(p[4], p[5], p[6]));
                            (self.x, self.y) = p[6];
                        }
                        // pop pop setcurrentpoint gets the end point.
                        self.ps = alloc::vec![self.y, self.x];
                        return;
                    }
                    _ => {}
                }
                // Hint replacement (3) and the rest hand their arguments
                // back for `pop`.
                self.ps = args.into_iter().collect();
            }
            // pop
            17 => {
                let v = self.ps.pop().unwrap_or(0.0);
                self.stack.push(v);
            }
            // setcurrentpoint
            33 => {
                self.x = self.arg(0);
                self.y = self.arg(1);
            }
            _ => {}
        }
        if e != 16 && e != 17 && e != 12 {
            self.stack.clear();
        }
    }
}

/// Adobe StandardEncoding's names for codes 32..=126 (index code - 32)
/// and the accents seac uses.
const STD: [&[u8]; 95] = [
    b"space", b"exclam", b"quotedbl", b"numbersign", b"dollar", b"percent", b"ampersand", b"quoteright", b"parenleft", b"parenright",
    b"asterisk", b"plus", b"comma", b"hyphen", b"period", b"slash", b"zero", b"one", b"two", b"three", b"four", b"five", b"six",
    b"seven", b"eight", b"nine", b"colon", b"semicolon", b"less", b"equal", b"greater", b"question", b"at", b"A", b"B", b"C", b"D",
    b"E", b"F", b"G", b"H", b"I", b"J", b"K", b"L", b"M", b"N", b"O", b"P", b"Q", b"R", b"S", b"T", b"U", b"V", b"W", b"X", b"Y",
    b"Z", b"bracketleft", b"backslash", b"bracketright", b"asciicircum", b"underscore", b"quoteleft", b"a", b"b", b"c", b"d", b"e",
    b"f", b"g", b"h", b"i", b"j", b"k", b"l", b"m", b"n", b"o", b"p", b"q", b"r", b"s", b"t", b"u", b"v", b"w", b"x", b"y", b"z",
    b"braceleft", b"bar", b"braceright", b"asciitilde",
];

fn std_name(code: usize) -> Option<&'static [u8]> {
    match code {
        32..=126 => Some(STD[code - 32]),
        193 => Some(b"grave"),
        194 => Some(b"acute"),
        195 => Some(b"circumflex"),
        196 => Some(b"tilde"),
        197 => Some(b"macron"),
        198 => Some(b"breve"),
        199 => Some(b"dotaccent"),
        200 => Some(b"dieresis"),
        202 => Some(b"ring"),
        203 => Some(b"cedilla"),
        205 => Some(b"hungarumlaut"),
        206 => Some(b"ogonek"),
        207 => Some(b"caron"),
        245 => Some(b"dotlessi"),
        _ => None,
    }
}

/// Latin-1's glyph names (U+00A0..U+00FF).
const LATIN1: [&[u8]; 96] = [
    b"space", b"exclamdown", b"cent", b"sterling", b"currency", b"yen", b"brokenbar", b"section", b"dieresis", b"copyright",
    b"ordfeminine", b"guillemotleft", b"logicalnot", b"hyphen", b"registered", b"macron", b"degree", b"plusminus", b"twosuperior",
    b"threesuperior", b"acute", b"mu", b"paragraph", b"periodcentered", b"cedilla", b"onesuperior", b"ordmasculine",
    b"guillemotright", b"onequarter", b"onehalf", b"threequarters", b"questiondown", b"Agrave", b"Aacute", b"Acircumflex",
    b"Atilde", b"Adieresis", b"Aring", b"AE", b"Ccedilla", b"Egrave", b"Eacute", b"Ecircumflex", b"Edieresis", b"Igrave",
    b"Iacute", b"Icircumflex", b"Idieresis", b"Eth", b"Ntilde", b"Ograve", b"Oacute", b"Ocircumflex", b"Otilde", b"Odieresis",
    b"multiply", b"Oslash", b"Ugrave", b"Uacute", b"Ucircumflex", b"Udieresis", b"Yacute", b"Thorn", b"germandbls", b"agrave",
    b"aacute", b"acircumflex", b"atilde", b"adieresis", b"aring", b"ae", b"ccedilla", b"egrave", b"eacute", b"ecircumflex",
    b"edieresis", b"igrave", b"iacute", b"icircumflex", b"idieresis", b"eth", b"ntilde", b"ograve", b"oacute", b"ocircumflex",
    b"otilde", b"odieresis", b"divide", b"oslash", b"ugrave", b"uacute", b"ucircumflex", b"udieresis", b"yacute", b"thorn",
    b"ydieresis",
];

/// The glyph name of a character.
fn glyph_name(ch: char) -> Option<&'static [u8]> {
    let c = ch as u32;
    Some(match c {
        0x27 => b"quotesingle",
        0x60 => b"grave",
        0x20..=0x7e => STD[c as usize - 32],
        0xa0..=0xff => LATIN1[c as usize - 0xa0],
        0x0192 => b"florin",
        0x0131 => b"dotlessi",
        0x2017 => b"underscoredbl",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eexec_round_trip() {
        // Encrypting is the inverse: c = p ^ (r >> 8), r from c.
        let plain = b"abcd/Subrs 0 array";
        let mut r: u16 = 55665;
        let mut enc = Vec::new();
        for &p in plain.iter() {
            let c = p ^ (r >> 8) as u8;
            enc.push(c);
            r = (c as u16).wrapping_add(r).wrapping_mul(52845).wrapping_add(22719);
        }
        assert_eq!(decrypt(&enc, 55665, 4), b"/Subrs 0 array");
    }

    #[test]
    fn ibm_helvetica() {
        extern crate std;
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/unpacked/disk_5/PSFONTS/HELV.PFB");
        let Ok(data) = std::fs::read(p) else { return };
        let t = Type1::parse(&data).expect("parses");
        assert_eq!(t.name, b"Helvetica");
        assert_eq!(t.units, 1000.0);
        let (o, w) = t.outline_of(b"H");
        assert!(!o.is_empty());
        assert!((700.0..=760.0).contains(&w), "H width {w}");
        assert!((690..=760).contains(&t.ascender), "ascender {}", t.ascender);
        let r = t.render('H', t.scale_for(13.0), 0.0);
        assert!(r.cov.iter().filter(|&&c| c > 200).count() > 10);
        // An accented letter (seac or its own outline).
        let r = t.render('é', t.scale_for(13.0), 0.0);
        assert!(r.cov.iter().filter(|&&c| c > 100).count() > 8);
    }
}

#[cfg(test)]
mod dump {
    #[test]
    #[ignore]
    fn show() {
        extern crate std;
        use std::println;
        let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/unpacked/disk_5/PSFONTS/HELV.PFB")).unwrap();
        let t = super::Type1::parse(&data).unwrap();
        println!("subrs {} glyphs {}", t.subrs.len(), t.glyphs.len());
        for ch in ['d', 'g', 'é', 'Å', '&'] {
            let r = t.render(ch, t.scale_for(13.0), 0.0);
            println!("{ch} adv {}", r.advance);
            for y in 0..r.h {
                let row: std::string::String = (0..r.w).map(|x| match r.cov[(y * r.w + x) as usize] { 0 => '.', 1..=100 => '-', 101..=200 => '+', _ => '#' }).collect();
                println!("{row}");
            }
        }
    }
}
