//! W32USER.DLL: USER32, GDI32 and COMDLG32 of rs2's Win32 layer
//! (docs/WIN32.md). The kernel's PE loader sends programs' imports of
//! USER32, GDI32, MSIMG32 and COMDLG32 here: one DLL, because windows,
//! DCs and GDI objects share their state.
//!
//! The window manager and GDI are WIN16's (userland/win16) in 32-bit
//! form: every Windows window is a PM window (a frame for a top-level
//! one), messages are translated both ways and coordinates flipped
//! (Windows' y grows down), and DCs are PM presentation spaces in RGB
//! mode. Handles stay small numbers, the same kinds WIN16 gives out:
//! windows 0x4004 and up, GDI objects 0x0100 and up, DCs 0x8002 and up,
//! menus and icons in ranges of their own.
//!
//! - text.rs: the character calls and wsprintf;
//! - pm.rs: the PMWIN and PMGPI calls this is built on;
//! - wnd.rs: classes, windows, messages, painting, DefWindowProc;
//! - user32.rs: USER32's entry points;
//! - gdi.rs, bitmap.rs, region.rs, draw.rs: GDI (DCs, objects, text,
//!   drawing, bitmaps and DIBs, regions); gdi32.rs: GDI32's entry points;
//! - ctl.rs (BUTTON, STATIC), edit.rs (EDIT), listbox.rs (LISTBOX,
//!   COMBOBOX), scroll.rs (SCROLLBAR and window scroll bars): the
//!   built-in controls; dialog.rs: dialogs and the dialog manager;
//! - comdlg.rs: COMDLG32's common dialogs;
//! - caret.rs, icon.rs, menu.rs: the caret, icons and cursors, menus;
//! - res.rs: resources (through W32KERN's FindResource).
//!
//! Text is kept in the process's code page (ANSI); the W calls and
//! windows of classes registered with RegisterClassW convert.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

mod bitmap;
mod caret;
mod comdlg;
mod cptab;
mod ctl;
mod dialog;
mod draw;
mod edit;
mod gdi;
mod gdi32;
mod hook;
mod icon;
mod kern;
mod listbox;
mod mdi;
mod menu;
mod pm;
mod print;
mod private;
mod region;
mod res;
mod scroll;
mod text;
mod thunks;
mod user32;
mod wnd;

pub use pm::trace;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}


/// State of the DLL (its data is per process). Window procedures may
/// call back in: callers keep borrows short.
pub struct Global<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Global<T> {}
impl<T> Global<T> {
    pub const fn new(v: T) -> Self {
        Global(UnsafeCell::new(v))
    }
    #[allow(clippy::mut_from_ref)]
    pub fn get(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}

/// A spin lock for the few tables threads change at the same time.
pub struct Lock(AtomicBool);
impl Lock {
    pub const fn new() -> Lock {
        Lock(AtomicBool::new(false))
    }
    pub fn with<R>(&self, f: impl FnOnce() -> R) -> R {
        while self.0.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            unsafe { os2::DosSleep(0) };
        }
        let r = f();
        self.0.store(false, Ordering::Release);
        r
    }
}

// ---- Threads and their PM message queues ----

/// A thread's PM anchor block and message queue, made on its first
/// window or message call (as Windows makes a thread's queue).
#[derive(Clone, Copy)]
pub struct Queue {
    pub tid: u32,
    pub hab: u32,
    pub hmq: u32,
}

static QUEUES: Global<Vec<Queue>> = Global::new(Vec::new());
static QLOCK: Lock = Lock::new();

/// The thread's ID (TIB2's, which a Win32 TEB points at as an OS/2 TIB
/// does); for a Windows 3.x task, whose threads take turns, the first
/// thread's.
pub fn tid() -> u32 {
    if kern::one_queue() {
        return 1;
    }
    let v: u32;
    unsafe {
        core::arch::asm!("mov {0}, dword ptr fs:[0x0c]", "mov {0}, dword ptr [{0}]", out(reg) v, options(nostack, readonly));
    }
    v
}

/// The thread's queue, made if it has none (hmq 0: PM refused, as for a
/// program that is not a PM one).
pub fn queue() -> Queue {
    let t = tid();
    if let Some(q) = QLOCK.with(|| QUEUES.get().iter().find(|q| q.tid == t).copied()) {
        return q;
    }
    let hab = unsafe { pm::WinInitialize(0) };
    let hmq = unsafe { pm::WinCreateMsgQueue(hab, 0) };
    let q = Queue { tid: t, hab, hmq };
    QLOCK.with(|| QUEUES.get().push(q));
    if hmq != 0 {
        wnd::queue_created(hab);
    }
    q
}

/// The thread's anchor block.
pub fn hab() -> u32 {
    queue().hab
}

/// The queue of a thread, if it has one.
pub fn queue_of(t: u32) -> Option<Queue> {
    QLOCK.with(|| QUEUES.get().iter().find(|q| q.tid == t).copied())
}

// ---- Strings ----

/// The bytes of a NUL-terminated string.
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

/// A string argument as bytes; a resource ID or atom (below 0x10000) as
/// None.
pub fn far_str<'a>(p: u32) -> Option<&'a [u8]> {
    if p >> 16 == 0 { None } else { Some(unsafe { cstr(p as *const u8) }) }
}

/// A W string argument in the process code page; None for an ID.
pub fn wide_str(p: u32) -> Option<Vec<u8>> {
    if p >> 16 == 0 { None } else { Some(narrow(unsafe { wstr(p as *const u16) })) }
}

