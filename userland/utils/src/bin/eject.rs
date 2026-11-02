//! EJECT: eject the medium of a removable drive. EJECT drive:
//!
//! Optical drives get the CD-ROM category's eject (0x80/0x44), other
//! drives the logical disk unlock-and-eject (category 8, function 0x40,
//! command 2).
#![no_std]
#![no_main]
extern crate alloc;
use os2::*;
use utils::msgs::err;
use utils::*;
os2::entry!(main);

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let Some(d) = (a.len() == 1).then(|| drive_arg(Some(&a[0]))).flatten() else {
        err(1003, &[], "The command syntax is not correct.\r\n");
        return 1003;
    };
    let name = [b'A' + d, b':', 0];
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(
            name.as_ptr(),
            &mut hf,
            &mut act,
            0,
            0,
            OPEN_ACTION_OPEN_IF_EXISTS,
            OPEN_FLAGS_DASD | OPEN_FLAGS_FAIL_ON_ERROR | OPEN_SHARE_DENYNONE | OPEN_ACCESS_READONLY,
            core::ptr::null_mut(),
        )
    };
    if rc != 0 {
        error(rc);
        return rc;
    }
    let is_cd = disk::fs_name(d).is_some_and(|f| f == "CDFS" || f == "UDF");
    let mut sig = *b"CD01";
    let mut p = [2u8, 0];
    let (mut pl, mut dl) = (0u32, 0u32);
    let rc = unsafe {
        if is_cd {
            DosDevIOCtl(hf, 0x80, 0x44, sig.as_mut_ptr(), 4, &mut pl, core::ptr::null_mut(), 0, &mut dl)
        } else {
            DosDevIOCtl(hf, IOCTL_DISK, 0x40, p.as_mut_ptr(), 2, &mut pl, core::ptr::null_mut(), 0, &mut dl)
        }
    };
    unsafe { DosClose(hf) };
    if rc != 0 {
        error(rc);
    }
    rc
}
