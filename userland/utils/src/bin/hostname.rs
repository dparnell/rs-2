//! HOSTNAME: the host name (%HOSTNAME%, else rs2's default "rs2").
#![no_std]
#![no_main]
extern crate alloc;
use utils::*;
os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    if !args(s).is_empty() {
        eprint("HOSTNAME: set the name with SET HOSTNAME=name in CONFIG.SYS\r\n");
        return 1;
    }
    outln(&os2::net::hostname());
    0
}
