//! Drawing helpers over GPI.

use alloc::vec::Vec;

use crate::consts::*;
use crate::sys::*;

/// Use RGB colours on this PS (LCOLF_RGB: colours are 0xRRGGBB).
pub fn rgb_mode(hps: u32) {
    unsafe { GpiCreateLogColorTable(hps, 0, LCOLF_RGB, 0, 0, core::ptr::null()) };
}

pub fn color(hps: u32, c: i32) {
    unsafe { GpiSetColor(hps, c) };
}

pub fn move_to(hps: u32, x: i32, y: i32) {
    unsafe { GpiMove(hps, &pt(x, y)) };
}

pub fn line_to(hps: u32, x: i32, y: i32) {
    unsafe { GpiLine(hps, &pt(x, y)) };
}

pub fn line(hps: u32, x0: i32, y0: i32, x1: i32, y1: i32) {
    move_to(hps, x0, y0);
    line_to(hps, x1, y1);
}

/// A filled rectangle (inclusive-exclusive like RECTL) in the current
/// colour.
pub fn fill(hps: u32, r: RectL, c: i32) {
    if r.r <= r.l || r.t <= r.b {
        return;
    }
    color(hps, c);
    move_to(hps, r.l, r.b);
    unsafe { GpiBox(hps, DRO_FILL, &pt(r.r - 1, r.t - 1), 0, 0) };
}

/// A rectangle's outline.
pub fn frame(hps: u32, r: RectL, c: i32) {
    if r.r <= r.l || r.t <= r.b {
        return;
    }
    color(hps, c);
    move_to(hps, r.l, r.b);
    unsafe { GpiBox(hps, DRO_OUTLINE, &pt(r.r - 1, r.t - 1), 0, 0) };
}

/// A rounded box, filled and outlined.
pub fn round_box(hps: u32, r: RectL, radius: i32, fill_c: i32, edge: i32) {
    color(hps, fill_c);
    move_to(hps, r.l, r.b);
    unsafe { GpiBox(hps, DRO_FILL, &pt(r.r - 1, r.t - 1), radius * 2, radius * 2) };
    color(hps, edge);
    move_to(hps, r.l, r.b);
    unsafe { GpiBox(hps, DRO_OUTLINE, &pt(r.r - 1, r.t - 1), radius * 2, radius * 2) };
}

/// A 3-D frame: light top and left, dark bottom and right.
pub fn bevel(hps: u32, r: RectL, light: i32, dark: i32, width: i32) {
    for i in 0..width {
        color(hps, light);
        line(hps, r.l + i, r.b + i, r.l + i, r.t - 1 - i);
        line_to(hps, r.r - 1 - i, r.t - 1 - i);
        color(hps, dark);
        line(hps, r.r - 1 - i, r.t - 1 - i, r.r - 1 - i, r.b + i);
        line_to(hps, r.l + i, r.b + i);
    }
}

/// A filled polygon.
pub fn polygon(hps: u32, pts: &[PointL], c: i32) {
    if pts.len() < 3 {
        return;
    }
    color(hps, c);
    unsafe {
        GpiBeginArea(hps, BA_NOBOUNDARY | BA_ALTERNATE);
        GpiMove(hps, &pts[0]);
        GpiPolyLine(hps, pts.len() as i32 - 1, pts[1..].as_ptr());
        GpiEndArea(hps);
    }
}

/// A filled ellipse inside the rectangle.
pub fn ellipse(hps: u32, r: RectL, c: i32) {
    let (cx, cy) = ((r.l + r.r) / 2, (r.b + r.t) / 2);
    let p = [(r.r - r.l) / 2, (r.t - r.b) / 2, 0, 0];
    color(hps, c);
    unsafe {
        GpiSetArcParams(hps, p.as_ptr());
        GpiMove(hps, &pt(cx, cy));
        GpiFullArc(hps, DRO_FILL, 0x10000);
    }
}

/// An ellipse's outline.
pub fn ellipse_outline(hps: u32, r: RectL, c: i32) {
    let (cx, cy) = ((r.l + r.r) / 2, (r.b + r.t) / 2);
    let p = [(r.r - r.l) / 2, (r.t - r.b) / 2, 0, 0];
    color(hps, c);
    unsafe {
        GpiSetArcParams(hps, p.as_ptr());
        GpiMove(hps, &pt(cx, cy));
        GpiFullArc(hps, DRO_OUTLINE, 0x10000);
    }
}

/// Text at a point (the baseline) in the current font and colour.
pub fn text_at(hps: u32, x: i32, y: i32, s: &[u8], c: i32) {
    color(hps, c);
    unsafe { GpiCharStringAt(hps, &pt(x, y), s.len() as i32, s.as_ptr()) };
}

/// Text drawn with WinDrawText in a rectangle (DT_* flags).
pub fn draw_text(hps: u32, r: RectL, s: &[u8], fg: i32, bg: i32, fl: u32) {
    let mut rr = r;
    unsafe { WinDrawText(hps, s.len() as i32, s.as_ptr(), &mut rr, fg, bg, fl) };
}

