//! NAMPIPES.DLL: OS/2 1.x's named pipe module (DosMakeNmPipe and the rest
//! of the 16-bit named pipe calls). Warp's is a forwarder module: its 14
//! entries go to the kernel's 16-bit DOSCALLS entries 399-412, which rs2
//! implements in kernel/src/api/npipe.rs. This one is the same
//! (exports.def), with nothing of its own but the initialisation.

#![no_std]
#![no_main]

extern crate os2;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}
