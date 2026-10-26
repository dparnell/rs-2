//! The clipboard: Windows' on PM's, shared with every PM program.
//!
//! - CF_TEXT (and CF_OEMTEXT, the same text here) is PM's CF_TEXT.
//! - CF_DIB is kept as it is, and given to PM programs as a PM bitmap
//!   (CF_BITMAP; a Windows DIB is an OS/2 2.0 bitmap with a 40-byte
//!   header). A PM bitmap on the clipboard is a DIB (24 bits) for
//!   Windows programs.
//! - CF_BITMAP is a GDI bitmap (bitmap.rs): PM gets a PM bitmap with its
//!   picture, and a PM bitmap comes back as a new GDI bitmap.
//! - CF_METAFILEPICT keeps its METAFILEPICT and the metafile's bits
//!   (Windows programs paste it; PM programs do not see it).
//! - Registered formats (RegisterClipboardFormat) are global atoms, the
//!   same numbers in every process and in PM; Windows' other predefined
//!   formats are PM formats named "Win16 format n".
//! - Data other than text is a PM CFI_POINTER item holding the size and
//!   the bytes, so GlobalSize comes back exact.
//! - Delayed rendering (SetClipboardData with no data) is PM's: the
//!   owner gets WM_RENDERFORMAT (PM's WM_RENDERFMT), WM_RENDERALLFORMATS
//!   and WM_DESTROYCLIPBOARD; the viewer WM_DRAWCLIPBOARD (PM has one
//!   viewer, no chain).

use alloc::vec::Vec;
use thunk16::{flat, Pascal};

use crate::kernel::{global_alloc, global_free, global_size, TASK};
use crate::pm::*;
use crate::{far_str, user, Global};

unsafe extern "C" {
    #[link_name = "PMWIN.739"]
    fn WinEnumClipbrdFmts(hab: u32, fmt: u32) -> u32;
    #[link_name = "PMWIN.855"]
    fn WinSetClipbrdOwner(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.808"]
    fn WinQueryClipbrdOwner(hab: u32) -> u32;
    #[link_name = "PMWIN.856"]
    fn WinSetClipbrdViewer(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.809"]
    fn WinQueryClipbrdViewer(hab: u32) -> u32;
    #[link_name = "PMGPI.598"]
    fn GpiCreateBitmap(hps: u32, hdr: *const u8, opts: u32, bits: *const u8, info: *const u8) -> u32;
    #[link_name = "PMGPI.371"]
    fn GpiDeleteBitmap(hbm: u32) -> u32;
    #[link_name = "PMGPI.506"]
    fn GpiSetBitmap(hps: u32, hbm: u32) -> u32;
    #[link_name = "PMGPI.601"]
    fn GpiQueryBitmapInfoHeader(hbm: u32, p: *mut u8) -> u32;
    #[link_name = "PMGPI.599"]
    fn GpiQueryBitmapBits(hps: u32, start: i32, n: i32, buf: *mut u8, bmi: *mut u8) -> i32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, ty: i32, tok: *const u8, n: i32, dop: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
}

pub const CF_TEXT: u16 = 1;
pub const CF_BITMAP: u16 = 2;
pub const CF_METAFILEPICT: u16 = 3;
pub const CF_OEMTEXT: u16 = 7;
pub const CF_DIB: u16 = 8;
const CF_DSPTEXT: u16 = 0x81;

/// PM's formats and item kinds.
const PM_CF_TEXT: u32 = 1;
const PM_CF_BITMAP: u32 = 2;
const PM_CF_DSPTEXT: u32 = 3;
const CFI_HANDLE: u32 = 0x0200;
const CFI_POINTER: u32 = 0x0400;

// PM's clipboard messages and Windows'.
const PM_RENDERFMT: u32 = 0x0060;
const PM_RENDERALLFMTS: u32 = 0x0061;
const PM_DESTROYCLIPBOARD: u32 = 0x0062;
const PM_DRAWCLIPBOARD: u32 = 0x0067;
const W_RENDERFORMAT: u16 = 0x0305;
const W_RENDERALLFORMATS: u16 = 0x0306;
const W_DESTROYCLIPBOARD: u16 = 0x0307;
const W_DRAWCLIPBOARD: u16 = 0x0308;

struct Clip {
    open: bool,
    /// The window that opened the clipboard (the owner after
    /// EmptyClipboard).
    opener: u16,
    /// Rendering a delayed format (SetClipboardData without opening).
    rendering: bool,
    /// Copies GetClipboardData handed out (freed at the next
    /// OpenClipboard).
    given: Vec<u16>,
}

static CLIP: Global<Clip> = Global::new(Clip { open: false, opener: 0, rendering: false, given: Vec::new() });

fn c() -> &'static mut Clip {
    CLIP.get()
}

fn hab() -> u32 {
    TASK.get().hab
}

/// The PM format of a Windows format (None: not kept).
fn pm_format(f: u16) -> Option<u32> {
    match f {
        CF_TEXT | CF_OEMTEXT => Some(PM_CF_TEXT),
        CF_DSPTEXT => Some(PM_CF_DSPTEXT),
        CF_BITMAP | 0x82 => Some(PM_CF_BITMAP),
        0xc000.. => Some(f as u32),
        0 => None,
        _ => Some(crate::dde::add_global_atom(alloc::format!("Win16 format {f}").as_bytes()) as u32),
    }
}

/// The Windows format of a PM format.
fn win_format(f: u32) -> Option<u16> {
    match f {
        PM_CF_TEXT => Some(CF_TEXT),
        PM_CF_DSPTEXT => Some(CF_DSPTEXT),
        PM_CF_BITMAP => Some(CF_BITMAP),
        0xc000..=0xffff => {
            let n = crate::dde::global_atom_name(f as u16);
            match n.strip_prefix(b"Win16 format ") {
                Some(d) => d.iter().try_fold(0u16, |v, &c| c.is_ascii_digit().then(|| v * 10 + (c - b'0') as u16)),
                None => Some(f as u16),
            }
        }
        _ => None,
    }
}

/// Is format `f` on PM's clipboard?
fn pm_has(f: u32) -> bool {
    let mut info = 0;
    unsafe { WinQueryClipbrdFmtInfo(hab(), f, &mut info) != 0 }
}

/// Give PM giveable memory with `bytes` (CFI_POINTER).
fn put_bytes(f: u32, bytes: &[u8], sized: bool) -> bool {
    let n = bytes.len() + if sized { 4 } else { 0 };
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), n.max(1) as u32, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | os2::OBJ_GIVEABLE) } != 0 {
        return false;
    }
    unsafe {
        let mut q = p;
        if sized {
            (q as *mut u32).write_unaligned(bytes.len() as u32);
            q = q.add(4);
        }
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), q, bytes.len());
        WinSetClipbrdData(hab(), p as u32, f, CFI_POINTER) != 0
    }
}

