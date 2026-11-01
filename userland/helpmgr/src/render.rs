//! Measuring and drawing laid-out panels with GPI. The fonts of a help
//! file's font table (and of dynamic data) are logical fonts: as IBM's
//! help manager matches each to the nearest image font of its face and
//! point size (FontFindFontMetrics), rs2 takes the nearest of the sizes
//! IBM's image fonts come in for Helv, Tms Rmn and Courier, at least the
//! system font's height, drawn with the scaled faces; System fonts and
//! entry 0 are the system font. Styles (`:hp1.` ...) are the logical
//! fonts' fsSelection: italic, bold, underscored.

use crate::pm::*;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use ipf::bitmap::Bitmap;
use ipf::layout::{FontMetrics, Metrics, Page, RunKind};

/// A font of the panel: face name and point size (height, width).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontSpec {
    pub face: Vec<u8>,
    pub height: u16,
    pub width: u16,
}

/// The fonts of `doc` (its font table: numbers 0-15).
pub fn doc_fonts(doc: &ipf::Doc<'_>) -> Vec<FontSpec> {
    doc.fonts().into_iter().take(16).map(|f| FontSpec { face: f.face, height: f.height, width: f.width }).collect()
}

/// rs2's face and pixel height (0: the system font as it is) for font
/// `f`.
fn resolve(f: &FontSpec) -> (&'static [u8], i32) {
    let face = f.face.to_ascii_uppercase();
    // Point sizes of IBM's image fonts.
    let (name, sizes): (&'static [u8], &[i32]) = if face.starts_with(b"HELV") {
        (b"Helvetica", &[8, 10, 12, 14, 18, 24])
    } else if face.starts_with(b"TMS RMN") || face.starts_with(b"TIMES") {
        (b"Times New Roman", &[8, 10, 12, 14, 18, 24])
    } else if face.starts_with(b"COURIER") {
        (b"Courier", &[8, 10, 12])
    } else {
        return (b"", 0);
    };
    if f.height == 0 {
        return (name, 0);
    }
    // Pixels at 96 dpi, matched on the font's height as IBM matches
    // lMaxBaselineExt.
    let want = f.height as i32 * 4 / 3;
    let pt = sizes.iter().copied().min_by_key(|&p| (p * 4 / 3 - want).abs()).unwrap_or(10);
    (name, (pt * 4 / 3).max(16))
}

/// Measures and selects fonts in a presentation space.
pub struct Gpi {
    pub hps: u32,
    sys: Option<FontMetrics>,
    pub fonts: Vec<FontSpec>,
    /// Logical fonts made (font, style), and the one selected.
    made: Vec<(u8, u8)>,
    cur: (u8, u8),
    metrics: BTreeMap<(u8, u8), FontMetrics>,
}

/// FATTRS fsSelection of a style (1 italic, 2 bold, 4 underline).
fn selection(style: u8) -> u16 {
    (style & 1 != 0) as u16 | ((style & 2 != 0) as u16) << 5 | ((style & 4 != 0) as u16) << 1
}

