//! THAILIB.DLL: the Thai keyboard layer calls the language viewer
//! (BDVIEWER) uses. rs2 has no Thai keyboard layouts or code page 874, so
//! the keyboard has no national layer to switch to: the layer is still
//! recorded for the window system (PMMERGE's WinSetKbdLayer, which tells
//! the language viewer), as IBM's THAILIB does.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate os2;

mod exports;

unsafe extern "C" {
    #[link_name = "PMMERGE.3502"]
    fn WinSetKbdLayer(h: u32, layer: u32, fl: u32) -> u32;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// THAIINITIALIZELIBRARY(): nothing to set up.
#[unsafe(no_mangle)]
pub extern "C" fn THAIINITIALIZELIBRARY() -> u32 {
    1
}

/// BASEOS_SETKBDLAYER(idCodePage, fNational): IBM's sets the keyboard
/// device's layer (KBD$ category 4 function 0x58) for a Thai code page;
/// rs2's keyboards have none.
#[unsafe(no_mangle)]
pub extern "C" fn BASEOS_SETKBDLAYER(_cp: u32, _national: u32) -> u32 {
    0
}

/// THAISETKEYBOARDLAYOUT(hab, hwnd, idLayer): the layer of the focus
/// window.
#[unsafe(no_mangle)]
pub extern "C" fn THAISETKEYBOARDLAYOUT(_hab: u32, _h: u32, layer: u32) -> u32 {
    BASEOS_SETKBDLAYER(874, layer);
    unsafe { WinSetKbdLayer(1, layer, 0) }
}
