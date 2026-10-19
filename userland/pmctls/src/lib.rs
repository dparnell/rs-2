//! PMCTLS.DLL: the standard file and font dialogs (WinFileDlg,
//! WinFontDlg). They are ordinary PM dialogs built from templates made
//! in memory, run through PMWIN like an application's.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod exports;
mod filedlg;
mod fontdlg;
mod pm;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}
