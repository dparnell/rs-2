//! File helpers shared by the file utilities (COMP, REPLACE, BACKUP,
//! RESTORE, EAUTIL, UNPACK...).

use alloc::string::String;
use alloc::vec::Vec;

use os2::path::cstr;
use os2::*;

/// FILESTATUS3 of a path.
pub fn status(p: &str) -> Option<FileStatus3> {
    let mut fs = FileStatus3::default();
    let c = cstr(p);
    let rc = unsafe { DosQueryPathInfo(c.as_ptr(), FIL_STANDARD, &mut fs as *mut _ as *mut u8, core::mem::size_of::<FileStatus3>() as u32) };
    (rc == 0).then_some(fs)
}

pub fn exists(p: &str) -> bool {
    status(p).is_some()
}

/// Fully qualified name (DosQueryPathInfo level 5).
pub fn full_path(p: &str) -> String {
    let c = cstr(p);
    let mut buf = [0u8; 260];
    if unsafe { DosQueryPathInfo(c.as_ptr(), 5, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
        buf.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()
    } else {
        String::from(p)
    }
}

/// The last component of a path.
pub fn base_name(p: &str) -> &str {
    p.rsplit(['\\', '/', ':']).next().unwrap_or(p)
}

pub fn read_file(p: &str) -> Result<Vec<u8>, u32> {
    let hf = crate::open_read(p)?;
    let d = crate::read_all(hf);
    unsafe { DosClose(hf) };
    Ok(d)
}

/// Create or replace a file with `data`.
pub fn write_file(p: &str, data: &[u8]) -> Result<(), u32> {
    let hf = create(p, 0)?;
    let r = write_all(hf, data);
    unsafe { DosClose(hf) };
    r
}

/// Create (or truncate) a file for writing.
pub fn create(p: &str, attr: u32) -> Result<u32, u32> {
    let c = cstr(p);
    let (mut hf, mut act) = (0, 0);
    let rc = unsafe {
        DosOpen(
            c.as_ptr(),
            &mut hf,
            &mut act,
            0,
            attr,
            OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
            OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYWRITE,
            core::ptr::null_mut(),
        )
    };
    if rc == 0 { Ok(hf) } else { Err(rc) }
}

pub fn write_all(hf: u32, data: &[u8]) -> Result<(), u32> {
    let mut done = 0;
    while done < data.len() {
        let mut n = 0;
        let rc = unsafe { DosWrite(hf, data[done..].as_ptr(), (data.len() - done) as u32, &mut n) };
        if rc != 0 {
            return Err(rc);
        }
        if n == 0 {
            return Err(112);
        }
        done += n as usize;
    }
    Ok(())
}

/// The target name for `name` under the wildcard `pattern` (DosEditName).
pub fn edit_name(name: &str, pattern: &str) -> String {
    let (cs, cp) = (cstr(name), cstr(pattern));
    let mut buf = [0u8; 260];
    if unsafe { DosEditName(1, cs.as_ptr(), cp.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0 {
        buf.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()
    } else {
        String::from(name)
    }
}

/// Join a directory (with or without a trailing separator) and a name.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir.ends_with(['\\', ':', '/']) {
        alloc::format!("{dir}{name}")
    } else {
        alloc::format!("{dir}\\{name}")
    }
}

/// One answer character from the keyboard or redirected input (no echo
/// of redirected input); `None` at the end of redirected input.
pub fn answer() -> Option<char> {
    let mut buf = [0u8; 128];
    let mut n = 0u32;
    if crate::stdin_is_console() {
        unsafe { DosRead(0, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
    } else {
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
        if n == 0 {
            return None;
        }
    }
    Some(buf[..n as usize].iter().map(|&b| (b as char).to_ascii_uppercase()).find(|c| !c.is_whitespace()).unwrap_or('\r'))
}

/// A Y/N question (message `n`) answered without echo of redirected
/// input; the end of redirected input answers no.
pub fn yes_no(n: u32, subs: &[&str], fallback: &str) -> bool {
    loop {
        crate::out(&crate::msgs::text(n, subs, crate::msgs::Class::Info, fallback));
        match answer() {
            Some('Y') => return true,
            Some('N') | None => return false,
            _ => crate::out("\r\n"),
        }
    }
}

/// Wait for Enter (message `n` as the prompt).
pub fn press_enter(n: u32, subs: &[&str], fallback: &str) {
    crate::out(&crate::msgs::text(n, subs, crate::msgs::Class::Info, fallback));
    let _ = answer();
}

/// A directory entry with its dates (FILEFINDBUF3).
#[derive(Clone)]
pub struct Found {
    pub name: String,
    pub attr: u32,
    pub size: u32,
    /// Creation, last access, last write: (packed date, packed time).
    pub created: (u16, u16),
    pub accessed: (u16, u16),
    pub written: (u16, u16),
}

/// Directory entries matching a specification, with their dates.
pub fn find_all(spec: &str, attr: u32) -> Vec<Found> {
    let mut v = Vec::new();
    let c = cstr(spec);
    let mut hdir = HDIR_CREATE;
    let mut buf = alloc::vec![0u8; 16384];
    let mut count = 100;
    let mut rc = unsafe { DosFindFirst(c.as_ptr(), &mut hdir, attr, buf.as_mut_ptr(), buf.len() as u32, &mut count, FIL_STANDARD) };
    if rc != 0 {
        return v;
    }
    let u16at = |e: &[u8], o: usize| u16::from_le_bytes([e[o], e[o + 1]]);
    loop {
        let mut off = 0usize;
        for _ in 0..count {
            let e = &buf[off..];
            let next = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
            let n = e[28] as usize;
            let name: String = e[29..29 + n].iter().map(|&b| b as char).collect();
            if name != "." && name != ".." {
                v.push(Found {
                    name,
                    attr: u32::from_le_bytes([e[24], e[25], e[26], e[27]]),
                    size: u32::from_le_bytes([e[16], e[17], e[18], e[19]]),
                    created: (u16at(e, 4), u16at(e, 6)),
                    accessed: (u16at(e, 8), u16at(e, 10)),
                    written: (u16at(e, 12), u16at(e, 14)),
                });
            }
            if next == 0 {
                break;
            }
            off += next;
        }
        count = 100;
        rc = unsafe { DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
        if rc != 0 {
            break;
        }
    }
    unsafe { DosFindClose(hdir) };
    v
}

/// Files matching `pattern` under `dir` (and in its subdirectories when
/// `sub`), with paths relative to `dir`. With `dirs`, each directory
/// visited is listed too, before its contents.
pub fn walk(dir: &str, pattern: &str, sub: bool, dirs: bool, attr: u32, out: &mut Vec<(String, Found)>) {
    walk_in(dir, "", pattern, sub, dirs, attr, out);
}

fn walk_in(root: &str, rel: &str, pattern: &str, sub: bool, dirs: bool, attr: u32, out: &mut Vec<(String, Found)>) {
    let here = join(root, rel);
    for e in find_all(&join(&here, pattern), attr & !FILE_DIRECTORY) {
        if e.attr & FILE_DIRECTORY == 0 {
            let r = if rel.is_empty() { e.name.clone() } else { alloc::format!("{rel}\\{}", e.name) };
            out.push((r, e));
        }
    }
    if sub {
        for e in find_all(&join(&here, "*"), attr | FILE_DIRECTORY) {
            if e.attr & FILE_DIRECTORY != 0 {
                let r = if rel.is_empty() { e.name.clone() } else { alloc::format!("{rel}\\{}", e.name) };
                if dirs {
                    out.push((r.clone(), e));
                }
                walk_in(root, &r, pattern, sub, dirs, attr, out);
            }
        }
    }
}

/// Create every directory of `path` that does not exist.
pub fn make_dirs(path: &str) -> u32 {
    let mut cur = String::new();
    let mut rc = 0;
    for (i, part) in path.split('\\').enumerate() {
        if i > 0 {
            cur.push('\\');
        }
        cur.push_str(part);
        if part.is_empty() || part.ends_with(':') || exists(&cur) {
            continue;
        }
        let c = cstr(&cur);
        rc = unsafe { DosCreateDir(c.as_ptr(), core::ptr::null_mut()) };
    }
    rc
}

/// Current date and time.
pub fn now() -> DateTime {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    dt
}

/// FAT-style packed date and time of a DateTime.
pub fn packed(dt: &DateTime) -> (u16, u16) {
    let d = ((dt.year.saturating_sub(1980) as u16) << 9) | ((dt.month as u16) << 5) | dt.day as u16;
    let t = ((dt.hours as u16) << 11) | ((dt.minutes as u16) << 5) | (dt.seconds as u16 / 2);
    (d, t)
}

pub use os2_backup::Ea;

/// The extended attributes of a file or directory (DosEnumAttribute for
/// the names, then DosQueryPathInfo level 3 for the values).
pub fn read_eas(path: &str) -> Result<Vec<Ea>, u32> {
    let s = cstr(path);
    let mut dena = alloc::vec![0u8; 65536];
    let mut count = 0xffff_ffffu32;
    let rc = unsafe { DosEnumAttribute(ENUMEA_REFTYPE_PATH, s.as_ptr(), 1, dena.as_mut_ptr(), dena.len() as u32, &mut count, 1) };
    if rc != 0 {
        return Err(rc);
    }
    if count == 0 || count == 0xffff_ffff {
        return Ok(Vec::new());
    }
    let mut gea = alloc::vec![0u8; 4];
    let mut starts = Vec::new();
    let mut need = 4usize;
    let mut at = 0usize;
    for _ in 0..count {
        let next = u32::from_le_bytes([dena[at], dena[at + 1], dena[at + 2], dena[at + 3]]) as usize;
        let cb_name = dena[at + 5] as usize;
        let cb_value = u16::from_le_bytes([dena[at + 6], dena[at + 7]]) as usize;
        while gea.len() % 4 != 0 {
            gea.push(0);
        }
        starts.push(gea.len());
        gea.extend_from_slice(&0u32.to_le_bytes());
        gea.push(cb_name as u8);
        gea.extend_from_slice(&dena[at + 8..at + 8 + cb_name]);
        gea.push(0);
        need += 12 + cb_name + cb_value;
        if next == 0 {
            break;
        }
        at += next;
    }
    for w in starts.windows(2) {
        let off = (w[1] - w[0]) as u32;
        gea[w[0]..w[0] + 4].copy_from_slice(&off.to_le_bytes());
    }
    let n = gea.len() as u32;
    gea[0..4].copy_from_slice(&n.to_le_bytes());
    let mut fea = alloc::vec![0u8; need + 64];
    let cb = fea.len() as u32;
    fea[0..4].copy_from_slice(&cb.to_le_bytes());
    let mut op = EaOp2 { gea2: gea.as_mut_ptr(), fea2: fea.as_mut_ptr(), error: 0 };
    let rc = unsafe { DosQueryPathInfo(s.as_ptr(), 3, &mut op as *mut _ as *mut u8, core::mem::size_of::<EaOp2>() as u32) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(os2_backup::parse_fea2list(&fea))
}

/// Set extended attributes on a file or directory (DosSetPathInfo level
/// 2); nothing to do for none.
pub fn write_eas(path: &str, eas: &[Ea]) -> u32 {
    if eas.is_empty() {
        return 0;
    }
    let s = cstr(path);
    let mut fea = os2_backup::fea2list(eas);
    let op = EaOp2 { gea2: core::ptr::null_mut(), fea2: fea.as_mut_ptr(), error: 0 };
    unsafe { DosSetPathInfo(s.as_ptr(), 2, &op as *const _ as *const u8, core::mem::size_of::<EaOp2>() as u32, 0) }
}

/// Set a file's attributes and last-write date and time (other dates
/// unchanged).
pub fn set_info(path: &str, attr: u32, date: u16, time: u16) -> u32 {
    let Some(mut st) = status(path) else { return 2 };
    st.fdate_creation = 0;
    st.ftime_creation = 0;
    st.fdate_access = 0;
    st.ftime_access = 0;
    st.fdate_write = date;
    st.ftime_write = time;
    st.attr = attr & 0x27;
    let c = cstr(path);
    unsafe { DosSetPathInfo(c.as_ptr(), FIL_STANDARD, &st as *const _ as *const u8, core::mem::size_of::<FileStatus3>() as u32, 0) }
}
