//! PMDRAG.DLL: direct manipulation (drag and drop), as IBM's (whose code
//! is in PMCTLS.DLL) does it:
//!
//! - String handles are atoms of the system atom table (a name starting
//!   with `!` or `#` gets a 0xFF byte in front, so it is not taken for an
//!   integer atom), so every process sees them.
//! - A DRAGINFO is giveable and gettable shared memory (the same address
//!   in every process): a 0x2c-byte header (magic 0xF00E0E0F, the items'
//!   address) in front of the DRAGINFO, then the DRAGITEMs. Targets get
//!   at it with DrgAccessDraginfo (DosGetSharedMem) and let go of it with
//!   DrgFreeDraginfo (DosFreeMem); the source frees it after DrgDrag.
//!   DRAGTRANSFERs are the same, behind a header with magic 0xE0E00F0F.
//! - DrgDrag is modal: it captures the mouse for the source window and
//!   reads the thread's messages itself until the terminating button goes
//!   up (or Esc). Over each window it sends DM_DRAGOVER (the DRAGINFO and
//!   the pointer position), and the source DM_DRAGOVERNOTIFY with the
//!   answer; leaving a window sends DM_DRAGLEAVE; F1 posts DM_DROPHELP.
//!   At the end a window that answered DOR_DROP gets DM_DROP (sent), with
//!   usOperation set to the operation it chose if the user chose none,
//!   and DrgDrag returns it. Ctrl, Shift and Ctrl+Shift held down make
//!   the operation DO_COPY, DO_MOVE and DO_LINK (at the start, and as
//!   they change).
//! - The drag images (icons, bitmaps, polygons; stretched, transparent;
//!   halftoned for DO_COPY) go with the pointer: PMWIN makes them and the
//!   pointer one shape (`Rs2DragPointer`, at most 64 pixels square; IBM
//!   draws the images on the screen next to the pointer) and holds it
//!   while the drag runs (`Rs2HoldPointer`). The pointer in it is the
//!   arrow over windows that take the drop, the "illegal" pointer
//!   elsewhere; DrgSetDragPointer replaces it while the pointer stays over
//!   the same window.
//! - Lazy drag (pick up and drop) and DrgDragFiles/DrgAcceptDroppedFiles:
//!   lazy.rs and files.rs.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod exports;
mod files;
mod lazy;

pub use files::{DrgAcceptDroppedFiles, DrgDragFiles};
pub use lazy::{DrgCancelLazyDrag, DrgLazyDrag, DrgLazyDrop};

use alloc::vec::Vec;
use pmwin::drag as rmf;

