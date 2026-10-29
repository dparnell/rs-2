//! SHELL's registration database (RegOpenKey, RegCreateKey,
//! RegSetValue, RegQueryValue, RegEnumKey, RegDeleteKey, RegCloseKey):
//! Windows 3.1's one tree under HKEY_CLASSES_ROOT, each key with a
//! string value. It is kept in C:\WINDOWS\REG.DAT as the text REGEDIT
//! merges (`REGEDIT` on the first line, then `HKEY_CLASSES_ROOT\key =
//! value` lines), not Windows' binary file, and read again on each call
//! (every Windows task is a process). `REGEDIT /s FILE.REG`, which setup
//! programs start to register OLE (Delphi's installer: OLE2.REG), is
//! merged here when there is no REGEDIT.EXE (compat.rs).

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::Global;

const ERROR_SUCCESS: u32 = 0;
const ERROR_BADKEY: u32 = 2;
const ERROR_CANTOPEN: u32 = 3;
const ERROR_CANTWRITE: u32 = 5;
const ERROR_INVALID_PARAMETER: u32 = 7;
const HKEY_CLASSES_ROOT: u32 = 1;
const ROOT: &[u8] = b"HKEY_CLASSES_ROOT";
const FILE: &[u8] = b"C:\\WINDOWS\\REG.DAT";

/// Open keys: HKEY = index + 2 (1 is HKEY_CLASSES_ROOT), the key's path.
static OPEN: Global<Vec<Option<Vec<u8>>>> = Global::new(Vec::new());

/// The keys (paths below the root, backslashes between names) and their
/// values.
fn load() -> Vec<(Vec<u8>, Vec<u8>)> {
    let Some(data) = crate::module::read_file(FILE, 4 << 20) else { return Vec::new() };
    let mut v: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for line in data.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some((k, val)) = parse_line(line) {
            set(&mut v, &k, Some(val));
        }
    }
    v
}

/// `HKEY_CLASSES_ROOT\key = value` (or a key alone).
fn parse_line(line: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let rest = strip_prefix_nocase(line, ROOT)?;
    let rest = rest.strip_prefix(b"\\").unwrap_or(rest);
    let (k, val) = match find(rest, b" = ") {
        Some(i) => (&rest[..i], &rest[i + 3..]),
        None => (rest.strip_suffix(b" =").unwrap_or(rest), &b""[..]),
    };
    let k = trim(k);
    (!k.is_empty()).then(|| (k.to_vec(), val.to_vec()))
}

fn strip_prefix_nocase<'a>(s: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    (s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p)).then(|| &s[p.len()..])
}

fn find(s: &[u8], pat: &[u8]) -> Option<usize> {
    s.windows(pat.len()).position(|w| w == pat)
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|&c| c != b' ' && c != b'\t').unwrap_or(s.len());
    let b = s.iter().rposition(|&c| c != b' ' && c != b'\t').map_or(a, |i| i + 1);
    &s[a..b]
}

