//! OS/2 API bindings and a minimal runtime for programs written in Rust.
//!
//! Imports are declared with `#[link_name = "MODULE.ordinal"]`; `elf2lx`
//! turns the resulting undefined symbols into LX import fixups. All 32-bit
//! OS/2 APIs use the `_System` convention, which is cdecl (`extern "C"`).

#![no_std]

extern crate alloc;

pub mod dos;
pub mod fmt;
pub mod heap;
pub mod msg;
pub mod path;
pub mod vio;

/// Programs using `alloc` get the OS/2 heap.
#[cfg(feature = "heap")]
#[global_allocator]
static HEAP: heap::Os2Heap = heap::Os2Heap;

pub use dos::*;
pub use msg::*;

/// Write bytes to standard output.
pub fn write(b: &[u8]) {
    let mut n = 0;
    unsafe { DosWrite(1, b.as_ptr(), b.len() as u32, &mut n) };
}

pub fn print(s: &str) {
    write(s.as_bytes());
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

/// Arguments the loader passes to a 32-bit program's entry point.
pub struct Startup {
    pub hmod: u32,
    pub env: *const u8,
    pub cmdline: *const u8,
}

impl Startup {
    /// The command line arguments (after the program name).
    pub fn args(&self) -> &'static [u8] {
        unsafe {
            let mut p = self.cmdline;
            while *p != 0 {
                p = p.add(1);
            }
            p = p.add(1);
            let start = p;
            while *p != 0 {
                p = p.add(1);
            }
            core::slice::from_raw_parts(start, p.offset_from(start) as usize)
        }
    }
}

/// Define the program entry point: `os2::entry!(main)` where
/// `fn main(s: &os2::Startup) -> u32`.
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

#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn memset(d: *mut u8, c: i32, n: usize) -> *mut u8 {
    for i in 0..n {
        unsafe { d.add(i).write_volatile(c as u8) };
    }
    d
}

#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        unsafe { d.add(i).write_volatile(s.add(i).read_volatile()) };
    }
    d
}

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

#[unsafe(no_mangle)]
#[allow(suspicious_runtime_symbol_definitions)]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const u8) -> usize {
    let mut n = 0;
    while unsafe { s.add(n).read_volatile() } != 0 {
        n += 1;
    }
    n
}

/// Referenced by the precompiled `core`; never called with panic=abort.
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}

#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Resume() -> ! {
    loop {}
}
