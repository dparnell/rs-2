//! Measuring and drawing laid-out panels with GPI. Every font of a help
//! file is drawn in the presentation space's default font (rs2's system
//! font, monospaced, which also serves `:xmp.`); bold is drawn twice a
//! pixel apart, underlined text gets a line.

use crate::pm::*;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use ipf::bitmap::Bitmap;
use ipf::layout::{FontMetrics, Metrics, Page, RunKind};

/// Measures text in a presentation space.
pub struct Gpi {
    pub hps: u32,
    sys: Option<FontMetrics>,
}

impl Gpi {
    pub fn new(hps: u32) -> Self {
        Gpi { hps, sys: None }
    }

    fn text_width(&self, w: &[u8]) -> i32 {
        if w.is_empty() {
            return 0;
        }
        let mut pts = [PointL::default(); 5];
        unsafe { GpiQueryTextBox(self.hps, w.len() as i32, w.as_ptr(), 5, pts.as_mut_ptr()) };
        // TXTBOX_CONCAT: where the next character starts.
        pts[4].x
    }
}

impl Metrics for Gpi {
    fn system(&mut self) -> FontMetrics {
        if let Some(m) = self.sys {
            return m;
        }
        let mut fm = [0u8; 208];
        unsafe { GpiQueryFontMetrics(self.hps, fm.len() as i32, fm.as_mut_ptr()) };
        let rd = |o: usize| i32::from_le_bytes([fm[o], fm[o + 1], fm[o + 2], fm[o + 3]]);
        let (desc, ave, ext) = (rd(80), rd(100), rd(112));
        let m = FontMetrics { height: ext.max(8), space: ave.max(4), descent: desc.max(0) };
        self.sys = Some(m);
        m
    }

    fn font(&mut self, _font: u8, _style: u8) -> FontMetrics {
        let s = self.system();
        FontMetrics { space: self.text_width(b" ").max(1), ..s }
    }

    fn width(&mut self, _font: u8, _style: u8, word: &[u8]) -> i32 {
        self.text_width(word)
    }
}

/// PM colour for a help file colour index (0: `default`).
pub fn colour(index: u8, default: i32) -> i32 {
    match index & 0x0f {
        0 => default,
        7 => 14,
        14 => CLR_BLACK,
        i => i as i32,
    }
}

/// Decoded bitmaps by (library, offset); None when they do not decode.
pub type Bitmaps = BTreeMap<(usize, u32), Option<Bitmap>>;

/// BITMAPINFO (1.x header and palette) for `b`.
fn info(b: &Bitmap) -> Vec<u8> {
    let mut v = Vec::with_capacity(12 + b.palette.len() * 3);
    v.extend_from_slice(&12u32.to_le_bytes());
    v.extend_from_slice(&(b.cx as u16).to_le_bytes());
    v.extend_from_slice(&(b.cy as u16).to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&b.bpp.to_le_bytes());
    for c in &b.palette {
        v.extend_from_slice(&[*c as u8, (*c >> 8) as u8, (*c >> 16) as u8]);
    }
    v
}

/// What to draw: the page scrolled by `top` pixels into a window of
/// `w` by `h`, with link `sel` marked.
pub struct Draw<'a, 'p> {
    pub hps: u32,
    pub page: &'a Page<'p>,
    pub top: i32,
    pub w: i32,
    pub h: i32,
    pub sel: Option<u16>,
    pub data: &'p [u8],
    pub image_start: u32,
    pub lib: usize,
    pub bitmaps: &'a mut Bitmaps,
}

impl Draw<'_, '_> {
    /// Window y of page y.
    fn wy(&self, y: i32) -> i32 {
        self.h - (y - self.top)
    }

    pub fn draw(&mut self) {
        let hps = self.hps;
        unsafe { GpiSetBackMix(hps, BM_LEAVEALONE) };
        for i in 0..self.page.runs.len() {
            let r = &self.page.runs[i];
            match r.kind {
                RunKind::Word(w) => {
                    let base = self.wy(r.y);
                    if base + r.height < 0 || base - r.height > self.h {
                        continue;
                    }
                    if r.bg != 0 {
                        let fill = RectL { l: r.x, b: base - r.descent, r: r.x + r.width, t: base - r.descent + r.height };
                        unsafe { WinFillRect(hps, &fill, colour(r.bg, SYSCLR_HELPBACKGROUND)) };
                    }
                    let c = if r.link.is_some() { SYSCLR_HELPHILITE } else { colour(r.fg, SYSCLR_HELPTEXT) };
                    unsafe { GpiSetColor(hps, c) };
                    let p = PointL { x: r.x, y: base };
                    unsafe { GpiCharStringAt(hps, &p, w.len() as i32, w.as_ptr()) };
                    if r.style & 2 != 0 {
                        let p = PointL { x: r.x + 1, y: base };
                        unsafe { GpiCharStringAt(hps, &p, w.len() as i32, w.as_ptr()) };
                    }
                    if r.style & 4 != 0 {
                        unsafe {
                            GpiMove(hps, &PointL { x: r.x, y: base - 1 });
                            GpiLine(hps, &PointL { x: r.x + r.width - 1, y: base - 1 });
                        }
                    }
                }
                RunKind::Bitmap { offset, cx, cy, fit, .. } => {
                    let (x0, y0, x1, y1) = if fit {
                        (0, 0, self.w, self.h)
                    } else {
                        let b = self.wy(r.y);
                        (r.x, b, r.x + cx, b + cy)
                    };
                    if y1 < 0 || y0 > self.h {
                        continue;
                    }
                    let key = (self.lib, offset);
                    if !self.bitmaps.contains_key(&key) {
                        let b = ipf::bitmap::decode(self.data, self.image_start, offset, (0, 0)).ok();
                        self.bitmaps.insert(key, b);
                    }
                    if let Some(Some(b)) = self.bitmaps.get(&key) {
                        let bmi = info(b);
                        let pts = [
                            PointL { x: x0, y: y0 },
                            PointL { x: x1, y: y1 },
                            PointL { x: 0, y: 0 },
                            PointL { x: b.cx as i32, y: b.cy as i32 },
                        ];
                        unsafe { GpiDrawBits(hps, b.bits.as_ptr(), bmi.as_ptr(), 4, pts.as_ptr(), ROP_SRCCOPY, 0) };
                    }
                }
            }
        }
        // The selected link: a box around each of its line pieces.
        if let Some(l) = self.sel {
            unsafe { GpiSetColor(hps, SYSCLR_HELPHILITE) };
            for (x0, t, x1, b) in self.page.link_boxes(l) {
                let (bt, bb) = (self.wy(t), self.wy(b));
                unsafe {
                    GpiMove(hps, &PointL { x: x0 - 1, y: bb - 1 });
                    GpiBox(hps, DRO_OUTLINE, &PointL { x: x1, y: bt }, 0, 0);
                }
            }
        }
    }
}