/// UTF-16 to the process code page.
pub fn narrow(w: &[u16]) -> Vec<u8> {
    w.iter().map(|&c| kern::wide_to_byte(c)).collect()
}

/// The process code page to UTF-16.
pub fn to_wide(b: &[u8]) -> Vec<u16> {
    b.iter().map(|&c| kern::byte_to_wide(c)).collect()
}

/// A NUL-terminated copy.
pub fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

pub fn zw(s: &[u16]) -> Vec<u16> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// Copy text to a buffer of `max` characters (NUL-terminated, truncated
/// to fit): the characters copied.
pub fn copy_out(t: &[u8], buf: *mut u8, max: i32) -> usize {
    if buf.is_null() || max <= 0 {
        return 0;
    }
    let n = t.len().min(max as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(t.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n
}

pub fn copy_out_w(t: &[u16], buf: *mut u16, max: i32) -> usize {
    if buf.is_null() || max <= 0 {
        return 0;
    }
    let n = t.len().min(max as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(t.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n
}

/// The pointer of a flat address (WIN16's 16:16 addresses are flat here).
pub fn flat(p: u32) -> *mut u8 {
    p as *mut u8
}

// ---- RECT and POINT ----

/// A RECT (left, top, right, bottom: 32-bit).
pub fn read_rect(p: u32) -> Option<[i32; 4]> {
    if p == 0 {
        return None;
    }
    let q = p as *const i32;
    Some(unsafe { [q.read_unaligned(), q.add(1).read_unaligned(), q.add(2).read_unaligned(), q.add(3).read_unaligned()] })
}

pub fn write_rect(p: u32, r: [i32; 4]) {
    if p == 0 {
        return;
    }
    let q = p as *mut i32;
    for (i, v) in r.iter().enumerate() {
        unsafe { q.add(i).write_unaligned(*v) };
    }
}

pub fn read_point(p: u32) -> Option<(i32, i32)> {
    if p == 0 {
        return None;
    }
    let q = p as *const i32;
    Some(unsafe { (q.read_unaligned(), q.add(1).read_unaligned()) })
}

pub fn write_point(p: u32, v: (i32, i32)) {
    if p == 0 {
        return;
    }
    let q = p as *mut i32;
    unsafe {
        q.write_unaligned(v.0);
        q.add(1).write_unaligned(v.1);
    }
}

pub fn rd32(p: u32, o: u32) -> u32 {
    unsafe { ((p + o) as *const u32).read_unaligned() }
}

pub fn wr32(p: u32, o: u32, v: u32) {
    unsafe { ((p + o) as *mut u32).write_unaligned(v) }
}

pub fn rd16(p: u32, o: u32) -> u16 {
    unsafe { ((p + o) as *const u16).read_unaligned() }
}

/// `SET W32TRACE=1`: trace windows and messages to the kernel log.
pub fn tracing() -> bool {
    static T: Global<u8> = Global::new(2);
    let t = T.get();
    if *t == 2 {
        let mut v: *const u8 = core::ptr::null();
        *t = (unsafe { os2::DosScanEnv(b"W32TRACE\0".as_ptr(), &mut v) } == 0 && !v.is_null() && unsafe { *v } != b'0') as u8;
    }
    *t == 1
}

/// Call Windows code: a window procedure or callback with four
/// arguments (stdcall; esp is restored whatever it pops).
pub fn call4(f: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
    unsafe { rs2_w32u_call4(f, a, b, c, d) }
}

core::arch::global_asm!(
    ".globl rs2_w32u_call4",
    "rs2_w32u_call4:",
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

/// Callbacks with two, three and five arguments (stdcall).
pub fn call4_2(f: u32, a: u32, b: u32) -> u32 {
    unsafe { rs2_w32u_calln(f, 2, [a, b, 0, 0, 0].as_ptr()) }
}

pub fn call4_3(f: u32, a: u32, b: u32, c: u32) -> u32 {
    unsafe { rs2_w32u_calln(f, 3, [a, b, c, 0, 0].as_ptr()) }
}

pub fn call4_5(f: u32, a: u32, b: u32, c: u32, d: u32, e: u32) -> u32 {
    unsafe { rs2_w32u_calln(f, 5, [a, b, c, d, e].as_ptr()) }
}

// rs2_w32u_calln(f, n, args): push args[n-1]..args[0], call f.
core::arch::global_asm!(
    ".globl rs2_w32u_calln",
    "rs2_w32u_calln:",
    "push ebp",
    "mov ebp, esp",
    "push ebx",
    "push esi",
    "push edi",
    "mov ecx, [ebp+12]",
    "mov esi, [ebp+16]",
    "2:",
    "test ecx, ecx",
    "jz 3f",
    "dec ecx",
    "push dword ptr [esi+ecx*4]",
    "jmp 2b",
    "3:",
    "call dword ptr [ebp+8]",
    "lea esp, [ebp-12]",
    "pop edi",
    "pop esi",
    "pop ebx",
    "pop ebp",
    "ret",
);

unsafe extern "C" {
    fn rs2_w32u_calln(f: u32, n: u32, args: *const u32) -> u32;
    fn rs2_w32u_call4(f: u32, a: u32, b: u32, c: u32, d: u32) -> u32;
}

/// Set the thread's last error, giving a value back.
pub fn fail<T>(e: u32, v: T) -> T {
    kern::set_last_error(e);
    v
}
