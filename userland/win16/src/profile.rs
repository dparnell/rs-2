//! Windows' initialisation files: WIN.INI (GetProfileString...) and
//! private ones (GetPrivateProfileString...), text files of [sections]
//! and key=value lines, kept in the Windows directory unless the program
//! names a path. Section and key names are compared without case.

use alloc::vec::Vec;

use crate::far_str;

/// The Windows directory (GetWindowsDirectory): C:\WINDOWS, made when
/// first needed, with SYSTEM below it.
pub const WINDOWS_DIR: &[u8] = b"C:\\WINDOWS";
pub const SYSTEM_DIR: &[u8] = b"C:\\WINDOWS\\SYSTEM";

pub fn make_dirs() {
    for d in [WINDOWS_DIR, SYSTEM_DIR] {
        let mut z = d.to_vec();
        z.push(0);
        unsafe { os2::DosCreateDir(z.as_ptr(), core::ptr::null_mut()) };
    }
}

/// A profile's path: in the Windows directory unless it has one.
fn path(file: &[u8]) -> Vec<u8> {
    if file.iter().any(|&c| c == b'\\' || c == b':' || c == b'/') {
        return file.to_vec();
    }
    let mut p = WINDOWS_DIR.to_vec();
    p.push(b'\\');
    p.extend_from_slice(file);
    p
}

/// The file's lines (without line ends); none if it does not exist.
fn read(file: &[u8]) -> Vec<Vec<u8>> {
    let Some(data) = crate::module::read_file(&path(file), 1 << 20) else { return Vec::new() };
    data.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).filter(|l| l.first() != Some(&0x1a)).collect()
}

fn write(file: &[u8], lines: &[Vec<u8>]) -> bool {
    make_dirs();
    let mut z = path(file);
    z.push(0);
    let (mut hf, mut action) = (0u32, 0u32);
    // Replace (or create), write only, deny write.
    if unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut out = Vec::new();
    for l in lines {
        out.extend_from_slice(l);
        out.extend_from_slice(b"\r\n");
    }
    let mut n = 0u32;
    let ok = unsafe { os2::DosWrite(hf, out.as_ptr(), out.len() as u32, &mut n) } == 0;
    unsafe { os2::DosClose(hf) };
    ok
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|&c| c != b' ' && c != b'\t').unwrap_or(s.len());
    let b = s.iter().rposition(|&c| c != b' ' && c != b'\t').map_or(a, |i| i + 1);
    &s[a..b.max(a)]
}

/// A section header's name.
fn section_name(l: &[u8]) -> Option<&[u8]> {
    let t = trim(l);
    if t.first() != Some(&b'[') {
        return None;
    }
    let end = t.iter().position(|&c| c == b']').unwrap_or(t.len());
    Some(trim(&t[1..end]))
}

/// A key line's (key, value).
fn key_value(l: &[u8]) -> Option<(&[u8], &[u8])> {
    let t = trim(l);
    if t.is_empty() || t[0] == b';' || t[0] == b'[' {
        return None;
    }
    let eq = t.iter().position(|&c| c == b'=')?;
    Some((trim(&t[..eq]), trim(&t[eq + 1..])))
}

/// The line range of a section's body (after its header): (header,
/// end).
fn find_section(lines: &[Vec<u8>], app: &[u8]) -> Option<(usize, usize)> {
    let h = lines.iter().position(|l| section_name(l).is_some_and(|n| n.eq_ignore_ascii_case(app)))?;
    let end = lines[h + 1..].iter().position(|l| section_name(l).is_some()).map_or(lines.len(), |i| h + 1 + i);
    Some((h, end))
}

/// A value without surrounding quotes.
fn unquote(v: &[u8]) -> &[u8] {
    if v.len() >= 2 && (v[0] == b'"' && v[v.len() - 1] == b'"' || v[0] == b'\'' && v[v.len() - 1] == b'\'') {
        &v[1..v.len() - 1]
    } else {
        v
    }
}

/// A key's value.
pub fn get(file: &[u8], app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let lines = read(file);
    let (h, end) = find_section(&lines, app)?;
    lines[h + 1..end].iter().filter_map(|l| key_value(l)).find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| unquote(v).to_vec())
}

