//! GDI's bitmaps, memory DCs, bit-block transfers, DIBs and palettes.
//!
//! An HBITMAP is a PM bitmap (GpiCreateBitmap); a memory DC
//! (CreateCompatibleDC) is a PM memory presentation space on a memory
//! device context (DevOpenDC OD_MEMORY), so every GDI call draws into the
//! selected bitmap as it draws into a window. Like Windows', a new memory
//! DC has a 1x1 monochrome bitmap. Colour bitmaps are 24 bits per pixel
//! (the display's format, GetDeviceCaps BITSPIXEL); monochrome ones 1.
//!
//! BitBlt and StretchBlt are GpiBitBlt: Windows' ternary raster
//! operations are PM's (the ROP's high word is the ROP3 index). A
//! monochrome source's 1 bits take the target DC's background colour and
//! its 0 bits the text colour; a colour source going to a monochrome
//! bitmap gives 1 where it has the source DC's background colour.
//!
//! DIBs are PM's bitmap format already: Windows' BITMAPINFOHEADER is the
//! first 40 bytes of PM's BITMAPINFOHEADER2 (and BITMAPCOREHEADER is
//! PM's 1.x header), RGBQUAD is RGB2, rows go bottom-up in both, and
//! BI_RLE8/BI_RLE4 are BCA_RLE8/BCA_RLE4. A DDB's bits (CreateBitmap,
//! SetBitmapBits) go top-down with rows padded to words, and are turned
//! around here.

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::gdi::{self, Obj};
use crate::pm::*;
use crate::Global;

const PRIM_AREA: i32 = 4;
const PRIM_IMAGE: i32 = 5;
const CBM_INIT: u32 = 4;
const DIB_PAL_COLORS: u16 = 1;

fn w16(p: *const u8, o: usize) -> u16 {
    unsafe { (p.add(o) as *const u16).read_unaligned() }
}

fn l32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}

/// A BITMAPINFOHEADER (40 bytes) and colour table for PM.
fn info(w: i32, h: i32, bpp: u16, colors: &[u32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(40 + colors.len() * 4);
    v.extend_from_slice(&40u32.to_le_bytes());
    v.extend_from_slice(&(w as u32).to_le_bytes());
    v.extend_from_slice(&(h as u32).to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&bpp.to_le_bytes());
    v.extend_from_slice(&[0; 24]);
    for c in colors {
        // COLORREF 0x00BBGGRR -> RGBQUAD blue, green, red, 0.
        v.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, *c as u8, 0]);
    }
    v
}

/// The colour table of a DDB format: black and white, or the default
/// palette's colours.
fn ddb_colors(bpp: u16) -> Vec<u32> {
    match bpp {
        1 => alloc::vec![0, 0xffffff],
        4 => (0..16).map(|i| gdi::DEFAULT_PALETTE[if i < 8 { i } else { i + 4 }]).collect(),
        8 => (0..256).map(|i| if i < 10 { gdi::DEFAULT_PALETTE[i] } else if i >= 246 { gdi::DEFAULT_PALETTE[i - 236] } else { cube(i - 10) }).collect(),
        _ => Vec::new(),
    }
}

/// A 6x6x6 colour cube for the 8-bit DDB entries between the static
/// colours.
fn cube(i: usize) -> u32 {
    let i = (i % 216) as u32;
    let (r, g, b) = (i / 36, (i / 6) % 6, i % 6);
    (b * 51) << 16 | (g * 51) << 8 | r * 51
}

/// A new PM bitmap (bits per pixel 1 or 24).
fn pm_bitmap(w: i32, h: i32, bpp: u16) -> u32 {
    let hdr = info(w.max(1), h.max(1), bpp, &[]);
    unsafe { GpiCreateBitmap(0, hdr.as_ptr(), 0, core::ptr::null(), core::ptr::null()) }
}

/// A bitmap object for a new PM bitmap.
pub fn new_bitmap(w: i32, h: i32, bpp: u16) -> u16 {
    let bpp = if bpp == 1 { 1 } else { 24 };
    let hbm = pm_bitmap(w, h, bpp);
    if hbm == 0 {
        return 0;
    }
    gdi::new_obj(Obj::Bitmap { hbm, w: w.max(1), h: h.max(1), bpp })
}

fn bitmap_of(h: u16) -> Option<(u32, i32, i32, u16)> {
    match gdi::obj(h) {
        Some(Obj::Bitmap { hbm, w, h, bpp }) => Some((hbm, w, h, bpp)),
        _ => None,
    }
}

/// A scratch memory PS for setting and querying the bits of bitmaps no
/// DC has selected.
static SCRATCH: Global<u32> = Global::new(0);

