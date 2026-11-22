//! PMDEMO64.EXE: a 64-bit Presentation Manager program (docs/API64.md).
//!
//! A frame window whose client draws text and shapes with GPI and counts
//! timer ticks. With `/SELFTEST` it checks the 64-bit PM layer itself
//! (window procedures in both directions, WM_CREATE's CREATESTRUCT, QMSG,
//! CLASSINFO, subclassing, WinSendMsg results), closes itself after a few
//! ticks and prints PASS/FAIL lines and `pmdemo64 selftest done: N
//! failure(s)`.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use os2::pm::*;
use os2::*;

os2::entry!(main);
os2::app_type!(PM);
os2::resources!();

const ID_MAIN: u32 = 1;
const IDM_ABOUT: u16 = 11;
const IDM_EXIT: u16 = 12;
const IDS_HELLO: u32 = 20;
const IDD_ABOUT: u32 = 30;

static HELLO: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
static INITDLG: AtomicBool = AtomicBool::new(false);

/// The greeting from the string table (64 bytes kept).
fn hello() -> String {
    let mut b = [0u8; 64];
    for (i, w) in HELLO.iter().enumerate() {
        b[i * 8..i * 8 + 8].copy_from_slice(&w.load(Ordering::Relaxed).to_le_bytes());
    }
    let n = b.iter().position(|&c| c == 0).unwrap_or(64);
    b[..n].iter().map(|&c| c as char).collect()
}

unsafe extern "C" fn about_proc(hwnd: HWND, msg: u32, mp1: MPARAM, mp2: MPARAM) -> MRESULT {
    if msg == WM_INITDLG {
        INITDLG.store(true, Ordering::Relaxed);
    }
    unsafe { WinDefDlgProc(hwnd, msg, mp1, mp2) }
}

const CLASS: &core::ffi::CStr = c"PmHello64";

static HAB_: AtomicU32 = AtomicU32::new(0);
static TICKS: AtomicU32 = AtomicU32::new(0);
static SELFTEST: AtomicBool = AtomicBool::new(false);
static CREATE_OK: AtomicBool = AtomicBool::new(false);
static PAINTED: AtomicU32 = AtomicU32::new(0);
static FRAME_MSGS: AtomicU32 = AtomicU32::new(0);
static OLD_FRAME: AtomicU64 = AtomicU64::new(0);

fn text_at(hps: HPS, x: i32, y: i32, s: &str) -> i32 {
    let p = POINTL { x, y };
    unsafe { GpiCharStringAt(hps, &p, s.len() as i32, s.as_ptr()) }
}

