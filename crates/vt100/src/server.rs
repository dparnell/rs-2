//! The other side of a terminal: what a server (rs2's TELNETD) needs to
//! show a screen of PC text cells on a VT100/ANSI terminal and to turn
//! what the terminal sends back into PC keystrokes.
//!
//! [`Renderer`] keeps the screen it last sent and writes only the
//! differences: cursor positioning, SGR colours (the PC attribute's
//! foreground, background, intensity as bold, blink), erase to the end of
//! the line for blank tails, and whole-screen scrolls as line feeds at the
//! bottom when the screen moved up. Characters go out through an
//! [`Encoding`] (UTF-8 from a code page table, or one byte each through a
//! translation table).
//!
//! [`KeyDecoder`] reads the client's bytes: printable characters (UTF-8
//! or single bytes back through the code page), control characters as
//! Ctrl+letter, Enter, Tab, Backspace, Esc, and the VT100/VT220/xterm
//! sequences of the cursor, editing and function keys.

use alloc::vec::Vec;

/// How screen bytes go out to the terminal.
pub struct Encoding {
    bytes: [[u8; 4]; 256],
    len: [u8; 256],
}

/// The CP437 pictures of the control positions 0-31 and 127, which PC
/// text screens show (Unicode).
pub const CONTROL_GLYPHS: [u16; 33] = [
    0x0020, 0x263a, 0x263b, 0x2665, 0x2666, 0x2663, 0x2660, 0x2022, 0x25d8, 0x25cb, 0x25d9, 0x2642, 0x2640, 0x266a, 0x266b, 0x263c, 0x25ba, 0x25c4,
    0x2195, 0x203c, 0x00b6, 0x00a7, 0x25ac, 0x21a8, 0x2191, 0x2193, 0x2192, 0x2190, 0x221f, 0x2194, 0x25b2, 0x25bc, 0x2302,
];

impl Encoding {
    /// UTF-8, with `table` giving the Unicode of each byte of the screen's
    /// code page (control positions show their PC pictures).
    pub fn utf8(table: &[u16; 256]) -> Encoding {
        let mut e = Encoding { bytes: [[0; 4]; 256], len: [0; 256] };
        for b in 0..256usize {
            let u = match b {
                0..=31 => CONTROL_GLYPHS[b],
                127 => CONTROL_GLYPHS[32],
                _ => table[b],
            };
            let c = char::from_u32(u as u32).unwrap_or('?');
            let n = c.encode_utf8(&mut [0u8; 4]).len();
            c.encode_utf8(&mut e.bytes[b]);
            e.len[b] = n as u8;
        }
        e
    }

    /// One byte per character: `table` maps screen bytes to what the
    /// terminal shows (controls become '.' unless mapped to printable
    /// characters).
    pub fn bytes(table: &[u8; 256]) -> Encoding {
        let mut e = Encoding { bytes: [[0; 4]; 256], len: [1; 256] };
        for b in 0..256usize {
            let t = table[b];
            e.bytes[b][0] = if t < 0x20 || t == 0x7f || (0x80..0xa0).contains(&t) { b'.' } else { t };
        }
        e
    }

    fn put(&self, b: u8, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.bytes[b as usize][..self.len[b as usize] as usize]);
    }
}

/// Colour numbers: PC order (blue = 1, red = 4) to ANSI order (red = 1,
/// blue = 4).
const PC_TO_ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// A blank cell: what a cleared terminal shows for it.
fn blank_equiv(cell: u16) -> u16 {
    let ch = cell as u8;
    if matches!(ch, 0 | 0x20 | 0xff) { 0x0720 | (cell & 0x7000) } else { cell }
}

