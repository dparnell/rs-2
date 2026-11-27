//! GDI32's entry points, on gdi.rs, bitmap.rs, region.rs and draw.rs
//! (WIN16's GDI): 32-bit coordinates and structures, A and W text.
//!
//! DIB sections (CreateDIBSection) are bitmaps whose bits the program
//! writes directly: a PM bitmap and the program's buffer, kept in step
//! around each call that uses one (the buffer goes into the PM bitmap
//! before a call reads the bitmap, and comes back after a call drew into
//! it). PM's bitmaps are bottom-up with 1, 4, 8 or 24 bits per pel:
//! top-down DIBs and 16 and 32 bits per pel (BI_RGB and BI_BITFIELDS)
//! are converted on the way.

use alloc::vec::Vec;

use crate::bitmap::{self, display_bits, new_bitmap};
use crate::gdi::{self, dc, obj, Font, Obj, Pat, DcKind, FACES, OBJ_BASE, OPAQUE, TRANSPARENT};
use crate::pm::*;
use crate::region;
use crate::{read_rect, wide_str, write_point, write_rect, Global};

const CBM_INIT: u32 = 4;

/// Text arguments: `n` characters (-1: NUL-terminated) of an A or W
/// string, in the process code page.
pub fn text_arg(p: u32, n: i32, wide: bool) -> Vec<u8> {
    if p == 0 {
        return Vec::new();
    }
    if wide {
        let w = if n < 0 { unsafe { crate::wstr(p as *const u16) } } else { unsafe { core::slice::from_raw_parts(p as *const u16, n as usize) } };
        crate::narrow(w)
    } else if n < 0 {
        unsafe { crate::cstr(p as *const u8) }.to_vec()
    } else {
        unsafe { core::slice::from_raw_parts(p as *const u8, n as usize) }.to_vec()
    }
}

fn h16(h: u32) -> u16 {
    if h > 0xffff { 0 } else { h as u16 }
}

// ---- DIB sections ----

struct Section {
    h: u16,
    /// The program's bits.
    bits: u32,
    /// The BITMAPINFO it was made with (header and colours).
    bmi: Vec<u8>,
    w: i32,
    /// Negative: top-down.
    height: i32,
    bpp: u16,
    size: usize,
}

static SECTIONS: Global<Vec<Section>> = Global::new(Vec::new());

fn section(h: u16) -> Option<&'static Section> {
    SECTIONS.get().iter().find(|s| s.h == h)
}

/// The DIB section a memory DC has selected.
fn dc_section(hdc: u16) -> Option<u16> {
    let d = dc(hdc)?;
    (d.kind == DcKind::Memory && d.bitmap != 0).then_some(d.bitmap).filter(|&b| section(b).is_some())
}

/// The program's bits into the PM bitmap.
fn upload(h: u16) {
    let Some(s) = section(h) else { return };
    let (hdr, bits) = normalize_dib(0, s.bmi.as_ptr(), s.bits as *const u8, 0, s.height.abs());
    bitmap::with_ps(h, |hps, _| unsafe { GpiSetBitmapBits(hps, 0, s.height.abs(), bits.as_ptr(), hdr.as_ptr()) });
}

/// The PM bitmap's bits into the program's buffer.
fn download(h: u16) {
    let Some(s) = section(h) else { return };
    read_dib(h, s.bmi.as_ptr(), s.bits as *mut u8, 0, s.height.abs());
}

/// Run a call that draws on a DC (and reads from `src`), keeping DIB
/// sections in step.
pub fn drawing<R>(hdc: u16, src: u16, f: impl FnOnce() -> R) -> R {
    let ds = dc_section(hdc);
    if let Some(b) = ds {
        upload(b);
    }
    if src != hdc {
        if let Some(b) = dc_section(src) {
            upload(b);
        }
    }
    let r = f();
    if let Some(b) = ds {
        download(b);
    }
    r
}

fn l32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}

fn w16(p: *const u8, o: usize) -> u16 {
    unsafe { (p.add(o) as *const u16).read_unaligned() }
}

/// A DIB's (width, height (negative: top-down), bits per pel,
/// compression).
fn dib_fields(bmi: *const u8) -> (i32, i32, u16, u32) {
    if l32(bmi, 0) == 12 {
        (w16(bmi, 4) as i32, w16(bmi, 6) as i32, w16(bmi, 10), 0)
    } else {
        (l32(bmi, 4) as i32, l32(bmi, 8) as i32, w16(bmi, 14), l32(bmi, 16))
    }
}

/// The colour masks of a 16 or 32 bit DIB (BI_BITFIELDS: after a 40
/// byte header, or in a V4/V5 header; BI_RGB: 5-5-5 or 8-8-8).
fn masks(bmi: *const u8, bpp: u16, comp: u32) -> [u32; 3] {
    if comp == 3 {
        let o = if l32(bmi, 0) >= 52 { 40 } else { l32(bmi, 0) as usize };
        return [l32(bmi, o), l32(bmi, o + 4), l32(bmi, o + 8)];
    }
    if bpp == 16 { [0x7c00, 0x03e0, 0x001f] } else { [0xff_0000, 0xff00, 0xff] }
}

/// A channel of a pel by its mask, scaled to 8 bits.
fn channel(v: u32, m: u32) -> u8 {
    if m == 0 {
        return 0;
    }
    let shift = m.trailing_zeros();
    let bits = (m >> shift).count_ones();
    let c = (v & m) >> shift;
    if bits >= 8 { (c >> (bits - 8)) as u8 } else { ((c << (8 - bits)) | (c >> (2 * bits).saturating_sub(8).min(31))) as u8 }
}

/// A DIB as PM takes it: a 40-byte header (with colours for 8 bits or
/// fewer, DIB_PAL_COLORS indexes looked up in the DC's palette), rows
/// bottom-up, 1, 4, 8 or 24 bits per pel. `scans` rows of the bits.
pub fn normalize_dib(hdc: u16, bmi: *const u8, bits: *const u8, usage: u16, scans: i32) -> (Vec<u8>, Vec<u8>) {
    let (w, h, bpp, comp) = dib_fields(bmi);
    let scans = scans.max(0) as usize;
    let mut hdr = bitmap::dib_info(hdc, bmi, usage);
    let cb = l32(bmi, 0) as usize;
    // RLE: as it is.
    if comp == 1 || comp == 2 {
        let n = if cb >= 24 { l32(bmi, 20) as usize } else { 0 };
        let b = if bits.is_null() || n == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(bits, n) }.to_vec() };
        return (hdr, b);
    }
    let w = w.max(0) as usize;
    let stride = (w * bpp as usize).div_ceil(32) * 4;
    let src = |y: usize| -> &[u8] {
        if bits.is_null() {
            return &[];
        }
        // Row y counted from the bottom.
        let row = if h < 0 { scans - 1 - y } else { y };
        unsafe { core::slice::from_raw_parts(bits.add(row * stride), stride) }
    };
    if matches!(bpp, 1 | 4 | 8 | 24) && cb != 12 {
        let mut out = Vec::with_capacity(stride * scans);
        for y in 0..scans {
            out.extend_from_slice(src(y));
        }
        // A plain 40-byte header, bottom-up.
        let n = bitmap::dib_header(bmi, usage).1;
        let mut nh = Vec::with_capacity(40 + 4 * n);
        nh.extend_from_slice(&hdr[..40.min(hdr.len())]);
        nh.resize(40, 0);
        nh[0..4].copy_from_slice(&40u32.to_le_bytes());
        nh[8..12].copy_from_slice(&(scans as u32).to_le_bytes());
        nh[16..20].copy_from_slice(&0u32.to_le_bytes());
        nh.extend_from_slice(&hdr[cb.min(hdr.len())..]);
        return (nh, out);
    }
    if cb == 12 {
        return (hdr, (0..scans).flat_map(|y| src(y).to_vec()).collect());
    }
    // 16 and 32 bits per pel: to 24.
    let m = masks(bmi, bpp, comp);
    let ostride = (w * 3).div_ceil(4) * 4;
    let mut out = alloc::vec![0u8; ostride * scans];
    for y in 0..scans {
        let r = src(y);
        if r.is_empty() {
            break;
        }
        for x in 0..w {
            let v = if bpp == 16 { u16::from_le_bytes([r[2 * x], r[2 * x + 1]]) as u32 } else { u32::from_le_bytes([r[4 * x], r[4 * x + 1], r[4 * x + 2], r[4 * x + 3]]) };
            let o = y * ostride + 3 * x;
            out[o] = channel(v, m[2]);
            out[o + 1] = channel(v, m[1]);
            out[o + 2] = channel(v, m[0]);
        }
    }
    hdr.truncate(40);
    hdr[8..12].copy_from_slice(&(scans as u32).to_le_bytes());
    hdr[14..16].copy_from_slice(&24u16.to_le_bytes());
    hdr[16..20].copy_from_slice(&0u32.to_le_bytes());
    (hdr, out)
}

