//! The switch list calls (WinAddSwitchEntry & co.). The switch list is
//! PM state, kept by PMWIN (src/switch.rs there, rs2-private ordinals
//! 9010-9016); PMSHAPI does not import PMWIN (text-mode programs load
//! it), so these find PMWIN in the process, which every program with
//! windows has. Without it they fail.

use crate::pmwin;

/// PMERR_INVALID_SWITCH_HANDLE: what the calls that return an error code
/// return without PMWIN.
const PMERR_INVALID_SWITCH_HANDLE: u32 = 0x1202;

fn call1(ord: u32, a: u32) -> Option<u32> {
    let f: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(pmwin(ord)?) };
    Some(f(a))
}

fn call2(ord: u32, a: u32, b: u32) -> Option<u32> {
    let f: extern "C" fn(u32, u32) -> u32 = unsafe { core::mem::transmute(pmwin(ord)?) };
    Some(f(a, b))
}

/// WinAddSwitchEntry(pswctl): the entry's handle, 0 on errors.
#[unsafe(no_mangle)]
pub extern "C" fn WinAddSwitchEntry(p: u32) -> u32 {
    call1(9010, p).unwrap_or(0)
}

/// WinCreateSwitchEntry(hab, pswctl): WinAddSwitchEntry.
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateSwitchEntry(_hab: u32, p: u32) -> u32 {
    call1(9010, p).unwrap_or(0)
}

/// WinChangeSwitchEntry(hsw, pswctl): 0, or an error code.
#[unsafe(no_mangle)]
pub extern "C" fn WinChangeSwitchEntry(h: u32, p: u32) -> u32 {
    call2(9011, h, p).unwrap_or(PMERR_INVALID_SWITCH_HANDLE)
}

/// WinQuerySwitchEntry(hsw, pswctl): 0, or an error code.
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySwitchEntry(h: u32, p: u32) -> u32 {
    call2(9012, h, p).unwrap_or(PMERR_INVALID_SWITCH_HANDLE)
}

/// WinRemoveSwitchEntry(hsw): 0, or an error code.
#[unsafe(no_mangle)]
pub extern "C" fn WinRemoveSwitchEntry(h: u32) -> u32 {
    call1(9013, h).unwrap_or(PMERR_INVALID_SWITCH_HANDLE)
}

/// WinQuerySwitchHandle(hwnd, idProcess): the entry of a window or
/// process, 0 if none.
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySwitchHandle(hwnd: u32, pid: u32) -> u32 {
    call2(9014, hwnd, pid).unwrap_or(0)
}

/// WinQuerySwitchList(hab, pswblk, cbBuf): the number of entries (see
/// PMWIN).
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySwitchList(hab: u32, buf: u32, cb: u32) -> u32 {
    let Some(f) = pmwin(9015) else { return 0 };
    let f: extern "C" fn(u32, u32, u32) -> u32 = unsafe { core::mem::transmute(f) };
    f(hab, buf, cb)
}

/// WinSwitchToProgram(hsw): 0, or an error code.
#[unsafe(no_mangle)]
pub extern "C" fn WinSwitchToProgram(h: u32) -> u32 {
    call1(9016, h).unwrap_or(PMERR_INVALID_SWITCH_HANDLE)
}
