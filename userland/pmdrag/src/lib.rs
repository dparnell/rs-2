//! PMDRAG.DLL: direct manipulation (drag and drop).
//!
//! rs2 does not start drags yet, so no DM_* message ever arrives and
//! there is never drag information to access: the calls that would
//! start a drag or read its data fail, the presentation space calls work
//! on the window, and string handles work within a process so programs
//! that set up drag items before a drag find their names again.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate os2;

mod exports;

use core::sync::atomic::{AtomicU32, Ordering};

unsafe extern "C" {
    #[link_name = "PMWIN.757"]
    fn PmWinGetPS(hwnd: u32) -> u32;
    #[link_name = "PMWIN.848"]
    fn PmWinReleasePS(hps: u32) -> u32;
    #[link_name = "PMWIN.920"]
    fn PmWinSendMsg(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

// ---- String handles ----

const STRINGS: usize = 64;
const STRLEN: usize = 260;
static mut TABLE: [[u8; STRLEN]; STRINGS] = [[0; STRLEN]; STRINGS];
static USED: [AtomicU32; STRINGS] = [const { AtomicU32::new(0) }; STRINGS];

fn slot(h: u32) -> Option<usize> {
    let i = (h as usize).checked_sub(1)?;
    (i < STRINGS && USED[i].load(Ordering::Acquire) != 0).then_some(i)
}

fn name(i: usize) -> &'static [u8] {
    let t = unsafe { &(*core::ptr::addr_of!(TABLE))[i] };
    &t[..t.iter().position(|&b| b == 0).unwrap_or(STRLEN)]
}

/// DrgAddStrHandle(psz): a handle for a string (the same string gets
/// the same handle).
#[unsafe(no_mangle)]
pub extern "C" fn DrgAddStrHandle(psz: *const u8) -> u32 {
    if psz.is_null() {
        return 0;
    }
    let mut s = [0u8; STRLEN];
    let mut n = 0;
    while n < STRLEN - 1 {
        let b = unsafe { *psz.add(n) };
        if b == 0 {
            break;
        }
        s[n] = b;
        n += 1;
    }
    for i in 0..STRINGS {
        if USED[i].load(Ordering::Acquire) != 0 && name(i) == &s[..n] {
            USED[i].fetch_add(1, Ordering::AcqRel);
            return i as u32 + 1;
        }
    }
    for i in 0..STRINGS {
        if USED[i].compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            unsafe { (*core::ptr::addr_of_mut!(TABLE))[i] = s };
            return i as u32 + 1;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgDeleteStrHandle(h: u32) -> u32 {
    match slot(h) {
        Some(i) => {
            USED[i].fetch_sub(1, Ordering::AcqRel);
            1
        }
        None => 0,
    }
}

/// DrgQueryStrName(hstr, cbBuffer, pBuffer): bytes copied (without the
/// NUL).
#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryStrName(h: u32, cb: u32, buf: *mut u8) -> u32 {
    let Some(i) = slot(h) else { return 0 };
    if buf.is_null() || cb == 0 {
        return 0;
    }
    let s = name(i);
    let n = s.len().min(cb as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryStrNameLen(h: u32) -> u32 {
    slot(h).map_or(0, |i| name(i).len() as u32)
}

// ---- Presentation spaces for target emphasis ----

#[unsafe(no_mangle)]
pub extern "C" fn DrgGetPS(hwnd: u32) -> u32 {
    unsafe { PmWinGetPS(hwnd) }
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgReleasePS(hps: u32) -> u32 {
    unsafe { PmWinReleasePS(hps) }
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgSendTransferMsg(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe { PmWinSendMsg(hwnd, msg, mp1, mp2) }
}

// ---- Drag information: there is never a drag in progress ----

#[unsafe(no_mangle)]
pub extern "C" fn DrgAccessDraginfo(_pdinfo: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgAllocDraginfo(_n: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgReallocDraginfo(_old: u32, _n: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgAllocDragtransfer(_n: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgFreeDraginfo(_pdinfo: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgFreeDragtransfer(_p: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgDeleteDraginfoStrHandles(_pdinfo: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgDrag(_src: u32, _pdinfo: u32, _pdimg: u32, _n: u32, _vk: u32, _rsvd: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgDragFiles(_h: u32, _files: u32, _types: u32, _targets: u32, _n: u32, _ptr: u32, _vk: u32, _render: u32, _rsvd: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgAcceptDroppedFiles(_h: u32, _path: u32, _types: u32, _op: u32, _rsvd: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgLazyDrag(_src: u32, _pdinfo: u32, _pdimg: u32, _n: u32, _rsvd: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgCancelLazyDrag() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgLazyDrop(_target: u32, _op: u32, _ptl: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDragStatus() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDraginfoPtr(_rsvd: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDraginfoPtrFromHwnd(_h: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDraginfoPtrFromDragitem(_p: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgPostTransferMsg(_h: u32, _msg: u32, _p: u32, _fl: u32, _rsvd: u32, _retry: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgPushDraginfo(_pdinfo: u32, _dest: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDragitem(_pdinfo: u32, _cb: u32, _pditem: u32, _i: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDragitemCount(_pdinfo: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryDragitemPtr(_pdinfo: u32, _i: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryNativeRMF(_pditem: u32, _cb: u32, buf: *mut u8) -> u32 {
    if !buf.is_null() {
        unsafe { *buf = 0 };
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryNativeRMFLen(_pditem: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryTrueType(_pditem: u32, _cb: u32, buf: *mut u8) -> u32 {
    if !buf.is_null() {
        unsafe { *buf = 0 };
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgQueryTrueTypeLen(_pditem: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgSetDragPointer(_pdinfo: u32, _hptr: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgSetDragImage(_pdinfo: u32, _pdimg: u32, _n: u32, _rsvd: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgSetDragitem(_pdinfo: u32, _pditem: u32, _cb: u32, _i: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyNativeRMF(_pditem: u32, _rmf: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyRMF(_pditem: u32, _mech: u32, _fmt: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyTrueType(_pditem: u32, _ty: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyType(_pditem: u32, _ty: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DrgVerifyTypeSet(_pditem: u32, _ty: u32, _cb: u32, _out: u32) -> u32 {
    0
}
