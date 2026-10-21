//! Window manager checks: the text cursor, frames of classes derived from
//! WC_FRAME (EPM's), WinSetMultWindowPos, WinStoreWindowPos and
//! WinRestoreWindowPos, DDE, WinQueryQueueInfo and the spooler of a
//! system without printers.

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{check, WndProc};
use os2::DosAllocSharedMem;

unsafe extern "C" {
    #[link_name = "PMWIN.715"]
    fn WinCreateCursor(h: u32, x: i32, y: i32, cx: i32, cy: i32, fs: u32, clip: *const i32) -> u32;
    #[link_name = "PMWIN.880"]
    fn WinShowCursor(h: u32, show: u32) -> u32;
    #[link_name = "PMWIN.725"]
    fn WinDestroyCursor(h: u32) -> u32;
    #[link_name = "PMWIN.812"]
    fn WinQueryCursorInfo(desk: u32, p: *mut i32) -> u32;
    #[link_name = "PMWIN.925"]
    fn WinQueryClassInfo(hab: u32, name: u32, info: *mut u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: *const u8, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.837"]
    fn WinQueryWindowPos(h: u32, swp: *mut i32) -> u32;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(h: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.863"]
    fn WinSetMultWindowPos(hab: u32, swp: *const i32, n: u32) -> u32;
    #[link_name = "PMWIN.753"]
    fn WinGetLastError(hab: u32) -> u32;
    #[link_name = "PMWIN.824"]
    fn WinQueryQueueInfo(hmq: u32, p: *mut u32, cb: u32) -> u32;
    #[link_name = "PMWIN.718"]
    fn WinDdeInitiate(client: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32;
    #[link_name = "PMWIN.719"]
    fn WinDdePostMsg(to: u32, from: u32, wm: u32, dde: *mut u8, fl: u32) -> u32;
    #[link_name = "PMWIN.720"]
    fn WinDdeRespond(client: u32, server: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32;
    #[link_name = "PMWP.207"]
    fn WinStoreWindowPos(app: *const u8, key: *const u8, h: u32) -> u32;
    #[link_name = "PMWP.208"]
    fn WinRestoreWindowPos(app: *const u8, key: *const u8, h: u32) -> u32;
    #[link_name = "PMSPL.399"]
    fn SplEnumQueue(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, needed: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.397"]
    fn SplQueryQueue(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, needed: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, buf: *const u8, len: u32) -> u32;
}

const WM_FORMATFRAME: u32 = 0x41;
const WM_DDE_INITIATE: u32 = 0xa0;
const WM_DDE_DATA: u32 = 0xa5;
const WM_DDE_INITIATEACK: u32 = 0xa9;
const FID_TITLEBAR: u32 = 0x8003;
const FID_CLIENT: u32 = 0x8008;

/// WC_FRAME's procedure (WinQueryClassInfo), which the derived class
/// passes everything on to.
static FRAME_PROC: AtomicU32 = AtomicU32::new(0);
/// The derived frame's DDE messages: (msg, mp1, first byte of the
/// application name or the DDESTRUCT pointer).
static DDE: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
static ACK: AtomicU32 = AtomicU32::new(0);

extern "C" fn derived_frame(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_DDE_INITIATE | WM_DDE_INITIATEACK if mp2 != 0 => {
            // DDEINIT: cb, pszAppName, pszTopic, offConvContext.
            let p = mp2 as *const u32;
            let (app, topic) = unsafe { (*(p.add(1)) as *const u8, *(p.add(2)) as *const u8) };
            let ok = unsafe { *p == 16 && *app == b'A' && *topic == b'T' };
            if msg == WM_DDE_INITIATE {
                DDE[0].store(msg, Ordering::Relaxed);
                DDE[1].store(mp1, Ordering::Relaxed);
                DDE[2].store(ok as u32, Ordering::Relaxed);
            } else {
                ACK.store(ok as u32 | mp1 << 1, Ordering::Relaxed);
            }
            return 1;
        }
        WM_DDE_DATA => {
            DDE[0].store(msg, Ordering::Relaxed);
            DDE[2].store(mp2, Ordering::Relaxed);
            return 0;
        }
        _ => {}
    }
    let f: WndProc = unsafe { core::mem::transmute(FRAME_PROC.load(Ordering::Relaxed) as usize) };
    f(h, msg, mp1, mp2)
}

fn cursor_info() -> Option<[i32; 10]> {
    let mut c = [0i32; 10];
    (unsafe { WinQueryCursorInfo(1, c.as_mut_ptr()) } != 0).then_some(c)
}

fn cursor_checks(hab: u32, client: u32, other: u32) {
    unsafe {
        check("WinCreateCursor", WinCreateCursor(client, 3, 4, 0, 12, 4, null()) != 0);
        let c = cursor_info();
        check("WinQueryCursorInfo (cx 0: the border width)", c.is_some_and(|c| c[0] as u32 == client && c[1..6] == [3, 4, 1, 12, 4]));
        check("a new cursor is hidden: one WinShowCursor shows it", WinShowCursor(client, 1) != 0 && WinShowCursor(client, 1) == 0);
        check("WinShowCursor of a window without the cursor", WinShowCursor(other, 0) == 0);
        check("CURSOR_SETPOS moves it", WinCreateCursor(client, 9, 8, 0, 0, 0x8000, null()) != 0 && cursor_info().is_some_and(|c| c[1..5] == [9, 8, 1, 12]));
        check("CURSOR_SETPOS of another window fails", WinCreateCursor(other, 1, 1, 0, 0, 0x8000, null()) == 0);
        WinGetLastError(hab);
        check("invalid cursor flags", WinCreateCursor(client, 1, 1, 0, 0, 0x40, null()) == 0 && WinGetLastError(hab) & 0xffff == 0x1019);
        let clip = [0i32, 0, 50, 20];
        WinCreateCursor(other, 1, 2, 2, 2, 2, clip.as_ptr());
        check("a cursor for another window replaces it", cursor_info().is_some_and(|c| c[0] as u32 == other && c[5] == 2 && c[6..] == clip));
        check("WinDestroyCursor", WinDestroyCursor(client) == 0 && WinDestroyCursor(other) != 0 && cursor_info().is_none());
    }
}

fn frame_checks(hab: u32) -> u32 {
    unsafe {
        let mut info = [0u32; 3];
        WinQueryClassInfo(hab, 0xffff_0001, info.as_mut_ptr());
        FRAME_PROC.store(info[1], Ordering::Relaxed);
        check("WinQueryClassInfo(WC_FRAME)", info[1] != 0 && info[2] >= 4);
        WinRegisterClass(hab, b"CtlDerivedFrame\0".as_ptr(), derived_frame, 0, info[2]);
        // FRAMECDATA: cb, flCreateFlags (FCF_TITLEBAR | FCF_SYSMENU),
        // hmodResources, idResources.
        let mut fcd = [0u8; 12];
        fcd[0] = 12;
        fcd[2] = 3;
        let f = WinCreateWindow(1, b"CtlDerivedFrame\0".as_ptr(), b"Derived\0".as_ptr(), 0, 10, 20, 300, 200, 0, 3, 77, fcd.as_ptr(), 0);
        check("a frame of a class derived from WC_FRAME gets its controls", f != 0 && WinWindowFromID(f, FID_TITLEBAR) != 0);
        let client = WinCreateWindow(f, b"CtlTest\0".as_ptr(), null(), 0x8000_0000, 0, 0, 0, 0, f, 3, FID_CLIENT, null(), 0);
        let mut swps = [0i32; 9 * 8];
        let n = WinSendMsg(f, WM_FORMATFRAME, swps.as_mut_ptr() as u32, 0) as usize;
        let has = (0..n.min(8)).any(|i| swps[i * 9 + 6] as u32 == client);
        check("WM_FORMATFRAME places a client made with WinCreateWindow", client != 0 && has);

        // WinSetMultWindowPos: two windows at once.
        let a = WinCreateWindow(client, b"CtlTest\0".as_ptr(), null(), 0, 0, 0, 1, 1, client, 3, 1, null(), 0);
        let b = WinCreateWindow(client, b"CtlTest\0".as_ptr(), null(), 0, 0, 0, 1, 1, client, 3, 2, null(), 0);
        let set = [3i32, 30, 40, 5, 6, 3, a as i32, 0, 0, 3, 31, 41, 7, 8, 3, b as i32, 0, 0];
        let mut pa = [0i32; 9];
        let mut pb = [0i32; 9];
        let ok = WinSetMultWindowPos(hab, set.as_ptr(), 2) != 0 && WinQueryWindowPos(a, pa.as_mut_ptr()) != 0 && WinQueryWindowPos(b, pb.as_mut_ptr()) != 0;
        check("WinSetMultWindowPos", ok && pa[1..5] == [30, 40, 5, 6] && pb[1..5] == [31, 41, 7, 8]);

        // WinStoreWindowPos / WinRestoreWindowPos.
        let stored = WinStoreWindowPos(b"CtlTest\0".as_ptr(), b"Pos\0".as_ptr(), f);
        WinSetWindowPos(f, 3, 50, 60, 100, 90, 3);
        let restored = WinRestoreWindowPos(b"CtlTest\0".as_ptr(), b"Pos\0".as_ptr(), f);
        let mut p = [0i32; 9];
        WinQueryWindowPos(f, p.as_mut_ptr());
        check("WinStoreWindowPos and WinRestoreWindowPos", stored != 0 && restored != 0 && p[1..5] == [200, 300, 20, 10]);
        check("WinRestoreWindowPos without a record", WinRestoreWindowPos(b"CtlTest\0".as_ptr(), b"NoSuchKey\0".as_ptr(), f) == 0);
        // Remove the record again.
        PrfWriteProfileData(0xffff_ffff, b"CtlTest\0".as_ptr(), null(), null(), 0);
        f
    }
}

fn dde_checks(hab: u32, client: u32, frame: u32) {
    unsafe {
        WinDdeInitiate(client, b"App\0".as_ptr(), b"Topic\0".as_ptr(), null());
        check("WinDdeInitiate reaches top-level windows with a DDEINIT", DDE[0].load(Ordering::Relaxed) == WM_DDE_INITIATE && DDE[1].load(Ordering::Relaxed) == client && DDE[2].load(Ordering::Relaxed) == 1);
        let r = WinDdeRespond(frame, client, b"App\0".as_ptr(), b"Topic\0".as_ptr(), null());
        check("WinDdeRespond sends WM_DDE_INITIATEACK", r == 1 && ACK.load(Ordering::Relaxed) == (1 | client << 1));
        let mut p: *mut u8 = null_mut();
        DosAllocSharedMem(&mut p, null(), 64, 0x213);
        let ok = WinDdePostMsg(frame, client, WM_DDE_DATA, p, 0) != 0;
        crate::pump(hab);
        check("WinDdePostMsg posts the DDESTRUCT", ok && DDE[0].load(Ordering::Relaxed) == WM_DDE_DATA && DDE[2].load(Ordering::Relaxed) == p as u32);
    }
}

pub fn wm_checks(hab: u32, hmq: u32, client: u32, other: u32) {
    cursor_checks(hab, client, other);
    let f = frame_checks(hab);
    dde_checks(hab, client, f);
    unsafe {
        WinDestroyWindow(f);
        let mut qi = [0u32; 5];
        check("WinQueryQueueInfo", WinQueryQueueInfo(hmq, qi.as_mut_ptr(), 20) != 0 && qi[0] == 20 && qi[2] == 1);
        let (mut ret, mut total, mut need) = (9, 9, 9);
        let rc = SplEnumQueue(null(), 3, null_mut(), 0, &mut ret, &mut total, &mut need, 0);
        check("SplEnumQueue: no printer queues", rc == 0 && ret == 0 && total == 0 && need == 0);
        check("SplQueryQueue: NERR_QNotFound", SplQueryQueue(null(), b"LPT1Q\0".as_ptr(), 3, null_mut(), 0, &mut need) == 2150);
    }
}
