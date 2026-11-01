//! PMGRE.DLL: the Presentation Manager graphics engine's entry points.
//! Programs that bypass the GPI call the engine directly, through the
//! Toolkit's `GreXxx` macros (pmddim.h): `Gre32EntryN` (ordinals 62-70)
//! with N arguments, the first usually a device context, the second to
//! last a placeholder the engine fills in, and the last the function
//! number (`NGreXxx`, with the COM_ flags in its high word).
//!
//! IBM's PMGRE only forwards to PMMERGE, whose dispatcher looks up the
//! DC and jumps through the device's or the engine's function table.
//! rs2 has no device drivers: each entry hands its arguments to PMWIN
//! (`Rs2GreEntry`, private ordinal 9033), which owns device contexts,
//! presentation spaces and bitmaps and runs the functions it knows in
//! its GPI (src/gre.rs there; docs/PM.md "The graphics engine").
//! `InnerGre32EntryN` (72-80), the engine's own reentry, goes the same
//! way. GetDriverInfo32 (37) is what presentation drivers keep per DC
//! or bitmap: there are none, so it answers -1 as IBM's does for an
//! object without driver data.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate os2;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

unsafe extern "C" {
    /// Rs2GreEntry(pulArgs, cArgs): the arguments of a Gre32EntryN call
    /// as they are on the caller's stack.
    #[link_name = "PMWIN.9033"]
    fn Rs2GreEntry(args: *const u32, n: u32) -> u32;
}

/// `_System` entries with N arguments: the arguments stay on the caller's
/// stack (the caller removes them); PMWIN gets their address and count.
macro_rules! entries {
    ($($name:ident = $n:literal;)*) => {$(
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!(
                "lea eax, [esp + 4]", "push {n}", "push eax",
                "call {f}", "add esp, 8", "ret",
                n = const $n, f = sym Rs2GreEntry,
            );
        }
    )*};
}

entries! {
    Gre32Entry2 = 2;
    Gre32Entry3 = 3;
    Gre32Entry4 = 4;
    Gre32Entry5 = 5;
    Gre32Entry6 = 6;
    Gre32Entry7 = 7;
    Gre32Entry8 = 8;
    Gre32Entry9 = 9;
    Gre32Entry10 = 10;
    InnerGre32Entry2 = 2;
    InnerGre32Entry3 = 3;
    InnerGre32Entry4 = 4;
    InnerGre32Entry5 = 5;
    InnerGre32Entry6 = 6;
    InnerGre32Entry7 = 7;
    InnerGre32Entry8 = 8;
    InnerGre32Entry9 = 9;
    InnerGre32Entry10 = 10;
}

/// GetDriverInfo32(hObject, ulType, hdc): the driver data of a DC
/// (type 0) or bitmap (1); -1 when there is none.
#[unsafe(no_mangle)]
pub extern "C" fn GetDriverInfo32(_h: u32, _ty: u32, _hdc: u32) -> u32 {
    u32::MAX
}
