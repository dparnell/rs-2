//! Files: many open handles (DosSetMaxFH/DosSetRelMaxFH), create/delete
//! storms, handles inherited by children, handles released at exit.

use core::ptr::null_mut;

use os2::*;

use crate::api::*;
use crate::{check, counters, run_child};

const DIR: &[u8] = b"C:\\STRESSF\0";

fn name(i: u32) -> alloc::vec::Vec<u8> {
    let mut v = alloc::format!("C:\\STRESSF\\F{i:05}.DAT").into_bytes();
    v.push(0);
    v
}

fn open(path: &[u8], create: bool) -> Result<u32, u32> {
    let (mut h, mut act) = (0, 0);
    let flags = if create { OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS } else { OPEN_ACTION_OPEN_IF_EXISTS };
    let rc = unsafe { DosOpen(path.as_ptr(), &mut h, &mut act, 0, FILE_NORMAL, flags, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, null_mut()) };
    if rc == 0 { Ok(h) } else { Err(rc) }
}

/// Child: open 100 files and exit without closing them.
pub fn child_files() -> u32 {
    let mut req: i32 = 120;
    let mut max = 0;
    unsafe { DosSetRelMaxFH(&mut req, &mut max) };
    for i in 0..100 {
        if open(&name(i), false).is_err() {
            return 1;
        }
    }
    0
}

pub fn run() {
    let before = counters();
    unsafe {
        DosCreateDir(DIR.as_ptr(), null_mut());
        // Raise the handle limit.
        let mut req: i32 = 0;
        let mut max0 = 0;
        DosSetRelMaxFH(&mut req, &mut max0);
        req = 300;
        let mut max = 0;
        let rc = DosSetRelMaxFH(&mut req, &mut max);
        check("DosSetRelMaxFH +300", rc == 0 && max == max0 + 300);
        check("DosSetMaxFH below the current value fails", DosSetMaxFH(10) == 87);

        // Create 250 files and keep them open.
        let mut hs = alloc::vec::Vec::new();
        let mut ok = true;
        for i in 0..250 {
            match open(&name(i), true) {
                Ok(h) => {
                    let mut n = 0;
                    let data = alloc::format!("file {i}");
                    ok &= DosWrite(h, data.as_ptr(), data.len() as u32, &mut n) == 0 && n == data.len() as u32;
                    hs.push(h);
                }
                Err(rc) => {
                    println!("  open {i}: rc {rc}");
                    ok = false;
                    break;
                }
            }
        }
        check("250 files open at once", ok && hs.len() == 250);
        // Read them back through the same handles.
        let mut ok = true;
        for (i, &h) in hs.iter().enumerate() {
            let mut pos = 0;
            DosSetFilePtr(h, 0, FILE_BEGIN, &mut pos);
            let mut buf = [0u8; 16];
            let mut n = 0;
            DosRead(h, buf.as_mut_ptr(), 16, &mut n);
            ok &= &buf[..n as usize] == alloc::format!("file {i}").as_bytes();
        }
        check("250 files read back", ok);
        let mut ok = true;
        for h in hs.drain(..) {
            ok &= DosClose(h) == 0;
        }
        check("250 files closed", ok);

        // The same file opened 200 times.
        let mut ok = true;
        for _ in 0..200 {
            match open(&name(0), false) {
                Ok(h) => hs.push(h),
                Err(_) => ok = false,
            }
        }
        for h in hs.drain(..) {
            ok &= DosClose(h) == 0;
        }
        check("one file opened 200 times", ok);

        // A child leaves its files open: closed at its exit.
        let (cause, code) = run_child("files");
        check("child with 100 open files", cause == 0 && code == 0);
        let mid = counters();
        check("child's handles closed at exit", mid.open_files == before.open_files);

        // Delete/create storm.
        let mut ok = true;
        for round in 0..3 {
            for i in 0..250 {
                ok &= DosDelete(name(i).as_ptr()) == 0;
            }
            if round < 2 {
                for i in 0..250 {
                    match open(&name(i), true) {
                        Ok(h) => ok &= DosClose(h) == 0,
                        Err(_) => ok = false,
                    }
                }
            }
        }
        check("create/delete storm", ok);
        check("directory removed", DosDeleteDir(DIR.as_ptr()) == 0);
        check("closing a bad handle fails", DosClose(0x7777) == 6);
    }
    let after = counters();
    check("no open files leaked", after.open_files == before.open_files);
}
