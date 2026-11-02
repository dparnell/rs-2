//! A VT100/VT220 terminal (with the ANSI colour and the xterm sequences
//! full-screen programs commonly send) drawn on a screen of PC text
//! cells: a character in code page 437 and a PC attribute byte, as VIO
//! shows them. TELNET feeds it what the host sends and copies the rows
//! that changed to its window.
//!
//! What it does: the C0 controls (BEL, BS, HT, LF/VT/FF, CR, SO/SI,
//! CAN/SUB), ESC 7/8 D E M c H = > ( ) # 8 Z, CSI A-H J K L M P X @ S T
//! a d e f g h l m n r s u c `, DEC private modes 1 (cursor keys), 6
//! (origin), 7 (wrap), 25 (cursor), 47/1047/1049 (alternate screen, kept
//! as a clear), the scrolling region, tab stops, insert mode, the DEC
//! special graphics set (line drawing as CP437 box characters), SGR
//! colours (8 and bright), bold, underline, blink, reverse, concealed;
//! UTF-8 text is decoded and mapped to CP437 where it can be. Replies
//! (device attributes, status and cursor position reports) are queued
//! for the caller to send.

#![no_std]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

/// A key the terminal turns into what it sends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    /// F1-F12.
    F(u8),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    /// ESC followed by an intermediate byte (`(`, `)`, `#`...).
    EscInter(u8),
    Csi,
    /// OSC/DCS/APC/PM strings: skipped up to BEL or ST.
    Str,
    StrEsc,
}

#[derive(Clone, Copy)]
struct Pen {
    fg: u8,
    bg: u8,
    bold: bool,
    underline: bool,
    blink: bool,
    reverse: bool,
    concealed: bool,
}

const PEN: Pen = Pen { fg: 7, bg: 0, bold: false, underline: false, blink: false, reverse: false, concealed: false };

#[derive(Clone, Copy)]
struct Saved {
    row: usize,
    col: usize,
    pen: Pen,
    origin: bool,
    g: [bool; 2],
    gl: usize,
}

/// ANSI colour numbers to PC (CGA) colour numbers.
const PC: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

pub struct Term {
    pub cols: usize,
    pub rows: usize,
    cells: Vec<u16>,
    dirty: Vec<bool>,
    row: usize,
    col: usize,
    wrap_pending: bool,
    pen: Pen,
    saved: Saved,
    top: usize,
    bottom: usize,
    origin: bool,
    autowrap: bool,
    insert: bool,
    /// LNM: LF also returns the carriage, Enter sends CR LF.
    pub newline_mode: bool,
    /// DECCKM: the cursor keys send ESC O x.
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub cursor_visible: bool,
    /// G0 and G1 are the DEC special graphics set.
    g: [bool; 2],
    gl: usize,
    tabs: Vec<bool>,
    state: State,
    params: Vec<u32>,
    param_any: bool,
    private: u8,
    inter: u8,
    utf8: u32,
    utf8_left: u8,
    /// Decode UTF-8 (on by default).
    pub utf8_enabled: bool,
    /// The colour underlined text gets (a PC colour; IBM TELNET's -u).
    pub underline_color: u8,
    reply: Vec<u8>,
    bells: u32,
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Term {
        let cols = cols.max(2);
        let rows = rows.max(2);
        let mut t = Term {
            cols,
            rows,
            cells: vec![0x0720; cols * rows],
            dirty: vec![true; rows],
            row: 0,
            col: 0,
            wrap_pending: false,
            pen: PEN,
            saved: Saved { row: 0, col: 0, pen: PEN, origin: false, g: [false; 2], gl: 0 },
            top: 0,
            bottom: rows - 1,
            origin: false,
            autowrap: true,
            insert: false,
            newline_mode: false,
            app_cursor: false,
            app_keypad: false,
            cursor_visible: true,
            g: [false; 2],
            gl: 0,
            tabs: vec![false; cols],
            state: State::Ground,
            params: Vec::new(),
            param_any: false,
            private: 0,
            inter: 0,
            utf8: 0,
            utf8_left: 0,
            utf8_enabled: true,
            underline_color: 0x0b,
            reply: Vec::new(),
            bells: 0,
        };
        t.reset_tabs();
        t
    }

