//! HELPMGR's unpublished ordinals 38-44 (studied in IBM's HELPMGR with
//! Ghidra): MscCleanUp, the process's exit list routine, and the file
//! name helpers IBM's help manager uses for libraries and bookmark
//! files (MscAppendINF, MscMakeFullyQualifiedPathName,
//! MscGenerateCPFileName). IBM's work on wide-character copies of
//! the names (`_DBMtoDBX`); rs2's on the bytes, which is the same for
//! single-byte code pages. ChopDot is the exception: IBM exports its
//! wide-character form, and so does rs2.

use crate::pm::*;
use alloc::vec::Vec;

/// MscCleanUp(ulTermCode) (38): the exit list routine the first help
/// instance of a process adds (IBM's HMWindowProc does on WM_CREATE):
/// the help hook of the process's help instances goes (books have none)
/// and the exit list goes on.
#[unsafe(no_mangle)]
pub extern "C" fn MscCleanUp(_code: u32) {
    unsafe { os2::dos::DosEnterCritSec() };
    crate::release_hooks();
    unsafe {
        os2::dos::DosExitCritSec();
        // EXLST_EXIT.
        os2::dos::DosExitList(3, exit_routine());
    }
}

/// MscCleanUp's entry point (the stack-aligning one) for DosExitList.
pub fn exit_routine() -> os2::dos::ExitListFn {
    unsafe { core::mem::transmute::<extern "C" fn(), os2::dos::ExitListFn>(crate::exports::__a_MscCleanUp) }
}

/// ChopDot(pwsz, cch) (40): the length of the first `cch` wide
/// characters of a name without its trailing blanks and dots; `.` and
/// `..` stay as they are.
#[unsafe(no_mangle)]
pub extern "C" fn ChopDot(s: *const u16, n: u32) -> u32 {
    let n = n as u16 as usize;
    if s.is_null() || n == 0 || unsafe { *s } == 0 {
        return 0;
    }
    let w = unsafe { core::slice::from_raw_parts(s, n) };
    let dot = b'.' as u16;
    if (n == 1 && w[0] == dot) || (n == 2 && w[0] == dot && w[1] == dot) {
        return n as u32;
    }
    w.iter().rposition(|&c| c != b' ' as u16 && c != dot).map_or(0, |i| i as u32 + 1)
}

/// The byte form of ChopDot.
fn chop(w: &[u8]) -> usize {
    if w.is_empty() || w[0] == 0 {
        return 0;
    }
    if w == b"." || w == b".." {
        return w.len();
    }
    w.iter().rposition(|&c| c != b' ' && c != b'.').map_or(0, |i| i + 1)
}

fn put(dst: *mut u8, s: &[u8]) {
    if dst.is_null() {
        return;
    }
    unsafe {
        core::ptr::copy(s.as_ptr(), dst, s.len());
        *dst.add(s.len()) = 0;
    }
}

fn is_sep(c: u8) -> bool {
    c == b'\\' || c == b'/'
}

/// HPFSSplitPath(pszPath, pszDrive, pszDir, pszName, pszExt) (39): the
/// parts of a path. The drive (`C:`); the directory up to its last
/// separator, each part without trailing blanks and dots; the name and
/// the extension (from the last dot, if it has at most three characters
/// after it, else part of the name), trailing blanks and dots dropped.
/// `pszPath` itself is left holding the name and extension as they were.
#[unsafe(no_mangle)]
pub extern "C" fn HPFSSplitPath(path: *mut u8, drive: *mut u8, dir: *mut u8, name: *mut u8, ext: *mut u8) {
    let p: Vec<u8> = unsafe { cstr(path) }.to_vec();
    let (drv, rest): (&[u8], &[u8]) = if p.len() >= 2 && p[1] == b':' { (&p[..2], &p[2..]) } else { (&[], &p[..]) };
    let mut d: Vec<u8> = Vec::new();
    let last = rest.iter().rposition(|&c| is_sep(c));
    let file = match last {
        None => rest,
        Some(l) => {
            // Each part starts with its separator (the first may not):
            // `\a.\b` gives `\a\b\`.
            let mut i = 0;
            while i < l {
                let mut k = 1;
                while i + k < rest.len() && !is_sep(rest[i + k]) {
                    k += 1;
                }
                d.extend_from_slice(&rest[i..i + chop(&rest[i..i + k])]);
                i += k;
            }
            d.push(rest[l]);
            &rest[l + 1..]
        }
    };
    let mut end = file.len();
    if end > 2 || (end == 2 && file[0] != b'.') {
        while end > 0 && (file[end - 1] == b' ' || file[end - 1] == b'.') {
            end -= 1;
        }
    }
    let (n, e) = match file[..end].iter().rposition(|&c| c == b'.') {
        Some(dot) if end - dot <= 4 => (&file[..dot], &file[dot..end]),
        _ => (&file[..end], &b""[..]),
    };
    let n = &n[..n.len().min(0x103)];
    let file = file.to_vec();
    put(path, &file);
    put(drive, drv);
    put(dir, &d);
    put(name, n);
    put(ext, e);
}

