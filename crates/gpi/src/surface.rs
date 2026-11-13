//! A drawing target: 32-bit pixels, top row first in memory, addressed in
//! device coordinates (bottom row is y = 0). A surface can note what it
//! draws in a [`Damage`] list (the screen does, for the display driver).

use crate::color::Rgb;
use crate::damage::Damage;
use crate::palette::Physical;
use crate::rect::Rect;

#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pixels: *mut u32,
    pub width: i32,
    pub height: i32,
    /// Pixels per row in memory.
    pub stride: i32,
    /// Red in the low byte (UEFI RGBX) instead of 0x00RRGGBB (BGRX).
    pub red_low: bool,
    /// Where drawing is noted (null: nowhere).
    damage: *mut Damage,
    /// A 256-colour display's hardware palette: pels get the nearest of
    /// its colours (null: true colour).
    palette: *mut Physical,
}

unsafe impl Send for Surface {}

impl Surface {
    /// # Safety
    /// `pixels` must point to `stride * height` writable pixels that stay
    /// valid while the surface is used.
    pub unsafe fn new(pixels: *mut u32, width: i32, height: i32, stride: i32, red_low: bool) -> Self {
        Surface { pixels, width, height, stride, red_low, damage: core::ptr::null_mut(), palette: core::ptr::null_mut() }
    }

    /// Note what is drawn from now on in `d` (null: nowhere).
    ///
    /// # Safety
    /// `d` must stay valid while the surface (or a copy) draws, and only
    /// one thread may draw at a time.
    pub unsafe fn set_damage(&mut self, d: *mut Damage) {
        self.damage = d;
    }

    /// Draw in the colours of palette `p` (null: true colour).
    ///
    /// # Safety
    /// As for [`Surface::set_damage`].
    pub unsafe fn set_palette(&mut self, p: *mut Physical) {
        self.palette = p;
    }

    pub fn palette(&self) -> *mut Physical {
        self.palette
    }

    fn note(&self, r: Rect) {
        if let Some(d) = unsafe { self.damage.as_mut() } {
            d.add(r);
        }
    }

    /// The pixel memory.
    pub fn pixels(&self) -> *mut u32 {
        self.pixels
    }

