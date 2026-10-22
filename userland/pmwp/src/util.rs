//! Helpers for the Workplace Shell classes: strings, profiles (OS2.INI),
//! extended attributes, files.

use alloc::boxed::Box;
use alloc::vec::Vec;

unsafe extern "C" {
    #[link_name = "PMSHAPI.117"]
    fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.101"]
    fn PrfQueryProfileSize(hini: u32, app: *const u8, key: *const u8, size: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32;
}

pub const HINI_USER: u32 = 0xffff_ffff;
pub const HINI_SYSTEM: u32 = 0xffff_fffe;

/// The bytes of a NUL-terminated string.
pub fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

pub fn cstr_static(p: *const u8) -> &'static [u8] {
    cstr(p)
}

/// A NUL-terminated copy.
pub fn z(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s);
    v.push(0);
    v
}

/// A string kept in instance data: `None` or NUL-terminated bytes.
pub type Str = Option<Box<[u8]>>;

pub fn str_new(s: &[u8]) -> Str {
    Some(z(s).into_boxed_slice())
}

pub fn str_bytes(s: &Str) -> &[u8] {
    match s {
        Some(b) => &b[..b.len() - 1],
        None => &[],
    }
}

pub fn str_ptr(s: &Str) -> *const u8 {
    match s {
        Some(b) => b.as_ptr(),
        None => b"\0".as_ptr(),
    }
}