/// MakeHPFSFileList(pszSeparators, pszList) (41): the names in `pszList`
/// separated by any of `pszSeparators` (blanks before a name skipped, a
/// name in quotes taken as it is), put back in `pszList` one after the
/// other, each NUL-ended; the number of names, 0 if a quote is not
/// closed.
#[unsafe(no_mangle)]
pub extern "C" fn MakeHPFSFileList(seps: *const u8, list: *mut u8) -> u32 {
    if list.is_null() {
        return 0;
    }
    let seps = unsafe { cstr(seps) };
    let s: Vec<u8> = unsafe { cstr(list) }.to_vec();
    let mut out: Vec<u8> = Vec::with_capacity(s.len() + 1);
    let mut n = 0;
    let mut i = 0;
    loop {
        while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        while i < s.len() && seps.contains(&s[i]) {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        let q = s[i];
        if q == b'"' || q == b'\'' {
            let Some(k) = s[i + 1..].iter().position(|&c| c == q) else { return 0 };
            out.extend_from_slice(&s[i + 1..i + 1 + k]);
            out.push(0);
            n += 1;
            i += k + 2;
            continue;
        }
        match s[i..].iter().position(|c| seps.contains(c)) {
            Some(k) => {
                out.extend_from_slice(&s[i..i + k]);
                out.push(0);
                n += 1;
                i += k + 1;
            }
            None => {
                out.extend_from_slice(&s[i..]);
                out.push(0);
                n += 1;
                break;
            }
        }
    }
    GoodList(list, out.as_ptr(), n)
}

/// GoodList(pszTo, pszFrom, cNames) (42): copy `cNames` NUL-ended names
/// one after the other; returns `cNames`.
#[unsafe(no_mangle)]
pub extern "C" fn GoodList(to: *mut u8, from: *const u8, n: u32) -> u32 {
    let (mut t, mut f) = (to, from);
    for _ in 0..n {
        let s = unsafe { cstr(f) };
        let len = s.len();
        put(t, s);
        unsafe {
            t = t.add(len + 1);
            f = f.add(len + 1);
        }
    }
    n
}

/// IsValidHPFSName(pszName) (43): 1 to 255 characters, not `.` or `..`,
/// none of them control characters or `<>;"'|\/`.
#[unsafe(no_mangle)]
pub extern "C" fn IsValidHPFSName(name: *const u8) -> u32 {
    let s = unsafe { cstr(name) };
    if s.is_empty() || s.len() > 255 || s == b"." || s == b".." {
        return 0;
    }
    !s.iter().any(|&c| c < 0x20 || b"<>;\"'|\\/".contains(&c)) as u32
}

/// HPFSMakeFullPathName(pszDrive, pszDir, pszName, pszExt, pszPath) (44):
/// `pszPath` = drive, directory, name and extension. An empty drive
/// becomes the current one (not for a UNC directory, `\\server`), an
/// empty directory that drive's current directory (`\DIR\`, or `\`),
/// both written back to their buffers.
#[unsafe(no_mangle)]
pub extern "C" fn HPFSMakeFullPathName(drive: *mut u8, dir: *mut u8, name: *const u8, ext: *const u8, out: *mut u8) {
    if out.is_null() || drive.is_null() || dir.is_null() {
        return;
    }
    unsafe { *out = 0 };
    let mut cur = [0u8; 260];
    let mut cb = 260u32;
    let d0 = unsafe { *drive };
    if d0 == 0 {
        let unc = unsafe { cstr(dir) }.starts_with(b"\\\\");
        if !unc {
            let (mut disk, mut map) = (0u32, 0u32);
            unsafe { os2::dos::DosQueryCurrentDisk(&mut disk, &mut map) };
            put(drive, &[b'@' + disk as u8, b':']);
            unsafe { os2::dos::DosQueryCurrentDir(disk, cur.as_mut_ptr(), &mut cb) };
        }
    } else {
        unsafe { os2::dos::DosQueryCurrentDir((d0 as u32).wrapping_sub(0x40), cur.as_mut_ptr(), &mut cb) };
    }
    if unsafe { *dir } == 0 {
        let c = unsafe { cstr(cur.as_ptr()) };
        let mut d = Vec::with_capacity(c.len() + 2);
        d.push(b'\\');
        if !c.is_empty() {
            d.extend_from_slice(c);
            d.push(b'\\');
        }
        put(dir, &d);
    }
    let mut p: Vec<u8> = Vec::new();
    for s in [drive as *const u8, dir as *const u8, name, ext] {
        p.extend_from_slice(unsafe { cstr(s) });
    }
    put(out, &p);
}
