//! PMVIOP.DLL: IBM's AVIO and VIO window DLL. What programs import of it
//! is `WinDefAVioWindowProc` (ordinal 30, a 16-bit `_pascal far` entry
//! the Toolkit's os2386.lib and os2286.lib both import by ordinal,
//! `WIN16DEFAVIOWINDOWPROC`): the default window procedure of a window
//! showing a VIO presentation space. rs2 keeps AVIO presentation spaces
//! in the kernel and draws them in PMWIN (VIOCALLS 55-66, docs/PM.md
//! "AVIO"); IBM's other PMVIOP entries are its VIO window (shield),
//! console and Win-OS/2 clipboard internals, called by BVHWNDW, PMVDMP,
//! SEAMLESS and PMMERGE, which rs2 does not have (its windowed VIO
//! sessions are PMVIOWIN.EXE, its DOS sessions VDM.EXE).

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

use thunk16::Pascal;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

unsafe extern "C" {
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, rcl: *mut i32) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.9030"]
    fn Rs2AvioPaint(h: u32) -> u32;
}

const WM_PAINT: u32 = 0x0023;

thunk16::entries16_pascal! {
    dispatch = dispatch16;
    WIN16DEFAVIOWINDOWPROC = 30, 14;
}

extern "C" fn dispatch16(idx: u32, args: *const u8) -> u32 {
    let mut a = Pascal::new(args, 14);
    match idx {
        30 => {
            let (h, msg, mp1, mp2) = (a.l(), a.w() as u32, a.l(), a.l());
            WinDefAVioWindowProc(h, msg, mp1, mp2)
        }
        _ => 0,
    }
}

/// WinDefAVioWindowProc(hwnd, msg, mp1, mp2): WM_PAINT draws the whole
/// presentation space associated with the window's device context (as a
/// VioShowPS would) and validates the window; everything else, and a
/// window without a VIO presentation space, goes to WinDefWindowProc.
fn WinDefAVioWindowProc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == WM_PAINT {
        let hps = unsafe { WinBeginPaint(h, 0, core::ptr::null_mut()) };
        unsafe {
            Rs2AvioPaint(h);
            WinEndPaint(hps);
        }
        return 0;
    }
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}
