//! The OS/2 CD and the target drive: finding the CD, unpacking files from
//! its packed files (`os2-pack`, as OS/2's UNPACK does), and writing them
//! with their dates, attributes and extended attributes.

use alloc::vec;
use alloc::vec::Vec;
use os2::dos::*;

/// A file the CD names for a target, as stored there.
pub struct Unpacked {
    pub data: Vec<u8>,
    pub date: u16,
    pub time: u16,
    pub attributes: u32,
    /// Extended attributes (a FEA2LIST), if it has any.
    pub eas: Option<Vec<u8>>,
}

pub fn z(t: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(t.len() + 1);
    v.extend_from_slice(t);
    v.push(0);
    v
}

/// `X:` and a path from the root.
pub fn on(drive: u8, path: &[u8]) -> Vec<u8> {
    let mut v = vec![drive, b':'];
    if !path.starts_with(b"\\") {
        v.push(b'\\');
    }
    v.extend_from_slice(path);
    v
}

/// The drives there are (bit 0: A:).
pub fn drives() -> u32 {
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { DosQueryCurrentDisk(&mut cur, &mut map) };
    map
}

/// The drive the system was started from.
pub fn boot_drive() -> u8 {
    let mut d = 3u32;
    // QSV_BOOT_DRIVE.
    unsafe { DosQuerySysInfo(5, 5, &mut d, 4) };
    b'A' + (d.clamp(1, 26) - 1) as u8
}

/// Whether file or directory `path` exists.
pub fn exists(path: &[u8]) -> bool {
    let p = z(path);
    let mut buf = [0u8; 64];
    unsafe { DosQueryPathInfo(p.as_ptr(), 1, buf.as_mut_ptr(), 24) == 0 }
}

/// The first drive holding the OS/2 Warp CD (its `OS2IMAGE` directory).
pub fn find_cd() -> Option<u8> {
    let map = drives();
    (2..26u8).filter(|d| map & (1 << d) != 0).map(|d| b'A' + d).find(|&d| exists(&on(d, b"\\OS2IMAGE\\DISK_4\\BUNDLE")))
}

pub fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut action) = (0u32, 0u32);
    // Open an existing file, read-only, deny write.
    if unsafe { DosOpen(p.as_ptr(), &mut hf, &mut action, 0, 0, 0x0001, 0x0020, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut size = 0u32;
    unsafe {
        DosSetFilePtr(hf, 0, 2, &mut size);
        DosSetFilePtr(hf, 0, 0, &mut 0);
    }
    let mut buf = vec![0u8; size as usize];
    let mut got = 0u32;
    let rc = unsafe { DosRead(hf, buf.as_mut_ptr(), size, &mut got) };
    unsafe { DosClose(hf) };
    (rc == 0 && got == size).then_some(buf)
}

/// The last packed file read, kept for the next files from it.
pub struct Cd {
    pub drive: u8,
    name: Vec<u8>,
    data: Option<Vec<u8>>,
    dec: os2_pack::Decompressor,
}

impl Cd {
    pub fn new(drive: u8) -> Self {
        Cd { drive, name: Vec::new(), data: None, dec: os2_pack::Decompressor::new() }
    }

    /// File `target` from CD file `source` (a packed file holding it, or
    /// the file itself).
    pub fn get(&mut self, target: &[u8], source: &[u8]) -> Result<Unpacked, &'static str> {
        if self.name != source {
            self.data = read_file(&on(self.drive, source));
            self.name = source.to_vec();
        }
        let Some(data) = &self.data else { return Err("is not on the CD") };
        if !os2_pack::is_packed(data) {
            return Ok(Unpacked { data: data.clone(), date: 0, time: 0, attributes: 0x20, eas: None });
        }
        let entries = os2_pack::entries(data).map_err(|_| "cannot be read from the CD")?;
        let e = entries.iter().find(|e| e.path.eq_ignore_ascii_case(target)).ok_or("is not in its file on the CD")?;
        let out = e.decompress(&mut self.dec).map_err(|_| "cannot be unpacked")?;
        let eas = if e.ea_size > 4 { e.decompress_ea(&mut self.dec).ok().flatten() } else { None };
        Ok(Unpacked { data: out, date: e.date, time: e.time, attributes: e.attributes, eas })
    }
}

/// Make the directories along `path` (a file's full name).
fn make_dirs(path: &[u8]) {
    let mut i = 3;
    while let Some(n) = path[i..].iter().position(|&c| c == b'\\') {
        let d = z(&path[..i + n]);
        unsafe { DosCreateDir(d.as_ptr(), core::ptr::null_mut()) };
        i += n + 1;
    }
}

