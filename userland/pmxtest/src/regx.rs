//! The rest of the registry: REGISTRY's W calls (no Open32 forwarder
//! has them: imported from REGISTRY.DLL itself), PMWINX's RegFlushKey,
//! and keys that map their values (an OS/2 profile, the locale, the print
//! queues) as IBM's REGISTRY does.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use crate::check;

unsafe extern "C" {
    #[link_name = "PMWINX.1030"]
    fn RegFlushKey(h: u32) -> u32;
    #[link_name = "PMWINX.541"]
    fn RegCloseKey(h: u32) -> u32;
    #[link_name = "PMWINX.542"]
    fn RegCreateKey(h: u32, sub: *const u8, out: *mut u32) -> u32;
    #[link_name = "PMWINX.544"]
    fn RegDeleteKey(h: u32, sub: *const u8) -> u32;
    #[link_name = "PMWINX.545"]
    fn RegDeleteValue(h: u32, name: *const u8) -> u32;
    #[link_name = "PMWINX.547"]
    fn RegEnumKeyEx(h: u32, i: u32, name: *mut u8, pcch: *mut u32, r: u32, class: *mut u8, pcclass: *mut u32, ft: *mut u32) -> u32;
    #[link_name = "PMWINX.548"]
    fn RegEnumValue(h: u32, i: u32, name: *mut u8, pcch: *mut u32, r: u32, ty: *mut u32, data: *mut u8, pcb: *mut u32) -> u32;
    #[link_name = "PMWINX.550"]
    fn RegOpenKeyEx(h: u32, sub: *const u8, opt: u32, sam: u32, out: *mut u32) -> u32;
    #[link_name = "PMWINX.551"]
    fn RegQueryInfoKey(h: u32, class: *mut u8, pcclass: *mut u32, r: u32, subkeys: *mut u32, max_sub: *mut u32, max_class: *mut u32, values: *mut u32, max_name: *mut u32, max_data: *mut u32, sec: *mut u32, ft: *mut u32) -> u32;
    #[link_name = "PMWINX.553"]
    fn RegQueryValueEx(h: u32, name: *const u8, r: u32, ty: *mut u32, data: *mut u8, pcb: *mut u32) -> u32;
    #[link_name = "PMWINX.555"]
    fn RegSetValueEx(h: u32, name: *const u8, r: u32, ty: u32, data: *const u8, cb: u32) -> u32;

    #[link_name = "REGISTRY.5"]
    fn RegCreateKeyExW(h: u32, sub: *const u16, r: u32, class: *const u16, opt: u32, sam: u32, sec: u32, out: *mut u32, disp: *mut u32) -> u32;
    #[link_name = "REGISTRY.8"]
    fn RegDeleteKeyW(h: u32, sub: *const u16) -> u32;
    #[link_name = "REGISTRY.10"]
    fn RegDeleteValueW(h: u32, name: *const u16) -> u32;
    #[link_name = "REGISTRY.13"]
    fn RegEnumKeyExW(h: u32, i: u32, name: *mut u16, pcch: *mut u32, r: u32, class: *mut u16, pcclass: *mut u32, ft: *mut u32) -> u32;
    #[link_name = "REGISTRY.14"]
    fn RegEnumKeyW(h: u32, i: u32, name: *mut u16, cch: u32) -> u32;
    #[link_name = "REGISTRY.16"]
    fn RegEnumValueW(h: u32, i: u32, name: *mut u16, pcch: *mut u32, r: u32, ty: *mut u32, data: *mut u8, pcb: *mut u32) -> u32;
    #[link_name = "REGISTRY.24"]
    fn RegOpenKeyExW(h: u32, sub: *const u16, opt: u32, sam: u32, out: *mut u32) -> u32;
    #[link_name = "REGISTRY.25"]
    fn RegOpenKeyW(h: u32, sub: *const u16, out: *mut u32) -> u32;
    #[link_name = "REGISTRY.27"]
    fn RegQueryInfoKeyW(h: u32, class: *mut u16, pcclass: *mut u32, r: u32, subkeys: *mut u32, max_sub: *mut u32, max_class: *mut u32, values: *mut u32, max_name: *mut u32, max_data: *mut u32, sec: *mut u32, ft: *mut u32) -> u32;
    #[link_name = "REGISTRY.32"]
    fn RegQueryValueExW(h: u32, name: *const u16, r: u32, ty: *mut u32, data: *mut u8, pcb: *mut u32) -> u32;
    #[link_name = "REGISTRY.33"]
    fn RegQueryValueW(h: u32, sub: *const u16, data: *mut u16, pcb: *mut u32) -> u32;
    #[link_name = "REGISTRY.44"]
    fn RegSetValueExW(h: u32, name: *const u16, r: u32, ty: u32, data: *const u8, cb: u32) -> u32;
    #[link_name = "REGISTRY.45"]
    fn RegSetValueW(h: u32, sub: *const u16, ty: u32, data: *const u16, cb: u32) -> u32;

    #[link_name = "PMSHAPI.102"]
    fn PrfOpenProfile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMSHAPI.103"]
    fn PrfCloseProfile(hini: u32) -> u32;
    #[link_name = "PMSHAPI.118"]
    fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32;
}