unsafe extern "C" fn client_proc(hwnd: HWND, msg: u32, mp1: MPARAM, mp2: MPARAM) -> MRESULT {
    match msg {
        WM_CREATE => {
            // mp2 is the 64-bit CREATESTRUCT.
            let cs = unsafe { &*(mp2 as *const CREATESTRUCT) };
            let class = unsafe { cstr(cs.pszClass) };
            CREATE_OK.store(class == CLASS.to_bytes() && cs.hwndParent != 0, Ordering::Relaxed);
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        WM_PAINT => {
            let mut r = RECTL::default();
            let hps = unsafe { WinBeginPaint(hwnd, 0, core::ptr::null_mut()) };
            unsafe {
                WinQueryWindowRect(hwnd, &mut r);
                WinFillRect(hps, &r, CLR_WHITE);
            }
            let top = r.yTop;
            let mut ok = 1;
            unsafe { GpiSetColor(hps, CLR_DARKBLUE) };
            ok &= text_at(hps, 20, top - 30, &hello());
            unsafe { GpiSetColor(hps, CLR_BLACK) };
            let here = client_proc as *const () as usize;
            ok &= text_at(hps, 20, top - 55, &format!("Window procedure at {here:#x}, MPARAM is {} bytes", size_of::<MPARAM>()));
            ok &= text_at(hps, 20, top - 75, &format!("Timer ticks: {}", TICKS.load(Ordering::Relaxed)));
            // Shapes.
            let colors = [CLR_RED, CLR_GREEN, CLR_BLUE, CLR_YELLOW, CLR_CYAN];
            for (i, &c) in colors.iter().enumerate() {
                let x = 20 + i as i32 * 70;
                unsafe {
                    GpiSetColor(hps, c);
                    GpiMove(hps, &POINTL { x, y: 20 });
                    GpiBox(hps, DRO_FILL, &POINTL { x: x + 60, y: 80 }, 0, 0);
                    GpiSetColor(hps, CLR_BLACK);
                    GpiBox(hps, DRO_OUTLINE, &POINTL { x: x + 60, y: 80 }, 0, 0);
                }
            }
            unsafe {
                GpiSetColor(hps, CLR_DARKBLUE);
                GpiMove(hps, &POINTL { x: 420, y: 50 });
                GpiFullArc(hps, DRO_OUTLINEFILL, 30 << 16);
                WinEndPaint(hps);
            }
            if ok == 1 {
                PAINTED.fetch_add(1, Ordering::Relaxed);
            }
            0
        }
        WM_TIMER => {
            let n = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
            unsafe { WinInvalidateRect(hwnd, core::ptr::null(), 0) };
            if SELFTEST.load(Ordering::Relaxed) && n >= 3 && PAINTED.load(Ordering::Relaxed) > 0 {
                unsafe { WinPostMsg(hwnd, WM_CLOSE, 0, 0) };
            }
            0
        }
        WM_COMMAND => {
            match short1frommp(mp1) {
                IDM_ABOUT => unsafe {
                    WinDlgBox(HWND_DESKTOP, hwnd, about_proc, 0, IDD_ABOUT, core::ptr::null_mut());
                },
                IDM_EXIT => unsafe {
                    WinPostMsg(hwnd, WM_CLOSE, 0, 0);
                },
                _ => {}
            }
            0
        }
        // Answers with a value from all 64 bits of what it was sent (the
        // 32-bit PMWIN keeps the low halves).
        WM_USER => mp1.wrapping_mul(3).wrapping_add(mp2),
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}

/// Subclasses the frame: counts its messages, then the frame's own (32-bit)
/// procedure handles them.
unsafe extern "C" fn frame_proc(hwnd: HWND, msg: u32, mp1: MPARAM, mp2: MPARAM) -> MRESULT {
    FRAME_MSGS.fetch_add(1, Ordering::Relaxed);
    let old: PFNWP = unsafe { core::mem::transmute(OLD_FRAME.load(Ordering::Relaxed)) };
    unsafe { old(hwnd, msg, mp1, mp2) }
}

fn main(s: &Startup) -> u32 {
    let selftest = s.args().windows(9).any(|w| w.eq_ignore_ascii_case(b"/SELFTEST"));
    SELFTEST.store(selftest, Ordering::Relaxed);
    let mut results: Vec<(String, bool)> = Vec::new();
    let mut check = |name: &str, ok: bool| results.push((String::from(name), ok));

    let hab = unsafe { WinInitialize(0) };
    HAB_.store(hab, Ordering::Relaxed);
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    check("WinInitialize and WinCreateMsgQueue", hab != 0 && hmq != 0);
    let ok = unsafe { WinRegisterClass(hab, CLASS.as_ptr().cast(), client_proc, CS_SIZEREDRAW, 0) };
    check("WinRegisterClass", ok != 0);
    let mut ci = CLASSINFO { flClassStyle: 0, pfnWindowProc: None, cbWindowData: 99 };
    let ok = unsafe { WinQueryClassInfo(hab, CLASS.as_ptr().cast(), &mut ci) } != 0;
    let same = ci.pfnWindowProc.map(|p| p as usize) == Some(client_proc as *const () as usize);
    check("WinQueryClassInfo gives the 64-bit procedure", ok && same && ci.cbWindowData == 0);

    let mut text = [0u8; 64];
    let n = unsafe { WinLoadString(hab, 0, IDS_HELLO, 64, text.as_mut_ptr()) };
    for (i, w) in HELLO.iter().enumerate() {
        w.store(u64::from_le_bytes(text[i * 8..i * 8 + 8].try_into().unwrap()), Ordering::Relaxed);
    }
    check("WinLoadString (a 64-bit module's string table)", n > 0 && hello().starts_with("Hello from a 64-bit"));

    let mut fl = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_MENU;
    let mut client = 0;
    let frame = unsafe {
        WinCreateStdWindow(
            HWND_DESKTOP,
            0,
            &mut fl,
            CLASS.as_ptr().cast(),
            c"64-bit PM".as_ptr().cast(),
            0,
            0,
            ID_MAIN,
            &mut client,
        )
    };
    check("WinCreateStdWindow", frame != 0 && client != 0);
    check("WM_CREATE's CREATESTRUCT (64-bit)", CREATE_OK.load(Ordering::Relaxed));
    let p = unsafe { WinQueryWindowPtr(client, QWP_PFNWP) };
    check("WinQueryWindowPtr(QWP_PFNWP) gives the 64-bit procedure", p as usize == client_proc as *const () as usize);
    let old = unsafe { WinSubclassWindow(frame, frame_proc) };
    OLD_FRAME.store(old.map_or(0, |p| p as usize as u64), Ordering::Relaxed);
    check("WinSubclassWindow returns the frame's procedure", old.is_some());
    let r = unsafe { WinSendMsg(client, WM_USER, 0x1000, 7) };
    check("WinSendMsg to a 64-bit window procedure", r == 0x3007);
    check("the frame's menu (a 64-bit module's resource)", unsafe { WinWindowFromID(frame, FID_MENU) } != 0);
    let dlg = unsafe { WinLoadDlg(HWND_DESKTOP, frame, about_proc, 0, IDD_ABOUT, core::ptr::null_mut()) };
    check("WinLoadDlg (dialog template resource, 64-bit dialog procedure)", dlg != 0 && INITDLG.load(Ordering::Relaxed));
    let ok_button = unsafe { WinWindowFromID(dlg, DID_OK) };
    check("the dialog's OK button", ok_button != 0);
    unsafe { WinDestroyWindow(dlg) };
    let mut title = [0u8; 32];
    let n = unsafe { WinQueryWindowText(frame, 32, title.as_mut_ptr()) };
    check("WinQueryWindowText", &title[..n as usize] == b"64-bit PM");

    unsafe {
        WinSetWindowPos(frame, HWND_TOP, 60, 80, 560, 300, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
        WinStartTimer(hab, client, 1, if selftest { 300 } else { 1000 });
    }
    let mut q = QMSG::default();
    let mut got = 0u32;
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        got += 1;
        unsafe { WinDispatchMsg(hab, &q) };
    }
    check("message loop (WinGetMsg, WinDispatchMsg)", got > 0 && q.msg == WM_QUIT);
    check("timer ticks", TICKS.load(Ordering::Relaxed) >= 3 || !selftest);
    check("WM_PAINT drew", PAINTED.load(Ordering::Relaxed) > 0);
    check("subclassed frame saw messages", FRAME_MSGS.load(Ordering::Relaxed) > 0);
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    let failed = results.iter().filter(|r| !r.1).count();
    if selftest {
        for (name, ok) in &results {
            os2::println!("{}: {name}", if *ok { "PASS" } else { "FAIL" });
        }
        os2::println!("pmdemo64 selftest done: {failed} failure(s)");
    }
    failed as u32
}
