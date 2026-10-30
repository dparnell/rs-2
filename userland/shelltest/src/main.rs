//! Self-checks of what a desktop shell needs from PMWIN: pointers and
//! icons (system pointers, pointer info and bitmaps, creating and
//! destroying, the mouse pointer's shape, file icons through PMWP),
//! frames (icons, minimize and restore, the minimized window viewer)
//! and the container (records, insertion and queries, emphasis,
//! notifications, sorting, views, the tree, searching, removal). Each
//! check prints PASS or FAIL, then "shelltest done: N failure(s)"; the
//! window with a container of icons stays up until it is closed
//! (`SHELLTST QUIT` ends at once). The exit code is N.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::vec::Vec;
use core::ptr::null;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

os2::entry!(main);

#[repr(C)]
#[derive(Default)]
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

type WndProc = extern "C" fn(u32, u32, u32, u32) -> u32;

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, fcf: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: u32, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.918"]
    fn WinPeekMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.837"]
    fn WinQueryWindowPos(h: u32, swp: *mut i32) -> u32;
    #[link_name = "PMWIN.775"]
    fn WinIsWindowVisible(h: u32) -> u32;
    #[link_name = "PMWIN.843"]
    fn WinQueryWindowULong(h: u32, i: i32) -> u32;
    #[link_name = "PMWIN.929"]
    fn WinSubclassWindow(h: u32, p: WndProc) -> u32;
    #[link_name = "PMWIN.860"]
    fn WinSetFocus(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.829"]
    fn WinQuerySysValue(desk: u32, i: i32) -> i32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, r: *mut i32) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, r: *const i32, c: i32) -> u32;
    #[link_name = "PMWIN.828"]
    fn WinQuerySysPointer(desk: u32, id: i32, copy: u32) -> u32;
    #[link_name = "PMWIN.822"]
    fn WinQueryPointerInfo(h: u32, pi: *mut u32) -> u32;
    #[link_name = "PMWIN.727"]
    fn WinDestroyPointer(h: u32) -> u32;
    #[link_name = "PMWIN.942"]
    fn WinCreatePointerIndirect(desk: u32, pi: *const u32) -> u32;
    #[link_name = "PMWIN.717"]
    fn WinCreatePointer(desk: u32, hbm: u32, ptr: u32, x: i32, y: i32) -> u32;
    #[link_name = "PMWIN.866"]
    fn WinSetPointer(desk: u32, h: u32) -> u32;
    #[link_name = "PMWIN.821"]
    fn WinQueryPointer(desk: u32) -> u32;
    #[link_name = "PMWIN.732"]
    fn WinDrawPointer(hps: u32, x: i32, y: i32, h: u32, fs: u32) -> u32;
    #[link_name = "PMGPI.573"]
    fn GpiQueryBitmapParameters(hbm: u32, p: *mut u32) -> u32;
    #[link_name = "PMWIN.9003"]
    fn Rs2SetMinimizeViewer(h: u32) -> u32;
    #[link_name = "PMWIN.757"]
    fn WinGetPS(h: u32) -> u32;
    #[link_name = "PMWIN.848"]
    fn WinReleasePS(hps: u32) -> u32;
    #[link_name = "PMGPI.592"]
    fn GpiCreateLogColorTable(hps: u32, opts: u32, fmt: i32, start: i32, n: i32, t: *const i32) -> u32;
    #[link_name = "PMGPI.476"]
    fn GpiQueryPel(hps: u32, p: *const i32) -> i32;
    #[link_name = "PMWP.209"]
    fn WinLoadFileIcon(name: *const u8, private: u32) -> u32;
    #[link_name = "PMWP.210"]
    fn WinSetFileIcon(name: *const u8, info: *const u32) -> u32;
    #[link_name = "PMWP.216"]
    fn WinFreeFileIcon(h: u32) -> u32;
}

const WM_PAINT: u32 = 0x0023;
const WM_CONTROL: u32 = 0x0030;
const WM_CHAR: u32 = 0x007a;
const WM_MINMAXFRAME: u32 = 0x0046;
const WM_SETICON: u32 = 0x0047;
const WM_QUERYICON: u32 = 0x0048;
const WM_RS2_MINWINDOW: u32 = 0x0ff8;

const SWP_SIZE: u32 = 0x1;
const SWP_MOVE: u32 = 0x2;
const SWP_SHOW: u32 = 0x8;
const SWP_ACTIVATE: u32 = 0x80;
const SWP_MINIMIZE: u32 = 0x400;
const SWP_RESTORE: u32 = 0x1000;
const WS_VISIBLE: u32 = 0x8000_0000;
const WS_MINIMIZED: u32 = 0x0100_0000;

