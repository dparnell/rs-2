//! Text cells on a pixel display: what a VIO window (and, later, an AVIO
//! presentation space) needs that does not depend on PM.
//!
//! * [`render_bgr24`] draws a range of cells as 24-bit pixels (OS/2
//!   bitmap order: blue, green, red; optionally bottom-up) with an 8-pixel
//!   wide bitmap font of any height, the 16-colour palette of the
//!   session, attribute bit 7 as blink or bright background, a cursor
//!   (scan lines) and a marked block shown inverted.
//! * [`block_text`] and [`all_text`] turn cells into CF_TEXT (trailing
//!   blanks dropped, CR LF line ends) for Copy and Copy All.
//! * [`paste_keys`] turns CF_TEXT into keystrokes (character and US
//!   scan code) for Paste.
//!
//! Cells are VGA text cells: character in the low byte, attribute in the
//! high byte (foreground in bits 0-3, background in 4-6, bit 7 blink or
//! bright background).

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::ops::Range;

/// Width of a character cell in pixels.
pub const CELL_W: usize = 8;

/// A screen of cells, row by row.
#[derive(Clone, Copy)]
pub struct Grid<'a> {
    pub cells: &'a [u16],
    pub cols: usize,
    pub rows: usize,
}

impl Grid<'_> {
    /// The cell at (`row`, `col`); a blank outside the cells given.
    pub fn cell(&self, row: usize, col: usize) -> u16 {
        if row >= self.rows || col >= self.cols {
            return 0x0720;
        }
        self.cells.get(row * self.cols + col).copied().unwrap_or(0x0720)
    }
}

/// A font of 256 glyphs, `height` bytes each, bit 7 the leftmost pixel.
#[derive(Clone, Copy)]
pub struct Font<'a> {
    pub glyphs: &'a [u8],
    pub height: usize,
}

impl Font<'_> {
    fn bits(&self, ch: u8, y: usize) -> u8 {
        self.glyphs.get(ch as usize * self.height + y).copied().unwrap_or(0)
    }
}

/// Marked cells (inclusive): a rectangular block, as VIO windows mark
/// text, or with `lines` the text from (`top`, `left`) to (`bottom`,
/// `right`) in reading order, as terminals select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub top: usize,
    pub left: usize,
    pub bottom: usize,
    pub right: usize,
    pub lines: bool,
}

impl Block {
    /// The block with corners `a` and `b` (row, column), in any order.
    pub fn spanning(a: (usize, usize), b: (usize, usize)) -> Block {
        Block { top: a.0.min(b.0), left: a.1.min(b.1), bottom: a.0.max(b.0), right: a.1.max(b.1), lines: false }
    }

    /// The text between cells `a` and `b` (row, column), in any order.
    pub fn text(a: (usize, usize), b: (usize, usize)) -> Block {
        let (s, e) = if a <= b { (a, b) } else { (b, a) };
        Block { top: s.0, left: s.1, bottom: e.0, right: e.1, lines: true }
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        if self.lines {
            return (row, col) >= (self.top, self.left) && (row, col) <= (self.bottom, self.right);
        }
        row >= self.top && row <= self.bottom && col >= self.left && col <= self.right
    }

    /// The part inside a `rows` x `cols` screen, if any.
    pub fn clamp(self, rows: usize, cols: usize) -> Option<Block> {
        if rows == 0 || cols == 0 || self.top >= rows || self.left >= cols {
            return None;
        }
        let right = if self.lines && self.bottom >= rows { cols - 1 } else { self.right.min(cols - 1) };
        Some(Block { bottom: self.bottom.min(rows - 1), right, ..self })
    }

    /// The columns of `row` marked (`row` inside the block).
    fn columns(&self, row: usize, cols: usize) -> (usize, usize) {
        if !self.lines {
            return (self.left, self.right);
        }
        let first = if row == self.top { self.left } else { 0 };
        let last = if row == self.bottom { self.right } else { cols - 1 };
        (first, last)
    }
}

/// The text cursor: its cell and the scan lines it covers (in the
/// font's height; `first` > `last` wraps around, as on a VGA).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub first: usize,
    pub last: usize,
}

impl Cursor {
    fn covers(&self, y: usize) -> bool {
        if self.first <= self.last { y >= self.first && y <= self.last } else { y >= self.first || y <= self.last }
    }
}

