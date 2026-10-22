//! File operations of the shell, free of its windows (DESKTEST includes
//! this file): directory listings, sizes of trees, copying (DosCopy for
//! each file, so its EAs, attributes and dates come along; directories
//! are made one by one with their EAs, so a copy reports each file),
//! moving (DosMove on one drive, else copy and delete), deleting trees.

#![allow(dead_code)]

use alloc::vec::Vec;

use crate::pm::z;

pub fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(name);
    p
}

/// A path without a trailing backslash (except a drive's root, `C:\`).
pub fn strip(p: &[u8]) -> &[u8] {
    match p.strip_suffix(b"\\") {
        Some(t) if t.len() > 2 => t,
        _ => p,
    }
}

/// The directory part of a path (with its backslash).
pub fn parent(p: &[u8]) -> Vec<u8> {
    let t = strip(p);
    match t.iter().rposition(|&c| c == b'\\') {
        Some(i) => t[..=i].to_vec(),
        None => Vec::new(),
    }
}

pub fn name_of(p: &[u8]) -> &[u8] {
    let t = strip(p);
    match t.iter().rposition(|&c| c == b'\\') {
        Some(i) => &t[i + 1..],
        None => t,
    }
}

/// (attributes, size) of a path, if it exists.
pub fn stat(p: &[u8]) -> Option<(u32, u32)> {
    let c = z(strip(p));
    let mut st = os2::FileStatus3::default();
    let rc = unsafe { os2::DosQueryPathInfo(c.as_ptr(), 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<os2::FileStatus3>() as u32) };
    (rc == 0).then_some((st.attr, st.cb_file))
}

pub fn exists(p: &[u8]) -> bool {
    stat(p).is_some()
}

pub fn is_dir(p: &[u8]) -> bool {
    // A drive's root is a directory.
    let t = strip(p);
    if t.len() == 3 && t[1] == b':' {
        return true;
    }
    stat(p).is_some_and(|s| s.0 & 0x10 != 0)
}

pub fn drive_of(p: &[u8]) -> u8 {
    if p.len() >= 2 && p[1] == b':' { p[0].to_ascii_uppercase() } else { 0 }
}

pub fn same_path(a: &[u8], b: &[u8]) -> bool {
    strip(a).eq_ignore_ascii_case(strip(b))
}

/// Whether `inner` is `outer` or inside it.
pub fn is_within(inner: &[u8], outer: &[u8]) -> bool {
    let (i, o) = (strip(inner).to_ascii_uppercase(), strip(outer).to_ascii_uppercase());
    i == o || (i.len() > o.len() && i.starts_with(&o) && (o.ends_with(b"\\") || i[o.len()] == b'\\'))
}

/// Whether a drive's file system takes long names (HPFS, JFS, FAT32's
/// long names are not OS/2's): not FAT.
pub fn long_names(p: &[u8]) -> bool {
    let d = drive_of(p);
    if d == 0 {
        return false;
    }
    let dev = [d, b':', 0];
    let mut buf = [0u8; 64];
    let mut cb = buf.len() as u32;
    if unsafe { os2::DosQueryFSAttach(dev.as_ptr(), 0, 1, buf.as_mut_ptr(), &mut cb) } != 0 {
        return false;
    }
    let cb_name = u16::from_le_bytes([buf[2], buf[3]]) as usize;
    let at = 8 + cb_name + 1;
    let fs = crate::pm::until_nul(buf.get(at..).unwrap_or(&[]));
    !fs.eq_ignore_ascii_case(b"FAT")
}

