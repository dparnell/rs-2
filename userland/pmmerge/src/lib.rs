//! PMMERGE.DLL: on OS/2 Warp 4 the merged PM DLL behind PMWIN, PMGPI
//! and friends, which forward their entry points to it. rs2's PMWIN and
//! PMSHAPI implement those APIs themselves; this DLL has PMMERGE's own
//! ordinals for them, which some programs import directly (forward.rs),
//! and the PMMERGE entry points without a public name.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate os2;

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
