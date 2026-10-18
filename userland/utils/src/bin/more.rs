//! MORE: show input a screen at a time.
#![no_std]
#![no_main]
use utils::*;
os2::entry!(main);
fn main(_s: &os2::Startup) -> u32 {
    let data = read_all(0);
    // The keyboard is read through the console device, not stdin.
    let c = os2::path::cstr("KBD$");
    let (mut kbd, mut act) = (0, 0);
    unsafe { os2::DosOpen(c.as_ptr(), &mut kbd, &mut act, 0, 0, 1, 0x40, core::ptr::null_mut()) };
    for (i, l) in lines(&data).iter().enumerate() {
        outln(l);
        if (i + 1) % 23 == 0 {
            out("-- More --");
            let mut b = [0u8; 16];
            let mut n = 0;
            unsafe { os2::DosRead(kbd, b.as_mut_ptr(), 16, &mut n) };
            out("\r          \r");
        }
    }
    0
}
