//! Icons and pointers: the OS/2 pointer/icon file format (.ICO, .PTR and
//! RT_POINTER resources), choosing the image for a display, scaling,
//! the AND/XOR mask bitmaps and drawing.
//!
//! A file is either one image or an array of them, each behind a
//! BITMAPARRAYFILEHEADER ("BA": cbSize, offNext, cxDisplay, cyDisplay)
//! for a device resolution (0 x 0: any). An image starts with a
//! BITMAPFILEHEADER whose type is 'IC' (icon), 'PT' (pointer), 'CI'
//! (colour icon) or 'CP' (colour pointer), with the hot spot and the
//! offset of its bits (from the start of the file), and a 1.x
//! (BITMAPINFOHEADER) or 2.x (BITMAPINFOHEADER2) header with a colour
//! table. Its 1 bpp bitmap is twice the image's height: the lower half
//! (the first scan lines) is the XOR mask, the upper half the AND mask.
//! A colour image is followed by a second header for the colour bitmap.
//!
//! Monochrome:  AND 0 XOR 0 black, AND 0 XOR 1 white,
//!              AND 1 XOR 0 transparent, AND 1 XOR 1 inverted screen.
//! Colour:      AND 0: the colour pixel; AND 1: transparent (XOR 0) or
//!              inverted screen (XOR 1).

use alloc::vec;
use alloc::vec::Vec;

use crate::bitmap::{self, Bitmap, Info};
use crate::color::Rgb;
use crate::rect::{Point, Rect};
use crate::surface::Surface;

/// Pixel values of an [`Image`]: `OPAQUE | rgb`, `TRANSPARENT` or `INVERT`.
pub const TRANSPARENT: u32 = 0;
pub const INVERT: u32 = 0x0100_0000;
pub const OPAQUE: u32 = 0xff00_0000;

/// A decoded icon or pointer image, top row first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

impl Image {
    pub fn new(w: i32, h: i32) -> Image {
        Image { w, h, px: vec![TRANSPARENT; (w.max(0) * h.max(0)) as usize] }
    }

