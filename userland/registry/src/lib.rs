//! REGISTRY.DLL: the OS/2 registry, the Win32 registry calls (RegOpenKeyExA
//! and friends) that Open32 (PMWINX.DLL forwards its Reg* calls here) and
//! the Registry Editor (REGEDIT2.EXE) use. IBM's REGISTRY keeps its keys in
//! \OS2\SYSTEM\SYSTEM.DAT and USER.DAT, binary files it maps into shared
//! memory; rs2 keeps them in two profiles (OS/2 INI files, through
//! PMSHAPI, which shares them between processes and writes them through
//! safely): \OS2\SYSTEM\REGSYS.INI for HKEY_LOCAL_MACHINE and REGUSER.INI
//! for HKEY_USERS.
//!
//! A key is a profile application named by its path from the root
//! ("HKLM\SOFTWARE\IBM", upper case: names are case-insensitive) with a
//! key "K" holding the name as it was given; a value is a key "V" + its
//! name in upper case holding the type (ULONG), the length of the name as
//! given (USHORT), that name and the data. The predefined keys are IBM's
//! (Open32's os2win.h): HKEY_CLASSES_ROOT is HKEY_LOCAL_MACHINE\SOFTWARE\
//! Classes, HKEY_CURRENT_USER is HKEY_USERS\.default, HKEY_CURRENT_CONFIG
//! is HKEY_LOCAL_MACHINE\CONFIG\0001, as in IBM's REGISTRY; the Win32
//! values of the handles (0x80000000 on) are taken too. HKEY_DYN_DATA's
//! keys are kept in the system file under DYN. Keys are deleted with their
//! subkeys (as in Windows 95). Data are bytes in the process code page
//! (the W calls are not there).

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;

mod exports;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

unsafe extern "C" {
    #[link_name = "PMSHAPI.102"]
    fn PrfOpenProfile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMSHAPI.101"]
    fn PrfQueryProfileSize(hini: u32, app: *const u8, key: *const u8, size: *mut u32) -> u32;
    #[link_name = "PMSHAPI.117"]
    fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32;
}

const ERROR_SUCCESS: u32 = 0;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_NO_MORE_ITEMS: u32 = 259;
const ERROR_BADKEY: u32 = 1010;
const ERROR_CANTOPEN: u32 = 1011;

const REG_SZ: u32 = 1;
const REG_CREATED_NEW_KEY: u32 = 1;
const REG_OPENED_EXISTING_KEY: u32 = 2;

/// The two files.
#[derive(Clone, Copy, PartialEq)]
enum Hive {
    System,
    User,
}

/// An open key: its file and path (upper case, from the root: "HKLM",
/// "HKLM\SOFTWARE").
#[derive(Clone)]
struct Key {
    hive: Hive,
    path: Vec<u8>,
}

/// Open keys of this process (the DLL's data are per process): handle
/// = index + KEY_BASE.
static mut OPEN: Vec<Option<Key>> = Vec::new();
static mut FILES: [u32; 2] = [0, 0];
const KEY_BASE: u32 = 0x5245_0000;

fn open_keys() -> &'static mut Vec<Option<Key>> {
    unsafe { &mut *core::ptr::addr_of_mut!(OPEN) }
}

/// The profile of a hive, opened on first use.
fn file(h: Hive) -> u32 {
    let i = h as usize;
    let files = unsafe { &mut *core::ptr::addr_of_mut!(FILES) };
    if files[i] == 0 {
        let mut drive = 3u32;
        // QSV_BOOT_DRIVE.
        unsafe { os2::DosQuerySysInfo(5, 5, &mut drive, 4) };
        let mut name = Vec::new();
        name.push(b'@' + drive as u8);
        name.extend_from_slice(if h == Hive::System { b":\\OS2\\SYSTEM\\REGSYS.INI\0" } else { b":\\OS2\\SYSTEM\\REGUSER.INI\0" });
        files[i] = unsafe { PrfOpenProfile(0, name.as_ptr()) };
        // The keys other predefined keys stand for (IBM's KRegInit makes
        // them too).
        if files[i] != 0 {
            match h {
                Hive::System => {
                    create(h, b"HKLM", b"SOFTWARE\\Classes");
                    create(h, b"HKLM", b"CONFIG\\0001");
                }
                Hive::User => {
                    create(h, b"HKU", b".default");
                }
            }
        }
    }
    files[i]
}

