//! Fonts. The built-in font is unscii-16 (8x16, public domain) laid out
//! in code page 850, the system's default; OS/2 image fonts (the RT_FONT
//! resources of .FON files) load with `Font::from_os2`; outline fonts
//! (TrueType, `ttf.rs`; Type 1, `type1.rs`) are rendered at a size with
//! `Font::from_outline`, anti-aliased: each glyph a coverage map.
//! PMWIN installs the system font (`set_system`); until then it is
//! unscii.

use alloc::vec::Vec;

/// Records of 20 bytes: Unicode code point (u32 LE) and 16 rows.
static UNSCII: &[u8] = include_bytes!("../../../kernel/assets/unscii-16.bin");

const CP850_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐\
└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}";

/// A bitmap font: the built-in monospaced one, or a loaded image font
/// with a glyph and width per character.
pub struct Font {
    pub name: &'static str,
    /// Character width (the average for a proportional font).
    pub width: i32,
    pub height: i32,
    /// Pixels below the baseline.
    pub descender: i32,
    glyphs: [[u8; 16]; 256],
    loaded: Option<Vec<Glyph>>,
    /// An outline font's glyphs at this size (coverage maps).
    rendered: Option<Vec<crate::ttf::Rendered>>,
    /// The outlines it was rendered from (other sizes come from them).
    pub source: Option<&'static Source>,
    /// Pels to the em (for outline fonts, the size it was made at).
    pub em: i32,
}

/// What an outline font file offers: TrueType or Type 1.
pub trait Outlines: Sync {
    /// Pels per font unit for an em of `px` pels.
    fn scale_for(&self, px: f64) -> f64;
    /// A character's glyph at `scale`, emboldened by `bold` pels.
    fn render(&self, ch: char, scale: f64, bold: f64) -> crate::ttf::Rendered;
    /// (ascender, descender, average width) in font units.
    fn metrics(&self) -> (i32, i32, i32);
}

impl Outlines for crate::ttf::Ttf {
    fn scale_for(&self, px: f64) -> f64 {
        crate::ttf::Ttf::scale_for(self, px)
    }
    fn render(&self, ch: char, scale: f64, bold: f64) -> crate::ttf::Rendered {
        crate::ttf::Ttf::render(self, ch, scale, bold)
    }
    fn metrics(&self) -> (i32, i32, i32) {
        (self.ascender, self.descender, self.avg_width)
    }
}

/// An outline face as fonts are made from it: the outlines, simulated
/// bold (a fraction of the em), and the sizes made so far (shared by the
/// processes that use them; touched only with PM's lock held).
pub struct Source {
    pub name: &'static str,
    pub outlines: &'static dyn Outlines,
    pub bold: f64,
    sizes: core::cell::UnsafeCell<Vec<(i32, &'static Font)>>,
}

unsafe impl Sync for Source {}

/// At most this many sizes of a face are kept; others are scaled.
const MAX_SIZES: usize = 32;

impl Source {
    pub fn new(name: &'static str, outlines: &'static dyn Outlines, bold: f64) -> Source {
        Source { name, outlines, bold, sizes: core::cell::UnsafeCell::new(Vec::new()) }
    }

