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

pub mod disk;

pub fn stdin_is_console() -> bool {
    let (mut ty, mut attr) = (0, 0);
    unsafe { DosQueryHType(0, &mut ty, &mut attr) };
    ty & 0xff == 1
}

/// Prompt and read a line from standard input (trimmed).
pub fn ask(prompt: &str) -> String {
    out(prompt);
    let mut buf = [0u8; 128];
    let mut n = 0;
    if stdin_is_console() {
        // The keyboard delivers a whole line.
        unsafe { DosRead(0, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    } else {
        // A file or pipe: take one line of it, echoed like typed input.
        while (n as usize) < buf.len() {
            let mut got = 0;
            if unsafe { DosRead(0, buf[n as usize..].as_mut_ptr(), 1, &mut got) } != 0 || got == 0 {
                break;
            }
            n += 1;
            if buf[n as usize - 1] == b'\n' {
                break;
            }
        }
        os2::write(&buf[..n as usize]);
    }
    String::from(buf[..n as usize].iter().map(|&b| b as char).collect::<String>().trim())
}

/// Ask a Y/N question; true for yes.
pub fn yes(prompt: &str) -> bool {
    loop {
        let a = ask(prompt);
        match a.chars().next().map(|c| c.to_ascii_uppercase()) {
            Some('Y') => return true,
            Some('N') => return false,
            // End of redirected input.
            None if !stdin_is_console() => return false,
            _ => {}
        }
    }
}

/// A drive argument `d:` (0 = A:), or the current drive if `None`.
pub fn drive_arg(a: Option<&str>) -> Option<u8> {
    match a {
        Some(s) => {
            let b = s.as_bytes();
            (b.len() == 2 && b[1] == b':' && b[0].is_ascii_alphabetic()).then(|| b[0].to_ascii_uppercase() - b'A')
        }
        None => {
            let (mut d, mut map) = (0, 0);
            unsafe { DosQueryCurrentDisk(&mut d, &mut map) };
            Some((d - 1) as u8)
        }
    }
}

/// Volume serial number as `XXXX-XXXX`.
pub fn serial(s: u32) -> String {
    alloc::format!("{:04X}-{:04X}", s >> 16, s & 0xffff)
}

/// Current volume label and serial number (DosQueryFSInfo level 2).
pub fn volume_label(drive: u8) -> Result<(String, u32), u32> {
    let mut buf = [0u8; 20];
    let rc = unsafe { DosQueryFSInfo(drive as u32 + 1, FSIL_VOLSER, buf.as_mut_ptr(), buf.len() as u32) };
    if rc != 0 {
        return Err(rc);
    }
    let n = (buf[4] as usize).min(11);
    let label = buf[5..5 + n].iter().map(|&b| b as char).collect();
    Ok((label, u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])))
}

/// Set the volume label (DosSetFSInfo level 2).
pub fn set_volume_label(drive: u8, label: &str) -> u32 {
    let mut buf = [0u8; 13];
    let n = label.len().min(11);
    buf[0] = n as u8;
    buf[1..1 + n].copy_from_slice(&label.as_bytes()[..n]);
    unsafe { DosSetFSInfo(drive as u32 + 1, FSIL_VOLSER, buf.as_ptr(), (n + 2) as u32) }
}

/// OS/2 message text for an error code (OSO001.MSG), without the line end.
pub fn sys_message(rc: u32) -> String {
    let mut buf = [0u8; 512];
    let mut n = 0;
    let file = b"OSO001.MSG\0";
    let r = unsafe { DosGetMessage(core::ptr::null(), 0, buf.as_mut_ptr(), buf.len() as u32, rc, file.as_ptr(), &mut n) };
    if r != 0 {
        // No message file: short texts for the common codes.
        let t = match rc {
            2 => "The file cannot be found.",
            3 => "The path cannot be found.",
            5 => "Access is denied.",
            15 => "The drive specified is not valid.",
            19 => "The disk is write-protected.",
            21 => "The drive is not ready.",
            32 => "The file is in use by another process.",
            50 => "The request is not supported.",
            87 => "A parameter is not correct.",
            108 => "The drive is locked by another process.",
            112 => "There is not enough space on the disk.",
            _ => "",
        };
        return alloc::format!("SYS{rc:04}: {t}");
    }
    String::from(buf[..n as usize].iter().map(|&b| b as char).collect::<String>().trim_end())
}

/// Print an OS/2 error message for `rc` on standard error.
pub fn error(rc: u32) {
    eprint(&sys_message(rc));
    eprint("\r\n");
}

/// Right-align a number in `w` columns.
pub fn num(n: u64, w: usize) -> String {
    alloc::format!("{n:>w$}")
}

pub fn is_dir(p: &str) -> bool {
    let mut fs = FileStatus3::default();
    let c = cstr(p);
    let rc = unsafe { DosQueryPathInfo(c.as_ptr(), FIL_STANDARD, &mut fs as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) };
    rc == 0 && fs.attr & FILE_DIRECTORY != 0
}