/// The bytes of a sized item.
fn get_bytes(f: u32) -> Option<Vec<u8>> {
    let p = unsafe { WinQueryClipbrdData(hab(), f) } as *const u8;
    if p.is_null() {
        return None;
    }
    let n = unsafe { (p as *const u32).read_unaligned() } as usize;
    Some(unsafe { core::slice::from_raw_parts(p.add(4), n) }.to_vec())
}

fn block_bytes(h: u16) -> Vec<u8> {
    let n = global_size(h) as usize;
    let n = if n == 0 { 0x10000 - 0 } else { n };
    unsafe { core::slice::from_raw_parts(flat(((h | 7) as u32) << 16), n) }.to_vec()
}

/// A new global block with `bytes`, handed out by GetClipboardData.
fn new_block(bytes: &[u8]) -> u16 {
    let h = global_alloc(0x2042, bytes.len().max(1) as u32);
    if h != 0 {
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), flat(((h | 7) as u32) << 16), bytes.len()) };
        c().given.push(h);
    }
    h
}

/// The size of a DIB's header and colour table.
fn dib_header_size(d: &[u8]) -> usize {
    if d.len() < 16 {
        return d.len();
    }
    let rd = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let fix = rd(0) as usize;
    if fix == 12 {
        let bits = u16::from_le_bytes([d[10], d[11]]);
        return 12 + if bits <= 8 { 3 << bits } else { 0 };
    }
    let bits = u16::from_le_bytes([d[14], d[15]]);
    let used = if fix >= 36 && d.len() >= 36 { rd(32) } else { 0 };
    let n = if used != 0 { used as usize } else if bits <= 8 { 1 << bits } else { 0 };
    // BI_BITFIELDS: three masks.
    let masks = if fix == 40 && d.len() >= 20 && rd(16) == 3 { 12 } else { 0 };
    fix + n * 4 + masks
}

