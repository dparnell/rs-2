//! The object API from other processes: the Workplace Shell's objects
//! live in the shell's process, so a program's WinCreateObject (and the
//! other object calls) goes there.
//!
//! The shell keeps a shared memory block (`\SHAREMEM\RS2WPS.MEM`) saying
//! where it is (its object window and process), and the requests pass
//! through it: a caller takes the mutex `\SEM32\RS2WPS\CALL`, writes the
//! request (an operation, numbers and strings), posts WM_WPS_CALL to the
//! shell's object window (posting needs no message queue, so programs
//! without PM can call too) and waits for the event `\SEM32\RS2WPS\DONE`,
//! which the shell posts with the result after doing the call on its PM
//! thread, where the objects and their views are.

use alloc::vec::Vec;

use crate::pm::*;
use crate::util::cstr;

unsafe extern "C" {
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> u32;
    #[link_name = "DOSCALLS.332"]
    fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> u32;
}

const MEM: &[u8] = b"\\SHAREMEM\\RS2WPS.MEM\0";
const CALL: &[u8] = b"\\SEM32\\RS2WPS\\CALL\0";
const DONE: &[u8] = b"\\SEM32\\RS2WPS\\DONE\0";
const MAGIC: u32 = 0x5350_5752; // "RWPS"
const SIZE: u32 = 0x10000;
pub const WM_WPS_CALL: u32 = WM_USER + 0x500;

// The block: magic, object window, process, operation, result, numbers
// (4), then the strings (4, NUL-terminated, at fixed places).
const O_MAGIC: usize = 0;
const O_HWND: usize = 4;
const O_PID: usize = 8;
const O_OP: usize = 12;
const O_RESULT: usize = 16;
const O_NUM: usize = 20;
const O_STR: usize = 64;
const STR_SIZE: usize = 0x3000;

pub const OP_CREATE: u32 = 1;
pub const OP_QUERY: u32 = 2;
pub const OP_SETDATA: u32 = 3;
pub const OP_DESTROY: u32 = 4;
pub const OP_OPEN: u32 = 5;
pub const OP_SAVE: u32 = 6;
pub const OP_MOVE: u32 = 7;
pub const OP_COPY: u32 = 8;
pub const OP_SHADOW: u32 = 9;
pub const OP_REGISTER: u32 = 10;
pub const OP_DEREGISTER: u32 = 11;

static mut BLOCK: u32 = 0;
static mut DONE_SEM: u32 = 0;

fn my_pid() -> u32 {
    let mut tib = core::ptr::null_mut();
    let mut pib = core::ptr::null_mut();
    unsafe {
        os2::DosGetInfoBlocks(&mut tib, &mut pib);
        (*pib).pid
    }
}

// ---- The shell's side ----

extern "C" fn server_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    if msg == WM_WPS_CALL {
        serve();
        return 0;
    }
    unsafe { WinDefWindowProc(h, msg, a, b) }
}

/// Make the shell reachable (the shell's process, on its PM thread).
pub fn start_server(hab: u32) -> bool {
    let mut p: *mut u8 = core::ptr::null_mut();
    // PAG_READ | PAG_WRITE | PAG_COMMIT
    if unsafe { os2::DosAllocSharedMem(&mut p, MEM.as_ptr(), SIZE, 0x13) } != 0 {
        return false;
    }
    let mut mtx = 0;
    let mut ev = 0;
    unsafe {
        os2::DosCreateMutexSem(CALL.as_ptr(), &mut mtx, 0, 0);
        os2::DosCreateEventSem(DONE.as_ptr(), &mut ev, 0, 0);
        DONE_SEM = ev;
        WinRegisterClass(hab, b"WPS_Server\0".as_ptr(), server_proc, 0, 0);
    }
    // HWND_OBJECT (an object window: never shown).
    let hwnd = unsafe { WinCreateWindow(2, b"WPS_Server\0".as_ptr() as u32, b"\0".as_ptr(), 0, 0, 0, 0, 0, 0, HWND_TOP, 0, core::ptr::null(), core::ptr::null()) };
    let base = p as u32;
    unsafe { BLOCK = base };
    wr(base, O_HWND, hwnd);
    wr(base, O_PID, my_pid());
    wr(base, O_MAGIC, MAGIC);
    hwnd != 0
}