const HKEY_LOCAL_MACHINE: u32 = 0xffff_ffef;
const HKEY_CURRENT_USER: u32 = 0xffff_ffee;
const REG_SZ: u32 = 1;
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_INVALID_NAME: u32 = 123;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_NO_MORE_ITEMS: u32 = 259;

/// A NUL-terminated Unicode string from ASCII (and Latin-1) text.
fn w(s: &str) -> Vec<u16> {
    s.chars().map(|c| c as u16).chain([0]).collect()
}

fn wbytes(s: &str) -> Vec<u8> {
    w(s).iter().flat_map(|c| c.to_le_bytes()).collect()
}

/// A value read through the A call: (rc, type, data).
fn query(h: u32, name: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut ty = 0;
    let mut buf = [0u8; 256];
    let mut cb = buf.len() as u32;
    let rc = unsafe { RegQueryValueEx(h, name.as_ptr(), 0, &mut ty, buf.as_mut_ptr(), &mut cb) };
    (rc, ty, if rc == 0 { buf[..cb as usize].to_vec() } else { Vec::new() })
}

/// The values' names as RegEnumValue lists them, up to the first error
/// (returned too).
fn value_names(h: u32) -> (Vec<Vec<u8>>, u32) {
    let mut v = Vec::new();
    for i in 0.. {
        let mut n = [0u8; 64];
        let mut cn = n.len() as u32;
        let mut cd = 0;
        let rc = unsafe { RegEnumValue(h, i, n.as_mut_ptr(), &mut cn, 0, null_mut(), null_mut(), &mut cd) };
        if rc != 0 {
            return (v, rc);
        }
        v.push(n[..cn as usize].to_vec());
    }
    (v, 0)
}

pub fn regx_checks() {
    wide_checks();
    map_checks();
    uls_checks();
    spooler_checks();
}