/// A PM bitmap of a DIB.
fn dib_to_pm(d: &[u8]) -> u32 {
    let hs = dib_header_size(d);
    if d.len() < hs || hs < 12 {
        return 0;
    }
    unsafe { GpiCreateBitmap(0, d.as_ptr(), 4, d.as_ptr().add(hs), d.as_ptr()) }
}

/// A 24-bit DIB of a PM bitmap.
pub fn pm_to_dib(hbm: u32) -> Option<Vec<u8>> {
    let mut hdr = [0u8; 40];
    hdr[0] = 40;
    if unsafe { GpiQueryBitmapInfoHeader(hbm, hdr.as_mut_ptr()) } == 0 {
        return None;
    }
    let w = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
    let h = u32::from_le_bytes(hdr[8..12].try_into().unwrap()) as usize;
    let stride = (w * 3).next_multiple_of(4);
    let mut info = [0u8; 40];
    info[0] = 40;
    info[4..8].copy_from_slice(&(w as u32).to_le_bytes());
    info[8..12].copy_from_slice(&(h as u32).to_le_bytes());
    info[12] = 1;
    info[14] = 24;
    info[20..24].copy_from_slice(&((stride * h) as u32).to_le_bytes());
    let mut bits = alloc::vec![0u8; stride * h];
    let hab = hab();
    let dc = unsafe { DevOpenDC(hab, 8, b"*\0".as_ptr(), 0, core::ptr::null(), 0) };
    if dc == 0 {
        return None;
    }
    let size = [0i32, 0];
    // PU_PELS | GPIT_MICRO | GPIA_ASSOC
    let ps = unsafe { GpiCreatePS(hab, dc, size.as_ptr(), 0x0008 | 0x1000 | 0x4000) };
    let mut ok = false;
    if ps != 0 {
        unsafe {
            GpiSetBitmap(ps, hbm);
            let mut q = info;
            ok = GpiQueryBitmapBits(ps, 0, h as i32, bits.as_mut_ptr(), q.as_mut_ptr()) > 0;
            GpiSetBitmap(ps, 0);
            GpiDestroyPS(ps);
        }
    }
    unsafe { DevCloseDC(dc) };
    if !ok {
        return None;
    }
    let mut d = info.to_vec();
    d.extend_from_slice(&bits);
    Some(d)
}

fn free_given() {
    for h in core::mem::take(&mut c().given) {
        global_free(h);
    }
}

/// SetClipboardData(fmt, hData): the handle (0 on failure).
fn set_data(f: u16, h: u16) -> u32 {
    if !c().open && !c().rendering {
        return 0;
    }
    let Some(pf) = pm_format(f) else { return 0 };
    if h == 0 {
        // Delayed rendering: the owner renders it when asked.
        let info = if pf == PM_CF_BITMAP { CFI_HANDLE } else { CFI_POINTER };
        let ok = unsafe { WinSetClipbrdData(hab(), 0, pf, info) } != 0;
        if f == CF_DIB {
            unsafe { WinSetClipbrdData(hab(), 0, PM_CF_BITMAP, CFI_HANDLE) };
        }
        return ok as u32;
    }
    if !c().open {
        // The WM_RENDERFORMAT's format.
        c().rendering = false;
    }
    let ok = match f {
        CF_TEXT | CF_OEMTEXT | CF_DSPTEXT => {
            let t = unsafe { crate::cstr(flat(((h | 7) as u32) << 16)) }.to_vec();
            let mut z = t;
            z.push(0);
            let ok = put_bytes(pf, &z, false);
            if ok {
                global_free(h);
            }
            ok
        }
        CF_BITMAP | 0x82 => {
            // The bitmap goes to the clipboard.
            match bitmap_pm(h) {
                Some(hbm) => unsafe { WinSetClipbrdData(hab(), hbm, pf, CFI_HANDLE) != 0 },
                None => false,
            }
        }
        CF_METAFILEPICT | 0x83 => {
            // METAFILEPICT (mm, xExt, yExt, hMF) and the metafile's bits.
            let mp = block_bytes(h);
            let hmf = u16::from_le_bytes([mp[6], mp[7]]);
            let mut v = mp[..8].to_vec();
            if hmf != 0 {
                v.extend_from_slice(&block_bytes(hmf));
            }
            let ok = put_bytes(pf, &v, true);
            if ok {
                global_free(hmf);
                global_free(h);
            }
            ok
        }
        _ => {
            let b = block_bytes(h);
            let ok = put_bytes(pf, &b, true);
            if ok && f == CF_DIB && !pm_has(PM_CF_BITMAP) {
                let hbm = dib_to_pm(&b);
                if hbm != 0 && unsafe { WinSetClipbrdData(hab(), hbm, PM_CF_BITMAP, CFI_HANDLE) } == 0 {
                    unsafe { GpiDeleteBitmap(hbm) };
                }
            }
            if ok {
                global_free(h);
            }
            ok
        }
    };
    if ok { h as u32 } else { 0 }
}

