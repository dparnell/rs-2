//! The system-wide part of the REXX API: external data queues,
//! registrations made from DLLs and the macro space live in one named
//! shared memory object (the same address in every process) managed with
//! DosSubAllocMem, behind one named mutex. Registrations of entry points
//! in executables are the registering process's own (crate::local).

use core::ptr::{null_mut, copy_nonoverlapping};

use os2::*;

const MEM_NAME: &[u8] = b"\\SHAREMEM\\RS2\\REXXAPI.MEM\0";
const MUTEX_NAME: &[u8] = b"\\SEM32\\RS2\\REXXAPI.SEM\0";
const EVENT_NAME: &[u8] = b"\\SEM32\\RS2\\REXXAPI.EV\0";
const SIZE: u32 = 2 * 1024 * 1024;
const HEAP_OFF: u32 = 0x1000;
const MAGIC: u32 = 0x5852_5253; // "SRRX"

unsafe extern "C" {
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> APIRET;
    #[link_name = "DOSCALLS.332"]
    fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> APIRET;
}

#[repr(C)]
pub struct Root {
    magic: u32,
    pub queues: *mut Queue,
    pub regs: *mut Reg,
    pub macros: *mut Macro,
    pub counter: u32,
}

#[repr(C)]
pub struct Queue {
    pub next: *mut Queue,
    pub first: *mut Item,
    pub last: *mut Item,
    pub count: u32,
    /// Session number of a session queue (0: a named queue).
    pub session: u32,
    pub name: *mut u8,
}

#[repr(C)]
pub struct Item {
    pub next: *mut Item,
    pub len: u32,
    pub time: [u8; 12],
    // data follows
}

/// A registration from a DLL: function (1), subcommand handler (2),
/// exit (3).
#[repr(C)]
pub struct Reg {
    pub next: *mut Reg,
    pub kind: u32,
    pub name: *mut u8,
    pub module: *mut u8,
    pub proc_: *mut u8,
    pub user: [u8; 8],
    pub drop: u32,
    pub pid: u32,
}

#[repr(C)]
pub struct Macro {
    pub next: *mut Macro,
    pub name: *mut u8,
    pub pos: u32,
    pub len: u32,
    pub data: *mut u8,
}

static mut ROOT: *mut Root = null_mut();
static mut MUTEX: u32 = 0;
static mut EVENT: u32 = 0;

fn heap() -> *mut u8 {
    unsafe { (ROOT as *mut u8).add(HEAP_OFF as usize) }
}

/// The API's shared state, locked; None if it cannot be set up.
pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe { DosReleaseMutexSem(MUTEX) };
    }
}

impl Guard {
    pub fn root(&self) -> &mut Root {
        unsafe { &mut *ROOT }
    }
}

pub fn lock() -> Option<Guard> {
    unsafe {
        if MUTEX == 0 {
            let mut h = 0;
            let mut rc = DosCreateMutexSem(MUTEX_NAME.as_ptr(), &mut h, 0, 0);
            if rc != 0 {
                rc = DosOpenMutexSem(MUTEX_NAME.as_ptr(), &mut h);
            }
            if rc != 0 {
                return None;
            }
            MUTEX = h;
        }
        if DosRequestMutexSem(MUTEX, u32::MAX) != 0 {
            return None;
        }
        if ROOT.is_null() {
            let mut p: *mut u8 = null_mut();
            let mut rc = DosGetNamedSharedMem(&mut p, MEM_NAME.as_ptr(), PAG_READ | PAG_WRITE);
            if rc != 0 {
                rc = DosAllocSharedMem(&mut p, MEM_NAME.as_ptr(), SIZE, PAG_COMMIT | PAG_READ | PAG_WRITE);
            }
            if rc != 0 || p.is_null() {
                DosReleaseMutexSem(MUTEX);
                return None;
            }
            let root = p as *mut Root;
            if (*root).magic != MAGIC {
                core::ptr::write_bytes(p, 0, HEAP_OFF as usize);
                DosSubSetMem(p.add(HEAP_OFF as usize), 1, SIZE - HEAP_OFF);
                (*root).magic = MAGIC;
            }
            ROOT = root;
            let mut ev = 0;
            if DosCreateEventSem(EVENT_NAME.as_ptr(), &mut ev, 0, 0) != 0 {
                DosOpenEventSem(EVENT_NAME.as_ptr(), &mut ev);
            }
            EVENT = ev;
        }
        Some(Guard)
    }
}

/// Wake processes waiting for a queue entry.
pub fn post() {
    unsafe { DosPostEventSem(EVENT) };
}

/// Wait for a queue entry to be added (the lock is not held).
pub fn wait() {
    unsafe {
        let mut n = 0;
        DosResetEventSem(EVENT, &mut n);
        DosWaitEventSem(EVENT, 200);
    }
}

/// Allocate shared memory (with an 8-byte size header).
pub fn alloc(n: usize) -> *mut u8 {
    let total = (n + 8) as u32;
    let mut p: *mut u8 = null_mut();
    if unsafe { DosSubAllocMem(heap(), &mut p, total) } != 0 {
        return null_mut();
    }
    unsafe {
        *(p as *mut u32) = total;
        p.add(8)
    }
}

pub fn free(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe {
        let b = p.sub(8);
        let total = *(b as *const u32);
        DosSubFreeMem(heap(), b, total);
    }
}

/// A copy of a byte string as a C string in shared memory.
pub fn dup(s: &[u8]) -> *mut u8 {
    let p = alloc(s.len() + 1);
    if !p.is_null() {
        unsafe {
            copy_nonoverlapping(s.as_ptr(), p, s.len());
            *p.add(s.len()) = 0;
        }
    }
    p
}

pub fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return b"";
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

pub fn eq_nocase(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_ascii_uppercase() == y.to_ascii_uppercase())
}