/// (name, is a directory, size) of a directory's entries.
pub fn listing(dir: &[u8]) -> Vec<(Vec<u8>, bool, u32)> {
    let spec = z(&join(dir, b"*"));
    let mut buf = alloc::vec![0u8; 4096];
    let mut hdir = 0xffff_ffffu32;
    let mut count = 32u32;
    let mut out = Vec::new();
    let mut rc = unsafe { os2::DosFindFirst(spec.as_ptr(), &mut hdir, 0x37, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    while rc == 0 && count > 0 {
        let mut at = 0usize;
        for _ in 0..count {
            let u = |o: usize| u32::from_le_bytes([buf[at + o], buf[at + o + 1], buf[at + o + 2], buf[at + o + 3]]);
            let (next, size, attr) = (u(0), u(16), u(24));
            let n = buf[at + 28] as usize;
            let name = buf[at + 29..at + 29 + n].to_vec();
            if name != b"." && name != b".." {
                out.push((name, attr & 0x10 != 0, size));
            }
            if next == 0 {
                break;
            }
            at += next as usize;
        }
        count = 32;
        rc = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    }
    unsafe { os2::DosFindClose(hdir) };
    out
}

/// Whether a directory has a subdirectory (for the tree view's buttons).
pub fn has_subdirs(dir: &[u8]) -> bool {
    let spec = z(&join(dir, b"*"));
    let mut buf = alloc::vec![0u8; 1024];
    let mut hdir = 0xffff_ffffu32;
    let mut count = 8u32;
    // MUST_HAVE_DIRECTORY | FILE_DIRECTORY | hidden, system, read-only, archived.
    let mut rc = unsafe { os2::DosFindFirst(spec.as_ptr(), &mut hdir, 0x1037, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    let mut found = false;
    while rc == 0 && count > 0 && !found {
        let mut at = 0usize;
        for _ in 0..count {
            let next = u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]);
            let n = buf[at + 28] as usize;
            let name = &buf[at + 29..at + 29 + n];
            if name != b"." && name != b".." {
                found = true;
                break;
            }
            if next == 0 {
                break;
            }
            at += next as usize;
        }
        count = 8;
        if !found {
            rc = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
        }
    }
    unsafe { os2::DosFindClose(hdir) };
    found
}

/// Files and bytes in a file or tree.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Totals {
    pub files: u32,
    pub bytes: u64,
}

pub fn measure(p: &[u8]) -> Totals {
    let mut t = Totals::default();
    if is_dir(p) {
        for (name, dir, size) in listing(p) {
            if dir {
                let s = measure(&join(p, &name));
                t.files += s.files;
                t.bytes += s.bytes;
            } else {
                t.files += 1;
                t.bytes += size as u64;
            }
        }
    } else if let Some((_, size)) = stat(p) {
        t.files = 1;
        t.bytes = size as u64;
    }
    t
}

/// Copy the EAs of `src` to `dst` (a directory's: DosCopy brings a
/// file's). DENA2 list from DosEnumAttribute, the GEA2 list for
/// DosQueryPathInfo's FIL_QUERYEASFROMLIST, the FEA2 list back with
/// DosSetPathInfo's FIL_QUERYEASIZE.
pub fn copy_eas(src: &[u8], dst: &[u8]) -> u32 {
    let s = z(strip(src));
    let d = z(strip(dst));
    let mut dena = alloc::vec![0u8; 16384];
    let mut count = 0xffff_ffffu32;
    // ENUMEA_REFTYPE_PATH, from the first, ENUMEA_LEVEL_NO_VALUE.
    let rc = unsafe { os2::DosEnumAttribute(1, s.as_ptr(), 1, dena.as_mut_ptr(), dena.len() as u32, &mut count, 1) };
    if rc != 0 || count == 0 || count == 0xffff_ffff {
        return 0;
    }
    // GEA2LIST: cbList, then GEA2s (oNextEntryOffset, cbName, name, NUL),
    // each on a 4-byte boundary.
    let mut gea = alloc::vec![0u8; 4];
    let mut need = 4usize;
    let mut at = 0usize;
    let mut last = 0usize;
    for _ in 0..count {
        let next = u32::from_le_bytes([dena[at], dena[at + 1], dena[at + 2], dena[at + 3]]) as usize;
        let cb_name = dena[at + 5] as usize;
        let cb_value = u16::from_le_bytes([dena[at + 6], dena[at + 7]]) as usize;
        let name = &dena[at + 8..at + 8 + cb_name];
        while gea.len() % 4 != 0 {
            gea.push(0);
        }
        last = gea.len();
        gea.extend_from_slice(&0u32.to_le_bytes());
        gea.push(cb_name as u8);
        gea.extend_from_slice(name);
        gea.push(0);
        need += 12 + cb_name + cb_value;
        if next == 0 {
            break;
        }
        at += next;
    }
    // Link the GEA2s.
    let mut p = 4;
    while p < last {
        let cb = gea[p + 4] as usize;
        let len = (5 + cb + 1).next_multiple_of(4);
        gea[p..p + 4].copy_from_slice(&(len as u32).to_le_bytes());
        p += len;
    }
    let n = gea.len() as u32;
    gea[0..4].copy_from_slice(&n.to_le_bytes());
    let mut fea = alloc::vec![0u8; need + 64];
    let cb = fea.len() as u32;
    fea[0..4].copy_from_slice(&cb.to_le_bytes());
    let mut op = os2::EaOp2 { gea2: gea.as_mut_ptr(), fea2: fea.as_mut_ptr(), error: 0 };
    let rc = unsafe { os2::DosQueryPathInfo(s.as_ptr(), 3, &mut op as *mut _ as *mut u8, core::mem::size_of::<os2::EaOp2>() as u32) };
    if rc != 0 {
        return rc;
    }
    let op = os2::EaOp2 { gea2: core::ptr::null_mut(), fea2: fea.as_mut_ptr(), error: 0 };
    let rc = unsafe { os2::DosSetPathInfo(d.as_ptr(), 2, &op as *const _ as *const u8, core::mem::size_of::<os2::EaOp2>() as u32, 0) };
    // A target that holds no EAs (ERROR_EAS_NOT_SUPPORTED) is fine.
    if rc == 282 { 0 } else { rc }
}

pub fn delete_file(p: &[u8]) -> u32 {
    let c = z(p);
    unsafe { os2::DosDelete(c.as_ptr()) }
}

/// Delete a file, or a directory with everything in it.
pub fn delete_path(p: &[u8]) -> u32 {
    if is_dir(p) {
        delete_tree(p)
    } else {
        delete_file(p)
    }
}

pub fn delete_tree(dir: &[u8]) -> u32 {
    for (name, d, _) in listing(dir) {
        let p = join(dir, &name);
        let rc = if d { delete_tree(&p) } else { delete_file(&p) };
        if rc != 0 {
            return rc;
        }
    }
    let c = z(strip(dir));
    unsafe { os2::DosDeleteDir(c.as_ptr()) }
}

/// What a copy reports: a file about to be copied (its path and size);
/// the callback returns false to stop (ERROR_INTERRUPT).
pub type Progress<'a> = &'a mut dyn FnMut(&[u8], u32) -> bool;

pub const ERROR_INTERRUPT: u32 = 95;

/// Copy a file or a tree to `dst` (which must not exist, or, with
/// `replace`, is replaced).
pub fn copy_tree(src: &[u8], dst: &[u8], replace: bool, f: Progress) -> u32 {
    if is_within(dst, src) {
        // ERROR_CURRENT_DIRECTORY... a folder into itself.
        return 16;
    }
    if replace && exists(dst) {
        let rc = delete_path(dst);
        if rc != 0 {
            return rc;
        }
    }
    copy_inner(src, dst, f)
}

fn copy_inner(src: &[u8], dst: &[u8], f: Progress) -> u32 {
    if !is_dir(src) {
        let size = stat(src).map_or(0, |s| s.1);
        if !f(src, size) {
            return ERROR_INTERRUPT;
        }
        let (a, b) = (z(src), z(dst));
        return unsafe { os2::DosCopy(a.as_ptr(), b.as_ptr(), 0) };
    }
    let d = z(strip(dst));
    let rc = unsafe { os2::DosCreateDir(d.as_ptr(), core::ptr::null_mut()) };
    if rc != 0 {
        return rc;
    }
    let rc = copy_eas(src, dst);
    if rc != 0 {
        return rc;
    }
    // Attributes (read-only, hidden...) of the directory.
    if let Some((attr, _)) = stat(src) {
        if attr & 0x27 != 0 {
            let s = z(strip(src));
            let mut st = os2::FileStatus3::default();
            let sz = core::mem::size_of::<os2::FileStatus3>() as u32;
            unsafe {
                if os2::DosQueryPathInfo(s.as_ptr(), 1, &mut st as *mut _ as *mut u8, sz) == 0 {
                    st.attr &= 0x27;
                    os2::DosSetPathInfo(d.as_ptr(), 1, &st as *const _ as *const u8, sz, 0);
                }
            }
        }
    }
    for (name, _, _) in listing(src) {
        let rc = copy_inner(&join(src, &name), &join(dst, &name), f);
        if rc != 0 {
            return rc;
        }
    }
    0
}

/// Move a file or tree to `dst`: renamed on the same drive, else copied
/// and deleted.
pub fn move_path(src: &[u8], dst: &[u8], replace: bool, f: Progress) -> u32 {
    if is_within(dst, src) {
        return 16;
    }
    if drive_of(src) == drive_of(dst) && drive_of(src) != 0 {
        if replace && exists(dst) && !same_path(src, dst) {
            let rc = delete_path(dst);
            if rc != 0 {
                return rc;
            }
        }
        let size = if is_dir(src) { 0 } else { stat(src).map_or(0, |s| s.1) };
        if !f(src, size) {
            return ERROR_INTERRUPT;
        }
        let (a, b) = (z(strip(src)), z(strip(dst)));
        return unsafe { os2::DosMove(a.as_ptr(), b.as_ptr()) };
    }
    let rc = copy_tree(src, dst, replace, f);
    if rc != 0 {
        return rc;
    }
    delete_path(src)
}
