//! Text measurement and painting through GPI: webcore's fonts are GPI
//! outline fonts (rs2's Helvetica, Times New Roman and Courier faces, any
//! size, anti-aliased), its display list is drawn with GpiBox,
//! GpiCharStringAt, GpiFullArc and GpiDrawBits on an RGB presentation
//! space.

use std::collections::HashMap;

use webcore::css::Color;
use webcore::layout::{ControlKind, FontKey, Fonts, Item, Laid};
use webcore::style::{Family, ListStyle};

use crate::pm::*;

pub struct GpiFonts {
    pub hps: u32,
    lcids: HashMap<FontKey, i32>,
    current: Option<FontKey>,
    widths: HashMap<FontKey, Box<[f32; 256]>>,
    metrics: HashMap<FontKey, (f32, f32)>,
    next: i32,
}

fn face(f: FontKey) -> String {
    let base = match f.family {
        Family::Sans => "Helvetica",
        Family::Serif => "Times New Roman",
        Family::Mono => "Courier",
    };
    match (f.bold, f.italic) {
        (false, false) => base.to_string(),
        (true, false) => format!("{base} Bold"),
        (false, true) => format!("{base} Italic"),
        (true, true) => format!("{base} Bold Italic"),
    }
}

impl GpiFonts {
    pub fn new(hps: u32) -> GpiFonts {
        GpiFonts { hps, lcids: HashMap::new(), current: None, widths: HashMap::new(), metrics: HashMap::new(), next: 1 }
    }

    /// Make `f` the PS's font.
    pub fn select(&mut self, f: FontKey) {
        if self.current == Some(f) {
            return;
        }
        let id = match self.lcids.get(&f) {
            Some(&id) => id,
            None => {
                if self.next > 250 {
                    // Out of ids: start over.
                    unsafe { GpiSetCharSet(self.hps, 0) };
                    for (_, id) in self.lcids.drain() {
                        unsafe { GpiDeleteSetId(self.hps, id) };
                    }
                    self.next = 1;
                }
                let id = self.next;
                self.next += 1;
                let name = face(f);
                let attrs = FAttrs::outline(name.as_bytes());
                unsafe {
                    GpiSetCharSet(self.hps, 0);
                    GpiDeleteSetId(self.hps, id);
                    GpiCreateLogFont(self.hps, core::ptr::null(), id, &attrs);
                }
                self.lcids.insert(f, id);
                id
            }
        };
        unsafe {
            GpiSetCharSet(self.hps, id);
            GpiSetCharBox(self.hps, &SizeL { cx: (f.size as i32) << 16, cy: (f.size as i32) << 16 });
        }
        self.current = Some(f);
    }

    /// The PS was reset (GpiSetCharSet(0) by someone else).
    pub fn forget_current(&mut self) {
        self.current = None;
    }

    fn table(&mut self, f: FontKey) -> &[f32; 256] {
        if !self.widths.contains_key(&f) {
            self.select(f);
            let mut t = Box::new([0f32; 256]);
            for (c, w) in t.iter_mut().enumerate().skip(32) {
                let ch = [c as u8];
                let mut p = [PointL::default(); 5];
                unsafe { GpiQueryTextBox(self.hps, 1, ch.as_ptr(), 5, p.as_mut_ptr()) };
                // TXTBOX_CONCAT: where the next character starts.
                *w = p[4].x as f32;
            }
            self.widths.insert(f, t);
        }
        &self.widths[&f]
    }
}

impl Fonts for GpiFonts {
    fn width(&mut self, f: FontKey, text: &str) -> f32 {
        let bytes = webcore::codepage::encode(text, 850);
        let t = self.table(f);
        bytes.iter().map(|&b| t[b as usize]).sum()
    }

    fn metrics(&mut self, f: FontKey) -> (f32, f32) {
        if let Some(&m) = self.metrics.get(&f) {
            return m;
        }
        self.select(f);
        let mut fm = [0u8; 208];
        unsafe { GpiQueryFontMetrics(self.hps, fm.len() as i32, fm.as_mut_ptr()) };
        let rd = |o: usize| i32::from_le_bytes(fm[o..o + 4].try_into().unwrap());
        // FONTMETRICS: lMaxAscender at 0x4c, lMaxDescender at 0x50.
        let (mut a, mut d) = (rd(0x4c) as f32, rd(0x50) as f32);
        if a <= 0.0 {
            a = (f.size as f32 * 0.9).round();
            d = (f.size as f32 * 0.25).round();
        }
        self.metrics.insert(f, (a, d));
        (a, d)
    }
}

/// A decoded image, ready for GpiDrawBits.
pub struct Bitmap {
    pub w: u32,
    pub h: u32,
    /// 24-bit BGR rows from the bottom, each padded to 4 bytes.
    pub bits: Vec<u8>,
}

