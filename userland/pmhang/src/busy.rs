//! RS2\PMBUSY.EXE: a PM window whose procedure spins forever without
//! system calls when clicked. See common.rs.

#![no_std]
#![no_main]

mod common;

os2::entry!(main);

fn main(_s: &os2::Startup) -> u32 {
    common::run(common::Mode::Busy, b"Hang test (spins)\0")
}
