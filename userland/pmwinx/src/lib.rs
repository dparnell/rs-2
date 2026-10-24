//! PMWINX.DLL: Open32, the Win32 calls on OS/2, as far as programs on the
//! OS/2 CD need them: the registry calls of the Registry Editor
//! (REGEDIT2.EXE). IBM's PMWINX forwards them to REGISTRY.DLL (ordinals
//! 541-555, exports.def); so does this one.

#![no_std]
#![no_main]

extern crate os2;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}