impl Bitmap {
    /// Alpha is blended over `bg` (no alpha in GpiDrawBits).
    pub fn from_image(img: &webcore::image::Image, bg: Color) -> Bitmap {
        let (w, h) = (img.w as usize, img.h as usize);
        let stride = (w * 3).div_ceil(4) * 4;
        let mut bits = vec![0u8; stride * h];
        let (br, bgc, bb) = ((bg >> 16) & 255, (bg >> 8) & 255, bg & 255);
        for y in 0..h {
            let row = &mut bits[(h - 1 - y) * stride..];
            for x in 0..w {
                let s = &img.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                let a = s[3] as u32;
                let mix = |c: u8, b: u32| ((c as u32 * a + b * (255 - a)) / 255) as u8;
                row[x * 3] = mix(s[2], bb);
                row[x * 3 + 1] = mix(s[1], bgc);
                row[x * 3 + 2] = mix(s[0], br);
            }
        }
        Bitmap { w: img.w, h: img.h, bits }
    }
}

/// What a form control shows.
#[derive(Clone, Default)]
pub struct ControlState {
    pub value: String,
    pub checked: bool,
}

pub struct Painter<'a> {
    pub hps: u32,
    pub fonts: &'a mut GpiFonts,
    /// The view's height and the scroll position (page y at the top).
    pub height: i32,
    pub scroll: i32,
    pub x_off: i32,
}

fn rgb(c: Color, bg: Color) -> i32 {
    let a = c >> 24;
    if a >= 255 {
        return (c & 0xff_ffff) as i32;
    }
    let mix = |s: u32, d: u32| (s * a + d * (255 - a)) / 255;
    (mix(c >> 16 & 255, bg >> 16 & 255) << 16 | mix(c >> 8 & 255, bg >> 8 & 255) << 8 | mix(c & 255, bg & 255)) as i32
}