fn memory_ps() -> (u32, u32) {
    let hab = crate::kernel::TASK.get().hab;
    let dc = unsafe { DevOpenDC(hab, 8, b"*\0".as_ptr(), 0, core::ptr::null(), 0) };
    if dc == 0 {
        return (0, 0);
    }
    let size = [0i32; 2];
    // PU_PELS | GPIT_MICRO | GPIA_ASSOC
    let hps = unsafe { GpiCreatePS(hab, dc, size.as_ptr(), 0x0008 | 0x1000 | 0x4000) };
    if hps == 0 {
        unsafe { DevCloseDC(dc) };
        return (0, 0);
    }
    unsafe { GpiCreateLogColorTable(hps, 0, LCOLF_RGB, 0, 0, core::ptr::null()) };
    (dc, hps)
}

/// Run `f` on a PS that has the bitmap `h` selected: the DC's that has
/// it, or the scratch PS.
fn with_ps<R>(h: u16, f: impl FnOnce(u32, i32) -> R) -> Option<R> {
    let (hbm, _, bh, _) = bitmap_of(h)?;
    if let Some(hps) = gdi::dc_with_bitmap(h) {
        return Some(f(hps, bh));
    }
    let s = SCRATCH.get();
    if *s == 0 {
        *s = memory_ps().1;
    }
    let hps = *s;
    if hps == 0 || unsafe { GpiSetBitmap(hps, hbm) } == u32::MAX {
        return None;
    }
    let r = f(hps, bh);
    unsafe { GpiSetBitmap(hps, 0) };
    Some(r)
}

/// CreateCompatibleDC(hdc): a memory DC with a 1x1 monochrome bitmap.
pub fn create_memory_dc() -> u16 {
    let (dc, hps) = memory_ps();
    if hps == 0 {
        return 0;
    }
    let def = pm_bitmap(1, 1, 1);
    unsafe { GpiSetBitmap(hps, def) };
    let h = gdi::new_dc(hps, 0, 1, gdi::DcKind::Memory);
    if let Some(d) = gdi::dc(h) {
        d.mem = dc;
        d.def_bm = def;
    }
    h
}

/// DeleteDC of a memory DC: its PS and device context, and the default
/// bitmap (the selected one is the program's).
pub fn delete_memory(d: &gdi::Dc) {
    unsafe {
        GpiSetBitmap(d.hps, 0);
        GpiDeleteBitmap(d.def_bm);
        GpiDestroyPS(d.hps);
        DevCloseDC(d.mem);
    }
}

/// SelectObject(hdc, hbitmap) on a memory DC: the previous bitmap.
pub fn select(hdc: u16, h: u16) -> u16 {
    if h == default_bitmap() {
        let old = select_default(hdc);
        return if old == 0 { h } else { old };
    }
    let Some((hbm, _, bh, _)) = bitmap_of(h) else { return 0 };
    let Some(d) = gdi::dc(hdc) else { return 0 };
    if d.kind != gdi::DcKind::Memory {
        return 0;
    }
    // Another DC may hold it (Windows refuses that too).
    if gdi::dc_with_bitmap(h).is_some_and(|hps| hps != d.hps) {
        return 0;
    }
    if unsafe { GpiSetBitmap(d.hps, hbm) } == u32::MAX {
        return 0;
    }
    let old = core::mem::replace(&mut d.bitmap, h);
    d.height = bh;
    if old == 0 { default_bitmap() } else { old }
}

/// The handle SelectObject returns for a memory DC's default bitmap
/// (selecting it back puts the default back).
fn default_bitmap() -> u16 {
    static DEF: Global<u16> = Global::new(0);
    let d = DEF.get();
    if *d == 0 {
        *d = gdi::new_obj(Obj::Bitmap { hbm: 0, w: 1, h: 1, bpp: 1 });
    }
    *d
}

/// Select a memory DC's own default bitmap again (so that a bitmap can be
/// deleted).
pub fn deselect(hdc: u16) -> u16 {
    select_default(hdc)
}

/// Select a memory DC's own default bitmap again.
fn select_default(hdc: u16) -> u16 {
    let Some(d) = gdi::dc(hdc) else { return 0 };
    unsafe { GpiSetBitmap(d.hps, d.def_bm) };
    d.height = 1;
    core::mem::replace(&mut d.bitmap, 0)
}

