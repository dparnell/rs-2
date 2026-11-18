//! NEWMAIL: plays a mail program to the WarpCenter's mail indicator
//! (rs2's own test tool; IBM's indicator was fed by UltiMail).
//!
//!   NEWMAIL [count] [seconds]
//!
//! Makes the In-basket (`<CCINBOX>`, a folder on the desktop) if there is
//! none, and keeps `count` (1 by default) new messages in IBM's named
//! shared memory \SHAREMEM\INBOX.MEM for `seconds` (60 by default; 0
//! until stopped with Ctrl+Break), then 0. NEWMAIL /REMOVE takes the
//! In-basket away again.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use utils::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWP.281"]
    fn WinCreateObject(class: *const u8, title: *const u8, setup: *const u8, location: *const u8, fl: u32) -> u32;
    #[link_name = "PMWP.252"]
    fn WinQueryObject(id: *const u8) -> u32;
    #[link_name = "PMWP.251"]
    fn WinDestroyObject(h: u32) -> u32;
}

const ID: &[u8] = b"<CCINBOX>\0";

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    if a.iter().any(|x| x == "/?" || x == "-?") {
        outln("Syntax: NEWMAIL [count] [seconds]  |  NEWMAIL /REMOVE");
        return 0;
    }
    if a.first().is_some_and(|x| x.eq_ignore_ascii_case("/REMOVE")) {
        let h = unsafe { WinQueryObject(ID.as_ptr()) };
        if h != 0 {
            unsafe { WinDestroyObject(h) };
        }
        outln("In-basket removed.");
        return 0;
    }
    let count: u32 = a.first().and_then(|x| x.parse().ok()).unwrap_or(1);
    let secs: u32 = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(60);
    if unsafe { WinQueryObject(ID.as_ptr()) } == 0 {
        // CO_FAILIFEXISTS
        let h = unsafe { WinCreateObject(b"WPFolder\0".as_ptr(), b"In-basket\0".as_ptr(), b"OBJECTID=<CCINBOX>\0".as_ptr(), b"<WP_DESKTOP>\0".as_ptr(), 0) };
        if h == 0 {
            eprint("NEWMAIL: could not make the In-basket\r\n");
            return 1;
        }
        outln("In-basket made on the desktop.");
    }
    let mut p: *mut u8 = core::ptr::null_mut();
    // PAG_READ | PAG_WRITE | PAG_COMMIT
    let name = b"\\SHAREMEM\\INBOX.MEM\0";
    let mut rc = unsafe { os2::DosAllocSharedMem(&mut p, name.as_ptr(), 4096, 0x13) };
    if rc != 0 {
        rc = unsafe { os2::DosGetNamedSharedMem(&mut p, name.as_ptr(), 3) };
    }
    if rc != 0 || p.is_null() {
        eprint(&format!("NEWMAIL: no INBOX.MEM (error {rc})\r\n"));
        return 1;
    }
    let cell = p as *mut u32;
    unsafe { cell.write_volatile(count) };
    if secs == 0 {
        outln(&format!("{count} new message(s); Ctrl+Break ends."));
        loop {
            unsafe { os2::DosSleep(60_000) };
        }
    }
    outln(&format!("{count} new message(s) for {secs} s."));
    unsafe { os2::DosSleep(secs * 1000) };
    unsafe { cell.write_volatile(0) };
    // The indicator looks every 2 s: keep the 0 there long enough.
    unsafe { os2::DosSleep(3000) };
    outln("No new mail.");
    unsafe { os2::DosFreeMem(p) };
    0
}
