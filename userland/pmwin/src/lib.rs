//! PMWIN.DLL: rs2's Presentation Manager window manager. It also
//! implements the GPI (PMGPI.DLL forwards to it), since presentation
//! spaces belong to windows. See docs/PM.md.

#![no_std]
#![no_main]

extern crate alloc;
extern crate os2;

mod color;
mod exports;
mod gpiapi;
mod misc;
mod msg;
mod pointer;
mod state;
mod win;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}