fn upper(s: &[u8]) -> Vec<u8> {
    s.iter().map(|c| c.to_ascii_uppercase()).collect()
}

fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// The key a handle stands for: the predefined keys (Open32's and
/// Win32's handles) and the open ones.
fn key_of(h: u32) -> Option<Key> {
    let k = |hive, p: &[u8]| Some(Key { hive, path: p.to_vec() });
    match h {
        0xffff_ffec | 0x8000_0000 => k(Hive::System, b"HKLM\\SOFTWARE\\CLASSES"),
        0xffff_ffed | 0x8000_0003 => k(Hive::User, b"HKU"),
        0xffff_ffee | 0x8000_0001 => k(Hive::User, b"HKU\\.DEFAULT"),
        0xffff_ffef | 0x8000_0002 => k(Hive::System, b"HKLM"),
        0xffff_fff0 | 0x8000_0005 => k(Hive::System, b"HKLM\\CONFIG\\0001"),
        0xffff_fff1 | 0x8000_0006 => k(Hive::System, b"DYN"),
        _ => {
            let i = h.checked_sub(KEY_BASE)? as usize;
            open_keys().get(i)?.clone()
        }
    }
}

fn predefined(h: u32) -> bool {
    (0xffff_ffec..=0xffff_fffb).contains(&h) || (0x8000_0000..=0x8000_0006).contains(&h)
}

/// The paths of the subkeys a relative path names ("a\b", "\a", "").
fn join(base: &[u8], sub: &[u8]) -> Vec<u8> {
    let mut p = base.to_vec();
    for part in sub.split(|&c| c == b'\\').filter(|s| !s.is_empty()) {
        p.push(b'\\');
        p.extend(upper(part));
    }
    p
}

/// Every key name (profile application) in a hive.
fn apps(h: Hive) -> Vec<Vec<u8>> {
    names(file(h), core::ptr::null())
}

/// The NUL-separated list of applications (app NULL) or keys of one.
fn names(hini: u32, app: *const u8) -> Vec<Vec<u8>> {
    let mut cb = 0;
    if hini == 0 || unsafe { PrfQueryProfileSize(hini, app, core::ptr::null(), &mut cb) } == 0 || cb == 0 {
        return Vec::new();
    }
    let mut buf = alloc::vec![0u8; cb as usize];
    let mut n = cb;
    unsafe { PrfQueryProfileData(hini, app, core::ptr::null(), buf.as_mut_ptr(), &mut n) };
    buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(|s| s.to_vec()).collect()
}

fn get(hini: u32, app: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let (a, k) = (z(app), z(key));
    let mut cb = 0;
    if unsafe { PrfQueryProfileSize(hini, a.as_ptr(), k.as_ptr(), &mut cb) } == 0 {
        return None;
    }
    let mut buf = alloc::vec![0u8; cb as usize];
    let mut n = cb;
    if cb > 0 && unsafe { PrfQueryProfileData(hini, a.as_ptr(), k.as_ptr(), buf.as_mut_ptr(), &mut n) } == 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(buf)
}

fn put(hini: u32, app: &[u8], key: Option<&[u8]>, data: Option<&[u8]>) -> bool {
    let a = z(app);
    let k = key.map(z);
    let kp = k.as_ref().map_or(core::ptr::null(), |k| k.as_ptr());
    let (dp, n) = data.map_or((core::ptr::null(), 0), |d| (if d.is_empty() { b"\0".as_ptr() } else { d.as_ptr() }, d.len() as u32));
    unsafe { PrfWriteProfileData(hini, a.as_ptr(), kp, dp, n) != 0 }
}

/// A root key ("HKLM", "HKU", "DYN") always exists.
fn exists(k: &Key) -> bool {
    !k.path.contains(&b'\\') || get(file(k.hive), &k.path, b"K").is_some()
}