impl Gpi {
    pub fn new(hps: u32, fonts: Vec<FontSpec>) -> Self {
        Gpi { hps, sys: None, fonts, made: Vec::new(), cur: (0, 0), metrics: BTreeMap::new() }
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

    /// Select font `font` in `style` (logical font id 1 + font * 8 +
    /// style; the system font itself is 0).
    pub fn select(&mut self, font: u8, style: u8) {
        let (font, style) = (font.min(30), style & 7);
        if self.cur == (font, style) {
            return;
        }
        self.cur = (font, style);
        let (face, px) = match self.fonts.get(font as usize) {
            Some(f) if font != 0 => resolve(f),
            _ => (&b""[..], 0),
        };
        if face.is_empty() && style == 0 {
            unsafe { GpiSetCharSet(self.hps, 0) };
            return;
        }
        let lcid = 1 + font as i32 * 8 + style as i32;
        if !self.made.contains(&(font, style)) {
            // FATTRS: usRecordLength, fsSelection, lMatch, szFacename,
            // idRegistry, usCodePage, lMaxBaselineExt, lAveCharWidth,
            // fsType, fsFontUse.
            let mut fat = [0u8; 56];
            fat[0..2].copy_from_slice(&56u16.to_le_bytes());
            fat[2..4].copy_from_slice(&selection(style).to_le_bytes());
            let name: &[u8] = if face.is_empty() { b"System Proportional" } else { face };
            fat[8..8 + name.len()].copy_from_slice(name);
            fat[42..44].copy_from_slice(&850u16.to_le_bytes());
            if px != 0 {
                fat[54..56].copy_from_slice(&4u16.to_le_bytes());
            }
            unsafe { GpiCreateLogFont(self.hps, core::ptr::null(), lcid, fat.as_ptr()) };
            self.made.push((font, style));
        }
        unsafe { GpiSetCharSet(self.hps, lcid) };
        if px != 0 {
            // FIXED character box: the em square.
            let b = [px << 16, px << 16];
            unsafe { GpiSetCharBox(self.hps, b.as_ptr()) };
        }
    }

    fn query_metrics(&self) -> FontMetrics {
        let mut fm = [0u8; 208];
        unsafe { GpiQueryFontMetrics(self.hps, fm.len() as i32, fm.as_mut_ptr()) };
        let rd = |o: usize| i32::from_le_bytes([fm[o], fm[o + 1], fm[o + 2], fm[o + 3]]);
        let (desc, ave, ext) = (rd(80), rd(100), rd(112));
        FontMetrics { height: ext.max(8), space: ave.max(4), descent: desc.max(0) }
    }
}

impl Drop for Gpi {
    fn drop(&mut self) {
        if !self.made.is_empty() {
            unsafe {
                GpiSetCharSet(self.hps, 0);
                GpiDeleteSetId(self.hps, -1);
            }
        }
    }
}

impl Metrics for Gpi {
    fn system(&mut self) -> FontMetrics {
        if let Some(m) = self.sys {
            return m;
        }
        self.select(0, 0);
        let m = self.query_metrics();
        self.sys = Some(m);
        m
    }

    fn font(&mut self, font: u8, style: u8) -> FontMetrics {
        if let Some(m) = self.metrics.get(&(font, style)) {
            return *m;
        }
        self.select(font, style);
        let m = FontMetrics { space: self.text_width(b" ").max(1), ..self.query_metrics() };
        self.metrics.insert((font, style), m);
        m
    }

    fn width(&mut self, font: u8, style: u8, word: &[u8]) -> i32 {
        self.select(font, style);
        self.text_width(word)
    }

    /// Fonts of dynamic data are numbers 16 and up ("default": 0).
    fn face(&mut self, face: &[u8], width: u16, height: u16) -> u8 {
        if face.eq_ignore_ascii_case(b"default") {
            return 0;
        }
        let f = FontSpec { face: face.to_vec(), height, width };
        while self.fonts.len() < 16 {
            self.fonts.push(FontSpec { face: Vec::new(), height: 0, width: 0 });
        }
        if let Some(i) = self.fonts[16..].iter().position(|x| *x == f) {
            return 16 + i as u8;
        }
        if self.fonts.len() >= 31 {
            return 0;
        }
        self.fonts.push(f);
        self.fonts.len() as u8 - 1
    }