/// Scan lines `first..=last` of a cell `from` lines high, scaled to a
/// cell `to` lines high (a VIO cursor shape on another font).
pub fn scale_lines(first: usize, last: usize, from: usize, to: usize) -> (usize, usize) {
    if from == 0 || to == 0 || from == to {
        return (first.min(to.saturating_sub(1)), last.min(to.saturating_sub(1)));
    }
    let f = |v: usize| ((v * to + from / 2) / from).min(to - 1);
    // Keep a cursor on the last line on the last line.
    let last = if last + 1 >= from { to - 1 } else { f(last) };
    (f(first), last)
}

/// How cells are drawn.
#[derive(Clone, Copy)]
pub struct Style {
    /// The 16 attribute colours, 0x00RRGGBB.
    pub palette: [u32; 16],
    /// Attribute bit 7 blinks (true) or brightens the background.
    pub blink: bool,
    /// Blinking characters are in their invisible phase.
    pub blink_off: bool,
    pub cursor: Option<Cursor>,
    /// Marked cells, shown with foreground and background swapped.
    pub mark: Option<Block>,
}

/// Foreground and background colour indices of a cell at (`row`, `col`).
pub fn cell_colors(cell: u16, style: &Style, row: usize, col: usize) -> (u8, u8) {
    let attr = (cell >> 8) as u8;
    let mut fg = attr & 15;
    let mut bg = (attr >> 4) & if style.blink { 7 } else { 15 };
    if style.blink && attr & 0x80 != 0 && style.blink_off {
        fg = bg;
    }
    if style.mark.is_some_and(|m| m.contains(row, col)) {
        core::mem::swap(&mut fg, &mut bg);
        if fg == bg {
            fg ^= 0x0f;
        }
    }
    (fg, bg)
}

/// Bytes per pixel row of a 24-bit bitmap `width` pixels wide (rows are
/// padded to 4 bytes).
pub fn bgr24_stride(width: usize) -> usize {
    (width * 3 + 3) & !3
}

/// Draw the cells of `rows` x `cols` (cell ranges) into `out` as 24-bit
/// pixels (blue, green, red), `stride` bytes per pixel row: the band is
/// `cols.len() * 8` pixels wide and `rows.len() * font.height` high, its
/// top row first, or last when `bottom_up` (OS/2 bitmaps).
pub fn render_bgr24(grid: &Grid, font: &Font, style: &Style, rows: Range<usize>, cols: Range<usize>, out: &mut [u8], stride: usize, bottom_up: bool) {
    let h = font.height;
    let band = rows.len() * h;
    let bgr = |c: u32| [c as u8, (c >> 8) as u8, (c >> 16) as u8];
    for (ri, row) in rows.clone().enumerate() {
        for (ci, col) in cols.clone().enumerate() {
            let cell = grid.cell(row, col);
            let (fg, bg) = cell_colors(cell, style, row, col);
            let (fg, bg) = (bgr(style.palette[fg as usize]), bgr(style.palette[bg as usize]));
            let cursor = style.cursor.filter(|c| c.row == row && c.col == col);
            for y in 0..h {
                let mut bits = font.bits(cell as u8, y);
                if cursor.is_some_and(|c| c.covers(y)) {
                    bits = 0xff;
                }
                let py = ri * h + y;
                let line = if bottom_up { band - 1 - py } else { py };
                let at = line * stride + ci * CELL_W * 3;
                let Some(px) = out.get_mut(at..at + CELL_W * 3) else { continue };
                for x in 0..CELL_W {
                    let c = if bits & (0x80 >> x) != 0 { &fg } else { &bg };
                    px[x * 3..x * 3 + 3].copy_from_slice(c);
                }
            }
        }
    }
}

/// The text of a block of cells: one line per row with its trailing
/// blanks removed, CR LF after each line (in a text mark, not after a last
/// line that stops before the end of its row). NUL characters read as
/// blanks.
pub fn block_text(grid: &Grid, block: Block) -> Vec<u8> {
    let mut out = Vec::new();
    let Some(b) = block.clamp(grid.rows, grid.cols) else { return out };
    for row in b.top..=b.bottom {
        let (first, last) = b.columns(row, grid.cols);
        let mut line: Vec<u8> = (first..=last).map(|c| grid.cell(row, c) as u8).map(|ch| if ch == 0 { b' ' } else { ch }).collect();
        while line.last() == Some(&b' ') {
            line.pop();
        }
        out.extend_from_slice(&line);
        if !b.lines || row < b.bottom || last == grid.cols - 1 {
            out.extend_from_slice(b"\r\n");
        }
    }
    out
}