/// Create the key and the keys above it; `names` holds the names as
/// given for the components after `base`.
fn create(hive: Hive, base: &[u8], sub: &[u8]) -> bool {
    let hini = file(hive);
    let mut p = base.to_vec();
    for part in sub.split(|&c| c == b'\\').filter(|s| !s.is_empty()) {
        p.push(b'\\');
        p.extend(upper(part));
        if get(hini, &p, b"K").is_none() && !put(hini, &p, Some(b"K"), Some(part)) {
            return false;
        }
    }
    true
}

/// The subkeys of a key: (path, name as given), sorted by name.
fn children(k: &Key) -> Vec<(Vec<u8>, Vec<u8>)> {
    let hini = file(k.hive);
    let mut prefix = k.path.clone();
    prefix.push(b'\\');
    let mut v: Vec<(Vec<u8>, Vec<u8>)> = apps(k.hive)
        .into_iter()
        .filter(|a| a.starts_with(&prefix) && !a[prefix.len()..].contains(&b'\\') && a.len() > prefix.len())
        .map(|a| {
            let name = get(hini, &a, b"K").unwrap_or_else(|| a[prefix.len()..].to_vec());
            (a, name)
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// A value: (name as given, type, data).
fn decode(raw: &[u8]) -> Option<(Vec<u8>, u32, Vec<u8>)> {
    if raw.len() < 6 {
        return None;
    }
    let ty = u32::from_le_bytes(raw[0..4].try_into().ok()?);
    let n = u16::from_le_bytes(raw[4..6].try_into().ok()?) as usize;
    let name = raw.get(6..6 + n)?.to_vec();
    Some((name, ty, raw[6 + n..].to_vec()))
}

fn encode(name: &[u8], ty: u32, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(6 + name.len() + data.len());
    v.extend_from_slice(&ty.to_le_bytes());
    v.extend_from_slice(&(name.len() as u16).to_le_bytes());
    v.extend_from_slice(name);
    v.extend_from_slice(data);
    v
}

/// The values of a key, in the order they were made.
fn values(k: &Key) -> Vec<(Vec<u8>, u32, Vec<u8>)> {
    let hini = file(k.hive);
    let a = z(&k.path);
    names(hini, a.as_ptr()).into_iter().filter(|n| n.first() == Some(&b'V')).filter_map(|n| get(hini, &k.path, &n).and_then(|raw| decode(&raw))).collect()
}

fn value_key(name: &[u8]) -> Vec<u8> {
    let mut v = alloc::vec![b'V'];
    v.extend(upper(name));
    v
}

fn new_handle(k: Key) -> u32 {
    let keys = open_keys();
    let i = match keys.iter().position(|e| e.is_none()) {
        Some(i) => {
            keys[i] = Some(k);
            i
        }
        None => {
            keys.push(Some(k));
            keys.len() - 1
        }
    };
    KEY_BASE + i as u32
}

/// Copy `src` (and a NUL when `nul`) into a caller's buffer of *pcb
/// bytes; *pcb becomes the size needed.
fn copy_out(src: &[u8], nul: bool, buf: *mut u8, pcb: *mut u32) -> u32 {
    let need = src.len() + nul as usize;
    if pcb.is_null() {
        return if buf.is_null() { ERROR_SUCCESS } else { ERROR_INVALID_PARAMETER };
    }
    let have = unsafe { *pcb } as usize;
    unsafe { *pcb = need as u32 };
    if buf.is_null() {
        return ERROR_SUCCESS;
    }
    if have < need {
        return ERROR_MORE_DATA;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), buf, src.len());
        if nul {
            *buf.add(src.len()) = 0;
        }
    }
    ERROR_SUCCESS
}

/// The string data a value of type REG_SZ, REG_EXPAND_SZ or REG_MULTI_SZ
/// is stored with: its NUL is part of the data.
fn is_string(ty: u32) -> bool {
    matches!(ty, 1 | 2 | 7)
}

// ---- The calls ----

/// RegOpenKeyExA(hKey, lpSubKey, ulOptions, samDesired, phkResult).
#[unsafe(no_mangle)]
pub extern "C" fn RegOpenKeyExA(h: u32, sub: *const u8, _opt: u32, _sam: u32, out: *mut u32) -> u32 {
    if out.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let Some(base) = key_of(h) else { return ERROR_BADKEY };
    if file(base.hive) == 0 {
        return ERROR_CANTOPEN;
    }
    let k = Key { hive: base.hive, path: join(&base.path, &cstr(sub)) };
    if !exists(&k) {
        return ERROR_FILE_NOT_FOUND;
    }
    unsafe { *out = new_handle(k) };
    ERROR_SUCCESS
}

/// RegOpenKeyA(hKey, lpSubKey, phkResult).
#[unsafe(no_mangle)]
pub extern "C" fn RegOpenKeyA(h: u32, sub: *const u8, out: *mut u32) -> u32 {
    RegOpenKeyExA(h, sub, 0, 0, out)
}

/// RegCreateKeyExA(hKey, lpSubKey, Reserved, lpClass, dwOptions,
/// samDesired, lpSecurityAttributes, phkResult, lpdwDisposition).
#[unsafe(no_mangle)]
pub extern "C" fn RegCreateKeyExA(h: u32, sub: *const u8, _r: u32, _class: *const u8, _opt: u32, _sam: u32, _sec: u32, out: *mut u32, disp: *mut u32) -> u32 {
    if out.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let Some(base) = key_of(h) else { return ERROR_BADKEY };
    if file(base.hive) == 0 {
        return ERROR_CANTOPEN;
    }
    if !exists(&base) {
        return ERROR_BADKEY;
    }
    let sub = cstr(sub);
    let k = Key { hive: base.hive, path: join(&base.path, &sub) };
    let was = exists(&k);
    if !was && !create(base.hive, &base.path, &sub) {
        return ERROR_ACCESS_DENIED;
    }
    if !disp.is_null() {
        unsafe { *disp = if was { REG_OPENED_EXISTING_KEY } else { REG_CREATED_NEW_KEY } };
    }
    unsafe { *out = new_handle(k) };
    ERROR_SUCCESS
}

/// RegCreateKeyA(hKey, lpSubKey, phkResult).
#[unsafe(no_mangle)]
pub extern "C" fn RegCreateKeyA(h: u32, sub: *const u8, out: *mut u32) -> u32 {
    RegCreateKeyExA(h, sub, 0, core::ptr::null(), 0, 0, 0, out, core::ptr::null_mut())
}

/// RegCloseKey(hKey).
#[unsafe(no_mangle)]
pub extern "C" fn RegCloseKey(h: u32) -> u32 {
    if predefined(h) {
        return ERROR_SUCCESS;
    }
    let Some(i) = h.checked_sub(KEY_BASE) else { return ERROR_BADKEY };
    match open_keys().get_mut(i as usize) {
        Some(e @ Some(_)) => {
            *e = None;
            ERROR_SUCCESS
        }
        _ => ERROR_BADKEY,
    }
}

/// RegFlushKey(hKey): the profiles are written through already.
#[unsafe(no_mangle)]
pub extern "C" fn RegFlushKey(h: u32) -> u32 {
    if key_of(h).is_some() { ERROR_SUCCESS } else { ERROR_BADKEY }
}

/// RegDeleteKeyA(hKey, lpSubKey): with its subkeys and values. A
/// predefined key itself cannot be deleted.
#[unsafe(no_mangle)]
pub extern "C" fn RegDeleteKeyA(h: u32, sub: *const u8) -> u32 {
    let Some(base) = key_of(h) else { return ERROR_BADKEY };
    let sub = cstr(sub);
    if sub.iter().all(|&c| c == b'\\') && predefined(h) {
        return ERROR_INVALID_PARAMETER;
    }
    let k = Key { hive: base.hive, path: join(&base.path, &sub) };
    if !k.path.contains(&b'\\') {
        return ERROR_ACCESS_DENIED;
    }
    if !exists(&k) {
        return ERROR_FILE_NOT_FOUND;
    }
    let hini = file(k.hive);
    let mut prefix = k.path.clone();
    prefix.push(b'\\');
    for a in apps(k.hive) {
        if a == k.path || a.starts_with(&prefix) {
            put(hini, &a, None, None);
        }
    }
    ERROR_SUCCESS
}

/// RegDeleteValueA(hKey, lpValueName).
#[unsafe(no_mangle)]
pub extern "C" fn RegDeleteValueA(h: u32, name: *const u8) -> u32 {
    let Some(k) = key_of(h) else { return ERROR_BADKEY };
    let vk = value_key(&cstr(name));
    let hini = file(k.hive);
    if get(hini, &k.path, &vk).is_none() {
        return ERROR_FILE_NOT_FOUND;
    }
    if put(hini, &k.path, Some(&vk), None) { ERROR_SUCCESS } else { ERROR_ACCESS_DENIED }
}

/// RegEnumKeyExA(hKey, dwIndex, lpName, lpcName, lpReserved, lpClass,
/// lpcClass, lpftLastWriteTime): *lpcName counts characters without the
/// NUL on return.
#[unsafe(no_mangle)]
pub extern "C" fn RegEnumKeyExA(h: u32, i: u32, name: *mut u8, pcch: *mut u32, _r: u32, class: *mut u8, pcclass: *mut u32, ft: *mut u32) -> u32 {
    let Some(k) = key_of(h) else { return ERROR_BADKEY };
    if name.is_null() || pcch.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let kids = children(&k);
    let Some((_, n)) = kids.get(i as usize) else { return ERROR_NO_MORE_ITEMS };
    let mut cb = unsafe { *pcch };
    let r = copy_out(n, true, name, &mut cb);
    if r != ERROR_SUCCESS {
        return r;
    }
    unsafe { *pcch = n.len() as u32 };
    if !class.is_null() && !pcclass.is_null() && unsafe { *pcclass } > 0 {
        unsafe {
            *class = 0;
            *pcclass = 0;
        }
    }
    if !ft.is_null() {
        unsafe {
            *ft = 0;
            *ft.add(1) = 0;
        }
    }
    ERROR_SUCCESS
}

/// RegEnumKeyA(hKey, dwIndex, lpName, cchName).
#[unsafe(no_mangle)]
pub extern "C" fn RegEnumKeyA(h: u32, i: u32, name: *mut u8, cch: u32) -> u32 {
    let mut n = cch;
    RegEnumKeyExA(h, i, name, &mut n, 0, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut())
}

/// RegEnumValueA(hKey, dwIndex, lpValueName, lpcValueName, lpReserved,
/// lpType, lpData, lpcbData).
#[unsafe(no_mangle)]
pub extern "C" fn RegEnumValueA(h: u32, i: u32, name: *mut u8, pcch: *mut u32, _r: u32, ty: *mut u32, data: *mut u8, pcb: *mut u32) -> u32 {
    let Some(k) = key_of(h) else { return ERROR_BADKEY };
    if name.is_null() || pcch.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let vals = values(&k);
    let Some((n, t, d)) = vals.get(i as usize) else { return ERROR_NO_MORE_ITEMS };
    let mut cb = unsafe { *pcch };
    let r = copy_out(n, true, name, &mut cb);
    if r != ERROR_SUCCESS {
        return r;
    }
    unsafe { *pcch = n.len() as u32 };
    if !ty.is_null() {
        unsafe { *ty = *t };
    }
    if !pcb.is_null() {
        return copy_out(d, false, data, pcb);
    }
    ERROR_SUCCESS
}

/// RegQueryValueExA(hKey, lpValueName, lpReserved, lpType, lpData,
/// lpcbData): a missing unnamed value reads as an empty REG_SZ.
#[unsafe(no_mangle)]
pub extern "C" fn RegQueryValueExA(h: u32, name: *const u8, _r: u32, ty: *mut u32, data: *mut u8, pcb: *mut u32) -> u32 {
    let Some(k) = key_of(h) else { return ERROR_BADKEY };
    if pcb.is_null() && !data.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let name = cstr(name);
    let raw = get(file(k.hive), &k.path, &value_key(&name));
    let (t, d) = match raw.as_deref().and_then(decode) {
        Some((_, t, d)) => (t, d),
        None if name.is_empty() => (REG_SZ, alloc::vec![0]),
        None => return ERROR_FILE_NOT_FOUND,
    };
    if !ty.is_null() {
        unsafe { *ty = t };
    }
    copy_out(&d, false, data, pcb)
}

/// RegQueryValueA(hKey, lpSubKey, lpValue, lpcbValue): the unnamed value
/// of a subkey, a string.
#[unsafe(no_mangle)]
pub extern "C" fn RegQueryValueA(h: u32, sub: *const u8, data: *mut u8, pcb: *mut u32) -> u32 {
    let mut k = 0;
    let r = RegOpenKeyExA(h, sub, 0, 0, &mut k);
    if r != ERROR_SUCCESS {
        return r;
    }
    let r = RegQueryValueExA(k, b"\0".as_ptr(), 0, core::ptr::null_mut(), data, pcb);
    RegCloseKey(k);
    r
}

/// RegSetValueExA(hKey, lpValueName, Reserved, dwType, lpData, cbData):
/// a string's data are taken up to its NUL when cbData leaves it out.
#[unsafe(no_mangle)]
pub extern "C" fn RegSetValueExA(h: u32, name: *const u8, _r: u32, ty: u32, data: *const u8, cb: u32) -> u32 {
    let Some(k) = key_of(h) else { return ERROR_BADKEY };
    if !exists(&k) {
        return ERROR_BADKEY;
    }
    let mut d = if data.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(data, cb as usize) }.to_vec() };
    if is_string(ty) && d.last() != Some(&0) {
        d.push(0);
    }
    let name = cstr(name);
    let hini = file(k.hive);
    // The key of a root's values.
    if get(hini, &k.path, b"K").is_none() && !put(hini, &k.path, Some(b"K"), Some(&k.path)) {
        return ERROR_ACCESS_DENIED;
    }
    if put(hini, &k.path, Some(&value_key(&name)), Some(&encode(&name, ty, &d))) { ERROR_SUCCESS } else { ERROR_ACCESS_DENIED }
}

