//! SYSDLLS: keep system DLLs loaded. SYSDLLS module...
//!
//! Stands in for the Presentation Manager shell's `SYS_DLLS LoadOneTime`
//! list: loads each module (running its initialisation, which may start
//! server threads, as REXXINIT does for the REXX API) and then stays
//! resident so the module and whatever it created live on.
#![no_std]
#![no_main]
extern crate alloc;
use os2::path::cstr;
use os2::*;
use utils::*;
os2::entry!(main);
fn main(s: &os2::Startup) -> u32 {
    let mut loaded = 0;
    for name in args(s) {
        let c = cstr(&name);
        let mut fail = [0u8; 64];
        let mut h = 0u32;
        let rc = unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, c.as_ptr(), &mut h) };
        if rc == 0 {
            loaded += 1;
        } else {
            eprint(&alloc::format!("SYSDLLS: cannot load {name}: error {rc}\r\n"));
        }
    }
    if loaded == 0 {
        return 1;
    }
    loop {
        unsafe { DosSleep(0x7fff_ffff) };
    }
}