/// GetDIBits into a DIB of any format: the PM bitmap's rows (24 bits
/// for 16 and 32 bit DIBs), converted and turned around as the header
/// asks. The rows copied.
fn read_dib(h: u16, bmi: *const u8, bits: *mut u8, start: i32, scans: i32) -> i32 {
    let Some((_, bw, bh, _)) = bitmap::bitmap_of(h) else { return 0 };
    let (w, dh, bpp, comp) = dib_fields(bmi);
    let w = (w.max(0) as usize).min(bw as usize).max(1);
    let scans = scans.clamp(0, bh - start.max(0));
    if bits.is_null() || scans <= 0 {
        return 0;
    }
    let cb = l32(bmi, 0) as usize;
    let direct = matches!(bpp, 1 | 4 | 8 | 24);
    let qbpp = if direct { bpp } else { 24 };
    let qstride = (bw as usize * qbpp as usize).div_ceil(32) * 4;
    let mut buf = alloc::vec![0u8; qstride * scans as usize];
    let n = bitmap::dib_header(bmi, 0).1;
    let mut q = Vec::with_capacity(40 + 4 * n);
    q.extend_from_slice(unsafe { core::slice::from_raw_parts(bmi, 40.min(cb)) });
    q.resize(40, 0);
    q[0..4].copy_from_slice(&40u32.to_le_bytes());
    q[4..8].copy_from_slice(&(bw as u32).to_le_bytes());
    q[8..12].copy_from_slice(&(scans as u32).to_le_bytes());
    q[14..16].copy_from_slice(&qbpp.to_le_bytes());
    q[16..20].copy_from_slice(&0u32.to_le_bytes());
    if direct && bpp <= 8 {
        q.extend_from_slice(unsafe { core::slice::from_raw_parts(bmi.add(cb), 4 * n) });
    }
    let got = bitmap::with_ps(h, |hps, _| unsafe { GpiQueryBitmapBits(hps, start, scans, buf.as_mut_ptr(), q.as_mut_ptr()) }).unwrap_or(0).max(0) as usize;
    // PM gives the colour table of 1, 4 and 8-bit bits: the caller's.
    if direct && bpp <= 8 && q.len() >= 40 + 4 * n {
        unsafe { core::ptr::copy_nonoverlapping(q.as_ptr().add(40), bmi.add(cb) as *mut u8, 4 * n) };
    }
    let dstride = (w * bpp as usize).div_ceil(32) * 4;
    let total = dh.unsigned_abs() as usize;
    let m = masks(bmi, bpp, comp);
    let pack = |v: u8, mask: u32| -> u32 {
        if mask == 0 {
            return 0;
        }
        let shift = mask.trailing_zeros();
        let bits = (mask >> shift).count_ones();
        let c = if bits >= 8 { (v as u32) << (bits - 8) } else { v as u32 >> (8 - bits) };
        c << shift & mask
    };
    for y in 0..got {
        // The buffer holds the scans asked for, from `start` up (a top-down
        // DIB's from its top down).
        let drow = if dh < 0 { (got).saturating_sub(1 + y) } else { y };
        let _ = total;
        let s = &buf[y * qstride..(y + 1) * qstride];
        let d = unsafe { core::slice::from_raw_parts_mut(bits.add(drow * dstride), dstride) };
        if direct {
            let k = dstride.min(qstride);
            d[..k].copy_from_slice(&s[..k]);
            continue;
        }
        for x in 0..w {
            let (b, g, r) = (s[3 * x], s[3 * x + 1], s[3 * x + 2]);
            let v = pack(r, m[0]) | pack(g, m[1]) | pack(b, m[2]);
            if bpp == 16 {
                d[2 * x..2 * x + 2].copy_from_slice(&(v as u16).to_le_bytes());
            } else {
                d[4 * x..4 * x + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    got as i32
}

/// CreateDIBSection(hdc, bmi, usage, ppvBits, hSection, offset).
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CreateDIBSection(hdc: u32, bmi: *const u8, usage: u32, ppv: *mut u32, _section: u32, _offset: u32) -> u32 {
    if bmi.is_null() {
        return 0;
    }
    let (w, h, bpp, _) = dib_fields(bmi);
    if w <= 0 || h == 0 || !matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32) {
        return crate::fail(87, 0);
    }
    let stride = (w as usize * bpp as usize).div_ceil(32) * 4;
    let size = stride * h.unsigned_abs() as usize;
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocMem(&mut p, size.max(1) as u32, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT) } != 0 {
        return crate::fail(8, 0);
    }
    let b = new_bitmap(w, h.abs(), if bpp == 1 { 1 } else { 24 });
    if b == 0 {
        unsafe { os2::DosFreeMem(p) };
        return crate::fail(8, 0);
    }
    // The header and colours as given (DIB_PAL_COLORS: looked up now).
    let (len, _) = bitmap::dib_header(bmi, usage as u16);
    let mut info = bitmap::dib_info(h16(hdc), bmi, usage as u16);
    if l32(bmi, 16) == 3 && l32(bmi, 0) == 40 {
        info.extend_from_slice(unsafe { core::slice::from_raw_parts(bmi.add(40), 12) });
    }
    let _ = len;
    SECTIONS.get().push(Section { h: b, bits: p as u32, bmi: info, w, height: h, bpp, size });
    upload(b);
    if !ppv.is_null() {
        unsafe { *ppv = p as u32 };
    }
    b as u32
}

/// A DIB section's bitmap is deleted: its bits go.
fn section_deleted(h: u16) {
    let v = SECTIONS.get();
    if let Some(i) = v.iter().position(|s| s.h == h) {
        let s = v.remove(i);
        unsafe { os2::DosFreeMem(s.bits as *mut u8) };
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GdiFlush() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GdiSetBatchLimit(_n: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GdiGetBatchLimit() -> u32 {
    1
}

// ---- DC attributes ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetBkColor(h: u32, c: u32) -> u32 {
    if dc(h16(h)).is_none() {
        return 0xffff_ffff;
    }
    gdi::set_bk_color(h16(h), c)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetTextColor(h: u32, c: u32) -> u32 {
    if dc(h16(h)).is_none() {
        return 0xffff_ffff;
    }
    gdi::set_text_color(h16(h), c)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetBkColor(h: u32) -> u32 {
    gdi::bk_color(h16(h))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextColor(h: u32) -> u32 {
    gdi::text_color(h16(h))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetBkMode(h: u32, m: u32) -> u32 {
    if m != OPAQUE as u32 && m != TRANSPARENT as u32 {
        return 0;
    }
    gdi::set_bk_mode(h16(h), m as u16) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetBkMode(h: u32) -> u32 {
    dc(h16(h)).map_or(0, |d| d.s.bk_mode as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetROP2(h: u32, r: u32) -> u32 {
    dc(h16(h)).map_or(0, |d| core::mem::replace(&mut d.s.rop2, r as u16) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetROP2(h: u32) -> u32 {
    dc(h16(h)).map_or(0, |d| d.s.rop2 as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetTextAlign(h: u32, a: u32) -> u32 {
    dc(h16(h)).map_or(0xffff_ffff, |d| core::mem::replace(&mut d.s.align, a as u16) as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextAlign(h: u32) -> u32 {
    dc(h16(h)).map_or(0xffff_ffff, |d| d.s.align as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetStretchBltMode(h: u32, m: u32) -> u32 {
    gdi::set_stretch_mode(h16(h), m as u16) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetStretchBltMode(h: u32) -> u32 {
    gdi::stretch_mode(h16(h)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetPolyFillMode(h: u32, m: u32) -> u32 {
    gdi::set_poly_fill_mode(h16(h), m as u16) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetPolyFillMode(h: u32) -> u32 {
    gdi::poly_fill_mode(h16(h)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetTextCharacterExtra(_h: u32, _n: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextCharacterExtra(_h: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetTextJustification(_h: u32, _e: i32, _b: i32) -> i32 {
    1
}

/// SetGraphicsMode: GM_COMPATIBLE (1); GM_ADVANCED (2) is accepted, but
/// there are no world transforms.
#[unsafe(no_mangle)]
pub extern "stdcall" fn SetGraphicsMode(_h: u32, m: i32) -> i32 {
    if m == 1 || m == 2 { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetGraphicsMode(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWorldTransform(_h: u32, _x: u32) -> i32 {
    crate::fail(120, 0)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ModifyWorldTransform(_h: u32, _x: u32, _m: u32) -> i32 {
    crate::fail(120, 0)
}

/// GetWorldTransform: the identity.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWorldTransform(_h: u32, x: *mut f32) -> i32 {
    if !x.is_null() {
        for (i, v) in [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0].iter().enumerate() {
            unsafe { x.add(i).write_unaligned(*v) };
        }
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetLayout(_h: u32, _l: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetLayout(_h: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetBrushOrgEx(_h: u32, _x: i32, _y: i32, old: u32) -> i32 {
    write_point(old, (0, 0));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetBrushOrgEx(_h: u32, p: u32) -> i32 {
    write_point(p, (0, 0));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMapperFlags(_h: u32, _f: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetICMMode(_h: u32, _m: i32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetArcDirection(_h: u32, _d: i32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMiterLimit(_h: u32, _l: f32, _old: u32) -> i32 {
    1
}

// ---- Mapping ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetMapMode(h: u32, m: i32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    if !(1..=8).contains(&m) {
        return 0;
    }
    let m = m as u16;
    let old = core::mem::replace(&mut d.s.map, m);
    let dpi = if d.kind == DcKind::Printer { 300 } else { 96 };
    if let Some((w, v)) = gdi::fixed_extents(m, dpi) {
        d.s.wext = w;
        d.s.vext = v;
    } else if m == gdi::MM_TEXT || (old != gdi::MM_ISOTROPIC && old != gdi::MM_ANISOTROPIC) {
        d.s.wext = (1, 1);
        d.s.vext = (1, 1);
    }
    old as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetMapMode(h: u32) -> i32 {
    dc(h16(h)).map_or(0, |d| d.s.map as i32)
}

/// Set one of the window/viewport origins/extents (0 window origin, 1
/// window extent, 2 viewport origin, 3 viewport extent).
fn set_mapping(h: u32, which: u32, x: i32, y: i32, old: u32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let fixed_ext = which % 2 == 1 && d.s.map != gdi::MM_ISOTROPIC && d.s.map != gdi::MM_ANISOTROPIC;
    let slot = match which {
        0 => &mut d.s.worg,
        1 => &mut d.s.wext,
        2 => &mut d.s.vorg,
        _ => &mut d.s.vext,
    };
    write_point(old, *slot);
    if !fixed_ext && !(which % 2 == 1 && (x == 0 || y == 0)) {
        *slot = (x, y);
    }
    if d.s.map == gdi::MM_ISOTROPIC && which % 2 == 1 {
        gdi::isotropic(d);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowOrgEx(h: u32, x: i32, y: i32, old: u32) -> i32 {
    set_mapping(h, 0, x, y, old)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetWindowExtEx(h: u32, x: i32, y: i32, old: u32) -> i32 {
    set_mapping(h, 1, x, y, old)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetViewportOrgEx(h: u32, x: i32, y: i32, old: u32) -> i32 {
    set_mapping(h, 2, x, y, old)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetViewportExtEx(h: u32, x: i32, y: i32, old: u32) -> i32 {
    set_mapping(h, 3, x, y, old)
}

fn offset_org(h: u32, window: bool, dx: i32, dy: i32, old: u32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let slot = if window { &mut d.s.worg } else { &mut d.s.vorg };
    write_point(old, *slot);
    *slot = (slot.0 + dx, slot.1 + dy);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OffsetWindowOrgEx(h: u32, x: i32, y: i32, old: u32) -> i32 {
    offset_org(h, true, x, y, old)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OffsetViewportOrgEx(h: u32, x: i32, y: i32, old: u32) -> i32 {
    offset_org(h, false, x, y, old)
}

fn scale_ext(h: u32, window: bool, xn: i32, xd: i32, yn: i32, yd: i32, old: u32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let aniso = d.s.map == gdi::MM_ISOTROPIC || d.s.map == gdi::MM_ANISOTROPIC;
    let slot = if window { &mut d.s.wext } else { &mut d.s.vext };
    let o = *slot;
    write_point(old, o);
    if aniso && xd != 0 && yd != 0 {
        *slot = (o.0 * xn / xd, o.1 * yn / yd);
        if d.s.map == gdi::MM_ISOTROPIC {
            gdi::isotropic(d);
        }
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ScaleWindowExtEx(h: u32, xn: i32, xd: i32, yn: i32, yd: i32, old: u32) -> i32 {
    scale_ext(h, true, xn, xd, yn, yd, old)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ScaleViewportExtEx(h: u32, xn: i32, xd: i32, yn: i32, yd: i32, old: u32) -> i32 {
    scale_ext(h, false, xn, xd, yn, yd, old)
}

fn get_mapping(h: u32, which: u32, p: u32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    write_point(p, match which {
        0 => d.s.worg,
        1 => d.s.wext,
        2 => d.s.vorg,
        _ => d.s.vext,
    });
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowOrgEx(h: u32, p: u32) -> i32 {
    get_mapping(h, 0, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetWindowExtEx(h: u32, p: u32) -> i32 {
    get_mapping(h, 1, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetViewportOrgEx(h: u32, p: u32) -> i32 {
    get_mapping(h, 2, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetViewportExtEx(h: u32, p: u32) -> i32 {
    get_mapping(h, 3, p)
}

fn convert_points(h: u32, p: u32, n: i32, to_log: bool) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    for i in 0..n.max(0) as u32 {
        let Some((x, y)) = crate::read_point(p + 8 * i) else { break };
        write_point(p + 8 * i, if to_log { gdi::to_log(d, x, y) } else { gdi::to_dev(d, x, y) });
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DPtoLP(h: u32, p: u32, n: i32) -> i32 {
    convert_points(h, p, n, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LPtoDP(h: u32, p: u32, n: i32) -> i32 {
    convert_points(h, p, n, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDCOrgEx(h: u32, p: u32) -> i32 {
    match gdi::dc_origin(h16(h)) {
        Some(o) => {
            write_point(p, o);
            1
        }
        None => 0,
    }
}

// ---- Lines and shapes ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn MoveToEx(h: u32, x: i32, y: i32, old: u32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let o = core::mem::replace(&mut d.s.pos, (x, y));
    write_point(old, o);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCurrentPositionEx(h: u32, p: u32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    write_point(p, d.s.pos);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LineTo(h: u32, x: i32, y: i32) -> i32 {
    let hdc = h16(h);
    drawing(hdc, hdc, || {
        let Some(d) = dc(hdc) else { return 0 };
        let (x0, y0) = d.s.pos;
        d.s.pos = (x, y);
        let Some((color, width)) = gdi::pen_color(d) else { return 1 };
        let color = if d.s.rop2 == 6 || d.s.rop2 == 7 { 0xffffff ^ color } else { color };
        let from = gdi::pm_pt(d, x0, y0);
        let to = gdi::pm_pt(d, x, y);
        unsafe {
            GpiSetColor(d.hps, gdi::rgb(color));
            GpiSetLineWidth(d.hps, width << 16);
            GpiMove(d.hps, &from);
            GpiLine(d.hps, &to);
        }
        1
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Rectangle(h: u32, l: i32, t: i32, r: i32, b: i32) -> i32 {
    let hdc = h16(h);
    drawing(hdc, hdc, || gdi::rectangle(hdc, l, t, r, b) as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RoundRect(h: u32, l: i32, t: i32, r: i32, b: i32, w: i32, hh: i32) -> i32 {
    let hdc = h16(h);
    drawing(hdc, hdc, || gdi::round_rect(hdc, l, t, r, b, w, hh) as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Ellipse(h: u32, l: i32, t: i32, r: i32, b: i32) -> i32 {
    let hdc = h16(h);
    drawing(hdc, hdc, || {
        let Some(d) = dc(hdc) else { return 0 };
        gdi::ellipse(d, l, t, r, b);
        1
    })
}

fn read_pts(p: u32, n: i32) -> Vec<(i32, i32)> {
    if p == 0 || n <= 0 {
        return Vec::new();
    }
    region::read_points(p as *const u8, n as usize)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Polygon(h: u32, p: u32, n: i32) -> i32 {
    let hdc = h16(h);
    let pts = read_pts(p, n);
    if pts.len() < 2 {
        return 0;
    }
    drawing(hdc, hdc, || gdi::poly(hdc, &pts, true, true) as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Polyline(h: u32, p: u32, n: i32) -> i32 {
    let hdc = h16(h);
    let pts = read_pts(p, n);
    if pts.len() < 2 {
        return 0;
    }
    drawing(hdc, hdc, || gdi::poly(hdc, &pts, false, false) as i32)
}

/// PolylineTo: from the current position through the points.
#[unsafe(no_mangle)]
pub extern "stdcall" fn PolylineTo(h: u32, p: u32, n: u32) -> i32 {
    let hdc = h16(h);
    let mut pts = read_pts(p, n as i32);
    let Some(d) = dc(hdc) else { return 0 };
    pts.insert(0, d.s.pos);
    if let Some(&l) = pts.last() {
        d.s.pos = l;
    }
    drawing(hdc, hdc, || gdi::poly(hdc, &pts, false, false) as i32)
}

fn poly_counts(p: u32, c: u32, n: i32) -> Vec<Vec<(i32, i32)>> {
    let mut polys = Vec::new();
    if p == 0 || c == 0 {
        return polys;
    }
    let mut at = 0;
    for i in 0..n.max(0) as u32 {
        let k = crate::rd32(c, 4 * i) as i32;
        polys.push(read_pts(p + 8 * at, k));
        at += k.max(0) as u32;
    }
    polys
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PolyPolygon(h: u32, p: u32, c: u32, n: i32) -> i32 {
    let hdc = h16(h);
    let polys = poly_counts(p, c, n);
    drawing(hdc, hdc, || gdi::poly_poly(hdc, &polys) as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PolyPolyline(h: u32, p: u32, c: u32, n: u32) -> i32 {
    let hdc = h16(h);
    let polys = poly_counts(p, c, n as i32);
    drawing(hdc, hdc, || {
        for pl in &polys {
            gdi::poly(hdc, pl, false, false);
        }
        1
    })
}

/// PolyBezier: each segment as a line of points along the curve.
#[unsafe(no_mangle)]
pub extern "stdcall" fn PolyBezier(h: u32, p: u32, n: u32) -> i32 {
    let pts = read_pts(p, n as i32);
    if pts.len() < 4 {
        return 0;
    }
    let mut line = alloc::vec![pts[0]];
    let mut i = 0;
    while i + 3 < pts.len() {
        let (a, b, c, d) = (pts[i], pts[i + 1], pts[i + 2], pts[i + 3]);
        for k in 1..=16 {
            let t = k as f32 / 16.0;
            let u = 1.0 - t;
            let f = |a: i32, b: i32, c: i32, d: i32| (u * u * u * a as f32 + 3.0 * u * u * t * b as f32 + 3.0 * u * t * t * c as f32 + t * t * t * d as f32) as i32;
            line.push((f(a.0, b.0, c.0, d.0), f(a.1, b.1, c.1, d.1)));
        }
        i += 3;
    }
    let hdc = h16(h);
    drawing(hdc, hdc, || gdi::poly(hdc, &line, false, false) as i32)
}

fn arc_like(h: u32, kind: u8, l: i32, t: i32, r: i32, b: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let hdc = h16(h);
    let mut pts = crate::draw::arc_points(l, t, r, b, x1, y1, x2, y2);
    drawing(hdc, hdc, || match kind {
        0 => gdi::poly(hdc, &pts, false, false) as i32,
        1 => {
            pts.insert(0, ((l + r) / 2, (t + b) / 2));
            gdi::poly(hdc, &pts, true, true) as i32
        }
        _ => gdi::poly(hdc, &pts, true, true) as i32,
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Arc(h: u32, l: i32, t: i32, r: i32, b: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    arc_like(h, 0, l, t, r, b, x1, y1, x2, y2)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Pie(h: u32, l: i32, t: i32, r: i32, b: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    arc_like(h, 1, l, t, r, b, x1, y1, x2, y2)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn Chord(h: u32, l: i32, t: i32, r: i32, b: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    arc_like(h, 2, l, t, r, b, x1, y1, x2, y2)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PatBlt(h: u32, x: i32, y: i32, w: i32, hh: i32, rop: u32) -> i32 {
    let hdc = h16(h);
    let r = [x.min(x + w), y.min(y + hh), x.max(x + w), y.max(y + hh)];
    drawing(hdc, hdc, || {
        let br = gdi::brush_of(hdc);
        match rop {
            0x0055_0009 => gdi::invert(hdc, r),
            0x0000_0042 => gdi::fill(hdc, r, 0),
            0x00ff_0062 => gdi::fill(hdc, r, 0xffffff),
            0x00f0_0021 if !gdi::is_pattern(br) => gdi::fill_rect(hdc, r, br),
            _ => {
                bitmap::blit(hdc, r[0], r[1], r[2] - r[0], r[3] - r[1], 0, 0, 0, rop);
            }
        }
        1
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetPixel(h: u32, x: i32, y: i32, c: u32) -> u32 {
    let hdc = h16(h);
    if dc(hdc).is_none() {
        return 0xffff_ffff;
    }
    drawing(hdc, hdc, || gdi::fill(hdc, [x, y, x + 1, y + 1], c));
    let c = gdi::rgb(c) as u32;
    (c & 0xff) << 16 | c & 0xff00 | (c >> 16) & 0xff
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetPixelV(h: u32, x: i32, y: i32, c: u32) -> i32 {
    (SetPixel(h, x, y, c) != 0xffff_ffff) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetPixel(h: u32, x: i32, y: i32) -> u32 {
    let hdc = h16(h);
    if let Some(b) = dc_section(hdc) {
        upload(b);
    }
    let Some(d) = dc(hdc) else { return 0xffff_ffff };
    let c = unsafe { GpiQueryPel(d.hps, &gdi::pm_pt(d, x, y)) } as u32;
    (c & 0xff) << 16 | (c & 0xff00) | (c >> 16) & 0xff
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FloodFill(h: u32, x: i32, y: i32, c: u32) -> i32 {
    let hdc = h16(h);
    drawing(hdc, hdc, || crate::draw::flood(hdc, x, y, c, false) as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExtFloodFill(h: u32, x: i32, y: i32, c: u32, t: u32) -> i32 {
    let hdc = h16(h);
    drawing(hdc, hdc, || crate::draw::flood(hdc, x, y, c, t == 1) as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SaveDC(h: u32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    d.saved.push((d.s, d.clipped));
    d.saved.len() as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RestoreDC(h: u32, lvl: i32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let n = d.saved.len() as i32;
    let keep = if lvl < 0 { n + lvl } else { lvl - 1 };
    if keep < 0 || keep >= n {
        return 0;
    }
    let (s, clipped) = d.saved[keep as usize];
    d.saved.truncate(keep as usize);
    d.s = s;
    if d.clipped && !clipped {
        let mut old = 0;
        unsafe { GpiSetClipRegion(d.hps, 0, &mut old) };
        d.clipped = false;
    }
    1
}

// ---- Objects ----

/// The stock objects (GetStockObject(i) is object i): WHITE_BRUSH ...
/// SYSTEM_FIXED_FONT (16), DEFAULT_GUI_FONT (17), DC_BRUSH (18), DC_PEN
/// (19).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetStockObject(i: i32) -> u32 {
    match i {
        0..=8 | 10..=16 => {
            gdi::obj(OBJ_BASE);
            (OBJ_BASE + 2 * i as u16) as u32
        }
        17 => gui_font() as u32,
        18 => (OBJ_BASE) as u32,
        19 => (OBJ_BASE + 2 * 7) as u32,
        _ => 0,
    }
}

/// DEFAULT_GUI_FONT: 8-point MS Sans Serif.
fn gui_font() -> u16 {
    static F: Global<u16> = Global::new(0);
    let f = F.get();
    if *f == 0 {
        *f = gdi::new_obj(Obj::Font(gdi::font_of(-11, 0, 400, false, false, 0x22, b"MS Sans Serif")));
    }
    *f
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SelectObject(h: u32, o: u32) -> u32 {
    let (hdc, o) = (h16(h), h16(o));
    let Some(d) = dc(hdc) else { return 0 };
    match obj(o) {
        Some(Obj::Brush { .. }) => core::mem::replace(&mut d.s.brush, o) as u32,
        Some(Obj::Pen { .. }) => core::mem::replace(&mut d.s.pen, o) as u32,
        Some(Obj::Font(_)) => core::mem::replace(&mut d.s.font, o) as u32,
        Some(Obj::Bitmap { .. }) => {
            if let Some(old) = dc_section(hdc) {
                download(old);
            }
            let r = bitmap::select(hdc, o) as u32;
            if section(o).is_some() {
                upload(o);
            }
            r
        }
        Some(Obj::Region(_)) => region::select_clip(hdc, o),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DeleteObject(o: u32) -> i32 {
    let o = h16(o);
    if o == 0 {
        return 0;
    }
    let sec = section(o).is_some();
    let ok = gdi::delete_object(o);
    if ok && sec {
        section_deleted(o);
    }
    ok as i32
}

/// GetObjectType: OBJ_PEN 1, OBJ_BRUSH 2, OBJ_DC 3, OBJ_PAL 5, OBJ_FONT
/// 6, OBJ_BITMAP 7, OBJ_REGION 8, OBJ_MEMDC 10.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetObjectType(h: u32) -> u32 {
    let h = h16(h);
    if let Some(d) = dc(h) {
        return if d.kind == DcKind::Memory { 10 } else { 3 };
    }
    match obj(h) {
        Some(Obj::Pen { .. }) => 1,
        Some(Obj::Brush { .. }) => 2,
        Some(Obj::Palette(_)) => 5,
        Some(Obj::Font(_)) => 6,
        Some(Obj::Bitmap { .. }) => 7,
        Some(Obj::Region(_)) => 8,
        _ => crate::fail(6, 0),
    }
}

/// GetCurrentObject(hdc, type).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCurrentObject(h: u32, ty: u32) -> u32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    (match ty {
        1 => d.s.pen,
        2 => d.s.brush,
        5 => d.s.palette,
        6 => d.s.font,
        7 => d.bitmap,
        _ => 0,
    }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateSolidBrush(c: u32) -> u32 {
    gdi::new_obj(Obj::Brush { color: c & 0xffffff, null: false, pat: None, src: 0 }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateHatchBrush(style: i32, c: u32) -> u32 {
    let s = (style as u16).min(5);
    gdi::new_obj(Obj::Brush { color: c & 0xffffff, null: false, pat: Some(Pat::Hatch(s)), src: s }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreatePatternBrush(b: u32) -> u32 {
    bitmap::pattern_brush(h16(b)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDIBPatternBrushPt(p: *const u8, usage: u32) -> u32 {
    bitmap::dib_pattern_brush(p, usage as u16) as u32
}

/// LOGBRUSH: lbStyle, lbColor, lbHatch (BS_SOLID 0, BS_NULL 1,
/// BS_HATCHED 2, BS_PATTERN 3, BS_DIBPATTERNPT 6).
fn brush_from(style: u32, c: u32, hatch: u32) -> u16 {
    match style {
        2 => gdi::new_obj(Obj::Brush { color: c & 0xffffff, null: false, pat: Some(Pat::Hatch((hatch as u16).min(5))), src: hatch as u16 }),
        3 => bitmap::pattern_brush(h16(hatch)),
        5 | 6 => bitmap::dib_pattern_brush(hatch as *const u8, c as u16),
        _ => gdi::new_obj(Obj::Brush { color: c & 0xffffff, null: style == 1, pat: None, src: 0 }),
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateBrushIndirect(p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    brush_from(crate::rd32(p, 0), crate::rd32(p, 4), crate::rd32(p, 8)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreatePen(style: i32, width: i32, c: u32) -> u32 {
    gdi::new_obj(Obj::Pen { style: (style & 0xf) as u16, width: width.max(0) as u16, color: c & 0xffffff }) as u32
}

/// LOGPEN: lopnStyle, lopnWidth (POINT), lopnColor.
#[unsafe(no_mangle)]
pub extern "stdcall" fn CreatePenIndirect(p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    CreatePen(crate::rd32(p, 0) as i32, crate::rd32(p, 4) as i32, crate::rd32(p, 12))
}

/// ExtCreatePen(style, width, LOGBRUSH, nstyles, styles): the pen's
/// colour from the brush.
#[unsafe(no_mangle)]
pub extern "stdcall" fn ExtCreatePen(style: u32, width: u32, lb: u32, _n: u32, _s: u32) -> u32 {
    let c = if lb != 0 { crate::rd32(lb, 4) } else { 0 };
    let s = if lb != 0 && crate::rd32(lb, 0) == 1 { 5 } else { style & 0xf };
    gdi::new_obj(Obj::Pen { style: s as u16, width: width as u16, color: c & 0xffffff }) as u32
}

/// CreateFont's arguments to a font.
fn font(height: i32, width: i32, weight: i32, italic: u32, underline: u32, pitch: u32, face: &[u8]) -> u32 {
    let weight = if weight <= 0 { 400 } else { weight as u16 };
    gdi::new_obj(Obj::Font(gdi::font_of(height, width, weight, italic & 0xff != 0, underline & 0xff != 0, pitch as u8, face))) as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CreateFontA(h: i32, w: i32, _e: i32, _o: i32, weight: i32, it: u32, ul: u32, _so: u32, _cs: u32, _op: u32, _cp: u32, _q: u32, pitch: u32, face: *const u8) -> u32 {
    font(h, w, weight, it, ul, pitch, unsafe { crate::cstr(face) })
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CreateFontW(h: i32, w: i32, _e: i32, _o: i32, weight: i32, it: u32, ul: u32, _so: u32, _cs: u32, _op: u32, _cp: u32, _q: u32, pitch: u32, face: *const u16) -> u32 {
    font(h, w, weight, it, ul, pitch, &crate::narrow(unsafe { crate::wstr(face) }))
}

/// LOGFONT: lfHeight, lfWidth, lfEscapement, lfOrientation, lfWeight,
/// lfItalic, lfUnderline, lfStrikeOut, lfCharSet, lfOutPrecision,
/// lfClipPrecision, lfQuality, lfPitchAndFamily, lfFaceName[32].
fn font_indirect(p: u32, wide: bool) -> u32 {
    if p == 0 {
        return 0;
    }
    let b = |o: u32| unsafe { *((p + o) as *const u8) } as u32;
    let face = if wide {
        let w = unsafe { core::slice::from_raw_parts((p + 28) as *const u16, 32) };
        let n = w.iter().position(|&c| c == 0).unwrap_or(32);
        crate::narrow(&w[..n])
    } else {
        let a = unsafe { core::slice::from_raw_parts((p + 28) as *const u8, 32) };
        let n = a.iter().position(|&c| c == 0).unwrap_or(32);
        a[..n].to_vec()
    };
    font(crate::rd32(p, 0) as i32, crate::rd32(p, 4) as i32, crate::rd32(p, 16) as i32, b(20), b(21), b(27), &face)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateFontIndirectA(p: u32) -> u32 {
    font_indirect(p, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateFontIndirectW(p: u32) -> u32 {
    font_indirect(p, true)
}

/// CreateFontIndirectEx(ENUMLOGFONTEXDV): its LOGFONT comes first.
#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateFontIndirectExW(p: u32) -> u32 {
    font_indirect(p, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateFontIndirectExA(p: u32) -> u32 {
    font_indirect(p, false)
}

/// The face name of a font.
fn face_name(f: &Font) -> &'static [u8] {
    FACES[(f.wface as usize).min(FACES.len() - 1)].name
}

/// A LOGFONT (A: 60 bytes, W: 92) of a font.
fn logfont(f: &Font, wide: bool) -> Vec<u8> {
    let wf = &FACES[(f.wface as usize).min(FACES.len() - 1)];
    let h = if f.height == 0 { 16 } else { f.height };
    let mut b = Vec::with_capacity(92);
    for v in [h, f.width, 0, 0, f.weight as i32] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&[f.italic as u8, f.underline as u8, 0, wf.charset, 0, 0, 0, wf.family]);
    if wide {
        let mut name = [0u16; 32];
        for (i, &c) in wf.name.iter().take(31).enumerate() {
            name[i] = c as u16;
        }
        for c in name {
            b.extend_from_slice(&c.to_le_bytes());
        }
    } else {
        let mut name = [0u8; 32];
        let n = wf.name.len().min(31);
        name[..n].copy_from_slice(&wf.name[..n]);
        b.extend_from_slice(&name);
    }
    b
}

/// GetObject: LOGPEN, LOGBRUSH, LOGFONT, BITMAP (DIBSECTION for a DIB
/// section when there is room), or a palette's entry count.
fn get_object(o: u32, cb: i32, p: *mut u8, wide: bool) -> i32 {
    let o = h16(o);
    let mut b = Vec::new();
    match obj(o) {
        Some(Obj::Brush { color, null, pat, src }) => {
            let style: u32 = match pat {
                _ if null => 1,
                None => 0,
                Some(Pat::Hatch(_)) => 2,
                Some(_) if bitmap::bitmap_of(src).is_some() => 3,
                Some(_) => 6,
            };
            for v in [style, color, if style <= 1 { 0 } else { src as u32 }] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        Some(Obj::Pen { style, width, color }) => {
            for v in [style as u32, width as u32, 0, color] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        // BITMAP: type, width, height, bytes per row, planes, bits per
        // pel, bits.
        Some(Obj::Bitmap { w, h, bpp, .. }) => {
            let sec = section(o);
            let (sbpp, bits) = sec.map_or((bpp, 0), |s| (s.bpp, s.bits));
            let row = if sec.is_some() { (w * sbpp as i32 + 31) / 32 * 4 } else { (w * bpp as i32 + 15) / 16 * 2 };
            for v in [0, w, h, row] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(&1u16.to_le_bytes());
            b.extend_from_slice(&sbpp.to_le_bytes());
            b.extend_from_slice(&bits.to_le_bytes());
            // DIBSECTION: the header, dsBitfields, dshSection, dsOffset.
            if let Some(s) = sec.filter(|_| cb >= 84) {
                let mut hdr = s.bmi[..40.min(s.bmi.len())].to_vec();
                hdr.resize(40, 0);
                hdr[0..4].copy_from_slice(&40u32.to_le_bytes());
                b.extend_from_slice(&hdr);
                let m = masks(s.bmi.as_ptr(), s.bpp, l32(s.bmi.as_ptr(), 16));
                for v in [m[0], m[1], m[2], 0, 0] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        Some(Obj::Palette(p)) => b.extend_from_slice(&(p.len() as u16).to_le_bytes()),
        Some(Obj::Font(f)) => b = logfont(&f, wide),
        _ => return 0,
    }
    if p.is_null() {
        return b.len() as i32;
    }
    let n = b.len().min(cb.max(0) as usize);
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, n) };
    n as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetObjectA(o: u32, cb: i32, p: *mut u8) -> i32 {
    get_object(o, cb, p, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetObjectW(o: u32, cb: i32, p: *mut u8) -> i32 {
    get_object(o, cb, p, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UnrealizeObject(_o: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDCBrushColor(h: u32, c: u32) -> u32 {
    let _ = h;
    c
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDCPenColor(_h: u32, c: u32) -> u32 {
    c
}

// ---- Text ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn TextOutA(h: u32, x: i32, y: i32, s: u32, n: i32) -> i32 {
    text_out(h, x, y, &text_arg(s, n.max(0), false))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TextOutW(h: u32, x: i32, y: i32, s: u32, n: i32) -> i32 {
    text_out(h, x, y, &text_arg(s, n.max(0), true))
}

fn text_out(h: u32, x: i32, y: i32, t: &[u8]) -> i32 {
    let hdc = h16(h);
    if dc(hdc).is_none() {
        return 0;
    }
    drawing(hdc, hdc, || gdi::text_out(hdc, x, y, t));
    1
}

/// ExtTextOut(hdc, x, y, options, rect, text, n, dx): ETO_OPAQUE 2,
/// ETO_CLIPPED 4.
fn ext_text_out(h: u32, x: i32, y: i32, opts: u32, r: u32, t: &[u8]) -> i32 {
    let hdc = h16(h);
    if dc(hdc).is_none() {
        return 0;
    }
    drawing(hdc, hdc, || {
        let rect = read_rect(r);
        if let Some(rc) = rect {
            if opts & 2 != 0 {
                gdi::fill(hdc, rc, gdi::bk_color(hdc));
            }
            if opts & 4 != 0 {
                gdi::clip(hdc, rc);
            }
        }
        if !t.is_empty() {
            gdi::text_out(hdc, x, y, t);
        }
        if rect.is_some() && opts & 4 != 0 {
            gdi::unclip(hdc);
        }
    });
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExtTextOutA(h: u32, x: i32, y: i32, o: u32, r: u32, s: u32, n: u32, _dx: u32) -> i32 {
    ext_text_out(h, x, y, o, r, &text_arg(s, n as i32, false))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExtTextOutW(h: u32, x: i32, y: i32, o: u32, r: u32, s: u32, n: u32, _dx: u32) -> i32 {
    // ETO_GLYPH_INDEX (0x10): glyph indexes are not characters here.
    ext_text_out(h, x, y, o, r, &if o & 0x10 != 0 { Vec::new() } else { text_arg(s, n as i32, true) })
}

fn extent(h: u32, t: &[u8], p: u32) -> i32 {
    let hdc = h16(h);
    if dc(hdc).is_none() {
        return 0;
    }
    write_point(p, (gdi::text_width(hdc, t), gdi::line_height(hdc)));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextExtentPoint32A(h: u32, s: u32, n: i32, p: u32) -> i32 {
    extent(h, &text_arg(s, n.max(0), false), p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextExtentPoint32W(h: u32, s: u32, n: i32, p: u32) -> i32 {
    extent(h, &text_arg(s, n.max(0), true), p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextExtentPointA(h: u32, s: u32, n: i32, p: u32) -> i32 {
    extent(h, &text_arg(s, n.max(0), false), p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextExtentPointW(h: u32, s: u32, n: i32, p: u32) -> i32 {
    extent(h, &text_arg(s, n.max(0), true), p)
}

/// GetTextExtentExPoint(hdc, text, n, max, fit, dx, size): how many
/// characters fit in `max`, and each one's end.
fn extent_ex(h: u32, t: &[u8], max: i32, fit: u32, dx: u32, size: u32) -> i32 {
    let hdc = h16(h);
    if dc(hdc).is_none() {
        return 0;
    }
    let w = gdi::char_widths(hdc);
    let mut x = 0;
    let mut k = 0;
    for (i, &c) in t.iter().enumerate() {
        x += w[c as usize] as i32;
        if dx != 0 {
            crate::wr32(dx, 4 * i as u32, x as u32);
        }
        if x <= max {
            k = i + 1;
        }
    }
    if fit != 0 {
        crate::wr32(fit, 0, k as u32);
    }
    write_point(size, (x, gdi::line_height(hdc)));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextExtentExPointA(h: u32, s: u32, n: i32, max: i32, fit: u32, dx: u32, size: u32) -> i32 {
    extent_ex(h, &text_arg(s, n.max(0), false), max, fit, dx, size)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextExtentExPointW(h: u32, s: u32, n: i32, max: i32, fit: u32, dx: u32, size: u32) -> i32 {
    extent_ex(h, &text_arg(s, n.max(0), true), max, fit, dx, size)
}

/// TEXTMETRIC (A: 56 bytes, W: 60) from the DC's font.
fn text_metrics(h: u32, p: u32, wide: bool) -> i32 {
    let hdc = h16(h);
    if dc(hdc).is_none() || p == 0 {
        return 0;
    }
    let t = gdi::text_metrics(hdc);
    let i16at = |o: usize| i16::from_le_bytes([t[o], t[o + 1]]) as i32;
    let mut b = Vec::with_capacity(60);
    // Height, Ascent, Descent, InternalLeading, ExternalLeading,
    // AveCharWidth, MaxCharWidth, Weight, Overhang, DigitizedAspectX/Y.
    for v in [i16at(0), i16at(2), i16at(4), i16at(6), i16at(8), i16at(10), i16at(12), i16at(14), 0, 96, 96] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    // First, last, default and break characters.
    if wide {
        for c in [0x20u16, 0xfffc, 0x1f, 0x20] {
            b.extend_from_slice(&c.to_le_bytes());
        }
    } else {
        b.extend_from_slice(&[0x20, 0xff, 0x1f, 0x20]);
    }
    // Italic, Underlined, StruckOut, PitchAndFamily, CharSet.
    b.extend_from_slice(&[t[16], t[17], t[18], t[23], t[24]]);
    b.resize(if wide { 60 } else { 56 }, 0);
    unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p as *mut u8, b.len()) };
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextMetricsA(h: u32, p: u32) -> i32 {
    text_metrics(h, p, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextMetricsW(h: u32, p: u32) -> i32 {
    text_metrics(h, p, true)
}

fn text_face(h: u32, cb: i32, buf: u32, wide: bool) -> i32 {
    let face: &[u8] = match dc(h16(h)).and_then(|d| obj(d.s.font)) {
        Some(Obj::Font(f)) => face_name(&f),
        _ => b"System",
    };
    if buf == 0 {
        return face.len() as i32 + 1;
    }
    if wide { crate::copy_out_w(&crate::to_wide(face), buf as *mut u16, cb) as i32 } else { crate::copy_out(face, buf as *mut u8, cb) as i32 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextFaceA(h: u32, cb: i32, buf: u32) -> i32 {
    text_face(h, cb, buf, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextFaceW(h: u32, cb: i32, buf: u32) -> i32 {
    text_face(h, cb, buf, true)
}

fn char_width(h: u32, first: u32, last: u32, p: u32) -> i32 {
    let hdc = h16(h);
    if p == 0 || dc(hdc).is_none() || last < first {
        return 0;
    }
    let w = gdi::char_widths(hdc);
    let ave = gdi::ave_width(hdc);
    for (i, c) in (first..=last).enumerate() {
        let v = if c < 256 { w[c as usize] as i32 } else { ave };
        crate::wr32(p, 4 * i as u32, v as u32);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCharWidthA(h: u32, f: u32, l: u32, p: u32) -> i32 {
    char_width(h, f, l, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCharWidthW(h: u32, f: u32, l: u32, p: u32) -> i32 {
    char_width(h, f, l, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCharWidth32A(h: u32, f: u32, l: u32, p: u32) -> i32 {
    char_width(h, f, l, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCharWidth32W(h: u32, f: u32, l: u32, p: u32) -> i32 {
    char_width(h, f, l, p)
}

/// GetCharABCWidths: A and C 0, B the width.
fn abc(h: u32, first: u32, last: u32, p: u32) -> i32 {
    let hdc = h16(h);
    if p == 0 || dc(hdc).is_none() || last < first {
        return 0;
    }
    let w = gdi::char_widths(hdc);
    for (i, c) in (first..=last).enumerate() {
        let o = p + 12 * i as u32;
        crate::wr32(o, 0, 0);
        crate::wr32(o, 4, if c < 256 { w[c as usize] as u32 } else { gdi::ave_width(hdc) as u32 });
        crate::wr32(o, 8, 0);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCharABCWidthsA(h: u32, f: u32, l: u32, p: u32) -> i32 {
    abc(h, f, l, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetCharABCWidthsW(h: u32, f: u32, l: u32, p: u32) -> i32 {
    abc(h, f, l, p)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextCharset(h: u32) -> i32 {
    match dc(h16(h)).and_then(|d| obj(d.s.font)) {
        Some(Obj::Font(f)) => FACES[(f.wface as usize).min(FACES.len() - 1)].charset as i32,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTextCharsetInfo(h: u32, p: u32, _f: u32) -> i32 {
    if p != 0 {
        unsafe { core::ptr::write_bytes(p as *mut u8, 0, 24) };
    }
    GetTextCharset(h)
}

/// GetOutlineTextMetrics: not available (no TrueType here).
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetOutlineTextMetricsA(_h: u32, _n: u32, _p: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetOutlineTextMetricsW(_h: u32, _n: u32, _p: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKerningPairsA(_h: u32, _n: u32, _p: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetKerningPairsW(_h: u32, _n: u32, _p: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetFontData(_h: u32, _t: u32, _o: u32, _b: u32, _n: u32) -> u32 {
    0xffff_ffff
}

/// GetGlyphOutline: GDI_ERROR.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetGlyphOutlineA(_h: u32, _c: u32, _f: u32, _m: u32, _n: u32, _b: u32, _t: u32) -> u32 {
    0xffff_ffff
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetGlyphOutlineW(_h: u32, _c: u32, _f: u32, _m: u32, _n: u32, _b: u32, _t: u32) -> u32 {
    0xffff_ffff
}

/// The font faces for EnumFontFamilies(Ex) and EnumFonts: the callback
/// gets (ENUMLOGFONT(EX), NEW)TEXTMETRIC(EX), FontType, lParam) for each
/// face (or the one named), until it returns 0.
fn enum_fonts(h: u32, name: &[u8], f: u32, lp: u32, wide: bool) -> i32 {
    let _ = h16(h);
    // One font per face (no face named), or each size of a raster face
    // and each style of a TrueType one (WIN16's EnumFonts).
    let mut list: Vec<(usize, i32, u32, bool)> = Vec::new();
    for (i, wf) in FACES.iter().enumerate() {
        if name.is_empty() {
            let height = if wf.tt { -16 } else { wf.sizes.first().copied().unwrap_or(16) };
            list.push((i, height, if i == 0 { 700 } else { 400 }, false));
        } else if wf.name.eq_ignore_ascii_case(name) {
            if wf.tt {
                for (weight, italic) in [(400, false), (700, false), (400, true), (700, true)] {
                    list.push((i, -16, weight, italic));
                }
            } else {
                for &hh in wf.sizes {
                    list.push((i, hh, if i == 0 { 700 } else { 400 }, false));
                }
            }
        }
    }
    let mut r = 1;
    for (i, height, weight, italic) in list {
        let wf = &FACES[i];
        let ft = gdi::face_font(i as u8, height, weight as u16, italic);
        // NEWTEXTMETRICEX: the font's metrics in a scratch DC.
        let mem = bitmap::create_memory_dc();
        let fo = gdi::new_obj(Obj::Font(ft));
        gdi::select_font(mem, fo);
        let mut tm = [0u8; 96];
        text_metrics(mem as u32, tm.as_mut_ptr() as u32, wide);
        gdi::release_dc(mem);
        gdi::delete_object(fo);
        // NEWTEXTMETRIC's TrueType fields after TEXTMETRIC (56 bytes, 60
        // for W): ntmFlags (NTM_ITALIC 1, NTM_BOLD 0x20, NTM_REGULAR 0x40),
        // ntmSizeEM, ntmCellHeight, ntmAvgWidth.
        if wf.tt {
            let o = if wide { 60 } else { 56 };
            let rd = |k: usize| i32::from_le_bytes(tm[k..k + 4].try_into().unwrap());
            let flags = if italic { 1 } else { 0 } | if weight >= 700 { 0x20 } else { 0 } | if !italic && weight < 700 { 0x40 } else { 0 };
            let (height, internal, ave) = (rd(0), rd(12), rd(20));
            for (i, v) in [flags, 2048, (height - internal) as u32, ave as u32].iter().enumerate() {
                tm[o + 4 * i..o + 4 * i + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        // ENUMLOGFONTEX: LOGFONT (the cell height, the average width),
        // full name (64), style (32), script (32).
        let mut lf = logfont(&ft, wide);
        let rdtm = |o: usize| i32::from_le_bytes(tm[o..o + 4].try_into().unwrap());
        lf[0..4].copy_from_slice(&rdtm(0).to_le_bytes());
        lf[4..8].copy_from_slice(&rdtm(20).to_le_bytes());
        let style: &[u8] = match (weight >= 700, italic) {
            (false, false) => b"Regular",
            (true, false) => b"Bold",
            (false, true) => b"Italic",
            (true, true) => b"Bold Italic",
        };
        let mut full = wf.name.to_vec();
        if wf.tt && style != b"Regular" {
            full.push(b' ');
            full.extend_from_slice(style);
        }
        let pad = |b: &mut Vec<u8>, s: &[u8], n: usize| {
            if wide {
                for k in 0..n {
                    let c = if k + 1 < n { s.get(k).copied().unwrap_or(0) } else { 0 } as u16;
                    b.extend_from_slice(&c.to_le_bytes());
                }
            } else {
                for k in 0..n {
                    b.push(if k + 1 < n { s.get(k).copied().unwrap_or(0) } else { 0 });
                }
            }
        };
        pad(&mut lf, &full, 64);
        pad(&mut lf, style, 32);
        pad(&mut lf, b"Western", 32);
        // RASTER_FONTTYPE 1, TRUETYPE_FONTTYPE 4.
        let ty = if wf.tt { 4 } else { 1 };
        r = crate::wnd::call_enum_font(f, lf.as_ptr() as u32, tm.as_ptr() as u32, ty, lp);
        if r == 0 {
            break;
        }
    }
    r as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumFontFamiliesA(h: u32, name: u32, f: u32, lp: u32) -> i32 {
    enum_fonts(h, crate::far_str(name).unwrap_or(b""), f, lp, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumFontFamiliesW(h: u32, name: u32, f: u32, lp: u32) -> i32 {
    enum_fonts(h, &wide_str(name).unwrap_or_default(), f, lp, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumFontsA(h: u32, name: u32, f: u32, lp: u32) -> i32 {
    enum_fonts(h, crate::far_str(name).unwrap_or(b""), f, lp, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumFontsW(h: u32, name: u32, f: u32, lp: u32) -> i32 {
    enum_fonts(h, &wide_str(name).unwrap_or_default(), f, lp, true)
}

/// EnumFontFamiliesEx(hdc, LOGFONT, proc, lParam, flags): the face named
/// in the LOGFONT (all faces for an empty name).
#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumFontFamiliesExA(h: u32, lf: u32, f: u32, lp: u32, _fl: u32) -> i32 {
    let name = if lf == 0 { Vec::new() } else { unsafe { crate::cstr((lf + 28) as *const u8) }.to_vec() };
    enum_fonts(h, &name, f, lp, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EnumFontFamiliesExW(h: u32, lf: u32, f: u32, lp: u32, _fl: u32) -> i32 {
    let name = if lf == 0 { Vec::new() } else { crate::narrow(unsafe { crate::wstr((lf + 28) as *const u16) }) };
    enum_fonts(h, &name, f, lp, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AddFontResourceA(_f: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AddFontResourceW(_f: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AddFontResourceExA(_f: u32, _fl: u32, _r: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AddFontResourceExW(_f: u32, _fl: u32, _r: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RemoveFontResourceA(_f: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RemoveFontResourceW(_f: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AddFontMemResourceEx(_p: u32, _n: u32, _r: u32, count: u32) -> u32 {
    if count != 0 {
        crate::wr32(count, 0, 0);
    }
    0
}

// ---- The device ----

/// GetDeviceCaps: PM's display (true colour, or 256 colours with a
/// palette), 96 dots per inch.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDeviceCaps(h: u32, i: i32) -> i32 {
    if let Some(v) = crate::print::caps(h16(h), i as u16) {
        return v as i32;
    }
    let q = |v| unsafe { WinQuerySysValue(HWND_DESKTOP, v) };
    let bits = display_bits();
    let pal = bits == 8;
    match i {
        0 => 0x400,      // DRIVERVERSION
        2 => 1,          // TECHNOLOGY: DT_RASDISPLAY
        4 => q(SV_CXSCREEN) * 254 / 960,
        6 => q(SV_CYSCREEN) * 254 / 960,
        8 => q(SV_CXSCREEN),
        10 => q(SV_CYSCREEN),
        12 => if pal { 8 } else { 32 }, // BITSPIXEL
        14 => 1,         // PLANES
        16 => 0,         // NUMBRUSHES
        18 => 0,         // NUMPENS
        20 => 0,         // NUMMARKERS
        22 => 0,         // NUMFONTS
        24 if pal => 20, // NUMCOLORS
        24 => -1,
        26 => 0,         // PDEVICESIZE
        28 => 0xff,      // CURVECAPS
        30 => 0xfe,      // LINECAPS
        32 => 0xff,      // POLYGONALCAPS
        34 => 0x7807,    // TEXTCAPS
        36 => 1,         // CLIPCAPS
        // RASTERCAPS: BITBLT, BITMAP64, GDI20_OUTPUT, DI_BITMAP,
        // DIBTODEV, STRETCHBLT, STRETCHDIB (and PALETTE).
        38 => 0x0001 | 0x0008 | 0x0010 | 0x0080 | 0x0200 | 0x0800 | 0x2000 | if pal { 0x0100 } else { 0 },
        40 | 42 => 36,   // ASPECTX, ASPECTY
        44 => 51,        // ASPECTXY
        88 | 90 => 96,   // LOGPIXELSX/Y
        104 if pal => 256, // SIZEPALETTE
        106 if pal => 20,  // NUMRESERVED
        108 => if pal { 18 } else { 24 }, // COLORRES
        116 => 60,       // VREFRESH
        117 => q(SV_CYSCREEN),
        118 => q(SV_CXSCREEN),
        120 => 0,        // SHADEBLENDCAPS
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateCompatibleDC(_h: u32) -> u32 {
    bitmap::create_memory_dc() as u32
}

/// CreateDC and CreateIC: the display (a memory DC: the screen itself
/// is PM's); printers are not reached this way yet.
/// CreateDC / CreateIC: a printer's DC (print.rs), or a memory DC for
/// the display.
fn create_dc(d: u32, dev: u32, o: u32, dm: u32, wide: bool, output: bool) -> u32 {
    let s = |p: u32| -> Vec<u8> { if wide { crate::wide_str(p).unwrap_or_default() } else { crate::far_str(p).unwrap_or(b"").to_vec() } };
    // (A DEVMODEW's fields after the device name are DEVMODEA's 32 bytes
    // further on.)
    let dm = if wide && dm != 0 { dm + 32 } else { dm };
    match crate::print::create(&s(d), &s(dev), &s(o), dm, output) {
        Some(h) => h as u32,
        None => bitmap::create_memory_dc() as u32,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDCA(d: u32, dev: u32, o: u32, i: u32) -> u32 {
    create_dc(d, dev, o, i, false, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDCW(d: u32, dev: u32, o: u32, i: u32) -> u32 {
    create_dc(d, dev, o, i, true, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateICA(d: u32, dev: u32, o: u32, i: u32) -> u32 {
    create_dc(d, dev, o, i, false, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateICW(d: u32, dev: u32, o: u32, i: u32) -> u32 {
    create_dc(d, dev, o, i, true, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DeleteDC(h: u32) -> i32 {
    let hdc = h16(h);
    if crate::print::is_printer(hdc) {
        gdi::release_dc(hdc);
        crate::print::close(hdc);
        return 1;
    }
    if dc(hdc).is_some_and(|d| d.kind != DcKind::Memory) {
        return 0;
    }
    if let Some(b) = dc_section(hdc) {
        download(b);
    }
    gdi::release_dc(hdc) as i32
}

// ---- Clipping and regions ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn IntersectClipRect(h: u32, l: i32, t: i32, r: i32, b: i32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let rc = gdi::pm_rect(d, [l, t, r, b]);
    d.clipped = true;
    unsafe { GpiIntersectClipRectangle(d.hps, &rc) as i32 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExcludeClipRect(h: u32, l: i32, t: i32, r: i32, b: i32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let rc = gdi::pm_rect(d, [l, t, r, b]);
    d.clipped = true;
    unsafe { GpiExcludeClipRectangle(d.hps, &rc) as i32 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SelectClipRgn(h: u32, r: u32) -> i32 {
    region::select_clip(h16(h), h16(r)) as i32
}

/// ExtSelectClipRgn(hdc, hrgn, mode): RGN_COPY (5) and NULL; the other
/// modes combine with the region's bounding rectangle.
#[unsafe(no_mangle)]
pub extern "stdcall" fn ExtSelectClipRgn(h: u32, r: u32, mode: i32) -> i32 {
    if mode == 5 || r == 0 {
        return SelectClipRgn(h, r);
    }
    let Some(rs) = region::rects(h16(r)) else { return 0 };
    let b = region::bounds(&rs);
    match mode {
        1 => IntersectClipRect(h, b[0], b[1], b[2], b[3]),
        4 => ExcludeClipRect(h, b[0], b[1], b[2], b[3]),
        _ => SelectClipRgn(h, r),
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClipBox(h: u32, r: u32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let mut rc = RectL::default();
    let kind = unsafe { GpiQueryClipBox(d.hps, &mut rc) };
    let (l, t) = gdi::to_log(d, rc.l, d.height - rc.t - 1);
    let (rr, b) = gdi::to_log(d, rc.r + 1, d.height - rc.b);
    write_rect(r, [l.min(rr), t.min(b), l.max(rr), t.max(b)]);
    if (1..=3).contains(&kind) { kind } else { 2 }
}

/// GetClipRgn: 0, no clip region kept as a region.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetClipRgn(_h: u32, _r: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OffsetClipRgn(h: u32, x: i32, y: i32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let (x0, y0) = gdi::to_dev(d, 0, 0);
    let (x1, y1) = gdi::to_dev(d, x, y);
    let p = PointL { x: x1 - x0, y: -(y1 - y0) };
    unsafe { GpiOffsetClipRegion(d.hps, &p) as i32 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PtVisible(h: u32, x: i32, y: i32) -> i32 {
    let Some(d) = dc(h16(h)) else { return 0 };
    let mut rc = RectL::default();
    unsafe { GpiQueryClipBox(d.hps, &mut rc) };
    let p = gdi::pm_pt(d, x, y);
    (p.x >= rc.l && p.x <= rc.r && p.y >= rc.b && p.y <= rc.t) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RectVisible(h: u32, r: u32) -> i32 {
    let Some(q) = read_rect(r) else { return 0 };
    let Some(d) = dc(h16(h)) else { return 0 };
    let mut rc = RectL::default();
    unsafe { GpiQueryClipBox(d.hps, &mut rc) };
    let p = gdi::pm_rect(d, q);
    (p.l <= rc.r && p.r > rc.l && p.b <= rc.t && p.t > rc.b) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateRectRgn(l: i32, t: i32, r: i32, b: i32) -> u32 {
    region::new(region::rect(l, t, r, b)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateRectRgnIndirect(p: u32) -> u32 {
    let r = read_rect(p).unwrap_or([0; 4]);
    CreateRectRgn(r[0], r[1], r[2], r[3])
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateEllipticRgn(l: i32, t: i32, r: i32, b: i32) -> u32 {
    region::new(region::ellipse(l, t, r, b)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateEllipticRgnIndirect(p: u32) -> u32 {
    let r = read_rect(p).unwrap_or([0; 4]);
    CreateEllipticRgn(r[0], r[1], r[2], r[3])
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreatePolygonRgn(p: u32, n: i32, mode: i32) -> u32 {
    let pts = read_pts(p, n);
    if pts.len() < 3 {
        return region::new(Vec::new()) as u32;
    }
    region::new(region::polygons(&[pts], mode as u16)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreatePolyPolygonRgn(p: u32, c: u32, n: i32, mode: i32) -> u32 {
    region::new(region::polygons(&poly_counts(p, c, n), mode as u16)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateRoundRectRgn(l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> u32 {
    let (w, h) = (w.min(r - l), h.min(b - t));
    let mut v = region::combine(&region::rect(l + w / 2, t, r - w / 2, b), &region::rect(l, t + h / 2, r, b - h / 2), 2);
    for (x, y) in [(l, t), (r - w, t), (l, b - h), (r - w, b - h)] {
        v = region::combine(&v, &region::ellipse(x, y, x + w, y + h), 2);
    }
    region::new(v) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CombineRgn(d: u32, s1: u32, s2: u32, m: i32) -> i32 {
    let Some(r1) = region::region(h16(s1)) else { return 0 };
    let r = if m == 5 {
        r1
    } else {
        let Some(r2) = region::region(h16(s2)) else { return 0 };
        region::combine(&r1, &r2, m as u16)
    };
    region::set(h16(d), r) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EqualRgn(a: u32, b: u32) -> i32 {
    match (region::region(h16(a)), region::region(h16(b))) {
        (Some(p), Some(q)) => region::combine(&p, &q, 3).is_empty() as i32,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn OffsetRgn(h: u32, dx: i32, dy: i32) -> i32 {
    let Some(r) = region::region(h16(h)) else { return 0 };
    region::set(h16(h), r.iter().map(|q| [q[0] + dx, q[1] + dy, q[2] + dx, q[3] + dy]).collect()) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetRgnBox(h: u32, p: u32) -> i32 {
    let Some(r) = region::region(h16(h)) else { return 0 };
    write_rect(p, region::bounds(&r));
    region::kind(&r) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PtInRegion(h: u32, x: i32, y: i32) -> i32 {
    region::region(h16(h)).is_some_and(|r| r.iter().any(|q| x >= q[0] && x < q[2] && y >= q[1] && y < q[3])) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RectInRegion(h: u32, p: u32) -> i32 {
    let q = read_rect(p).unwrap_or([0; 4]);
    region::region(h16(h)).is_some_and(|r| !region::combine(&r, &region::rect(q[0], q[1], q[2], q[3]), 1).is_empty()) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetRectRgn(h: u32, l: i32, t: i32, r: i32, b: i32) -> i32 {
    region::set(h16(h), region::rect(l, t, r, b));
    1
}

/// GetRegionData(hrgn, size, RGNDATA): RGNDATAHEADER (dwSize, iType,
/// nCount, nRgnSize, rcBound) and the rectangles.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetRegionData(h: u32, size: u32, p: u32) -> u32 {
    let Some(r) = region::region(h16(h)) else { return 0 };
    let need = 32 + 16 * r.len() as u32;
    if p == 0 || size < need {
        return need;
    }
    for (o, v) in [(0, 32), (4, 1), (8, r.len() as u32), (12, 16 * r.len() as u32)] {
        crate::wr32(p, o, v);
    }
    write_rect(p + 16, region::bounds(&r));
    for (i, q) in r.iter().enumerate() {
        write_rect(p + 32 + 16 * i as u32, *q);
    }
    need
}

/// ExtCreateRegion(xform, size, RGNDATA): the rectangles (no transform).
#[unsafe(no_mangle)]
pub extern "stdcall" fn ExtCreateRegion(_x: u32, _size: u32, p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let n = crate::rd32(p, 8);
    let v = (0..n.min(65536)).filter_map(|i| read_rect(p + 32 + 16 * i)).collect();
    region::new(region::combine(&v, &Vec::new(), 2)) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FillRgn(h: u32, r: u32, br: u32) -> i32 {
    let hdc = h16(h);
    let Some(rs) = region::region(h16(r)) else { return 0 };
    drawing(hdc, hdc, || {
        for q in rs {
            gdi::fill_rect(hdc, q, h16(br));
        }
    });
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PaintRgn(h: u32, r: u32) -> i32 {
    let br = gdi::brush_of(h16(h));
    FillRgn(h, r, br as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn InvertRgn(h: u32, r: u32) -> i32 {
    let hdc = h16(h);
    let Some(rs) = region::region(h16(r)) else { return 0 };
    drawing(hdc, hdc, || {
        for q in rs {
            gdi::invert(hdc, q);
        }
    });
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FrameRgn(h: u32, r: u32, br: u32, w: i32, hh: i32) -> i32 {
    let hdc = h16(h);
    let Some(r) = region::region(h16(r)) else { return 0 };
    let shift = |dx: i32, dy: i32| -> region::Rects { r.iter().map(|q| [q[0] + dx, q[1] + dy, q[2] + dx, q[3] + dy]).collect() };
    let mut inner = r.clone();
    for (dx, dy) in [(w, 0), (-w, 0), (0, hh), (0, -hh)] {
        inner = region::combine(&inner, &shift(dx, dy), 1);
    }
    drawing(hdc, hdc, || {
        for q in region::combine(&r, &inner, 4) {
            gdi::fill_rect(hdc, q, h16(br));
        }
    });
    1
}

// ---- Bitmaps ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateCompatibleBitmap(h: u32, w: i32, hh: i32) -> u32 {
    let mono = dc(h16(h)).is_some_and(|d| d.kind == DcKind::Memory) && bitmap::is_mono(h16(h));
    new_bitmap(w.max(1), hh.max(1), if mono { 1 } else { display_bits() }) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDiscardableBitmap(h: u32, w: i32, hh: i32) -> u32 {
    CreateCompatibleBitmap(h, w, hh)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateBitmap(w: i32, h: i32, planes: u32, bpp: u32, bits: *const u8) -> u32 {
    bitmap::create_bitmap(w, h, (planes * bpp) as u16, bits)
}

/// BITMAP: bmType, bmWidth, bmHeight, bmWidthBytes, bmPlanes,
/// bmBitsPixel, bmBits.
#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateBitmapIndirect(p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let bpp = crate::rd16(p, 16) * crate::rd16(p, 18);
    bitmap::create_bitmap(crate::rd32(p, 4) as i32, crate::rd32(p, 8) as i32, bpp, crate::rd32(p, 20) as *const u8)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetBitmapBits(h: u32, n: u32, p: *const u8) -> i32 {
    bitmap::set_ddb_bits(h16(h), p, n as usize) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetBitmapBits(h: u32, n: i32, p: *mut u8) -> i32 {
    bitmap::get_ddb_bits(h16(h), p, n.max(0) as usize) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetBitmapDimensionEx(_h: u32, p: u32) -> i32 {
    write_point(p, (0, 0));
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetBitmapDimensionEx(_h: u32, _x: i32, _y: i32, p: u32) -> i32 {
    write_point(p, (0, 0));
    1
}

/// SetDIBits(hdc, hbm, start, scans, bits, bmi, usage).
#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDIBits(hdc: u32, hbm: u32, start: u32, scans: u32, bits: *const u8, bmi: *const u8, usage: u32) -> i32 {
    if bits.is_null() || bmi.is_null() {
        return 0;
    }
    let b = h16(hbm);
    let (hdr, data) = normalize_dib(h16(hdc), bmi, bits, usage as u16, scans as i32);
    let r = bitmap::with_ps(b, |hps, _| unsafe { GpiSetBitmapBits(hps, start as i32, scans as i32, data.as_ptr(), hdr.as_ptr()) }).unwrap_or(0).max(0);
    if section(b).is_some() {
        download(b);
    }
    r
}

/// GetDIBits(hdc, hbm, start, scans, bits, bmi, usage): no bits, the
/// header is filled in.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDIBits(_hdc: u32, hbm: u32, start: u32, scans: u32, bits: *mut u8, bmi: *mut u8, _usage: u32) -> i32 {
    let b = h16(hbm);
    let Some((_, w, bh, bpp)) = bitmap::bitmap_of(b) else { return 0 };
    if bmi.is_null() {
        return 0;
    }
    if section(b).is_some() {
        upload(b);
    }
    let cb = l32(bmi, 0) as usize;
    if cb >= 16 && (bits.is_null() || w16(bmi, 14) == 0) {
        let sbpp = section(b).map_or(if bpp == 1 { 1 } else { 24 }, |s| s.bpp);
        unsafe {
            (bmi.add(4) as *mut u32).write_unaligned(w as u32);
            (bmi.add(8) as *mut u32).write_unaligned(bh as u32);
            (bmi.add(12) as *mut u16).write_unaligned(1);
            if w16(bmi, 14) == 0 {
                (bmi.add(14) as *mut u16).write_unaligned(sbpp);
            }
            if cb >= 24 {
                let stride = (w as usize * w16(bmi, 14) as usize).div_ceil(32) * 4;
                (bmi.add(16) as *mut u32).write_unaligned(0);
                (bmi.add(20) as *mut u32).write_unaligned((stride * bh as usize) as u32);
            }
        }
        if bits.is_null() {
            return bh;
        }
    }
    read_dib(b, bmi, bits, start as i32, scans as i32)
}

/// CreateDIBitmap(hdc, header, init, bits, bmi, usage).
#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateDIBitmap(hdc: u32, hdr: *const u8, init: u32, bits: *const u8, bmi: *const u8, usage: u32) -> u32 {
    if hdr.is_null() {
        return 0;
    }
    let (w, h, bpp, _) = dib_fields(hdr);
    let mono = bpp == 1 && dc(h16(hdc)).is_none_or(|d| d.kind == DcKind::Memory && bitmap::is_mono(h16(hdc)));
    let b = new_bitmap(w.max(1), h.unsigned_abs().max(1) as i32, if mono { 1 } else { display_bits() });
    if b != 0 && init & CBM_INIT != 0 && !bits.is_null() && !bmi.is_null() {
        SetDIBits(hdc, b as u32, 0, h.unsigned_abs(), bits, bmi, usage);
    }
    b as u32
}

/// SetDIBitsToDevice(hdc, x, y, w, h, sx, sy, start, scans, bits, bmi,
/// usage): the bits hold rows start.. of the DIB (counted from the
/// bottom).
#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDIBitsToDevice(h: u32, x: i32, y: i32, w: u32, hh: u32, sx: i32, sy: i32, start: u32, scans: u32, bits: *const u8, bmi: *const u8, usage: u32) -> i32 {
    if bmi.is_null() || bits.is_null() {
        return 0;
    }
    let hdc = h16(h);
    let (_, dib_h, _, _) = dib_fields(bmi);
    let (w, hh) = (w as i32, hh as i32);
    // In the normalized (bottom-up) rows: the source's top row.
    let (scans, start) = (scans as i32, start as i32);
    let sy_bu = dib_h.abs() - sy - hh - start;
    let (hdr, data) = normalize_dib(hdc, bmi, bits, usage as u16, scans);
    drawing(hdc, hdc, || {
        let Some(d) = dc(hdc) else { return 0 };
        let (dh, hps) = (d.height, d.hps);
        let (x, y) = gdi::to_dev(d, x, y);
        bitmap::blit_colors(hdc);
        let pts = [
            PointL { x, y: dh - y - hh },
            PointL { x: x + w - 1, y: dh - y - 1 },
            PointL { x: sx, y: sy_bu },
            PointL { x: sx + w, y: sy_bu + hh },
        ];
        unsafe { GpiDrawBits(hps, data.as_ptr(), hdr.as_ptr(), 4, pts.as_ptr(), 0xcc, 2) };
        scans
    })
}

/// StretchDIBits(hdc, x, y, w, h, sx, sy, sw, sh, bits, bmi, usage, rop).
#[unsafe(no_mangle)]
pub extern "stdcall" fn StretchDIBits(h: u32, x: i32, y: i32, w: i32, hh: i32, sx: i32, sy: i32, sw: i32, sh: i32, bits: *const u8, bmi: *const u8, usage: u32, rop: u32) -> i32 {
    if bmi.is_null() || bits.is_null() {
        return 0;
    }
    let hdc = h16(h);
    let (_, dib_h, _, _) = dib_fields(bmi);
    let (hdr, data) = normalize_dib(hdc, bmi, bits, usage as u16, dib_h.abs());
    drawing(hdc, hdc, || {
        let Some(d) = dc(hdc) else { return 0 };
        let (dh, hps) = (d.height, d.hps);
        // A negative width or height mirrors the picture.
        let r = gdi::dev_rect(d, [x, y, x + w, y + hh]);
        let (dx, dy, dw, dhh) = (r[0], r[1], r[2] - r[0], r[3] - r[1]);
        bitmap::blit_colors(hdc);
        let flip_x = (w < 0) != (sw < 0);
        let flip_y = (hh < 0) != (sh < 0);
        let (sx0, sw) = if sw < 0 { (sx + sw, -sw) } else { (sx, sw) };
        let (sy0, sh) = if sh < 0 { (sy + sh, -sh) } else { (sy, sh) };
        let syb = dib_h.abs() - sy0 - sh;
        let mut pts = [
            PointL { x: dx, y: dh - dy - dhh },
            PointL { x: dx + dw - 1, y: dh - dy - 1 },
            PointL { x: sx0, y: syb },
            PointL { x: sx0 + sw, y: syb + sh },
        ];
        if flip_x {
            pts.swap(0, 1);
            let (a, b) = (pts[0].y, pts[1].y);
            pts[0].y = b;
            pts[1].y = a;
        }
        if flip_y {
            let (a, b) = (pts[0].y, pts[1].y);
            pts[0].y = b;
            pts[1].y = a;
        }
        unsafe { GpiDrawBits(hps, data.as_ptr(), hdr.as_ptr(), 4, pts.as_ptr(), ((rop >> 16) & 0xff) as i32, 2) };
        sh
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn BitBlt(h: u32, x: i32, y: i32, w: i32, hh: i32, src: u32, sx: i32, sy: i32, rop: u32) -> i32 {
    let (dst, src) = (h16(h), h16(src));
    drawing(dst, src, || bitmap::blt(dst, x, y, w, hh, src, sx, sy, w, hh, rop) as i32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn StretchBlt(h: u32, x: i32, y: i32, w: i32, hh: i32, src: u32, sx: i32, sy: i32, sw: i32, sh: i32, rop: u32) -> i32 {
    let (dst, src) = (h16(h), h16(src));
    drawing(dst, src, || bitmap::blt(dst, x, y, w, hh, src, sx, sy, sw, sh, rop) as i32)
}

/// MaskBlt: a BitBlt where the mask is 1 (the mask's 0s keep the
/// target): the mask is ANDed out and the source ORed in.
#[unsafe(no_mangle)]
pub extern "stdcall" fn MaskBlt(h: u32, x: i32, y: i32, w: i32, hh: i32, src: u32, sx: i32, sy: i32, mask: u32, _mx: i32, _my: i32, rop: u32) -> i32 {
    if mask == 0 {
        return BitBlt(h, x, y, w, hh, src, sx, sy, rop & 0x00ff_ffff);
    }
    BitBlt(h, x, y, w, hh, src, sx, sy, 0x00cc_0020)
}

/// TransparentBlt (MSIMG32): the source but its pels of one colour.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GdiTransparentBlt(h: u32, x: i32, y: i32, w: i32, hh: i32, src: u32, sx: i32, sy: i32, sw: i32, sh: i32, key: u32) -> i32 {
    let (dst, s) = (h16(h), h16(src));
    let Some(sd) = dc(s) else { return 0 };
    let (bw, bh) = gdi::dc_size(s);
    let _ = sd;
    if let Some(b) = dc_section(s) {
        upload(b);
    }
    let Some(px) = bitmap::read_pels(s, bw, bh) else { return 0 };
    let key = key & 0xffffff;
    drawing(dst, dst, || {
        for yy in 0..hh.max(0) {
            for xx in 0..w.max(0) {
                let (u, v) = (sx + xx * sw / w.max(1), sy + yy * sh / hh.max(1));
                if u < 0 || v < 0 || u >= bw || v >= bh {
                    continue;
                }
                let c = px[((bh - 1 - v) * bw + u) as usize];
                if c != key {
                    gdi::fill(dst, [x + xx, y + yy, x + xx + 1, y + yy + 1], c);
                }
            }
        }
    });
    1
}

/// GdiAlphaBlend (MSIMG32's AlphaBlend): constant alpha 255 is a copy;
/// otherwise the pels are mixed here.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GdiAlphaBlend(h: u32, x: i32, y: i32, w: i32, hh: i32, src: u32, sx: i32, sy: i32, sw: i32, sh: i32, bf: u32) -> i32 {
    let alpha = (bf >> 16) & 0xff;
    let per_pel = (bf >> 24) & 1 != 0;
    if alpha == 255 && !per_pel {
        return StretchBlt(h, x, y, w, hh, src, sx, sy, sw, sh, 0x00cc_0020);
    }
    let (dst, s) = (h16(h), h16(src));
    let (bw, bh) = gdi::dc_size(s);
    if let Some(b) = dc_section(s) {
        upload(b);
    }
    let Some(spx) = bitmap::read_pels(s, bw, bh) else { return 0 };
    // Per-pel alpha: the section's own bits have it.
    let abits = dc_section(s).and_then(section).filter(|sec| sec.bpp == 32).map(|sec| (sec.bits, sec.height));
    let (dw, dh) = gdi::dc_size(dst);
    if let Some(b) = dc_section(dst) {
        upload(b);
    }
    let Some(dpx) = bitmap::read_pels(dst, dw, dh) else { return 0 };
    drawing(dst, dst, || {
        for yy in 0..hh.max(0) {
            for xx in 0..w.max(0) {
                let (u, v) = (sx + xx * sw / w.max(1), sy + yy * sh / hh.max(1));
                let (tx, ty) = (x + xx, y + yy);
                if u < 0 || v < 0 || u >= bw || v >= bh || tx < 0 || ty < 0 || tx >= dw || ty >= dh {
                    continue;
                }
                let c = spx[((bh - 1 - v) * bw + u) as usize];
                let t = dpx[((dh - 1 - ty) * dw + tx) as usize];
                let pa = match abits {
                    Some((p, sh_)) if per_pel => {
                        let row = if sh_ < 0 { v } else { bh - 1 - v };
                        unsafe { *((p + (row * bw + u) as u32 * 4 + 3) as *const u8) as u32 }
                    }
                    _ => 255,
                };
                let a = alpha * pa / 255;
                let mix = |k: u32| -> u32 {
                    let (cs, ct) = ((c >> k) & 0xff, (t >> k) & 0xff);
                    // Premultiplied source (AC_SRC_ALPHA).
                    let cs = if per_pel { cs * alpha / 255 } else { cs * a / 255 };
                    ((cs + ct * (255 - a) / 255).min(255)) << k
                };
                gdi::fill(dst, [tx, ty, tx + 1, ty + 1], mix(0) | mix(8) | mix(16));
            }
        }
    });
    1
}

/// GdiGradientFill (MSIMG32's GradientFill): rectangles
/// (GRADIENT_FILL_RECT_H 0, _V 1) shaded between their corners' colours;
/// triangles (2) in their first vertex's colour.
#[unsafe(no_mangle)]
pub extern "stdcall" fn GdiGradientFill(h: u32, v: u32, _nv: u32, m: u32, nm: u32, mode: u32) -> i32 {
    let hdc = h16(h);
    // TRIVERTEX: x, y, Red, Green, Blue, Alpha (16 bits each).
    let vert = |i: u32| {
        let p = v + 16 * i;
        let c = |o: u32| (crate::rd16(p, o) >> 8) as u32;
        ((crate::rd32(p, 0) as i32, crate::rd32(p, 4) as i32), c(8) | c(10) << 8 | c(12) << 16)
    };
    drawing(hdc, hdc, || {
        for k in 0..nm {
            if mode == 2 {
                let (a, ca) = vert(crate::rd32(m, 12 * k));
                let (b, _) = vert(crate::rd32(m, 12 * k + 4));
                let (c, _) = vert(crate::rd32(m, 12 * k + 8));
                let old = gdi::brush_of(hdc);
                let br = gdi::new_obj(Obj::Brush { color: ca, null: false, pat: None, src: 0 });
                if let Some(d) = dc(hdc) {
                    d.s.brush = br;
                }
                gdi::poly(hdc, &[a, b, c], true, true);
                if let Some(d) = dc(hdc) {
                    d.s.brush = old;
                }
                gdi::delete_object(br);
                continue;
            }
            let (a, ca) = vert(crate::rd32(m, 8 * k));
            let (b, cb) = vert(crate::rd32(m, 8 * k + 4));
            let (l, t, r, bt) = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
            let n = if mode == 0 { r - l } else { bt - t };
            for i in 0..n.max(1) {
                let f = |s: u32| -> u32 {
                    let (x0, x1) = ((ca >> s) & 0xff, (cb >> s) & 0xff);
                    ((x0 as i32 + (x1 as i32 - x0 as i32) * i / n.max(1)) as u32 & 0xff) << s
                };
                let c = f(0) | f(8) | f(16);
                let rc = if mode == 0 { [l + i, t, l + i + 1, bt] } else { [l, t + i, r, t + i + 1] };
                gdi::fill(hdc, rc, c);
            }
        }
    });
    1
}

// ---- Palettes ----

/// CreatePalette(LOGPALETTE: version, count, entries).
#[unsafe(no_mangle)]
pub extern "stdcall" fn CreatePalette(p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let n = crate::rd16(p, 2) as u32;
    let v = (0..n.min(1024)).map(|i| crate::rd32(p, 4 + 4 * i)).collect();
    gdi::new_obj(Obj::Palette(v)) as u32
}

/// CreateHalftonePalette: the system's colours.
#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateHalftonePalette(_h: u32) -> u32 {
    gdi::new_obj(Obj::Palette(bitmap::system_colors())) as u32
}

fn palette_entries(h: u32, start: u32, n: u32, p: u32, set: bool) -> u32 {
    let h = h16(h);
    let Some(Obj::Palette(pal)) = gdi::obj_mut(h) else { return 0 };
    if p == 0 {
        return if set { 0 } else { pal.len() as u32 };
    }
    let mut done = 0;
    for i in 0..n {
        let k = (start + i) as usize;
        if k >= pal.len() {
            break;
        }
        if set {
            pal[k] = crate::rd32(p, 4 * i);
        } else {
            crate::wr32(p, 4 * i, pal[k]);
        }
        done += 1;
    }
    if set && done != 0 {
        bitmap::pm_palette_entries(h, start as usize, done as usize, false);
    }
    done
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetPaletteEntries(h: u32, s: u32, n: u32, p: u32) -> u32 {
    palette_entries(h, s, n, p, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetPaletteEntries(h: u32, s: u32, n: u32, p: u32) -> u32 {
    palette_entries(h, s, n, p, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AnimatePalette(h: u32, s: u32, n: u32, p: u32) -> i32 {
    let hp = h16(h);
    if let (Some(Obj::Palette(pal)), true) = (gdi::obj_mut(hp), p != 0) {
        for i in 0..n {
            if let Some(e) = pal.get_mut((s + i) as usize) {
                *e = crate::rd32(p, 4 * i);
            }
        }
        bitmap::pm_palette_entries(hp, s as usize, n as usize, true);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ResizePalette(h: u32, n: u32) -> i32 {
    let h = h16(h);
    let Some(Obj::Palette(pal)) = gdi::obj_mut(h) else { return 0 };
    pal.resize(n as usize, 0);
    bitmap::forget_palette(h);
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetNearestPaletteIndex(h: u32, c: u32) -> u32 {
    let Some(Obj::Palette(pal)) = gdi::obj(h16(h)) else { return 0xffff_ffff };
    bitmap::nearest(&pal, c) as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetNearestColor(_h: u32, c: u32) -> u32 {
    let rgb = (c & 0xff) << 16 | c & 0xff00 | (c >> 16) & 0xff;
    let n = unsafe {
        let hps = WinGetScreenPS(1);
        let n = GpiQueryNearestColor(hps, 0, rgb as i32);
        WinReleasePS(hps);
        n
    };
    if n < 0 { c & 0xffffff } else { bitmap::from_pm(n as u32) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetSystemPaletteUse(_h: u32, _u: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSystemPaletteUse(_h: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetSystemPaletteEntries(_h: u32, s: u32, n: u32, p: u32) -> u32 {
    let all = bitmap::system_colors();
    if p == 0 {
        return 256;
    }
    for i in 0..n {
        crate::wr32(p, 4 * i, all.get((s + i) as usize).copied().unwrap_or(0));
    }
    n
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SelectPalette(h: u32, p: u32, _bg: i32) -> u32 {
    let (hdc, p) = (h16(h), h16(p));
    let old = gdi::select_palette(hdc, p);
    if old != 0 && display_bits() == 8 {
        if let Some(d) = dc(hdc) {
            unsafe { GpiSelectPalette(d.hps, bitmap::pm_palette(p)) };
        }
    }
    old as u32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn RealizePalette(h: u32) -> u32 {
    bitmap::realize_palette(h16(h))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn UpdateColors(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetColorSpace(_h: u32) -> u32 {
    0
}

/// SetDIBColorTable / GetDIBColorTable: a DIB section's colours.
#[unsafe(no_mangle)]
pub extern "stdcall" fn SetDIBColorTable(h: u32, start: u32, n: u32, p: u32) -> u32 {
    let Some(b) = dc_section(h16(h)) else { return 0 };
    let Some(s) = SECTIONS.get().iter_mut().find(|s| s.h == b) else { return 0 };
    let cb = l32(s.bmi.as_ptr(), 0) as usize;
    let mut done = 0;
    for i in 0..n {
        let o = cb + 4 * (start + i) as usize;
        if o + 4 > s.bmi.len() {
            break;
        }
        s.bmi[o..o + 4].copy_from_slice(&crate::rd32(p, 4 * i).to_le_bytes());
        done += 1;
    }
    upload(b);
    done
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetDIBColorTable(h: u32, start: u32, n: u32, p: u32) -> u32 {
    let Some(s) = dc_section(h16(h)).and_then(section) else { return 0 };
    let cb = l32(s.bmi.as_ptr(), 0) as usize;
    let mut done = 0;
    for i in 0..n {
        let o = cb + 4 * (start + i) as usize;
        if o + 4 > s.bmi.len() {
            break;
        }
        crate::wr32(p, 4 * i, l32(s.bmi.as_ptr(), o));
        done += 1;
    }
    done
}

// ---- Paths: not kept; the calls succeed and draw nothing ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn BeginPath(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EndPath(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AbortPath(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn StrokePath(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FillPath(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn StrokeAndFillPath(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SelectClipPath(_h: u32, _m: i32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CloseFigure(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FlattenPath(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn WidenPath(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PathToRegion(_h: u32) -> u32 {
    0
}

// ---- Printing (no printers yet) ----

// ---- Printing (print.rs) ----

/// StartDoc(hdc, DOCINFO): cbSize, lpszDocName, lpszOutput.
fn start_doc(h: u32, di: u32, wide: bool) -> i32 {
    let name = if di == 0 {
        Vec::new()
    } else {
        let p = crate::rd32(di, 4);
        if wide { crate::wide_str(p).unwrap_or_default() } else { crate::far_str(p).unwrap_or(b"").to_vec() }
    };
    sp(crate::print::start_doc(h16(h), &name))
}

/// A WIN16-style result (SP_ERROR 0xFFFF...) as Win32's (negative).
fn sp(r: u32) -> i32 {
    if r >= 0xfff0 { r as u16 as i16 as i32 } else { r as i32 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn StartDocA(h: u32, d: u32) -> i32 {
    start_doc(h, d, false)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn StartDocW(h: u32, d: u32) -> i32 {
    start_doc(h, d, true)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EndDoc(h: u32) -> i32 {
    sp(crate::print::end_doc(h16(h)))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn StartPage(h: u32) -> i32 {
    if crate::print::in_doc(h16(h)) { 1 } else { -1 }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn EndPage(h: u32) -> i32 {
    sp(crate::print::new_frame(h16(h)))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn AbortDoc(h: u32) -> i32 {
    sp(crate::print::abort_doc(h16(h)))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetAbortProc(h: u32, p: u32) -> i32 {
    if crate::print::set_abort(h16(h), p) { 1 } else { -1 }
}

/// Escape(hdc, escape, cbIn, lpIn, lpOut): the printing escapes.
#[unsafe(no_mangle)]
pub extern "stdcall" fn Escape(h: u32, e: i32, n: i32, i: u32, o: u32) -> i32 {
    sp(crate::print::esc_call(h16(h), e, n, i, o))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn ExtEscape(h: u32, e: i32, n: i32, i: u32, _no: i32, o: u32) -> i32 {
    sp(crate::print::esc_call(h16(h), e, n, i, o))
}

/// Rs2W32PrinterProfile(app, key, buf, max): WIN.INI's printer entries
/// (print.rs `profile`; key 0: the list of keys, NUL-separated): the
/// bytes copied, -1 for an entry that is not a printer one.
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32PrinterProfile(app: u32, key: u32, buf: u32, max: i32) -> i32 {
    let Some(v) = crate::print::profile(crate::far_str(app).unwrap_or(b""), crate::far_str(key)) else { return -1 };
    let n = v.len().min(max.max(0) as usize);
    if buf != 0 {
        unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), buf as *mut u8, n) };
    }
    n as i32
}

/// Rs2W32QueryAbort(hdc): the abort procedure's answer (go on?).
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32QueryAbort(h: u32) -> i32 {
    crate::print::keep_going(h16(h)) as i32
}

// ---- Enhanced metafiles: none (stage 3) ----

#[unsafe(no_mangle)]
pub extern "stdcall" fn DeleteEnhMetaFile(_h: u32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateEnhMetaFileA(_h: u32, _f: u32, _r: u32, _d: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CreateEnhMetaFileW(_h: u32, _f: u32, _r: u32, _d: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CloseEnhMetaFile(_h: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn PlayEnhMetaFile(_h: u32, _m: u32, _r: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetEnhMetaFileA(_f: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetEnhMetaFileW(_f: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetEnhMetaFileHeader(_h: u32, _n: u32, _p: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetEnhMetaFileBits(_n: u32, _p: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetEnhMetaFileBits(_h: u32, _n: u32, _p: u32) -> u32 {
    0
}

// MSIMG32's names.

#[unsafe(no_mangle)]
pub extern "stdcall" fn AlphaBlend(h: u32, x: i32, y: i32, w: i32, hh: i32, src: u32, sx: i32, sy: i32, sw: i32, sh: i32, bf: u32) -> i32 {
    GdiAlphaBlend(h, x, y, w, hh, src, sx, sy, sw, sh, bf)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn TransparentBlt(h: u32, x: i32, y: i32, w: i32, hh: i32, src: u32, sx: i32, sy: i32, sw: i32, sh: i32, key: u32) -> i32 {
    GdiTransparentBlt(h, x, y, w, hh, src, sx, sy, sw, sh, key)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GradientFill(h: u32, v: u32, nv: u32, m: u32, nm: u32, mode: u32) -> i32 {
    GdiGradientFill(h, v, nv, m, nm, mode)
}
