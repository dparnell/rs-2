//! W32CRT.DLL: the C runtime of rs2's Win32 layer (docs/WIN32.md).
//! Windows programs' imports of MSVCRT.DLL (and MSVCR70-120, UCRTBASE and
//! the UCRT's `api-ms-win-crt-*` DLLs) come here; it exports both the old
//! runtime's names (`__getmainargs`, `_iob`) and the UCRT's
//! (`__stdio_common_vfprintf`, `__acrt_iob_func`). Functions are cdecl.
//!
//! - start.rs: program start and end, the arguments and the environment,
//!   atexit, signals, locales, errno;
//! - mem.rs: malloc and friends;
//! - string.rs: strings, wide strings, characters, conversions, sorting;
//! - fmt.rs: the printf and scanf engines, strtod;
//! - io.rs: file descriptors (_open, _read...) over Windows handles;
//! - stdio.rs: FILE streams;
//! - fs.rs: files and directories by name (_stat, _findfirst...);
//! - math.rs: <math.h>, on the x87 unit;
//! - time.rs: <time.h>;
//! - misc.rs: setjmp, threads, processes, the console (conio).

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(clippy::missing_safety_doc)]
#![allow(dead_code)]
#![allow(unused_unsafe)]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

mod fmt;
mod fs;
mod io;
mod k;
mod math;
mod mem;
mod misc;
mod start;
mod stdio;
mod string;
mod thunks;
mod time;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, flag: u32) -> u32 {
    if flag == 0 {
        start::init();
        io::init();
        stdio::init();
    }
    1
}

// ---- errno ----

pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const EIO: i32 = 5;
pub const E2BIG: i32 = 7;
pub const ENOEXEC: i32 = 8;
pub const EBADF: i32 = 9;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EEXIST: i32 = 17;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const EMFILE: i32 = 24;
pub const ENOSPC: i32 = 28;
pub const ESPIPE: i32 = 29;
pub const EPIPE: i32 = 32;
pub const EDOM: i32 = 33;
pub const ERANGE: i32 = 34;
pub const ENOTEMPTY: i32 = 41;
pub const EILSEQ: i32 = 42;

/// errno and _doserrno per thread, by thread ID (OS/2 has at most
/// 0x3FC threads a process).
static mut ERRNO_TAB: [[i32; 2]; 1024] = [[0; 2]; 1024];

fn errno_slot(which: usize) -> *mut i32 {
    let t = (unsafe { k::GetCurrentThreadId() } as usize).min(1023);
    unsafe { &raw mut ERRNO_TAB[t][which] }
}

pub fn set_errno(e: i32) {
    unsafe { *errno_slot(0) = e };
}

pub fn errno() -> i32 {
    unsafe { *errno_slot(0) }
}

/// errno for a Windows error (as the C runtime's _dosmaperr).
pub fn dos_errno(win: u32) -> i32 {
    unsafe { *errno_slot(1) = win as i32 };
    match win {
        2 | 3 | 15 | 18 | 53 | 67 | 123 | 161 | 206 => ENOENT,
        4 => EMFILE,
        5 | 19 | 32 | 33 | 36 | 65 | 82 | 108 | 110 => EACCES,
        6 => EBADF,
        8 | 14 => ENOMEM,
        17 => EXDEV,
        80 | 183 => EEXIST,
        112 => ENOSPC,
        109 | 232 => EPIPE,
        145 => ENOTEMPTY,
        _ => EINVAL,
    }
}

pub const EXDEV: i32 = 18;

pub fn set_dos_errno(win: u32) {
    set_errno(dos_errno(win));
}

#[unsafe(no_mangle)]
pub extern "C" fn _errno() -> *mut i32 {
    errno_slot(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn __doserrno() -> *mut i32 {
    errno_slot(1)
}

#[unsafe(no_mangle)]
pub extern "C" fn _set_errno(e: i32) -> i32 {
    set_errno(e);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _get_errno(p: *mut i32) -> i32 {
    unsafe { *p = errno() };
    0
}

// ---- Locks and globals ----

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
                unsafe { k::Sleep(0) };
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

// ---- C strings ----

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

pub fn z(b: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(b.len() + 1);
    v.extend_from_slice(b);
    v.push(0);
    v
}

/// UTF-16 to the process code page.
pub fn narrow(w: &[u16]) -> Vec<u8> {
    if w.is_empty() {
        return Vec::new();
    }
    let n = unsafe { k::WideCharToMultiByte(0, 0, w.as_ptr(), w.len() as i32, core::ptr::null_mut(), 0, core::ptr::null(), core::ptr::null_mut()) };
    let mut v = alloc::vec![0u8; n.max(0) as usize];
    unsafe { k::WideCharToMultiByte(0, 0, w.as_ptr(), w.len() as i32, v.as_mut_ptr(), n, core::ptr::null(), core::ptr::null_mut()) };
    v
}

/// The process code page to UTF-16.
pub fn widen(b: &[u8]) -> Vec<u16> {
    if b.is_empty() {
        return Vec::new();
    }
    let n = unsafe { k::MultiByteToWideChar(0, 0, b.as_ptr(), b.len() as i32, core::ptr::null_mut(), 0) };
    let mut v = alloc::vec![0u16; n.max(0) as usize];
    unsafe { k::MultiByteToWideChar(0, 0, b.as_ptr(), b.len() as i32, v.as_mut_ptr(), n) };
    v
}

/// A heap copy of `s` with a NUL, for returning to C (freed by free()).
pub fn c_alloc_str(s: &[u8]) -> *mut u8 {
    let p = mem::heap_alloc(s.len() + 1);
    if !p.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            *p.add(s.len()) = 0;
        }
    }
    p
}

/// Call C code: f(a, b, c, d) (cdecl or stdcall), keeping the registers
/// and the stack whatever it does.
pub fn call(f: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
    unsafe { rs2_crt_call4(f, a, b, c, d) }
}

core::arch::global_asm!(
    ".globl rs2_crt_call4",
    "rs2_crt_call4:",
    "push ebp",
    "mov ebp, esp",
    "push ebx",
    "push esi",
    "push edi",
    "and esp, -16",
    "sub esp, 4",
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
    fn rs2_crt_call4(f: u32, a: u32, b: u32, c: u32, d: u32) -> u32;
}
