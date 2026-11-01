//! The graphics engine's entry points (PMGRE.DLL's Gre32EntryN, docs/PM.md
//! "The graphics engine"): PMGRE hands every call here with its
//! arguments as they are on the caller's stack, the function number
//! (`NGreXxx`) last.
//!
//! IBM's dispatcher (PMMERGE's Dispatch32) works on the low word of the
//! function number: bit 0x4000 says the first argument is a DC, whose
//! handle is checked (an unknown one fails with PMERR_INV_HDC and the
//! function's default error value, from a table by the low byte);
//! the high word (COM_DRAW, COM_TRANSFORM ...) is replaced by the DC's
//! own flags, so a program passes 0 there; the low 9 bits index the
//! device's function table, whose empty slots fail with
//! PMERR_DEV_FUNC_NOT_INSTALLED and 0. rs2 has no device drivers: the
//! functions below run on the DC's presentation space in the GPI, in
//! device coordinates (the program's points are not transformed). The
//! others fail as an empty slot does.

use alloc::vec::Vec;

use gpi::Rect;

use crate::gpiapi::{self, PointL, RectL, GPI_ERROR};
use crate::gpiobj;
use crate::state::st;

const PMERR_DEV_FUNC_NOT_INSTALLED: u32 = 0x0010_2023;
const PMERR_INV_HDC: u32 = 0x0008_207c;
const PMERR_INV_HBITMAP: u32 = 0x0008_207b;

const NGRE_QUERY_ENGINE_VERSION: u32 = 0x020a;
const NGRE_SET_BITMAP_OWNER: u32 = 0x021a;
const NGRE_BITBLT: u32 = 0x6026;
const NGRE_SAVE_SCREEN_BITS: u32 = 0x402c;
const NGRE_RESTORE_SCREEN_BITS: u32 = 0x402d;
const NGRE_GET_DC_ORIGIN: u32 = 0x40ba;
const NGRE_QUERY_DEVICE_CAPS: u32 = 0x40d1;
const NGRE_ESCAPE: u32 = 0x40d2;
const NGRE_GET_HANDLE: u32 = 0x4203;
const NGRE_SET_HANDLE: u32 = 0x4204;
const NGRE_SET_DC_OWNER: u32 = 0x420b;

/// The engine version GreQueryEngineVersion reports (Warp 4.5's).
const ENGINE_VERSION: u32 = 0x23f;

/// RestoreScreenBits options: put the bits back on the screen, forget
/// them (COMETRUN probes with 1 and erases its comet with 3).
const RSB_RESTORE: u32 = 1;
const RSB_FREE: u32 = 2;

/// GreSetHandle's slots per DC.
const HANDLES: usize = 8;

const HSB_BASE: u32 = 0x5000_0001;

/// Pels saved by GreSaveScreenBits: the device rectangle of the DC they
/// came from and its pels, bottom row first.
struct Saved {
    hdc: u32,
    r: Rect,
    pels: Vec<Option<u32>>,
    back: u32,
}

/// The engine's objects of a process (in gpiobj's Objects).
#[derive(Default)]
pub struct Objects {
    saved: Vec<Option<Saved>>,
    /// GreSetHandle's handles: (hdc, slots).
    handles: Vec<(u32, [u32; HANDLES])>,
}

fn objects() -> &'static mut Objects {
    &mut gpiobj::objects().gre
}

/// A failing function's value when its DC is not valid: -1 (GPI_ALTERROR)
/// for the queries IBM's table lists, else 0. Engine functions (bit
/// 0x200) and device functions have a table each, by the low byte.
fn default_error(f: u32) -> u32 {
    const ENGINE: &[u8] = &[0x03, 0x05, 0x0a, 0x12, 0x17, 0x23, 0x28, 0x30, 0x42, 0x44, 0x45, 0x46, 0x49, 0x52, 0x57, 0x58, 0x60, 0x61, 0x66];
    const DEVICE: &[u8] = &[0x27, 0x30, 0x31, 0x73, 0xaa, 0xbc, 0xc4, 0xc8, 0xc9, 0xca, 0xcb, 0xd2, 0xd3, 0xd4, 0xd8, 0xda, 0xdb, 0xe3, 0xe5, 0xea, 0xf2, 0xf7];
    let table = if f & 0x200 != 0 { ENGINE } else { DEVICE };
    if table.contains(&(f as u8)) { u32::MAX } else { 0 }
}