    fn reset_tabs(&mut self) {
        for (i, t) in self.tabs.iter_mut().enumerate() {
            *t = i % 8 == 0 && i > 0;
        }
    }

    /// Full reset (RIS).
    pub fn reset(&mut self) {
        let (c, r) = (self.cols, self.rows);
        let keep = (self.utf8_enabled, self.underline_color);
        *self = Term::new(c, r);
        (self.utf8_enabled, self.underline_color) = keep;
    }

    /// Change the size (contents kept where they fit).
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.max(2), rows.max(2));
        let mut cells = vec![0x0720; cols * rows];
        for r in 0..rows.min(self.rows) {
            for c in 0..cols.min(self.cols) {
                cells[r * cols + c] = self.cells[r * self.cols + c];
            }
        }
        self.cells = cells;
        self.cols = cols;
        self.rows = rows;
        self.dirty = vec![true; rows];
        self.top = 0;
        self.bottom = rows - 1;
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
        self.tabs = vec![false; cols];
        self.reset_tabs();
    }

    /// The cell at (row, col): character in the low byte, attribute in the high.
    pub fn cell(&self, row: usize, col: usize) -> u16 {
        self.cells[row * self.cols + col]
    }

    /// One row's cells.
    pub fn row_cells(&self, row: usize) -> &[u16] {
        &self.cells[row * self.cols..(row + 1) * self.cols]
    }

    /// A row's text (CP437 bytes).
    pub fn row_text(&self, row: usize) -> Vec<u8> {
        self.row_cells(row).iter().map(|&c| c as u8).collect()
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col.min(self.cols - 1))
    }

    /// Rows changed since the last call (and forget them).
    pub fn take_dirty(&mut self) -> Vec<usize> {
        let v = (0..self.rows).filter(|&r| self.dirty[r]).collect();
        self.dirty.iter_mut().for_each(|d| *d = false);
        v
    }

    /// Replies to send to the host.
    pub fn take_reply(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.reply)
    }

    /// Bells rung since the last call.
    pub fn take_bells(&mut self) -> u32 {
        core::mem::take(&mut self.bells)
    }

    /// What a key sends.
    pub fn key(&self, k: Key) -> &'static [u8] {
        let app = self.app_cursor;
        match k {
            Key::Up => if app { b"\x1bOA" } else { b"\x1b[A" },
            Key::Down => if app { b"\x1bOB" } else { b"\x1b[B" },
            Key::Right => if app { b"\x1bOC" } else { b"\x1b[C" },
            Key::Left => if app { b"\x1bOD" } else { b"\x1b[D" },
            Key::Home => b"\x1b[1~",
            Key::Insert => b"\x1b[2~",
            Key::Delete => b"\x1b[3~",
            Key::End => b"\x1b[4~",
            Key::PageUp => b"\x1b[5~",
            Key::PageDown => b"\x1b[6~",
            Key::F(n) => match n {
                1 => b"\x1bOP",
                2 => b"\x1bOQ",
                3 => b"\x1bOR",
                4 => b"\x1bOS",
                5 => b"\x1b[15~",
                6 => b"\x1b[17~",
                7 => b"\x1b[18~",
                8 => b"\x1b[19~",
                9 => b"\x1b[20~",
                10 => b"\x1b[21~",
                11 => b"\x1b[23~",
                _ => b"\x1b[24~",
            },
        }
    }

    /// What Enter sends.
    pub fn enter(&self) -> &'static [u8] {
        if self.newline_mode { b"\r\n" } else { b"\r" }
    }

    fn attr(&self) -> u8 {
        let p = &self.pen;
        let mut fg = p.fg;
        let mut bg = p.bg;
        if p.underline && fg == 7 {
            fg = self.underline_color & 7;
        }
        if p.reverse {
            core::mem::swap(&mut fg, &mut bg);
        }
        let mut a = (fg & 7) | (bg & 7) << 4;
        if p.bold || (p.underline && !p.reverse && self.underline_color & 8 != 0) {
            a |= 0x08;
        }
        if p.blink {
            a |= 0x80;
        }
        if p.concealed {
            a = (a & 0xf0) | (a >> 4 & 7);
        }
        a
    }

    fn blank(&self) -> u16 {
        // Erased cells take the background colour, as on a VT220 with colour.
        let bg = if self.pen.reverse { self.pen.fg } else { self.pen.bg };
        0x20 | ((bg & 7) as u16) << 12 | 0x0700
    }

    fn mark(&mut self, r: usize) {
        if r < self.rows {
            self.dirty[r] = true;
        }
    }

    fn put_cell(&mut self, r: usize, c: usize, v: u16) {
        if r < self.rows && c < self.cols {
            self.cells[r * self.cols + c] = v;
            self.dirty[r] = true;
        }
    }

    /// Scroll rows top..=bottom up by n (new blank rows at the bottom).
    fn scroll_up(&mut self, top: usize, bottom: usize, n: usize) {
        let n = n.min(bottom + 1 - top);
        let w = self.cols;
        self.cells.copy_within((top + n) * w..(bottom + 1) * w, top * w);
        let b = self.blank();
        self.cells[(bottom + 1 - n) * w..(bottom + 1) * w].fill(b);
        for r in top..=bottom {
            self.dirty[r] = true;
        }
    }

    fn scroll_down(&mut self, top: usize, bottom: usize, n: usize) {
        let n = n.min(bottom + 1 - top);
        let w = self.cols;
        self.cells.copy_within(top * w..(bottom + 1 - n) * w, (top + n) * w);
        let b = self.blank();
        self.cells[top * w..(top + n) * w].fill(b);
        for r in top..=bottom {
            self.dirty[r] = true;
        }
    }

    fn linefeed(&mut self) {
        self.wrap_pending = false;
        if self.row == self.bottom {
            self.scroll_up(self.top, self.bottom, 1);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.row == self.top {
            self.scroll_down(self.top, self.bottom, 1);
        } else if self.row > 0 {
            self.row -= 1;
        }
    }

    fn print(&mut self, ch: u8) {
        if self.wrap_pending {
            self.wrap_pending = false;
            if self.autowrap {
                self.col = 0;
                self.linefeed();
            }
        }
        let a = self.attr();
        let (r, c) = (self.row, self.col.min(self.cols - 1));
        if self.insert {
            let w = self.cols;
            let base = r * w;
            self.cells.copy_within(base + c..base + w - 1, base + c + 1);
        }
        self.put_cell(r, c, ch as u16 | (a as u16) << 8);
        if c + 1 >= self.cols {
            self.wrap_pending = true;
            self.col = self.cols - 1;
        } else {
            self.col = c + 1;
        }
    }

    fn graphic(&self, b: u8) -> u8 {
        if !self.g[self.gl] || !(0x5f..=0x7e).contains(&b) {
            return b;
        }
        const DEC: [u8; 32] = [
            0x20, 0x04, 0xb1, 0x09, 0x0c, 0x0d, 0x0a, 0xf8, 0xf1, 0x0a, 0x0b, 0xd9, 0xbf, 0xda, 0xc0, 0xc5, 0xc4, 0xc4, 0xc4, 0xc4, 0x5f,
            0xc3, 0xb4, 0xc1, 0xc2, 0xb3, 0xf3, 0xf2, 0xe3, 0xd8, 0x9c, 0xfa,
        ];
        DEC[(b - 0x5f) as usize]
    }

    /// Feed bytes from the host.
    pub fn feed(&mut self, data: &[u8]) {
        for &b in data {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        // Controls act in any state (except inside strings).
        if b < 0x20 && !matches!(self.state, State::Str | State::StrEsc) {
            self.utf8_left = 0;
            self.control(b);
            return;
        }
        if b == 0x7f {
            return;
        }
        match self.state {
            State::Ground => self.text(b),
            State::Esc => self.esc(b),
            State::EscInter(i) => {
                self.state = State::Ground;
                match i {
                    b'(' | b')' => {
                        let n = (i == b')') as usize;
                        self.g[n] = b == b'0';
                    }
                    b'#' if b == b'8' => {
                        // DECALN: fill the screen with E.
                        for r in 0..self.rows {
                            for c in 0..self.cols {
                                self.put_cell(r, c, 0x0745);
                            }
                        }
                    }
                    _ => {}
                }
            }
            State::Csi => self.csi_byte(b),
            State::Str => {
                if b == 0x1b {
                    self.state = State::StrEsc;
                } else if b == 0x07 || b == 0x9c {
                    self.state = State::Ground;
                }
            }
            State::StrEsc => self.state = if b == b'\\' { State::Ground } else { State::Str },
        }
    }

    fn text(&mut self, b: u8) {
        if b < 0x80 || !self.utf8_enabled {
            self.utf8_left = 0;
            let ch = self.graphic(b);
            self.print(ch);
            return;
        }
        if self.utf8_left > 0 && b & 0xc0 == 0x80 {
            self.utf8 = self.utf8 << 6 | (b & 0x3f) as u32;
            self.utf8_left -= 1;
            if self.utf8_left == 0 {
                let ch = unicode_to_cp437(self.utf8);
                self.print(ch);
            }
            return;
        }
        let (v, n) = match b {
            0xc2..=0xdf => ((b & 0x1f) as u32, 1),
            0xe0..=0xef => ((b & 0x0f) as u32, 2),
            0xf0..=0xf4 => ((b & 0x07) as u32, 3),
            // Not UTF-8: a byte of the host's 8-bit set.
            _ => {
                self.utf8_left = 0;
                self.print(b);
                return;
            }
        };
        self.utf8 = v;
        self.utf8_left = n;
    }

    fn control(&mut self, b: u8) {
        match b {
            0x07 => {
                if self.state == State::Ground {
                    self.bells += 1;
                }
            }
            0x08 => {
                self.wrap_pending = false;
                self.col = self.col.min(self.cols - 1).saturating_sub(1);
            }
            0x09 => {
                self.wrap_pending = false;
                let mut c = self.col + 1;
                while c < self.cols - 1 && !self.tabs[c] {
                    c += 1;
                }
                self.col = c.min(self.cols - 1);
            }
            0x0a..=0x0c => {
                self.linefeed();
                if self.newline_mode {
                    self.col = 0;
                }
            }
            0x0d => {
                self.wrap_pending = false;
                self.col = 0;
            }
            0x0e => self.gl = 1,
            0x0f => self.gl = 0,
            0x18 | 0x1a => self.state = State::Ground,
            0x1b => {
                self.state = State::Esc;
            }
            _ => {}
        }
    }

    fn save(&mut self) {
        self.saved = Saved { row: self.row, col: self.col, pen: self.pen, origin: self.origin, g: self.g, gl: self.gl };
    }

    fn restore(&mut self) {
        let s = self.saved;
        self.row = s.row.min(self.rows - 1);
        self.col = s.col.min(self.cols - 1);
        self.pen = s.pen;
        self.origin = s.origin;
        self.g = s.g;
        self.gl = s.gl;
        self.wrap_pending = false;
    }

    fn esc(&mut self, b: u8) {
        self.state = State::Ground;
        match b {
            b'[' => {
                self.state = State::Csi;
                self.params.clear();
                self.param_any = false;
                self.private = 0;
                self.inter = 0;
            }
            b']' | b'P' | b'_' | b'^' | b'X' => self.state = State::Str,
            b'(' | b')' | b'#' | b'%' | b'*' | b'+' | b' ' => self.state = State::EscInter(b),
            b'7' => self.save(),
            b'8' => self.restore(),
            b'D' => self.linefeed(),
            b'E' => {
                self.col = 0;
                self.linefeed();
            }
            b'M' => self.reverse_index(),
            b'H' => {
                let c = self.col.min(self.cols - 1);
                self.tabs[c] = true;
            }
            b'c' => self.reset(),
            b'=' => self.app_keypad = true,
            b'>' => self.app_keypad = false,
            b'Z' => self.reply.extend_from_slice(b"\x1b[?1;2c"),
            _ => {}
        }
    }

    fn csi_byte(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => {
                if self.params.is_empty() {
                    self.params.push(0);
                }
                let p = self.params.last_mut().unwrap();
                *p = p.saturating_mul(10).saturating_add((b - b'0') as u32).min(9999);
                self.param_any = true;
            }
            b';' | b':' => {
                if self.params.is_empty() {
                    self.params.push(0);
                }
                self.params.push(0);
            }
            b'<'..=b'?' => self.private = b,
            0x20..=0x2f => self.inter = b,
            0x40..=0x7e => {
                self.state = State::Ground;
                self.csi(b);
            }
            _ => self.state = State::Ground,
        }
    }

    fn p(&self, i: usize, default: u32) -> u32 {
        match self.params.get(i) {
            Some(&v) if v != 0 => v,
            _ => default,
        }
    }

    fn home_row(&self) -> usize {
        if self.origin { self.top } else { 0 }
    }

    fn goto(&mut self, row: usize, col: usize) {
        self.wrap_pending = false;
        let (lo, hi) = if self.origin { (self.top, self.bottom) } else { (0, self.rows - 1) };
        self.row = (lo + row).min(hi);
        self.col = col.min(self.cols - 1);
    }

    fn erase(&mut self, r: usize, from: usize, to: usize) {
        let b = self.blank();
        for c in from..to.min(self.cols) {
            self.put_cell(r, c, b);
        }
    }

    fn csi(&mut self, f: u8) {
        let n = self.p(0, 1) as usize;
        let col = self.col.min(self.cols - 1);
        if self.private == b'?' {
            if f == b'h' || f == b'l' {
                let on = f == b'h';
                for i in 0..self.params.len().max(1) {
                    match self.params.get(i).copied().unwrap_or(0) {
                        1 => self.app_cursor = on,
                        6 => {
                            self.origin = on;
                            self.goto(0, 0);
                        }
                        7 => self.autowrap = on,
                        25 => self.cursor_visible = on,
                        47 | 1047 | 1049 => {
                            if f == b'h' && self.params.get(i) == Some(&1049) {
                                self.save();
                            }
                            let b = self.blank();
                            self.cells.fill(b);
                            self.dirty.iter_mut().for_each(|d| *d = true);
                            if f == b'l' && self.params.get(i) == Some(&1049) {
                                self.restore();
                            }
                        }
                        _ => {}
                    }
                }
            } else if f == b'n' && self.p(0, 0) == 6 {
                let s = alloc::format!("\x1b[?{};{}R", self.row + 1, col + 1);
                self.reply.extend_from_slice(s.as_bytes());
            }
            return;
        }
        if self.private == b'>' {
            if f == b'c' {
                self.reply.extend_from_slice(b"\x1b[>1;10;0c");
            }
            return;
        }
        if self.inter != 0 {
            // DECSTR (CSI ! p) and friends.
            if self.inter == b'!' && f == b'p' {
                self.pen = PEN;
                self.top = 0;
                self.bottom = self.rows - 1;
                self.origin = false;
                self.autowrap = true;
                self.insert = false;
                self.app_cursor = false;
            }
            return;
        }
        match f {
            b'A' => {
                let lo = if self.row >= self.top { self.top } else { 0 };
                self.row = self.row.saturating_sub(n).max(lo);
                self.wrap_pending = false;
            }
            b'B' | b'e' => {
                let hi = if self.row <= self.bottom { self.bottom } else { self.rows - 1 };
                self.row = (self.row + n).min(hi);
                self.wrap_pending = false;
            }
            b'C' | b'a' => {
                self.col = (col + n).min(self.cols - 1);
                self.wrap_pending = false;
            }
            b'D' => {
                self.col = col.saturating_sub(n);
                self.wrap_pending = false;
            }
            b'E' => {
                let hi = if self.row <= self.bottom { self.bottom } else { self.rows - 1 };
                self.row = (self.row + n).min(hi);
                self.col = 0;
                self.wrap_pending = false;
            }
            b'F' => {
                self.row = self.row.saturating_sub(n);
                self.col = 0;
                self.wrap_pending = false;
            }
            b'G' | b'`' => {
                self.col = (n - 1).min(self.cols - 1);
                self.wrap_pending = false;
            }
            b'd' => {
                let r = self.p(0, 1) as usize - 1;
                self.goto(r, col);
            }
            b'H' | b'f' => {
                let (r, c) = (self.p(0, 1) as usize - 1, self.p(1, 1) as usize - 1);
                self.goto(r, c);
            }
            b'J' => {
                let r = self.row;
                match self.p(0, 0) {
                    0 => {
                        self.erase(r, col, self.cols);
                        for rr in r + 1..self.rows {
                            self.erase(rr, 0, self.cols);
                        }
                    }
                    1 => {
                        for rr in 0..r {
                            self.erase(rr, 0, self.cols);
                        }
                        self.erase(r, 0, col + 1);
                    }
                    _ => {
                        for rr in 0..self.rows {
                            self.erase(rr, 0, self.cols);
                        }
                    }
                }
            }
            b'K' => {
                let r = self.row;
                match self.p(0, 0) {
                    0 => self.erase(r, col, self.cols),
                    1 => self.erase(r, 0, col + 1),
                    _ => self.erase(r, 0, self.cols),
                }
            }
            b'X' => {
                let r = self.row;
                self.erase(r, col, col + n);
            }
            b'L' | b'M' => {
                if self.row >= self.top && self.row <= self.bottom {
                    if f == b'L' {
                        self.scroll_down(self.row, self.bottom, n);
                    } else {
                        self.scroll_up(self.row, self.bottom, n);
                    }
                    self.col = 0;
                }
            }
            b'P' => {
                let (r, w) = (self.row, self.cols);
                let n = n.min(w - col);
                let base = r * w;
                self.cells.copy_within(base + col + n..base + w, base + col);
                let b = self.blank();
                self.cells[base + w - n..base + w].fill(b);
                self.mark(r);
            }
            b'@' => {
                let (r, w) = (self.row, self.cols);
                let n = n.min(w - col);
                let base = r * w;
                self.cells.copy_within(base + col..base + w - n, base + col + n);
                let b = self.blank();
                self.cells[base + col..base + col + n].fill(b);
                self.mark(r);
            }
            b'S' => self.scroll_up(self.top, self.bottom, n),
            b'T' => self.scroll_down(self.top, self.bottom, n),
            b'g' => match self.p(0, 0) {
                0 => self.tabs[col] = false,
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            b'h' | b'l' => {
                let on = f == b'h';
                for &v in &self.params {
                    match v {
                        4 => self.insert = on,
                        20 => self.newline_mode = on,
                        _ => {}
                    }
                }
            }
            b'm' => self.sgr(),
            b'n' => match self.p(0, 0) {
                5 => self.reply.extend_from_slice(b"\x1b[0n"),
                6 => {
                    let r = self.row + 1 - self.home_row();
                    let s = alloc::format!("\x1b[{};{}R", r, col + 1);
                    self.reply.extend_from_slice(s.as_bytes());
                }
                _ => {}
            },
            b'c' => {
                if self.p(0, 0) == 0 {
                    // A VT220 with advanced video.
                    self.reply.extend_from_slice(b"\x1b[?62;1;6c");
                }
            }
            b'r' => {
                let top = self.p(0, 1) as usize - 1;
                let bottom = (self.p(1, self.rows as u32) as usize - 1).min(self.rows - 1);
                if top < bottom {
                    self.top = top;
                    self.bottom = bottom;
                    self.goto(0, 0);
                }
            }
            b's' => self.save(),
            b'u' => self.restore(),
            _ => {}
        }
    }

    fn sgr(&mut self) {
        let params: Vec<u32> = if self.params.is_empty() { vec![0] } else { self.params.clone() };
        let mut i = 0;
        while i < params.len() {
            let v = params[i];
            match v {
                0 => self.pen = PEN,
                1 => self.pen.bold = true,
                2 | 22 => self.pen.bold = false,
                4 => self.pen.underline = true,
                24 => self.pen.underline = false,
                5 | 6 => self.pen.blink = true,
                25 => self.pen.blink = false,
                7 => self.pen.reverse = true,
                27 => self.pen.reverse = false,
                8 => self.pen.concealed = true,
                28 => self.pen.concealed = false,
                30..=37 => self.pen.fg = PC[(v - 30) as usize],
                39 => self.pen.fg = 7,
                40..=47 => self.pen.bg = PC[(v - 40) as usize],
                49 => self.pen.bg = 0,
                90..=97 => {
                    self.pen.fg = PC[(v - 90) as usize];
                    self.pen.bold = true;
                }
                100..=107 => self.pen.bg = PC[(v - 100) as usize],
                // 256-colour and RGB forms: take the nearest of the eight.
                38 | 48 => {
                    let c = match params.get(i + 1) {
                        Some(5) => {
                            let n = params.get(i + 2).copied().unwrap_or(0);
                            i += 2;
                            if n < 16 { PC[(n & 7) as usize] } else { 7 }
                        }
                        Some(2) => {
                            let rgb = [2, 3, 4].map(|k| params.get(i + k).copied().unwrap_or(0));
                            i += 4;
                            ((rgb[0] > 127) as u8) << 2 | ((rgb[1] > 127) as u8) << 1 | (rgb[2] > 127) as u8
                        }
                        _ => 7,
                    };
                    if v == 38 {
                        self.pen.fg = c;
                    } else {
                        self.pen.bg = c;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// A Unicode character in code page 437 ('?' when it has none).
pub fn unicode_to_cp437(u: u32) -> u8 {
    const HIGH: [u16; 128] = [
        0x00c7, 0x00fc, 0x00e9, 0x00e2, 0x00e4, 0x00e0, 0x00e5, 0x00e7, 0x00ea, 0x00eb, 0x00e8, 0x00ef, 0x00ee, 0x00ec, 0x00c4, 0x00c5,
        0x00c9, 0x00e6, 0x00c6, 0x00f4, 0x00f6, 0x00f2, 0x00fb, 0x00f9, 0x00ff, 0x00d6, 0x00dc, 0x00a2, 0x00a3, 0x00a5, 0x20a7, 0x0192,
        0x00e1, 0x00ed, 0x00f3, 0x00fa, 0x00f1, 0x00d1, 0x00aa, 0x00ba, 0x00bf, 0x2310, 0x00ac, 0x00bd, 0x00bc, 0x00a1, 0x00ab, 0x00bb,
        0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x2561, 0x2562, 0x2556, 0x2555, 0x2563, 0x2551, 0x2557, 0x255d, 0x255c, 0x255b, 0x2510,
        0x2514, 0x2534, 0x252c, 0x251c, 0x2500, 0x253c, 0x255e, 0x255f, 0x255a, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256c, 0x2567,
        0x2568, 0x2564, 0x2565, 0x2559, 0x2558, 0x2552, 0x2553, 0x256b, 0x256a, 0x2518, 0x250c, 0x2588, 0x2584, 0x258c, 0x2590, 0x2580,
        0x03b1, 0x00df, 0x0393, 0x03c0, 0x03a3, 0x03c3, 0x00b5, 0x03c4, 0x03a6, 0x0398, 0x03a9, 0x03b4, 0x221e, 0x03c6, 0x03b5, 0x2229,
        0x2261, 0x00b1, 0x2265, 0x2264, 0x2320, 0x2321, 0x00f7, 0x2248, 0x00b0, 0x2219, 0x00b7, 0x221a, 0x207f, 0x00b2, 0x25a0, 0x00a0,
    ];
    if u < 0x80 {
        return u as u8;
    }
    if let Some(i) = HIGH.iter().position(|&c| c as u32 == u) {
        return 0x80 + i as u8;
    }
    // Lookalikes for common characters CP437 lacks.
    match u {
        0x2018 | 0x2019 | 0x00b4 => b'\'',
        0x201c | 0x201d => b'"',
        0x2013 | 0x2014 | 0x2212 => b'-',
        0x2026 => b'.',
        0x2022 => 0x07,
        0x250f | 0x256d => 0xda,
        0x2513 | 0x256e => 0xbf,
        0x2517 | 0x2570 => 0xc0,
        0x251b | 0x256f => 0xd9,
        0x2501 => 0xc4,
        0x2503 => 0xb3,
        0x25b2 => 0x1e,
        0x25bc => 0x1f,
        0x25c6 => 0x04,
        0x2192 => 0x1a,
        0x2190 => 0x1b,
        0x2191 => 0x18,
        0x2193 => 0x19,
        0x00a9 => b'c',
        0x00ae => b'r',
        _ => b'?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &Term, r: usize) -> alloc::string::String {
        t.row_text(r).iter().map(|&b| b as char).collect::<alloc::string::String>().trim_end().into()
    }

    #[test]
    fn plain_text_and_wrap() {
        let mut t = Term::new(10, 3);
        t.feed(b"hello\r\nworld 12345X");
        assert_eq!(text(&t, 0), "hello");
        assert_eq!(text(&t, 1), "world 1234");
        assert_eq!(text(&t, 2), "5X");
        t.feed(b"\r\nnext");
        assert_eq!(text(&t, 0), "world 1234", "scrolled");
        assert_eq!(text(&t, 2), "next");
    }

    #[test]
    fn cursor_moves_and_erase() {
        let mut t = Term::new(20, 5);
        t.feed(b"\x1b[2J\x1b[3;5HX\x1b[1;1Habc\x1b[K");
        assert_eq!(text(&t, 2), "    X");
        assert_eq!(text(&t, 0), "abc");
        t.feed(b"\x1b[3;1H\x1b[2K");
        assert_eq!(text(&t, 2), "");
        t.feed(b"\x1b[5;20H\x1b[6n");
        assert_eq!(t.take_reply(), b"\x1b[5;20R");
        t.feed(b"\x1b[c");
        assert_eq!(t.take_reply(), b"\x1b[?62;1;6c");
    }

    #[test]
    fn colours() {
        let mut t = Term::new(10, 2);
        t.feed(b"\x1b[1;31;44mR\x1b[0mN\x1b[7mV");
        assert_eq!(t.cell(0, 0), b'R' as u16 | 0x1c << 8);
        assert_eq!(t.cell(0, 1), b'N' as u16 | 0x07 << 8);
        assert_eq!(t.cell(0, 2), b'V' as u16 | 0x70 << 8);
    }

    #[test]
    fn scroll_region() {
        let mut t = Term::new(5, 5);
        for i in 0..5 {
            t.feed(alloc::format!("\x1b[{};1H{}", i + 1, i).as_bytes());
        }
        t.feed(b"\x1b[2;4r\x1b[4;1H\n");
        assert_eq!(text(&t, 0), "0");
        assert_eq!(text(&t, 1), "2");
        assert_eq!(text(&t, 2), "3");
        assert_eq!(text(&t, 3), "");
        assert_eq!(text(&t, 4), "4");
        t.feed(b"\x1b[2;1H\x1bM");
        assert_eq!(text(&t, 1), "");
        assert_eq!(text(&t, 2), "2");
        t.feed(b"\x1b[r\x1b[2;1H\x1b[M");
        assert_eq!(text(&t, 1), "2");
    }

    #[test]
    fn line_drawing_and_utf8() {
        let mut t = Term::new(10, 2);
        t.feed(b"\x1b(0lqk\x1b(Bx");
        assert_eq!(t.row_text(0)[..4], [0xda, 0xc4, 0xbf, b'x']);
        t.feed("\r\n\u{e9}\u{2500}\u{2588}".as_bytes());
        assert_eq!(t.row_text(1)[..3], [0x82, 0xc4, 0xdb]);
    }

    #[test]
    fn insert_delete_chars_and_osc() {
        let mut t = Term::new(10, 2);
        t.feed(b"abcdef\x1b[1;2H\x1b[2P");
        assert_eq!(text(&t, 0), "adef");
        t.feed(b"\x1b[2@");
        assert_eq!(text(&t, 0), "a  def");
        t.feed(b"\x1b]0;title\x07Z");
        assert_eq!(text(&t, 0), "aZ def");
        assert!(!t.take_dirty().is_empty());
        assert!(t.take_dirty().is_empty());
    }

    #[test]
    fn keys() {
        let mut t = Term::new(10, 2);
        assert_eq!(t.key(Key::Up), b"\x1b[A");
        t.feed(b"\x1b[?1h");
        assert_eq!(t.key(Key::Up), b"\x1bOA");
    }
}
