//! The locales' icon: a globe (rs2's own picture, pmwin::shellicons),
//! made into a PM icon as OS/2's program loads its globe from GLOCMRI.

use pmkit::*;

/// The globe as an HPOINTER (0 if it cannot be made).
pub fn globe() -> u32 {
    let Some((big, mini)) = pmwin::shellicons::icon(pmwin::shellicons::INTERNET) else { return 0 };
    let (m1, c1) = bitmaps(&big);
    let (m2, c2) = bitmaps(&mini);
    let pi = PointerInfo { pointer: 0, x_hotspot: 16, y_hotspot: 16, hbm_pointer: m1, hbm_color: c1, hbm_mini_pointer: m2, hbm_mini_color: c2 };
    let h = unsafe { WinCreatePointerIndirect(HWND_DESKTOP, &pi) };
    unsafe {
        for b in [m1, c1, m2, c2] {
            GpiDeleteBitmap(b);
        }
    }
    h
}

/// (mask bitmap, colour bitmap) of a picture.
fn bitmaps(img: &::gpi::icon::Image) -> (u32, u32) {
    let (mask, colour) = img.to_bitmaps(true);
    (bitmap(&mask, 1), colour.map_or(0, |c| bitmap(&c, 24)))
}

/// A GPI bitmap from a picture (top row first) at 1 or 24 bits a pel.
fn bitmap(b: &::gpi::bitmap::Bitmap, bpp: u16) -> u32 {
    let (w, h) = (b.width, b.height);
    let row = (w as usize * bpp as usize).div_ceil(32) * 4;
    let mut bits = alloc::vec![0u8; row * h as usize];
    // Scan lines from the bottom.
    for y in 0..h {
        let src = &b.pixels[((h - 1 - y) * w) as usize..((h - y) * w) as usize];
        let dst = &mut bits[y as usize * row..(y as usize + 1) * row];
        for (x, &p) in src.iter().enumerate() {
            if bpp == 1 {
                if p & 0xff_ffff != 0 {
                    dst[x / 8] |= 0x80 >> (x % 8);
                }
            } else {
                dst[x * 3] = p as u8;
                dst[x * 3 + 1] = (p >> 8) as u8;
                dst[x * 3 + 2] = (p >> 16) as u8;
            }
        }
    }
    let mut bih = BitmapInfoHeader2::new(w as u32, h as u32, bpp);
    bih.cb = 16;
    // BITMAPINFO2 with the colour table (black, white) for 1 bpp.
    let mut info = [0u8; 24];
    info[..16].copy_from_slice(unsafe { core::slice::from_raw_parts(&bih as *const BitmapInfoHeader2 as *const u8, 16) });
    info[16..24].copy_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0]);
    unsafe { GpiCreateBitmap(0, &bih, CBM_INIT, bits.as_ptr(), info.as_ptr()) }
}