/// A DDB's bits (rows top-down, padded to words) into the bitmap.
fn set_ddb_bits(h: u16, bits: *const u8, len: usize) -> u32 {
    let Some((_, w, bh, bpp)) = bitmap_of(h) else { return 0 };
    if bits.is_null() {
        return 0;
    }
    let src_stride = ((w as usize * bpp as usize).div_ceil(16)) * 2;
    let dst_stride = ((w as usize * bpp as usize).div_ceil(32)) * 4;
    let rows = (len / src_stride.max(1)).min(bh as usize);
    let mut buf = alloc::vec![0u8; dst_stride * bh as usize];
    for y in 0..rows {
        let s = unsafe { core::slice::from_raw_parts(bits.add(y * src_stride), src_stride) };
        let at = (bh as usize - 1 - y) * dst_stride;
        let n = src_stride.min(dst_stride);
        buf[at..at + n].copy_from_slice(&s[..n]);
    }
    let bmi = info(w, bh, bpp, &ddb_colors(bpp));
    with_ps(h, |hps, _| unsafe { GpiSetBitmapBits(hps, 0, bh, buf.as_ptr(), bmi.as_ptr()) });
    (rows * src_stride) as u32
}

/// GetBitmapBits: the DDB's bits, top-down.
fn get_ddb_bits(h: u16, bits: *mut u8, len: usize) -> u32 {
    let Some((_, w, bh, bpp)) = bitmap_of(h) else { return 0 };
    let dst_stride = ((w as usize * bpp as usize).div_ceil(16)) * 2;
    let src_stride = ((w as usize * bpp as usize).div_ceil(32)) * 4;
    let mut buf = alloc::vec![0u8; src_stride * bh as usize];
    let mut bmi = info(w, bh, bpp, &ddb_colors(bpp));
    with_ps(h, |hps, _| unsafe { GpiQueryBitmapBits(hps, 0, bh, buf.as_mut_ptr(), bmi.as_mut_ptr()) });
    if bits.is_null() {
        return (dst_stride * bh as usize) as u32;
    }
    let rows = (len / dst_stride.max(1)).min(bh as usize);
    for y in 0..rows {
        let at = (bh as usize - 1 - y) * src_stride;
        let n = src_stride.min(dst_stride);
        unsafe { core::ptr::copy_nonoverlapping(buf[at..].as_ptr(), bits.add(y * dst_stride), n) };
    }
    (rows * dst_stride) as u32
}

/// The size of a DIB header and colour table (DIB_PAL_COLORS: WORD
/// indexes) and its colour count.
fn dib_header(bmi: *const u8, usage: u16) -> (usize, usize) {
    let cb = l32(bmi, 0) as usize;
    let bpp = if cb == 12 { w16(bmi, 10) } else { w16(bmi, 14) };
    let used = if cb >= 36 { l32(bmi, 32) as usize } else { 0 };
    let n = if bpp > 8 { 0 } else if used != 0 { used.min(256) } else { 1 << bpp };
    let es = if usage == DIB_PAL_COLORS { 2 } else if cb == 12 { 3 } else { 4 };
    (cb + n * es, n)
}

/// A DIB header for PM: DIB_PAL_COLORS indexes become the colours of
/// the DC's palette.
fn dib_info(hdc: u16, bmi: *const u8, usage: u16) -> Vec<u8> {
    let (len, n) = dib_header(bmi, usage);
    let cb = l32(bmi, 0) as usize;
    if usage != DIB_PAL_COLORS {
        return unsafe { core::slice::from_raw_parts(bmi, len) }.to_vec();
    }
    let pal = gdi::palette_colors(hdc);
    let mut v = unsafe { core::slice::from_raw_parts(bmi, cb) }.to_vec();
    for k in 0..n {
        let i = w16(bmi, cb + 2 * k) as usize;
        let c = pal.get(i).copied().unwrap_or(0);
        v.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        if cb != 12 {
            v.push(0);
        }
    }
    v
}

/// The bytes of a DIB's bits (`scans` rows).
fn dib_len(bmi: &[u8], scans: i32) -> usize {
    let cb = l32(bmi.as_ptr(), 0) as usize;
    let (w, bpp, comp, image) = if cb == 12 {
        (w16(bmi.as_ptr(), 4) as usize, w16(bmi.as_ptr(), 10) as usize, 0, 0)
    } else {
        (l32(bmi.as_ptr(), 4) as usize, w16(bmi.as_ptr(), 14) as usize, l32(bmi.as_ptr(), 16), l32(bmi.as_ptr(), 20) as usize)
    };
    if comp != 0 && image != 0 {
        return image;
    }
    (w * bpp).div_ceil(32) * 4 * scans.max(0) as usize
}

fn dib_size(bmi: *const u8) -> (i32, i32, u16) {
    if l32(bmi, 0) == 12 {
        (w16(bmi, 4) as i32, w16(bmi, 6) as i32, w16(bmi, 10))
    } else {
        (l32(bmi, 4) as i32, l32(bmi, 8) as i32, w16(bmi, 14))
    }
}