const CM_ALLOCDETAILFIELDINFO: u32 = 0x0330;
const CM_ALLOCRECORD: u32 = 0x0331;
const CM_ARRANGE: u32 = 0x0332;
const CM_FREERECORD: u32 = 0x0336;
const CM_INSERTDETAILFIELDINFO: u32 = 0x0338;
const CM_INSERTRECORD: u32 = 0x0339;
const CM_QUERYCNRINFO: u32 = 0x033d;
const CM_QUERYDETAILFIELDINFO: u32 = 0x033e;
const CM_QUERYRECORD: u32 = 0x0340;
const CM_QUERYRECORDEMPHASIS: u32 = 0x0341;
const CM_QUERYRECORDRECT: u32 = 0x0343;
const CM_REMOVERECORD: u32 = 0x0346;
const CM_SEARCHSTRING: u32 = 0x0348;
const CM_SETCNRINFO: u32 = 0x0349;
const CM_SETRECORDEMPHASIS: u32 = 0x034a;
const CM_SORTRECORD: u32 = 0x034b;
const CM_EXPANDTREE: u32 = 0x034f;

const CN_ENTER: u32 = 106;
const CN_EMPHASIS: u32 = 108;
const CN_EXPANDTREE: u32 = 117;

const CRA_SELECTED: u32 = 0x1;
const CRA_CURSORED: u32 = 0x4;
const CRA_INUSE: u32 = 0x8;
const CRA_EXPANDED: u32 = 0x80;
const CMA_FIRST: u32 = 0x10;
const CMA_LAST: u32 = 0x20;
const CMA_END: u32 = 0x40;
const CMA_NEXT: u32 = 0x100;
const CMA_FIRSTCHILD: u32 = 0x2;
const CMA_PARENT: u32 = 0x1;
const CMA_FREE: u32 = 0x1;
const CMA_INVALIDATE: u32 = 0x2;
const CMA_FLWINDOWATTR: u32 = 0x4;
const CMA_CNRTITLE: u32 = 0x1;
const CV_ICON: u32 = 0x4;
const CV_DETAIL: u32 = 0x8;
const CV_TREE: u32 = 0x40;
const CA_CONTAINERTITLE: u32 = 0x200;
const CA_TITLESEPARATOR: u32 = 0x400;
const CA_DETAILSVIEWTITLES: u32 = 0x8000;
const CFA_STRING: u32 = 0x800;
const CFA_ULONG: u32 = 0x2_0000;
const CFA_SEPARATOR: u32 = 0x200;

// RECORDCORE offsets.
const R_ATTR: usize = 4;
const R_X: usize = 8;
const R_PSZICON: usize = 20;
const R_HPTR: usize = 24;
const R_PSZTEXT: usize = 44;
const R_PSZNAME: usize = 48;
const R_PSZTREE: usize = 52;
/// The user data after the RECORDCORE: a size.
const R_SIZE: usize = 56;

static FAILS: AtomicU32 = AtomicU32::new(0);
static MINMAX: AtomicU32 = AtomicU32::new(0);
static OLD_FRAME: AtomicU32 = AtomicU32::new(0);
static VIEWER_MSGS: AtomicU32 = AtomicU32::new(0);
static ENTERS: AtomicU32 = AtomicU32::new(0);
static EMPHASIS: AtomicU32 = AtomicU32::new(0);
static EXPANDS: AtomicU32 = AtomicU32::new(0);
static MOVES: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

fn mp(lo: u32, hi: u32) -> u32 {
    (lo & 0xffff) | hi << 16
}

fn send(h: u32, m: u32, a: u32, b: u32) -> u32 {
    unsafe { WinSendMsg(h, m, a, b) }
}

fn rd(p: u32, off: usize) -> u32 {
    unsafe { ((p as usize + off) as *const u32).read_unaligned() }
}

fn wr(p: u32, off: usize, v: u32) {
    unsafe { ((p as usize + off) as *mut u32).write_unaligned(v) };
}

fn pump(hab: u32) {
    let mut q = QMsg::default();
    // PM_REMOVE
    while unsafe { WinPeekMsg(hab, &mut q, 0, 0, 0, 1) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
}

extern "C" fn client_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_PAINT => {
            let mut r = [0i32; 4];
            let hps = unsafe { WinBeginPaint(h, 0, r.as_mut_ptr()) };
            unsafe {
                WinFillRect(hps, r.as_ptr(), -34);
                // The message box icons, normal and mini, and halftoned.
                for (i, id) in [11, 12, 13, 14, 19, 20, 22].iter().enumerate() {
                    let p = WinQuerySysPointer(1, *id, 0);
                    WinDrawPointer(hps, 8 + i as i32 * 40, 8, p, 0);
                    WinDrawPointer(hps, 16 + i as i32 * 40, 44, p, 4);
                    WinDrawPointer(hps, 8 + i as i32 * 40, 64, p, 1);
                }
                WinEndPaint(hps);
            }
            0
        }
        WM_CONTROL => {
            match a >> 16 {
                CN_ENTER => ENTERS.fetch_add(1, Ordering::Relaxed),
                CN_EMPHASIS => EMPHASIS.fetch_add(1, Ordering::Relaxed),
                CN_EXPANDTREE => EXPANDS.fetch_add(1, Ordering::Relaxed),
                _ => 0,
            };
            0
        }
        WM_RS2_MINWINDOW => {
            VIEWER_MSGS.fetch_add(1, Ordering::Relaxed);
            0
        }
        _ => unsafe { WinDefWindowProc(h, m, a, b) },
    }
}