/// The text of the whole screen (Copy All): every row as in
/// [`block_text`], without the empty rows at the bottom.
pub fn all_text(grid: &Grid) -> Vec<u8> {
    if grid.rows == 0 || grid.cols == 0 {
        return Vec::new();
    }
    let mut t = block_text(grid, Block::spanning((0, 0), (grid.rows - 1, grid.cols - 1)));
    while t.ends_with(b"\r\n\r\n") {
        t.truncate(t.len() - 2);
    }
    if t == b"\r\n" {
        t.clear();
    }
    t
}

// Set-1 scan codes of the US layout for printable ASCII, unshifted and
// shifted (as the keyboard driver's own tables).
const NORMAL: &[u8; 0x3a] = b"\x00\x1b1234567890-=\x08\tqwertyuiop[]\r\x00asdfghjkl;'`\x00\\zxcvbnm,./\x00*\x00 ";
const SHIFTED: &[u8; 0x3a] = b"\x00\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\r\x00ASDFGHJKL:\"~\x00|ZXCVBNM<>?\x00*\x00 ";

/// Scan code of the key that types `ch` on a US keyboard (0 if none).
pub fn scan_of(ch: u8) -> u8 {
    if ch == 0 {
        return 0;
    }
    NORMAL.iter().position(|&c| c == ch).or_else(|| SHIFTED.iter().position(|&c| c == ch)).map_or(0, |p| p as u8)
}