/// SetDIBits into a bitmap.
fn set_dib_bits(hdc: u16, h: u16, start: i32, scans: i32, bits: *const u8, bmi: *const u8, usage: u16) -> i32 {
    if bits.is_null() || bmi.is_null() {
        return 0;
    }
    let hdr = dib_info(hdc, bmi, usage);
    with_ps(h, |hps, _| unsafe { GpiSetBitmapBits(hps, start, scans, bits, hdr.as_ptr()) }).unwrap_or(0).max(0)
}

/// GetDIBits: the bits in the format the header asks for (no bits: the
/// header is filled in).
fn get_dib_bits(h: u16, start: i32, scans: i32, bits: *mut u8, bmi: *mut u8) -> i32 {
    let Some((_, w, bh, bpp)) = bitmap_of(h) else { return 0 };
    if bmi.is_null() {
        return 0;
    }
    let cb = l32(bmi, 0) as usize;
    if cb >= 16 {
        unsafe {
            (bmi.add(4) as *mut u32).write_unaligned(w as u32);
            (bmi.add(8) as *mut u32).write_unaligned(bh as u32);
            (bmi.add(12) as *mut u16).write_unaligned(1);
            if w16(bmi, 14) == 0 {
                (bmi.add(14) as *mut u16).write_unaligned(bpp);
            }
            if cb >= 24 {
                let stride = (w as usize * w16(bmi, 14) as usize).div_ceil(32) * 4;
                (bmi.add(16) as *mut u32).write_unaligned(0);
                (bmi.add(20) as *mut u32).write_unaligned((stride * bh as usize) as u32);
            }
        }
    }
    if bits.is_null() {
        return bh;
    }
    with_ps(h, |hps, _| unsafe { GpiQueryBitmapBits(hps, start, scans, bits, bmi) }).unwrap_or(0).max(0)
}

/// Set the target's colours for a blit: the pattern is the brush, a
/// monochrome source's 1s the background colour and 0s the text colour.
fn blit_colors(hdc: u16) {
    let Some(d) = gdi::dc(hdc) else { return };
    let brush = gdi::brush_color(gdi::brush_of(hdc)).unwrap_or(0xffffff);
    let mut area = [0u8; 24];
    area[0..4].copy_from_slice(&gdi::rgb(brush).to_le_bytes());
    let mut image = [0u8; 12];
    image[0..4].copy_from_slice(&gdi::rgb(gdi::bk_color(hdc)).to_le_bytes());
    image[4..8].copy_from_slice(&gdi::rgb(gdi::text_color(hdc)).to_le_bytes());
    unsafe {
        GpiSetAttrs(d.hps, PRIM_AREA, 1, 0, area.as_ptr());
        GpiSetAttrs(d.hps, PRIM_IMAGE, 3, 0, image.as_ptr());
    }
}

/// Whether the DC draws into a monochrome bitmap.
fn is_mono(hdc: u16) -> bool {
    gdi::dc(hdc).is_some_and(|d| d.kind == gdi::DcKind::Memory && (d.bitmap == 0 || bitmap_of(d.bitmap).is_some_and(|b| b.3 == 1)))
}

/// BitBlt.
#[allow(clippy::too_many_arguments)]
pub fn blit(dst: u16, x: i32, y: i32, w: i32, h: i32, src: u16, sx: i32, sy: i32, rop: u32) -> u32 {
    blt(dst, x, y, w, h, src, sx, sy, w, h, rop)
}

/// A copy of a bitmap.
pub fn copy_bitmap(h: u16) -> u16 {
    let Some((_, w, bh, bpp)) = bitmap_of(h) else { return 0 };
    let n = new_bitmap(w, bh, bpp);
    let (a, b) = (create_memory_dc(), create_memory_dc());
    select(a, h);
    select(b, n);
    blt(b, 0, 0, w, bh, a, 0, 0, w, bh, 0x00cc_0020);
    select_default(a);
    select_default(b);
    gdi::release_dc(a);
    gdi::release_dc(b);
    n
}

/// CreateBitmap with DDB bits, for icons.
pub fn create_ddb(w: i32, h: i32, bpp: u16, bits: *const u8) -> u16 {
    create_bitmap(w, h, bpp, bits) as u16
}

