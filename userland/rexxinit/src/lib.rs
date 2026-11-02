//! REXXINIT.DLL: keeps the REXX API's system-wide state alive. IBM's
//! starts the REXX API server from the PM shell's SYS_DLLS list; rs2's
//! kernel loads it at boot through SYSDLLS.EXE, which stays resident. Its
//! initialisation sets up REXXAPI's shared memory (queues, DLL
//! registrations, the macro space), so external data queues outlive the
//! programs that use them, as on OS/2.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

unsafe extern "C" {
    #[link_name = "REXXAPI.5"]
    fn RexxQueryQueue(name: *const u8, count: *mut u32) -> u32;
}

fn init() -> u32 {
    let mut n = 0u32;
    // the first API call creates the shared state
    unsafe { RexxQueryQueue(b"SESSION\0".as_ptr(), &mut n) };
    0
}

/// _DLL_InitTerm(hmod, flag): set up the API state when loaded.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, flag: u32) -> u32 {
    if flag == 0 {
        init();
    }
    1
}

/// RxApiInit: IBM's export; sets up the API state.
#[unsafe(no_mangle)]
pub extern "C" fn __a_RxApiInit() -> u32 {
    init()
}

/// dllinit: IBM's other export.
#[unsafe(no_mangle)]
pub extern "C" fn __a_dllinit() -> u32 {
    init()
}