/// Copy a NUL-separated list (double NUL at the end) to a buffer: the
/// bytes copied but the last NUL (Windows' count, size - 2 when cut).
fn copy_list(items: &[Vec<u8>], buf: *mut u8, n: i32) -> u32 {
    if buf.is_null() || n < 2 {
        return 0;
    }
    let n = n as usize;
    let mut out = Vec::new();
    for i in items {
        out.extend_from_slice(i);
        out.push(0);
    }
    out.push(0);
    if out.len() > n {
        out.truncate(n - 2);
        out.push(0);
        out.push(0);
        unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), buf, n) };
        return (n - 2) as u32;
    }
    unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), buf, out.len()) };
    (out.len() - 1) as u32
}

/// GetProfileString / GetPrivateProfileString: no section name lists the
/// sections, no key the section's keys.
pub fn get_string(file: &[u8], app: u32, key: u32, def: u32, buf: *mut u8, n: i32) -> u32 {
    let lines = read(file);
    let Some(app) = far_str(app) else {
        let names: Vec<Vec<u8>> = lines.iter().filter_map(|l| section_name(l)).map(|s| s.to_vec()).collect();
        return copy_list(&names, buf, n);
    };
    let sec = find_section(&lines, app);
    let Some(key) = far_str(key) else {
        let keys: Vec<Vec<u8>> = sec.map_or(Vec::new(), |(h, e)| lines[h + 1..e].iter().filter_map(|l| key_value(l)).map(|(k, _)| k.to_vec()).collect());
        return copy_list(&keys, buf, n);
    };
    let v = sec.and_then(|(h, e)| lines[h + 1..e].iter().filter_map(|l| key_value(l)).find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| unquote(v).to_vec()));
    let v = v.unwrap_or_else(|| trim(far_str(def).unwrap_or(b"")).to_vec());
    crate::user::copy_out(&v, buf, n) as u32
}

/// GetProfileInt / GetPrivateProfileInt: the value's leading digits (0
/// if it has none), or the default.
pub fn get_int(file: &[u8], app: u32, key: u32, def: u16) -> u32 {
    let (Some(app), Some(key)) = (far_str(app), far_str(key)) else { return def as u32 };
    let Some(v) = get(file, app, key) else { return def as u32 };
    let (neg, d) = match v.first() {
        Some(b'-') => (true, &v[1..]),
        _ => (false, &v[..]),
    };
    let n = d.iter().take_while(|c| c.is_ascii_digit()).fold(0u32, |a, &c| a.wrapping_mul(10).wrapping_add((c - b'0') as u32));
    (if neg { n.wrapping_neg() } else { n }) & 0xffff
}

/// WriteProfileString / WritePrivateProfileString: no key deletes the
/// section, no value the key.
pub fn write_string(file: &[u8], app: u32, key: u32, value: u32) -> u32 {
    let Some(app) = far_str(app) else { return 1 };
    let mut lines = read(file);
    let sec = find_section(&lines, app);
    let Some(key) = far_str(key) else {
        if let Some((h, e)) = sec {
            lines.drain(h..e);
        }
        return write(file, &lines) as u32;
    };
    let value = far_str(value);
    match sec {
        Some((h, e)) => {
            let at = (h + 1..e).find(|&i| key_value(&lines[i]).is_some_and(|(k, _)| k.eq_ignore_ascii_case(key)));
            match (at, value) {
                (Some(i), Some(v)) => lines[i] = [key, b"=", v].concat(),
                (Some(i), None) => {
                    lines.remove(i);
                }
                (None, Some(v)) => {
                    // After the section's last non-empty line.
                    let mut i = e;
                    while i > h + 1 && trim(&lines[i - 1]).is_empty() {
                        i -= 1;
                    }
                    lines.insert(i, [key, b"=", v].concat());
                }
                (None, None) => return 1,
            }
        }
        None => {
            let Some(v) = value else { return 1 };
            if lines.last().is_some_and(|l| !trim(l).is_empty()) {
                lines.push(Vec::new());
            }
            lines.push([b"[", app, b"]"].concat());
            lines.push([key, b"=", v].concat());
        }
    }
    write(file, &lines) as u32
}