/// StretchBlt (and BitBlt: the source the target's size).
#[allow(clippy::too_many_arguments)]
fn blt(dst: u16, x: i32, y: i32, w: i32, h: i32, src: u16, sx: i32, sy: i32, sw: i32, sh: i32, rop: u32) -> u32 {
    let Some(d) = gdi::dc(dst) else { return 0 };
    let (dps, dh) = (d.hps, d.height);
    let mut rop3 = (rop >> 16) as u8;
    let r = gdi::dev_rect(d, [x, y, x + w, y + h]);
    let (x, y, w, h) = (r[0], r[1], r[2] - r[0], r[3] - r[1]);
    let mut pts = [
        PointL { x, y: dh - y - h },
        PointL { x: x + w, y: dh - y },
        PointL::default(),
        PointL::default(),
    ];
    let uses_src = (rop3 >> 2) & 0x33 != rop3 & 0x33;
    blit_colors(dst);
    let (sps, n) = if uses_src {
        let Some(s) = gdi::dc(src) else { return 0 };
        let r = gdi::dev_rect(s, [sx, sy, sx + sw, sy + sh]);
        let (sx, sy, sw, sh) = (r[0], r[1], r[2] - r[0], r[3] - r[1]);
        pts[2] = PointL { x: sx, y: s.height - sy - sh };
        pts[3] = PointL { x: sx + sw, y: s.height - sy };
        let sps = s.hps;
        // Colour to monochrome: 1 where the source has its background
        // colour (PM gives 0 there, so the source is inverted in the ROP).
        if is_mono(dst) && !is_mono(src) {
            let mut image = [0u8; 12];
            image[4..8].copy_from_slice(&gdi::rgb(gdi::bk_color(src)).to_le_bytes());
            unsafe { GpiSetAttrs(sps, PRIM_IMAGE, 2, 0, image.as_ptr()) };
            rop3 = (0..8).fold(0, |r, i| r | ((rop3 >> (i ^ 2)) & 1) << i);
        }
        (sps, if (sw, sh) == (w, h) { 3 } else { 4 })
    } else {
        (0, 2)
    };
    // BBO_IGNORE: stretching drops pels.
    (unsafe { GpiBitBlt(dps, sps, n, pts.as_ptr(), rop3 as i32, 2) } == 1) as u32
}

/// Draw a DIB on a DC (SetDIBitsToDevice, StretchDIBits).
#[allow(clippy::too_many_arguments)]
fn draw_dib(hdc: u16, x: i32, y: i32, w: i32, h: i32, sx: i32, sy: i32, sw: i32, sh: i32, bits: *const u8, bmi: *const u8, usage: u16, rop: u32) -> i32 {
    if bits.is_null() || bmi.is_null() {
        return 0;
    }
    let Some(d) = gdi::dc(hdc) else { return 0 };
    let dh = d.height;
    let hps = d.hps;
    let r = gdi::dev_rect(d, [x, y, x + w, y + h]);
    let (x, y, w, h) = (r[0], r[1], r[2] - r[0], r[3] - r[1]);
    let hdr = dib_info(hdc, bmi, usage);
    let (_, dib_h, _) = dib_size(bmi);
    blit_colors(hdc);
    // Target corners inclusive (world = device here); the source in the
    // DIB's own (bottom-up) pels.
    let pts = [
        PointL { x, y: dh - y - h },
        PointL { x: x + w - 1, y: dh - y - 1 },
        PointL { x: sx, y: dib_h - sy - sh },
        PointL { x: sx + sw, y: dib_h - sy },
    ];
    unsafe { GpiDrawBits(hps, bits, hdr.as_ptr(), 4, pts.as_ptr(), ((rop >> 16) & 0xff) as i32, 2) };
    sh
}

/// A bitmap from a Windows RT_BITMAP resource (a DIB: header, colours,
/// bits): monochrome stays monochrome.
pub fn from_dib(data: &[u8]) -> u16 {
    if data.len() < 16 {
        return 0;
    }
    let p = data.as_ptr();
    let (w, h, bpp) = dib_size(p);
    let (len, _) = dib_header(p, 0);
    if w <= 0 || h <= 0 || len > data.len() {
        return 0;
    }
    let b = new_bitmap(w, h, if bpp == 1 { 1 } else { 24 });
    if b != 0 {
        with_ps(b, |hps, _| unsafe { GpiSetBitmapBits(hps, 0, h, p.add(len), p) });
    }
    b
}

/// The average colour of a bitmap (a pattern brush is that colour).
fn average(h: u16) -> u32 {
    let Some((_, w, bh, _)) = bitmap_of(h) else { return 0 };
    let (w, bh) = (w.min(8), bh.min(8));
    let mut bmi = info(w, bh, 24, &[]);
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let mut buf = alloc::vec![0u8; stride * bh as usize];
    with_ps(h, |hps, _| unsafe { GpiQueryBitmapBits(hps, 0, bh, buf.as_mut_ptr(), bmi.as_mut_ptr()) });
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for y in 0..bh as usize {
        for x in 0..w as usize {
            let o = y * stride + x * 3;
            b += buf[o] as u32;
            g += buf[o + 1] as u32;
            r += buf[o + 2] as u32;
        }
    }
    let n = (w * bh).max(1) as u32;
    (b / n) << 16 | (g / n) << 8 | r / n
}