/// Write file `path` (replacing it) with `f`'s contents, date, time,
/// attributes and extended attributes.
pub fn write(path: &[u8], f: &Unpacked) -> Result<(), u32> {
    make_dirs(path);
    let p = z(path);
    // A read-only copy from earlier is replaced too.
    let mut st = [0u8; 24];
    if unsafe { DosQueryPathInfo(p.as_ptr(), 1, st.as_mut_ptr(), 24) } == 0 && st[20] & 1 != 0 {
        st[20] &= !1;
        unsafe { DosSetPathInfo(p.as_ptr(), 1, st.as_ptr(), 24, 0) };
    }
    let (mut hf, mut action) = (0u32, 0u32);
    // Create or replace; write-only, deny write.
    let rc = unsafe { DosOpen(p.as_ptr(), &mut hf, &mut action, f.data.len() as u32, 0, 0x0012, 0x0021, core::ptr::null_mut()) };
    if rc != 0 {
        return Err(rc);
    }
    let mut done = 0u32;
    let rc = unsafe { DosWrite(hf, f.data.as_ptr(), f.data.len() as u32, &mut done) };
    unsafe { DosClose(hf) };
    if rc != 0 || done as usize != f.data.len() {
        return Err(if rc != 0 { rc } else { 112 });
    }
    if let Some(eas) = &f.eas {
        // EAOP2: no GEA2LIST, the FEA2LIST, oError.
        let op: [u32; 3] = [0, eas.as_ptr() as u32, 0];
        unsafe { DosSetPathInfo(p.as_ptr(), 2, op.as_ptr() as *const u8, 12, 0) };
    }
    if f.date != 0 {
        // FILESTATUS3: creation, access and write dates and times (0:
        // unchanged), sizes, attributes.
        let mut st = [0u8; 24];
        st[8..10].copy_from_slice(&f.date.to_le_bytes());
        st[10..12].copy_from_slice(&f.time.to_le_bytes());
        st[20..24].copy_from_slice(&(f.attributes & 0x27).to_le_bytes());
        unsafe { DosSetPathInfo(p.as_ptr(), 1, st.as_ptr(), 24, 0) };
    }
    Ok(())
}

/// Whether drive `d` can be written (a test file is made and deleted).
pub fn writable(d: u8) -> bool {
    let path = on(d, b"\\APPINST.TMP");
    let f = Unpacked { data: b"rs2".to_vec(), date: 0, time: 0, attributes: 0, eas: None };
    let ok = write(&path, &f).is_ok();
    let p = z(&path);
    unsafe { DosDelete(p.as_ptr()) };
    ok
}

/// Add `dir` to CONFIG.SYS's `SET PATH=` and `LIBPATH=` lines of drive
/// `d` when they do not have it. Returns whether CONFIG.SYS changed.
pub fn add_to_paths(d: u8, dir: &[u8]) -> bool {
    let path = on(d, b"\\CONFIG.SYS");
    let Some(text) = read_file(&path) else { return false };
    let mut out: Vec<u8> = Vec::with_capacity(text.len() + 64);
    let mut changed = false;
    let mut lines = text.split(|&c| c == b'\n').peekable();
    while let Some(line) = lines.next() {
        let body = line.strip_suffix(b"\r").unwrap_or(line);
        let upper: Vec<u8> = body.iter().map(|c| c.to_ascii_uppercase()).collect();
        let is_path = upper.starts_with(b"SET PATH=") || upper.starts_with(b"LIBPATH=");
        let has = upper[upper.iter().position(|&c| c == b'=').map_or(0, |i| i + 1)..]
            .split(|&c| c == b';')
            .any(|e| e.eq_ignore_ascii_case(dir));
        out.extend_from_slice(body);
        if is_path && !has {
            if !body.ends_with(b";") {
                out.push(b';');
            }
            out.extend_from_slice(dir);
            changed = true;
        }
        if lines.peek().is_some() {
            if line.ends_with(b"\r") {
                out.push(b'\r');
            }
            out.push(b'\n');
        }
    }
    if changed {
        let f = Unpacked { data: out, date: 0, time: 0, attributes: 0, eas: None };
        return write(&path, &f).is_ok();
    }
    false
}

/// Set `key=value` in section `[section]` of the Windows-style .INI file
/// `path` (keys and sections compare case-insensitively; a missing key
/// goes at the end of its section, a missing section at the end of the
/// file). False when the file cannot be read or written.
pub fn set_ini(path: &[u8], section: &[u8], key: &[u8], value: &[u8]) -> bool {
    let Some(text) = read_file(path) else { return false };
    let mut lines: Vec<Vec<u8>> = text.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut entry = key.to_vec();
    entry.push(b'=');
    entry.extend_from_slice(value);
    let header = |l: &[u8]| l.first() == Some(&b'[');
    let start = lines.iter().position(|l| l.len() > section.len() + 1 && header(l) && l[1..=section.len()].eq_ignore_ascii_case(section) && l[section.len() + 1] == b']');
    match start {
        None => {
            let mut h = b"[".to_vec();
            h.extend_from_slice(section);
            h.push(b']');
            lines.push(Vec::new());
            lines.push(h);
            lines.push(entry);
        }
        Some(s) => {
            let end = lines[s + 1..].iter().position(|l| header(l)).map_or(lines.len(), |i| s + 1 + i);
            let found = lines[s + 1..end].iter().position(|l| {
                let k = l.split(|&c| c == b'=').next().unwrap_or(b"");
                l.contains(&b'=') && k.trim_ascii().eq_ignore_ascii_case(key)
            });
            match found {
                Some(i) => lines[s + 1 + i] = entry,
                None => {
                    // After the section's last non-empty line.
                    let mut at = end;
                    while at > s + 1 && lines[at - 1].is_empty() {
                        at -= 1;
                    }
                    lines.insert(at, entry);
                }
            }
        }
    }
    let mut out = Vec::with_capacity(text.len() + 64);
    for l in &lines {
        out.extend_from_slice(l);
        out.extend_from_slice(b"\r\n");
    }
    let f = Unpacked { data: out, date: 0, time: 0, attributes: 0, eas: None };
    write(path, &f).is_ok()
}