/// GetClipboardData(fmt): a handle (the clipboard's).
fn get_data(f: u16) -> u16 {
    match f {
        CF_TEXT | CF_OEMTEXT | CF_DSPTEXT => {
            let pf = if f == CF_DSPTEXT { PM_CF_DSPTEXT } else { PM_CF_TEXT };
            let p = unsafe { WinQueryClipbrdData(hab(), pf) } as *const u8;
            if p.is_null() {
                return 0;
            }
            let mut t = unsafe { crate::cstr(p) }.to_vec();
            t.push(0);
            new_block(&t)
        }
        CF_BITMAP | 0x82 => {
            let hbm = unsafe { WinQueryClipbrdData(hab(), PM_CF_BITMAP) };
            if hbm == 0 { 0 } else { bitmap_from_pm(hbm) }
        }
        CF_DIB => {
            if let Some(b) = pm_format(CF_DIB).and_then(get_bytes) {
                return new_block(&b);
            }
            let hbm = unsafe { WinQueryClipbrdData(hab(), PM_CF_BITMAP) };
            if hbm == 0 {
                return 0;
            }
            pm_to_dib(hbm).map_or(0, |d| new_block(&d))
        }
        CF_METAFILEPICT | 0x83 => {
            let Some(b) = pm_format(f).and_then(get_bytes) else { return 0 };
            if b.len() < 8 {
                return 0;
            }
            let hmf = if b.len() > 8 { new_block(&b[8..]) } else { 0 };
            let mut mp = b[..8].to_vec();
            mp[6..8].copy_from_slice(&hmf.to_le_bytes());
            new_block(&mp)
        }
        _ => {
            let Some(pf) = pm_format(f) else { return 0 };
            let Some(b) = get_bytes(pf) else { return 0 };
            new_block(&b)
        }
    }
}

/// Is a Windows format available (with the ones made from others)?
fn available(f: u16) -> bool {
    match f {
        CF_TEXT | CF_OEMTEXT => pm_has(PM_CF_TEXT),
        CF_DIB => pm_format(CF_DIB).is_some_and(pm_has) || pm_has(PM_CF_BITMAP),
        0 => false,
        _ => pm_format(f).is_some_and(pm_has),
    }
}

/// The Windows formats on the clipboard, in PM's order (CF_OEMTEXT after
/// CF_TEXT, CF_DIB after a bitmap).
fn formats() -> Vec<u16> {
    let mut v = Vec::new();
    let mut f = 0;
    loop {
        f = unsafe { WinEnumClipbrdFmts(hab(), f) };
        if f == 0 {
            break;
        }
        if let Some(w) = win_format(f) {
            if !v.contains(&w) {
                v.push(w);
            }
            if w == CF_TEXT && !v.contains(&CF_OEMTEXT) {
                v.push(CF_OEMTEXT);
            }
        }
    }
    if v.contains(&CF_BITMAP) && !v.contains(&CF_DIB) {
        v.push(CF_DIB);
    }
    v
}

/// A Windows window for a PM window (a proxy for another task's).
fn hwnd_for(pm: u32) -> u16 {
    if pm == 0 { 0 } else { crate::dde::proxy(pm) }
}

fn pm_for(h: u16) -> u32 {
    user::wnd(h).map(|w| w.client).or_else(|| crate::dde::foreign(h)).unwrap_or(0)
}