    /// Pixel at (x, y), y counted from the bottom as in PM.
    pub fn at(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return TRANSPARENT;
        }
        self.px[((self.h - 1 - y) * self.w + x) as usize]
    }

    pub fn set(&mut self, x: i32, y: i32, v: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[((self.h - 1 - y) * self.w + x) as usize] = v;
        }
    }

    /// Scaled to `w` x `h`. Shrinking samples each target pixel's area
    /// and keeps the dominant kind of pixel (averaging opaque colours),
    /// so thin outlines survive; growing repeats pixels.
    pub fn scaled(&self, w: i32, h: i32) -> Image {
        if (w, h) == (self.w, self.h) || self.w <= 0 || self.h <= 0 {
            return self.clone();
        }
        let mut out = Image::new(w, h);
        for ty in 0..h {
            let y0 = ty * self.h / h;
            let y1 = ((ty + 1) * self.h / h).max(y0 + 1);
            for tx in 0..w {
                let x0 = tx * self.w / w;
                let x1 = ((tx + 1) * self.w / w).max(x0 + 1);
                let (mut n, mut opaque, mut inv, mut r, mut g, mut b) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
                for y in y0..y1 {
                    for x in x0..x1 {
                        n += 1;
                        let v = self.at(x, y);
                        if v & OPAQUE == OPAQUE {
                            opaque += 1;
                            r += (v >> 16) & 0xff;
                            g += (v >> 8) & 0xff;
                            b += v & 0xff;
                        } else if v == INVERT {
                            inv += 1;
                        }
                    }
                }
                let v = if opaque * 2 >= n || (opaque > 0 && opaque >= inv && opaque * 3 >= n) {
                    OPAQUE | (r / opaque) << 16 | (g / opaque) << 8 | b / opaque
                } else if inv > 0 && inv * 2 >= n {
                    INVERT
                } else {
                    TRANSPARENT
                };
                out.set(tx, ty, v);
            }
        }
        out
    }

    /// Nothing is drawn by this image.
    pub fn is_blank(&self) -> bool {
        self.px.iter().all(|&p| p == TRANSPARENT)
    }

    /// The pixels for a hardware cursor (0xAARRGGBB, top row first):
    /// inverted pixels become black.
    pub fn argb(&self) -> Vec<u32> {
        self.px.iter().map(|&p| if p & OPAQUE == OPAQUE { p } else if p == INVERT { 0xff00_0000 } else { 0 }).collect()
    }

    /// The AND/XOR mask bitmap (1 bpp, twice the height: AND mask in the
    /// upper half) and, for colour images, the colour bitmap, as the
    /// pointer formats and POINTERINFO have them.
    pub fn to_bitmaps(&self, colour: bool) -> (Bitmap, Option<Bitmap>) {
        let (w, h) = (self.w, self.h);
        let mut mask = Bitmap::new(w, 2 * h, 1, 1);
        let mut col = colour.then(|| Bitmap::new(w, h, 1, 24));
        for y in 0..h {
            for x in 0..w {
                let v = self.at(x, y);
                let (and, xor, c) = match v {
                    TRANSPARENT => (true, false, 0),
                    INVERT => (true, true, 0),
                    v => {
                        let rgb = v & 0xff_ffff;
                        // Monochrome: light colours are white.
                        let light = ((rgb >> 16) & 0xff) + ((rgb >> 8) & 0xff) + (rgb & 0xff) >= 384;
                        (false, !colour && light, rgb)
                    }
                };
                // Bitmap pixels are top row first; y counts from the bottom.
                let set = |bm: &mut Bitmap, yy: i32, on: bool| {
                    let i = ((bm.height - 1 - yy) * bm.width + x) as usize;
                    bm.pixels[i] = if on { 0xffffff } else { 0 };
                };
                set(&mut mask, y + h, and);
                set(&mut mask, y, xor);
                if let Some(cb) = col.as_mut() {
                    let i = ((h - 1 - y) * w + x) as usize;
                    cb.pixels[i] = c;
                }
            }
        }
        (mask, col)
    }

    /// An image from a mask bitmap (1 bpp, twice the height, AND mask in
    /// the upper half) and an optional colour bitmap.
    pub fn from_bitmaps(mask: &Bitmap, colour: Option<&Bitmap>) -> Image {
        let w = mask.width;
        let h = mask.height / 2;
        let mut img = Image::new(w, h);
        let bit = |bm: &Bitmap, x: i32, y: i32| -> bool {
            let i = ((bm.height - 1 - y) * bm.width + x) as usize;
            bm.pixels.get(i).is_some_and(|&p| p & 0xff_ffff != 0)
        };
        for y in 0..h {
            for x in 0..w {
                let and = bit(mask, x, y + h);
                let xor = bit(mask, x, y);
                let v = match (and, xor, colour) {
                    (true, false, _) => TRANSPARENT,
                    (true, true, _) => INVERT,
                    (false, _, Some(c)) => {
                        let cy = y * c.height / h.max(1);
                        let cx = x * c.width / w.max(1);
                        let i = ((c.height - 1 - cy) * c.width + cx) as usize;
                        OPAQUE | c.pixels.get(i).copied().unwrap_or(0) & 0xff_ffff
                    }
                    (false, x, None) => OPAQUE | if x { 0xffffff } else { 0 },
                };
                img.set(x, y, v);
            }
        }
        img
    }
}

/// One image of a pointer/icon file.
#[derive(Clone, Debug)]
pub struct Entry {
    /// 'IC', 'PT', 'CI' or 'CP' as a little-endian u16.
    pub kind: u16,
    /// Hot spot, from the bottom-left corner.
    pub hot: Point,
    /// Device resolution the image was made for (0 x 0: any).
    pub display: (u16, u16),
    /// Bits per pixel of the colour bitmap (1 for monochrome).
    pub bits: u16,
    pub image: Image,
}

pub const KIND_IC: u16 = 0x4349; // "IC"
pub const KIND_PT: u16 = 0x5450; // "PT"
pub const KIND_CI: u16 = 0x4943; // "CI"
pub const KIND_CP: u16 = 0x5043; // "CP"

