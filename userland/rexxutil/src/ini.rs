//! SysIni: profile (INI file) access through PMSHAPI's Prf calls.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::*;

use crate::rx::*;
use crate::wps::*;

const HINI_PROFILE: u32 = 0;
const HINI_USERPROFILE: u32 = 0xffff_ffff;
const HINI_SYSTEMPROFILE: u32 = 0xffff_fffe;

/// SysIni([file], app [, key [, value]]): query, set or delete a value;
/// `app` 'ALL:' (with a stem as third argument) lists the applications,
/// `key` 'ALL:' (with a stem as fourth) an application's keys, 'DELETE:'
/// deletes. Returns the value, "", or "ERROR:".
pub fn sys_ini(a: &Args, r: &mut Ret) -> u32 {
    let orig_cap = r.cap();
    r.set(b"");
    if a.n() < 2 || a.n() > 4 || !a.valid(1) {
        return INVALID;
    }
    let file: Vec<u8> = if a.valid(0) { a.cstr(0).to_vec() } else { b"USER".to_vec() };
    let mut app: Vec<u8> = a.cstr(1).to_vec();
    let mut key: Vec<u8> = if a.n() > 2 && a.exists(2) { a.cstr(2).to_vec() } else { Vec::new() };
    let value: Vec<u8> = if a.n() == 4 { a.bytes(3).to_vec() } else { Vec::new() };
    // A wildcard query: None, or Some(true) for the applications.
    let mut wild: Option<bool> = None;
    let mut stem = Vec::new();
    if eqi(&app, b"ALL:") {
        app.clear();
        wild = Some(true);
        if a.n() != 3 {
            return INVALID;
        }
        stem = stem_name(a.cstr(2));
    } else if eqi(&key, b"ALL:") {
        key.clear();
        wild = Some(false);
        if a.n() != 4 {
            return INVALID;
        }
        stem = stem_name(a.cstr(3));
    }
    let (Some(open), Some(close), Some(qprof), Some(qdata), Some(write)) =
        (PrfOpenProfile(), PrfCloseProfile(), PrfQueryProfile(), PrfQueryProfileData(), PrfWriteProfileData())
    else {
        return INVALID;
    };
    let Some(hab) = Hab::new() else {
        return INVALID;
    };
    let hini = if eqi(&file, b"BOTH") {
        HINI_PROFILE
    } else if eqi(&file, b"USER") {
        HINI_USERPROFILE
    } else if eqi(&file, b"SYSTEM") {
        HINI_SYSTEMPROFILE
    } else {
        let mut user = [0u8; 256];
        let mut sys = [0u8; 256];
        let mut prf: [u32; 4] = [256, user.as_mut_ptr() as u32, 256, sys.as_mut_ptr() as u32];
        let f = cz(&file);
        let h = if unsafe { qprof(hab.hab, prf.as_mut_ptr()) } != 0 {
            if eqi(&file, &user[..c_len(user.as_ptr())]) {
                HINI_USERPROFILE
            } else if eqi(&file, &sys[..c_len(sys.as_ptr())]) {
                HINI_SYSTEMPROFILE
            } else {
                unsafe { open(hab.hab, f.as_ptr()) }
            }
        } else {
            unsafe { open(hab.hab, f.as_ptr()) }
        };
        if h == 0 && !eqi(&file, b"BOTH") {
            r.set(b"ERROR:");
            return 0;
        }
        h
    };
    let (appz, keyz) = (cz(&app), cz(&key));
    let failed;
    let mut list: *mut u8 = null_mut();
    if (a.n() == 3 && !eqi(&key, b"DELETE:")) || wild.is_some() {
        let mut size: u32 = 0xffff;
        if unsafe { DosAllocMem(&mut list, 0xffff, PAG_COMMIT | PAG_WRITE | OBJ_TILE) } != 0 {
            unsafe { close(hini) };
            drop(hab);
            r.set(b"2");
            return 0;
        }
        let ok = match wild {
            Some(true) => unsafe { qdata(hini, null(), null(), list, &mut size) },
            Some(false) => unsafe { qdata(hini, appz.as_ptr(), null(), list, &mut size) },
            None => unsafe { qdata(hini, appz.as_ptr(), keyz.as_ptr(), list, &mut size) },
        };
        failed = ok == 0;
        if failed {
            r.set(b"ERROR:");
        } else if wild.is_none() {
            let data = unsafe { core::slice::from_raw_parts(list, size as usize) };
            if size as usize > orig_cap {
                let mut p: *mut u8 = null_mut();
                if unsafe { DosAllocMem(&mut p, size, PAG_COMMIT | PAG_WRITE | PAG_READ) } != 0 {
                    unsafe { DosFreeMem(list) };
                    unsafe { close(hini) };
                    drop(hab);
                    r.set(b"2");
                    return 0;
                }
                unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len()) };
                r.give(p, data.len());
            } else {
                r.set(data);
            }
        }
    } else {
        let ok = if eqi(&key, b"DELETE:") || a.n() == 2 || !a.valid(2) {
            unsafe { write(hini, appz.as_ptr(), null(), null(), 0) }
        } else if eqi(a.cstr(3), b"DELETE:") || !a.valid(3) {
            unsafe { write(hini, appz.as_ptr(), keyz.as_ptr(), null(), 0) }
        } else {
            unsafe { write(hini, appz.as_ptr(), keyz.as_ptr(), value.as_ptr(), value.len() as u32) }
        };
        failed = ok == 0;
        if failed {
            r.set(b"ERROR:");
        }
    }
    unsafe { close(hini) };
    drop(hab);
    if wild.is_some() {
        let mut count = 0u32;
        if !failed {
            let data = unsafe { core::slice::from_raw_parts(list, 0xffff) };
            let mut x = 0;
            loop {
                let len = data[x..].iter().position(|&c| c == 0).unwrap_or(data.len() - x);
                if len != 0 {
                    let mut item = &data[x..x + len];
                    x += len + 1;
                    count += 1;
                    if item.last() == Some(&b'\n') {
                        item = &item[..item.len() - 1];
                    }
                    if set_var(SHV_SET, &tail(&stem, &dec(count as i64)), item) == SHV_BADN {
                        unsafe { DosFreeMem(list) };
                        return INVALID;
                    }
                }
                if x >= data.len() || data[x] == 0 {
                    break;
                }
            }
        }
        unsafe { DosFreeMem(list) };
        if set_var(SHV_SET, &tail(&stem, b"0"), &dec(count as i64)) == SHV_BADN {
            return INVALID;
        }
    } else if !list.is_null() {
        unsafe { DosFreeMem(list) };
    }
    0
}
