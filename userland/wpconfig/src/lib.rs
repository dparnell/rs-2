//! WPCONFIG.DLL: IBM's library of the System Setup classes (WPSystem,
//! WPClock, WPMouse, WPKeyboard, WPSound and the palettes). rs2 has these
//! classes in PMWP.DLL; this module forwards IBM's exports (their class
//! data and NewClass functions, with IBM's ordinals) to PMWP's private
//! ordinals 9101-9154, so that class libraries built against WPCONFIG
//! load (Object REXX's REXXWPS.DLL names it among its modules) and can
//! subclass them.

#![no_std]
#![no_main]

extern crate os2;

unsafe extern "C" {
    #[link_name = "PMWP.9149"]
    fn WPSystemNewClass(major: i32, minor: i32) -> usize;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// SOMInitModule(major, minor, class): as IBM's, the module's classes are
/// made when the class manager loads it (PMWP makes the others, as their
/// parents, on first use).
#[unsafe(no_mangle)]
pub extern "C" fn SOMInitModule(_major: i32, _minor: i32, _class: *const u8) {
    unsafe { WPSystemNewClass(0, 0) };
}