    /// Draw into other memory of the same geometry from now on.
    ///
    /// # Safety
    /// As for [`Surface::new`].
    pub unsafe fn set_pixels(&mut self, pixels: *mut u32) {
        self.pixels = pixels;
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    fn encode(&self, c: Rgb) -> u32 {
        let c = match unsafe { self.palette.as_mut() } {
            Some(p) => p.map(c),
            None => c,
        };
        if self.red_low { (c >> 16) & 0xff | c & 0xff00 | (c & 0xff) << 16 } else { c & 0xff_ffff }
    }

    fn decode(&self, v: u32) -> Rgb {
        if self.red_low { (v >> 16) & 0xff | v & 0xff00 | (v & 0xff) << 16 } else { v & 0xff_ffff }
    }

    fn index(&self, x: i32, y: i32) -> usize {
        ((self.height - 1 - y) * self.stride + x) as usize
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Rgb> {
        self.bounds().contains(crate::Point::new(x, y)).then(|| self.decode(unsafe { *self.pixels.add(self.index(x, y)) }))
    }

    /// Set one pixel (no clipping beyond the surface bounds).
    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if self.bounds().contains(crate::Point::new(x, y)) {
            let v = self.encode(c);
            unsafe { *self.pixels.add(self.index(x, y)) = v };
            if let Some(d) = unsafe { self.damage.as_mut() } {
                d.add_point(x, y);
            }
        }
    }

    /// Fill `r`, clipped to the surface.
    pub fn fill(&mut self, r: Rect, c: Rgb) {
        let r = r.intersect(&self.bounds());
        if r.is_empty() {
            return;
        }
        self.note(r);
        let v = self.encode(c);
        for y in r.bottom..r.top {
            let row = self.index(r.left, y);
            for i in 0..r.width() as usize {
                unsafe { *self.pixels.add(row + i) = v };
            }
        }
    }

    /// Copy the pixels `dx`, `dy` away from `dst` (to the left of and
    /// below it for positive offsets) into `dst`, a row at a time; source
    /// and destination may overlap. Both are clipped to the surface.
    /// The raw pixel values move (no colour conversion).
    pub fn copy_within(&mut self, dst: Rect, dx: i32, dy: i32) {
        let d = dst.intersect(&self.bounds()).intersect(&self.bounds().offset(dx, dy));
        if d.is_empty() {
            return;
        }
        self.note(d);
        let w = d.width() as usize;
        let row = |y: i32| {
            let (s, t) = (self.index(d.left - dx, y - dy), self.index(d.left, y));
            unsafe { core::ptr::copy(self.pixels.add(s), self.pixels.add(t), w) };
        };
        // A row written must not be a row still to be read: going up,
        // start at the top.
        if dy > 0 {
            (d.bottom..d.top).rev().for_each(row);
        } else {
            (d.bottom..d.top).for_each(row);
        }
    }

    /// The raw pixels of `r` (clipped), rows from the bottom, appended
    /// to `out`; the rectangle read.
    pub fn read_raw(&self, r: Rect, out: &mut alloc::vec::Vec<u32>) -> Rect {
        let r = r.intersect(&self.bounds());
        for y in r.bottom..r.top {
            let i = self.index(r.left, y);
            out.extend_from_slice(unsafe { core::slice::from_raw_parts(self.pixels.add(i), r.width() as usize) });
        }
        r
    }

    /// Write raw pixels (as [`Surface::read_raw`] gives them) to `r`,
    /// which must lie inside the surface.
    pub fn write_raw(&mut self, r: Rect, px: &[u32]) {
        let r = r.intersect(&self.bounds());
        if r.is_empty() || px.len() < (r.width() * r.height()) as usize {
            return;
        }
        self.note(r);
        let w = r.width() as usize;
        for (k, y) in (r.bottom..r.top).enumerate() {
            let i = self.index(r.left, y);
            unsafe { core::ptr::copy_nonoverlapping(px.as_ptr().add(k * w), self.pixels.add(i), w) };
        }
    }

    /// Apply `f` to every pixel of `r` (clipped), for raster operations.
    pub fn map(&mut self, r: Rect, mut f: impl FnMut(i32, i32, Rgb) -> Rgb) {
        let r = r.intersect(&self.bounds());
        if !r.is_empty() {
            self.note(r);
        }
        for y in r.bottom..r.top {
            for x in r.left..r.right {
                let i = self.index(x, y);
                let old = self.decode(unsafe { *self.pixels.add(i) });
                let new = self.encode(f(x, y, old));
                unsafe { *self.pixels.add(i) = new };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A 6x5 surface whose pixel at (x, y) is 10 * y + x.
    fn numbered(buf: &mut Vec<u32>) -> Surface {
        *buf = (0..30).map(|i| (10 * (4 - i / 6) + i % 6) as u32).collect();
        unsafe { Surface::new(buf.as_mut_ptr(), 6, 5, 6, false) }
    }

    fn at(s: &Surface, x: i32, y: i32) -> u32 {
        s.get(x, y).unwrap()
    }

    #[test]
    fn copy_within_overlapping() {
        for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0), (2, 2), (-1, -2)] {
            let mut buf = Vec::new();
            let mut s = numbered(&mut buf);
            let dst = Rect::new(0, 0, 6, 5);
            s.copy_within(dst, dx, dy);
            let d = dst.intersect(&dst.offset(dx, dy));
            for y in 0..5 {
                for x in 0..6 {
                    let want = if d.contains(crate::Point::new(x, y)) { (10 * (y - dy) + x - dx) as u32 } else { (10 * y + x) as u32 };
                    assert_eq!(at(&s, x, y), want, "shift ({dx}, {dy}) at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn raw_round_trip() {
        let mut buf = Vec::new();
        let mut s = numbered(&mut buf);
        let mut px = Vec::new();
        let got = s.read_raw(Rect::new(1, 1, 3, 4), &mut px);
        assert_eq!(got, Rect::new(1, 1, 3, 4));
        assert_eq!(px, [11, 12, 21, 22, 31, 32]);
        s.write_raw(Rect::new(3, 0, 5, 3), &px);
        assert_eq!([at(&s, 3, 0), at(&s, 4, 0), at(&s, 3, 2), at(&s, 4, 2)], [11, 12, 31, 32]);
    }
}
