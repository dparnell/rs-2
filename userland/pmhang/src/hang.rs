//! RS2\PMHANG.EXE: a PM window whose procedure blocks forever (DosSleep)
//! when clicked. See common.rs.

#![no_std]
#![no_main]

mod common;

os2::entry!(main);

fn main(_s: &os2::Startup) -> u32 {
    common::run(common::Mode::Sleep, b"Hang test (blocks)\0")
}