/// A frame subclassed to count WM_MINMAXFRAME.
extern "C" fn frame_sub(h: u32, m: u32, a: u32, b: u32) -> u32 {
    if m == WM_MINMAXFRAME {
        MINMAX.fetch_add(1, Ordering::Relaxed);
    }
    let old: WndProc = unsafe { core::mem::transmute(OLD_FRAME.load(Ordering::Relaxed) as usize) };
    old(h, m, a, b)
}

/// Compare records by their size field, largest first.
extern "C" fn by_size_desc(a: u32, b: u32, _s: u32) -> i32 {
    let (x, y) = (rd(a, R_SIZE), rd(b, R_SIZE));
    if x > y { -1 } else if x < y { 1 } else { 0 }
}

/// (x, y, cx, cy, fl) of a window.
fn pos(h: u32) -> (i32, i32, i32, i32, u32) {
    let mut swp = [0i32; 9];
    unsafe { WinQueryWindowPos(h, swp.as_mut_ptr()) };
    (swp[4], swp[3], swp[2], swp[1], swp[0] as u32)
}

fn pointers() {
    let ids = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 18, 19, 20, 21, 22];
    let all: Vec<u32> = ids.iter().map(|&i| unsafe { WinQuerySysPointer(1, i, 0) }).collect();
    check("WinQuerySysPointer: every SPTR_*", all.iter().all(|&h| h != 0));
    let mut uniq = all.clone();
    uniq.sort();
    uniq.dedup();
    check("system pointers are distinct", uniq.len() == all.len());
    check("WinQuerySysPointer: no SPTR 15", unsafe { WinQuerySysPointer(1, 15, 0) } == 0);
    check("system pointers cannot be destroyed", unsafe { WinDestroyPointer(all[12]) } == 0);
    check("SV_CXICON/SV_CYICON 32", unsafe { WinQuerySysValue(1, 38) == 32 && WinQuerySysValue(1, 39) == 32 });
    // POINTERINFO: fPointer, xHot, yHot, hbmPointer, hbmColor, mini x2.
    let mut pi = [0u32; 7];
    let ok = unsafe { WinQueryPointerInfo(all[12], pi.as_mut_ptr()) } != 0;
    check("WinQueryPointerInfo (error icon)", ok && pi[0] == 0 && pi[3] != 0 && pi[4] != 0 && pi[5] != 0);
    let mut bmp = [0u32; 4];
    bmp[0] = 12;
    unsafe { GpiQueryBitmapParameters(pi[3], bmp.as_mut_ptr()) };
    let (cx, cy) = (bmp[1] & 0xffff, bmp[1] >> 16);
    check("pointer mask bitmap is 32 x 64", (cx, cy) == (32, 64));
    let mut ai = [0u32; 7];
    unsafe { WinQueryPointerInfo(all[0], ai.as_mut_ptr()) };
    check("arrow is a pointer with its hot spot at the tip", ai[0] == 1 && ai[1] == 0 && ai[2] == 31);
    let copy = unsafe { WinCreatePointerIndirect(1, pi.as_ptr()) };
    check("WinCreatePointerIndirect", copy != 0 && !all.contains(&copy));
    let mut ci = [0u32; 7];
    check("its pointer info", unsafe { WinQueryPointerInfo(copy, ci.as_mut_ptr()) } != 0 && ci[1] == pi[1] && ci[2] == pi[2]);
    check("WinDestroyPointer", unsafe { WinDestroyPointer(copy) } != 0);
    check("WinDestroyPointer again fails", unsafe { WinDestroyPointer(copy) } == 0);
    let mono = unsafe { WinCreatePointer(1, ai[3], 1, 0, 31) };
    check("WinCreatePointer from a mask bitmap", mono != 0);
    let copy2 = unsafe { WinQuerySysPointer(1, 2, 1) };
    check("WinQuerySysPointer copy can be destroyed", copy2 != 0 && copy2 != all[1] && unsafe { WinDestroyPointer(copy2) } != 0);
    unsafe { WinSetPointer(1, all[2]) };
    check("WinSetPointer/WinQueryPointer (wait)", unsafe { WinQueryPointer(1) } == all[2]);
    unsafe { WinSetPointer(1, mono) };
    check("WinSetPointer with a created pointer", unsafe { WinQueryPointer(1) } == mono);
    unsafe {
        WinSetPointer(1, all[0]);
        WinDestroyPointer(mono);
    }
    check("pointer back to the arrow", unsafe { WinQueryPointer(1) } == all[0]);
}

/// A 32 x 32 monochrome icon file ('IC'): a filled square.
fn icon_file() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"IC");
    b.extend_from_slice(&26u32.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(&32u32.to_le_bytes());
    b.extend_from_slice(&12u32.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(&64u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0xff, 0xff, 0xff]);
    // XOR mask (32 rows): white square in the middle; AND mask: 0 there.
    for y in 0..32 {
        let inside = (8..24).contains(&y);
        let row: u32 = if inside { 0x0000_ffff << 8 } else { 0 };
        b.extend_from_slice(&row.to_be_bytes());
    }
    for y in 0..32 {
        let inside = (8..24).contains(&y);
        let row: u32 = if inside { !(0x0000_ffffu32 << 8) } else { 0xffff_ffff };
        b.extend_from_slice(&row.to_be_bytes());
    }
    b
}

