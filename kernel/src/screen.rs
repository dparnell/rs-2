//! The physical text screen.
//!
//! Modern machines boot without a VGA text mode, so the screen is a grid of
//! (character, attribute) cells, exactly like VGA text memory, rendered into
//! the linear framebuffer with an 8x16, 8x14 or 8x8 font (the largest that
//! fits the number of rows: at 1024x768, 25 and 43 rows use 8x16, 50 rows
//! 8x14 and 60 rows 8x8). Kernel messages and VIO sessions both draw into
//! it. The colours go through VGA-style palette registers and a DAC, and
//! attribute bit 7 means blinking or a bright background, as on a VGA.
//!
//! Every session has its own `Screen`; only the foreground one is
//! `visible` and draws into the framebuffer, the others just keep their
//! cells until they are switched to.
//!
//! A session can map a "physical" text buffer (VioGetPhysBuf): a shared
//! object holding the cells in VGA layout that programs write directly;
//! [`tick`] renders what changed.
//!
//! The rows drawn are noted as damage for display drivers that copy
//! changes to the display ([`crate::display::take_damage`]); while PM is
//! shown, PM reports what it draws itself.

use bootinfo::{Framebuffer, PHYS_MAP_BASE};

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;

use core::sync::atomic::{AtomicU32, Ordering};

use crate::codepage::CodePage;
use crate::sync::SpinLock;
use crate::vm::SharedMem;

pub const CELL_W: usize = 8;
/// Tallest font.
pub const CELL_H: usize = 16;
pub const MAX_COLS: usize = 132;
pub const MAX_ROWS: usize = 100;
/// Lines kept that scrolled off a windowable session's screen.
pub const HISTORY: usize = 1000;

/// Note that framebuffer rows `top..end` were drawn (whole rows: the
/// width is clipped when the damage is taken).
pub fn mark_dirty(top: u32, end: u32) {
    if top < end {
        crate::display::damage(0, top, u32::MAX, end);
    }
}

/// After PM gave the screen to a text session: when to redraw the text
/// screen again (PM may still have been drawing for a moment) and how
/// many more times.
static REDRAW_AT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static REDRAW_LEFT: AtomicU32 = AtomicU32::new(0);

/// Font heights, tallest first.
pub const FONT_HEIGHTS: [usize; 3] = [16, 14, 8];

/// Default VGA palette registers: attribute colour -> DAC entry.
const DEFAULT_PALREGS: [u8; 16] = [0, 1, 2, 3, 4, 5, 0x14, 7, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f];

/// Default DAC entry `i` (6 bits per gun): the EGA 64-colour palette
/// (bits 0-2 = blue, green, red at 2/3, bits 3-5 at 1/3 intensity).
fn default_dac(i: usize) -> (u8, u8, u8) {
    let gun = |hi: usize, lo: usize| ((i >> hi & 1) * 0x2a + (i >> lo & 1) * 0x15) as u8;
    if i < 64 { (gun(2, 5), gun(1, 4), gun(0, 3)) } else { (0, 0, 0) }
}

static FONT16: &[u8] = include_bytes!("../assets/unscii-16.bin");
static FONT14: &[u8] = include_bytes!("../assets/unscii-14.bin");
static FONT8: &[u8] = include_bytes!("../assets/unscii-8.bin");