/// The system bitmaps (LoadBitmap(NULL, OBM_*)): rs2's own drawing, in
/// the button face colour.
fn system_bitmap(id: u16) -> u16 {
    let (w, h) = match id {
        // OBM_CHECKBOXES: 4 x 3 boxes of 14 pels.
        32759 => (56, 42),
        _ => (16, 16),
    };
    let b = new_bitmap(w, h, 24);
    let hdc = create_memory_dc();
    select(hdc, b);
    gdi::fill(hdc, [0, 0, w, h], gdi::sys_color(gdi::COLOR_BTNFACE));
    let dark = gdi::sys_color(gdi::COLOR_BTNTEXT);
    let check = |hdc: u16, ox: i32, oy: i32| {
        for i in 0..3 {
            gdi::fill(hdc, [ox + 3 + i, oy + 6 + i, ox + 4 + i, oy + 9 + i], dark);
        }
        for i in 0..5 {
            gdi::fill(hdc, [ox + 6 + i, oy + 7 - i, ox + 7 + i, oy + 10 - i], dark);
        }
    };
    match id {
        // OBM_CHECK
        32760 => check(hdc, 1, 1),
        32759 => {
            for row in 0..3 {
                for col in 0..4 {
                    let (ox, oy) = (col * 14, row * 14);
                    gdi::fill(hdc, [ox + 1, oy + 1, ox + 13, oy + 13], 0xffffff);
                    gdi::frame(hdc, [ox + 1, oy + 1, ox + 13, oy + 13], dark);
                    if col % 2 == 1 {
                        check(hdc, ox, oy);
                    }
                }
            }
        }
        _ => {}
    }
    select_default(hdc);
    gdi::release_dc(hdc);
    b
}

