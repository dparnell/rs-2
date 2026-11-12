//! Clipboard text between OS/2 and hosts.
//!
//! OS/2's CF_TEXT is the process code page's bytes with CR LF line ends,
//! ended by a NUL; hosts exchange UTF-8 with LF (the SPICE agent
//! protocol, VMware) or UTF-16 (VirtualBox). Characters the code page
//! does not have become a close ASCII equivalent where there is one
//! (typographic quotes, dashes, the ellipsis, no-break spaces: what a
//! Mac's clipboard is full of), else `?`.

use alloc::vec::Vec;

/// CF_TEXT (up to its NUL) as UTF-8 with LF line ends.
pub fn to_utf8(cp: u16, text: &[u8]) -> Vec<u8> {
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    let mut out = Vec::with_capacity(end);
    let mut buf = [0u8; 4];
    let mut i = 0;
    while i < end {
        let b = text[i];
        i += 1;
        if b == b'\r' && text.get(i) == Some(&b'\n') && i < end {
            continue;
        }
        let c = char::from_u32(uls::cp_to_ucs(cp, b) as u32).unwrap_or('?');
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
    out
}

/// A close ASCII spelling of a character the code page lacks.
fn fallback(c: char) -> &'static str {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' | '\u{2032}' => "'",
        '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' | '\u{2033}' => "\"",
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2212}' => "-",
        '\u{2014}' | '\u{2015}' => "--",
        '\u{2026}' => "...",
        '\u{00a0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' | '\u{202f}' => " ",
        '\u{200b}' | '\u{feff}' => "",
        '\u{2022}' => "*",
        '\u{2122}' => "(TM)",
        '\u{00a9}' => "(C)",
        '\u{00ae}' => "(R)",
        _ => "?",
    }
}

/// Unicode text (any line ends) as CF_TEXT bytes of code page `cp`, with
/// CR LF line ends; no NUL.
pub fn from_chars(cp: u16, chars: impl Iterator<Item = char>) -> Vec<u8> {
    // Reverse table of the code page, sorted by character.
    let mut rev: Vec<(u16, u8)> = (0x20u16..=0xff).filter(|&b| b != 0x7f).map(|b| (uls::cp_to_ucs(cp, b as u8), b as u8)).collect();
    rev.sort_unstable();
    let byte = |c: char| -> Option<u8> {
        let u = c as u32;
        if u < 0x20 || u == 0x7f {
            return Some(u as u8);
        }
        if u > 0xffff {
            return None;
        }
        rev.binary_search_by_key(&(u as u16), |&(c, _)| c).ok().map(|i| rev[i].1)
    };
    let mut out = Vec::new();
    let mut prev_cr = false;
    for c in chars {
        match c {
            '\0' => {}
            '\n' => {
                if !prev_cr {
                    out.push(b'\r');
                }
                out.push(b'\n');
            }
            '\r' => out.push(b'\r'),
            c => match byte(c) {
                Some(b) => out.push(b),
                None => out.extend_from_slice(fallback(c).as_bytes()),
            },
        }
        prev_cr = c == '\r';
    }
    // A lone CR (classic Mac line end) is a line end too.
    let mut fixed = Vec::with_capacity(out.len());
    for (i, &b) in out.iter().enumerate() {
        fixed.push(b);
        if b == b'\r' && out.get(i + 1) != Some(&b'\n') {
            fixed.push(b'\n');
        }
    }
    fixed
}

/// UTF-8 (invalid sequences become U+FFFD) as CF_TEXT bytes.
pub fn from_utf8(cp: u16, s: &[u8]) -> Vec<u8> {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    from_chars(cp, Utf8Lossy { s: &s[..end] })
}

/// UTF-16LE (as VirtualBox gives it) as CF_TEXT bytes.
pub fn from_utf16le(cp: u16, s: &[u8]) -> Vec<u8> {
    let units = s.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).take_while(|&u| u != 0);
    from_chars(cp, char::decode_utf16(units).map(|r| r.unwrap_or('\u{fffd}')))
}

/// CF_TEXT as UTF-16LE with CR LF line ends and a terminating NUL (as
/// VirtualBox wants it).
pub fn to_utf16le(cp: u16, text: &[u8]) -> Vec<u8> {
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    let mut out = Vec::with_capacity(end * 2 + 2);
    for &b in &text[..end] {
        out.extend_from_slice(&uls::cp_to_ucs(cp, b).to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

struct Utf8Lossy<'a> {
    s: &'a [u8],
}

impl Iterator for Utf8Lossy<'_> {
    type Item = char;
    fn next(&mut self) -> Option<char> {
        let s = self.s;
        let b0 = *s.first()?;
        let (len, init) = match b0 {
            0x00..=0x7f => (1, b0 as u32),
            0xc2..=0xdf => (2, (b0 & 0x1f) as u32),
            0xe0..=0xef => (3, (b0 & 0x0f) as u32),
            0xf0..=0xf4 => (4, (b0 & 0x07) as u32),
            _ => {
                self.s = &s[1..];
                return Some('\u{fffd}');
            }
        };
        let mut c = init;
        for i in 1..len {
            match s.get(i) {
                Some(&b) if b & 0xc0 == 0x80 => c = (c << 6) | (b & 0x3f) as u32,
                _ => {
                    self.s = &s[i..];
                    return Some('\u{fffd}');
                }
            }
        }
        self.s = &s[len..];
        Some(char::from_u32(c).unwrap_or('\u{fffd}'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os2_to_utf8() {
        // Code page 850: 0x82 is é, 0x9c £.
        assert_eq!(to_utf8(850, b"caf\x82\r\n\x9c5\0junk"), "café\n£5".as_bytes());
        assert_eq!(to_utf8(437, b"a\rb"), b"a\rb");
    }

    #[test]
    fn utf8_to_os2() {
        assert_eq!(from_utf8(850, "café\n£5".as_bytes()), b"caf\x82\r\n\x9c5");
        assert_eq!(from_utf8(850, b"a\r\nb\rc"), b"a\r\nb\r\nc");
        assert_eq!(from_utf8(850, "\u{201c}hi\u{201d} \u{2014} ok\u{2026} \u{4e2d}".as_bytes()), b"\"hi\" -- ok... ?");
        assert_eq!(from_utf8(850, b"bad\xff\xc3"), b"bad??");
    }

    #[test]
    fn utf16() {
        assert_eq!(from_utf16le(850, &[0x63, 0, 0xe9, 0, 0x0a, 0, 0, 0, 0x41, 0]), b"c\x82\r\n");
        assert_eq!(to_utf16le(850, b"\x82\r\n\0"), [0xe9, 0, 0x0d, 0, 0x0a, 0, 0, 0]);
    }
}
