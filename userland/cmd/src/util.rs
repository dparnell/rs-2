//! Helpers: output, paths, file information.

use alloc::string::String;
use alloc::vec::Vec;

use os2::path::cstr;
use os2::*;

pub fn out(s: &str) {
    os2::print(s);
}

pub fn outln(s: &str) {
    os2::print(s);
    os2::print("\r\n");
}

pub fn err_out(s: &str) {
    let mut n = 0;
    unsafe { DosWrite(2, s.as_ptr(), s.len() as u32, &mut n) };
}

/// Print an error the way the system reports it: "SYSnnnn: text".
pub fn sys_error(rc: u32) {
    let text = match rc {
        2 => "The system cannot find the file specified.",
        3 => "The system cannot find the path specified.",
        4 => "The system cannot open the file.",
        5 => "Access is denied.",
        8 => "There is not enough memory available.",
        15 => "The system cannot find the drive specified.",
        16 => "The directory cannot be removed.",
        17 => "The system cannot move the file to a different disk drive.",
        18 => "There are no more files.",
        19 => "The diskette or disk is write protected.",
        21 => "The drive is not ready.",
        32 => "The process cannot access the file because it is being used by another process.",
        80 => "The file already exists.",
        82 => "The directory or file cannot be created.",
        87 => "The parameter is incorrect.",
        110 => "The system cannot open the device or file specified.",
        112 => "There is not enough space on the disk.",
        123 => "The file name, directory name, or volume label syntax is incorrect.",
        145 => "The directory is not empty.",
        191 => "The program cannot be run in OS/2 mode.",
        193 => "The file is not a valid OS/2 program.",
        206 => "The file name or extension is too long.",
        _ => "An unexpected error occurred.",
    };
    err_out(&alloc::format!("SYS{rc:04}: {text}\r\n"));
}

pub fn current_drive() -> u32 {
    let (mut d, mut map) = (0, 0);
    unsafe { DosQueryCurrentDisk(&mut d, &mut map) };
    d
}

/// Current directory of a drive (1 = A:), as "X:\path".
pub fn current_dir(drive: u32) -> String {
    let d = if drive == 0 { current_drive() } else { drive };
    let mut buf = [0u8; 260];
    let mut cb = buf.len() as u32;
    let rc = unsafe { DosQueryCurrentDir(d, buf.as_mut_ptr(), &mut cb) };
    let mut s = String::new();
    s.push((b'A' + d as u8 - 1) as char);
    s.push_str(":\\");
    if rc == 0 {
        s.push_str(&os2::path::from_bytes(&buf));
    }
    s
}

/// Fully qualified form of a path (DosQueryPathInfo level 5).
pub fn full_path(p: &str) -> String {
    let mut buf = [0u8; 260];
    let c = cstr(p);
    let rc = unsafe { DosQueryPathInfo(c.as_ptr(), FIL_QUERYFULLNAME, buf.as_mut_ptr(), buf.len() as u32) };
    if rc == 0 { os2::path::from_bytes(&buf) } else { String::from(p) }
}

pub fn path_info(p: &str) -> Option<FileStatus3> {
    let mut fs = FileStatus3::default();
    let c = cstr(p);
    let rc = unsafe {
        DosQueryPathInfo(c.as_ptr(), FIL_STANDARD, &mut fs as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32)
    };
    (rc == 0).then_some(fs)
}

pub fn is_dir(p: &str) -> bool {
    let t = p.trim_end_matches('\\');
    if t.len() == 2 && t.ends_with(':') || t.is_empty() {
        return true;
    }
    path_info(p).is_some_and(|f| f.attr & FILE_DIRECTORY != 0)
}

pub fn exists(p: &str) -> bool {
    path_info(p).is_some()
}

/// One directory search result.
pub struct Found {
    pub name: String,
    pub attr: u32,
    pub size: u32,
    pub date: u16,
    pub time: u16,
}

/// All matches of a wildcard specification.
pub fn find(spec: &str, attr: u32) -> Result<Vec<Found>, u32> {
    let mut v = Vec::new();
    let c = cstr(spec);
    let mut hdir = HDIR_CREATE;
    let mut buf = alloc::vec![0u8; 8192];
    let mut count = 64;
    let mut rc = unsafe { DosFindFirst(c.as_ptr(), &mut hdir, attr, buf.as_mut_ptr(), buf.len() as u32, &mut count, FIL_STANDARD) };
    if rc != 0 {
        return Err(rc);
    }
    loop {
        let mut off = 0usize;
        for _ in 0..count {
            let e = &buf[off..];
            let next = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
            let date = u16::from_le_bytes([e[12], e[13]]);
            let time = u16::from_le_bytes([e[14], e[15]]);
            let size = u32::from_le_bytes([e[16], e[17], e[18], e[19]]);
            let attr = u32::from_le_bytes([e[24], e[25], e[26], e[27]]);
            let n = e[28] as usize;
            let name: String = e[29..29 + n].iter().map(|&b| b as char).collect();
            v.push(Found { name, attr, size, date, time });
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
    Ok(v)
}

pub fn has_wildcards(s: &str) -> bool {
    s.contains(['*', '?'])
}

/// Directory part of a path (with trailing backslash) and the name part.
pub fn split_path(p: &str) -> (String, String) {
    match p.rfind(['\\', '/', ':']) {
        Some(i) => (String::from(&p[..=i]), String::from(&p[i + 1..])),
        None => (String::new(), String::from(p)),
    }
}

/// Format a FAT date as mm-dd-yy and time as h:mma.
pub fn fmt_date(d: u16) -> String {
    let (y, m, day) = (1980 + (d >> 9), (d >> 5) & 15, d & 31);
    alloc::format!("{:02}-{:02}-{:02}", m, day, y % 100)
}

pub fn fmt_time(t: u16) -> String {
    let (h, m) = (t >> 11, (t >> 5) & 63);
    let (h12, ap) = match h {
        0 => (12, 'a'),
        1..=11 => (h, 'a'),
        12 => (12, 'p'),
        _ => (h - 12, 'p'),
    };
    alloc::format!("{:>2}:{:02}{}", h12, m, ap)
}

/// Read a whole line from standard input (without the line end); None at
/// end of file.
pub fn read_line() -> Option<String> {
    let mut buf = [0u8; 512];
    let mut n = 0;
    let rc = unsafe { DosRead(0, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    if rc != 0 || n == 0 {
        return None;
    }
    let s: String = buf[..n as usize].iter().map(|&b| b as char).collect();
    Some(String::from(s.trim_end_matches(['\r', '\n'])))
}

/// Group digits with commas.
pub fn commas(mut v: u64) -> String {
    let mut parts = Vec::new();
    loop {
        if v < 1000 {
            parts.push(alloc::format!("{v}"));
            break;
        }
        parts.push(alloc::format!("{:03}", v % 1000));
        v /= 1000;
    }
    parts.reverse();
    parts.join(",")
}