unsafe extern "C" {
    #[link_name = "PMWIN.757"]
    fn WinGetPS(hwnd: u32) -> u32;
    #[link_name = "PMWIN.848"]
    fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.919"]
    fn WinPostMsg(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.918"]
    fn WinPeekMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.830"]
    fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.700"]
    fn WinAddAtom(t: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.721"]
    fn WinDeleteAtom(t: u32, a: u32) -> u32;
    #[link_name = "PMWIN.802"]
    fn WinQueryAtomName(t: u32, a: u32, buf: *mut u8, cch: u32) -> u32;
    #[link_name = "PMWIN.801"]
    fn WinQueryAtomLength(t: u32, a: u32) -> u32;
    #[link_name = "PMWIN.823"]
    fn WinQueryPointerPos(desk: u32, p: *mut [i32; 2]) -> u32;
    #[link_name = "PMWIN.900"]
    fn WinWindowFromPoint(parent: u32, p: *const [i32; 2], children: u32) -> u32;
    #[link_name = "PMWIN.852"]
    fn WinSetCapture(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.828"]
    fn WinQuerySysPointer(desk: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMWIN.752"]
    fn WinGetKeyState(desk: u32, vk: i32) -> i32;
    #[link_name = "PMWIN.838"]
    fn WinQueryWindowProcess(h: u32, pid: *mut u32, tid: *mut u32) -> u32;
    #[link_name = "PMWIN.772"]
    fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.753"]
    fn WinGetLastError(hab: u32) -> u32;
    #[link_name = "PMWIN.977"]
    fn WinSetErrorInfo(e: u32, fs: u32) -> u32;
    #[link_name = "DOSCALLS.302"]
    fn DosGetSharedMem(pb: *mut u8, flags: u32) -> u32;
    #[link_name = "PMWIN.9009"]
    fn Rs2DragPointer(base: u32, pdimg: *const u8, n: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.9017"]
    fn Rs2HoldPointer(h: u32) -> u32;
    #[link_name = "PMWIN.727"]
    fn WinDestroyPointer(h: u32) -> u32;
    #[link_name = "DOSCALLS.303"]
    fn DosGiveSharedMem(pb: *mut u8, pid: u32, flags: u32) -> u32;
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct QMsg {
    hwnd: u32,
    msg: u32,
    mp1: u32,
    mp2: u32,
    time: u32,
    x: i32,
    y: i32,
    reserved: u32,
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(hmod: u32, flag: u32) -> u32 {
    lazy::init_term(hmod, flag);
    1
}

const HWND_DESKTOP: u32 = 1;

// Messages.
const WM_QUIT: u32 = 0x002a;
const WM_MOUSEMOVE: u32 = 0x0070;
const WM_BUTTON1DOWN: u32 = 0x0071;
const WM_BUTTON1UP: u32 = 0x0072;
const WM_BUTTON3DBLCLK: u32 = 0x0079;
const WM_BUTTON2UP: u32 = 0x0075;
const WM_BUTTON3UP: u32 = 0x0078;
const WM_CHAR: u32 = 0x007a;
const WM_ENDDRAG: u32 = 0x0421;
const DM_DROP: u32 = 0x032f;
const DM_DRAGOVER: u32 = 0x032e;
const DM_DRAGLEAVE: u32 = 0x032d;
const DM_DROPHELP: u32 = 0x032c;
const DM_RENDER: u32 = 0x0329;
const DM_RENDERPREPARE: u32 = 0x0327;
const DM_DRAGOVERNOTIFY: u32 = 0x0321;

// Keys.
const VK_BUTTON1: u32 = 1;
const VK_BUTTON2: u32 = 2;
const VK_BUTTON3: u32 = 3;
const VK_ENDDRAG: u32 = 0x38;
const VK_SHIFT: i32 = 9;
const VK_CTRL: i32 = 10;
const VK_ALT: i32 = 11;
const VK_ALTGRAF: i32 = 12;
const VK_ESC: u32 = 0x0f;
const VK_F1: u32 = 0x20;
const KC_VIRTUALKEY: u32 = 0x0002;
const KC_KEYUP: u32 = 0x0040;
const KC_PREVDOWN: u32 = 0x0080;

const SPTR_ILLEGAL: i32 = 18;
const SPTR_ARROW: i32 = 1;

const DOR_NODROP: u16 = 0;
const DOR_DROP: u16 = 1;
const DOR_NODROPOP: u16 = 2;

const DRG_ICON: u32 = 1;

// Errors (WinSetErrorInfo).
const PMERR_INVALID_PARAMETER: u32 = 0x1645;
const PMERR_INVALID_HWND: u32 = 0x1001;
const PMERR_DRAG_IN_PROGRESS: u32 = 0x1f01;
const PMERR_SOURCE_SAME_AS_TARGET: u32 = 0x1502;
const PMERR_ACCESS_DENIED: u32 = 0x150d;
const PMERR_MEMORY: u32 = 0x203e;

fn error(e: u32) {
    unsafe { WinSetErrorInfo(e, 0x6000) };
}

/// PAG_READ | PAG_WRITE | PAG_EXECUTE | PAG_COMMIT | OBJ_GETTABLE | OBJ_GIVEABLE.
const SHARED: u32 = 0x317;

fn rd(p: u32, off: u32) -> u32 {
    unsafe { ((p + off) as *const u32).read_unaligned() }
}

fn wr(p: u32, off: u32, v: u32) {
    unsafe { ((p + off) as *mut u32).write_unaligned(v) }
}

fn rd16(p: u32, off: u32) -> u16 {
    unsafe { ((p + off) as *const u16).read_unaligned() }
}

fn wr16(p: u32, off: u32, v: u16) {
    unsafe { ((p + off) as *mut u16).write_unaligned(v) }
}

fn pid_of(h: u32) -> u32 {
    let (mut pid, mut tid) = (0, 0);
    unsafe { WinQueryWindowProcess(h, &mut pid, &mut tid) };
    pid
}

fn my_pid() -> u32 {
    let mut tib = core::ptr::null_mut();
    let mut pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    if pib.is_null() { 0 } else { unsafe { (*pib).pid } }
}

fn pointer_pos() -> [i32; 2] {
    let mut p = [0i32; 2];
    unsafe { WinQueryPointerPos(HWND_DESKTOP, &mut p) };
    p
}

fn cstr(p: u32) -> Vec<u8> {
    let mut v = Vec::new();
    if p == 0 {
        return v;
    }
    let mut q = p as *const u8;
    unsafe {
        while *q != 0 && v.len() < 4096 {
            v.push(*q);
            q = q.add(1);
        }
    }
    v
}

/// Copy a string to a caller's buffer of `cb` bytes (NUL included);
/// returns the bytes copied without the NUL.
fn copy_out(s: &[u8], cb: u32, buf: *mut u8) -> u32 {
    if buf.is_null() || cb == 0 {
        return 0;
    }
    let n = s.len().min(cb as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as u32
}

// ---- String handles ----

fn atoms() -> u32 {
    unsafe { WinQuerySystemAtomTable() }
}

/// DrgAddStrHandle(psz): a handle for a string (the same string gets the
/// same handle, counted).
#[unsafe(no_mangle)]
pub extern "C" fn DrgAddStrHandle(psz: *const u8) -> u32 {
    if psz.is_null() {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    let s = cstr(psz as u32);
    let mut name = Vec::with_capacity(s.len() + 2);
    if matches!(s.first(), Some(b'!' | b'#')) {
        name.push(0xff);
    }
    name.extend_from_slice(&s);
    name.push(0);
    let a = unsafe { WinAddAtom(atoms(), name.as_ptr()) };
    if a == 0 {
        error(PMERR_INVALID_PARAMETER);
    }
    a
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgDeleteStrHandle(h: u32) -> u32 {
    let ok = h != 0 && unsafe { WinDeleteAtom(atoms(), h) } == 0;
    if !ok {
        error(PMERR_INVALID_PARAMETER);
    }
    ok as u32
}

/// A handle's string (without the escape byte).
fn str_name(h: u32) -> Option<Vec<u8>> {
    if h == 0 {
        return None;
    }
    let n = unsafe { WinQueryAtomLength(atoms(), h) };
    if n == 0 {
        return None;
    }
    let mut buf = alloc::vec![0u8; n as usize + 1];
    let got = unsafe { WinQueryAtomName(atoms(), h, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    buf.truncate(got);
    if buf.first() == Some(&0xff) {
        buf.remove(0);
    }
    Some(buf)
}

/// DrgQueryStrName(hstr, cbBuffer, pBuffer): bytes copied (without the
/// NUL).
#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryStrName(h: u32, cb: u32, buf: *mut u8) -> u32 {
    if cb < 2 || buf.is_null() {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    match str_name(h) {
        Some(s) => copy_out(&s, cb, buf),
        None => {
            unsafe { *buf = 0 };
            error(PMERR_INVALID_PARAMETER);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryStrNameLen(h: u32) -> u32 {
    str_name(h).map_or(0, |s| s.len() as u32)
}

// ---- Presentation spaces for target emphasis ----

#[unsafe(no_mangle)]
pub extern "C" fn DrgGetPS(hwnd: u32) -> u32 {
    unsafe { WinGetPS(hwnd) }
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgReleasePS(hps: u32) -> u32 {
    unsafe { WinReleasePS(hps) }
}

// ---- DRAGINFO ----

const DI_MAGIC: u32 = 0xf00e_0e0f;
const DX_MAGIC: u32 = 0xe0e0_0f0f;
/// The header in front of a DRAGINFO.
const DI_HEAD: u32 = 0x2c;
const DRAGITEM: u32 = 0x24;
const DRAGTRANSFER: u32 = 0x1c;

// DRAGINFO fields.
const DI_CB: u32 = 0;
const DI_CBITEM: u32 = 4;
const DI_OP: u32 = 6;
const DI_SOURCE: u32 = 8;
const DI_X: u32 = 12;
const DI_Y: u32 = 14;
const DI_COUNT: u32 = 16;
const DI_ITEMS: u32 = 20;

// DRAGITEM fields.
const IT_TYPE: u32 = 8;
const IT_RMF: u32 = 12;

fn items(pdinfo: u32) -> u32 {
    rd(pdinfo - DI_HEAD, 4)
}

fn count(pdinfo: u32) -> u32 {
    rd16(pdinfo, DI_COUNT) as u32
}

/// Whether `p` is a DRAGINFO this process can read.
fn valid(p: u32) -> bool {
    if p < 0x1000 + DI_HEAD {
        return false;
    }
    let (mut cb, mut fl) = (DI_HEAD + DI_ITEMS, 0u32);
    let base = (p - DI_HEAD) as *mut u8;
    // PAG_READ and committed.
    unsafe { os2::DosQueryMem(base, &mut cb, &mut fl) == 0 && fl & 1 != 0 && fl & 0x10 != 0 && rd(p - DI_HEAD, 0) == DI_MAGIC }
}

/// DrgAllocDraginfo(cditem)
#[unsafe(no_mangle)]
pub extern "C" fn DrgAllocDraginfo(n: u32) -> u32 {
    if n == 0 || n > 0xffff {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    let size = n * DRAGITEM + 0x40;
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), size, SHARED) } != 0 || p.is_null() {
        error(PMERR_MEMORY);
        return 0;
    }
    unsafe { core::ptr::write_bytes(p, 0, size as usize) };
    let base = p as u32;
    let di = base + DI_HEAD;
    wr(base, 0, DI_MAGIC);
    wr(base, 4, di + DI_ITEMS);
    wr(di, DI_CB, size - DI_HEAD);
    wr16(di, DI_CBITEM, DRAGITEM as u16);
    wr16(di, DI_OP, rmf::DO_DEFAULT);
    let pt = pointer_pos();
    wr16(di, DI_X, pt[0] as u16);
    wr16(di, DI_Y, pt[1] as u16);
    wr16(di, DI_COUNT, n as u16);
    di
}

/// DrgReallocDraginfo(pdinfoOld, cditem): a new DRAGINFO with the old
/// one's fields and items (as many as fit); the old one is freed.
#[unsafe(no_mangle)]
pub extern "C" fn DrgReallocDraginfo(old: u32, n: u32) -> u32 {
    if old != 0 && !valid(old) {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    let new = DrgAllocDraginfo(n);
    if new == 0 || old == 0 {
        return new;
    }
    wr16(new, DI_OP, rd16(old, DI_OP));
    wr(new, DI_SOURCE, rd(old, DI_SOURCE));
    let keep = count(old).min(n);
    unsafe { core::ptr::copy_nonoverlapping(items(old) as *const u8, items(new) as *mut u8, (keep * DRAGITEM) as usize) };
    free_draginfo(old);
    new
}

/// DrgAccessDraginfo(pdinfo): the target gets at a DRAGINFO.
#[unsafe(no_mangle)]
pub extern "C" fn DrgAccessDraginfo(p: u32) -> u32 {
    if p < DI_HEAD {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    let ok = unsafe { DosGetSharedMem((p - DI_HEAD) as *mut u8, 3) } == 0;
    if !ok {
        error(PMERR_ACCESS_DENIED);
    }
    ok as u32
}

fn free_draginfo(p: u32) -> bool {
    unsafe { os2::DosFreeMem((p - DI_HEAD) as *mut u8) == 0 }
}

/// DrgFreeDraginfo(pdinfo): a process lets go of a DRAGINFO (it goes
/// away when the last one has). The source cannot while its drag runs.
#[unsafe(no_mangle)]
pub extern "C" fn DrgFreeDraginfo(p: u32) -> u32 {
    if !valid(p) {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    if (drag().is_some() || lazy::draginfo() == p) && pid_of(rd(p, DI_SOURCE)) == my_pid() {
        error(PMERR_SOURCE_SAME_AS_TARGET);
        return 0;
    }
    free_draginfo(p) as u32
}

/// DrgPushDraginfo(pdinfo, hwndDest): give a DRAGINFO to the process of
/// a window.
#[unsafe(no_mangle)]
pub extern "C" fn DrgPushDraginfo(p: u32, dest: u32) -> u32 {
    if p < DI_HEAD {
        return 0;
    }
    let pid = pid_of(dest);
    if pid == 0 {
        error(PMERR_INVALID_HWND);
        return 0;
    }
    if pid == my_pid() {
        return 1;
    }
    let rc = unsafe { DosGiveSharedMem((p - DI_HEAD) as *mut u8, pid, 3) };
    if rc != 0 {
        error(if rc == 8 { PMERR_MEMORY } else { PMERR_ACCESS_DENIED });
    }
    (rc == 0) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDragitemCount(p: u32) -> u32 {
    if p == 0 { 0 } else { count(p) }
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDragitemPtr(p: u32, i: u32) -> u32 {
    if p == 0 || i >= count(p) {
        return 0;
    }
    items(p) + i * DRAGITEM
}

/// DrgQueryDragitem(pdinfo, cbBuffer, pditem, iItem)
#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDragitem(p: u32, cb: u32, out: u32, i: u32) -> u32 {
    if p == 0 || out == 0 || i >= count(p) {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    let n = cb.min(DRAGITEM);
    unsafe { core::ptr::copy_nonoverlapping((items(p) + i * DRAGITEM) as *const u8, out as *mut u8, n as usize) };
    1
}

/// DrgSetDragitem(pdinfo, pditem, cbBuffer, iItem)
#[unsafe(no_mangle)]
pub extern "C" fn DrgSetDragitem(p: u32, item: u32, cb: u32, i: u32) -> u32 {
    if p == 0 || item == 0 || i >= count(p) {
        return 0;
    }
    let n = cb.min(DRAGITEM);
    unsafe { core::ptr::copy_nonoverlapping(item as *const u8, (items(p) + i * DRAGITEM) as *mut u8, n as usize) };
    1
}

/// DrgDeleteDraginfoStrHandles(pdinfo): the string handles of every item
/// (each handle once).
#[unsafe(no_mangle)]
pub extern "C" fn DrgDeleteDraginfoStrHandles(p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let mut seen: Vec<u32> = Vec::new();
    for i in 0..count(p) {
        let it = items(p) + i * DRAGITEM;
        for off in [8, 12, 16, 20, 24] {
            let h = rd(it, off);
            if h != 0 && !seen.contains(&h) {
                seen.push(h);
            }
        }
    }
    let mut ok = true;
    for h in seen {
        ok &= unsafe { WinDeleteAtom(atoms(), h) } == 0;
    }
    ok as u32
}

/// The DRAGINFO a DRAGITEM belongs to (by the header in front of it).
#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDraginfoPtrFromDragitem(pditem: u32) -> u32 {
    find_head(pditem, DI_MAGIC).map_or(0, |b| b + DI_HEAD)
}

/// The block that holds `p`: the page with the magic at its start, at
/// or below `p`'s page.
fn find_head(p: u32, magic: u32) -> Option<u32> {
    let mut b = p & !0xfff;
    for _ in 0..64 {
        if b == 0 {
            return None;
        }
        let (mut cb, mut fl) = (4u32, 0u32);
        if unsafe { os2::DosQueryMem(b as *mut u8, &mut cb, &mut fl) } != 0 || fl & 0x11 != 0x11 {
            return None;
        }
        if rd(b, 0) == magic {
            return Some(b);
        }
        b -= 0x1000;
    }
    None
}

// ---- DRAGTRANSFER ----

const DX_HEAD: u32 = 16;

/// DrgAllocDragtransfer(cdxfer)
#[unsafe(no_mangle)]
pub extern "C" fn DrgAllocDragtransfer(n: u32) -> u32 {
    if n == 0 || n > 0xffff {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    let size = DX_HEAD + n * DRAGTRANSFER;
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), size, SHARED) } != 0 || p.is_null() {
        error(PMERR_MEMORY);
        return 0;
    }
    unsafe { core::ptr::write_bytes(p, 0, size as usize) };
    let base = p as u32;
    wr(base, 0, DX_MAGIC);
    wr(base, 4, n);
    wr(base, 8, my_pid());
    base + DX_HEAD
}

/// DrgFreeDragtransfer(pdxfer): this process lets go of the
/// DRAGTRANSFER block (it goes away when the last one has).
#[unsafe(no_mangle)]
pub extern "C" fn DrgFreeDragtransfer(p: u32) -> u32 {
    let Some(b) = find_head(p, DX_MAGIC) else {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    };
    (unsafe { os2::DosFreeMem(b as *mut u8) } == 0) as u32
}

/// Give a DRAGTRANSFER's block to the process of the window it goes to;
/// fsReply starts at 0.
fn give_transfer(h: u32, p: u32) {
    if p == 0 {
        return;
    }
    if let Some(b) = find_head(p, DX_MAGIC) {
        wr16(p, 0x1a, 0);
        let pid = pid_of(h);
        if pid != 0 && pid != my_pid() {
            unsafe { DosGiveSharedMem(b as *mut u8, pid, 3) };
        }
    }
}

/// DrgSendTransferMsg(hwnd, msg, mp1, mp2): DM_RENDER and
/// DM_RENDERPREPARE give the DRAGTRANSFER (mp1) to the receiver first.
#[unsafe(no_mangle)]
pub extern "C" fn DrgSendTransferMsg(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == DM_RENDER || msg == DM_RENDERPREPARE {
        give_transfer(hwnd, mp1);
    }
    unsafe { WinSendMsg(hwnd, msg, mp1, mp2) }
}

/// DrgPostTransferMsg(hwnd, msg, pdxfer, fl, ulReserved, fRetry): posts,
/// retrying (and dispatching this thread's messages) while the
/// receiver's queue is full if fRetry.
#[unsafe(no_mangle)]
pub extern "C" fn DrgPostTransferMsg(hwnd: u32, msg: u32, pdxfer: u32, fl: u32, _rsvd: u32, retry: u32) -> u32 {
    give_transfer(hwnd, pdxfer);
    let mut retry = retry != 0;
    loop {
        let ok = unsafe { WinPostMsg(hwnd, msg, pdxfer, fl) };
        // PMERR_QUEUE_FULL
        if ok != 0 || !retry || unsafe { WinGetLastError(0) } & 0xffff != 0x1038 {
            return ok;
        }
        let mut q = QMsg::default();
        while unsafe { WinPeekMsg(0, &mut q, 0, 0, 0, 1) } != 0 {
            if q.msg == WM_QUIT {
                unsafe { WinPostMsg(q.hwnd, WM_QUIT, q.mp1, q.mp2) };
                retry = false;
                break;
            }
            unsafe { WinDispatchMsg(0, &q) };
        }
        if retry {
            unsafe { os2::DosSleep(1000) };
        }
    }
}

// ---- Rendering mechanisms, formats and types ----

fn item_str(pditem: u32, off: u32) -> Vec<u8> {
    if pditem == 0 { Vec::new() } else { str_name(rd(pditem, off)).unwrap_or_default() }
}

fn arg(p: u32) -> Option<Vec<u8>> {
    (p != 0).then(|| cstr(p))
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryNativeRMF(pditem: u32, cb: u32, buf: *mut u8) -> u32 {
    match rmf::native_rmf(&item_str(pditem, IT_RMF)) {
        Some(s) => {
            copy_out(&s, cb, buf);
            1
        }
        None => {
            copy_out(b"", cb, buf);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryNativeRMFLen(pditem: u32) -> u32 {
    rmf::native_rmf(&item_str(pditem, IT_RMF)).map_or(0, |s| s.len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryTrueType(pditem: u32, cb: u32, buf: *mut u8) -> u32 {
    let t = item_str(pditem, IT_TYPE);
    match rmf::true_type(&t) {
        Some(s) => {
            copy_out(s, cb, buf);
            1
        }
        None => {
            copy_out(b"", cb, buf);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryTrueTypeLen(pditem: u32) -> u32 {
    let t = item_str(pditem, IT_TYPE);
    rmf::true_type(&t).map_or(0, |s| s.len() as u32)
}

/// DrgVerifyNativeRMF(pditem, pszRMF)
#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyNativeRMF(pditem: u32, prmf: u32) -> u32 {
    let Some(r) = arg(prmf) else { return 0 };
    rmf::native_is(&item_str(pditem, IT_RMF), &r) as u32
}

/// DrgVerifyRMF(pditem, pszMech, pszFmt): NULL matches any.
#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyRMF(pditem: u32, mech: u32, fmt: u32) -> u32 {
    let (m, f) = (arg(mech), arg(fmt));
    rmf::rmf_has(&item_str(pditem, IT_RMF), m.as_deref(), f.as_deref()) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyTrueType(pditem: u32, ty: u32) -> u32 {
    let Some(want) = arg(ty) else { return 0 };
    let t = item_str(pditem, IT_TYPE);
    rmf::true_type(&t).is_some_and(|x| rmf::true_type(&want) == Some(x)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyType(pditem: u32, ty: u32) -> u32 {
    let Some(want) = arg(ty) else { return 0 };
    rmf::has_type(&item_str(pditem, IT_TYPE), &want) as u32
}

/// DrgVerifyTypeSet(pditem, pszType, cbMatch, pszMatch): the first type
/// of the list that the item has.
#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyTypeSet(pditem: u32, ty: u32, cb: u32, out: *mut u8) -> u32 {
    let Some(want) = arg(ty) else { return 0 };
    let t = item_str(pditem, IT_TYPE);
    match rmf::first_match(&t, &want) {
        Some(m) => {
            copy_out(m, cb, out);
            1
        }
        None => 0,
    }
}

// ---- Dragging ----

/// The drag this process (its one window thread doing DrgDrag) runs.
struct Drag {
    pdinfo: u32,
    target: u32,
    /// What ends it: VK_BUTTONn or VK_ENDDRAG.
    term: u32,
    /// The target takes a drop (DOR_DROP), and with which operation.
    droppable: bool,
    reply_op: u16,
    /// The target never takes one (DOR_NEVERDROP; or not this default
    /// operation): no more DM_DRAGOVER until the pointer leaves it.
    never: bool,
    /// The drag images (DRAGIMAGEs), a pointer the target set, and the
    /// shape shown (with what it was made of).
    images: Vec<u8>,
    pointer: u32,
    shape: Shape,
    done: bool,
}

/// The pointer and the images shown together (a PMWIN pointer).
#[derive(Default)]
struct Shape {
    hptr: u32,
    made_of: (u32, bool, u32),
    generation: u32,
}

const DRAGIMAGE: usize = 0x18;

fn copy_images(pdimg: u32, n: u32) -> Vec<u8> {
    if pdimg == 0 {
        return Vec::new();
    }
    let n = n.min(256) as usize;
    unsafe { core::slice::from_raw_parts(pdimg as *const u8, n * DRAGIMAGE) }.to_vec()
}

impl Shape {
    /// Show `base` with `images` (halftoned for a copy), made again only
    /// when something changed.
    fn show(&mut self, base: u32, images: &[u8], halftone: bool) {
        let key = (base, halftone, self.generation);
        if self.hptr != 0 && self.made_of == key {
            return;
        }
        let old = self.hptr;
        self.hptr = unsafe { Rs2DragPointer(base, images.as_ptr(), (images.len() / DRAGIMAGE) as u32, halftone as u32) };
        self.made_of = key;
        unsafe { Rs2HoldPointer(if self.hptr != 0 { self.hptr } else { base }) };
        if old != 0 {
            unsafe { WinDestroyPointer(old) };
        }
    }

    fn changed(&mut self) {
        self.generation += 1;
    }

    fn end(&mut self) {
        unsafe { Rs2HoldPointer(0) };
        if self.hptr != 0 {
            unsafe { WinDestroyPointer(self.hptr) };
        }
        self.hptr = 0;
    }
}

/// The images are drawn halftoned for a copy (IBM's vfDrawHalftone).
fn halftone(op: u16, reply_op: u16) -> bool {
    op == rmf::DO_COPY || (op == rmf::DO_DEFAULT && reply_op == rmf::DO_COPY)
}

static mut DRAG: Option<Drag> = None;

fn drag() -> Option<&'static mut Drag> {
    unsafe { (*core::ptr::addr_of_mut!(DRAG)).as_mut() }
}

fn key_down(vk: i32) -> bool {
    (unsafe { WinGetKeyState(HWND_DESKTOP, vk) } & 0x8000) != 0
}

/// The operation from the keys down now (`changed`: a key going down or
/// up that the key state may not show yet).
fn keys_operation(changed: Option<(u32, bool)>) -> u16 {
    let mut ctrl = key_down(VK_CTRL);
    let mut shift = key_down(VK_SHIFT);
    let mut alt = key_down(VK_ALT) || key_down(VK_ALTGRAF);
    if let Some((vk, down)) = changed {
        match vk as i32 {
            VK_CTRL => ctrl = down,
            VK_SHIFT => shift = down,
            VK_ALT | VK_ALTGRAF => alt = down,
            _ => {}
        }
    }
    rmf::operation(ctrl, shift, alt)
}

fn send_dragover() {
    let Some(d) = drag() else { return };
    let (di, target) = (d.pdinfo, d.target);
    let pos = rd(di, DI_X);
    let reply = if target != 0 { unsafe { WinSendMsg(target, DM_DRAGOVER, di, pos) } } else { 0 };
    let source = rd(di, DI_SOURCE);
    unsafe { WinSendMsg(source, DM_DRAGOVERNOTIFY, di, reply) };
    let Some(d) = drag() else { return };
    let (code, op) = (reply as u16, (reply >> 16) as u16);
    let user_op = rd16(di, DI_OP);
    d.droppable = false;
    match code {
        DOR_DROP => {
            // With the default operation the target must name one it can do.
            if user_op == rmf::DO_DEFAULT && !matches!(op, rmf::DO_COPY | rmf::DO_MOVE | rmf::DO_LINK | 0x80) && op < rmf::DO_UNKNOWN {
                d.never = true;
                d.reply_op = 0;
            } else {
                d.droppable = true;
                d.reply_op = op;
            }
        }
        DOR_NODROP | DOR_NODROPOP => {}
        _ => d.never = true,
    }
}

fn show_pointer() {
    let Some(d) = drag() else { return };
    let base = if d.pointer != 0 {
        d.pointer
    } else {
        unsafe { WinQuerySysPointer(HWND_DESKTOP, if d.droppable { SPTR_ARROW } else { SPTR_ILLEGAL }, 0) }
    };
    let ht = halftone(rd16(d.pdinfo, DI_OP), d.reply_op);
    d.shape.show(base, &d.images, ht);
}

fn mouse_move() {
    let p = pointer_pos();
    let under = unsafe { WinWindowFromPoint(HWND_DESKTOP, &p, 1) };
    let Some(d) = drag() else { return };
    wr16(d.pdinfo, DI_X, p[0] as u16);
    wr16(d.pdinfo, DI_Y, p[1] as u16);
    if under != d.target {
        let (old, di) = (d.target, d.pdinfo);
        d.target = under;
        d.never = false;
        d.droppable = false;
        d.pointer = 0;
        if old != 0 {
            unsafe { WinSendMsg(old, DM_DRAGLEAVE, di, 0) };
        }
    }
    if drag().is_some_and(|d| !d.never) {
        send_dragover();
    }
    show_pointer();
}

fn char_msg(mp1: u32, mp2: u32) {
    let fs = mp1 & 0xffff;
    if fs & KC_PREVDOWN != 0 && fs & KC_KEYUP == 0 {
        return;
    }
    let vk = mp2 >> 16;
    let Some(d) = drag() else { return };
    if fs & KC_VIRTUALKEY != 0 && fs & KC_KEYUP == 0 && (vk == VK_ESC || vk == VK_F1) {
        if vk == VK_F1 && d.target != 0 {
            let di = d.pdinfo;
            if pid_of(d.target) != my_pid() {
                DrgPushDraginfo(di, d.target);
            }
            unsafe { WinPostMsg(d.target, DM_DROPHELP, di, 0) };
        }
        d.droppable = false;
        d.done = true;
        return;
    }
    let changed = (fs & KC_VIRTUALKEY != 0).then_some((vk, fs & KC_KEYUP == 0));
    let op = keys_operation(changed);
    if op != rd16(d.pdinfo, DI_OP) {
        wr16(d.pdinfo, DI_OP, op);
        if !d.never {
            send_dragover();
        }
        show_pointer();
    }
}

fn button_up(msg: u32) {
    let Some(d) = drag() else { return };
    let ends = match d.term {
        VK_BUTTON1 => msg == WM_BUTTON1UP,
        VK_BUTTON2 => msg == WM_BUTTON2UP,
        VK_BUTTON3 => msg == WM_BUTTON3UP,
        // PM follows WM_BUTTON2UP with WM_ENDDRAG; either ends it.
        VK_ENDDRAG => msg == WM_ENDDRAG || msg == WM_BUTTON2UP,
        _ => false,
    };
    if ends {
        d.done = true;
    }
}

/// DrgDrag(hwndSource, pdinfo, pdimg, cdimg, vkTerminate, pRsvd): the
/// window that took the drop (it got DM_DROP), or 0.
#[unsafe(no_mangle)]
pub extern "C" fn DrgDrag(src: u32, pdinfo: u32, pdimg: u32, cdimg: u32, vk: u32, _rsvd: u32) -> u32 {
    if pdinfo == 0 || !valid(pdinfo) {
        error(PMERR_INVALID_PARAMETER);
        return 0;
    }
    if drag().is_some() || lazy::active() {
        error(PMERR_DRAG_IN_PROGRESS);
        return 0;
    }
    let hab = unsafe { WinQueryAnchorBlock(src) };
    let cur = rd(pdinfo, DI_SOURCE);
    if cur == 0 || unsafe { WinIsWindow(hab, cur) } == 0 {
        if unsafe { WinIsWindow(hab, src) } == 0 {
            error(PMERR_INVALID_HWND);
            return 0;
        }
        wr(pdinfo, DI_SOURCE, src);
    }
    let images = copy_images(pdimg, cdimg);
    let p = pointer_pos();
    wr16(pdinfo, DI_X, p[0] as u16);
    wr16(pdinfo, DI_Y, p[1] as u16);
    wr16(pdinfo, DI_OP, keys_operation(None));
    let target = unsafe { WinWindowFromPoint(HWND_DESKTOP, &p, 1) };
    unsafe {
        DRAG = Some(Drag { pdinfo, target, term: vk, droppable: false, reply_op: 0, never: false, images, pointer: 0, shape: Shape::default(), done: false });
        WinSetCapture(HWND_DESKTOP, src);
    }
    send_dragover();
    show_pointer();
    let mut quit = None;
    let mut q = QMsg::default();
    while drag().is_some_and(|d| !d.done) {
        if unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } == 0 {
            quit = Some(q);
            break;
        }
        match q.msg {
            WM_MOUSEMOVE => mouse_move(),
            WM_BUTTON1UP | WM_BUTTON2UP | WM_BUTTON3UP | WM_ENDDRAG => button_up(q.msg),
            WM_BUTTON1DOWN..=WM_BUTTON3DBLCLK => {}
            WM_CHAR => char_msg(q.mp1, q.mp2),
            _ => unsafe {
                WinDispatchMsg(hab, &q);
            },
        }
        // The source window went away: the drag ends.
        if unsafe { WinIsWindow(hab, src) } == 0 {
            if let Some(d) = drag() {
                d.droppable = false;
                d.done = true;
            }
        }
    }
    unsafe { WinSetCapture(HWND_DESKTOP, 0) };
    let d = drag().unwrap();
    d.shape.end();
    let (target, droppable, reply_op) = (d.target, d.droppable && quit.is_none(), d.reply_op);
    let result = if droppable && target != 0 {
        let op = rd16(pdinfo, DI_OP);
        if op == rmf::DO_DEFAULT || (op == rmf::DO_UNKNOWN && reply_op > rmf::DO_UNKNOWN) {
            wr16(pdinfo, DI_OP, reply_op);
        }
        unsafe { WinSendMsg(target, DM_DROP, pdinfo, 0) };
        target
    } else {
        if target != 0 {
            unsafe { WinSendMsg(target, DM_DRAGLEAVE, pdinfo, 0) };
        }
        0
    };
    unsafe { DRAG = None };
    if let Some(q) = quit {
        unsafe { WinPostMsg(q.hwnd, WM_QUIT, q.mp1, q.mp2) };
    }
    result
}

/// DrgSetDragPointer(pdinfo, hptr): the pointer while over this target.
#[unsafe(no_mangle)]
pub extern "C" fn DrgSetDragPointer(pdinfo: u32, hptr: u32) -> u32 {
    match drag() {
        Some(d) if d.pdinfo == pdinfo => {
            d.pointer = hptr;
            1
        }
        _ => 0,
    }
}

/// DrgSetDragImage(pdinfo, pdimg, cdimg, pRsvd): new images for the drag.
#[unsafe(no_mangle)]
pub extern "C" fn DrgSetDragImage(pdinfo: u32, pdimg: u32, cdimg: u32, _rsvd: u32) -> u32 {
    match drag() {
        Some(d) if d.pdinfo == pdinfo && pdimg != 0 && cdimg > 0 => {
            d.images = copy_images(pdimg, cdimg);
            d.shape.changed();
            show_pointer();
            1
        }
        _ => lazy::set_images(pdinfo, pdimg, cdimg) as u32,
    }
}

/// DrgQueryDragStatus: DGS_DRAGINPROGRESS while this process drags,
/// DGS_LAZYDRAGINPROGRESS while a lazy drag runs (in any process).
#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDragStatus() -> u32 {
    if drag().is_some() {
        1
    } else if lazy::active() {
        2
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDraginfoPtr(_rsvd: u32) -> u32 {
    drag().map_or_else(lazy::draginfo, |d| d.pdinfo)
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDraginfoPtrFromHwnd(h: u32) -> u32 {
    match drag() {
        Some(d) if rd(d.pdinfo, DI_SOURCE) == h => d.pdinfo,
        _ => Some(lazy::draginfo()).filter(|&p| p != 0 && rd(p, DI_SOURCE) == h).unwrap_or(0),
    }
}

// ---- Not done ----

