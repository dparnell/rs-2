//! W32KERN.DLL: KERNEL32 of rs2's Win32 layer (docs/WIN32.md). Windows
//! programs' imports of KERNEL32 come here (the kernel's PE loader maps
//! the name); every call is a Windows `stdcall` function on rs2's OS/2
//! API.
//!
//! - handles.rs: the handle table (files, events, mutexes, semaphores,
//!   threads, processes, searches);
//! - file.rs: files, directories and paths;
//! - mem.rs: VirtualAlloc and the heaps;
//! - sync.rs: threads, waits, events, mutexes, semaphores, critical
//!   sections, TLS slots, the Interlocked calls;
//! - process.rs: the command line, the environment, processes, modules;
//! - text.rs: code pages and strings;
//! - time.rs: times and clocks;
//! - console.rs: the console on VIO and KBD;
//! - misc.rs: versions, system information, errors, the rest.
//!
//! Text is the process's OS/2 code page (GetACP says which); W calls
//! convert to and from UTF-16 with it.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]
#![allow(dead_code)]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

mod console;
mod cptab;
mod file;
mod handles;
mod mem;
mod misc;
mod process;
mod sync;
mod text;
mod time;
mod thunks;
#[path = "../../w32ntdll/src/pe.rs"]
mod pe;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, flag: u32) -> u32 {
    if flag == 0 {
        process::init();
        handles::init();
        console::init();
    }
    1
}

// ---- W32NTDLL ----

unsafe extern "C" {
    #[link_name = "W32NTDLL.Rs2W32ThreadAttach"]
    pub fn Rs2W32ThreadAttach();
    #[link_name = "W32NTDLL.Rs2W32ThreadDetach"]
    pub fn Rs2W32ThreadDetach();
    #[link_name = "W32NTDLL.Rs2W32ProcessDetach"]
    pub fn Rs2W32ProcessDetach();
    #[link_name = "W32NTDLL.Rs2W32InitModules"]
    pub fn Rs2W32InitModules() -> u32;
    #[link_name = "W32NTDLL.Rs2W32SetUnhandledFilter"]
    pub fn Rs2W32SetUnhandledFilter(f: u32) -> u32;
    #[link_name = "W32NTDLL.Rs2W32QueryUnhandledFilter"]
    pub fn Rs2W32QueryUnhandledFilter() -> u32;
    #[link_name = "W32NTDLL.Rs2W32HmteOfBase"]
    pub fn Rs2W32HmteOfBase(base: u32) -> u32;
    #[link_name = "W32NTDLL.Rs2W32BaseOfHmte"]
    pub fn Rs2W32BaseOfHmte(hmte: u32) -> u32;
    #[link_name = "RS2API.433"]
    pub fn Rs2W32Modules(buf: *mut u32, max: u32) -> u32;
    #[link_name = "RS2API.434"]
    pub fn Rs2W32AllocAt(addr: u32, size: u32, flags: u32) -> u32;
    #[link_name = "RS2API.4"]
    pub fn Rs2DebugTrace(psz: *const u8) -> u32;
}

// ---- The TEB and errors ----

pub const TEB_LAST_ERROR: usize = 0x34;
pub const TEB_TID: usize = 0x24;
pub const TEB_PID: usize = 0x20;
pub const TEB_PEB: usize = 0x30;
pub const TEB_TLS_SLOTS: usize = 0xe10;

#[inline]
pub fn teb() -> *mut u8 {
    let t: u32;
    unsafe { core::arch::asm!("mov {}, fs:[0x18]", out(reg) t, options(nostack, readonly, preserves_flags)) };
    t as *mut u8
}

pub fn teb_get(off: usize) -> u32 {
    unsafe { (teb().add(off) as *const u32).read_volatile() }
}

pub fn teb_set(off: usize, v: u32) {
    unsafe { (teb().add(off) as *mut u32).write_volatile(v) }
}

pub fn set_error(e: u32) {
    teb_set(TEB_LAST_ERROR, e);
}

pub fn last_error() -> u32 {
    teb_get(TEB_LAST_ERROR)
}

/// Set the last error and return `v`.
pub fn fail<T>(e: u32, v: T) -> T {
    set_error(e);
    v
}

/// An OS/2 error as Windows reports it (most numbers are the same).
pub fn win_error(rc: u32) -> u32 {
    match rc {
        // ERROR_INVALID_EXE_SIGNATURE, ERROR_BAD_EXE_FORMAT
        191 | 193 => 193,
        // ERROR_MOD_NOT_FOUND
        126 => 126,
        // ERROR_FILENAME_EXCED_RANGE
        206 => 206,
        // ERROR_NO_MORE_FILES
        18 => 18,
        r => r,
    }
}

pub const TRUE: i32 = 1;
pub const FALSE: i32 = 0;

