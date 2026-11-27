//! The handle table: Windows HANDLEs are `(index + 1) * 4` into it. The
//! standard handles start as the process's OS/2 handles 0, 1 and 2.

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::sync::Waitable;
use crate::{err, fail, Global, FALSE, TRUE};

pub const INVALID_HANDLE_VALUE: u32 = 0xffff_ffff;
/// GetCurrentProcess() and GetCurrentThread().
pub const CURRENT_PROCESS: u32 = 0xffff_ffff;
pub const CURRENT_THREAD: u32 = 0xffff_fffe;

#[derive(Clone)]
pub enum Obj {
    /// An OS/2 file handle (files, devices, pipes).
    File { hf: u32 },
    /// A directory search (FindFirstFile).
    Find { hdir: u32 },
    /// Events, mutexes, semaphores, threads, processes.
    Wait(Arc<Waitable>),
}

static TABLE: Global<Vec<Option<Obj>>> = Global::new(Vec::new());
/// The standard handles (STD_INPUT, STD_OUTPUT, STD_ERROR).
static STD: Global<[u32; 3]> = Global::new([0; 3]);

pub fn init() {
    let hs: [u32; 3] = core::array::from_fn(|i| add(Obj::File { hf: i as u32 }));
    STD.with(|s| *s = hs);
}

pub fn add(o: Obj) -> u32 {
    TABLE.with(|t| {
        let i = match t.iter().position(|x| x.is_none()) {
            Some(i) => {
                t[i] = Some(o);
                i
            }
            None => {
                t.push(Some(o));
                t.len() - 1
            }
        };
        (i as u32 + 1) * 4
    })
}

pub fn get(h: u32) -> Option<Obj> {
    if h == 0 || h % 4 != 0 {
        return None;
    }
    TABLE.with(|t| t.get((h / 4 - 1) as usize).cloned().flatten())
}

pub fn remove(h: u32) -> Option<Obj> {
    if h == 0 || h % 4 != 0 {
        return None;
    }
    TABLE.with(|t| t.get_mut((h / 4 - 1) as usize).and_then(|x| x.take()))
}

/// The OS/2 file handle behind a handle.
pub fn file(h: u32) -> Option<u32> {
    match get(h) {
        Some(Obj::File { hf }) => Some(hf),
        _ => None,
    }
}

pub fn waitable(h: u32) -> Option<Arc<Waitable>> {
    if h == CURRENT_PROCESS || h == CURRENT_THREAD {
        return None;
    }
    match get(h) {
        Some(Obj::Wait(w)) => Some(w),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn CloseHandle(h: u32) -> i32 {
    if h == CURRENT_PROCESS || h == CURRENT_THREAD {
        return TRUE;
    }
    match remove(h) {
        Some(Obj::File { hf }) => {
            // The standard handles stay open (programs close them at
            // their end; their OS/2 handles belong to the session).
            if hf > 2 {
                unsafe { os2::DosClose(hf) };
            }
            TRUE
        }
        Some(Obj::Find { hdir }) => {
            unsafe { os2::DosFindClose(hdir) };
            TRUE
        }
        Some(Obj::Wait(_)) => TRUE,
        None => fail(err::INVALID_HANDLE, FALSE),
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn DuplicateHandle(_sp: u32, h: u32, _tp: u32, out: *mut u32, _access: u32, _inherit: i32, options: u32) -> i32 {
    let o = match h {
        CURRENT_PROCESS => Some(Obj::Wait(crate::process::current_process_object())),
        CURRENT_THREAD => crate::sync::current_thread_object().map(Obj::Wait),
        _ => get(h),
    };
    let Some(o) = o else { return fail(err::INVALID_HANDLE, FALSE) };
    let o = match o {
        Obj::File { hf } => {
            let mut new = 0xffff_ffffu32;
            if unsafe { os2::DosDupHandle(hf, &mut new) } != 0 {
                return fail(err::INVALID_HANDLE, FALSE);
            }
            Obj::File { hf: new }
        }
        x => x,
    };
    if !out.is_null() {
        unsafe { *out = add(o) };
    }
    // DUPLICATE_CLOSE_SOURCE
    if options & 1 != 0 {
        CloseHandle(h);
    }
    TRUE
}

pub const STD_INPUT_HANDLE: u32 = 0xffff_fff6;
pub const STD_OUTPUT_HANDLE: u32 = 0xffff_fff5;
pub const STD_ERROR_HANDLE: u32 = 0xffff_fff4;

fn std_index(n: u32) -> Option<usize> {
    match n {
        STD_INPUT_HANDLE => Some(0),
        STD_OUTPUT_HANDLE => Some(1),
        STD_ERROR_HANDLE => Some(2),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetStdHandle(n: u32) -> u32 {
    match std_index(n) {
        Some(i) => STD.with(|s| s[i]),
        None => fail(err::INVALID_HANDLE, INVALID_HANDLE_VALUE),
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetStdHandle(n: u32, h: u32) -> i32 {
    match std_index(n) {
        Some(i) => {
            STD.with(|s| s[i] = h);
            TRUE
        }
        None => fail(err::INVALID_HANDLE, FALSE),
    }
}

/// The OS/2 handle of a standard handle (for the console and for child
/// processes).
pub fn std_file(i: usize) -> Option<u32> {
    file(STD.with(|s| s[i]))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetHandleCount(n: u32) -> u32 {
    n
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetHandleInformation(h: u32, flags: *mut u32) -> i32 {
    if get(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    if !flags.is_null() {
        unsafe { *flags = 0 };
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SetHandleInformation(h: u32, _mask: u32, _flags: u32) -> i32 {
    if get(h).is_none() {
        return fail(err::INVALID_HANDLE, FALSE);
    }
    TRUE
}
