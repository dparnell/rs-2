//! The physical text screen.
//!
//! Modern machines boot without a VGA text mode, so the screen is a grid of
//! (character, attribute) cells, exactly like VGA text memory, rendered into
//! the linear framebuffer with an 8x16 font. Kernel messages and VIO
//! sessions both draw into it.

use bootinfo::{Framebuffer, PHYS_MAP_BASE};

use crate::codepage::CodePage;
use crate::sync::SpinLock;

pub const CELL_W: usize = 8;
pub const CELL_H: usize = 16;
pub const MAX_COLS: usize = 132;
pub const MAX_ROWS: usize = 60;

/// The standard VGA palette (RGB).
const PALETTE: [u32; 16] = [
    0x000000, 0x0000aa, 0x00aa00, 0x00aaaa, 0xaa0000, 0xaa00aa, 0xaa5500, 0xaaaaaa,
    0x555555, 0x5555ff, 0x55ff55, 0x55ffff, 0xff5555, 0xff55ff, 0xffff55, 0xffffff,
];

static FONT: &[u8] = include_bytes!("../assets/unscii-16.bin");

fn font_glyph(c: char) -> Option<&'static [u8]> {
    let n = FONT.len() / 20;
    let key = c as u32;
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let rec = &FONT[mid * 20..mid * 20 + 20];
        let cp = u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]);
        match cp.cmp(&key) {
            core::cmp::Ordering::Equal => return Some(&rec[4..]),
            core::cmp::Ordering::Less => lo = mid + 1,
            core::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

pub struct Screen {
    fb: Framebuffer,
    pixels: *mut u32,
    pub cols: usize,
    pub rows: usize,
    /// Pixel origin of the text area (centred in the framebuffer).
    x0: usize,
    y0: usize,
    cells: [[u16; MAX_COLS]; MAX_ROWS],
    glyphs: [[u8; CELL_H]; 256],
    codepage: CodePage,
    palette: [u32; 16],
    /// Text cursor for kernel console output.
    pub cur_row: usize,
    pub cur_col: usize,
    /// Hardware-style cursor shown for VIO.
    cursor: Option<(usize, usize)>,
    /// Attribute used by the kernel console.
    pub attr: u8,
}

unsafe impl Send for Screen {}

pub static SCREEN: SpinLock<Option<&'static mut Screen>> = SpinLock::new(None);

/// Backing storage for the screen state (too big for the boot stack).
static mut SCREEN_STATE: core::mem::MaybeUninit<Screen> = core::mem::MaybeUninit::uninit();

impl Screen {
    /// Initialise the screen in place.
    fn init_at(slot: &'static mut core::mem::MaybeUninit<Screen>, fb: Framebuffer) -> &'static mut Screen {
        let s = slot.as_mut_ptr();
        unsafe {
            core::ptr::addr_of_mut!((*s).cells).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*s).glyphs).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*s).fb).write(fb);
            core::ptr::addr_of_mut!((*s).pixels).write((PHYS_MAP_BASE + fb.base) as *mut u32);
            core::ptr::addr_of_mut!((*s).cols).write(80);
            core::ptr::addr_of_mut!((*s).rows).write(25);
            core::ptr::addr_of_mut!((*s).x0).write(0);
            core::ptr::addr_of_mut!((*s).y0).write(0);
            core::ptr::addr_of_mut!((*s).codepage).write(CodePage::Cp850);
            core::ptr::addr_of_mut!((*s).palette).write([0; 16]);
            core::ptr::addr_of_mut!((*s).cur_row).write(0);
            core::ptr::addr_of_mut!((*s).cur_col).write(0);
            core::ptr::addr_of_mut!((*s).cursor).write(None);
            core::ptr::addr_of_mut!((*s).attr).write(0x07);
        }
        let s = unsafe { &mut *s };
        for row in s.cells.iter_mut() {
            row.fill(0x0720);
        }
        for (i, &rgb) in PALETTE.iter().enumerate() {
            let (r, g, b) = (rgb >> 16, (rgb >> 8) & 0xff, rgb & 0xff);
            s.palette[i] = if fb.format == 0 { r | g << 8 | b << 16 } else { rgb };
        }
        s.set_codepage(CodePage::Cp850);
        s.set_mode(80, 25);
        s
    }

    pub fn codepage(&self) -> CodePage {
        self.codepage
    }

    pub fn set_codepage(&mut self, cp: CodePage) {
        self.codepage = cp;
        for b in 0..256 {
            let c = cp.glyph(b as u8);
            let g = font_glyph(c).or_else(|| font_glyph('?')).unwrap();
            self.glyphs[b].copy_from_slice(g);
        }
        self.redraw();
    }

    /// Change the text geometry, clamped to what fits the framebuffer.
    pub fn set_mode(&mut self, cols: usize, rows: usize) {
        let max_c = (self.fb.width as usize / CELL_W).min(MAX_COLS);
        let max_r = (self.fb.height as usize / CELL_H).min(MAX_ROWS);
        self.cols = cols.min(max_c).max(1);
        self.rows = rows.min(max_r).max(1);
        self.x0 = (self.fb.width as usize - self.cols * CELL_W) / 2;
        self.y0 = (self.fb.height as usize - self.rows * CELL_H) / 2;
        // Clear the whole framebuffer so no stale border remains.
        let total = self.fb.stride as usize * self.fb.height as usize;
        for i in 0..total {
            unsafe { self.pixels.add(i).write_volatile(0) };
        }
        self.cur_row = self.cur_row.min(self.rows - 1);
        self.cur_col = 0;
        self.redraw();
    }

    pub fn cell(&self, row: usize, col: usize) -> u16 {
        self.cells[row][col]
    }

    /// Store a (char | attr << 8) cell and draw it.
    pub fn put(&mut self, row: usize, col: usize, cell: u16) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        self.cells[row][col] = cell;
        self.draw(row, col);
    }

    fn draw(&mut self, row: usize, col: usize) {
        let cell = self.cells[row][col];
        let ch = (cell & 0xff) as usize;
        let attr = (cell >> 8) as usize;
        let fg = self.palette[attr & 15];
        let bg = self.palette[(attr >> 4) & 15];
        let show_cursor = self.cursor == Some((row, col));
        let stride = self.fb.stride as usize;
        let px = self.x0 + col * CELL_W;
        let py = self.y0 + row * CELL_H;
        let glyph = &self.glyphs[ch];
        for y in 0..CELL_H {
            let mut bits = glyph[y];
            if show_cursor && y >= CELL_H - 3 {
                bits = 0xff;
            }
            let line = unsafe { self.pixels.add((py + y) * stride + px) };
            for x in 0..CELL_W {
                let c = if bits & (0x80 >> x) != 0 { fg } else { bg };
                unsafe { line.add(x).write_volatile(c) };
            }
        }
    }

    pub fn redraw(&mut self) {
        for r in 0..self.rows {
            for c in 0..self.cols {
                self.draw(r, c);
            }
        }
    }

    pub fn set_cursor(&mut self, pos: Option<(usize, usize)>) {
        let old = self.cursor;
        self.cursor = pos;
        if let Some((r, c)) = old {
            if r < self.rows && c < self.cols {
                self.draw(r, c);
            }
        }
        if let Some((r, c)) = pos {
            if r < self.rows && c < self.cols {
                self.draw(r, c);
            }
        }
    }

    /// Scroll rows `top..=bottom` up by `n`, filling with `fill`.
    pub fn scroll_up(&mut self, top: usize, bottom: usize, n: usize, fill: u16) {
        let bottom = bottom.min(self.rows - 1);
        if top > bottom || n == 0 {
            return;
        }
        let n = n.min(bottom - top + 1);
        for r in top..=bottom {
            for c in 0..self.cols {
                self.cells[r][c] = if r + n <= bottom { self.cells[r + n][c] } else { fill };
            }
        }
        // Move the pixels of the surviving rows in one copy, then draw only
        // the rows that came into view.
        let stride = self.fb.stride as usize;
        let row_px = CELL_H * stride;
        let keep = bottom + 1 - top - n;
        if keep > 0 {
            unsafe {
                let dst = self.pixels.add((self.y0 + top * CELL_H) * stride);
                let src = dst.add(n * row_px);
                core::ptr::copy(src, dst, keep * row_px);
            }
        }
        let hide = self.cursor;
        for r in top + keep..=bottom {
            for c in 0..self.cols {
                self.draw(r, c);
            }
        }
        // The cursor may have been moved along with the pixels.
        if let Some((r, c)) = hide {
            if r >= top && r <= bottom && r >= n {
                self.draw(r - n, c);
            }
            if r < self.rows && c < self.cols {
                self.draw(r, c);
            }
        }
    }

    /// Kernel console output: interpret CR, LF, BS and TAB.
    pub fn write_char(&mut self, b: u8) {
        match b {
            b'\n' => {
                self.cur_col = 0;
                self.cur_row += 1;
            }
            b'\r' => self.cur_col = 0,
            8 => self.cur_col = self.cur_col.saturating_sub(1),
            b'\t' => self.cur_col = (self.cur_col + 8) & !7,
            _ => {
                if self.cur_col >= self.cols {
                    self.cur_col = 0;
                    self.cur_row += 1;
                }
                self.scroll_if_needed();
                let cell = b as u16 | (self.attr as u16) << 8;
                self.put(self.cur_row, self.cur_col, cell);
                self.cur_col += 1;
            }
        }
        self.scroll_if_needed();
    }

    fn scroll_if_needed(&mut self) {
        if self.cur_row >= self.rows {
            let fill = 0x20 | (self.attr as u16) << 8;
            self.scroll_up(0, self.rows - 1, 1, fill);
            self.cur_row = self.rows - 1;
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            let b = self.codepage.from_char(c).unwrap_or(b'?');
            self.write_char(if c == '\n' { b'\n' } else { b });
        }
    }
}

pub fn init(fb: Framebuffer) {
    if fb.base == 0 {
        return;
    }
    let slot = unsafe { &mut *core::ptr::addr_of_mut!(SCREEN_STATE) };
    *SCREEN.lock() = Some(Screen::init_at(slot, fb));
}

pub fn with<R>(f: impl FnOnce(&mut Screen) -> R) -> Option<R> {
    SCREEN.lock().as_mut().map(|s| f(s))
}