pub mod err {
    pub const SUCCESS: u32 = 0;
    pub const FILE_NOT_FOUND: u32 = 2;
    pub const PATH_NOT_FOUND: u32 = 3;
    pub const ACCESS_DENIED: u32 = 5;
    pub const INVALID_HANDLE: u32 = 6;
    pub const NOT_ENOUGH_MEMORY: u32 = 8;
    pub const INVALID_DATA: u32 = 13;
    pub const OUTOFMEMORY: u32 = 14;
    pub const NO_MORE_FILES: u32 = 18;
    pub const NOT_SUPPORTED: u32 = 50;
    pub const FILE_EXISTS: u32 = 80;
    pub const INVALID_PARAMETER: u32 = 87;
    pub const CALL_NOT_IMPLEMENTED: u32 = 120;
    pub const INSUFFICIENT_BUFFER: u32 = 122;
    pub const INVALID_NAME: u32 = 123;
    pub const MOD_NOT_FOUND: u32 = 126;
    pub const PROC_NOT_FOUND: u32 = 127;
    pub const NEGATIVE_SEEK: u32 = 131;
    pub const ALREADY_EXISTS: u32 = 183;
    pub const ENVVAR_NOT_FOUND: u32 = 203;
    pub const MORE_DATA: u32 = 234;
    pub const NO_MORE_ITEMS: u32 = 259;
    pub const NOT_OWNER: u32 = 288;
    pub const TOO_MANY_POSTS: u32 = 298;
    pub const INVALID_ADDRESS: u32 = 487;
}

// ---- Strings ----

/// The bytes of a NUL-terminated string (without the NUL).
pub unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// The code units of a NUL-terminated UTF-16 string.
pub unsafe fn wstr<'a>(p: *const u16) -> &'a [u16] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// A NUL-terminated copy.
pub fn z(b: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(b.len() + 1);
    v.extend_from_slice(b);
    v.push(0);
    v
}

/// Copy `s` and a NUL into a buffer of `size` bytes, Windows' way for
/// calls returning a length: the length copied (without the NUL), or the
/// size needed (with the NUL) if it does not fit.
pub unsafe fn put_str(s: &[u8], buf: *mut u8, size: u32) -> u32 {
    if buf.is_null() || (size as usize) < s.len() + 1 {
        return s.len() as u32 + 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
    }
    s.len() as u32
}

/// As [`put_str`] for UTF-16 (sizes in characters).
pub unsafe fn put_wstr(s: &[u16], buf: *mut u16, size: u32) -> u32 {
    if buf.is_null() || (size as usize) < s.len() + 1 {
        return s.len() as u32 + 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
    }
    s.len() as u32
}

// ---- A lock ----

pub struct Lock(AtomicBool);

impl Lock {
    pub const fn new() -> Lock {
        Lock(AtomicBool::new(false))
    }

    pub fn lock(&self) -> LockGuard<'_> {
        let mut spins = 0;
        while self.0.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            spins += 1;
            if spins > 64 {
                unsafe { os2::DosSleep(0) };
            }
            core::hint::spin_loop();
        }
        LockGuard(self)
    }
}

pub struct LockGuard<'a>(&'a Lock);

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        (self.0).0.store(false, Ordering::Release);
    }
}

/// Process-wide data guarded by a [`Lock`].
pub struct Global<T> {
    lock: Lock,
    v: core::cell::UnsafeCell<T>,
}

unsafe impl<T> Sync for Global<T> {}

impl<T> Global<T> {
    pub const fn new(v: T) -> Self {
        Global { lock: Lock::new(), v: core::cell::UnsafeCell::new(v) }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let _g = self.lock.lock();
        f(unsafe { &mut *self.v.get() })
    }
}

/// Write a line to the kernel's log (OutputDebugString, traces).
pub fn trace(s: &[u8]) {
    let b = z(s);
    unsafe { Rs2DebugTrace(b.as_ptr()) };
}

/// Call Windows code (cdecl or stdcall) with four arguments.
pub fn call(f: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
    unsafe { rs2_call4(f, a, b, c, d) }
}

core::arch::global_asm!(
    ".globl rs2_call4",
    "rs2_call4:",
    "push ebp",
    "mov ebp, esp",
    "push ebx",
    "push esi",
    "push edi",
    "push dword ptr [ebp+24]",
    "push dword ptr [ebp+20]",
    "push dword ptr [ebp+16]",
    "push dword ptr [ebp+12]",
    "call dword ptr [ebp+8]",
    "lea esp, [ebp-12]",
    "pop edi",
    "pop esi",
    "pop ebx",
    "pop ebp",
    "ret",
);

unsafe extern "C" {
    fn rs2_call4(f: u32, a: u32, b: u32, c: u32, d: u32) -> u32;
}
