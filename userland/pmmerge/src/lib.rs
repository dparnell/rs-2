//! PMMERGE.DLL: on OS/2 Warp 4 the merged PM DLL behind PMWIN, PMGPI
//! and friends, which forward their entry points to it. rs2's PMWIN and
//! PMSHAPI implement those APIs themselves; this DLL has PMMERGE's own
//! ordinals for them, which some programs import directly (forward.rs),
//! and the PMMERGE entry points without a public name.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod bidi;
mod exports;

/// `name = "MODULE.ordinal";`: an entry that jumps to that import.
macro_rules! forward {
    ($($name:ident = $target:literal;)*) => {$(
        #[allow(non_snake_case)]
        mod $name {
            unsafe extern "C" {
                #[link_name = $target]
                pub fn target();
            }
        }
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!("jmp {}", sym $name::target);
        }
    )*};
}

mod forward;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// Ordinal 5119 (no public name): the System Editor's help hook passes
/// help requests it does not handle itself here (usMode, idTopic,
/// idSubTopic, prcPosition). Help is not shown yet: not processed.
#[unsafe(no_mangle)]
pub extern "C" fn PmMergeHelpHook(_mode: u32, _topic: u32, _subtopic: u32, _prcl: u32) -> u32 {
    0
}

/// WinQueryCpType(hmq) (ordinal 5480): the kind of the code page (of the
/// queue, or the process's for 0), as IBM's PMMERGE classifies them: 2
/// Arabic, 6 Hebrew (bidirectional; IBM's VIEWDOC then loads PMBIDI),
/// otherwise 1 for rs2's code pages (437 and 850).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryCpType(_hmq: u32) -> u32 {
    1
}

unsafe extern "C" {
    #[link_name = "PMWIN.977"]
    fn WinSetErrorInfo(e: u32, fs: u32) -> u32;
}

/// Per-thread blocks of AAB32GetReg register 3 (by thread id).
static mut REGS: [[u16; 8]; 128] = [[0; 8]; 128];

/// The thread ID (FS:[0x0C] is the TIB2, which starts with it).
fn tid() -> usize {
    let t: u32;
    unsafe {
        core::arch::asm!("mov {t}, fs:[0x0c]", "mov {t}, [{t}]", t = out(reg) t, options(nostack, readonly, preserves_flags));
    }
    t as usize
}

/// AAB32GetReg(lReg) (ordinal 5000, no public name): IBM's PM DLLs
/// (PMGPI, PMPIC) take register 3, a block of the thread in which they
/// note the error they raise and its severity, and then report it with
/// AAB32SetReg(1, severity << 16 | code). Other registers read 0.
#[unsafe(no_mangle)]
pub extern "C" fn AAB32GetReg(reg: u32) -> u32 {
    if reg != 3 {
        return 0;
    }
    unsafe { core::ptr::addr_of_mut!(REGS[tid() % 128]) as u32 }
}

/// AAB32SetReg(lReg, ulValue) (ordinal 5001): register 1 is the error
/// WinGetLastError returns.
#[unsafe(no_mangle)]
pub extern "C" fn AAB32SetReg(reg: u32, v: u32) -> u32 {
    if reg == 1 {
        unsafe { WinSetErrorInfo(v, 0) };
    }
    1
}