    /// The face at an em of `px` pels (made once, then kept).
    pub fn at(&'static self, px: i32) -> Option<&'static Font> {
        if !(4..=400).contains(&px) {
            return None;
        }
        let sizes = unsafe { &mut *self.sizes.get() };
        if let Some(&(_, f)) = sizes.iter().find(|(p, _)| *p == px) {
            return Some(f);
        }
        if sizes.len() >= MAX_SIZES {
            return None;
        }
        let f: &'static Font = alloc::boxed::Box::leak(alloc::boxed::Box::new(self.make(px)));
        sizes.push((px, f));
        Some(f)
    }

    fn make(&'static self, px: i32) -> Font {
        let s = self.outlines.scale_for(px as f64);
        let (a, d, w) = self.outlines.metrics();
        let up = |v: i32| crate::math::ceil(v as f64 * s) as i32;
        let bold = self.bold * px as f64;
        let mut f = Font::from_outline(self.name, up(a), up(d), crate::math::round(w as f64 * s + bold), &|c| self.outlines.render(c, s, bold));
        f.source = Some(self);
        f.em = px;
        f
    }
}

/// A loaded font's glyph: its width (also its advance) and rows of
/// `(width + 7) / 8` bytes from the top, bit 7 leftmost.
#[derive(Clone, Default)]
struct Glyph {
    width: i32,
    rows: Vec<u8>,
}

/// What a loaded OS/2 font says about itself (FOCAMETRICS).
#[derive(Clone)]
pub struct Os2FontInfo {
    pub family: Vec<u8>,
    pub face: Vec<u8>,
    /// The FOCAMETRICS fields from usRegistryId on (SHORTs, as stored).
    pub fields: Vec<i16>,
}

fn lookup(c: char) -> Option<[u8; 16]> {
    let n = UNSCII.len() / 20;
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let rec = &UNSCII[mid * 20..mid * 20 + 20];
        let cp = u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]);
        match cp.cmp(&(c as u32)) {
            core::cmp::Ordering::Equal => return rec[4..20].try_into().ok(),
            core::cmp::Ordering::Less => lo = mid + 1,
            core::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

/// The graphic characters of code page 850's control positions
/// (0x01-0x1F, as OS/2's fonts have them; Klondike draws its suits with
/// them). BEL to CR stay blank: text passes tabs and line ends.
const CP850_LOW: &str = "\u{263a}\u{263b}\u{2665}\u{2666}\u{2663}\u{2660}       \u{266b}\u{263c}\u{25ba}\u{25c4}\u{2195}\u{203c}\u{b6}\u{a7}\u{25ac}\u{21a8}\u{2191}\u{2193}\u{2192}\u{2190}\u{221f}\u{2194}\u{25b2}\u{25bc}";

fn cp850(b: u8) -> char {
    match b {
        0x01..=0x1f => CP850_LOW.chars().nth((b - 1) as usize).unwrap_or(' '),
        0x20..=0x7e => b as char,
        0x80..=0xff => CP850_HIGH.chars().nth((b - 0x80) as usize).unwrap_or('?'),
        _ => ' ',
    }
}

impl Font {
    fn build() -> Font {
        let mut glyphs = [[0u8; 16]; 256];
        for (b, g) in glyphs.iter_mut().enumerate() {
            *g = lookup(cp850(b as u8)).or_else(|| lookup('?')).unwrap_or([0; 16]);
        }
        Font { name: "System VIO", width: 8, height: 16, descender: 4, glyphs, loaded: None, rendered: None, source: None, em: 16 }
    }

    /// An OS/2 image font resource (FONTSIGNATURE "OS/2 FONT",
    /// FOCAMETRICS, FONTDEFINITIONHEADER, the character definitions and
    /// the glyphs: per glyph, columns of 8 pels, each `yCellHeight`
    /// bytes from the top).
    pub fn from_os2(b: &[u8]) -> Option<(Font, Os2FontInfo)> {
        let u32_at = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
        let i16_at = |o: usize| b.get(o..o + 2).map(|x| i16::from_le_bytes([x[0], x[1]]));
        if u32_at(0)? != 0xffff_fffe || b.get(8..17)? != b"OS/2 FONT" {
            return None;
        }
        let m = u32_at(4)? as usize;
        if u32_at(m)? != 1 {
            return None;
        }
        let msize = u32_at(m + 4)? as usize;
        let cstr = |o: usize| -> Option<Vec<u8>> {
            let x = b.get(o..o + 32)?;
            Some(x[..x.iter().position(|&c| c == 0).unwrap_or(32)].to_vec())
        };
        let (family, face) = (cstr(m + 8)?, cstr(m + 40)?);
        let fields: Vec<i16> = (0..46).map(|i| i16_at(m + 72 + 2 * i).unwrap_or(0)).collect();
        let (first, last) = (fields[21].max(0) as usize, fields[22].max(0) as usize);
        let d = m + msize;
        if u32_at(d)? != 2 {
            return None;
        }
        let chardef = i16_at(d + 10)? as u16;
        let cell = i16_at(d + 12)?.max(6) as usize;
        let (cw, ch, inc) = (i16_at(d + 14)? as i32, i16_at(d + 16)? as i32, i16_at(d + 18)? as i32);
        let base = i16_at(d + 26)? as i32;
        if !(1..=256).contains(&ch) {
            return None;
        }
        // The character definitions follow the 28-byte header (ulSize
        // counts them and the glyphs too).
        let defs = d + 28;
        let mut glyphs = alloc::vec![Glyph::default(); 256];
        for i in 0..=last {
            let c = first + i;
            if c > 255 {
                break;
            }
            let o = defs + i * cell;
            let off = u32_at(o)? as usize;
            // FONTDEFCHAR3 has A, B and C spaces; the others the width.
            let w = if chardef & 0x0038 != 0 { i16_at(o + 6)? as i32 } else { i16_at(o + 4)? as i32 };
            let w = if w <= 0 { cw.max(inc) } else { w };
            let stride = ((w + 7) / 8) as usize;
            let mut rows = alloc::vec![0u8; stride * ch as usize];
            for col in 0..stride {
                for r in 0..ch as usize {
                    rows[r * stride + col] = *b.get(off + col * ch as usize + r).unwrap_or(&0);
                }
            }
            glyphs[c] = Glyph { width: w, rows };
        }
        // Characters the font lacks are its default character.
        let def = (first + fields[23].max(0) as usize).min(255);
        let dflt = glyphs[def].clone();
        for g in glyphs.iter_mut().filter(|g| g.width == 0) {
            *g = dflt.clone();
        }
        let avg = if fields[10] > 0 { fields[10] as i32 } else { cw.max(1) };
        let name: &'static str = alloc::boxed::Box::leak(alloc::string::String::from_utf8_lossy(&face).into_owned().into_boxed_str());
        let font = Font { name, width: avg, height: ch, descender: (ch - base).max(0), glyphs: [[0; 16]; 256], loaded: Some(glyphs), rendered: None, source: None, em: ch };
        Some((font, Os2FontInfo { family, face, fields }))
    }

    /// An outline font at `px` pels to the em: the characters of code
    /// page 850 rendered by `render` (character, scale) at the scale
    /// `scale`; `ascender` and `descender` (positive) in pels give the
    /// cell; `avg` is the average character width in pels.
    pub fn from_outline(name: &'static str, ascender: i32, descender: i32, avg: i32, render: &dyn Fn(char) -> crate::ttf::Rendered) -> Font {
        let mut v = Vec::with_capacity(256);
        for b in 0..=255u8 {
            let mut r = if b == 0 { crate::ttf::Rendered::default() } else { render(cp850(b)) };
            // Line ends and tabs have no ink.
            if matches!(b, 0x07..=0x0d) {
                r = crate::ttf::Rendered { advance: r.advance, ..Default::default() };
            }
            v.push(r);
        }
        Font { name, width: avg.max(1), height: ascender + descender, descender, glyphs: [[0; 16]; 256], loaded: None, rendered: Some(v), source: None, em: 0 }
    }

    /// Whether glyphs have partial coverage (draw with `coverage`).
    pub fn smooth(&self) -> bool {
        self.rendered.is_some()
    }

    /// The columns (relative to the pen) a glyph may ink: (first, count).
    pub fn ink(&self, b: u8) -> (i32, i32) {
        match &self.rendered {
            Some(g) => (g[b as usize].left, g[b as usize].w),
            None => (0, self.char_width(b)),
        }
    }

    /// Coverage 0..=255 of pel (`col`, `r`) of character `b` (`col`
    /// relative to the pen, `r` from the top of the cell).
    pub fn coverage(&self, b: u8, r: i32, col: i32) -> u8 {
        match &self.rendered {
            Some(g) => {
                let g = &g[b as usize];
                // The glyph's top row in cell rows.
                let x = col - g.left;
                let y = r - (self.height - self.descender - g.top);
                if x < 0 || y < 0 || x >= g.w || y >= g.h {
                    0
                } else {
                    g.cov[(y * g.w + x) as usize]
                }
            }
            None => {
                if self.pixel(b, r, col) {
                    255
                } else {
                    0
                }
            }
        }
    }

    /// Row `r` (0 = top) of the glyph for byte `b`; bit 7 is the leftmost
    /// pixel (the first 8 columns of a loaded font's glyph).
    pub fn row(&self, b: u8, r: usize) -> u8 {
        if self.rendered.is_some() {
            return (0..8).fold(0, |a, c| a | if self.pixel(b, r as i32, c) { 0x80 >> c } else { 0 });
        }
        match &self.loaded {
            Some(g) => g[b as usize].rows.get(r * ((g[b as usize].width + 7) / 8) as usize).copied().unwrap_or(0),
            None => self.glyphs[b as usize][r],
        }
    }

    /// Width (and advance) of character `b`.
    pub fn char_width(&self, b: u8) -> i32 {
        if let Some(g) = &self.rendered {
            return g[b as usize].advance;
        }
        match &self.loaded {
            Some(g) => g[b as usize].width,
            None => self.width,
        }
    }

    /// Whether pel (`col`, `r`) of character `b`'s glyph is set (0, 0 is
    /// its top left).
    pub fn pixel(&self, b: u8, r: i32, col: i32) -> bool {
        if r < 0 || r >= self.height {
            return false;
        }
        if self.rendered.is_some() {
            return self.coverage(b, r, col) >= 128;
        }
        if col < 0 {
            return false;
        }
        match &self.loaded {
            Some(g) => {
                let g = &g[b as usize];
                let stride = (g.width + 7) / 8;
                col < g.width && g.rows.get((r * stride + col / 8) as usize).is_some_and(|&x| x & (0x80 >> (col % 8)) != 0)
            }
            None => col < 8 && self.glyphs[b as usize][r as usize] & (0x80 >> col) != 0,
        }
    }

    pub fn text_width(&self, s: &[u8]) -> i32 {
        if self.loaded.is_some() || self.rendered.is_some() {
            s.iter().map(|&b| self.char_width(b)).sum()
        } else {
            self.width * s.len() as i32
        }
    }
}

static mut VIO: Option<Font> = None;
static SYSTEM: core::sync::atomic::AtomicPtr<Font> = core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// The built-in monospaced font, unscii (built on first use; not thread
/// safe on the very first call, which PMGPI makes during
/// initialisation).
pub fn vio() -> &'static Font {
    unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(VIO);
        slot.get_or_insert_with(Font::build)
    }
}

