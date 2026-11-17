//! LINK386.EXE: the linear-executable (LX) linker.
#![no_std]
#![no_main]
os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    link::main(s, omflink::Prog::Link386)
}
