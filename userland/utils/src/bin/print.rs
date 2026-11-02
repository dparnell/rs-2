//! PRINT: print files, or cancel print jobs.
//! PRINT [/D:device] [/B] file...      PRINT [/D:device] /C | /T
//!
//! Files are written to the printer device (LPT1 unless /D names
//! another), which the kernel hands to the spooler as a raw job on the
//! queue printing to that port. Text stops at Ctrl+Z unless /B. /C
//! deletes the job the device's printer is printing, /T every job of the
//! queues that print to it (through PMSPL's SplEnumDevice, SplEnumQueue,
//! SplDeleteJob and SplPurgeQueue).
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use os2::path::cstr;
use os2::*;
use utils::msgs::{err, say};
use utils::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMSPL.382"]
    fn SplEnumDevice(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.399"]
    fn SplEnumQueue(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.386"]
    fn SplDeleteJob(comp: *const u8, q: *const u8, id: u32) -> u32;
    #[link_name = "PMSPL.404"]
    fn SplPurgeQueue(comp: *const u8, q: *const u8) -> u32;
}

fn pstr(p: u32) -> String {
    if p == 0 {
        return String::new();
    }
    let mut s = String::new();
    let mut a = p as *const u8;
    unsafe {
        while *a != 0 && s.len() < 512 {
            s.push(*a as char);
            a = a.add(1);
        }
    }
    s
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn enumerate(f: unsafe extern "C" fn(*const u8, u32, *mut u8, u32, *mut u32, *mut u32, *mut u32, u32) -> u32, _size: usize) -> (Vec<u8>, u32) {
    let mut buf = alloc::vec![0u8; 16384];
    let (mut ret, mut tot, mut need) = (0, 0, 0);
    let rc = unsafe { f(core::ptr::null(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut tot, &mut need, 0) };
    if rc != 0 && rc != 234 {
        return (Vec::new(), 0);
    }
    (buf, ret)
}

/// Cancel jobs on the queues printing to `device`: the current job, or
/// all of them.
fn cancel(device: &str, all: bool) -> u32 {
    // PRDINFO3 (packed): printer, user, port, job id, status, ...
    let (devs, nd) = enumerate(SplEnumDevice, 34);
    let printers: Vec<(String, u16)> = (0..nd as usize)
        .map(|i| &devs[i * 34..])
        .filter(|r| pstr(rd32(r, 8)).eq_ignore_ascii_case(device))
        .map(|r| (pstr(rd32(r, 0)), rd16(r, 12)))
        .collect();
    // PRQINFO3 (packed, 44 bytes): name, ..., printers at +32.
    let (qs, nq) = enumerate(SplEnumQueue, 44);
    let queues: Vec<String> = (0..nq as usize)
        .map(|i| &qs[i * 44..])
        .filter(|r| pstr(rd32(r, 32)).split(',').any(|p| printers.iter().any(|(n, _)| n.eq_ignore_ascii_case(p))))
        .map(|r| pstr(rd32(r, 0)))
        .collect();
    if queues.is_empty() {
        say(1777, &[], "No print queue prints to that device, so the job\r\ncannot be cancelled.\r\n");
        return 1;
    }
    for q in &queues {
        let c = cstr(q);
        if all {
            unsafe { SplPurgeQueue(core::ptr::null(), c.as_ptr()) };
        }
    }
    let busy: Vec<u16> = printers.iter().map(|p| p.1).filter(|&j| j != 0).collect();
    if busy.is_empty() {
        if !all {
            say(1775, &[], "Nothing is printing now.  The print queue shows\r\nthe state of the job.\r\n");
        }
        return 0;
    }
    for j in busy {
        for q in &queues {
            let c = cstr(q);
            if unsafe { SplDeleteJob(core::ptr::null(), c.as_ptr(), j as u32) } == 0 {
                break;
            }
        }
    }
    0
}

fn main(s: &os2::Startup) -> u32 {
    let mut device = String::from("LPT1");
    let (mut binary, mut cancel_one, mut cancel_all) = (false, false, false);
    let mut names: Vec<String> = Vec::new();
    for a in args(s) {
        let u = a.to_ascii_uppercase();
        match u.as_str() {
            "/B" => binary = true,
            "/C" => cancel_one = true,
            "/T" => cancel_all = true,
            _ if u.starts_with("/D:") => device = String::from(u[3..].trim_end_matches(':')),
            _ if u.starts_with('/') => {
                err(1761, &[&a], "The parameter %1 is not correct.\r\n");
                return 1;
            }
            _ => names.push(a),
        }
    }
    if device == "PRN" {
        device = String::from("LPT1");
    }
    if cancel_one || cancel_all {
        if !names.is_empty() {
            err(1003, &[], "The command syntax is not correct.\r\n");
            return 1;
        }
        return cancel(&device, cancel_all);
    }
    if names.is_empty() {
        err(1003, &[], "The command syntax is not correct.\r\n");
        return 1;
    }
    let mut rc = 0;
    for spec in &names {
        let dir = dir_of(spec);
        let found = find(spec, FILE_READONLY | FILE_ARCHIVED);
        if found.is_empty() {
            error(2);
            rc = 2;
            continue;
        }
        for f in found {
            let path = alloc::format!("{dir}{}", f.name);
            let mut data = match utils::files::read_file(&path) {
                Ok(d) => d,
                Err(e) => {
                    error(e);
                    rc = e;
                    continue;
                }
            };
            if !binary {
                if let Some(z) = data.iter().position(|&b| b == 0x1a) {
                    data.truncate(z);
                }
            }
            // One open per file: one print job per file.
            let c = cstr(&device);
            let (mut h, mut act) = (0, 0);
            let r = unsafe { DosOpen(c.as_ptr(), &mut h, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYNONE, core::ptr::null_mut()) };
            if r != 0 {
                err(1779, &[], "The print device is not installed or not ready.\r\n");
                return r;
            }
            let w = utils::files::write_all(h, &data);
            unsafe { DosClose(h) };
            if let Err(e) = w {
                error(e);
                rc = e;
            }
        }
    }
    rc
}