impl Entry {
    /// A pointer rather than an icon.
    pub fn is_pointer(&self) -> bool {
        self.kind == KIND_PT || self.kind == KIND_CP
    }
}

fn u16le(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// A BITMAPFILEHEADER(2) at `o`: kind, hot spot, info, offset of bits,
/// and the offset after its header and colour table.
fn file_header(b: &[u8], o: usize) -> Option<(u16, Point, Info, usize, usize)> {
    let kind = u16le(b, o)?;
    if !matches!(kind, KIND_IC | KIND_PT | KIND_CI | KIND_CP | 0x4d42) {
        return None;
    }
    let hx = u16le(b, o + 6)? as i16 as i32;
    let hy = u16le(b, o + 8)? as i16 as i32;
    let off = u32le(b, o + 10)? as usize;
    let info = Info::parse(b.get(o + 14..)?, true)?;
    let next = o + 14 + info.header_len().min(info.cbfix as usize + if info.bitcount <= 8 { info.colors.len() * if info.cbfix == 12 { 3 } else { 4 } } else { 0 });
    Some((kind, Point::new(hx, hy), info, off, next))
}

/// Decode `info.height` scans of an uncompressed or RLE bitmap.
fn decode(b: &[u8], info: &Info, off: usize) -> Option<Bitmap> {
    if info.width <= 0 || info.height <= 0 || info.width > 1024 || info.height > 2048 {
        return None;
    }
    let mut bm = Bitmap::new(info.width, info.height, 1, info.bitcount);
    let data = b.get(off..)?;
    if info.compression == 0 && data.len() < info.stride() * info.height as usize {
        return None;
    }
    bitmap::set_bits(&mut bm, info, 0, info.height, data);
    Some(bm)
}

/// One image at `o` (its BITMAPFILEHEADER).
fn image_at(b: &[u8], o: usize, display: (u16, u16)) -> Option<Entry> {
    let (kind, hot, mut info, off, next) = file_header(b, o)?;
    if info.bitcount != 1 {
        return None;
    }
    // The masks are bits, whatever the colour table says.
    info.colors = vec![0x000000, 0xffffff];
    let mask = decode(b, &info, off)?;
    let colour = if kind == KIND_CI || kind == KIND_CP {
        let (_, _, cinfo, coff, _) = file_header(b, next)?;
        Some((decode(b, &cinfo, coff)?, cinfo.bitcount))
    } else {
        None
    };
    let image = Image::from_bitmaps(&mask, colour.as_ref().map(|c| &c.0));
    Some(Entry { kind, hot, display, bits: colour.map_or(1, |c| c.1), image })
}

/// All images of a pointer/icon file or RT_POINTER resource.
pub fn parse(b: &[u8]) -> Vec<Entry> {
    let mut out = Vec::new();
    if u16le(b, 0) != Some(0x4142) {
        if let Some(e) = image_at(b, 0, (0, 0)) {
            out.push(e);
        }
        return out;
    }
    let mut o = 0usize;
    let mut seen = 0;
    loop {
        if u16le(b, o) != Some(0x4142) || seen > 64 {
            break;
        }
        seen += 1;
        let next = u32le(b, o + 6).unwrap_or(0) as usize;
        let display = (u16le(b, o + 10).unwrap_or(0), u16le(b, o + 12).unwrap_or(0));
        if let Some(e) = image_at(b, o + 14, display) {
            out.push(e);
        }
        if next == 0 || next <= o {
            break;
        }
        o = next;
    }
    out
}

/// The image to use at `size` x `size` pels on a `screen` display: one
/// of that size (made for any display or for this one, the most colours
/// first), else the nearest larger one, else the largest.
pub fn choose(entries: &[Entry], size: i32, screen: (i32, i32)) -> Option<&Entry> {
    let usable: Vec<&Entry> = entries.iter().filter(|e| !e.image.is_blank()).collect();
    let pool = if usable.is_empty() { entries.iter().collect() } else { usable };
    let fits = |e: &Entry| e.display == (0, 0) || (e.display.0 as i32, e.display.1 as i32) == screen;
    let rank = |e: &Entry| {
        let d = e.image.w - size;
        // Exact size first, then larger (scaling down looks better), then
        // smaller; for the display; more colours.
        let dist = if d == 0 { 0 } else if d > 0 { d } else { 1000 - d };
        (dist, !fits(e), u16::MAX - e.bits)
    };
    pool.into_iter().min_by_key(|e| rank(e))
}

/// Draw `img` with its bottom-left corner at device (x, y), clipped to
/// `clip`; halftoned draws every other pixel (a disabled look).
pub fn draw(s: &mut Surface, clip: &[Rect], x: i32, y: i32, img: &Image, halftone: bool) {
    let r = Rect::new(x, y, x + img.w, y + img.h);
    for c in clip {
        let a = r.intersect(c);
        if a.is_empty() {
            continue;
        }
        s.map(a, |px, py, old| {
            if halftone && (px + py) & 1 != 0 {
                return old;
            }
            match img.at(px - x, py - y) {
                TRANSPARENT => old,
                INVERT => !old & 0xff_ffff,
                v => v & 0xff_ffff,
            }
        });
    }
}

/// An opaque colour pixel.
pub fn rgb(c: Rgb) -> u32 {
    OPAQUE | c & 0xff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_1x(kind: &[u8; 2], w: i32, h: i32, and: &[&str], xor: &[&str]) -> Vec<u8> {
        // BITMAPFILEHEADER (14) + BITMAPINFOHEADER (12) + 2 RGB, then bits.
        let stride = ((w + 31) / 32 * 4) as usize;
        let off = 14 + 12 + 6;
        let mut b = Vec::new();
        b.extend_from_slice(kind);
        b.extend_from_slice(&((14 + 12) as u32).to_le_bytes());
        b.extend_from_slice(&(w as u16 / 2).to_le_bytes());
        b.extend_from_slice(&(h as u16 / 2).to_le_bytes());
        b.extend_from_slice(&(off as u32).to_le_bytes());
        b.extend_from_slice(&12u32.to_le_bytes());
        b.extend_from_slice(&(w as u16).to_le_bytes());
        b.extend_from_slice(&(2 * h as u16).to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 0xff, 0xff, 0xff]);
        // Scan lines bottom-up: XOR rows (bottom first), then AND rows.
        for rows in [xor, and] {
            for r in rows.iter().rev() {
                let mut line = vec![0u8; stride];
                for (i, ch) in r.bytes().enumerate() {
                    if ch == b'1' {
                        line[i / 8] |= 0x80 >> (i % 8);
                    }
                }
                b.extend_from_slice(&line);
            }
        }
        b
    }

    #[test]
    fn mono_pointer_masks() {
        // Row 0 (top): black, white, transparent, inverted.
        let f = file_1x(b"PT", 4, 2, &["0011", "1111"], &["0101", "0000"]);
        let e = parse(&f);
        assert_eq!(e.len(), 1);
        let e = &e[0];
        assert!(e.is_pointer());
        assert_eq!(e.hot, Point::new(2, 1));
        let i = &e.image;
        assert_eq!((i.w, i.h), (4, 2));
        assert_eq!(i.at(0, 1), OPAQUE);
        assert_eq!(i.at(1, 1), OPAQUE | 0xffffff);
        assert_eq!(i.at(2, 1), TRANSPARENT);
        assert_eq!(i.at(3, 1), INVERT);
        assert!((0..4).all(|x| i.at(x, 0) == TRANSPARENT));
    }

    #[test]
    fn bitmaps_round_trip() {
        let mut img = Image::new(5, 3);
        img.set(0, 0, rgb(0x123456));
        img.set(1, 2, INVERT);
        img.set(4, 1, rgb(0xffffff));
        let (m, c) = img.to_bitmaps(true);
        assert_eq!((m.width, m.height), (5, 6));
        assert_eq!(Image::from_bitmaps(&m, c.as_ref()), img);
        // Monochrome keeps black and white.
        let mut mono = Image::new(2, 1);
        mono.set(0, 0, rgb(0));
        mono.set(1, 0, rgb(0xffffff));
        let (m, _) = mono.to_bitmaps(false);
        assert_eq!(Image::from_bitmaps(&m, None), mono);
    }

    #[test]
    fn choose_sizes() {
        let mk = |w: i32, bits: u16, display: (u16, u16)| Entry {
            kind: KIND_CI,
            hot: Point::new(0, 0),
            display,
            bits,
            image: {
                let mut i = Image::new(w, w);
                i.set(0, 0, rgb(1));
                i
            },
        };
        let v = vec![mk(16, 1, (0, 0)), mk(32, 1, (0, 0)), mk(32, 4, (0, 0)), mk(40, 4, (1024, 768))];
        let c = choose(&v, 32, (1024, 768)).unwrap();
        assert_eq!((c.image.w, c.bits), (32, 4));
        assert_eq!(choose(&v, 16, (1024, 768)).unwrap().image.w, 16);
        assert_eq!(choose(&v, 20, (1024, 768)).unwrap().image.w, 32);
        assert_eq!(choose(&v, 64, (1024, 768)).unwrap().image.w, 40);
    }

    #[test]
    fn scaling_keeps_outline() {
        let mut i = Image::new(32, 32);
        for k in 0..32 {
            i.set(k, 0, rgb(0));
            i.set(0, k, rgb(0));
        }
        let s = i.scaled(16, 16);
        assert_eq!((s.w, s.h), (16, 16));
        assert!((0..16).all(|k| s.at(k, 0) != TRANSPARENT && s.at(0, k) != TRANSPARENT));
        assert_eq!(s.at(8, 8), TRANSPARENT);
    }

    #[test]
    fn draw_clips_and_inverts() {
        let mut px = vec![0x00ff00u32; 4 * 4];
        let mut s = unsafe { Surface::new(px.as_mut_ptr(), 4, 4, 4, false) };
        let mut img = Image::new(2, 2);
        img.set(0, 0, rgb(0xff0000));
        img.set(1, 0, INVERT);
        img.set(0, 1, TRANSPARENT);
        img.set(1, 1, rgb(0x0000ff));
        draw(&mut s, &[Rect::new(0, 0, 2, 4)], 1, 1, &img, false);
        assert_eq!(s.get(1, 1), Some(0xff0000));
        assert_eq!(s.get(2, 1), Some(0x00ff00), "clipped");
        assert_eq!(s.get(1, 2), Some(0x00ff00), "transparent");
        draw(&mut s, &[Rect::new(0, 0, 4, 4)], 1, 1, &img, false);
        assert_eq!(s.get(2, 1), Some(0xff00ff), "inverted");
        assert_eq!(s.get(2, 2), Some(0x0000ff));
    }

    /// The Toolkit's sample icons, when the media is there.
    #[test]
    fn toolkit_icons() {
        extern crate std;
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../work/media/boot/toolkit/os2tk45/samples/pm");
        let Ok(b) = std::fs::read(alloc::format!("{dir}/controls/sty_main.ico")) else { return };
        let e = parse(&b);
        assert_eq!(e.len(), 5);
        let c = choose(&e, 32, (1024, 768)).unwrap();
        assert_eq!((c.image.w, c.image.h, c.bits), (32, 32, 4));
        // The bottom rows are transparent, the window frame is drawn.
        assert_eq!(c.image.at(5, 0), TRANSPARENT);
        assert_ne!(c.image.at(16, 16), TRANSPARENT);
        let m = choose(&e, 16, (1024, 768)).unwrap();
        assert_eq!(m.image.w, 16);
        let Ok(b) = std::fs::read(alloc::format!("{dir}/dragdrop/dragdrop.ptr")) else { return };
        let e = parse(&b);
        assert!(e.iter().all(|x| x.is_pointer()));
        let c = choose(&e, 32, (1024, 768)).unwrap();
        assert!(!c.image.is_blank());
        for f in ["stdwnd/hello.ico", "controls/sty_ico1.ico", "dialog/dialog.ico", "bmpsamp/jigsaw.ico"] {
            let b = std::fs::read(alloc::format!("{dir}/{f}")).unwrap();
            let e = parse(&b);
            assert!(!e.is_empty(), "{f}");
            assert!(choose(&e, 32, (1024, 768)).is_some_and(|c| !c.image.is_blank()), "{f}");
        }
    }
}