/// Keystrokes (character, scan code) that type CF_TEXT `text`: CR LF,
/// a lone CR or a lone LF is Enter, a tab is Tab; other control
/// characters are left out; the text ends at a NUL.
pub fn paste_keys(text: &[u8]) -> Vec<(u8, u8)> {
    let mut v = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let ch = text[i];
        i += 1;
        match ch {
            0 => break,
            b'\r' => {
                if text.get(i) == Some(&b'\n') {
                    i += 1;
                }
                v.push((b'\r', 0x1c));
            }
            b'\n' => v.push((b'\r', 0x1c)),
            b'\t' => v.push((b'\t', 0x0f)),
            c if c < 0x20 || c == 0x7f => {}
            c => v.push((c, scan_of(c))),
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn font() -> Vec<u8> {
        // Glyph 'A': a full first row and a dot in the second; others empty.
        let mut f = vec![0u8; 256 * 4];
        f[b'A' as usize * 4] = 0xff;
        f[b'A' as usize * 4 + 1] = 0x80;
        f
    }

    fn style() -> Style {
        let mut palette = [0u32; 16];
        for (i, p) in palette.iter_mut().enumerate() {
            *p = (i as u32) * 0x010101;
        }
        Style { palette, blink: true, blink_off: false, cursor: None, mark: None }
    }

    fn px(out: &[u8], stride: usize, x: usize, y: usize) -> u8 {
        out[y * stride + x * 3]
    }

    #[test]
    fn renders_glyph_and_colours() {
        let cells = [0x1f00 | b'A' as u16, 0x0720];
        let g = Grid { cells: &cells, cols: 2, rows: 1 };
        let f = font();
        let font = Font { glyphs: &f, height: 4 };
        let stride = bgr24_stride(16);
        let mut out = vec![0xaa; stride * 4];
        render_bgr24(&g, &font, &style(), 0..1, 0..2, &mut out, stride, false);
        // Row 0 of 'A': foreground 15 across the cell.
        assert!((0..8).all(|x| px(&out, stride, x, 0) == 15));
        // Row 1: one foreground pixel, then background 1.
        assert_eq!(px(&out, stride, 0, 1), 15);
        assert_eq!(px(&out, stride, 1, 1), 1);
        // The blank cell: background 0 everywhere.
        assert!((8..16).all(|x| (0..4).all(|y| px(&out, stride, x, y) == 0)));
    }

    #[test]
    fn bottom_up_reverses_rows() {
        let cells = [0x0f00 | b'A' as u16];
        let g = Grid { cells: &cells, cols: 1, rows: 1 };
        let f = font();
        let font = Font { glyphs: &f, height: 4 };
        let stride = bgr24_stride(8);
        let mut out = vec![0; stride * 4];
        render_bgr24(&g, &font, &style(), 0..1, 0..1, &mut out, stride, true);
        assert_eq!(px(&out, stride, 7, 3), 15);
        assert_eq!(px(&out, stride, 7, 0), 0);
    }

    #[test]
    fn blink_and_intensity() {
        let s = Style { blink_off: true, ..style() };
        // Blinking: bit 7 hides the character in the off phase.
        assert_eq!(cell_colors(0x9e41, &s, 0, 0), (1, 1));
        // Intensity: bit 7 is a bright background.
        let s = Style { blink: false, ..s };
        assert_eq!(cell_colors(0x9e41, &s, 0, 0), (14, 9));
    }

    #[test]
    fn mark_swaps_colours() {
        let s = Style { mark: Some(Block::spanning((1, 3), (0, 1))), ..style() };
        assert_eq!(cell_colors(0x1700, &s, 0, 2), (1, 7));
        assert_eq!(cell_colors(0x1700, &s, 0, 0), (7, 1));
        // Same colours: still visible.
        assert_eq!(cell_colors(0x0000, &s, 1, 1), (15, 0));
    }

    #[test]
    fn cursor_lines() {
        let cells = [0x0700];
        let g = Grid { cells: &cells, cols: 1, rows: 1 };
        let f = font();
        let font = Font { glyphs: &f, height: 4 };
        let stride = bgr24_stride(8);
        let mut out = vec![0; stride * 4];
        let s = Style { cursor: Some(Cursor { row: 0, col: 0, first: 2, last: 3 }), ..style() };
        render_bgr24(&g, &font, &s, 0..1, 0..1, &mut out, stride, false);
        assert_eq!(px(&out, stride, 0, 1), 0);
        assert_eq!(px(&out, stride, 0, 2), 7);
        assert_eq!(px(&out, stride, 5, 3), 7);
        assert_eq!(scale_lines(14, 15, 16, 8), (7, 7));
        assert_eq!(scale_lines(0, 15, 16, 8), (0, 7));
        assert_eq!(scale_lines(12, 13, 14, 16), (14, 15));
    }

    #[test]
    fn copy_text() {
        let mut cells = vec![0x0720u16; 4 * 3];
        for (i, &b) in b"ab  ".iter().enumerate() {
            cells[i] = 0x0700 | b as u16;
        }
        for (i, &b) in b" xy\0".iter().enumerate() {
            cells[4 + i] = 0x0700 | b as u16;
        }
        let g = Grid { cells: &cells, cols: 4, rows: 3 };
        assert_eq!(block_text(&g, Block::spanning((0, 0), (1, 3))), b"ab\r\n xy\r\n");
        assert_eq!(block_text(&g, Block::spanning((1, 2), (0, 1))), b"b\r\nxy\r\n");
        assert_eq!(all_text(&g), b"ab\r\n xy\r\n");
        // Outside the screen.
        assert_eq!(block_text(&g, Block::spanning((5, 5), (6, 6))), b"");
        // Text marks: from a cell to a cell in reading order.
        assert_eq!(block_text(&g, Block::text((1, 2), (0, 1))), b"b\r\n xy");
        assert_eq!(block_text(&g, Block::text((0, 1), (0, 1))), b"b");
        assert_eq!(block_text(&g, Block::text((0, 0), (1, 3))), b"ab\r\n xy\r\n");
        let m = Block::text((0, 2), (1, 1));
        assert!(m.contains(0, 3) && m.contains(1, 0) && !m.contains(0, 1) && !m.contains(1, 2));
        let blank = vec![0x0720u16; 4];
        assert_eq!(all_text(&Grid { cells: &blank, cols: 2, rows: 2 }), b"");
    }

    #[test]
    fn paste() {
        assert_eq!(paste_keys(b"dir\r\nA\tb\nc\r"), vec![(b'd', 0x20), (b'i', 0x17), (b'r', 0x13), (b'\r', 0x1c), (b'A', 0x1e), (b'\t', 0x0f), (b'b', 0x30), (b'\r', 0x1c), (b'c', 0x2e), (b'\r', 0x1c)]);
        assert_eq!(paste_keys(b"x\x01y\0z"), vec![(b'x', 0x2d), (b'y', 0x15)]);
        assert_eq!(scan_of(b'?'), 0x35);
        assert_eq!(scan_of(0x82), 0);
    }
}
