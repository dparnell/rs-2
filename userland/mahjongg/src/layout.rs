//! Game layouts: where the 144 tiles lie, with the game's name, author
//! and board, read from and written to OS/2's Mahjongg game files
//! (`.MAH`: tagged text, `:GAME`, `:BOARD`, `:LEVEL n.` with the tiles'
//! positions in tile units, `:ELEVEL.`, `:EGAME.`). Positions are kept
//! in half tiles: x and y may be whole or half, y grows upwards.

use alloc::vec::Vec;

/// One tile's place: x and y in half tiles, the level from 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

pub const TILES: usize = 144;

#[derive(Clone)]
pub struct Layout {
    pub name: Vec<u8>,
    pub help: Vec<u8>,
    pub author: Vec<u8>,
    pub about: Vec<u8>,
    /// The board in tiles.
    pub dx: i32,
    pub dy: i32,
    /// Where the status box is, in tenths of tiles.
    pub x_stat: i32,
    pub y_stat: i32,
    /// The background colour (0xRRGGBB).
    pub color: u32,
    pub pos: Vec<Pos>,
}

impl Layout {
    pub fn empty() -> Layout {
        Layout { name: Vec::new(), help: Vec::new(), author: Vec::new(), about: Vec::new(), dx: 15, dy: 8, x_stat: 0, y_stat: 75, color: 0x2a7a50, pos: Vec::new() }
    }

    /// The layout the program starts with: the classic turtle, five
    /// levels.
    pub fn default_layout() -> Layout {
        let mut l = Layout::empty();
        l.name = b"Turtle".to_vec();
        l.author = b"rs2".to_vec();
        l.about = b"The traditional layout".to_vec();
        l.dx = 15;
        l.dy = 8;
        // The status box in the empty corner at the upper left.
        l.x_stat = 2;
        l.y_stat = 61;
        // Level 0: eight rows (from the top), the long rows ending in
        // the three outer tiles of the middle.
        let rows: [(i32, i32); 8] = [(1, 12), (3, 10), (2, 11), (1, 12), (1, 12), (2, 11), (3, 10), (1, 12)];
        for (r, &(a, b)) in rows.iter().enumerate() {
            let y = 7 - r as i32;
            for x in a..=b {
                l.pos.push(Pos { x: x * 2, y: y * 2, z: 0 });
            }
        }
        for x in [0, 13, 14] {
            l.pos.push(Pos { x: x * 2, y: 7, z: 0 });
        }
        for (z, (a, b)) in [(1, (4, 9)), (2, (5, 8)), (3, (6, 7))] {
            let (ya, yb) = (a - 3, b - 3);
            for y in ya..=yb {
                for x in a..=b {
                    l.pos.push(Pos { x: x * 2, y: y * 2, z });
                }
            }
        }
        l.pos.push(Pos { x: 13, y: 7, z: 4 });
        l
    }

    /// Two tiles of one level overlap.
    pub fn overlap(a: Pos, b: Pos) -> bool {
        a.z == b.z && (a.x - b.x).abs() < 2 && (a.y - b.y).abs() < 2
    }

    /// The highest level.
    pub fn levels(&self) -> i32 {
        self.pos.iter().map(|p| p.z).max().unwrap_or(0) + 1
    }

    /// Check a layout: 144 tiles, none overlapping another, all on the
    /// board. The message for the first problem.
    pub fn check(&self) -> Result<(), Vec<u8>> {
        if self.pos.len() != TILES {
            return Err(alloc::format!("There must be 144 tiles; this game has {}.", self.pos.len()).into_bytes());
        }
        for (i, &a) in self.pos.iter().enumerate() {
            if a.x < 0 || a.y < 0 || a.x > (self.dx - 1) * 2 || a.y > (self.dy - 1) * 2 {
                return Err(alloc::format!("The tile at ({}, {}) on level {} lies outside the board.", half(a.x), half(a.y), a.z).into_bytes());
            }
            for &b in &self.pos[i + 1..] {
                if Self::overlap(a, b) {
                    return Err(alloc::format!("The tile at ({}, {}) on level {} overlaps another.", half(a.x), half(a.y), a.z).into_bytes());
                }
            }
        }
        Ok(())
    }

    // ---- .MAH files ----