impl Painter<'_> {
    /// PM y of page y.
    fn y(&self, y: f32) -> i32 {
        self.height - (y.round() as i32 - self.scroll)
    }

    pub fn fill(&self, x: f32, y: f32, w: f32, h: f32, color: i32) {
        let (x0, x1) = (x.round() as i32 + self.x_off, (x + w).round() as i32 + self.x_off);
        let (yt, yb) = (self.y(y), self.y(y + h));
        if x1 <= x0 || yt <= yb {
            return;
        }
        unsafe {
            GpiSetColor(self.hps, color);
            GpiMove(self.hps, &pt(x0, yb));
            GpiBox(self.hps, DRO_FILL, &pt(x1 - 1, yt - 1), 0, 0);
        }
    }

    fn frame(&self, x: f32, y: f32, w: f32, h: f32, light: i32, dark: i32) {
        self.fill(x, y, w, 1.0, dark);
        self.fill(x, y, 1.0, h, dark);
        self.fill(x, y + h - 1.0, w, 1.0, light);
        self.fill(x + w - 1.0, y, 1.0, h, light);
    }

    pub fn text(&mut self, x: f32, y: f32, s: &str, font: FontKey, color: i32) {
        let b = webcore::codepage::encode(s, 850);
        if b.is_empty() {
            return;
        }
        self.fonts.select(font);
        unsafe {
            GpiSetColor(self.hps, color);
            GpiCharStringAt(self.hps, &pt(x.round() as i32 + self.x_off, self.y(y)), b.len() as i32, b.as_ptr());
        }
    }

    fn ellipse(&self, x: f32, y: f32, w: f32, h: f32, color: i32, filled: bool) {
        let (cx, cy) = ((x + w / 2.0).round() as i32 + self.x_off, self.y(y + h / 2.0));
        let r = (w / 2.0).max(1.0) as i32;
        let params = [r, r, 0, 0];
        unsafe {
            GpiSetColor(self.hps, color);
            GpiSetArcParams(self.hps, params.as_ptr());
            GpiMove(self.hps, &pt(cx, cy));
            GpiFullArc(self.hps, if filled { DRO_FILL } else { 2 }, 0x10000);
        }
    }

    /// Paint the items that meet the page band [top, bottom).
    pub fn paint(
        &mut self,
        laid: &Laid,
        top: f32,
        bottom: f32,
        images: &HashMap<usize, Bitmap>,
        controls: &HashMap<usize, ControlState>,
        dom: &webcore::dom::Dom,
    ) {
        let bg = laid.background;
        for it in &laid.items {
            match it {
                Item::Fill { r, color } => {
                    if r.y < bottom && r.y + r.h > top {
                        self.fill(r.x, r.y, r.w, r.h, rgb(*color, bg));
                    }
                }
                Item::Text { x, y, w, text, font, color, decoration } => {
                    let fs = font.size as f32;
                    if *y - fs * 1.2 > bottom || *y + fs * 0.5 < top {
                        continue;
                    }
                    let c = rgb(*color, bg);
                    self.text(*x, *y, text, *font, c);
                    let lw = (fs / 14.0).round().max(1.0);
                    if decoration & 1 != 0 {
                        self.fill(*x, y + 1.0 + fs / 16.0, *w, lw, c);
                    }
                    if decoration & 2 != 0 {
                        self.fill(*x, y - fs * 0.3, *w, lw, c);
                    }
                    if decoration & 4 != 0 {
                        self.fill(*x, y - fs * 0.9, *w, lw, c);
                    }
                }
                Item::Marker { r, kind, color } => {
                    if r.y < bottom && r.y + r.h > top {
                        let c = rgb(*color, bg);
                        match kind {
                            ListStyle::Square => self.fill(r.x, r.y, r.w, r.h, c),
                            ListStyle::Circle => self.ellipse(r.x, r.y, r.w, r.h, c, false),
                            _ => self.ellipse(r.x, r.y, r.w, r.h, c, true),
                        }
                    }
                }
                Item::Image { r, node } => {
                    if r.y >= bottom || r.y + r.h <= top {
                        continue;
                    }
                    match images.get(node) {
                        Some(b) => {
                            let bih = BitmapInfoHeader2::new(b.w, b.h, 24);
                            let (x0, yb) = (r.x.round() as i32 + self.x_off, self.y(r.y + r.h));
                            let (x1, yt) = ((r.x + r.w).round() as i32 + self.x_off, self.y(r.y));
                            let p = [pt(x0, yb), pt(x1 - 1, yt - 1), pt(0, 0), pt(b.w as i32, b.h as i32)];
                            unsafe {
                                GpiDrawBits(self.hps, b.bits.as_ptr(), &bih as *const BitmapInfoHeader2 as *const u8, 4, p.as_ptr(), ROP_SRCCOPY, BBO_IGNORE);
                            }
                        }
                        None => self.frame(r.x, r.y, r.w, r.h, 0xc0c0c0, 0x808080),
                    }
                }
                Item::Control { r, node, kind, font } => {
                    if r.y >= bottom || r.y + r.h <= top {
                        continue;
                    }
                    let st = controls.get(node).cloned().unwrap_or_default();
                    self.control(r.x, r.y, r.w, r.h, *kind, *font, &st, *node, dom);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn control(&mut self, x: f32, y: f32, w: f32, h: f32, kind: ControlKind, font: FontKey, st: &ControlState, node: usize, dom: &webcore::dom::Dom) {
        let (a, d) = self.fonts.metrics(font);
        let base = y + (h + a - d) / 2.0;
        match kind {
            ControlKind::Text | ControlKind::Password | ControlKind::TextArea => {
                self.fill(x, y, w, h, 0xffffff);
                self.frame(x, y, w, h, 0xe0e0e0, 0x707070);
                let shown = if kind == ControlKind::Password { "*".repeat(st.value.chars().count()) } else { st.value.clone() };
                let ty = if kind == ControlKind::TextArea { y + 4.0 + a } else { base };
                self.text(x + 4.0, ty, &shown, font, 0);
            }
            ControlKind::Checkbox | ControlKind::Radio => {
                self.fill(x, y, w, h, 0xffffff);
                self.frame(x, y, w, h, 0xe0e0e0, 0x606060);
                if st.checked {
                    self.fill(x + 3.0, y + 3.0, w - 6.0, h - 6.0, 0x202020);
                }
            }
            ControlKind::Select => {
                self.fill(x, y, w, h, 0xffffff);
                self.frame(x, y, w, h, 0xe0e0e0, 0x707070);
                self.fill(x + w - 18.0, y + 1.0, 17.0, h - 2.0, 0xd8d8d8);
                // A small down arrow.
                for i in 0..4 {
                    let f = i as f32;
                    self.fill(x + w - 13.0 + f, y + h / 2.0 - 2.0 + f, 8.0 - 2.0 * f, 1.0, 0x202020);
                }
                self.text(x + 4.0, base, &st.value, font, 0);
            }
            ControlKind::Submit | ControlKind::Button | ControlKind::Image => {
                self.fill(x, y, w, h, 0xdcdcdc);
                self.frame(x, y, w, h, 0x606060, 0xffffff);
                let label = if kind == ControlKind::Button && dom.element(node).is_some_and(|e| e.is("button")) {
                    dom.text_content(node).split_whitespace().collect::<Vec<_>>().join(" ")
                } else {
                    st.value.clone()
                };
                let tw = self.fonts.width(font, &label);
                self.text(x + (w - tw) / 2.0, base, &label, font, 0);
            }
        }
    }
}