/// A Windows program's own icon (NOTE16's: solid red, from its
/// RT_GROUP_ICON and RT_ICON resources), drawn in `client`.
fn windows_icon(client: u32) {
    let name = b"C:\\RS2\\NOTE16.EXE\0";
    let mut info = [0u8; 64];
    if unsafe { DosQueryPathInfo(name.as_ptr(), 1, info.as_mut_ptr(), 64) } != 0 {
        println!("SKIP a Windows program's icon (no C:\\RS2\\NOTE16.EXE)");
        return;
    }
    let (h, c) = icon_colour(client, name);
    check("WinLoadFileIcon (a Windows program): its own icon", h && c == 0xff0000);
    big_exe_icon(client, name);
}

/// A file's icon drawn in `client`: loaded, and the colour of a pel in it.
fn icon_colour(client: u32, name: &[u8]) -> (bool, i32) {
    let h = unsafe { WinLoadFileIcon(name.as_ptr(), 1) };
    let hps = unsafe { WinGetPS(client) };
    let c = unsafe {
        // LCOLF_RGB: GpiQueryPel answers RGB.
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
        WinDrawPointer(hps, 4, 4, h, 0);
        GpiQueryPel(hps, [20, 20].as_ptr())
    };
    unsafe {
        WinReleasePS(hps);
        WinFreeFileIcon(h);
    }
    (h != 0, c)
}

