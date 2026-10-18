//! rs2 command interpreter (CMD.EXE).

#![no_std]
#![no_main]

extern crate alloc;

mod batch;
mod builtin;
mod dir;
mod exec;
mod parse;
mod rexx;
mod shell;
mod util;

os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    shell::run(s)
}
