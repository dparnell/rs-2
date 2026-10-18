//! HELPMGR.DLL: the Information Presentation Facility help manager.
//!
//! Help windows are not implemented yet: a help instance can be created
//! and associated with windows so applications run normally, but help
//! requests show nothing.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate os2;

mod exports;

/// The handle of the (only) help instance.
const HELP_INSTANCE: u32 = 0xfffe_0001;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// WinCreateHelpInstance(hab, phinitHMInitStructure): HELPINIT starts
/// with cb and ulReturnCode.
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateHelpInstance(_hab: u32, init: *mut u32) -> u32 {
    if !init.is_null() {
        unsafe { init.add(1).write_unaligned(0) };
    }
    HELP_INSTANCE
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyHelpInstance(h: u32) -> u32 {
    (h == HELP_INSTANCE) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryHelpInstance(_hwnd: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn WinAssociateHelpInstance(_help: u32, _hwnd: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinLoadHelpTable(_help: u32, _id: u32, _hmod: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinCreateHelpTable(_help: u32, _table: u32) -> u32 {
    1
}