/// An executable with a lot of data appended (the Open Watcom installer
/// carries its 80 MB ZIP archive so) still shows its own icon: NOTE16
/// with 9 MB after it.
fn big_exe_icon(client: u32, src: &[u8]) {
    let big = b"C:\\SHBIG.EXE\0";
    // FSALLOCATE: idFileSystem, cSectorUnit, cUnit, cUnitAvail, cbSector.
    let mut fa = [0u32; 5];
    unsafe { DosQueryFSInfo(3, 1, fa.as_mut_ptr() as *mut u8, 18) };
    let free = fa[1] as u64 * fa[3] as u64 * (fa[4] & 0xffff) as u64;
    if free < 10 << 20 {
        println!("SKIP a 9 MB executable's icon (C: has {} KB free)", free >> 10);
        return;
    }
    let (mut hf, mut act) = (0u32, 0u32);
    let mut data = alloc::vec![0u8; 256 << 10];
    let mut n = 0u32;
    if unsafe { DosOpen(src.as_ptr(), &mut hf, &mut act, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } == 0 {
        unsafe {
            DosRead(hf, data.as_mut_ptr(), data.len() as u32, &mut n);
            DosClose(hf);
        }
    }
    data.truncate(n as usize);
    if unsafe { DosOpen(big.as_ptr(), &mut hf, &mut act, 0, 0, 0x12, 0x42, core::ptr::null_mut()) } == 0 {
        let zeros = alloc::vec![0u8; 64 << 10];
        let mut w = 0u32;
        unsafe {
            DosWrite(hf, data.as_ptr(), data.len() as u32, &mut w);
            for _ in 0..(9 << 20) / zeros.len() {
                DosWrite(hf, zeros.as_ptr(), zeros.len() as u32, &mut w);
            }
            DosClose(hf);
        }
    }
    let (h, c) = icon_colour(client, big);
    check("WinLoadFileIcon: a 9 MB executable's own icon", h && c == 0xff0000);
    unsafe { DosDelete(big.as_ptr()) };
}

fn file_icons() {
    let prog = unsafe { WinLoadFileIcon(b"C:\\RS2\\SHELLTST.EXE\0".as_ptr(), 1) };
    check("WinLoadFileIcon (a program)", prog != 0);
    let dir = unsafe { WinLoadFileIcon(b"C:\\OS2\0".as_ptr(), 1) };
    check("WinLoadFileIcon (a directory)", dir != 0);
    check("WinLoadFileIcon (missing file) fails", unsafe { WinLoadFileIcon(b"C:\\NOSUCH.XYZ\0".as_ptr(), 1) } == 0);
    check("WinFreeFileIcon", unsafe { WinFreeFileIcon(prog) != 0 && WinFreeFileIcon(dir) != 0 });
    // A data file with an icon of its own (.ICON EA, ICON_DATA).
    let name = b"C:\\SHTEST.DAT\0";
    let (mut hf, mut act) = (0u32, 0u32);
    // OPEN_ACTION_CREATE_IF_NEW | REPLACE_IF_EXISTS; READWRITE.
    let rc = unsafe { DosOpen(name.as_ptr(), &mut hf, &mut act, 0, 0, 0x12, 0x42, core::ptr::null_mut()) };
    if rc == 0 {
        let mut n = 0;
        unsafe {
            DosWrite(hf, b"data".as_ptr(), 4, &mut n);
            DosClose(hf);
        }
    }
    let data = icon_file();
    let info: [u32; 7] = [28, 3, 0, 0, 0, data.len() as u32, data.as_ptr() as u32];
    let set = unsafe { WinSetFileIcon(name.as_ptr(), info.as_ptr()) };
    check("WinSetFileIcon (ICON_DATA)", set != 0);
    let h = unsafe { WinLoadFileIcon(name.as_ptr(), 1) };
    let mut pi = [0u32; 7];
    let got = h != 0 && unsafe { WinQueryPointerInfo(h, pi.as_mut_ptr()) } != 0;
    // Monochrome as set: no colour bitmap.
    check("WinLoadFileIcon reads the .ICON EA back", got && pi[4] == 0);
    let clear: [u32; 7] = [28, 4, 0, 0, 0, 0, 0];
    check("WinSetFileIcon (ICON_CLEAR)", unsafe { WinSetFileIcon(name.as_ptr(), clear.as_ptr()) } != 0);
    let h2 = unsafe { WinLoadFileIcon(name.as_ptr(), 1) };
    let mut p2 = [0u32; 7];
    unsafe { WinQueryPointerInfo(h2, p2.as_mut_ptr()) };
    check("without the EA: the default data file icon (colour)", h2 != 0 && p2[4] != 0);
    unsafe {
        WinFreeFileIcon(h);
        WinFreeFileIcon(h2);
        DosDelete(name.as_ptr());
    }
    // An icon file is its own icon (ICONFILE=...\WATCOM.ICO).
    let ico = b"C:\\SHTEST.ICO\0";
    let mut hf = 0u32;
    if unsafe { DosOpen(ico.as_ptr(), &mut hf, &mut act, 0, 0, 0x12, 0x42, core::ptr::null_mut()) } == 0 {
        let mut n = 0;
        unsafe {
            DosWrite(hf, data.as_ptr(), data.len() as u32, &mut n);
            DosClose(hf);
        }
    }
    let hi = unsafe { WinLoadFileIcon(ico.as_ptr(), 1) };
    let mut p3 = [0u32; 7];
    let got = hi != 0 && unsafe { WinQueryPointerInfo(hi, p3.as_mut_ptr()) } != 0;
    check("WinLoadFileIcon of an .ICO file: the icon in it", got && p3[4] == 0);
    unsafe {
        WinFreeFileIcon(hi);
        DosDelete(ico.as_ptr());
    }
}


/// A client whose class has CS_MOVENOTIFY: counts its WM_MOVEs.
extern "C" fn move_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    if m == 0x0006 {
        MOVES.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    unsafe { WinDefWindowProc(h, m, a, b) }
}

/// WM_MOVE, as PM sends it: from WinSetWindowPos (not while
/// WinCreateStdWindow builds the frame), to a CS_MOVENOTIFY client when its
/// frame moves; and WinQueryWindowPos's fl.
fn moves(hab: u32) {
    // CS_MOVENOTIFY
    unsafe { WinRegisterClass(hab, b"ShMove\0".as_ptr(), move_proc, 1, 0) };
    let mut fcf = 1u32 | 2 | 8;
    let mut client = 0;
    let f = unsafe { WinCreateStdWindow(1, 0, &mut fcf, b"ShMove\0".as_ptr(), b"Move me\0".as_ptr(), 0, 0, 3, &mut client) };
    pump(hab);
    check("WM_MOVE: none while WinCreateStdWindow builds the frame", MOVES.load(Ordering::Relaxed) == 0);
    unsafe { WinSetWindowPos(f, 0, 100, 100, 200, 150, SWP_SIZE | SWP_MOVE | SWP_SHOW) };
    pump(hab);
    let n = MOVES.load(Ordering::Relaxed);
    check("WM_MOVE: a CS_MOVENOTIFY client hears its frame placed", n > 0);
    unsafe { WinSetWindowPos(f, 0, 120, 90, 0, 0, SWP_MOVE) };
    pump(hab);
    check("WM_MOVE: and moved", MOVES.load(Ordering::Relaxed) > n);
    let n = MOVES.load(Ordering::Relaxed);
    unsafe { WinSetWindowPos(f, 0, 0, 0, 220, 150, SWP_SIZE) };
    pump(hab);
    check("WM_MOVE: not when only sized", MOVES.load(Ordering::Relaxed) == n);
    let (x, y, _, _, fl) = pos(f);
    // SWP_SIZE | SWP_MOVE | SWP_ZORDER | SWP_SHOW | SWP_RESTORE
    check("WinQueryWindowPos: fl of a shown frame", (x, y) == (120, 90) && fl == 0x100f);
    unsafe { WinDestroyWindow(f) };
}

fn frames(hab: u32, viewer: u32) {
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX
    let mut fcf = 1u32 | 2 | 8 | 0x30;
    let mut client = 0;
    let f = unsafe { WinCreateStdWindow(1, 0, &mut fcf, b"ShTest\0".as_ptr(), b"Minimize me\0".as_ptr(), 0, 0, 2, &mut client) };
    check("a second frame", f != 0);
    unsafe {
        WinSetWindowPos(f, 0, 300, 300, 300, 200, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE);
        OLD_FRAME.store(WinSubclassWindow(f, frame_sub), Ordering::Relaxed);
    }
    pump(hab);
    let ic = unsafe { WinQuerySysPointer(1, 22, 0) };
    send(f, WM_SETICON, ic, 0);
    check("WM_SETICON/WM_QUERYICON", send(f, WM_QUERYICON, 0, 0) == ic);
    unsafe { WinSetWindowPos(f, 0, 0, 0, 0, 0, SWP_MINIMIZE) };
    pump(hab);
    let (x, y, cx, cy, fl) = pos(f);
    let style = unsafe { WinQueryWindowULong(f, -2) };
    check("SWP_MINIMIZE: WS_MINIMIZED, WinQueryWindowPos says SWP_MINIMIZE", style & WS_MINIMIZED != 0 && fl & SWP_MINIMIZE != 0);
    check("minimized: an icon at the bottom of the desktop", cx < 120 && cy < 100 && y < 100 && x >= 0);
    check("minimized: the client is hidden", unsafe { WinIsWindowVisible(client) } == 0 && unsafe { WinIsWindowVisible(f) } != 0);
    check("WM_MINMAXFRAME on minimize", MINMAX.load(Ordering::Relaxed) == 1);
    unsafe { WinSetWindowPos(f, 0, 0, 0, 0, 0, SWP_RESTORE) };
    pump(hab);
    let (x, y, cx, cy, fl) = pos(f);
    let style = unsafe { WinQueryWindowULong(f, -2) };
    check("SWP_RESTORE: back where it was", (x, y, cx, cy) == (300, 300, 300, 200) && style & WS_MINIMIZED == 0 && fl & SWP_MINIMIZE == 0);
    check("restored: the client shows", unsafe { WinIsWindowVisible(client) } != 0);
    check("WM_MINMAXFRAME on restore", MINMAX.load(Ordering::Relaxed) == 2);
    // A minimized window viewer takes minimized frames over.
    check("Rs2SetMinimizeViewer", unsafe { Rs2SetMinimizeViewer(viewer) } == 0);
    unsafe { WinSetWindowPos(f, 0, 0, 0, 0, 0, SWP_MINIMIZE) };
    pump(hab);
    check("with a viewer: minimized frames are hidden", unsafe { WinIsWindowVisible(f) } == 0 && unsafe { WinQueryWindowULong(f, -2) } & WS_MINIMIZED != 0);
    unsafe { WinSetWindowPos(f, 0, 0, 0, 0, 0, SWP_RESTORE) };
    pump(hab);
    check("with a viewer: restored and shown", unsafe { WinIsWindowVisible(f) } != 0 && pos(f).2 == 300);
    check("the viewer heard both", VIEWER_MSGS.load(Ordering::Relaxed) == 2);
    check("Rs2SetMinimizeViewer(0) returns the viewer", unsafe { Rs2SetMinimizeViewer(0) } == viewer);
    // Left minimized on the desktop for the screenshot.
    unsafe { WinSetWindowPos(f, 0, 0, 0, 0, 0, SWP_MINIMIZE) };
}

static NAMES: [&[u8]; 4] = [b"Beta\0", b"Alpha\0", b"Gamma\0", b"Delta\0"];
static SIZES: [u32; 4] = [20, 10, 40, 30];

fn container(hab: u32, client: u32) -> u32 {
    // CCS_EXTENDSEL | CCS_AUTOPOSITION, visible.
    let cnr = unsafe { WinCreateWindow(client, 0xffff_0025, b"\0".as_ptr(), WS_VISIBLE | 0x1 | 0x8, 8, 100, 560, 260, client, 3, 100, null(), 0) };
    check("WC_CONTAINER", cnr != 0);
    let first = send(cnr, CM_ALLOCRECORD, 4, 4);
    check("CM_ALLOCRECORD (4 records)", first != 0 && rd(first, 0) == 56);
    let mut recs = Vec::new();
    let mut r = first;
    while r != 0 {
        recs.push(r);
        r = rd(r, 16);
    }
    check("records are linked", recs.len() == 4);
    let icons = [19, 20, 22, 21];
    for (i, &r) in recs.iter().enumerate() {
        let n = NAMES[i].as_ptr() as u32;
        wr(r, R_PSZICON, n);
        wr(r, R_PSZNAME, n);
        wr(r, R_PSZTEXT, n);
        wr(r, R_PSZTREE, n);
        wr(r, R_HPTR, unsafe { WinQuerySysPointer(1, icons[i], 0) });
        wr(r, R_SIZE, SIZES[i]);
    }
    // RECORDINSERT: cb, pRecordOrder, pRecordParent, fInvalidate, zOrder, count.
    let ri: [u32; 6] = [24, CMA_END, 0, 1, 1, 4];
    check("CM_INSERTRECORD returns the count", send(cnr, CM_INSERTRECORD, first, ri.as_ptr() as u32) == 4);
    check("CM_FREERECORD of inserted records fails", send(cnr, CM_FREERECORD, recs.as_ptr() as u32, 1) == 0);
    check("CM_QUERYRECORD first/next/last", send(cnr, CM_QUERYRECORD, 0, mp(CMA_FIRST, 1)) == recs[0] && send(cnr, CM_QUERYRECORD, recs[0], mp(CMA_NEXT, 1)) == recs[1] && send(cnr, CM_QUERYRECORD, 0, mp(CMA_LAST, 1)) == recs[3]);
    check("CCS_AUTOPOSITION placed the icons", rd(recs[1], R_X) != rd(recs[0], R_X) || rd(recs[1], R_X + 4) != rd(recs[0], R_X + 4));
    // CNRINFO.
    let title = b"Shell test\0";
    let mut ci = [0u32; 23];
    ci[0] = 92;
    ci[4] = title.as_ptr() as u32;
    ci[5] = CV_ICON | CA_CONTAINERTITLE | CA_TITLESEPARATOR;
    check("CM_SETCNRINFO", send(cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR | CMA_CNRTITLE) != 0);
    let mut q = [0u32; 23];
    let n = send(cnr, CM_QUERYCNRINFO, q.as_mut_ptr() as u32, 92);
    check("CM_QUERYCNRINFO", n == 92 && q[5] == ci[5] && q[9] == 4 && q[4] == ci[4]);
    // Emphasis.
    check("CM_SETRECORDEMPHASIS (select)", send(cnr, CM_SETRECORDEMPHASIS, recs[2], mp(1, CRA_SELECTED)) != 0);
    check("selection is in the record", rd(recs[2], R_ATTR) & CRA_SELECTED != 0);
    check("CM_QUERYRECORDEMPHASIS", send(cnr, CM_QUERYRECORDEMPHASIS, CMA_FIRST, CRA_SELECTED) == recs[2]);
    send(cnr, CM_SETRECORDEMPHASIS, recs[3], mp(1, CRA_INUSE));
    check("in-use emphasis", send(cnr, CM_QUERYRECORDEMPHASIS, CMA_FIRST, CRA_INUSE) == recs[3]);
    send(cnr, CM_SETRECORDEMPHASIS, recs[0], mp(1, CRA_CURSORED));
    check("cursored emphasis", send(cnr, CM_QUERYRECORDEMPHASIS, CMA_FIRST, CRA_CURSORED) == recs[0]);
    // The rectangle of a record.
    let mut rr = [0i32; 4];
    let qr: [u32; 4] = [16, recs[1], 0, 0x0800 | 0x1000];
    check("CM_QUERYRECORDRECT", send(cnr, CM_QUERYRECORDRECT, rr.as_mut_ptr() as u32, qr.as_ptr() as u32) != 0 && rr[2] > rr[0] && rr[3] > rr[1]);
    // Keyboard: the cursor moves, selection follows (extended
    // selection), the owner hears CN_EMPHASIS; Enter is CN_ENTER.
    unsafe { WinSetFocus(1, cnr) };
    pump(hab);
    let before = EMPHASIS.load(Ordering::Relaxed);
    // KC_VIRTUALKEY | KC_SCANCODE, VK_RIGHT.
    send(cnr, WM_CHAR, 0x2 | 0x4 | 1 << 16, 0x17 << 16);
    check("arrow key: CN_EMPHASIS", EMPHASIS.load(Ordering::Relaxed) > before);
    check("arrow key: cursor and selection moved", rd(recs[1], R_ATTR) & (CRA_SELECTED | CRA_CURSORED) == CRA_SELECTED | CRA_CURSORED && rd(recs[2], R_ATTR) & CRA_SELECTED == 0);
    send(cnr, WM_CHAR, 0x2 | 0x4 | 1 << 16, 0x08 << 16);
    check("Enter: CN_ENTER", ENTERS.load(Ordering::Relaxed) == 1);
    // Sorting.
    check("CM_SORTRECORD", send(cnr, CM_SORTRECORD, by_size_desc as *const () as u32, 0) != 0);
    let order: Vec<u32> = {
        let mut v = Vec::new();
        let mut r = send(cnr, CM_QUERYRECORD, 0, mp(CMA_FIRST, 1));
        while r != 0 && v.len() < 10 {
            v.push(rd(r, R_SIZE));
            r = send(cnr, CM_QUERYRECORD, r, mp(CMA_NEXT, 1));
        }
        v
    };
    check("sorted by size, largest first", order == [40, 30, 20, 10]);
    // Searching.
    let ss: [u32; 5] = [20, b"GAM\0".as_ptr() as u32, 1, 0, CV_ICON];
    check("CM_SEARCHSTRING (prefix, any case)", send(cnr, CM_SEARCHSTRING, ss.as_ptr() as u32, CMA_FIRST) == recs[2]);
    // Detail view with two columns.
    let f1 = send(cnr, CM_ALLOCDETAILFIELDINFO, 2, 0);
    let f2 = if f1 != 0 { rd(f1, 24) } else { 0 };
    check("CM_ALLOCDETAILFIELDINFO", f1 != 0 && f2 != 0);
    wr(f1, 4, CFA_STRING | CFA_SEPARATOR);
    wr(f1, 12, b"Name\0".as_ptr() as u32);
    wr(f1, 16, R_PSZNAME as u32);
    wr(f2, 4, CFA_ULONG | 0x2);
    wr(f2, 12, b"Size\0".as_ptr() as u32);
    wr(f2, 16, R_SIZE as u32);
    let fi: [u32; 4] = [16, CMA_END, 1, 2];
    check("CM_INSERTDETAILFIELDINFO", send(cnr, CM_INSERTDETAILFIELDINFO, f1, fi.as_ptr() as u32) == 2);
    check("CM_QUERYDETAILFIELDINFO", send(cnr, CM_QUERYDETAILFIELDINFO, 0, CMA_FIRST) == f1 && send(cnr, CM_QUERYDETAILFIELDINFO, f1, CMA_NEXT) == f2);
    ci[5] = CV_DETAIL | CA_DETAILSVIEWTITLES;
    send(cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR);
    pump(hab);
    send(cnr, CM_QUERYCNRINFO, q.as_mut_ptr() as u32, 92);
    check("detail view: cFields 2", q[21] == 2 && q[5] & CV_DETAIL != 0);
    // A tree: a child record under the first one.
    let child = send(cnr, CM_ALLOCRECORD, 4, 1);
    wr(child, R_PSZTREE, b"Child\0".as_ptr() as u32);
    wr(child, R_PSZICON, b"Child\0".as_ptr() as u32);
    wr(child, R_PSZNAME, b"Child\0".as_ptr() as u32);
    wr(child, R_HPTR, unsafe { WinQuerySysPointer(1, 19, 0) });
    let top = send(cnr, CM_QUERYRECORD, 0, mp(CMA_FIRST, 1));
    let rc: [u32; 6] = [24, CMA_END, top, 1, 1, 1];
    check("CM_INSERTRECORD (child)", send(cnr, CM_INSERTRECORD, child, rc.as_ptr() as u32) == 5);
    check("CM_QUERYRECORD first child / parent", send(cnr, CM_QUERYRECORD, top, mp(CMA_FIRSTCHILD, 1)) == child && send(cnr, CM_QUERYRECORD, child, mp(CMA_PARENT, 1)) == top);
    ci[5] = CV_TREE | CV_ICON;
    send(cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR);
    send(cnr, CM_EXPANDTREE, top, 0);
    check("CM_EXPANDTREE: expanded and CN_EXPANDTREE", rd(top, R_ATTR) & CRA_EXPANDED != 0 && EXPANDS.load(Ordering::Relaxed) == 1);
    pump(hab);
    // Removal: the child with CMA_FREE.
    let arr = [child];
    check("CM_REMOVERECORD (child, free)", send(cnr, CM_REMOVERECORD, arr.as_ptr() as u32, mp(1, CMA_FREE | CMA_INVALIDATE)) == 4);
    check("removed record is gone", send(cnr, CM_QUERYRECORD, top, mp(CMA_FIRSTCHILD, 1)) == 0);
    // Back to the icon view, arranged, for the screenshot.
    ci[5] = CV_ICON | CA_CONTAINERTITLE | CA_TITLESEPARATOR;
    send(cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR);
    send(cnr, CM_ARRANGE, 0, 0);
    pump(hab);
    // A second container emptied with CMA_FREE of every record.
    let c2 = unsafe { WinCreateWindow(client, 0xffff_0025, b"\0".as_ptr(), 0, 0, 0, 100, 100, client, 3, 101, null(), 0) };
    let r2 = send(c2, CM_ALLOCRECORD, 0, 3);
    let ri2: [u32; 6] = [24, CMA_FIRST, 0, 0, 1, 3];
    send(c2, CM_INSERTRECORD, r2, ri2.as_ptr() as u32);
    check("CM_REMOVERECORD (all, free)", send(c2, CM_REMOVERECORD, 0, mp(0, CMA_FREE)) == 0 && send(c2, CM_QUERYRECORD, 0, mp(CMA_FIRST, 1)) == 0);
    unsafe { WinDestroyWindow(c2) };
    let _ = CMA_LAST;
    cnr
}

fn main(s: &Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    check("WinInitialize/WinCreateMsgQueue", hab != 0 && hmq != 0);
    let args: Vec<u8> = s.args().iter().copied().skip_while(|&c| c == b' ').collect();
    let quit = args.starts_with(b"QUIT");
    unsafe { WinRegisterClass(hab, b"ShTest\0".as_ptr(), client_proc, 0, 0) };
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX
    let mut fcf = 1u32 | 2 | 8 | 0x30;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(1, 0, &mut fcf, b"ShTest\0".as_ptr(), b"Shell test\0".as_ptr(), 0, 0, 1, &mut client) };
    check("test window", frame != 0 && client != 0);
    unsafe { WinSetWindowPos(frame, 0, 40, 260, 600, 420, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE) };
    pump(hab);
    pointers();
    file_icons();
    windows_icon(client);
    frames(hab, client);
    moves(hab);
    let cnr = container(hab, client);
    let f = FAILS.load(Ordering::Relaxed);
    println!("shelltest done: {f} failure(s)");
    if !quit {
        unsafe { WinSetFocus(1, cnr) };
        let mut q = QMsg::default();
        while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
            unsafe { WinDispatchMsg(hab, &q) };
        }
    }
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    f
}