/// The GDI and USER calls of this module.
pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // SetStretchBltMode(hdc, mode), GetStretchBltMode(hdc)
        3007 => {
            let (h, m) = (a.w(), a.w());
            gdi::set_stretch_mode(h, m) as u32
        }
        3088 => gdi::stretch_mode(a.w()) as u32,
        // BitBlt(hdc, x, y, w, h, src, sx, sy, rop)
        3034 => {
            let (h, x, y, w, hh, src, sx, sy, rop) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.w(), a.i(), a.i(), a.l());
            blt(h, x, y, w, hh, src, sx, sy, w, hh, rop)
        }
        // StretchBlt(hdc, x, y, w, h, src, sx, sy, sw, sh, rop)
        3035 => {
            let (h, x, y, w, hh, src) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.w());
            let (sx, sy, sw, sh, rop) = (a.i(), a.i(), a.i(), a.i(), a.l());
            blt(h, x, y, w, hh, src, sx, sy, sw, sh, rop)
        }
        // SetBitmapBits(hbm, count, bits), GetBitmapBits(hbm, count, bits)
        3106 => {
            let (h, n, p) = (a.w(), a.l(), a.p());
            set_ddb_bits(h, p, n as usize)
        }
        3074 => {
            let (h, n, p) = (a.w(), a.l(), a.p());
            get_ddb_bits(h, p, n as usize)
        }
        // CreateBitmap(w, h, planes, bpp, bits)
        3048 => {
            let (w, h, planes, bpp, p) = (a.i(), a.i(), a.w(), a.w(), a.p());
            create_bitmap(w, h, planes * bpp, p)
        }
        // CreateBitmapIndirect(BITMAP: type, width, height, widthbytes,
        // planes, bitspixel, bits)
        3049 => {
            let p = a.p();
            if p.is_null() {
                return Some(0);
            }
            let (w, h) = (w16(p, 2) as i16 as i32, w16(p, 4) as i16 as i32);
            let bpp = unsafe { *p.add(8) as u16 * *p.add(9) as u16 };
            create_bitmap(w, h, bpp, thunk16::flat(l32(p, 10)))
        }
        // CreateCompatibleBitmap(hdc, w, h), CreateDiscardableBitmap
        3051 | 3156 => {
            let (h, w, hh) = (a.w(), a.i(), a.i());
            new_bitmap(w, hh, if is_mono(h) { 1 } else { 24 }) as u32
        }
        // CreateCompatibleDC(hdc)
        3052 => {
            a.w();
            create_memory_dc() as u32
        }
        // CreateDC(driver, device, output, init), CreateIC: a memory DC
        // for the display (the screen itself is PM's).
        3053 | 3153 => {
            let _ = (a.l(), a.l(), a.l(), a.l());
            create_memory_dc() as u32
        }
        // DeleteDC(hdc)
        3068 => {
            let h = a.w();
            gdi::release_dc(h) as u32
        }
        // StretchDIBits(hdc, x, y, w, h, sx, sy, sw, sh, bits, bmi, usage, rop)
        3439 => {
            let (h, x, y, w, hh) = (a.w(), a.i(), a.i(), a.i(), a.i());
            let (sx, sy, sw, sh, bits, bmi, usage, rop) = (a.i(), a.i(), a.i(), a.i(), a.p(), a.p(), a.w(), a.l());
            draw_dib(h, x, y, w, hh, sx, sy, sw, sh, bits, bmi, usage, rop) as u32
        }
        // SetDIBits(hdc, hbm, start, scans, bits, bmi, usage)
        3440 => {
            let (h, hbm, start, scans, bits, bmi, usage) = (a.w(), a.w(), a.w(), a.w(), a.p(), a.p(), a.w());
            set_dib_bits(h, hbm, start as i32, scans as i32, bits, bmi, usage) as u32
        }
        // GetDIBits(hdc, hbm, start, scans, bits, bmi, usage)
        3441 => {
            let (_h, hbm, start, scans, bits, bmi, _usage) = (a.w(), a.w(), a.w(), a.w(), a.p(), a.p(), a.w());
            get_dib_bits(hbm, start as i32, scans as i32, bits, bmi) as u32
        }
        // CreateDIBitmap(hdc, header, init, bits, bmi, usage)
        3442 => {
            let (h, hdr, init, bits, bmi, usage) = (a.w(), a.p(), a.l(), a.p(), a.p(), a.w());
            if hdr.is_null() {
                return Some(0);
            }
            let (w, hh, _) = dib_size(hdr);
            let b = new_bitmap(w, hh, if is_mono(h) { 1 } else { 24 });
            if b != 0 && init & CBM_INIT != 0 && !bits.is_null() && !bmi.is_null() {
                set_dib_bits(h, b, 0, hh, bits, bmi, usage);
            }
            b as u32
        }
        // SetDIBitsToDevice(hdc, x, y, w, h, sx, sy, start, scans, bits, bmi, usage)
        3443 => {
            let (h, x, y, w, hh, sx, sy) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.i(), a.i());
            let (start, scans, bits, bmi, usage) = (a.w() as i32, a.w() as i32, a.p(), a.p(), a.w());
            if bmi.is_null() {
                return Some(0);
            }
            // The bits hold scans start.. of the DIB (bottom-up).
            let (_, dib_h, _) = dib_size(bmi);
            let sy = sy - (dib_h - start - scans);
            let mut hdr = dib_info(h, bmi, usage);
            if hdr.len() >= 12 && l32(hdr.as_ptr(), 0) != 12 {
                hdr[8..12].copy_from_slice(&(scans as u32).to_le_bytes());
            }
            let _ = dib_len(&hdr, scans);
            let Some(d) = gdi::dc(h) else { return Some(0) };
            let (dh, hps) = (d.height, d.hps);
            let (x, y) = gdi::to_dev(d, x, y);
            blit_colors(h);
            let pts = [
                PointL { x, y: dh - y - hh },
                PointL { x: x + w - 1, y: dh - y - 1 },
                PointL { x: sx, y: scans - sy - hh },
                PointL { x: sx + w, y: scans - sy },
            ];
            unsafe { GpiDrawBits(hps, bits, hdr.as_ptr(), 4, pts.as_ptr(), 0xcc, 2) };
            scans as u32
        }
        // CreatePatternBrush(hbm), CreateDIBPatternBrush(hglobal, usage):
        // a solid brush of the pattern's average colour.
        3060 => {
            let h = a.w();
            gdi::new_obj(Obj::Brush { color: average(h), null: false }) as u32
        }
        3445 => {
            let (_g, _u) = (a.w(), a.w());
            gdi::new_obj(Obj::Brush { color: 0x808080, null: false }) as u32
        }
        // CreatePalette(LOGPALETTE: version, count, entries)
        3360 => {
            let p = a.p();
            if p.is_null() {
                return Some(0);
            }
            let n = w16(p, 2) as usize;
            let v = (0..n.min(1024)).map(|i| l32(p, 4 + 4 * i) & 0xffffff).collect();
            gdi::new_obj(Obj::Palette(v)) as u32
        }
        // GetPaletteEntries(hpal, start, n, entries), SetPaletteEntries
        3363 | 3364 => {
            let (h, start, n, p) = (a.w(), a.w() as usize, a.w() as usize, a.p());
            let Some(Obj::Palette(pal)) = gdi::obj_mut(h) else { return Some(0) };
            if p.is_null() {
                return Some(if ord == 3363 { pal.len() as u32 } else { 0 });
            }
            let mut done = 0;
            for i in 0..n {
                let k = start + i;
                if k >= pal.len() {
                    break;
                }
                if ord == 3363 {
                    unsafe { (p.add(4 * i) as *mut u32).write_unaligned(pal[k]) };
                } else {
                    pal[k] = l32(p, 4 * i) & 0xffffff;
                }
                done += 1;
            }
            done
        }
        // RealizeDefaultPalette(hdc), UpdateColors(hdc)
        3365 | 3366 => {
            a.w();
            0
        }
        // AnimatePalette(hpal, start, n, entries): the entries change.
        3367 => {
            let (h, start, n, p) = (a.w(), a.w() as usize, a.w() as usize, a.p());
            if let (Some(Obj::Palette(pal)), false) = (gdi::obj_mut(h), p.is_null()) {
                for i in 0..n {
                    if let Some(e) = pal.get_mut(start + i) {
                        *e = l32(p, 4 * i) & 0xffffff;
                    }
                }
            }
            0
        }
        // ResizePalette(hpal, n)
        3368 => {
            let (h, n) = (a.w(), a.w() as usize);
            let Some(Obj::Palette(pal)) = gdi::obj_mut(h) else { return Some(0) };
            pal.resize(n, 0);
            1
        }
        // GetNearestPaletteIndex(hpal, color)
        3370 => {
            let (h, c) = (a.w(), a.l());
            let Some(Obj::Palette(pal)) = gdi::obj(h) else { return Some(0) };
            nearest(&pal, c) as u32
        }
        // GetNearestColor(hdc, color): the display has every colour.
        3154 => {
            let (_h, c) = (a.w(), a.l());
            c & 0xffffff
        }
        // SetSystemPaletteUse(hdc, use), GetSystemPaletteUse(hdc):
        // SYSPAL_STATIC.
        3373 => {
            a.w();
            a.w();
            1
        }
        3374 => {
            a.w();
            1
        }
        // GetSystemPaletteEntries(hdc, start, n, entries)
        3375 => {
            let (_h, start, n, p) = (a.w(), a.w() as usize, a.w() as usize, a.p());
            let all = ddb_colors(8);
            if p.is_null() {
                return Some(256);
            }
            for i in 0..n {
                let c = all.get(start + i).copied().unwrap_or(0);
                unsafe { (p.add(4 * i) as *mut u32).write_unaligned(c) };
            }
            n as u32
        }
        // SelectPalette(hdc, hpal, background), RealizePalette(hdc)
        2282 => {
            let (h, p, _bg) = (a.w(), a.w(), a.w());
            gdi::select_palette(h, p) as u32
        }
        2283 => {
            a.w();
            0
        }
        // LoadBitmap(hinst, name)
        2175 => {
            let (hi, n) = (a.w(), a.l());
            let Some(name) = crate::res::Name::of(n) else { return Some(0) };
            if hi == 0 {
                return Some(match name {
                    crate::res::Name::Id(id) if id >= 32734 => system_bitmap(id) as u32,
                    _ => 0,
                });
            }
            crate::res::find(hi, 2, &name).map_or(0, |d| from_dib(d) as u32)
        }
        _ => return None,
    })
}

