//! ANSI: show or switch the session's ANSI escape processing.
//! ANSI [ON|OFF] (VioGetAnsi/VioSetAnsi through their 16-bit entries).
#![no_std]
#![no_main]
extern crate alloc;
use os2::vio::{call16, ptr_words};
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "VIOCALLS.3"]
    fn VIO16GETANSI();
    #[link_name = "VIOCALLS.5"]
    fn VIO16SETANSI();
}

#[repr(C, align(16))]
struct Word(u16);

fn get() -> Result<bool, u16> {
    let mut w = Word(0);
    let p = ptr_words(&mut w.0 as *mut u16);
    let rc = unsafe { call16(VIO16GETANSI as *const () as usize, &[p[0], p[1], 0]) };
    let v = unsafe { core::ptr::read_volatile(&w.0) };
    if rc == 0 { Ok(v != 0) } else { Err(rc) }
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.len() > 1 {
        err(1003, &[], "The command syntax is not correct.\r\n");
        return 1003;
    }
    if let Some(w) = a.first() {
        let on = match w.to_ascii_uppercase().as_str() {
            "ON" => 1u16,
            "OFF" => 0,
            _ => {
                err(1003, &[], "The command syntax is not correct.\r\n");
                return 1003;
            }
        };
        let rc = unsafe { call16(VIO16SETANSI as *const () as usize, &[on, 0]) };
        if rc != 0 {
            error(rc as u32);
            return rc as u32;
        }
    }
    match get() {
        Ok(true) => say(1705, &[], "ANSI screen and keyboard control is on.\r\n"),
        Ok(false) => say(1706, &[], "ANSI screen and keyboard control is off.\r\n"),
        Err(rc) => {
            error(rc as u32);
            return rc as u32;
        }
    }
    0
}
