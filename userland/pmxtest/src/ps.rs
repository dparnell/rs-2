//! Presentation spaces as IBM's PMGPI and PMWIN hand them out: what
//! GpiCreatePS accepts (options, page size, one PS per device context),
//! WinReleasePS for cached presentation spaces only, and cached ones
//! going with their window and with the message queue that got them.

use core::ptr::null;

use os2::*;

use crate::check;
use crate::pm::*;

unsafe extern "C" {
    #[link_name = "PMGPI.369"]
    fn GpiCreatePS(hab: u32, hdc: u32, size: *const i32, opts: u32) -> u32;
    #[link_name = "PMGPI.379"]
    fn GpiDestroyPS(hps: u32) -> u32;
    #[link_name = "PMGPI.351"]
    fn GpiAssociate(hps: u32, hdc: u32) -> u32;
    #[link_name = "PMGPI.517"]
    fn GpiSetColor(hps: u32, clr: i32) -> u32;
    #[link_name = "PMGPI.610"]
    fn DevOpenDC(hab: u32, typ: i32, token: *const u8, n: i32, data: *const u32, comp: u32) -> u32;
    #[link_name = "PMGPI.604"]
    fn DevCloseDC(hdc: u32) -> u32;
    #[link_name = "PMWIN.753"]
    fn WinGetLastError(hab: u32) -> u32;
}

const OD_MEMORY: i32 = 8;
const PU_PELS: u32 = 8;
const GPIT_MICRO: u32 = 0x1000;
const GPIA_ASSOC: u32 = 0x4000;
const PMERR_DC_IS_ASSOCIATED: u32 = 0x2017;
const PMERR_INV_HPS: u32 = 0x207f;
const PMERR_INV_OR_INCOMPAT_OPTIONS: u32 = 0x20a9;
const PMERR_INV_PS_SIZE: u32 = 0x20ba;

fn err() -> u32 {
    unsafe { WinGetLastError(0) & 0xffff }
}

static mut THREAD_PS: u32 = 0;
static mut WINDOW: u32 = 0;

/// A second thread: a message queue, a cached PS of the window, the
/// queue destroyed without releasing it.
unsafe extern "C" fn thread(_: u32) {
    unsafe {
        let hab = WinInitialize(0);
        let hmq = WinCreateMsgQueue(hab, 0);
        THREAD_PS = WinGetPS(WINDOW);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
}

pub fn ps_checks(hab: u32, client: u32) {
    let size = [100i32, 100];
    let zero = [0i32, 0];
    unsafe { WinGetLastError(0) };
    let a = unsafe { GpiCreatePS(hab, 0, null(), PU_PELS) };
    let e1 = err();
    let b = unsafe { GpiCreatePS(hab, 0, zero.as_ptr(), PU_PELS) };
    let e2 = err();
    let c = unsafe { GpiCreatePS(hab, 0, size.as_ptr(), 0) };
    let e3 = err();
    let d = unsafe { GpiCreatePS(hab, 0, size.as_ptr(), PU_PELS | GPIT_MICRO) };
    let e4 = err();
    check(
        "GpiCreatePS refuses no page size, no units and an unassociated micro PS (IBM's errors)",
        a == 0 && e1 == PMERR_INV_PS_SIZE && b == 0 && e2 == PMERR_INV_OR_INCOMPAT_OPTIONS && c == 0 && e3 == PMERR_INV_OR_INCOMPAT_OPTIONS && d == 0 && e4 == PMERR_INV_OR_INCOMPAT_OPTIONS,
    );

    let dc = unsafe { DevOpenDC(hab, OD_MEMORY, b"*\0".as_ptr(), 0, null(), 0) };
    let p1 = unsafe { GpiCreatePS(hab, dc, zero.as_ptr(), PU_PELS | GPIT_MICRO | GPIA_ASSOC) };
    let p2 = unsafe { GpiCreatePS(hab, dc, zero.as_ptr(), PU_PELS | GPIA_ASSOC) };
    let e5 = err();
    unsafe { GpiAssociate(p1, 0) };
    let p3 = unsafe { GpiCreatePS(hab, dc, zero.as_ptr(), PU_PELS | GPIA_ASSOC) };
    check("one presentation space per device context (PMERR_DC_IS_ASSOCIATED)", dc != 0 && p1 != 0 && p2 == 0 && e5 == PMERR_DC_IS_ASSOCIATED && p3 != 0);
    let r = unsafe { WinReleasePS(p3) };
    let e6 = err();
    check("WinReleasePS of a GpiCreatePS presentation space: PMERR_INV_HPS", r == 0 && e6 == PMERR_INV_HPS && unsafe { GpiSetColor(p3, 1) } != 0);
    unsafe {
        GpiDestroyPS(p1);
        GpiDestroyPS(p3);
        DevCloseDC(dc);
    }

    // A cached PS goes with its window.
    let w = unsafe { WinCreateWindow(client, b"PmxTest\0".as_ptr(), null(), 0x8000_0000, 10, 10, 50, 50, client, 3, 81, null(), null()) };
    let hps = unsafe { WinGetPS(w) };
    let ok = unsafe { GpiSetColor(hps, 2) } != 0;
    unsafe { WinDestroyWindow(w) };
    let r = unsafe { WinReleasePS(hps) };
    check("a cached PS goes with its window", hps != 0 && ok && r == 0 && err() == PMERR_INV_HPS);

    // And with the message queue of the thread that got it.
    let w = unsafe { WinCreateWindow(client, b"PmxTest\0".as_ptr(), null(), 0x8000_0000, 10, 10, 50, 50, client, 3, 82, null(), null()) };
    unsafe { WINDOW = w };
    let mut tid = 0;
    unsafe {
        DosCreateThread(&mut tid, thread, 0, 0, 0x10000);
        DosWaitThread(&mut tid, 0);
    }
    let tps = unsafe { THREAD_PS };
    let r = unsafe { WinReleasePS(tps) };
    check("a cached PS goes with its thread's message queue", tps != 0 && r == 0 && err() == PMERR_INV_HPS);
    unsafe { WinDestroyWindow(w) };
}