/// Rs2GreEntry(pulArgs, cArgs): a Gre32EntryN call (PMGRE.DLL).
#[unsafe(no_mangle)]
pub extern "C" fn Rs2GreEntry(args: *const u32, n: u32) -> u32 {
    if args.is_null() || !(2..=10).contains(&n) {
        return 0;
    }
    let a: Vec<u32> = (0..n as usize).map(|i| unsafe { args.add(i).read_unaligned() }).collect();
    let f = a[n as usize - 1] & 0xffff;
    // rs2's DC handles are even (`hdc | 1` is the DC's own PS): IBM's
    // dispatcher clears the low two bits of its handles, rs2 the lowest.
    let hdc = a[0] & !1;
    if f & 0x4000 != 0 && !gpiobj::is_dc(hdc) {
        crate::misc::set_error(PMERR_INV_HDC);
        return default_error(f);
    }
    let arg = |i: usize| a.get(i).copied().unwrap_or(0);
    match (f, n) {
        (NGRE_QUERY_ENGINE_VERSION, _) => ENGINE_VERSION,
        (NGRE_SET_BITMAP_OWNER, 4) => {
            if !gpiobj::is_bitmap(a[0]) {
                crate::misc::set_error(PMERR_INV_HBITMAP);
                return 0;
            }
            1
        }
        (NGRE_SET_DC_OWNER, 4) => 1,
        (NGRE_BITBLT, 9) => bitblt(hdc, arg(1), arg(2) as i32, arg(3) as *const PointL, arg(4) as i32, arg(5), arg(6) as *const i32) as u32,
        (NGRE_SAVE_SCREEN_BITS, 4) => save_screen_bits(hdc, arg(1) as *const RectL),
        (NGRE_RESTORE_SCREEN_BITS, 6) => restore_screen_bits(hdc, arg(1), arg(2) as *const RectL, arg(3)),
        (NGRE_GET_DC_ORIGIN, 4) => get_dc_origin(hdc, arg(1) as *mut PointL),
        (NGRE_QUERY_DEVICE_CAPS, 6) => gpiobj::DevQueryCaps(hdc, arg(1) as i32, arg(3) as i32, arg(2) as *mut i32),
        (NGRE_ESCAPE, 8) => gpiobj::DevEscape(hdc, arg(1) as i32, arg(2) as i32, arg(3) as *const u8, arg(4) as *mut i32, arg(5) as *mut u8) as u32,
        (NGRE_GET_HANDLE, 4) => {
            let i = arg(1) as usize;
            objects().handles.iter().find(|(d, _)| *d == hdc).and_then(|(_, h)| h.get(i).copied()).unwrap_or(0)
        }
        (NGRE_SET_HANDLE, 5) => {
            let i = arg(2) as usize;
            if i >= HANDLES {
                return 0;
            }
            let o = objects();
            match o.handles.iter_mut().find(|(d, _)| *d == hdc) {
                Some((_, h)) => h[i] = arg(1),
                None => {
                    let mut h = [0; HANDLES];
                    h[i] = arg(1);
                    o.handles.push((hdc, h));
                }
            }
            1
        }
        _ => {
            crate::misc::set_error(PMERR_DEV_FUNC_NOT_INSTALLED);
            0
        }
    }
}