/// The system font: the one installed with `set_system`, else unscii.
pub fn system() -> &'static Font {
    let p = SYSTEM.load(core::sync::atomic::Ordering::Acquire);
    if p.is_null() { vio() } else { unsafe { &*p } }
}

/// Make `f` the system font (fonts already handed out stay valid).
pub fn set_system(f: &'static Font) {
    SYSTEM.store(f as *const Font as *mut Font, core::sync::atomic::Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An OS/2 font with characters 'A' (10 pels wide, a vertical bar at
    /// column 9) and 'B' (missing: the default), 4 pels high, baseline
    /// 3 pels from the top.
    fn sample() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&0xffff_fffeu32.to_le_bytes());
        b.extend_from_slice(&20u32.to_le_bytes());
        b.extend_from_slice(b"OS/2 FONT\0\0\0");
        let m = b.len();
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&168u32.to_le_bytes());
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"Test");
        b.extend_from_slice(&name);
        b.extend_from_slice(&name);
        let mut f = [0i16; 46];
        f[10] = 10; // xAveCharWidth
        f[21] = b'A' as i16; // usFirstChar
        f[22] = 1; // usLastChar (relative)
        f[23] = 0; // usDefaultChar (relative)
        for v in f {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.resize(m + 168, 0);
        let d = b.len();
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&48u32.to_le_bytes());
        for v in [0x47i16, 0x81, 6, 10, 4, 10, 0, 0, 0, 3] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(b.len(), d + 28);
        let glyph = b.len() + 12;
        b.extend_from_slice(&(glyph as u32).to_le_bytes());
        b.extend_from_slice(&10u16.to_le_bytes());
        b.extend_from_slice(&(glyph as u32).to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        // Column bytes 0-7 (empty), then 8-15 (bit 6 = column 9).
        b.extend_from_slice(&[0, 0, 0, 0, 0x40, 0x40, 0x40, 0x40]);
        b
    }

    #[test]
    fn os2_font() {
        let (f, info) = Font::from_os2(&sample()).expect("parses");
        assert_eq!(info.face, b"Test");
        assert_eq!((f.height, f.descender, f.width), (4, 1, 10));
        assert_eq!(f.char_width(b'A'), 10);
        assert!(f.pixel(b'A', 0, 9) && f.pixel(b'A', 3, 9) && !f.pixel(b'A', 0, 8));
        // 'B' has width 0 in the table: the font's cell width.
        assert_eq!(f.char_width(b'B'), 10);
        assert_eq!(f.text_width(b"AA"), 20);
        assert!(Font::from_os2(b"not a font").is_none());
    }
}

impl core::fmt::Debug for Font {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Font({} {}x{})", self.name, self.width, self.height)
    }
}