fn wide_checks() {
    unsafe {
        let base = w("SOFTWARE\\PmxWide");
        RegDeleteKeyW(HKEY_LOCAL_MACHINE, base.as_ptr());
        // A name with a character outside ASCII: é is 0x82 in the code
        // page (437 and 850 alike).
        let mut k = 0;
        let mut disp = 0;
        let rc = RegCreateKeyExW(HKEY_LOCAL_MACHINE, w("SOFTWARE\\PmxWide\\Caf\u{e9}").as_ptr(), 0, null(), 0, 0, 0, &mut k, &mut disp);
        check("RegCreateKeyExW makes a key (REG_CREATED_NEW_KEY)", rc == 0 && disp == 1);
        let mut a = 0;
        let rc = RegOpenKeyEx(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxWide\\Caf\x82\0".as_ptr(), 0, 0, &mut a);
        check("its name is in the code page for the A calls", rc == 0);
        RegCloseKey(a);

        // Strings converted both ways.
        let text = wbytes("wide text");
        let dw = 7u32.to_le_bytes();
        let ok = RegSetValueExW(k, w("Wide").as_ptr(), 0, REG_SZ, text.as_ptr(), text.len() as u32) == 0
            && RegSetValueEx(k, b"Narrow\0".as_ptr(), 0, REG_SZ, b"narrow\0".as_ptr(), 7) == 0
            && RegSetValueExW(k, w("Number").as_ptr(), 0, REG_DWORD, dw.as_ptr(), 4) == 0;
        check("RegSetValueExW: a string and a DWORD", ok);
        let (rc, ty, d) = query(k, b"WIDE\0");
        check("a W string read by RegQueryValueEx", rc == 0 && ty == REG_SZ && d == b"wide text\0");
        let mut ty = 0;
        let mut cb = 0;
        let rc = RegQueryValueExW(k, w("Narrow").as_ptr(), 0, &mut ty, null_mut(), &mut cb);
        check("RegQueryValueExW: the size of an A string in Unicode", rc == 0 && ty == REG_SZ && cb == 14);
        let mut buf = [0u8; 64];
        let mut cb = 6;
        let rc = RegQueryValueExW(k, w("Narrow").as_ptr(), 0, &mut ty, buf.as_mut_ptr(), &mut cb);
        check("RegQueryValueExW: ERROR_MORE_DATA and the size needed", rc == ERROR_MORE_DATA && cb == 14);
        let mut cb = buf.len() as u32;
        let rc = RegQueryValueExW(k, w("Narrow").as_ptr(), 0, &mut ty, buf.as_mut_ptr(), &mut cb);
        check("RegQueryValueExW: an A string", rc == 0 && buf[..cb as usize] == wbytes("narrow")[..]);
        let mut cb = buf.len() as u32;
        let rc = RegQueryValueExW(k, w("Number").as_ptr(), 0, &mut ty, buf.as_mut_ptr(), &mut cb);
        check("RegQueryValueExW: a DWORD as it is", rc == 0 && ty == REG_DWORD && cb == 4 && buf[..4] == dw);
        let rc1 = RegSetValueExW(k, w("Bad").as_ptr(), 1, REG_SZ, text.as_ptr(), text.len() as u32);
        let rc2 = RegSetValueExW(k, w("Bad").as_ptr(), 0, REG_BINARY, text.as_ptr(), 0x7fd9);
        let rc3 = RegSetValueExW(k, w("Bad").as_ptr(), 0, REG_SZ, null(), 0);
        check("RegSetValueExW: Reserved, too much data, a string without data", rc1 == ERROR_INVALID_PARAMETER && rc2 == ERROR_INVALID_PARAMETER && rc3 == ERROR_INVALID_PARAMETER);

        // Enumerations count characters.
        let mut names = Vec::new();
        for i in 0.. {
            let mut n = [0u16; 32];
            let mut cn = n.len() as u32;
            let mut cd = 0;
            if RegEnumValueW(k, i, n.as_mut_ptr(), &mut cn, 0, null_mut(), null_mut(), &mut cd) != 0 {
                break;
            }
            names.push((n[..cn as usize].to_vec(), cd));
        }
        let want = [(w("Wide"), 20), (w("Narrow"), 14), (w("Number"), 4)];
        let ok = names.len() == 3 && names.iter().zip(want.iter()).all(|(a, b)| a.0[..] == b.0[..b.0.len() - 1] && a.1 == b.1);
        check("RegEnumValueW: names and Unicode sizes", ok);
        let (mut nval, mut maxn, mut maxd) = (0, 0, 0);
        let rc = RegQueryInfoKeyW(k, null_mut(), null_mut(), 0, null_mut(), null_mut(), null_mut(), &mut nval, &mut maxn, &mut maxd, null_mut(), null_mut());
        check("RegQueryInfoKeyW: the longest name and data in Unicode", rc == 0 && nval == 3 && maxn == 6 && maxd == 20);
        let mut p = 0;
        RegOpenKeyW(HKEY_LOCAL_MACHINE, base.as_ptr(), &mut p);
        let mut n = [0u16; 16];
        let mut cn = n.len() as u32;
        let rc = RegEnumKeyExW(p, 0, n.as_mut_ptr(), &mut cn, 0, null_mut(), null_mut(), null_mut());
        check("RegEnumKeyExW: the name and its length in characters", rc == 0 && cn == 4 && n[..5] == w("Caf\u{e9}")[..]);
        let mut cn = 4;
        let rc = RegEnumKeyExW(p, 0, n.as_mut_ptr(), &mut cn, 0, null_mut(), null_mut(), null_mut());
        check("RegEnumKeyExW: ERROR_MORE_DATA without room for the NUL", rc == ERROR_MORE_DATA);
        check("RegEnumKeyW: past the end", RegEnumKeyW(p, 1, n.as_mut_ptr(), 16) == ERROR_NO_MORE_ITEMS);

        // The unnamed value of a subkey.
        let s = w("default");
        let rc = RegSetValueW(p, w("Sub").as_ptr(), REG_SZ, s.as_ptr(), 14);
        let mut v = [0u16; 16];
        let mut cb = 32;
        let rc2 = RegQueryValueW(p, w("Sub").as_ptr(), v.as_mut_ptr(), &mut cb);
        check("RegSetValueW/RegQueryValueW: a subkey's unnamed value", rc == 0 && rc2 == 0 && cb == 16 && v[..8] == s[..]);

        check("RegDeleteValueW", RegDeleteValueW(k, w("Wide").as_ptr()) == 0 && RegDeleteValueW(k, w("Wide").as_ptr()) == ERROR_FILE_NOT_FOUND);
        check("RegFlushKey (PMWINX 1030)", RegFlushKey(k) == 0);
        RegCloseKey(k);
        RegCloseKey(p);
        check("RegDeleteKeyW with its subkeys", RegDeleteKeyW(HKEY_LOCAL_MACHINE, base.as_ptr()) == 0);
        let mut g = 0;
        check("deleted W keys are gone", RegOpenKeyExW(HKEY_LOCAL_MACHINE, w("SOFTWARE\\PmxWide\\Caf\u{e9}").as_ptr(), 0, 0, &mut g) == ERROR_FILE_NOT_FOUND);
    }
}

fn map_checks() {
    unsafe {
        // What IBM's REGCONV makes: Windows' fonts from OS2.INI.
        let mut f = 0;
        let rc = RegOpenKeyEx(HKEY_LOCAL_MACHINE, b"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Fonts\0".as_ptr(), 0, 0, &mut f);
        let (_, ty, m) = query(f, b"OS2_INI_ACCESS_METHOD\0");
        let (_, _, a) = query(f, b"OS2_INI_APP_NAME\0");
        check("the Fonts key maps OS2.INI's PM_Fonts (profile_sz)", rc == 0 && ty == REG_SZ && m == b"profile_sz\0" && a == b"PM_Fonts\0");
        RegCloseKey(f);

        // A profile of our own.
        let mut drive = 3u32;
        os2::DosQuerySysInfo(5, 5, &mut drive, 4);
        let mut path = alloc::vec![b'@' + drive as u8];
        path.extend_from_slice(b":\\PMXMAP.INI\0");
        let hini = PrfOpenProfile(0, path.as_ptr());
        PrfWriteProfileData(hini, b"PmxApp\0".as_ptr(), null(), null(), 0);
        PrfWriteProfileData(hini, b"PmxApp\0".as_ptr(), b"One\0".as_ptr(), b"first\0".as_ptr(), 6);
        PrfWriteProfileData(hini, b"PmxApp\0".as_ptr(), b"Two\0".as_ptr(), [1u8, 2, 3].as_ptr(), 3);
        PrfCloseProfile(hini);

        RegDeleteKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxMap\0".as_ptr());
        let mut k = 0;
        RegCreateKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxMap\0".as_ptr(), &mut k);
        let set = |name: &[u8], v: &[u8]| RegSetValueEx(k, name.as_ptr(), 0, REG_SZ, v.as_ptr(), v.len() as u32);
        let ok = set(b"OS2_INI_FILE_NAME\0", &path) == 0 && set(b"os2_ini_app_name\0", b"PmxApp\0") == 0;
        check("OS2_INI_FILE_NAME and OS2_INI_APP_NAME set", ok);
        check("without an access method: ERROR_GEN_FAILURE", query(k, b"One\0").0 == 31);
        check("an unknown access method: ERROR_INVALID_NAME", set(b"OS2_INI_ACCESS_METHOD\0", b"files\0") == ERROR_INVALID_NAME);
        check("access method profile", set(b"OS2_INI_ACCESS_METHOD\0", b"Profile\0") == 0);
        let (rc, ty, d) = query(k, b"One\0");
        check("profile: a key of the application as REG_BINARY", rc == 0 && ty == REG_BINARY && d == b"first\0");
        let (rc, ty, d) = query(k, b"OS2_INI_APP_NAME\0");
        check("the key's own OS2_INI_* values read as stored", rc == 0 && ty == REG_SZ && d == b"PmxApp\0");
        check("profile: a key not in the profile is ERROR_INVALID_NAME", query(k, b"Three\0").0 == ERROR_INVALID_NAME);
        let (names, end) = value_names(k);
        check("profile: RegEnumValue lists the application's keys", names == [b"One".to_vec(), b"Two".to_vec()] && end == ERROR_NO_MORE_ITEMS);
        let (mut nval, mut maxn, mut maxd) = (0, 0, 0);
        let rc = RegQueryInfoKey(k, null_mut(), null_mut(), 0, null_mut(), null_mut(), null_mut(), &mut nval, &mut maxn, &mut maxd, null_mut(), null_mut());
        check("profile: RegQueryInfoKey counts the application's keys", rc == 0 && nval == 2 && maxn == 3 && maxd == 6);
        check("profile_sz", set(b"OS2_INI_ACCESS_METHOD\0", b"profile_sz\0") == 0);
        let (rc, ty, d) = query(k, b"Two\0");
        check("profile_sz: a key as REG_SZ (with a NUL)", rc == 0 && ty == REG_SZ && d == [1, 2, 3, 0]);
        let dw = 1u32.to_le_bytes();
        check("a mapping key takes no other values", RegSetValueEx(k, b"Other\0".as_ptr(), 0, REG_DWORD, dw.as_ptr(), 4) == ERROR_INVALID_NAME);
        check("nor are mapped values deleted", RegDeleteValue(k, b"One\0".as_ptr()) == ERROR_INVALID_NAME);
        // Without the application value the key reads nothing.
        check("the mapping values are deleted", RegDeleteValue(k, b"OS2_INI_APP_NAME\0".as_ptr()) == 0);
        check("without OS2_INI_APP_NAME: ERROR_FILE_NOT_FOUND", query(k, b"One\0").0 == ERROR_FILE_NOT_FOUND);
        let ok = RegDeleteValue(k, b"OS2_INI_FILE_NAME\0".as_ptr()) == 0 && RegDeleteValue(k, b"OS2_INI_ACCESS_METHOD\0".as_ptr()) == 0;
        check("without its OS2_INI_* values a key is an ordinary one", ok && RegSetValueEx(k, b"Other\0".as_ptr(), 0, REG_DWORD, dw.as_ptr(), 4) == 0);
        check("OS2_INI_FILE_NAME on a key with other values: ERROR_INVALID_NAME", set(b"OS2_INI_FILE_NAME\0", &path) == ERROR_INVALID_NAME);
        RegCloseKey(k);
        RegDeleteKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxMap\0".as_ptr());
        os2::DosDelete(path.as_ptr());
    }
}

fn uls_checks() {
    unsafe {
        let mut k = 0;
        let rc = RegOpenKeyEx(HKEY_CURRENT_USER, b"Control Panel\\International\0".as_ptr(), 0, 0, &mut k);
        check("HKEY_CURRENT_USER\\Control Panel\\International is there", rc == 0);
        let (rc, ty, d) = query(k, b"sShortDate\0");
        check("uls: sShortDate is the locale's date format", rc == 0 && ty == REG_SZ && d.len() > 2 && d.contains(&b'%'));
        let (rc, _, d) = query(k, b"ICOUNTRY\0");
        check("uls: names without case (iCountry)", rc == 0 && d.len() > 1 && d.last() == Some(&0));
        check("uls: an unknown item is ERROR_FILE_NOT_FOUND", query(k, b"sNothing\0").0 == ERROR_FILE_NOT_FOUND);
        let (names, end) = value_names(k);
        check("uls: RegEnumValue lists the 125 items", names.len() == 125 && names[0] == b"sDateTime" && names[124] == b"wLongDate" && end == ERROR_NO_MORE_ITEMS);
        let mut nval = 0;
        RegQueryInfoKey(k, null_mut(), null_mut(), 0, null_mut(), null_mut(), null_mut(), &mut nval, null_mut(), null_mut(), null_mut(), null_mut());
        check("uls: RegQueryInfoKey counts them", nval == 125);
        RegCloseKey(k);
    }
}

fn spooler_checks() {
    unsafe {
        RegDeleteKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxPrinters\0".as_ptr());
        let mut k = 0;
        RegCreateKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxPrinters\0".as_ptr(), &mut k);
        let rc = RegSetValueEx(k, b"OS2_INI_ACCESS_METHOD\0".as_ptr(), 0, REG_SZ, b"spooler_key\0".as_ptr(), 12);
        check("access method spooler_key", rc == 0);
        RegCloseKey(k);
        RegOpenKeyEx(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxPrinters\0".as_ptr(), 0, 0, &mut k);
        let mut nsub = 0;
        RegQueryInfoKey(k, null_mut(), null_mut(), 0, &mut nsub, null_mut(), null_mut(), null_mut(), null_mut(), null_mut(), null_mut(), null_mut());
        let mut ok = true;
        for i in 0..nsub {
            let mut n = [0u8; 64];
            let mut cn = n.len() as u32;
            RegEnumKeyEx(k, i, n.as_mut_ptr(), &mut cn, 0, null_mut(), null_mut(), null_mut());
            let mut q = 0;
            n[cn as usize] = 0;
            RegOpenKeyEx(k, n.as_ptr(), 0, 0, &mut q);
            let (rc, ty, d) = query(q, b"Name\0");
            let (names, _) = value_names(q);
            ok &= rc == 0 && ty == REG_SZ && d[..d.len() - 1] == n[..cn as usize] && names.len() == 23;
            let (_, pty, port) = query(q, b"Port\0");
            let (_, tty, _) = query(q, b"Time Out\0");
            ok &= pty == REG_SZ && tty == REG_DWORD;
            let s = |b: &[u8]| alloc::string::String::from_utf8_lossy(b).into_owned();
            os2::println!("  queue of {}: {} values, port {}", s(&n[..cn as usize]), names.len(), s(&port));
            RegCloseKey(q);
        }
        check("spooler_key: a subkey per queue, its 23 values (Name: the printer driver's device)", ok);
        let (rc, _, d) = query(k, b"Default\0");
        let (own, _) = value_names(k);
        let mut dq = 0;
        check("spooler_key: Default names a queue's key; only it is listed", nsub == 0 || (rc == 0 && own == [b"Default".to_vec()] && RegOpenKeyEx(k, d.as_ptr(), 0, 0, &mut dq) == 0));
        RegCloseKey(dq);
        RegCloseKey(k);
        RegDeleteKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxPrinters\0".as_ptr());
    }
}