/// The clipboard calls (USER ordinals), None for others.
pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    let hab = hab();
    Some(match ord {
        // OpenClipboard(hwnd)
        137 => {
            let h = a.w();
            if c().open {
                return Some(0);
            }
            free_given();
            let ok = unsafe { WinOpenClipbrd(hab) } != 0;
            c().open = ok;
            c().opener = h;
            ok as u32
        }
        // CloseClipboard
        138 => {
            if !c().open {
                return Some(0);
            }
            c().open = false;
            unsafe { WinCloseClipbrd(hab) };
            1
        }
        // EmptyClipboard: the window that opened it owns it now.
        139 => {
            if !c().open {
                return Some(0);
            }
            unsafe {
                WinEmptyClipbrd(hab);
                WinSetClipbrdOwner(hab, pm_for(c().opener));
            }
            1
        }
        // GetClipboardOwner
        140 => hwnd_for(unsafe { WinQueryClipbrdOwner(hab) }) as u32,
        // SetClipboardData(fmt, hData)
        141 => {
            let (f, h) = (a.w(), a.w());
            set_data(f, h)
        }
        // GetClipboardData(fmt)
        142 => {
            let f = a.w();
            if !c().open {
                return Some(0);
            }
            get_data(f) as u32
        }
        // CountClipboardFormats
        143 => formats().len() as u32,
        // EnumClipboardFormats(fmt): the next one (0 at the end).
        144 => {
            let f = a.w();
            let v = formats();
            match f {
                0 => v.first().copied().unwrap_or(0) as u32,
                _ => v.iter().position(|&x| x == f).and_then(|i| v.get(i + 1)).copied().unwrap_or(0) as u32,
            }
        }
        // RegisterClipboardFormat(name)
        145 => {
            let n = far_str(a.l()).unwrap_or(b"");
            if n.is_empty() { 0 } else { crate::dde::add_global_atom(n) as u32 }
        }
        // GetClipboardFormatName(fmt, buf, max)
        146 => {
            let (f, buf, max) = (a.w(), a.p(), a.i());
            if f < 0xc000 {
                return Some(0);
            }
            let n = crate::dde::global_atom_name(f);
            user::copy_out(&n, buf, max) as u32
        }
        // SetClipboardViewer(hwnd): the previous viewer.
        147 => {
            let h = a.w();
            let old = unsafe { WinQueryClipbrdViewer(hab) };
            unsafe { WinSetClipbrdViewer(hab, pm_for(h)) };
            hwnd_for(old) as u32
        }
        // GetClipboardViewer
        148 => hwnd_for(unsafe { WinQueryClipbrdViewer(hab) }) as u32,
        // ChangeClipboardChain(hwndRemove, hwndNext)
        149 => {
            let (rm, next) = (a.w(), a.w());
            if unsafe { WinQueryClipbrdViewer(hab) } == pm_for(rm) {
                unsafe { WinSetClipbrdViewer(hab, pm_for(next)) };
            }
            0
        }
        // IsClipboardFormatAvailable(fmt)
        193 => available(a.w()) as u32,
        _ => return None,
    })
}

/// PM's clipboard messages to a Windows window, as Windows' ones.
pub fn is_clip_message(m: u32) -> bool {
    matches!(m, PM_RENDERFMT | PM_RENDERALLFMTS | PM_DESTROYCLIPBOARD | PM_DRAWCLIPBOARD)
}

pub fn translate(m: u32, mp1: u32, _mp2: u32) -> (u16, u16, u32) {
    match m {
        PM_RENDERFMT => {
            // The program renders with SetClipboardData (unopened).
            c().rendering = true;
            (W_RENDERFORMAT, win_format(mp1).unwrap_or(0), 0)
        }
        PM_RENDERALLFMTS => (W_RENDERALLFORMATS, 0, 0),
        PM_DESTROYCLIPBOARD => (W_DESTROYCLIPBOARD, 0, 0),
        _ => (W_DRAWCLIPBOARD, 0, 0),
    }
}

/// A PM bitmap for the clipboard with a GDI bitmap's picture (the
/// clipboard keeps it; the program's bitmap stays its own).
fn bitmap_pm(h: u16) -> Option<u32> {
    let (hbm, ..) = crate::bitmap::bitmap_of(h)?;
    let copy = dib_to_pm(&pm_to_dib(hbm)?);
    (copy != 0).then_some(copy)
}

/// A GDI bitmap with (a copy of) a PM bitmap's picture.
fn bitmap_from_pm(hbm: u32) -> u16 {
    pm_to_dib(hbm).map_or(0, |d| crate::bitmap::from_dib(&d))
}
