//! Test DLL: exports a few functions and counts its initialisations.

#![no_std]
#![no_main]

// Runtime (panic handler, memcpy, ...).
extern crate os2;

use core::sync::atomic::{AtomicU32, Ordering};

static INITS: AtomicU32 = AtomicU32::new(0);

/// Initialisations since the module was loaded in the system (shared by
/// all processes; starts over when the DLL is loaded again after every
/// process let go of it).
#[unsafe(link_section = ".shared.inits")]
static SHARED_INITS: AtomicU32 = AtomicU32::new(0);

/// _DLL_InitTerm(hmod, flag): flag 0 = init, 1 = term. Returns 1 for
/// success.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, flag: u32) -> u32 {
    if flag == 0 {
        INITS.fetch_add(1, Ordering::Relaxed);
        SHARED_INITS.fetch_add(1, Ordering::Relaxed);
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
pub extern "C" fn TestSharedInits() -> u32 {
    SHARED_INITS.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn TestGreeting() -> *const u8 {
    b"hello from TESTDLL\0".as_ptr()
}

// A replacement VIO subsystem entry (VioRegister) in 16-bit code, reached
// through the code object's 16:16 alias. VioGetCurPos (index 0) reports
// row 7, column 9; everything else goes to the base VIO (AX = -1). The
// caller's stack may be the flat one, so it is addressed through EBP.
core::arch::global_asm!(
    ".section .text.vioreplace,\"ax\"",
    ".p2align 8",
    ".globl VIOREPLACE",
    "VIOREPLACE:",
    ".code16",
    "    push ebp",
    "    mov ebp, esp",
    // [ebp+4] router return, [ebp+8] index, [ebp+10] caller DS,
    // [ebp+12] caller return, [ebp+16] hvio, [ebp+18] pusColumn,
    // [ebp+22] pusRow.
    "    cmp word ptr [ebp + 8], 0",
    "    jne 2f",
    "    les bx, [ebp + 22]",
    "    mov word ptr es:[bx], 7",
    "    les bx, [ebp + 18]",
    "    mov word ptr es:[bx], 9",
    "    xor ax, ax",
    "    pop ebp",
    "    .byte 0xcb", // retf (16-bit)
    "2:",
    "    mov ax, 0xffff",
    "    pop ebp",
    "    .byte 0xcb", // retf (16-bit)
    ".code32",
);