fn str_at(base: u32, i: usize) -> Vec<u8> {
    cstr((base as usize + O_STR + i * STR_SIZE) as *const u8).to_vec()
}

fn serve() {
    let base = unsafe { BLOCK };
    if base == 0 {
        return;
    }
    let n = |i: usize| rd(base, O_NUM + 4 * i);
    let s = |i: usize| str_at(base, i);
    let z = |v: Vec<u8>| crate::util::z(&v);
    let r = match rd(base, O_OP) {
        OP_CREATE => crate::api::create_object(&s(0), &s(1), &s(2), &s(3), n(0)),
        OP_QUERY => {
            let id = z(s(0));
            crate::api::WinQueryObject(id.as_ptr())
        }
        OP_SETDATA => {
            let setup = z(s(0));
            crate::api::WinSetObjectData(n(0), setup.as_ptr())
        }
        OP_DESTROY => crate::api::WinDestroyObject(n(0)),
        OP_OPEN => crate::api::WinOpenObject(n(0), n(1), n(2)),
        OP_SAVE => crate::api::WinSaveObject(n(0), n(1)),
        OP_MOVE => crate::api::WinMoveObject(n(0), n(1), n(2)),
        OP_COPY => crate::api::WinCopyObject(n(0), n(1), n(2)),
        OP_SHADOW => crate::api::WinCreateShadow(n(0), n(1), n(2)),
        OP_REGISTER => {
            let (c, m) = (z(s(0)), z(s(1)));
            crate::api::WinRegisterObjectClass(c.as_ptr(), m.as_ptr())
        }
        OP_DEREGISTER => {
            let c = z(s(0));
            crate::api::WinDeregisterObjectClass(c.as_ptr())
        }
        _ => 0,
    };
    wr(base, O_RESULT, r);
    unsafe { os2::DosPostEventSem(DONE_SEM) };
}

// ---- The callers' side ----

/// The shell's block, if a shell is running in another process.
fn shell() -> Option<u32> {
    let mut p: *mut u8 = core::ptr::null_mut();
    // PAG_READ | PAG_WRITE
    if unsafe { os2::DosGetNamedSharedMem(&mut p, MEM.as_ptr(), 0x03) } != 0 || p.is_null() {
        return None;
    }
    let base = p as u32;
    (rd(base, O_MAGIC) == MAGIC && rd(base, O_PID) != my_pid() && unsafe { WinIsWindow(0, rd(base, O_HWND)) } != 0).then_some(base)
}

/// Whether a Workplace Shell runs in another process.
pub fn shell_running() -> bool {
    shell().is_some()
}

/// Do a call in the shell's process; None without a shell.
pub fn call(op: u32, nums: &[u32], strs: &[&[u8]]) -> Option<u32> {
    let base = shell()?;
    let (mut mtx, mut ev) = (0u32, 0u32);
    unsafe {
        if DosOpenMutexSem(CALL.as_ptr(), &mut mtx) != 0 || DosOpenEventSem(DONE.as_ptr(), &mut ev) != 0 {
            return None;
        }
        if os2::DosRequestMutexSem(mtx, 60_000) != 0 {
            os2::DosCloseMutexSem(mtx);
            os2::DosCloseEventSem(ev);
            return None;
        }
    }
    wr(base, O_OP, op);
    for (i, &v) in nums.iter().take(4).enumerate() {
        wr(base, O_NUM + 4 * i, v);
    }
    for (i, s) in strs.iter().take(4).enumerate() {
        let at = (base as usize + O_STR + i * STR_SIZE) as *mut u8;
        let n = s.len().min(STR_SIZE - 1);
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), at, n);
            *at.add(n) = 0;
        }
    }
    wr(base, O_RESULT, 0);
    let mut count = 0;
    let r = unsafe {
        os2::DosResetEventSem(ev, &mut count);
        WinPostMsg(rd(base, O_HWND), WM_WPS_CALL, 0, 0);
        os2::DosWaitEventSem(ev, 60_000)
    };
    let result = rd(base, O_RESULT);
    unsafe {
        os2::DosReleaseMutexSem(mtx);
        os2::DosCloseMutexSem(mtx);
        os2::DosCloseEventSem(ev);
    }
    (r == 0).then_some(result)
}
