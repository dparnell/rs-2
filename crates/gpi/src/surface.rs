//! A drawing target: 32-bit pixels, top row first in memory, addressed in
//! device coordinates (bottom row is y = 0).

use crate::color::Rgb;
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
}

unsafe impl Send for Surface {}

impl Surface {
    /// # Safety
    /// `pixels` must point to `stride * height` writable pixels that stay
    /// valid while the surface is used.
    pub unsafe fn new(pixels: *mut u32, width: i32, height: i32, stride: i32, red_low: bool) -> Self {
        Surface { pixels, width, height, stride, red_low }
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
        }
    }

    /// Fill `r`, clipped to the surface.
    pub fn fill(&mut self, r: Rect, c: Rgb) {
        let r = r.intersect(&self.bounds());
        if r.is_empty() {
            return;
        }
        let v = self.encode(c);
        for y in r.bottom..r.top {
            let row = self.index(r.left, y);
            for i in 0..r.width() as usize {
                unsafe { *self.pixels.add(row + i) = v };
            }
        }
    }

    /// Apply `f` to every pixel of `r` (clipped), for raster operations.
    pub fn map(&mut self, r: Rect, mut f: impl FnMut(i32, i32, Rgb) -> Rgb) {
        let r = r.intersect(&self.bounds());
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
