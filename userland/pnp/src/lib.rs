//! PNP.DLL: IBM's library of the Hardware Manager classes (WPHwManager,
//! WPDevice and its device classes). rs2 has these classes in PMWP.DLL;
//! this module forwards IBM's exports (their class data and NewClass
//! functions, with IBM's ordinals) to PMWP's private ordinals (12000 +
//! IBM's), so that class libraries built against PNP load and can
//! subclass them.

#![no_std]
#![no_main]

extern crate os2;

unsafe extern "C" {
    #[link_name = "PMWP.12005"]
    fn WPHwManagerNewClass(major: i32, minor: i32) -> usize;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// SOMInitModule(major, minor, class): as IBM's, the module's classes are
/// made when the class manager loads it (PMWP makes the device classes
/// when they are first used).
#[unsafe(no_mangle)]
pub extern "C" fn SOMInitModule(_major: i32, _minor: i32, _class: *const u8) {
    unsafe { WPHwManagerNewClass(0, 0) };
}