    fn image(&mut self, hbm: u32) -> (i32, i32) {
        let mut b = [0u8; 12];
        b[0] = 12;
        if unsafe { GpiQueryBitmapParameters(hbm, b.as_mut_ptr()) } == 0 {
            return (0, 0);
        }
        (u16::from_le_bytes([b[4], b[5]]) as i32, u16::from_le_bytes([b[6], b[7]]) as i32)
    }
}

/// The system font's average character width and line height (the
/// character unit of viewport sizes).
pub fn char_size() -> (i32, i32) {
    let hps = unsafe { WinGetPS(HWND_DESKTOP) };
    let m = Gpi::new(hps, Vec::new()).system();
    unsafe { WinReleasePS(hps) };
    (m.space, m.height)
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

/// Dynamic data's metafiles played into bitmaps: HMF, width, height and
/// the bitmap.
pub type MetaBitmaps = Vec<(u32, i32, i32, u32)>;

pub fn free_metafiles(m: &mut MetaBitmaps) {
    for (_, _, _, hbm) in m.drain(..) {
        unsafe { GpiDeleteBitmap(hbm) };
    }
}

/// FIXED a / b.
fn fixed_div(a: i32, b: i32) -> i32 {
    if b == 0 {
        return 0x10000;
    }
    ((a as i64) * 0x10000 / b as i64) as i32
}

/// Metafile `hmf` played into a new bitmap of `cx` by `cy` pels (IBM's
/// MtaPlayMetaFile, which DdfFormatCell has make a bitmap of the
/// metafile's rectangle): its boundary is found by playing it without
/// a device, then the default viewing transform centres that boundary
/// (and 5 pels around it) on the bitmap, scaled alike in x and y to fit
/// (MtaSetTransform), and it is played again (its fonts loaded, its
/// colour table taken) onto the erased bitmap. 0 if it cannot be made.
pub fn metafile_bitmap(hmf: u32, cx: i32, cy: i32) -> u32 {
    if cx <= 0 || cy <= 0 {
        return 0;
    }
    // OD_MEMORY.
    let hdc = unsafe { DevOpenDC(0, 8, b"*\0".as_ptr(), 0, core::ptr::null(), 0) };
    if hdc == 0 {
        return 0;
    }
    let size = [cx, cy];
    // PU_PELS | GPIA_ASSOC.
    let hps = unsafe { GpiCreatePS(0, hdc, size.as_ptr(), 0x4008) };
    if hps == 0 {
        unsafe { DevCloseDC(hdc) };
        return 0;
    }
    // PMF_SEGBASE, PMF_LOADTYPE, PMF_RESOLVE, PMF_LCIDS (LC_LOADDISC),
    // PMF_RESET, PMF_SUPPRESS, PMF_COLORTABLES (CTAB_REPLACE),
    // PMF_COLORREALIZABLE: IBM's DoDraw.
    let opts: [i32; 8] = [0, 0, 0, 3, 0, 0, 3, 0];
    let mut desc = [0u8; 250];
    let mut segs = 0i32;
    let play = |hps: u32, segs: &mut i32, desc: &mut [u8; 250]| unsafe { GpiPlayMetaFile(hps, hmf, 8, opts.as_ptr(), segs, 250, desc.as_mut_ptr()) };
    let mut b = RectL::default();
    unsafe {
        GpiAssociate(hps, 0);
        // DCTL_BOUNDARY.
        GpiSetDrawControl(hps, 3, 1);
        play(hps, &mut segs, &mut desc);
        GpiQueryBoundaryData(hps, &mut b);
        GpiSetDrawControl(hps, 3, 0);
        GpiAssociate(hps, hdc);
    }
    let mut fmt = [1i32, 24];
    unsafe { GpiQueryDeviceBitmapFormats(hps, 2, fmt.as_mut_ptr()) };
    let mut hdr = [0u8; 12];
    hdr[0] = 12;
    hdr[4..6].copy_from_slice(&(cx as u16).to_le_bytes());
    hdr[6..8].copy_from_slice(&(cy as u16).to_le_bytes());
    hdr[8..10].copy_from_slice(&(fmt[0] as u16).to_le_bytes());
    hdr[10..12].copy_from_slice(&(fmt[1] as u16).to_le_bytes());
    let hbm = unsafe { GpiCreateBitmap(hps, hdr.as_ptr(), 0, core::ptr::null(), core::ptr::null()) };
    if hbm != 0 && unsafe { GpiSetBitmap(hps, hbm) } != u32::MAX {
        let (l, r, bt, t) = (b.l - 5, b.r + 5, b.b - 5, b.t + 5);
        // The boundary's centre to the origin, ...
        let m: [i32; 9] = [0x10000, 0, 0, 0, 0x10000, 0, -(l + (r - l) / 2), -(bt + (t - bt) / 2), 1];
        unsafe { GpiSetDefaultViewMatrix(hps, 9, m.as_ptr(), 0) };
        // ... scaled to fit (the smaller of the two scales), ...
        let (bw, bh) = (r - l, t - bt);
        let k = if fixed_div(bh, bw) < fixed_div(cy, cx) { fixed_div(cx, bw) } else { fixed_div(cy, bh) };
        let m: [i32; 9] = [k, 0, 0, 0, k, 0, 0, 0, 1];
        unsafe { GpiSetDefaultViewMatrix(hps, 9, m.as_ptr(), 1) };
        // ... to the centre of the bitmap.
        let m: [i32; 9] = [0x10000, 0, 0, 0, 0x10000, 0, cx / 2, cy / 2, 1];
        unsafe {
            GpiSetDefaultViewMatrix(hps, 9, m.as_ptr(), 1);
            GpiErase(hps);
        }
        play(hps, &mut segs, &mut desc);
        unsafe { GpiSetBitmap(hps, 0) };
    }
    unsafe {
        GpiAssociate(hps, 0);
        GpiDestroyPS(hps);
        DevCloseDC(hdc);
    }
    hbm
}

/// What to draw: the page scrolled by `top` pixels into a window of
/// `w` by `h`, with link `sel` marked.
pub struct Draw<'a, 'p> {
    pub hps: u32,
    /// The panel's fonts, selected in `hps`.
    pub g: Gpi,
    pub page: &'a Page<'p>,
    pub top: i32,
    pub w: i32,
    pub h: i32,
    pub sel: Option<u16>,
    pub data: &'p [u8],
    pub image_start: u32,
    pub lib: usize,
    pub bitmaps: &'a mut Bitmaps,
    pub metafiles: &'a mut MetaBitmaps,
    /// Pels scrolled to the left.
    pub left: i32,
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
                    let x = r.x - self.left;
                    if x + r.width < 0 || x > self.w {
                        continue;
                    }
                    if r.bg != 0 {
                        let fill = RectL { l: x, b: base - r.descent, r: x + r.width, t: base - r.descent + r.height };
                        unsafe { WinFillRect(hps, &fill, colour(r.bg, SYSCLR_HELPBACKGROUND)) };
                    }
                    let c = if r.link.is_some() { SYSCLR_HELPHILITE } else { colour(r.fg, SYSCLR_HELPTEXT) };
                    unsafe { GpiSetColor(hps, c) };
                    self.g.select(r.font, r.style);
                    let p = PointL { x, y: base };
                    unsafe { GpiCharStringAt(hps, &p, w.len() as i32, w.as_ptr()) };
                }
                RunKind::Image { handle, metafile, cx, cy, fit } => {
                    let (x0, y0, x1, y1) = if fit {
                        (0, 0, self.w, self.h)
                    } else {
                        let b = self.wy(r.y);
                        (r.x - self.left, b, r.x - self.left + cx, b + cy)
                    };
                    if y1 < 0 || y0 > self.h {
                        continue;
                    }
                    if metafile {
                        // Played into a bitmap of the run's size (the
                        // window's for `fit`), kept until the page goes.
                        let (cx, cy) = (x1 - x0, y1 - y0);
                        let hbm = match self.metafiles.iter().find(|m| (m.0, m.1, m.2) == (handle, cx, cy)) {
                            Some(m) => m.3,
                            None => {
                                // One bitmap per metafile: an older size goes.
                                if let Some(i) = self.metafiles.iter().position(|m| m.0 == handle) {
                                    unsafe { GpiDeleteBitmap(self.metafiles.remove(i).3) };
                                }
                                let hbm = metafile_bitmap(handle, cx, cy);
                                if hbm != 0 {
                                    self.metafiles.push((handle, cx, cy, hbm));
                                }
                                hbm
                            }
                        };
                        if hbm != 0 {
                            unsafe { WinDrawBitmap(hps, hbm, core::ptr::null(), &PointL { x: x0, y: y0 }, 0, 0, DBM_NORMAL) };
                        }
                    } else if fit {
                        let dst = RectL { l: x0, b: y0, r: x1, t: y1 };
                        unsafe { WinDrawBitmap(hps, handle, core::ptr::null(), &dst as *const RectL as *const PointL, 0, 0, DBM_STRETCH) };
                    } else {
                        unsafe { WinDrawBitmap(hps, handle, core::ptr::null(), &PointL { x: x0, y: y0 }, 0, 0, DBM_NORMAL) };
                    }
                }
                RunKind::Bitmap { offset, cx, cy, fit, .. } => {
                    let (x0, y0, x1, y1) = if fit {
                        (0, 0, self.w, self.h)
                    } else {
                        let b = self.wy(r.y);
                        (r.x - self.left, b, r.x - self.left + cx, b + cy)
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
                let (x0, x1) = (x0 - self.left, x1 - self.left);
                unsafe {
                    GpiMove(hps, &PointL { x: x0 - 1, y: bb - 1 });
                    GpiBox(hps, DRO_OUTLINE, &PointL { x: x1, y: bt }, 0, 0);
                }
            }
        }
    }
}
