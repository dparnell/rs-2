//! Shared code for the base utilities.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use os2::path::cstr;
use os2::*;

pub fn args(s: &Startup) -> Vec<String> {
    let a: String = s.args().iter().map(|&b| b as char).collect();
    split(&a)
}

pub fn split(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                q = !q;
                any = true;
            }
            ' ' | '\t' if !q => {
                if any || !cur.is_empty() {
                    v.push(core::mem::take(&mut cur));
                    any = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        v.push(cur);
    }
    v
}

pub fn out(s: &str) {
    os2::print(s);
}

pub fn outln(s: &str) {
    os2::print(s);
    os2::print("\r\n");
}

pub fn eprint(s: &str) {
    let mut n = 0;
    unsafe { DosWrite(2, s.as_ptr(), s.len() as u32, &mut n) };
}

/// Read all of a handle.
pub fn read_all(hf: u32) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buf = alloc::vec![0u8; 16384];
    loop {
        let mut n = 0;
        if unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n as usize]);
    }
    data
}

pub fn open_read(path: &str) -> Result<u32, u32> {
    let c = cstr(path);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(c.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, core::ptr::null_mut())
    };
    if rc == 0 { Ok(hf) } else { Err(rc) }
}

/// Lines of a byte buffer (CR LF or LF; stops at Ctrl+Z).
pub fn lines(data: &[u8]) -> Vec<String> {
    let text: String = data.iter().take_while(|&&b| b != 0x1a).map(|&b| b as char).collect();
    text.lines().map(|l| String::from(l.trim_end_matches('\r'))).collect()
}

pub struct Entry {
    pub name: String,
    pub attr: u32,
    pub size: u32,
}

/// Directory entries matching a specification.
pub fn find(spec: &str, attr: u32) -> Vec<Entry> {
    let mut v = Vec::new();
    let c = cstr(spec);
    let mut hdir = HDIR_CREATE;
    let mut buf = alloc::vec![0u8; 8192];
    let mut count = 64;
    let mut rc = unsafe { DosFindFirst(c.as_ptr(), &mut hdir, attr, buf.as_mut_ptr(), buf.len() as u32, &mut count, FIL_STANDARD) };
    if rc != 0 {
        return v;
    }
    loop {
        let mut off = 0usize;
        for _ in 0..count {
            let e = &buf[off..];
            let next = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
            let size = u32::from_le_bytes([e[16], e[17], e[18], e[19]]);
            let attr = u32::from_le_bytes([e[24], e[25], e[26], e[27]]);
            let n = e[28] as usize;
            let name: String = e[29..29 + n].iter().map(|&b| b as char).collect();
            if name != "." && name != ".." {
                v.push(Entry { name, attr, size });
            }
            if next == 0 {
                break;
            }
            off += next;
        }
        count = 64;
        rc = unsafe { DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
        if rc != 0 {
            break;
        }
    }
    unsafe { DosFindClose(hdir) };
    v
}

pub fn dir_of(spec: &str) -> String {
    match spec.rfind(['\\', ':']) {
        Some(i) => String::from(&spec[..=i]),
        None => String::new(),
    }
}

pub fn is_dir(p: &str) -> bool {
    let mut fs = FileStatus3::default();
    let c = cstr(p);
    let rc = unsafe { DosQueryPathInfo(c.as_ptr(), FIL_STANDARD, &mut fs as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) };
    rc == 0 && fs.attr & FILE_DIRECTORY != 0
}