fn save(v: &[(Vec<u8>, Vec<u8>)]) -> bool {
    crate::profile::make_dirs();
    let mut t = b"REGEDIT\r\n".to_vec();
    for (k, val) in v {
        t.extend_from_slice(&[ROOT, b"\\", &k[..], b" = ", &val[..], b"\r\n"].concat());
    }
    crate::module::write_file(FILE, &t)
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Set (or with None, make) a key, its parents made too.
fn set(v: &mut Vec<(Vec<u8>, Vec<u8>)>, key: &[u8], value: Option<Vec<u8>>) {
    let mut at = 0;
    while let Some(i) = key[at..].iter().position(|&c| c == b'\\') {
        let parent = &key[..at + i];
        if !v.iter().any(|(k, _)| same(k, parent)) {
            v.push((parent.to_vec(), Vec::new()));
        }
        at += i + 1;
    }
    match v.iter_mut().find(|(k, _)| same(k, key)) {
        Some(e) => {
            if let Some(val) = value {
                e.1 = val;
            }
        }
        None => v.push((key.to_vec(), value.unwrap_or_default())),
    }
}

fn exists(v: &[(Vec<u8>, Vec<u8>)], key: &[u8]) -> bool {
    key.is_empty() || v.iter().any(|(k, _)| same(k, key))
}

/// The path of an open key (None: not a key handle).
fn key_path(h: u32) -> Option<Vec<u8>> {
    if h == HKEY_CLASSES_ROOT || h == 0 {
        return Some(Vec::new());
    }
    OPEN.get().get((h as usize).checked_sub(2)?)?.clone()
}

fn join(base: &[u8], sub: &[u8]) -> Vec<u8> {
    let sub = sub.strip_prefix(b"\\").unwrap_or(sub);
    match (base.is_empty(), sub.is_empty()) {
        (_, true) => base.to_vec(),
        (true, false) => sub.to_vec(),
        _ => [base, b"\\", sub].concat(),
    }
}

fn open_handle(path: Vec<u8>) -> u32 {
    let o = OPEN.get();
    let i = o.iter().position(|x| x.is_none()).unwrap_or_else(|| {
        o.push(None);
        o.len() - 1
    });
    o[i] = Some(path);
    i as u32 + 2
}

fn write_u32(p: *mut u8, v: u32) {
    if !p.is_null() {
        unsafe { (p as *mut u32).write_unaligned(v) };
    }
}

/// Merge a .REG file (REGEDIT /s FILE): true if it was one.
pub fn merge(file: &[u8]) -> bool {
    let Some(data) = crate::module::read_file(file, 4 << 20) else { return false };
    if !data.starts_with(b"REGEDIT") {
        return false;
    }
    let mut v = load();
    for line in data.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some((k, val)) = parse_line(line) {
            set(&mut v, &k, Some(val));
        }
    }
    save(&v)
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // RegOpenKey(hkey, subkey, phkey), RegCreateKey(hkey, subkey, phkey)
        6501 | 6502 => {
            let (h, sub, ph) = (a.l(), a.l(), a.p());
            let Some(base) = key_path(h) else { return Some(ERROR_BADKEY) };
            let path = join(&base, crate::far_str(sub).unwrap_or(b""));
            let mut v = load();
            if !exists(&v, &path) {
                if ord == 6501 {
                    return Some(ERROR_BADKEY);
                }
                set(&mut v, &path, None);
                if !save(&v) {
                    return Some(ERROR_CANTWRITE);
                }
            }
            write_u32(ph, if path.is_empty() { HKEY_CLASSES_ROOT } else { open_handle(path) });
            ERROR_SUCCESS
        }
        // RegCloseKey(hkey)
        6503 => {
            let h = a.l();
            if let Some(x) = (h as usize).checked_sub(2).and_then(|i| OPEN.get().get_mut(i)) {
                *x = None;
            }
            ERROR_SUCCESS
        }
        // RegDeleteKey(hkey, subkey): the key and the keys below it.
        6504 => {
            let (h, sub) = (a.l(), a.l());
            let Some(base) = key_path(h) else { return Some(ERROR_BADKEY) };
            let path = join(&base, crate::far_str(sub).unwrap_or(b""));
            let mut v = load();
            if path.is_empty() || !exists(&v, &path) {
                return Some(ERROR_BADKEY);
            }
            let below = [&path[..], b"\\"].concat();
            v.retain(|(k, _)| !same(k, &path) && !(k.len() > below.len() && same(&k[..below.len()], &below)));
            if save(&v) { ERROR_SUCCESS } else { ERROR_CANTWRITE }
        }
        // RegSetValue(hkey, subkey, type, data, cb): REG_SZ only.
        6505 => {
            let (h, sub, ty, data, _cb) = (a.l(), a.l(), a.l(), a.l(), a.l());
            if ty != 1 {
                return Some(ERROR_INVALID_PARAMETER);
            }
            let Some(base) = key_path(h) else { return Some(ERROR_BADKEY) };
            let path = join(&base, crate::far_str(sub).unwrap_or(b""));
            if path.is_empty() {
                return Some(ERROR_INVALID_PARAMETER);
            }
            let mut v = load();
            set(&mut v, &path, Some(crate::far_str(data).unwrap_or(b"").to_vec()));
            if save(&v) { ERROR_SUCCESS } else { ERROR_CANTWRITE }
        }
        // RegQueryValue(hkey, subkey, buf, pcb): the value and its size
        // with the NUL.
        6506 => {
            let (h, sub, buf, pcb) = (a.l(), a.l(), a.p(), a.p());
            let Some(base) = key_path(h) else { return Some(ERROR_BADKEY) };
            let path = join(&base, crate::far_str(sub).unwrap_or(b""));
            let v = load();
            let Some((_, val)) = v.iter().find(|(k, _)| same(k, &path)) else { return Some(ERROR_BADKEY) };
            let max = if pcb.is_null() { 0 } else { unsafe { (pcb as *const i32).read_unaligned() } };
            if !buf.is_null() && max > 0 {
                crate::user::copy_out(val, buf, max);
            }
            write_u32(pcb, val.len() as u32 + 1);
            ERROR_SUCCESS
        }
        // RegEnumKey(hkey, index, buf, cb): the index'th subkey's name.
        6507 => {
            let (h, idx, buf, cb) = (a.l(), a.l(), a.p(), a.l());
            let Some(base) = key_path(h) else { return Some(ERROR_BADKEY) };
            let v = load();
            let prefix = if base.is_empty() { Vec::new() } else { [&base[..], b"\\"].concat() };
            let names: Vec<&[u8]> = v
                .iter()
                .filter_map(|(k, _)| (k.len() > prefix.len() && same(&k[..prefix.len()], &prefix)).then(|| &k[prefix.len()..]))
                .filter(|rest| !rest.contains(&b'\\'))
                .collect();
            let Some(n) = names.get(idx as usize) else { return Some(259) };
            crate::user::copy_out(n, buf, cb as i32);
            ERROR_SUCCESS
        }
        _ => return None,
    })
}
