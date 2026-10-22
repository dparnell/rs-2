//! RS2\WPSHELL.EXE: the Workplace Shell's desktop, run from PMWP.DLL's
//! classes (a test shell until the real shell process): PM, the
//! Workplace Shell's objects in this process (Rs2WpsStart), the desktop
//! opened (WinOpenObject), the message loop.
//!
//! `WPSHELL DEMO` also makes a folder on the desktop with a program, a
//! text file and a shadow in it, and opens the folder, its details view
//! and the program's settings; `WPSHELL OPEN` makes them and opens only
//! the folder. `WPSHELL CAR` puts the Toolkit's Car sample (registered
//! from C:\SAMPLES\CAR.DLL) on the desktop and opens its settings.

#![no_std]
#![no_main]

extern crate alloc;

use os2::*;

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fs: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut [u32; 8], h: u32, f: u32, l: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const [u32; 8]) -> u32;
    #[link_name = "PMWP.9100"]
    fn Rs2WpsStart() -> u32;
    #[link_name = "PMWP.252"]
    fn WinQueryObject(id: *const u8) -> u32;
    #[link_name = "PMWP.286"]
    fn WinOpenObject(h: u32, view: u32, flag: u32) -> u32;
    #[link_name = "PMWP.281"]
    fn WinCreateObject(class: *const u8, title: *const u8, setup: *const u8, loc: *const u8, flags: u32) -> u32;
    #[link_name = "PMWP.200"]
    fn WinRegisterObjectClass(class: *const u8, module: *const u8) -> u32;
    #[link_name = "PMWP.289"]
    fn WinCreateShadow(h: u32, dest: u32, flags: u32) -> u32;
}

const OPEN_CONTENTS: u32 = 1;
const OPEN_SETTINGS: u32 = 2;
const OPEN_DETAILS: u32 = 102;

fn demo(all: bool) {
    unsafe {
        let f = WinCreateObject(b"WPFolder\0".as_ptr(), b"Demo Folder\0".as_ptr(), b"OBJECTID=<RS2_DEMO>\0".as_ptr(), b"<WP_DESKTOP>\0".as_ptr(), 2);
        let p = WinCreateObject(
            b"WPProgram\0".as_ptr(),
            b"System Editor\0".as_ptr(),
            b"EXENAME=C:\\OS2\\E.EXE;OBJECTID=<RS2_DEMO_E>\0".as_ptr(),
            b"<RS2_DEMO>\0".as_ptr(),
            2,
        );
        WinCreateObject(b"WPDataFile\0".as_ptr(), b"Read me.txt\0".as_ptr(), b"\0".as_ptr(), b"<RS2_DEMO>\0".as_ptr(), 2);
        let desk = WinQueryObject(b"<WP_DESKTOP>\0".as_ptr());
        if WinQueryObject(b"<RS2_DEMO_SH>\0".as_ptr()) == 0 && p != 0 {
            WinCreateShadow(p, f, 0);
        }
        let _ = desk;
        WinOpenObject(f, OPEN_CONTENTS, 1);
        if !all {
            return;
        }
        WinOpenObject(f, OPEN_DETAILS, 1);
        if p != 0 {
            WinOpenObject(p, OPEN_SETTINGS, 1);
        }
    }
}

fn main(s: &Startup) -> u32 {
    let args = s.args();
    let hab = unsafe { WinInitialize(0) };
    let hmq = if hab != 0 { unsafe { WinCreateMsgQueue(hab, 0) } } else { 0 };
    if hmq == 0 {
        println!("WPSHELL: Presentation Manager is not available");
        return 1;
    }
    if unsafe { Rs2WpsStart() } == 0 {
        println!("WPSHELL: the Workplace Shell's classes did not start");
        return 1;
    }
    let desk = unsafe { WinQueryObject(b"<WP_DESKTOP>\0".as_ptr()) };
    unsafe { WinOpenObject(desk, 0, 1) };
    let a = args.trim_ascii();
    if a.eq_ignore_ascii_case(b"CAR") {
        // The Toolkit's Car sample (C:\SAMPLES\CAR.DLL), on the desktop
        // with its settings open.
        unsafe {
            WinRegisterObjectClass(b"Car\0".as_ptr(), b"C:\\SAMPLES\\CAR.DLL\0".as_ptr());
            let c = WinCreateObject(b"Car\0".as_ptr(), b"My Car\0".as_ptr(), b"OBJECTID=<RS2_CAR>\0".as_ptr(), b"<WP_DESKTOP>\0".as_ptr(), 2);
            WinOpenObject(c, OPEN_SETTINGS, 1);
        }
    }
    if a.eq_ignore_ascii_case(b"DEMO") || a.eq_ignore_ascii_case(b"OPEN") {
        demo(a.eq_ignore_ascii_case(b"DEMO"));
    }
    let mut q = [0u32; 8];
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    0
}
