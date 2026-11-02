//! RXSUBCOM: registers, drops, queries and loads REXX subcommand
//! environments kept in DLLs, for REXX programs (`ADDRESS env`):
//!
//! ```text
//! RXSUBCOM REGISTER env dll procedure
//! RXSUBCOM DROP env [dll]
//! RXSUBCOM QUERY env [dll]
//! RXSUBCOM LOAD env [dll]
//! ```
//!
//! As IBM's, it prints nothing when the call is made: the return code is
//! the REXXAPI call's (RXSUBCOM_OK 0, RXSUBCOM_NOTREG 30, ...), which a
//! REXX program gets in `rc`. Missing parameters give one of REX.MSG's
//! messages 115-118 and 125 and return code -1.

#![no_std]
#![no_main]

extern crate alloc;

use rexxcmds::*;

os2::entry!(main);

/// Report a parameter error: REX.MSG message 115 + `which`.
fn usage(which: u32) -> ! {
    message(115 + which, b"REX.MSG\0", None);
    unsafe { os2::DosExit(1, 0xffff_ffff) };
    loop {}
}

fn main(s: &os2::Startup) -> u32 {
    let argv = args(s);
    if argv.is_empty() {
        usage(0);
    }
    let env = || argv[1].as_ptr();
    let dll = || argv.get(2).map(|a| a.as_ptr()).unwrap_or(b"\0".as_ptr());
    let cmd = &argv[0];
    if is(cmd, "REGISTER") {
        if argv.len() < 4 {
            usage(1);
        }
        unsafe { RexxRegisterSubcomDll(env(), argv[2].as_ptr(), argv[3].as_ptr(), core::ptr::null(), 0) }
    } else if is(cmd, "QUERY") {
        if argv.len() < 2 {
            usage(10);
        }
        let mut flag = 0u16;
        let mut user = [0u8; 8];
        unsafe { RexxQuerySubcom(env(), dll(), &mut flag, user.as_mut_ptr()) }
    } else if is(cmd, "DROP") {
        if argv.len() < 2 {
            usage(2);
        }
        unsafe { RexxDeregisterSubcom(env(), dll()) }
    } else if is(cmd, "LOAD") {
        if argv.len() < 2 {
            usage(3);
        }
        unsafe { RexxLoadSubcom(env(), dll()) }
    } else {
        usage(0)
    }
}