    /// Read a game file. Errors are messages for the user.
    pub fn parse(text: &[u8]) -> Result<Layout, Vec<u8>> {
        let mut l = Layout::empty();
        let mut p = Parser { t: text, i: 0 };
        let mut seen_game = false;
        let mut seen_board = false;
        let mut level: Option<i32> = None;
        loop {
            p.blanks();
            if p.i >= p.t.len() {
                break;
            }
            if p.t[p.i] == b':' {
                p.i += 1;
                let tag = p.word().to_ascii_uppercase();
                match &tag[..] {
                    b"GAME" => {
                        seen_game = true;
                        for (k, v) in p.pairs() {
                            match &k.to_ascii_lowercase()[..] {
                                b"name" => l.name = v,
                                b"help" => l.help = v,
                                b"author" => l.author = v,
                                b"about" => l.about = v,
                                _ => {}
                            }
                        }
                    }
                    b"BOARD" => {
                        if !seen_game {
                            return Err(b"There is no :GAME. tag before the :BOARD. tag.".to_vec());
                        }
                        seen_board = true;
                        for (k, v) in p.pairs() {
                            let n = tenths(&v);
                            match &k.to_ascii_lowercase()[..] {
                                b"dx" => l.dx = n / 10,
                                b"dy" => l.dy = n / 10,
                                b"x_stat" => l.x_stat = n,
                                b"y_stat" => l.y_stat = n,
                                b"color" => {
                                    if let Some(c) = parse_rgb(&v) {
                                        l.color = c;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    b"LEVEL" => {
                        if !seen_board {
                            return Err(b"There is no :BOARD. tag before a :LEVEL. tag.".to_vec());
                        }
                        p.blanks();
                        let z = p.number() / 10;
                        p.skip_to(b'.');
                        level = Some(z);
                    }
                    b"ELEVEL" => {
                        level = None;
                        p.skip_to(b'.');
                    }
                    b"EGAME" => break,
                    _ => {
                        p.skip_to(b'.');
                    }
                }
                continue;
            }
            if p.t[p.i] == b'(' {
                p.i += 1;
                let Some(z) = level else {
                    return Err(b"A tile's position is outside a :LEVEL. tag.".to_vec());
                };
                p.blanks();
                let x = p.number();
                p.blanks();
                if p.t.get(p.i) == Some(&b',') {
                    p.i += 1;
                }
                p.blanks();
                let y = p.number();
                p.skip_to(b')');
                // Tenths to half tiles (rounded).
                l.pos.push(Pos { x: (x + 2) / 5, y: (y + 2) / 5, z });
                continue;
            }
            p.i += 1;
        }
        if !seen_game {
            return Err(b"This is not a Mahjongg game file (there is no :GAME. tag).".to_vec());
        }
        if l.dx < 1 || l.dy < 1 {
            return Err(b"The board has no size.".to_vec());
        }
        Ok(l)
    }

    /// The game file's text (OS/2's layout, CR LF lines).
    pub fn write(&self) -> Vec<u8> {
        let mut o = Vec::new();
        let q = |v: &[u8]| {
            let mut s = b"\"".to_vec();
            s.extend_from_slice(v);
            s.push(b'"');
            s
        };
        let line = |o: &mut Vec<u8>, s: &[u8]| {
            o.extend_from_slice(s);
            o.extend_from_slice(b"\r\n");
        };
        line(&mut o, &[&b":GAME  type    = "[..], &q(b"Mahjongg Solitaire 2.00")].concat());
        line(&mut o, &[&b"       name    = "[..], &q(&self.name)].concat());
        line(&mut o, &[&b"       help    = "[..], &q(&self.help)].concat());
        line(&mut o, &[&b"       tileset = "[..], &q(b"DEFAULT")].concat());
        line(&mut o, &[&b"       author  = "[..], &q(&self.author)].concat());
        line(&mut o, &[&b"       about   = "[..], &q(&self.about)].concat());
        line(&mut o, b"       .");
        line(&mut o, alloc::format!(":BOARD dx      = {}", self.dx).as_bytes());
        line(&mut o, alloc::format!("       dy      = {}", self.dy).as_bytes());
        line(&mut o, alloc::format!("       x_stat  = '{}.{}'", self.x_stat / 10, self.x_stat % 10).as_bytes());
        line(&mut o, alloc::format!("       y_stat  = '{}.{}'", self.y_stat / 10, self.y_stat % 10).as_bytes());
        line(&mut o, b"       bitmap  = \"DEFAULT\"");
        line(&mut o, alloc::format!("       color   = \"RGB({},{},{})\"", self.color >> 16, (self.color >> 8) & 0xff, self.color & 0xff).as_bytes());
        line(&mut o, b"       use     = \"COLOR\"");
        line(&mut o, b"       .");
        for z in 0..self.levels() {
            line(&mut o, alloc::format!(":LEVEL {z}.").as_bytes());
            let mut s = Vec::new();
            let on: Vec<&Pos> = self.pos.iter().filter(|p| p.z == z).collect();
            for (i, p) in on.iter().enumerate() {
                if i % 6 == 0 {
                    s.extend_from_slice(b"  ");
                }
                s.extend_from_slice(alloc::format!("({:>4}, {:>4})", half(p.x), half(p.y)).as_bytes());
                if i + 1 < on.len() {
                    s.push(b',');
                    if i % 6 == 5 {
                        line(&mut o, &s);
                        s.clear();
                    } else {
                        s.push(b' ');
                    }
                }
            }
            if !s.is_empty() {
                line(&mut o, &s);
            }
        }
        line(&mut o, b":ELEVEL.");
        line(&mut o, b":EGAME.");
        o
    }
}

/// Half tiles as "x.0" or "x.5".
fn half(v: i32) -> alloc::string::String {
    alloc::format!("{}.{}", v / 2, if v % 2 != 0 { 5 } else { 0 })
}

/// "12", "7.8", "'16.3'" in tenths.
fn tenths(v: &[u8]) -> i32 {
    let mut p = Parser { t: v, i: 0 };
    p.number()
}

/// "RGB(191,126,63)"
fn parse_rgb(v: &[u8]) -> Option<u32> {
    let s = v.to_ascii_uppercase();
    let at = s.windows(4).position(|w| w == b"RGB(")?;
    let mut p = Parser { t: &s[at + 4..], i: 0 };
    let mut c = 0u32;
    for _ in 0..3 {
        p.blanks();
        let n = (p.number() / 10).clamp(0, 255) as u32;
        c = c << 8 | n;
        p.blanks();
        if p.t.get(p.i) == Some(&b',') {
            p.i += 1;
        }
    }
    Some(c)
}

struct Parser<'a> {
    t: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn blanks(&mut self) {
        while self.i < self.t.len() && matches!(self.t[self.i], b' ' | b'\t' | b'\r' | b'\n' | 0x1a) {
            self.i += 1;
        }
    }
    fn word(&mut self) -> Vec<u8> {
        let s = self.i;
        while self.i < self.t.len() && (self.t[self.i].is_ascii_alphanumeric() || self.t[self.i] == b'_') {
            self.i += 1;
        }
        self.t[s..self.i].to_vec()
    }
    fn skip_to(&mut self, c: u8) {
        while self.i < self.t.len() && self.t[self.i] != c {
            self.i += 1;
        }
        if self.i < self.t.len() {
            self.i += 1;
        }
    }
    /// A number in tenths ("16.3" is 163); quotes around it are skipped.
    fn number(&mut self) -> i32 {
        while self.i < self.t.len() && matches!(self.t[self.i], b'\'' | b'"' | b' ') {
            self.i += 1;
        }
        let neg = self.t.get(self.i) == Some(&b'-');
        if neg {
            self.i += 1;
        }
        let mut v = 0i32;
        while self.i < self.t.len() && self.t[self.i].is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add((self.t[self.i] - b'0') as i32);
            self.i += 1;
        }
        v *= 10;
        if self.t.get(self.i) == Some(&b'.') && self.t.get(self.i + 1).is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
            v += (self.t[self.i] - b'0') as i32;
            self.i += 1;
            while self.i < self.t.len() && self.t[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        if neg { -v } else { v }
    }
    /// `key = value` pairs up to the closing `.` of the tag.
    fn pairs(&mut self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        loop {
            self.blanks();
            if self.i >= self.t.len() {
                break;
            }
            if self.t[self.i] == b'.' {
                self.i += 1;
                break;
            }
            if self.t[self.i] == b':' {
                break;
            }
            let k = self.word();
            if k.is_empty() {
                self.i += 1;
                continue;
            }
            self.blanks();
            if self.t.get(self.i) == Some(&b'=') {
                self.i += 1;
            }
            self.blanks();
            let v = match self.t.get(self.i) {
                Some(&q) if q == b'"' || q == b'\'' => {
                    self.i += 1;
                    let s = self.i;
                    while self.i < self.t.len() && self.t[self.i] != q && self.t[self.i] != b'\n' {
                        self.i += 1;
                    }
                    let v = self.t[s..self.i].to_vec();
                    if self.i < self.t.len() && self.t[self.i] == q {
                        self.i += 1;
                    }
                    v
                }
                _ => {
                    let s = self.i;
                    while self.i < self.t.len() && !matches!(self.t[self.i], b' ' | b'\t' | b'\r' | b'\n') {
                        self.i += 1;
                    }
                    self.t[s..self.i].to_vec()
                }
            };
            out.push((k, v));
        }
        out
    }
}