/// CreateBitmap: monochrome (1 bit) or colour, with DDB bits.
fn create_bitmap(w: i32, h: i32, bpp: u16, bits: *const u8) -> u32 {
    let b = new_bitmap(w, h, bpp);
    if b != 0 && !bits.is_null() {
        let stride = ((w.max(1) as usize * if bpp == 1 { 1 } else { bpp as usize }).div_ceil(16)) * 2;
        if bpp == 1 || bpp == 24 {
            set_ddb_bits(b, bits, stride * h.max(1) as usize);
        } else {
            // 4 and 8 bits: indexes into the default colours.
            let src_stride = ((w as usize * bpp as usize).div_ceil(16)) * 2;
            let dst_stride = ((w as usize * bpp as usize).div_ceil(32)) * 4;
            let mut buf = alloc::vec![0u8; dst_stride * h as usize];
            for y in 0..h as usize {
                let s = unsafe { core::slice::from_raw_parts(bits.add(y * src_stride), src_stride) };
                let at = (h as usize - 1 - y) * dst_stride;
                let n = src_stride.min(dst_stride);
                buf[at..at + n].copy_from_slice(&s[..n]);
            }
            let bmi = info(w, h, bpp, &ddb_colors(bpp));
            with_ps(b, |hps, _| unsafe { GpiSetBitmapBits(hps, 0, h, buf.as_ptr(), bmi.as_ptr()) });
        }
    }
    b as u32
}

/// The index of the palette colour nearest to a COLORREF.
fn nearest(pal: &[u32], c: u32) -> usize {
    let d = |a: u32| {
        let dr = (a & 0xff) as i32 - (c & 0xff) as i32;
        let dg = ((a >> 8) & 0xff) as i32 - ((c >> 8) & 0xff) as i32;
        let db = ((a >> 16) & 0xff) as i32 - ((c >> 16) & 0xff) as i32;
        dr * dr + dg * dg + db * db
    };
    (0..pal.len()).min_by_key(|&i| d(pal[i])).unwrap_or(0)
}
