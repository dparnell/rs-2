//! Test DLL: exports a few functions and counts its initialisations.

#![no_std]
#![no_main]

// Runtime (panic handler, memcpy, ...).
extern crate os2;

use core::sync::atomic::{AtomicU32, Ordering};

static INITS: AtomicU32 = AtomicU32::new(0);

/// _DLL_InitTerm(hmod, flag): flag 0 = init, 1 = term. Returns 1 for
/// success.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, flag: u32) -> u32 {
    if flag == 0 {
        INITS.fetch_add(1, Ordering::Relaxed);
    } else {
        // Termination: visible in the output so tests can check it ran.
        let msg = b"TESTDLL terminated\r\n";
        let mut n = 0;
        unsafe { os2::DosWrite(1, msg.as_ptr(), msg.len() as u32, &mut n) };
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn TestAdd(a: u32, b: u32) -> u32 {
    a + b
}

#[unsafe(no_mangle)]
pub extern "C" fn TestInitCount() -> u32 {
    INITS.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn TestGreeting() -> *const u8 {
    b"hello from TESTDLL\0".as_ptr()
}
