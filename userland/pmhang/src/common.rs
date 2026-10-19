//! A PM program that stops reading its message queue on purpose: its
//! window procedure never returns from WM_BUTTON1DOWN (or the H key),
//! either blocked in DosSleep or spinning without system calls; with
//! button 2 it sleeps six seconds and then goes on. It tests that the rest
//! of the desktop keeps working (docs/PM.md).

use core::sync::atomic::{AtomicU32, Ordering};

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

#[repr(C)]
#[derive(Default)]
struct RectL {
    l: i32,
    b: i32,
    r: i32,
    t: i32,
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
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(h: u32, r: *mut RectL) -> u32;
    #[link_name = "PMWIN.913"]
    fn WinDrawText(hps: u32, cch: i32, text: *const u8, r: *mut RectL, fore: i32, back: i32, fl: u32) -> i32;
    #[link_name = "DOSCALLS.229"]
    fn DosSleep(ms: u32) -> u32;
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Mode {
    /// Blocked in the kernel (DosSleep).
    Sleep,
    /// Spinning in user mode, no system calls.
    Busy,
}

static MODE: AtomicU32 = AtomicU32::new(0);
static SPIN: AtomicU32 = AtomicU32::new(0);

const WM_PAINT: u32 = 0x23;
const WM_BUTTON1DOWN: u32 = 0x71;
const WM_BUTTON2DOWN: u32 = 0x74;
const WM_CHAR: u32 = 0x7a;
const KC_KEYUP: u32 = 0x40;
const KC_CHAR: u32 = 0x01;

fn hang() -> ! {
    if MODE.load(Ordering::Relaxed) == 1 {
        loop {
            SPIN.fetch_add(1, Ordering::Relaxed);
        }
    }
    loop {
        unsafe { DosSleep(u32::MAX) };
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe {
        match msg {
            WM_PAINT => {
                let hps = WinBeginPaint(h, 0, core::ptr::null_mut());
                let mut r = RectL::default();
                WinQueryWindowRect(h, &mut r);
                let busy = MODE.load(Ordering::Relaxed) == 1;
                // CLR_YELLOW / CLR_PINK
                WinFillRect(hps, &r, if busy { 3 } else { 6 });
                let t: &[u8] = if busy { b"Click (or press H) to spin forever" } else { b"Click (or press H) to block forever" };
                // DT_CENTER | DT_VCENTER, CLR_BLACK on the fill.
                WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut r, -1, 0, 0x100 | 0x400);
                WinEndPaint(hps);
                0
            }
            WM_BUTTON1DOWN => hang(),
            // Button 2: stop responding for six seconds, then carry on.
            WM_BUTTON2DOWN => {
                DosSleep(6000);
                1
            }
            WM_CHAR if mp1 & KC_KEYUP == 0 && mp1 & KC_CHAR != 0 && (mp2 & 0xff) as u8 | 0x20 == b'h' => hang(),
            _ => WinDefWindowProc(h, msg, mp1, mp2),
        }
    }
}

pub fn run(mode: Mode, title: &[u8]) -> u32 {
    MODE.store((mode == Mode::Busy) as u32, Ordering::Relaxed);
    unsafe {
        let hab = WinInitialize(0);
        let hmq = WinCreateMsgQueue(hab, 0);
        if hab == 0 || hmq == 0 {
            return 1;
        }
        WinRegisterClass(hab, b"HangTest\0".as_ptr(), client_proc, 0, 0);
        // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX |
        // FCF_SHELLPOSITION | FCF_TASKLIST
        let mut fcf = 0x1 | 0x2 | 0x8 | 0x30 | 0x400 | 0x800;
        let mut client = 0;
        let frame = WinCreateStdWindow(1, 0x8000_0000, &mut fcf, b"HangTest\0".as_ptr(), title.as_ptr(), 0, 0, 1, &mut client);
        let mut q = QMsg::default();
        while WinGetMsg(hab, &mut q, 0, 0, 0) != 0 {
            WinDispatchMsg(hab, &q);
        }
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}