/// The width of a string in the PS's current font.
pub fn text_width(hps: u32, s: &[u8]) -> i32 {
    if s.is_empty() {
        return 0;
    }
    let mut p = [PointL::default(); 5];
    unsafe { GpiQueryTextBox(hps, s.len() as i32, s.as_ptr(), 5, p.as_mut_ptr()) };
    p[4].x - p[0].x.min(p[1].x)
}

/// The font's height (max ascender + descender) and its descender.
pub fn font_height(hps: u32) -> (i32, i32) {
    let mut fm = [0u8; 208];
    unsafe { GpiQueryFontMetrics(hps, fm.len() as i32, fm.as_mut_ptr()) };
    // FONTMETRICS: lMaxAscender at 0x4c, lMaxDescender at 0x50.
    let asc = crate::rd32(&fm, 0x4c) as i32;
    let desc = crate::rd32(&fm, 0x50) as i32;
    (asc + desc, desc)
}

/// Select an outline font (`face`) at `height` pels into the PS as
/// logical font `id` (1..254).
pub fn select_font(hps: u32, id: i32, face: &[u8], height: i32) {
    let f = FAttrs::outline(face);
    unsafe {
        GpiSetCharSet(hps, 0);
        GpiDeleteSetId(hps, id);
        GpiCreateLogFont(hps, core::ptr::null(), id, &f);
        GpiSetCharSet(hps, id);
        GpiSetCharBox(hps, &SizeL { cx: height << 16, cy: height << 16 });
    }
}

/// A memory device context and its PS (for off-screen bitmaps).
pub struct MemPs {
    pub hdc: u32,
    pub hps: u32,
    pub hbm: u32,
    pub cx: i32,
    pub cy: i32,
}

impl MemPs {
    /// A memory PS of `cx` by `cy` pels with a 24-bit bitmap selected
    /// (pels, RGB colours).
    pub fn new(hab: u32, cx: i32, cy: i32) -> Option<MemPs> {
        let dop: [u32; 9] = [0; 9];
        let hdc = unsafe { DevOpenDC(hab, OD_MEMORY, b"*\0".as_ptr(), 0, dop.as_ptr(), 0) };
        if hdc == 0 {
            return None;
        }
        let size = SizeL { cx, cy };
        let hps = unsafe { GpiCreatePS(hab, hdc, &size, PU_PELS | GPIF_DEFAULT | GPIT_MICRO | GPIA_ASSOC) };
        if hps == 0 {
            unsafe { DevCloseDC(hdc) };
            return None;
        }
        let bih = BitmapInfoHeader2::new(cx as u32, cy as u32, 24);
        let hbm = unsafe { GpiCreateBitmap(hps, &bih, 0, core::ptr::null(), core::ptr::null()) };
        unsafe { GpiSetBitmap(hps, hbm) };
        rgb_mode(hps);
        Some(MemPs { hdc, hps, hbm, cx, cy })
    }

    /// Copy it to a PS at (x, y).
    pub fn blit_to(&self, hps: u32, x: i32, y: i32) {
        let p = [pt(x, y), pt(x + self.cx, y + self.cy), pt(0, 0)];
        unsafe { GpiBitBlt(hps, self.hps, 3, p.as_ptr(), ROP_SRCCOPY, BBO_IGNORE) };
    }
}

impl Drop for MemPs {
    fn drop(&mut self) {
        unsafe {
            GpiSetBitmap(self.hps, 0);
            if self.hbm != 0 {
                GpiDeleteBitmap(self.hbm);
            }
            GpiDestroyPS(self.hps);
            DevCloseDC(self.hdc);
        }
    }
}

/// 24-bit pels (BGR, rows from the bottom, each row padded to 4 bytes)
/// drawn at (x, y) with GpiDrawBits, stretched to cx by cy.
pub fn draw_rgb(hps: u32, bits: &[u8], w: i32, h: i32, x: i32, y: i32, cx: i32, cy: i32) {
    let bih = BitmapInfoHeader2::new(w as u32, h as u32, 24);
    let p = [pt(x, y), pt(x + cx - 1, y + cy - 1), pt(0, 0), pt(w, h)];
    unsafe { GpiDrawBits(hps, bits.as_ptr(), &bih as *const BitmapInfoHeader2 as *const u8, 4, p.as_ptr(), ROP_SRCCOPY, BBO_IGNORE) };
}

/// Pack 0xRRGGBB pels (rows from the top) into GpiDrawBits's 24-bit rows.
pub fn pack_rgb(pels: &[u32], w: usize, h: usize) -> Vec<u8> {
    let stride = (w * 3).div_ceil(4) * 4;
    let mut out = alloc::vec![0u8; stride * h];
    for y in 0..h {
        let row = &mut out[(h - 1 - y) * stride..];
        for x in 0..w {
            let c = pels[y * w + x];
            row[x * 3] = c as u8;
            row[x * 3 + 1] = (c >> 8) as u8;
            row[x * 3 + 2] = (c >> 16) as u8;
        }
    }
    out
}
