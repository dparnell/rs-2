//! WINCFG.DLL: IBM's library of the WIN-OS/2 Setup class (WPWinConfig).
//! rs2 has the class in PMWP.DLL; this module forwards IBM's exports
//! (its class data and NewClass functions, with IBM's ordinals) to
//! PMWP's private ordinals 9318-9323, as WPCONFIG.DLL forwards its own
//! WPWinConfig exports here, so that class libraries built against
//! WINCFG load and can subclass it.

#![no_std]
#![no_main]

extern crate os2;

unsafe extern "C" {
    #[link_name = "PMWP.9318"]
    fn WPWinConfigNewClass(major: i32, minor: i32) -> usize;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// SOMInitModule(major, minor, class): the class is made when the class
/// manager loads the module.
#[unsafe(no_mangle)]
pub extern "C" fn SOMInitModule(_major: i32, _minor: i32, _class: *const u8) {
    unsafe { WPWinConfigNewClass(0, 0) };
}
