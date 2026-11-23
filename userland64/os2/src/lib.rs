//! rs2's 64-bit OS/2 API (docs/API64.md): bindings and a minimal runtime.
//!
//! The bindings (`api`, re-exported here: `use os2::*`) are generated from
//! userland64/api/*.api by tools/scripts/gen_api64.py, the description the
//! C headers (userland64/include) come from too: the same names, types,
//! structures and constants as in C (HWND, QMSG, WinCreateStdWindow...).
//!
//! The API is OS/2's with 64-bit pointers: ULONG, APIRET and the handles
//! (HFILE, HEV, HMODULE, HWND...) stay 32 bits, pointers (PVOID, PSZ,
//! PFN, MPARAM) are 64. Calls use the System V convention (`extern "C"`).
//! Imports are declared with `#[link_name = "MODULE.ordinal"]`; they stay
//! undefined dynamic symbols that rs2's ELF64 loader binds.

#![no_std]

extern crate alloc;

pub mod api;
pub mod fmt;
pub mod heap;

/// Programs using `alloc` get the OS/2 heap.
#[cfg(feature = "heap")]
#[global_allocator]
static HEAP: heap::Os2Heap = heap::Os2Heap;

pub use api::*;

/// Write bytes to standard output.
pub fn write(b: &[u8]) {
    let mut n = 0;
    unsafe { DosWrite(1, b.as_ptr().cast(), b.len() as u32, &mut n) };
}

/// The code page bytes of text held in a `str` one character per byte.
pub fn text_bytes(s: &str) -> alloc::borrow::Cow<'_, [u8]> {
    if s.is_ascii() {
        return alloc::borrow::Cow::Borrowed(s.as_bytes());
    }
    alloc::borrow::Cow::Owned(s.chars().map(|c| if (c as u32) < 0x100 { c as u8 } else { b'?' }).collect())
}

/// Write text (see [`text_bytes`]) to a handle.
pub fn write_text(hf: u32, s: &str) {
    let b = text_bytes(s);
    let mut n = 0;
    unsafe { DosWrite(hf, b.as_ptr().cast(), b.len() as u32, &mut n) };
}

pub fn print(s: &str) {
    write_text(1, s);
}

#[macro_export]
macro_rules! println {
    () => { $crate::print("\r\n") };
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let mut w = $crate::fmt::Stdout;
        let _ = write!(w, $($arg)*);
        $crate::print("\r\n");
    }};
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let mut w = $crate::fmt::Stdout;
        let _ = write!(w, $($arg)*);
    }};
}

/// What the loader passes to a 64-bit program's entry point.
pub struct Startup {
    pub hmod: u32,
    pub env: *const u8,
    pub cmdline: *const u8,
}

impl Startup {
    /// The program name as started (the command line's first string).
    pub fn program(&self) -> &'static [u8] {
        unsafe { cstr(self.cmdline) }
    }

    /// The command line arguments (after the program name).
    pub fn args(&self) -> &'static [u8] {
        unsafe {
            let p = self.program();
            cstr(p.as_ptr().add(p.len() + 1))
        }
    }
}

/// The bytes of a NUL-terminated string.
pub unsafe fn cstr(p: *const u8) -> &'static [u8] {
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// The kind of program, for the loader (a data symbol it reads):
/// `os2::app_type!(PM)` for a Presentation Manager program; text programs
/// need nothing (window compatible).
#[macro_export]
macro_rules! app_type {
    (PM) => {
        #[unsafe(no_mangle)]
        #[used]
        pub static RS2_APPTYPE: u32 = 3;
    };
}

/// A NUL-terminated string literal as a PCSZ: `psz!("Hello")`.
#[macro_export]
macro_rules! psz {
    ($s:literal) => {
        concat!($s, "\0").as_ptr() as $crate::PCSZ
    };
}

/// Embed the module's resources: the table a build script made from the
/// module's `.RC` script with `os2_rc::host::compile_table64` and wrote to
/// `$OUT_DIR/resources.bin`. rs2's loader finds it by its name
/// (`RS2_RESOURCES`), so DosGetResource, WinLoadMenu, WinLoadDlg... see
/// them.
#[macro_export]
macro_rules! resources {
    () => {
        #[unsafe(no_mangle)]
        #[used]
        pub static RS2_RESOURCES: [u8; include_bytes!(concat!(env!("OUT_DIR"), "/resources.bin")).len()] =
            *include_bytes!(concat!(env!("OUT_DIR"), "/resources.bin"));
    };
}

/// Define the program entry point: `os2::entry!(main)` where
/// `fn main(s: &os2::Startup) -> u32`. The loader calls
/// `_start(hmod, 0, env, cmdline)`; returning ends the process.
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".text.start")]
        pub extern "C" fn _start(hmod: u32, _reserved: u32, env: *const u8, cmdline: *const u8) -> u32 {
            let s = $crate::Startup { hmod, env, cmdline };
            $main(&s)
        }
    };
}

#[cfg(feature = "runtime")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("panic: {}", info);
    unsafe { DosExit(1, 255) };
    loop {}
}

// ---- Support routines the compiler expects ----
//
// Volatile accesses keep LLVM from turning these loops back into calls to
// themselves.

#[cfg(feature = "runtime")]
#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn memset(d: *mut u8, c: i32, n: usize) -> *mut u8 {
    for i in 0..n {
        unsafe { d.add(i).write_volatile(c as u8) };
    }
    d
}

#[cfg(feature = "runtime")]
#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        unsafe { d.add(i).write_volatile(s.add(i).read_volatile()) };
    }
    d
}

#[cfg(feature = "runtime")]
#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    if (d as usize) < (s as usize) {
        for i in 0..n {
            unsafe { d.add(i).write_volatile(s.add(i).read_volatile()) };
        }
    } else {
        for i in (0..n).rev() {
            unsafe { d.add(i).write_volatile(s.add(i).read_volatile()) };
        }
    }
    d
}

#[cfg(feature = "runtime")]
#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let (x, y) = unsafe { (a.add(i).read_volatile(), b.add(i).read_volatile()) };
        if x != y {
            return x as i32 - y as i32;
        }
    }
    0
}

#[cfg(feature = "runtime")]
#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

#[cfg(feature = "runtime")]
#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn strlen(s: *const u8) -> usize {
    let mut n = 0;
    while unsafe { s.add(n).read_volatile() } != 0 {
        n += 1;
    }
    n
}

/// Referenced by the precompiled `core`; never called with panic=abort.
#[cfg(feature = "runtime")]
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}

#[cfg(feature = "runtime")]
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Resume() -> ! {
    loop {}
}