/// What the terminal shows now and the changes to it.
pub struct Renderer {
    cols: usize,
    rows: usize,
    shown: Vec<u16>,
    /// The terminal's cursor, when known.
    at: Option<(usize, usize)>,
    /// The attribute of the last SGR sent.
    attr: Option<u8>,
    cursor_on: Option<bool>,
    valid: bool,
    /// Attribute bit 7 blinks (else it brightens the background).
    pub blink: bool,
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer { cols: 0, rows: 0, shown: Vec::new(), at: None, attr: None, cursor_on: None, valid: false, blink: true }
    }

    /// Send the whole screen again at the next update.
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    fn goto(&mut self, r: usize, c: usize, out: &mut Vec<u8>) {
        if self.at == Some((r, c)) {
            return;
        }
        if c == 0 && r == 0 {
            out.extend_from_slice(b"\x1b[H");
        } else if c == 0 {
            out.extend_from_slice(alloc::format!("\x1b[{}H", r + 1).as_bytes());
        } else {
            out.extend_from_slice(alloc::format!("\x1b[{};{}H", r + 1, c + 1).as_bytes());
        }
        self.at = Some((r, c));
    }

    fn set_attr(&mut self, a: u8, out: &mut Vec<u8>) {
        if self.attr == Some(a) {
            return;
        }
        let fg = PC_TO_ANSI[(a & 7) as usize];
        let bg = PC_TO_ANSI[((a >> 4) & 7) as usize];
        out.extend_from_slice(b"\x1b[0");
        if a & 0x08 != 0 {
            out.extend_from_slice(b";1");
        }
        if a & 0x80 != 0 && self.blink {
            out.extend_from_slice(b";5");
        }
        out.extend_from_slice(alloc::format!(";3{}", fg).as_bytes());
        if a & 0x80 != 0 && !self.blink {
            out.extend_from_slice(alloc::format!(";10{}m", bg).as_bytes());
        } else {
            out.extend_from_slice(alloc::format!(";4{}m", bg).as_bytes());
        }
        self.attr = Some(a);
    }

    /// How many lines the screen moved up since it was shown (0: none
    /// worth sending as a scroll).
    fn scrolled_by(&self, cells: &[u16]) -> usize {
        let cols = self.cols;
        let row = |v: &[u16], r: usize| -> u64 {
            let mut h = 0xcbf29ce484222325u64;
            for &c in &v[r * cols..(r + 1) * cols] {
                h = (h ^ blank_equiv(c) as u64).wrapping_mul(0x100000001b3);
            }
            h
        };
        let old: Vec<u64> = (0..self.rows).map(|r| row(&self.shown, r)).collect();
        let new: Vec<u64> = (0..self.rows).map(|r| row(cells, r)).collect();
        let same = (0..self.rows).filter(|&r| old[r] == new[r]).count();
        let mut best = (0, same);
        for k in 1..self.rows {
            let m = (0..self.rows - k).filter(|&r| old[r + k] == new[r]).count();
            if m > best.1 + 2 {
                best = (k, m);
            }
        }
        best.0
    }

    /// Bring the terminal up to date with `cells` (`cols` x `rows`, char |
    /// attr << 8) and the cursor (None: hidden), appending what to send
    /// to `out`.
    pub fn update(&mut self, cols: usize, rows: usize, cells: &[u16], cursor: Option<(usize, usize)>, enc: &Encoding, out: &mut Vec<u8>) {
        if cells.len() < cols * rows || cols == 0 || rows == 0 {
            return;
        }
        let cells = &cells[..cols * rows];
        if !self.valid || cols != self.cols || rows != self.rows {
            self.cols = cols;
            self.rows = rows;
            self.shown = alloc::vec![0x0720; cols * rows];
            self.attr = None;
            self.cursor_on = None;
            out.extend_from_slice(b"\x1b[0m\x1b[r\x1b[H\x1b[2J");
            self.attr = Some(0x07);
            self.at = Some((0, 0));
            self.valid = true;
        }
        let k = self.scrolled_by(cells);
        if k > 0 {
            // Line feeds at the bottom move the terminal's lines up, with
            // blank (default coloured) lines coming in.
            self.set_attr(0x07, out);
            self.goto(rows - 1, 0, out);
            for _ in 0..k {
                out.push(b'\n');
            }
            self.shown.drain(..k * cols);
            self.shown.extend(core::iter::repeat_n(0x0720u16, k * cols));
        }
        for r in 0..rows {
            let row = &cells[r * cols..(r + 1) * cols];
            let old = self.shown[r * cols..(r + 1) * cols].to_vec();
            if (0..cols).all(|c| blank_equiv(row[c]) == blank_equiv(old[c])) {
                continue;
            }
            // A blank tail on the default background: erase to the end.
            let mut tail = cols;
            while tail > 0 && blank_equiv(row[tail - 1]) == 0x0720 {
                tail -= 1;
            }
            let tail_differs = (tail..cols).any(|c| blank_equiv(old[c]) != 0x0720);
            let mut c = 0;
            while c < tail {
                if blank_equiv(row[c]) == blank_equiv(old[c]) {
                    c += 1;
                    continue;
                }
                // Changed cells, with short unchanged gaps rewritten.
                let mut end = c + 1;
                let mut gap = 0;
                let mut j = c + 1;
                while j < tail {
                    if blank_equiv(row[j]) != blank_equiv(old[j]) {
                        end = j + 1;
                        gap = 0;
                    } else {
                        gap += 1;
                        if gap > 6 {
                            break;
                        }
                    }
                    j += 1;
                }
                self.goto(r, c, out);
                for &cell in &row[c..end] {
                    self.set_attr((cell >> 8) as u8, out);
                    enc.put(cell as u8, out);
                }
                self.at = if end < cols { Some((r, end)) } else { None };
                c = end;
            }
            if tail_differs {
                self.goto(r, tail, out);
                self.set_attr(0x07, out);
                out.extend_from_slice(b"\x1b[K");
            }
            self.shown[r * cols..(r + 1) * cols].copy_from_slice(row);
        }
        match cursor {
            Some((r, c)) if r < rows && c < cols => {
                self.goto(r, c, out);
                if self.cursor_on != Some(true) {
                    out.extend_from_slice(b"\x1b[?25h");
                    self.cursor_on = Some(true);
                }
            }
            _ => {
                if self.cursor_on != Some(false) {
                    out.extend_from_slice(b"\x1b[?25l");
                    self.cursor_on = Some(false);
                }
            }
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// A PC keystroke: character, scan code, KbdCharIn status (0x02: an
/// extended key's E0 character) and shift state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PcKey {
    pub ch: u8,
    pub scan: u8,
    pub ext: bool,
    pub shift: u16,
}

/// KBD shift state bits.
pub const SHIFT_LEFT: u16 = 0x0002;
pub const SHIFT_CTRL: u16 = 0x0004 | 0x0100;
pub const SHIFT_ALT: u16 = 0x0008 | 0x0200;

// Set-1 scan codes of the US layout for printable ASCII.
const NORMAL: &[u8; 0x3a] = b"\x00\x1b1234567890-=\x08\tqwertyuiop[]\r\x00asdfghjkl;'`\x00\\zxcvbnm,./\x00*\x00 ";
const SHIFTED: &[u8; 0x3a] = b"\x00\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\r\x00ASDFGHJKL:\"~\x00|ZXCVBNM<>?\x00*\x00 ";

fn ascii_key(ch: u8) -> PcKey {
    if let Some(p) = NORMAL.iter().position(|&c| c == ch).filter(|_| ch >= 0x20) {
        return PcKey { ch, scan: p as u8, ext: false, shift: 0 };
    }
    if let Some(p) = SHIFTED.iter().position(|&c| c == ch).filter(|_| ch >= 0x20) {
        return PcKey { ch, scan: p as u8, ext: false, shift: SHIFT_LEFT };
    }
    PcKey { ch, scan: 0, ext: false, shift: 0 }
}

/// Grey (E0) keys: Home, Up, PgUp, Left, Right, End, Down, PgDn, Ins, Del.
fn grey(scan: u8, shift: u16) -> PcKey {
    PcKey { ch: 0xe0, scan, ext: true, shift }
}

fn fkey(n: u8, shift: u16) -> PcKey {
    let base = match n {
        1..=10 => 0x3a + n,
        11 => 0x85,
        _ => 0x86,
    };
    // Shift, Ctrl and Alt give the other scan codes of F1-F12.
    let scan = match (n, shift) {
        (1..=10, s) if s & SHIFT_ALT != 0 => 0x67 + n,
        (1..=10, s) if s & SHIFT_CTRL != 0 => 0x5d + n,
        (1..=10, s) if s & SHIFT_LEFT != 0 => 0x53 + n,
        (11 | 12, s) if s & SHIFT_ALT != 0 => base + 6,
        (11 | 12, s) if s & SHIFT_CTRL != 0 => base + 4,
        (11 | 12, s) if s & SHIFT_LEFT != 0 => base + 2,
        _ => base,
    };
    PcKey { ch: 0, scan, ext: false, shift }
}

/// Reads keys from what a terminal sends.
pub struct KeyDecoder {
    pending: Vec<u8>,
    utf8: Vec<u8>,
    /// The screen's code page as Unicode (UTF-8 input mapped back); None:
    /// bytes go in as they are, through `in_table`.
    pub table: Option<[u16; 256]>,
    /// Bytes from the terminal to screen bytes (single-byte terminals).
    pub in_table: Option<[u8; 256]>,
    last_cr: bool,
}

impl KeyDecoder {
    pub fn new() -> KeyDecoder {
        KeyDecoder { pending: Vec::new(), utf8: Vec::new(), table: None, in_table: None, last_cr: false }
    }

    fn char_key(&self, b: u8, out: &mut Vec<PcKey>) {
        if b < 0x80 {
            out.push(ascii_key(b));
        } else {
            out.push(PcKey { ch: b, scan: 0, ext: false, shift: 0 });
        }
    }

    fn csi_key(params: &[u8], fin: u8) -> Option<PcKey> {
        let txt = core::str::from_utf8(params).ok()?;
        let mut it = txt.split(';');
        let n: u8 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        // xterm modifiers: 1 + (shift 1 | alt 2 | ctrl 4).
        let m: u8 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        let m = m.saturating_sub(1);
        let mut shift = 0;
        if m & 1 != 0 {
            shift |= SHIFT_LEFT;
        }
        if m & 2 != 0 {
            shift |= SHIFT_ALT;
        }
        if m & 4 != 0 {
            shift |= SHIFT_CTRL;
        }
        Some(match fin {
            b'A' => grey(0x48, shift),
            b'B' => grey(0x50, shift),
            b'C' => grey(0x4d, shift),
            b'D' => grey(0x4b, shift),
            b'H' => grey(0x47, shift),
            b'F' => grey(0x4f, shift),
            b'P' => fkey(1, shift),
            b'Q' => fkey(2, shift),
            b'R' => fkey(3, shift),
            b'S' => fkey(4, shift),
            b'Z' => PcKey { ch: 0, scan: 0x0f, ext: false, shift: SHIFT_LEFT },
            b'~' => match n {
                1 | 7 => grey(0x47, shift),
                2 => grey(0x52, shift),
                3 => grey(0x53, shift),
                4 | 8 => grey(0x4f, shift),
                5 => grey(0x49, shift),
                6 => grey(0x51, shift),
                11..=15 => fkey(n - 10, shift),
                17..=21 => fkey(n - 11, shift),
                23 | 24 => fkey(n - 12, shift),
                _ => return None,
            },
            _ => return None,
        })
    }

    /// Keys in `data`. A lone Esc at the end of the data is Esc (escape
    /// sequences arrive whole).
    pub fn feed(&mut self, data: &[u8]) -> Vec<PcKey> {
        let mut out = Vec::new();
        for &b in data {
            if !self.pending.is_empty() {
                self.pending.push(b);
                let p = &self.pending;
                if p.len() == 2 {
                    if b == b'[' || b == b'O' {
                        continue;
                    }
                    // Esc + a character: Alt+character.
                    self.pending.clear();
                    let mut k = ascii_key(b.to_ascii_lowercase());
                    k.ch = 0;
                    k.shift |= SHIFT_ALT;
                    if k.scan == 0 {
                        out.push(PcKey { ch: 0x1b, scan: 0x01, ext: false, shift: 0 });
                        self.char_key(b, &mut out);
                    } else {
                        out.push(k);
                    }
                    continue;
                }
                if (0x40..=0x7e).contains(&b) {
                    let seq = core::mem::take(&mut self.pending);
                    let params = &seq[2..seq.len() - 1];
                    if let Some(k) = Self::csi_key(params, b) {
                        out.push(k);
                    }
                } else if self.pending.len() > 16 {
                    self.pending.clear();
                }
                continue;
            }
            if !self.utf8.is_empty() || (b >= 0x80 && self.table.is_some()) {
                self.utf8.push(b);
                let need = match self.utf8[0] {
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf7 => 4,
                    _ => 1,
                };
                if self.utf8.len() < need {
                    continue;
                }
                let s = core::mem::take(&mut self.utf8);
                if let Some(c) = core::str::from_utf8(&s).ok().and_then(|s| s.chars().next()) {
                    let t = self.table.as_ref().unwrap();
                    if let Some(byte) = (0x80..=0xffusize).find(|&i| t[i] == c as u32 as u16) {
                        out.push(PcKey { ch: byte as u8, scan: 0, ext: false, shift: 0 });
                    }
                }
                continue;
            }
            let cr = core::mem::replace(&mut self.last_cr, false);
            match b {
                0x1b => self.pending.push(b),
                b'\r' => {
                    self.last_cr = true;
                    out.push(PcKey { ch: b'\r', scan: 0x1c, ext: false, shift: 0 });
                }
                // CR LF and CR NUL are one Enter.
                b'\n' | 0 if cr => {}
                b'\n' => out.push(PcKey { ch: b'\r', scan: 0x1c, ext: false, shift: 0 }),
                b'\t' => out.push(PcKey { ch: b'\t', scan: 0x0f, ext: false, shift: 0 }),
                8 | 0x7f => out.push(PcKey { ch: 8, scan: 0x0e, ext: false, shift: 0 }),
                1..=0x1a => {
                    let letter = b'a' + b - 1;
                    out.push(PcKey { ch: b, scan: ascii_key(letter).scan, ext: false, shift: SHIFT_CTRL });
                }
                0 => {}
                0x1c..=0x1f => out.push(PcKey { ch: b, scan: 0, ext: false, shift: SHIFT_CTRL }),
                _ => {
                    let b = self.in_table.as_ref().map_or(b, |t| t[b as usize]);
                    self.char_key(b, &mut out);
                }
            }
        }
        if self.pending.len() == 1 {
            self.pending.clear();
            out.push(PcKey { ch: 0x1b, scan: 0x01, ext: false, shift: 0 });
        }
        out
    }
}

impl Default for KeyDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Term;
    use alloc::vec;

    fn cp437() -> [u16; 256] {
        // Enough of CP437 for the tests: ASCII and the box characters.
        let mut t = [0u16; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = i as u16;
        }
        t[0xc4] = 0x2500;
        t[0xb3] = 0x2502;
        t[0xda] = 0x250c;
        t[0xdb] = 0x2588;
        t
    }

    fn screen(rows: &[&str], attr: u8) -> Vec<u16> {
        let mut v = vec![0x0720u16; 80 * 25];
        for (r, s) in rows.iter().enumerate() {
            for (c, b) in s.bytes().enumerate() {
                v[r * 80 + c] = b as u16 | (attr as u16) << 8;
            }
        }
        v
    }

    fn check(term: &Term, cells: &[u16]) {
        for r in 0..25 {
            for c in 0..80 {
                let want = cells[r * 80 + c];
                let got = term.cell(r, c);
                assert_eq!(got as u8, want as u8, "char at {r},{c}");
                if want as u8 != b' ' {
                    assert_eq!(got >> 8, want >> 8, "attr at {r},{c}");
                }
            }
        }
    }

    #[test]
    fn round_trip() {
        let enc = Encoding::utf8(&cp437());
        let mut r = Renderer::new();
        let mut t = Term::new(80, 25);
        let mut out = Vec::new();
        let mut a = screen(&["Hello", "", "world"], 0x1e);
        a[3 * 80 + 10] = 0xc4 | 0x4f00;
        a[3 * 80 + 11] = 0xdb | 0x0a00;
        a[24 * 80 + 79] = b'X' as u16 | 0x0700;
        r.update(80, 25, &a, Some((2, 5)), &enc, &mut out);
        t.feed(&out);
        check(&t, &a);
        assert_eq!(t.cursor(), (2, 5));
        // A change in the middle of a line.
        let mut b = a.clone();
        b[80 + 40] = b'!' as u16 | 0x0700;
        b[2] = b'L' as u16 | 0x1e00;
        out.clear();
        r.update(80, 25, &b, Some((1, 41)), &enc, &mut out);
        assert!(out.len() < 40, "{}", out.len());
        t.feed(&out);
        check(&t, &b);
        // Nothing changed: only the cursor.
        out.clear();
        r.update(80, 25, &b, Some((1, 41)), &enc, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn scrolls() {
        let enc = Encoding::utf8(&cp437());
        let mut r = Renderer::new();
        let mut t = Term::new(80, 25);
        let lines: Vec<alloc::string::String> = (0..25).map(|i| alloc::format!("line {i} of the screen")).collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let a = screen(&refs, 0x07);
        let mut out = Vec::new();
        r.update(80, 25, &a, Some((24, 0)), &enc, &mut out);
        t.feed(&out);
        let lines2: Vec<alloc::string::String> = (2..27).map(|i| alloc::format!("line {i} of the screen")).collect();
        let refs2: Vec<&str> = lines2.iter().map(|s| s.as_str()).collect();
        let b = screen(&refs2, 0x07);
        out.clear();
        r.update(80, 25, &b, Some((24, 0)), &enc, &mut out);
        assert!(out.len() < 120, "{}", out.len());
        t.feed(&out);
        check(&t, &b);
    }

    #[test]
    fn keys() {
        let mut d = KeyDecoder::new();
        let k = d.feed(b"a\x1b[A\x1b[3~\x1bOP\r\n\x03\x1b");
        assert_eq!(k[0], PcKey { ch: b'a', scan: 0x1e, ext: false, shift: 0 });
        assert_eq!(k[1], PcKey { ch: 0xe0, scan: 0x48, ext: true, shift: 0 });
        assert_eq!(k[2], PcKey { ch: 0xe0, scan: 0x53, ext: true, shift: 0 });
        assert_eq!(k[3], PcKey { ch: 0, scan: 0x3b, ext: false, shift: 0 });
        assert_eq!(k[4].scan, 0x1c);
        assert_eq!(k[5], PcKey { ch: 3, scan: 0x2e, ext: false, shift: SHIFT_CTRL });
        assert_eq!(k[6].ch, 0x1b);
        assert_eq!(k.len(), 7);
        // A sequence split over two reads.
        assert!(d.feed(b"\x1b[1").is_empty());
        assert_eq!(d.feed(b"5~")[0].scan, 0x3f);
        // UTF-8 back to the code page.
        d.table = Some(cp437());
        assert_eq!(d.feed("\u{2500}".as_bytes())[0].ch, 0xc4);
    }
}