/// GreBitblt(hdcDst, hdcSrc, cPoints, aptl, lRop, flOptions, pbbAttrs):
/// GpiBitBlt between the DCs' presentation spaces, in device coordinates
/// (target corners, top right exclusive, then the source's); the
/// BITBLTATTRS (cb, lColor, lBackColor) give the colours a monochrome
/// source turns into.
fn bitblt(dst: u32, src: u32, n: i32, p: *const PointL, rop: i32, opts: u32, attrs: *const i32) -> i32 {
    let dps = gpiobj::ps_of_dc(dst);
    let src = src & !1;
    let sps = if src != 0 && gpiobj::is_dc(src) { gpiobj::ps_of_dc(src) } else { 0 };
    if dps == 0 {
        return GPI_ERROR;
    }
    let colors = (!attrs.is_null() && unsafe { attrs.read_unaligned() } >= 12).then(|| unsafe { (attrs.add(1).read_unaligned(), attrs.add(2).read_unaligned()) });
    let saved = colors.and_then(|(fore, back)| {
        let e = gpiapi::entry(st(), dps)?;
        let old = e.ps.attrs.image;
        e.ps.attrs.image.color = fore;
        e.ps.attrs.image.back = back;
        Some(old)
    });
    let r = gpiobj::GpiBitBlt(dps, sps, n, p, rop, opts);
    if let Some(old) = saved
        && let Some(e) = gpiapi::entry(st(), dps)
    {
        e.ps.attrs.image = old;
    }
    r
}

/// GreSaveScreenBits(hdc, prcl): keep the pels of a device rectangle of
/// the DC (top right exclusive); a handle for GreRestoreScreenBits, 0 if
/// there is nothing to save.
fn save_screen_bits(hdc: u32, prcl: *const RectL) -> u32 {
    let Some(r) = gpiapi::read(prcl).map(|r| r.rect()) else { return 0 };
    let hps = gpiobj::ps_of_dc(hdc);
    if hps == 0 || r.is_empty() {
        return 0;
    }
    let Some((pels, _, back)) = gpiobj::source_pels(hps, r, r.width(), r.height()) else { return 0 };
    let o = objects();
    let sv = Saved { hdc, r, pels, back };
    match o.saved.iter().position(|s| s.is_none()) {
        Some(i) => {
            o.saved[i] = Some(sv);
            HSB_BASE + i as u32
        }
        None => {
            o.saved.push(Some(sv));
            HSB_BASE + o.saved.len() as u32 - 1
        }
    }
}

/// GreRestoreScreenBits(hdc, hsb, prcl, flOptions): RSB_RESTORE puts the
/// saved pels back (only those inside `prcl` when given), RSB_FREE
/// forgets them.
fn restore_screen_bits(hdc: u32, hsb: u32, prcl: *const RectL, opts: u32) -> u32 {
    let i = hsb.wrapping_sub(HSB_BASE) as usize;
    if !objects().saved.get(i).is_some_and(|s| s.as_ref().is_some_and(|s| s.hdc == hdc)) {
        return 0;
    }
    if opts & RSB_RESTORE != 0 {
        let Some(sv) = objects().saved[i].as_ref() else { return 0 };
        let r = sv.r;
        let t = gpiapi::read(prcl).map_or(r, |p| r.intersect(&p.rect()));
        if !t.is_empty() {
            let w = r.width();
            let pels: Vec<Option<u32>> = (t.bottom..t.top).flat_map(|y| (t.left..t.right).map(move |x| ((y - r.bottom) * w + x - r.left) as usize)).map(|k| sv.pels.get(k).copied().flatten()).collect();
            let back = sv.back;
            let hps = gpiobj::ps_of_dc(hdc);
            gpiobj::blit_to(hps, t, Some((pels, false, back)), 0xcc, (t.width(), t.height()));
        }
    }
    if opts & RSB_FREE != 0 {
        objects().saved[i] = None;
    }
    1
}

/// GreGetDCOrigin(hdc, pptl): where the DC's device space starts on the
/// screen (a window DC's window; 0, 0 for the others).
fn get_dc_origin(hdc: u32, pptl: *mut PointL) -> u32 {
    let hps = gpiobj::ps_of_dc(hdc);
    let s = st();
    gpiapi::refresh(s, hps);
    let o = gpiapi::entry(s, hps).filter(|e| e.hwnd != 0).map_or(gpi::Point::new(0, 0), |e| e.ps.origin);
    gpiapi::write(pptl, PointL { x: o.x, y: o.y }) as u32
}
