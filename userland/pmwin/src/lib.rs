//! PMWIN.DLL: rs2's Presentation Manager window manager. It also
//! implements the GPI (PMGPI.DLL forwards to it), since presentation
//! spaces belong to windows. See docs/PM.md.

#![no_std]
#![no_main]

extern crate alloc;
extern crate os2;

mod accel;
mod bitmap;
mod button;
mod color;
mod ctl;
mod dlg;
mod draw;
mod entry;
mod exports;
mod frame;
mod gpiapi;
mod listbox;
mod lock;
mod menu;
mod misc;
mod msg;
mod pointer;
mod res;
mod scrollbar;
mod state;
mod statics;
mod win;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}