pub fn eq_nocase(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

pub fn hex(v: u32) -> Vec<u8> {
    let mut s = Vec::new();
    let mut started = false;
    for i in (0..8).rev() {
        let d = (v >> (i * 4)) & 0xf;
        if d != 0 || started || i == 0 {
            started = true;
            s.push(b"0123456789ABCDEF"[d as usize]);
        }
    }
    s
}

pub fn parse_hex(s: &[u8]) -> Option<u32> {
    if s.is_empty() || s.len() > 8 {
        return None;
    }
    let mut v = 0u32;
    for &c in s {
        v = v << 4 | (c as char).to_digit(16)?;
    }
    Some(v)
}

pub fn parse_dec(s: &[u8]) -> Option<u32> {
    let s = s.trim_ascii();
    if s.is_empty() {
        return None;
    }
    let mut v = 0u32;
    for &c in s {
        v = v.checked_mul(10)?.checked_add((c as char).to_digit(10)?)?;
    }
    Some(v)
}

// ---- Profiles ----

pub fn prf_read(hini: u32, app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let (a, k) = (z(app), z(key));
    let mut n = 0u32;
    if unsafe { PrfQueryProfileSize(hini, a.as_ptr(), k.as_ptr(), &mut n) } == 0 {
        return None;
    }
    let mut buf = alloc::vec![0u8; n as usize];
    let mut got = n;
    if n > 0 && unsafe { PrfQueryProfileData(hini, a.as_ptr(), k.as_ptr(), buf.as_mut_ptr(), &mut got) } == 0 {
        return None;
    }
    buf.truncate(got as usize);
    Some(buf)
}

pub fn prf_write(hini: u32, app: &[u8], key: &[u8], data: Option<&[u8]>) -> bool {
    let (a, k) = (z(app), z(key));
    unsafe {
        match data {
            Some(d) => PrfWriteProfileData(hini, a.as_ptr(), k.as_ptr(), d.as_ptr(), d.len() as u32) != 0,
            None => PrfWriteProfileData(hini, a.as_ptr(), k.as_ptr(), core::ptr::null(), 0) != 0,
        }
    }
}

/// The keys of an application (NUL-separated list from a null key).
pub fn prf_keys(hini: u32, app: &[u8]) -> Vec<Vec<u8>> {
    let a = z(app);
    let mut n = 0u32;
    if unsafe { PrfQueryProfileSize(hini, a.as_ptr(), core::ptr::null(), &mut n) } == 0 || n == 0 {
        return Vec::new();
    }
    let mut buf = alloc::vec![0u8; n as usize];
    let mut got = n;
    if unsafe { PrfQueryProfileData(hini, a.as_ptr(), core::ptr::null(), buf.as_mut_ptr(), &mut got) } == 0 {
        return Vec::new();
    }
    buf.truncate(got as usize);
    buf.split(|&c| c == 0).filter(|k| !k.is_empty()).map(|k| k.to_vec()).collect()
}

// ---- Extended attributes ----

const FIL_STANDARD: u32 = 1;
const FIL_QUERYEASIZE: u32 = 2;
const FIL_QUERYEASFROMLIST: u32 = 3;
pub const EAT_BINARY: u16 = 0xfffe;

/// A file's EA value (type word and data), if it has one.
pub fn ea_read(path: &[u8], name: &[u8]) -> Option<(u16, Vec<u8>)> {
    let gl = (4 + 5 + name.len() + 1 + 3) & !3;
    let mut gea = alloc::vec![0u8; gl];
    gea[0..4].copy_from_slice(&(gl as u32).to_le_bytes());
    gea[8] = name.len() as u8;
    gea[9..9 + name.len()].copy_from_slice(name);
    let cb = 0x10000usize;
    let mut fea = alloc::vec![0u8; cb];
    fea[0..4].copy_from_slice(&(cb as u32).to_le_bytes());
    let eaop: [u32; 3] = [gea.as_ptr() as u32, fea.as_mut_ptr() as u32, 0];
    let p = z(path);
    if unsafe { os2::DosQueryPathInfo(p.as_ptr(), FIL_QUERYEASFROMLIST, eaop.as_ptr() as *mut u8, 12) } != 0 {
        return None;
    }
    let cb_name = fea[9] as usize;
    let cb_value = u16::from_le_bytes([fea[10], fea[11]]) as usize;
    let v = fea.get(12 + cb_name + 1..12 + cb_name + 1 + cb_value)?;
    if v.len() < 4 {
        return None;
    }
    let ty = u16::from_le_bytes([v[0], v[1]]);
    let n = (u16::from_le_bytes([v[2], v[3]]) as usize).min(v.len() - 4);
    Some((ty, v[4..4 + n].to_vec()))
}

/// Write an EA (typed: type word, length word, data); `None` removes it.
pub fn ea_write(path: &[u8], name: &[u8], value: Option<(u16, &[u8])>) -> bool {
    let data_len = value.map_or(0, |(_, d)| d.len());
    if data_len > 0xfff0 {
        return false;
    }
    let value_len = if value.is_some() { 4 + data_len } else { 0 };
    let cb = (4 + 8 + name.len() + 1 + value_len + 3) & !3;
    let mut b = alloc::vec![0u8; cb + 8];
    b[0..4].copy_from_slice(&(cb as u32).to_le_bytes());
    b[9] = name.len() as u8;
    b[10..12].copy_from_slice(&(value_len as u16).to_le_bytes());
    b[12..12 + name.len()].copy_from_slice(name);
    let v = 12 + name.len() + 1;
    if let Some((ty, d)) = value {
        b[v..v + 2].copy_from_slice(&ty.to_le_bytes());
        b[v + 2..v + 4].copy_from_slice(&(d.len() as u16).to_le_bytes());
        b[v + 4..v + 4 + d.len()].copy_from_slice(d);
    }
    let eaop: [u32; 3] = [0, b.as_mut_ptr() as u32, 0];
    let p = z(path);
    unsafe { os2::DosSetPathInfo(p.as_ptr(), FIL_QUERYEASIZE, eaop.as_ptr() as *const u8, 12, 0) == 0 }
}

// ---- Files ----

/// A path's attributes (FILESTATUS3), if it exists.
pub fn attributes(path: &[u8]) -> Option<u32> {
    let p = z(path);
    let mut fs3 = [0u8; 24];
    (unsafe { os2::DosQueryPathInfo(p.as_ptr(), FIL_STANDARD, fs3.as_mut_ptr(), 24) } == 0)
        .then(|| u32::from_le_bytes([fs3[20], fs3[21], fs3[22], fs3[23]]))
}

pub const FILE_DIRECTORY: u32 = 0x10;

pub fn is_dir(path: &[u8]) -> bool {
    path.len() == 3 && path[1] == b':' || attributes(path).is_some_and(|a| a & FILE_DIRECTORY != 0)
}

/// The names in a directory (not "." and ".."), with their attributes.
pub fn list_dir(dir: &[u8]) -> Vec<(Vec<u8>, u32)> {
    let mut spec = dir.to_vec();
    if !spec.ends_with(b"\\") {
        spec.push(b'\\');
    }
    spec.extend_from_slice(b"*");
    let spec = z(&spec);
    let mut out = Vec::new();
    let mut h: u32 = 0xffff_ffff;
    let mut buf = alloc::vec![0u8; 8192];
    let mut count: u32 = 64;
    // FIND_INCLUDE: directories, hidden, system, read-only and archived.
    let mut rc = unsafe { os2::DosFindFirst(spec.as_ptr(), &mut h, 0x37, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    while rc == 0 && count > 0 {
        // FILEFINDBUF3: oNextEntryOffset, dates/times (12), cbFile, cbFileAlloc,
        // attrFile, cchName, achName.
        let mut off = 0usize;
        for _ in 0..count {
            let e = &buf[off..];
            let next = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
            let attr = u32::from_le_bytes([e[24], e[25], e[26], e[27]]);
            let n = e[28] as usize;
            let name = &e[29..29 + n];
            if name != b"." && name != b".." {
                out.push((name.to_vec(), attr));
            }
            if next == 0 {
                break;
            }
            off += next;
        }
        count = 64;
        rc = unsafe { os2::DosFindNext(h, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    }
    unsafe { os2::DosFindClose(h) };
    out
}

pub fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(name);
    p
}

/// The directory part and the name of a path ("C:\A\B" → "C:\A", "B";
/// "C:\A" → "C:\", "A").
pub fn split_path(path: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let path = if path.len() > 3 { path.strip_suffix(b"\\").unwrap_or(path) } else { path };
    match path.iter().rposition(|&c| c == b'\\') {
        Some(i) if i <= 2 => (path[..i + 1].to_vec(), path[i + 1..].to_vec()),
        Some(i) => (path[..i].to_vec(), path[i + 1..].to_vec()),
        None => (Vec::new(), path.to_vec()),
    }
}

/// An absolute path in a standard form: upper-case drive, backslashes,
/// no trailing backslash except at the root.
pub fn normalize(path: &[u8]) -> Vec<u8> {
    let mut p: Vec<u8> = path.iter().map(|&c| if c == b'/' { b'\\' } else { c }).collect();
    if p.len() >= 2 && p[1] == b':' {
        p[0] = p[0].to_ascii_uppercase();
    }
    while p.len() > 3 && p.ends_with(b"\\") {
        p.pop();
    }
    if p.len() == 2 && p[1] == b':' {
        p.push(b'\\');
    }
    p
}

pub fn file_exists(path: &[u8]) -> bool {
    attributes(path).is_some()
}

pub fn create_dir(path: &[u8]) -> bool {
    let p = z(path);
    unsafe { os2::DosCreateDir(p.as_ptr(), core::ptr::null_mut()) == 0 }
}

pub fn create_file(path: &[u8]) -> bool {
    let p = z(path);
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS;
    // OPEN_SHARE_DENYNONE | OPEN_ACCESS_READWRITE.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut action, 0, 0, 0x11, 0x42, core::ptr::null_mut()) } != 0 {
        return false;
    }
    unsafe { os2::DosClose(hf) };
    true
}

pub fn delete_path(path: &[u8]) -> bool {
    let p = z(path);
    if is_dir(path) {
        unsafe { os2::DosDeleteDir(p.as_ptr()) == 0 }
    } else {
        unsafe { os2::DosDelete(p.as_ptr()) == 0 }
    }
}

pub fn rename_path(from: &[u8], to: &[u8]) -> bool {
    let (f, t) = (z(from), z(to));
    unsafe { os2::DosMove(f.as_ptr(), t.as_ptr()) == 0 }
}

/// The extension of a name (upper case, without the dot).
pub fn extension(name: &[u8]) -> Vec<u8> {
    match name.iter().rposition(|&c| c == b'.') {
        Some(i) => name[i + 1..].to_ascii_uppercase(),
        None => Vec::new(),
    }
}