/// RegSetValueA(hKey, lpSubKey, dwType, lpData, cbData): the unnamed
/// value of a subkey (made if need be), a REG_SZ.
#[unsafe(no_mangle)]
pub extern "C" fn RegSetValueA(h: u32, sub: *const u8, ty: u32, data: *const u8, _cb: u32) -> u32 {
    if ty != REG_SZ || data.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let mut k = 0;
    let r = RegCreateKeyA(h, sub, &mut k);
    if r != ERROR_SUCCESS {
        return r;
    }
    let s = cstr(data);
    let r = RegSetValueExA(k, b"\0".as_ptr(), 0, REG_SZ, s.as_ptr(), s.len() as u32);
    RegCloseKey(k);
    r
}

/// RegQueryInfoKeyA(hKey, lpClass, lpcClass, lpReserved, lpcSubKeys,
/// lpcMaxSubKeyLen, lpcMaxClassLen, lpcValues, lpcMaxValueNameLen,
/// lpcMaxValueLen, lpcbSecurityDescriptor, lpftLastWriteTime).
#[unsafe(no_mangle)]
pub extern "C" fn RegQueryInfoKeyA(
    h: u32,
    class: *mut u8,
    pcclass: *mut u32,
    _r: u32,
    subkeys: *mut u32,
    max_sub: *mut u32,
    max_class: *mut u32,
    nvalues: *mut u32,
    max_name: *mut u32,
    max_data: *mut u32,
    sec: *mut u32,
    ft: *mut u32,
) -> u32 {
    let Some(k) = key_of(h) else { return ERROR_BADKEY };
    let kids = children(&k);
    let vals = values(&k);
    let set = |p: *mut u32, v: usize| {
        if !p.is_null() {
            unsafe { *p = v as u32 };
        }
    };
    if !class.is_null() && !pcclass.is_null() && unsafe { *pcclass } > 0 {
        unsafe { *class = 0 };
    }
    set(pcclass, 0);
    set(subkeys, kids.len());
    set(max_sub, kids.iter().map(|k| k.1.len()).max().unwrap_or(0));
    set(max_class, 0);
    set(nvalues, vals.len());
    set(max_name, vals.iter().map(|v| v.0.len()).max().unwrap_or(0));
    set(max_data, vals.iter().map(|v| v.2.len()).max().unwrap_or(0));
    set(sec, 0);
    if !ft.is_null() {
        unsafe {
            *ft = 0;
            *ft.add(1) = 0;
        }
    }
    ERROR_SUCCESS
}

/// KRegInit/KRegTerm (IBM's REGISTRY server and converter call them):
/// nothing to set up.
#[unsafe(no_mangle)]
pub extern "C" fn KRegInit() -> u32 {
    ERROR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn KRegTerm() -> u32 {
    ERROR_SUCCESS
}