/// The glyph for `c` in the built-in font of height `h`.
fn font_glyph(c: char, h: usize) -> Option<&'static [u8]> {
    let font = match h {
        8 => FONT8,
        14 => FONT14,
        _ => FONT16,
    };
    let h = if h == 8 || h == 14 { h } else { 16 };
    let rec = 4 + h;
    let n = font.len() / rec;
    let key = c as u32;
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let r = &font[mid * rec..mid * rec + rec];
        let cp = u32::from_le_bytes([r[0], r[1], r[2], r[3]]);
        match cp.cmp(&key) {
            core::cmp::Ordering::Equal => return Some(&r[4..]),
            core::cmp::Ordering::Less => lo = mid + 1,
            core::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

/// The 256-character font of a code page at height `h` (`h` bytes per
/// character), as VioGetFont returns it.
pub fn rom_font(cp: CodePage, h: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(256 * h);
    for b in 0..256 {
        let g = font_glyph(cp.glyph(b as u8), h).or_else(|| font_glyph('?', h)).unwrap();
        v.extend_from_slice(&g[..h]);
    }
    v
}

/// The text buffer of VioGetPhysBuf: cells in VGA layout (row-major,
/// character then attribute) in a shared object mapped into programs.
pub struct PhysBuf {
    pub obj: Arc<SharedMem>,
    pages: Vec<*mut u8>,
}

impl PhysBuf {
    pub const SIZE: u32 = 0x8000;

    fn new() -> Option<PhysBuf> {
        let base = crate::vm::reserve_shared(Self::SIZE, None)?;
        let obj = SharedMem::new(base, Self::SIZE, crate::vm::pag::READ | crate::vm::pag::WRITE, None);
        let mut pages = Vec::new();
        for i in 0..(Self::SIZE / crate::vm::PAGE) as usize {
            let f = obj.frame(i)?;
            pages.push(crate::mm::phys::phys_to_virt(f));
        }
        for p in obj.pages.lock().iter_mut() {
            *p = crate::vm::PageState::Committed;
        }
        Some(PhysBuf { obj, pages })
    }

    fn at(&self, off: usize) -> *mut u16 {
        let page = crate::vm::PAGE as usize;
        unsafe { self.pages[off / page].add(off % page) as *mut u16 }
    }

    fn get(&self, i: usize) -> u16 {
        unsafe { self.at(i * 2).read_volatile() }
    }

    fn set(&self, i: usize, v: u16) {
        if i * 2 + 2 <= Self::SIZE as usize {
            unsafe { self.at(i * 2).write_volatile(v) }
        }
    }
}

/// The picture of a DOS session in a graphics mode: 8-bit pixels and
/// their palette (0x00RRGGBB), scaled onto the framebuffer (4:3).
pub struct Pixmap {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
    pub pal: [u32; 256],
    /// Bumped by every change.
    pub generation: u32,
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
    /// Height of a character cell (the font in use).
    pub cell_h: usize,
    /// A font loaded with VioSetFont is in use (until the next mode set).
    pub user_font: bool,
    codepage: CodePage,
    /// Colours of the 16 attribute values in framebuffer format.
    palette: [u32; 16],
    /// VGA palette registers and DAC (6 bits per gun).
    pub palregs: [u8; 16],
    pub dac: [(u8, u8, u8); 256],
    /// Border colour (attribute colour index).
    pub overscan: u8,
    /// Attribute bit 7 blinks the character (true) or brightens the
    /// background (false).
    pub blink: bool,
    blink_off: bool,
    /// Underline scan line (VIOSETULINELOC; monochrome only).
    pub underline: u16,
    /// Text cursor for kernel console output.
    pub cur_row: usize,
    pub cur_col: usize,
    /// Hardware-style cursor shown for VIO.
    cursor: Option<(usize, usize)>,
    /// VIOCURSORINFO: yStart, cEnd, cx, attr (0xFFFF = hidden).
    pub cursor_type: [u16; 4],
    /// Attribute used by the kernel console.
    pub attr: u8,
    /// Shown on the framebuffer (the foreground session).
    visible: bool,
    /// The session draws graphics itself (Presentation Manager): its text
    /// cells are never rendered.
    pub graphics: bool,
    /// Mapped VioGetPhysBuf buffer.
    pub phys: Option<PhysBuf>,
    /// Bumped by every change of what the screen shows.
    pub changes: u32,
    /// Bumped when the glyphs change (code page, font).
    pub font_gen: u32,
    /// A windowable VIO session: changes are reported to the PM host.
    pub watched: bool,
    /// Shown in a PM window now (the text-mode mouse pointer is not drawn).
    pub windowed: bool,
    /// A DOS session's graphics mode picture (text cells are not shown
    /// meanwhile).
    pub pixmap: Option<Pixmap>,
    /// Lines that scrolled off the top of a windowable session's whole
    /// screen, oldest first (the PM host's scroll back), and how many
    /// ever did.
    history: VecDeque<Vec<u16>>,
    pub scrolled: u32,
    /// Keys typed into the session's window (the host shows the screen
    /// again rather than its scroll back).
    pub keys: u32,
}

unsafe impl Send for Screen {}

/// The screens of all sessions; session 1 (the boot session) owns the
/// statically allocated one.
pub struct Screens {
    boot: Option<&'static mut Screen>,
    others: Vec<(u32, Box<Screen>)>,
    /// The session shown on the framebuffer.
    pub visible: u32,
}

pub static SCREEN: SpinLock<Screens> = SpinLock::new(Screens { boot: None, others: Vec::new(), visible: 1 });

impl Screens {
    pub fn get(&mut self, sid: u32) -> Option<&mut Screen> {
        if sid == 1 {
            return self.boot.as_deref_mut();
        }
        self.others.iter_mut().find(|(s, _)| *s == sid).map(|(_, b)| &mut **b)
    }
}

/// Backing storage for the screen state (too big for the boot stack).
static mut SCREEN_STATE: core::mem::MaybeUninit<Screen> = core::mem::MaybeUninit::uninit();

impl Screen {
    /// Initialise the boot screen in place.
    fn init_at(slot: &'static mut core::mem::MaybeUninit<Screen>, fb: Framebuffer) -> &'static mut Screen {
        let s = Screen::init_hidden(slot, fb, CodePage::Cp850);
        s.visible = true;
        s.set_mode(80, 25);
        s
    }

    /// A screen for a background session: same framebuffer, not visible.
    fn init_hidden(slot: &mut core::mem::MaybeUninit<Screen>, fb: Framebuffer, cp: CodePage) -> &mut Screen {
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
            core::ptr::addr_of_mut!((*s).cell_h).write(CELL_H);
            core::ptr::addr_of_mut!((*s).user_font).write(false);
            core::ptr::addr_of_mut!((*s).codepage).write(cp);
            core::ptr::addr_of_mut!((*s).palette).write([0; 16]);
            core::ptr::addr_of_mut!((*s).palregs).write(DEFAULT_PALREGS);
            core::ptr::addr_of_mut!((*s).dac).write([(0, 0, 0); 256]);
            core::ptr::addr_of_mut!((*s).overscan).write(0);
            core::ptr::addr_of_mut!((*s).blink).write(true);
            core::ptr::addr_of_mut!((*s).blink_off).write(false);
            core::ptr::addr_of_mut!((*s).underline).write(31);
            core::ptr::addr_of_mut!((*s).cur_row).write(0);
            core::ptr::addr_of_mut!((*s).cur_col).write(0);
            core::ptr::addr_of_mut!((*s).cursor).write(None);
            core::ptr::addr_of_mut!((*s).cursor_type).write([14, 15, 1, 0]);
            core::ptr::addr_of_mut!((*s).attr).write(0x07);
            core::ptr::addr_of_mut!((*s).visible).write(false);
            core::ptr::addr_of_mut!((*s).graphics).write(false);
            core::ptr::addr_of_mut!((*s).phys).write(None);
            core::ptr::addr_of_mut!((*s).changes).write(0);
            core::ptr::addr_of_mut!((*s).font_gen).write(0);
            core::ptr::addr_of_mut!((*s).watched).write(false);
            core::ptr::addr_of_mut!((*s).windowed).write(false);
            core::ptr::addr_of_mut!((*s).pixmap).write(None);
            core::ptr::addr_of_mut!((*s).history).write(VecDeque::new());
            core::ptr::addr_of_mut!((*s).scrolled).write(0);
            core::ptr::addr_of_mut!((*s).keys).write(0);
        }
        let s = unsafe { &mut *s };
        for row in s.cells.iter_mut() {
            row.fill(0x0720);
        }
        for i in 0..256 {
            s.dac[i] = default_dac(i);
        }
        s.update_palette();
        s.set_codepage(cp);
        s.set_mode(80, 25);
        s
    }

    pub fn codepage(&self) -> CodePage {
        self.codepage
    }

    /// Note a change of what the screen shows (for the PM host of a
    /// windowed session).
    fn touch(&mut self) {
        self.changes = self.changes.wrapping_add(1);
        if self.watched {
            crate::viowin::changed();
        }
    }

    /// Mark the screen as one of a windowable session and whether it is
    /// in its window now.
    pub fn set_windowed(&mut self, watched: bool, windowed: bool) {
        self.watched = watched;
        self.windowed = windowed;
        self.touch();
    }

    /// The cells of the text area, row by row.
    pub fn cells_vec(&self) -> Vec<u16> {
        let mut v = Vec::with_capacity(self.rows * self.cols);
        for r in 0..self.rows {
            v.extend_from_slice(&self.cells[r][..self.cols]);
        }
        v
    }

    /// Lines `end - n .. end` of the scroll back (numbered from the first
    /// line that ever scrolled off), `cols` cells each; lines no longer
    /// kept are blank.
    pub fn history_lines(&self, end: u32, n: usize, cols: usize) -> Vec<u16> {
        let end = end.min(self.scrolled);
        let first_kept = self.scrolled - self.history.len() as u32;
        let mut v = Vec::with_capacity(n * cols);
        for k in 0..n as u32 {
            let back = n as u32 - k;
            let row = (back <= end && end - back >= first_kept).then(|| &self.history[(end - back - first_kept) as usize]);
            for c in 0..cols {
                v.push(row.and_then(|r| r.get(c).copied()).unwrap_or(0x0720));
            }
        }
        v
    }

    /// A key was typed into the session's window.
    pub fn note_key(&mut self) {
        self.keys = self.keys.wrapping_add(1);
        self.touch();
    }

    /// Keep rows `top..top + n` for the scroll back (a clear of the whole
    /// region keeps only the rows down to the last one with text).
    fn keep_history(&mut self, top: usize, n: usize) {
        let mut n = n.min(self.rows - top);
        if top + n == self.rows {
            let blank = |r: &[u16]| r.iter().all(|&c| matches!(c as u8, 0 | b' '));
            n = (top..self.rows).rev().find(|&r| !blank(&self.cells[r][..self.cols])).map_or(0, |r| r + 1 - top);
        }
        for r in top..top + n {
            if self.history.len() == HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(self.cells[r][..self.cols].to_vec());
            self.scrolled = self.scrolled.wrapping_add(1);
        }
    }

    /// The 16 attribute colours as 0x00RRGGBB.
    pub fn palette_rgb(&self) -> [u32; 16] {
        let mut out = [0; 16];
        for (i, o) in out.iter_mut().enumerate() {
            let (r, g, b) = self.dac[(self.palregs[i] & 0x3f) as usize];
            let scale = |v: u8| ((v as u32 & 0x3f) * 255 + 31) / 63;
            *o = scale(r) << 16 | scale(g) << 8 | scale(b);
        }
        out
    }

    pub fn set_codepage(&mut self, cp: CodePage) {
        self.codepage = cp;
        self.user_font = false;
        self.load_font();
        self.touch();
        self.redraw();
    }

    /// Glyphs of the code page from the built-in font of the cell height.
    fn load_font(&mut self) {
        self.font_gen = self.font_gen.wrapping_add(1);
        let h = self.cell_h;
        for b in 0..256 {
            let c = self.codepage.glyph(b as u8);
            let g = font_glyph(c, h).or_else(|| font_glyph('?', h)).unwrap();
            self.glyphs[b] = [0; CELL_H];
            self.glyphs[b][..h].copy_from_slice(&g[..h]);
        }
    }

    /// The font in use, `cell_h` bytes per character.
    pub fn font(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(256 * self.cell_h);
        for g in self.glyphs.iter() {
            v.extend_from_slice(&g[..self.cell_h]);
        }
        v
    }

    /// Load a user font (VioSetFont): 256 characters of `h` rows. Fails if
    /// the current number of rows does not fit with that height.
    pub fn set_font(&mut self, h: usize, data: &[u8]) -> bool {
        if !(1..=CELL_H).contains(&h) || data.len() < 256 * h || self.rows * h > self.fb.height as usize {
            return false;
        }
        for b in 0..256 {
            self.glyphs[b] = [0; CELL_H];
            self.glyphs[b][..h].copy_from_slice(&data[b * h..b * h + h]);
        }
        self.user_font = true;
        self.font_gen = self.font_gen.wrapping_add(1);
        self.touch();
        if h != self.cell_h {
            self.cell_h = h;
            self.layout();
        } else {
            self.redraw();
        }
        true
    }

    /// Recompute the 16 attribute colours from the palette registers and
    /// the DAC.
    pub fn update_palette(&mut self) {
        for i in 0..16 {
            let (r, g, b) = self.dac[(self.palregs[i] & 0x3f) as usize];
            let scale = |v: u8| ((v as u32 & 0x3f) * 255 + 31) / 63;
            let (r, g, b) = (scale(r), scale(g), scale(b));
            self.palette[i] = if self.fb.format == 0 { r | g << 8 | b << 16 } else { r << 16 | g << 8 | b };
        }
        self.touch();
    }

    /// The largest built-in font height for `rows` rows, if any fits.
    pub fn font_for(&self, rows: usize) -> Option<usize> {
        FONT_HEIGHTS.into_iter().find(|&h| rows * h <= self.fb.height as usize)
    }

    /// Whether a `cols` x `rows` text mode fits the framebuffer.
    pub fn mode_fits(&self, cols: usize, rows: usize) -> bool {
        cols >= 1 && rows >= 1 && cols <= MAX_COLS && rows <= MAX_ROWS && cols * CELL_W <= self.fb.width as usize && self.font_for(rows).is_some()
    }

    /// Change the text geometry, clamped to what fits the framebuffer; the
    /// font is the largest that fits the rows. Resets the cursor shape.
    pub fn set_mode(&mut self, cols: usize, rows: usize) {
        let max_c = (self.fb.width as usize / CELL_W).min(MAX_COLS);
        let max_r = (self.fb.height as usize / FONT_HEIGHTS[2]).min(MAX_ROWS);
        self.cols = cols.min(max_c).max(1);
        self.rows = rows.min(max_r).max(1);
        let h = self.font_for(self.rows).unwrap_or(FONT_HEIGHTS[2]);
        let reload = h != self.cell_h || self.user_font;
        self.cell_h = h;
        self.user_font = false;
        if reload {
            self.load_font();
        }
        self.cursor_type = [(h - 2) as u16, (h - 1) as u16, 1, self.cursor_type[3]];
        self.cur_row = self.cur_row.min(self.rows - 1);
        self.cur_col = 0;
        self.sync_phys();
        self.touch();
        self.layout();
    }

    /// Centre the text area and redraw everything.
    fn layout(&mut self) {
        self.x0 = (self.fb.width as usize).saturating_sub(self.cols * CELL_W) / 2;
        self.y0 = (self.fb.height as usize).saturating_sub(self.rows * self.cell_h) / 2;
        if !self.visible || self.graphics {
            return;
        }
        if let Some(h) = self.pixmap.as_ref().map(|p| p.h) {
            self.fill_black();
            self.draw_pixmap(0, h);
            return;
        }
        self.clear_border();
        self.redraw();
    }

    /// Fill the whole framebuffer with black.
    fn fill_black(&mut self) {
        let (w, h, stride) = (self.fb.width as usize, self.fb.height as usize, self.fb.stride as usize);
        mark_dirty(0, h as u32);
        for y in 0..h {
            let line = unsafe { self.pixels.add(y * stride) };
            for x in 0..w {
                unsafe { line.add(x).write_volatile(0) };
            }
        }
    }

    /// Where a pixmap is shown: (x, y, width, height) of the largest 4:3
    /// area of the framebuffer, centred.
    fn pix_rect(&self) -> (usize, usize, usize, usize) {
        let (fw, fh) = (self.fb.width as usize, self.fb.height as usize);
        let tw = fw.min(fh * 4 / 3);
        let th = (tw * 3 / 4).min(fh);
        ((fw - tw) / 2, (fh - th) / 2, tw, th)
    }

    /// Draw source rows `y0..y1` of the pixmap.
    fn draw_pixmap(&mut self, y0: usize, y1: usize) {
        let Some(p) = self.pixmap.as_ref() else { return };
        let (rx, ry, tw, th) = self.pix_rect();
        if p.w == 0 || p.h == 0 || tw == 0 || th == 0 {
            return;
        }
        let bgr = self.fb.format != 0;
        let mut pal = [0u32; 256];
        for (i, c) in pal.iter_mut().enumerate() {
            let v = p.pal[i];
            *c = if bgr { v & 0xff_ffff } else { (v >> 16 & 0xff) | (v & 0xff00) | (v & 0xff) << 16 };
        }
        let xmap: Vec<u32> = (0..tw).map(|x| (x * p.w / tw) as u32).collect();
        // Destination rows whose source row is in y0..y1.
        let d0 = (y0 * th).div_ceil(p.h);
        let d1 = ((y1 * th).div_ceil(p.h)).min(th);
        let stride = self.fb.stride as usize;
        for dy in d0..d1 {
            let sy = dy * p.h / th;
            let src = &p.data[sy * p.w..sy * p.w + p.w];
            let line = unsafe { self.pixels.add((ry + dy) * stride + rx) };
            for (dx, &sx) in xmap.iter().enumerate() {
                unsafe { line.add(dx).write_volatile(pal[src[sx as usize] as usize]) };
            }
        }
        mark_dirty((ry + d0) as u32, (ry + d1) as u32);
    }

    /// Show a picture of `w` x `h` (a DOS session's graphics mode): rows
    /// `y0..` from `rows`, with palette `pal`.
    pub fn set_pixels(&mut self, w: usize, h: usize, y0: usize, rows: &[u8], pal: &[u32; 256]) {
        let new = !matches!(&self.pixmap, Some(p) if p.w == w && p.h == h);
        if new {
            self.pixmap = Some(Pixmap { w, h, data: alloc::vec![0; w * h], pal: *pal, generation: 0 });
        }
        let p = self.pixmap.as_mut().unwrap();
        let pal_changed = p.pal != *pal;
        p.pal = *pal;
        let n = if w == 0 { 0 } else { (rows.len() / w).min(h.saturating_sub(y0)) };
        p.data[y0 * w..(y0 + n) * w].copy_from_slice(&rows[..n * w]);
        p.generation = p.generation.wrapping_add(1);
        self.touch();
        if self.visible && !self.graphics {
            if new {
                self.fill_black();
                self.draw_pixmap(0, h);
            } else if pal_changed {
                self.draw_pixmap(0, h);
            } else {
                self.draw_pixmap(y0, y0 + n);
            }
        }
    }

    /// Back to the text screen.
    pub fn clear_pixels(&mut self) {
        if self.pixmap.take().is_some() {
            self.touch();
            self.layout();
        }
    }

    /// Fill the framebuffer outside the text area with the border colour.
    fn clear_border(&mut self) {
        if self.pixmap.is_some() {
            return;
        }
        let c = self.palette[(self.overscan & 15) as usize];
        let (w, h, stride) = (self.fb.width as usize, self.fb.height as usize, self.fb.stride as usize);
        let (tx1, ty1) = (self.x0 + self.cols * CELL_W, self.y0 + self.rows * self.cell_h);
        mark_dirty(0, h as u32);
        for y in 0..h {
            let line = unsafe { self.pixels.add(y * stride) };
            let inside = y >= self.y0 && y < ty1;
            for x in 0..w {
                if inside && x >= self.x0 && x < tx1 {
                    continue;
                }
                unsafe { line.add(x).write_volatile(c) };
            }
        }
    }

    /// Set the border colour.
    pub fn set_overscan(&mut self, c: u8) {
        self.overscan = c & 15;
        self.touch();
        if self.visible && !self.graphics {
            self.clear_border();
        }
    }

    pub fn cell(&self, row: usize, col: usize) -> u16 {
        self.cells[row][col]
    }

    /// The cell (row, column) under framebuffer pixel (x, y), clamped to
    /// the text area.
    pub fn cell_at(&self, x: usize, y: usize) -> (usize, usize) {
        let row = (y.saturating_sub(self.y0) / self.cell_h.max(1)).min(self.rows - 1);
        let col = (x.saturating_sub(self.x0) / CELL_W).min(self.cols - 1);
        (row, col)
    }

    /// Store a (char | attr << 8) cell and draw it.
    pub fn put(&mut self, row: usize, col: usize, cell: u16) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        self.cells[row][col] = cell;
        if let Some(p) = &self.phys {
            p.set(row * self.cols + col, cell);
        }
        self.touch();
        self.draw(row, col);
    }

    fn draw(&mut self, row: usize, col: usize) {
        if !self.visible || self.graphics || self.pixmap.is_some() {
            return;
        }
        let cell = self.cells[row][col];
        let ch = (cell & 0xff) as usize;
        let attr = (cell >> 8) as usize;
        let mut fg = self.palette[attr & 15];
        let bg = self.palette[(attr >> 4) & if self.blink { 7 } else { 15 }];
        if self.blink && attr & 0x80 != 0 && self.blink_off {
            fg = bg;
        }
        let h = self.cell_h;
        let cursor = if self.cursor == Some((row, col)) && self.cursor_type[3] != 0xffff {
            Some(self.cursor_lines())
        } else {
            None
        };
        let stride = self.fb.stride as usize;
        let px = self.x0 + col * CELL_W;
        let py = self.y0 + row * h;
        let glyph = &self.glyphs[ch];
        mark_dirty(py as u32, (py + h) as u32);
        for y in 0..h {
            let mut bits = glyph[y];
            if let Some((a, b)) = cursor {
                if (a <= b && y >= a && y <= b) || (a > b && (y >= a || y <= b)) {
                    bits = 0xff;
                }
            }
            let line = unsafe { self.pixels.add((py + y) * stride + px) };
            for x in 0..CELL_W {
                let c = if bits & (0x80 >> x) != 0 { fg } else { bg };
                unsafe { line.add(x).write_volatile(c) };
            }
        }
    }

    /// First and last scan line of the cursor. Negative values in
    /// VIOCURSORINFO are percentages of the cell height.
    pub fn cursor_lines(&self) -> (usize, usize) {
        let h = self.cell_h as i32;
        let line = |v: u16| -> usize {
            let v = v as i16 as i32;
            let l = if v < 0 { ((-v).min(100) * h + 99) / 100 - 1 } else { v };
            l.clamp(0, h - 1) as usize
        };
        (line(self.cursor_type[0]), line(self.cursor_type[1]))
    }

    pub fn redraw(&mut self) {
        for r in 0..self.rows {
            for c in 0..self.cols {
                self.draw(r, c);
            }
        }
    }

    /// The cursor position, if the cursor is drawn.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        self.cursor
    }

    pub fn set_cursor(&mut self, pos: Option<(usize, usize)>) {
        let old = self.cursor;
        self.cursor = pos;
        if old != pos {
            self.touch();
        }
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

    /// Change the cursor shape (VioSetCurType) and redraw it.
    pub fn set_cursor_type(&mut self, t: [u16; 4]) {
        self.cursor_type = t;
        self.touch();
        if let Some((r, c)) = self.cursor {
            if r < self.rows && c < self.cols {
                self.draw(r, c);
            }
        }
    }

    /// Switch attribute bit 7 between blink and background intensity.
    pub fn set_blink(&mut self, blink: bool) {
        if self.blink != blink {
            self.blink = blink;
            self.touch();
            self.redraw();
        }
    }

    /// Scroll rows `top..=bottom` up by `n`, filling with `fill`.
    pub fn scroll_up(&mut self, top: usize, bottom: usize, n: usize, fill: u16) {
        let bottom = bottom.min(self.rows - 1);
        if top > bottom || n == 0 {
            return;
        }
        let n = n.min(bottom - top + 1);
        self.touch();
        // The whole screen, or all of it below a status line (rs2's shell).
        if top <= 1 && bottom == self.rows - 1 && self.watched && !self.graphics && self.pixmap.is_none() {
            self.keep_history(top, n);
        }
        for r in top..=bottom {
            for c in 0..self.cols {
                self.cells[r][c] = if r + n <= bottom { self.cells[r + n][c] } else { fill };
            }
        }
        self.sync_phys_rows(top, bottom);
        if !self.visible || self.graphics || self.pixmap.is_some() {
            return;
        }
        // Move the pixels of the surviving rows in one copy, then draw only
        // the rows that came into view.
        let stride = self.fb.stride as usize;
        let row_px = self.cell_h * stride;
        let keep = bottom + 1 - top - n;
        if keep > 0 {
            mark_dirty((self.y0 + top * self.cell_h) as u32, (self.y0 + (top + keep) * self.cell_h) as u32);
            unsafe {
                let dst = self.pixels.add((self.y0 + top * self.cell_h) * stride);
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

    /// Copy the cells into the mapped physical buffer.
    fn sync_phys(&mut self) {
        let rows = self.rows;
        self.sync_phys_rows(0, rows - 1);
    }

    fn sync_phys_rows(&mut self, top: usize, bottom: usize) {
        let Some(p) = &self.phys else { return };
        for r in top..=bottom.min(self.rows - 1) {
            for c in 0..self.cols {
                p.set(r * self.cols + c, self.cells[r][c]);
            }
        }
    }

    /// Map (create) the physical text buffer of this screen.
    pub fn phys_buf(&mut self) -> Option<Arc<SharedMem>> {
        if self.phys.is_none() {
            self.phys = Some(PhysBuf::new()?);
            self.sync_phys();
        }
        self.phys.as_ref().map(|p| p.obj.clone())
    }

    /// Take over what programs wrote into the physical buffer without
    /// drawing (before a redraw).
    fn pull_phys(&mut self) {
        let Some(p) = &self.phys else { return };
        for r in 0..self.rows {
            for c in 0..self.cols {
                self.cells[r][c] = p.get(r * self.cols + c);
            }
        }
    }

    /// Pick up what programs wrote into the physical buffer.
    fn refresh_phys(&mut self) {
        let Some(p) = &self.phys else { return };
        let (rows, cols) = (self.rows, self.cols);
        let mut changed: Vec<(usize, usize, u16)> = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                let v = p.get(r * cols + c);
                if v != self.cells[r][c] {
                    changed.push((r, c, v));
                }
            }
        }
        if !changed.is_empty() {
            self.touch();
        }
        for (r, c, v) in changed {
            self.cells[r][c] = v;
            self.draw(r, c);
        }
    }

    /// Toggle the blink phase and redraw blinking cells.
    fn blink_tick(&mut self) {
        if !self.blink {
            return;
        }
        self.blink_off = !self.blink_off;
        for r in 0..self.rows {
            for c in 0..self.cols {
                if self.cells[r][c] & 0x8000 != 0 {
                    self.draw(r, c);
                }
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
            _ => self.write_cell(b as u16 | (self.attr as u16) << 8, true),
        }
        self.scroll_if_needed();
    }

    /// Put a cell at the cursor and advance; at the end of the line the
    /// cursor wraps if `wrap`, else stays on the last column.
    pub fn write_cell(&mut self, cell: u16, wrap: bool) {
        if self.cur_col >= self.cols {
            if wrap {
                self.cur_col = 0;
                self.cur_row += 1;
            } else {
                self.cur_col = self.cols - 1;
            }
        }
        self.scroll_if_needed();
        self.put(self.cur_row, self.cur_col, cell);
        self.cur_col += 1;
        if !wrap && self.cur_col >= self.cols {
            self.cur_col = self.cols - 1;
        }
    }

    pub fn scroll_if_needed(&mut self) {
        if self.cur_row >= self.rows {
            let fill = 0x20 | (self.attr as u16) << 8;
            let n = self.cur_row + 1 - self.rows;
            self.scroll_up(0, self.rows - 1, n, fill);
            self.cur_row = self.rows - 1;
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            let b = self.codepage.from_char(c).unwrap_or(b'?');
            self.write_char(if c == '\n' { b'\n' } else { b });
        }
    }

    /// Copy the text state of another screen (transparent pop-ups).
    pub fn copy_from(&mut self, o: &Screen) {
        self.cells = o.cells;
        self.glyphs = o.glyphs;
        self.cell_h = o.cell_h;
        self.user_font = o.user_font;
        self.codepage = o.codepage;
        self.palregs = o.palregs;
        self.dac = o.dac;
        self.overscan = o.overscan;
        self.blink = o.blink;
        self.cols = o.cols;
        self.rows = o.rows;
        self.cur_row = o.cur_row;
        self.cur_col = o.cur_col;
        self.cursor_type = o.cursor_type;
        self.font_gen = self.font_gen.wrapping_add(1);
        self.update_palette();
        self.layout();
    }
}

pub fn init(fb: Framebuffer) {
    if fb.base == 0 {
        return;
    }
    let slot = unsafe { &mut *core::ptr::addr_of_mut!(SCREEN_STATE) };
    SCREEN.lock().boot = Some(Screen::init_at(slot, fb));
}

/// The screen the calling thread's VIO output goes to (its session's, or
/// the pop-up it owns).
pub fn with<R>(f: impl FnOnce(&mut Screen) -> R) -> Option<R> {
    with_session(crate::session::io_session(), f)
}

pub fn with_session<R>(sid: u32, f: impl FnOnce(&mut Screen) -> R) -> Option<R> {
    SCREEN.lock().get(sid).map(f)
}

/// The framebuffer, if there is one.
pub fn framebuffer() -> Option<Framebuffer> {
    SCREEN.lock().boot.as_ref().map(|b| b.fb)
}

/// Move every screen to a new framebuffer (after a display mode change):
/// text modes that still fit stay, others shrink to fit; the visible
/// screen is redrawn.
pub fn set_framebuffer(fb: Framebuffer) {
    if framebuffer().is_none() {
        // The first framebuffer (the firmware had none for the loader).
        init(fb);
        return;
    }
    let mut g = SCREEN.lock();
    let Screens { boot, others, .. } = &mut *g;
    let all = boot.iter_mut().map(|b| &mut **b).chain(others.iter_mut().map(|(_, b)| &mut **b));
    for s in all {
        s.fb = fb;
        s.pixels = (PHYS_MAP_BASE + fb.base) as *mut u32;
        s.update_palette();
        if s.mode_fits(s.cols, s.rows) {
            s.layout();
        } else {
            let (c, r) = (s.cols, s.rows);
            s.set_mode(c, r);
        }
    }
}

/// Create the (hidden) screen of a new session.
pub fn create(sid: u32) {
    let mut g = SCREEN.lock();
    let Some(fb) = g.boot.as_ref().map(|b| b.fb) else { return };
    let cp = g.boot.as_ref().map_or(CodePage::Cp850, |b| b.codepage);
    let mut slot = Box::<Screen>::new_uninit();
    Screen::init_hidden(&mut slot, fb, cp);
    let b = unsafe { slot.assume_init() };
    g.others.push((sid, b));
}

pub fn destroy(sid: u32) {
    let mut g = SCREEN.lock();
    g.others.retain(|(s, _)| *s != sid);
}

/// Show session `sid` on the framebuffer.
pub fn show(sid: u32) {
    let mut g = SCREEN.lock();
    if g.get(sid).is_none() || g.visible == sid {
        return;
    }
    let old = g.visible;
    if let Some(s) = g.get(old) {
        s.visible = false;
    }
    let from_graphics = g.get(old).is_some_and(|s| s.graphics);
    g.visible = sid;
    if let Some(s) = g.get(sid) {
        s.visible = true;
        if s.graphics {
            // PM's picture (drawn before it was hidden, or repainted
            // now) replaces the text screen.
            mark_dirty(0, s.fb.height);
        } else {
            s.pull_phys();
            s.layout();
            if from_graphics {
                REDRAW_AT.store(crate::irq::ticks() + 64, Ordering::Relaxed);
                REDRAW_LEFT.store(3, Ordering::Relaxed);
            }
        }
    }
}

/// The session shown on the framebuffer.
pub fn visible() -> u32 {
    SCREEN.lock().visible
}

/// Timer tick: render changes to a mapped physical buffer (about 30 times
/// a second) and blink (about twice a second).
pub fn tick(ticks: u64) {
    if ticks % 32 != 0 {
        return;
    }
    let Some(mut g) = SCREEN.try_lock() else { return };
    // Windowed sessions' physical buffers (mapped while on the full
    // screen) are still picked up for their windows.
    let vis = g.visible;
    for (sid, s) in g.others.iter_mut() {
        if s.windowed && s.phys.is_some() && *sid != vis {
            s.refresh_phys();
        }
    }
    let v = g.visible;
    let Some(s) = g.get(v) else { return };
    if s.graphics {
        // PM reports what it draws (Rs2UpdateDisplay).
        return;
    }
    s.refresh_phys();
    if ticks % 256 == 0 {
        s.blink_tick();
    }
    if REDRAW_LEFT.load(Ordering::Relaxed) > 0 && ticks >= REDRAW_AT.load(Ordering::Relaxed) {
        REDRAW_LEFT.fetch_sub(1, Ordering::Relaxed);
        REDRAW_AT.store(ticks + 128, Ordering::Relaxed);
        s.clear_border();
        s.redraw();
    }
}
