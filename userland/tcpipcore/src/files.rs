//! Configuration files in %ETC% and their line format: fields separated
//! by blanks, `#` starts a comment.

use alloc::vec::Vec;

use os2::*;

/// The ETC directory: %ETC%, else \MPTN\ETC on the boot drive.
pub fn etc_dir() -> Vec<u8> {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(c"ETC".as_ptr() as *const u8, &mut v) } == 0 && !v.is_null() {
        let s = unsafe { crate::cstr::bytes(v) };
        let s = s.trim_ascii();
        if !s.is_empty() {
            let mut d = s.to_vec();
            while d.last() == Some(&b'\\') || d.last() == Some(&b'/') {
                d.pop();
            }
            return d;
        }
    }
    let mut drive = 3u32;
    unsafe { DosQuerySysInfo(5, 5, &mut drive, 4) };
    let mut d = Vec::new();
    d.push(b'A' + (drive.clamp(1, 26) - 1) as u8);
    d.extend_from_slice(b":\\MPTN\\ETC");
    d
}

/// An environment variable.
pub fn env(name: &core::ffi::CStr) -> Option<Vec<u8>> {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(name.as_ptr() as *const u8, &mut v) } != 0 || v.is_null() {
        return None;
    }
    Some(unsafe { crate::cstr::bytes(v) }.to_vec())
}

/// The contents of a file.
pub fn read(path: &[u8]) -> Option<Vec<u8>> {
    let z = crate::cstr::z(path);
    let mut h = 0u32;
    let mut action = 0u32;
    let rc = unsafe {
        DosOpen(z.as_ptr(), &mut h, &mut action, 0, FILE_NORMAL, OPEN_ACTION_FAIL_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE, core::ptr::null_mut())
    };
    if rc != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0u32;
        if unsafe { DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
        if out.len() > 4 << 20 {
            break;
        }
    }
    unsafe { DosClose(h) };
    Some(out)
}

/// A file of the ETC directory.
pub fn read_etc(name: &[u8]) -> Option<Vec<u8>> {
    let mut p = etc_dir();
    p.push(b'\\');
    p.extend_from_slice(name);
    read(&p)
}

/// The blank-separated fields of each line, comments removed.
pub fn records(text: &[u8]) -> impl Iterator<Item = Vec<&[u8]>> {
    text.split(|&c| c == b'\n').filter_map(|line| {
        let line = match line.iter().position(|&c| c == b'#' || c == 0x1a) {
            Some(i) => &line[..i],
            None => line,
        };
        let f: Vec<&[u8]> = line.split(|c| c.is_ascii_whitespace()).filter(|w| !w.is_empty()).collect();
        (!f.is_empty()).then_some(f)
    })
}
