//! The registry through Open32's calls (PMWINX, forwarded to REGISTRY),
//! as the Registry Editor uses them.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use crate::check;

unsafe extern "C" {
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
}

const HKEY_LOCAL_MACHINE: u32 = 0xffff_ffef;
const HKEY_CURRENT_USER: u32 = 0xffff_ffee;
const HKEY_USERS: u32 = 0xffff_ffed;
const HKEY_CLASSES_ROOT: u32 = 0xffff_ffec;
const REG_SZ: u32 = 1;
const REG_DWORD: u32 = 4;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_NO_MORE_ITEMS: u32 = 259;

fn subkeys(h: u32) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    for i in 0.. {
        let mut buf = [0u8; 128];
        let mut n = buf.len() as u32;
        if unsafe { RegEnumKeyEx(h, i, buf.as_mut_ptr(), &mut n, 0, null_mut(), null_mut(), null_mut()) } != 0 {
            break;
        }
        v.push(buf[..n as usize].to_vec());
    }
    v
}

pub fn reg_checks() {
    unsafe {
        // A fresh start (an earlier run may have left the key).
        RegDeleteKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxTest\0".as_ptr());
        let mut k = 0;
        let rc = RegCreateKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxTest\\Sub One\0".as_ptr(), &mut k);
        check("RegCreateKey makes a key and the keys above it", rc == 0 && k != 0);
        let s = b"a string\0";
        let dw = 0x1234_5678u32.to_le_bytes();
        let ok = RegSetValueEx(k, b"Text\0".as_ptr(), 0, REG_SZ, s.as_ptr(), s.len() as u32) == 0
            && RegSetValueEx(k, b"Number\0".as_ptr(), 0, REG_DWORD, dw.as_ptr(), 4) == 0
            && RegSetValueEx(k, b"\0".as_ptr(), 0, REG_SZ, b"default\0".as_ptr(), 8) == 0;
        check("RegSetValueEx: a string, a DWORD and the unnamed value", ok);
        RegCloseKey(k);

        // Names are case-insensitive, kept as given.
        let mut k2 = 0;
        let rc = RegOpenKeyEx(HKEY_LOCAL_MACHINE, b"software\\PMXTEST\\sub one\0".as_ptr(), 0, 0, &mut k2);
        check("RegOpenKeyEx with other case", rc == 0);
        let mut ty = 0;
        let mut buf = [0u8; 32];
        let mut cb = buf.len() as u32;
        let rc = RegQueryValueEx(k2, b"TEXT\0".as_ptr(), 0, &mut ty, buf.as_mut_ptr(), &mut cb);
        check("RegQueryValueEx: a string (with its NUL)", rc == 0 && ty == REG_SZ && &buf[..cb as usize] == s);
        let mut small = [0u8; 4];
        let mut cb = small.len() as u32;
        let rc = RegQueryValueEx(k2, b"Text\0".as_ptr(), 0, &mut ty, small.as_mut_ptr(), &mut cb);
        check("RegQueryValueEx: ERROR_MORE_DATA and the size needed", rc == ERROR_MORE_DATA && cb == 9);
        let mut v = 0u32;
        let mut cb = 4;
        let rc = RegQueryValueEx(k2, b"Number\0".as_ptr(), 0, &mut ty, &mut v as *mut u32 as *mut u8, &mut cb);
        check("RegQueryValueEx: a DWORD", rc == 0 && ty == REG_DWORD && v == 0x1234_5678);
        let mut cb = 0;
        let rc = RegQueryValueEx(k2, b"Missing\0".as_ptr(), 0, &mut ty, null_mut(), &mut cb);
        check("RegQueryValueEx: a missing value", rc == ERROR_FILE_NOT_FOUND);

        // Values and keys enumerated.
        let mut names = Vec::new();
        for i in 0.. {
            let mut n = [0u8; 32];
            let mut cn = n.len() as u32;
            let mut d = [0u8; 32];
            let mut cd = d.len() as u32;
            if RegEnumValue(k2, i, n.as_mut_ptr(), &mut cn, 0, &mut ty, d.as_mut_ptr(), &mut cd) != 0 {
                break;
            }
            names.push(n[..cn as usize].to_vec());
        }
        check("RegEnumValue lists the values as named", names == [b"Text".to_vec(), b"Number".to_vec(), Vec::new()]);
        let (mut nsub, mut nval, mut maxn, mut maxd) = (9, 0, 0, 0);
        let rc = RegQueryInfoKey(k2, null_mut(), null_mut(), 0, &mut nsub, null_mut(), null_mut(), &mut nval, &mut maxn, &mut maxd, null_mut(), null_mut());
        check("RegQueryInfoKey", rc == 0 && nsub == 0 && nval == 3 && maxn == 6 && maxd == 9);
        check("RegDeleteValue", RegDeleteValue(k2, b"Number\0".as_ptr()) == 0 && RegDeleteValue(k2, b"Number\0".as_ptr()) == ERROR_FILE_NOT_FOUND);
        RegCloseKey(k2);
        let mut k3 = 0;
        RegCreateKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxTest\\Another\0".as_ptr(), &mut k3);
        RegCloseKey(k3);
        let mut p = 0;
        RegOpenKeyEx(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxTest\0".as_ptr(), 0, 0, &mut p);
        check("RegEnumKeyEx lists the subkeys as named", subkeys(p) == [b"Another".to_vec(), b"Sub One".to_vec()]);
        let mut n = [0u8; 4];
        let mut cn = n.len() as u32;
        check("RegEnumKeyEx: past the end", RegEnumKeyEx(p, 2, n.as_mut_ptr(), &mut cn, 0, null_mut(), null_mut(), null_mut()) == ERROR_NO_MORE_ITEMS);
        RegCloseKey(p);
        check("SOFTWARE is under HKEY_LOCAL_MACHINE", subkeys(HKEY_LOCAL_MACHINE).iter().any(|k| k == b"SOFTWARE"));

        // The predefined keys IBM's REGISTRY maps.
        let mut c = 0;
        let mut u = 0;
        let ok = RegCreateKey(HKEY_CLASSES_ROOT, b".pmx\0".as_ptr(), &mut c) == 0 && RegOpenKeyEx(HKEY_LOCAL_MACHINE, b"SOFTWARE\\Classes\\.pmx\0".as_ptr(), 0, 0, &mut u) == 0;
        RegCloseKey(c);
        RegCloseKey(u);
        RegDeleteKey(HKEY_CLASSES_ROOT, b".pmx\0".as_ptr());
        check("HKEY_CLASSES_ROOT is HKEY_LOCAL_MACHINE\\SOFTWARE\\Classes", ok);
        let ok = RegCreateKey(HKEY_CURRENT_USER, b"PmxTest\0".as_ptr(), &mut c) == 0 && RegOpenKeyEx(HKEY_USERS, b".default\\PmxTest\0".as_ptr(), 0, 0, &mut u) == 0;
        RegCloseKey(c);
        RegCloseKey(u);
        RegDeleteKey(HKEY_CURRENT_USER, b"PmxTest\0".as_ptr());
        check("HKEY_CURRENT_USER is HKEY_USERS\\.default", ok);

        // Deleting takes the subkeys along.
        check("RegDeleteKey with its subkeys", RegDeleteKey(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxTest\0".as_ptr()) == 0);
        let mut g = 0;
        check("deleted keys are gone", RegOpenKeyEx(HKEY_LOCAL_MACHINE, b"SOFTWARE\\PmxTest\\Sub One\0".as_ptr(), 0, 0, &mut g) == ERROR_FILE_NOT_FOUND);
        check("a predefined key is not deleted", RegDeleteKey(HKEY_LOCAL_MACHINE, null()) != 0);
    }
}
