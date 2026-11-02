//! Semaphores, priorities, code pages, national language support, macro
//! space, function packages, LIBPATH extensions, messages, versions and
//! timing.

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::rx::*;

/// SysUtilVersion(): the package's version.
pub fn sys_util_version(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    r.set(b"2.00");
    0
}

fn os_version() -> (u32, u32) {
    let mut v = [0u32; 2];
    unsafe { DosQuerySysInfo(11, 12, v.as_mut_ptr(), 8) };
    (v[0], v[1])
}

fn version_text(prefix: &[u8]) -> Vec<u8> {
    let (major, minor) = os_version();
    let mut v = prefix.to_vec();
    v.extend(dec((major / 10) as i64));
    v.push(b'.');
    v.extend(dec_w(minor as i64, 2, true));
    v
}

/// SysOS2Ver(): "2.40" on Warp 4.
pub fn sys_os2_ver(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    r.set(&version_text(b""));
    0
}

/// SysVersion(): "OS/2 2.40" on Warp 4.
pub fn sys_version(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    r.set(&version_text(b"OS/2 "));
    0
}

/// SysBootDrive(): "C:".
pub fn sys_boot_drive(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    let mut d = 0u32;
    unsafe { DosQuerySysInfo(5, 5, &mut d, 4) };
    r.set(&[(d + 0x40) as u8, b':']);
    0
}

/// SysSleep(seconds): seconds with up to three decimals (more digits
/// are ignored), at most 9 characters.
pub fn sys_sleep(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let s = a.bytes(0);
    if s.is_empty() || s.len() > 9 {
        return INVALID;
    }
    let mut i = 0;
    let mut ms: u32 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        ms = ms * 10 + (s[i] - b'0') as u32;
        i += 1;
    }
    ms = ms.wrapping_mul(1000);
    if i < s.len() && s[i] == b'.' {
        i += 1;
        let (mut digits, mut frac) = (0, 0u32);
        while i < s.len() {
            if !s[i].is_ascii_digit() {
                return INVALID;
            }
            digits += 1;
            if digits < 4 {
                frac = frac * 10 + (s[i] - b'0') as u32;
            }
            i += 1;
        }
        while digits < 3 {
            frac *= 10;
            digits += 1;
        }
        ms = ms.wrapping_add(frac);
    } else if i < s.len() {
        return INVALID;
    }
    unsafe { DosSleep(ms) };
    r.set(b"0");
    0
}

/// SysWaitNamedPipe(name [, timeout]): DosWaitNPipe's return code.
pub fn sys_wait_named_pipe(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 2 || !a.valid(0) {
        return INVALID;
    }
    let mut timeout: i32 = 0;
    if a.n() == 2 {
        match string2long(a.cstr(1)) {
            Some(t) if t >= 0 || t == -1 => timeout = t,
            _ => return INVALID,
        }
    }
    let n = cz(a.cstr(0));
    r.num(unsafe { DosWaitNPipe(n.as_ptr(), timeout as u32) } as i32);
    0
}

/// SysAddFileHandle(n): the new number of file handles.
pub fn sys_add_file_handle(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let Some(mut n) = string2long(a.cstr(0)).filter(|&n| n >= 0) else {
        return INVALID;
    };
    let mut cur = 0u32;
    unsafe { DosSetRelMaxFH(&mut n, &mut cur) };
    r.num(cur as i32);
    0
}

/// SysSetFileHandle(n): DosSetMaxFH's return code.
pub fn sys_set_file_handle(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let Some(n) = string2long(a.cstr(0)).filter(|&n| n >= 2) else {
        return INVALID;
    };
    r.num(unsafe { DosSetMaxFH(n as u32) } as i32);
    0
}

/// A semaphore handle argument.
fn handle(a: &Args, i: usize) -> Option<u32> {
    string2ulong(a.cstr(i))
}

/// SysCreateEventSem([name]) / SysCreateMutexSem([name]): the handle
/// ("" on failure); a named semaphore that exists is opened.
fn create_sem(a: &Args, r: &mut Ret, event: bool) -> u32 {
    if a.n() > 1 {
        return INVALID;
    }
    let mut h = 0u32;
    let name = if a.n() == 1 && a.exists(0) { Some(cz(a.cstr(0))) } else { None };
    let np = name.as_ref().map_or(null(), |n| n.as_ptr());
    let mut rc = unsafe { if event { DosCreateEventSem(np, &mut h, 1, 0) } else { DosCreateMutexSem(np, &mut h, 1, 0) } };
    if rc != 0 && name.is_some() {
        rc = unsafe { if event { DosOpenEventSem(np, &mut h) } else { DosOpenMutexSem(np, &mut h) } };
    }
    if rc == 0 {
        r.unum(h);
    } else {
        r.set(b"");
    }
    0
}

pub fn sys_create_event_sem(a: &Args, r: &mut Ret) -> u32 {
    create_sem(a, r, true)
}

pub fn sys_create_mutex_sem(a: &Args, r: &mut Ret) -> u32 {
    create_sem(a, r, false)
}

/// The one-handle semaphore calls: the API's return code.
fn sem_call(a: &Args, r: &mut Ret, f: impl FnOnce(u32) -> u32) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let Some(h) = handle(a, 0) else {
        return INVALID;
    };
    r.num(f(h) as i32);
    0
}

pub fn sys_open_event_sem(a: &Args, r: &mut Ret) -> u32 {
    sem_call(a, r, |mut h| unsafe { DosOpenEventSem(null(), &mut h) })
}

pub fn sys_close_event_sem(a: &Args, r: &mut Ret) -> u32 {
    sem_call(a, r, |h| unsafe { DosCloseEventSem(h) })
}

pub fn sys_reset_event_sem(a: &Args, r: &mut Ret) -> u32 {
    sem_call(a, r, |h| {
        let mut n = 0u32;
        unsafe { DosResetEventSem(h, &mut n) }
    })
}

pub fn sys_post_event_sem(a: &Args, r: &mut Ret) -> u32 {
    sem_call(a, r, |h| unsafe { DosPostEventSem(h) })
}

pub fn sys_open_mutex_sem(a: &Args, r: &mut Ret) -> u32 {
    sem_call(a, r, |mut h| unsafe { DosOpenMutexSem(null(), &mut h) })
}

pub fn sys_close_mutex_sem(a: &Args, r: &mut Ret) -> u32 {
    sem_call(a, r, |h| unsafe { DosCloseMutexSem(h) })
}

pub fn sys_release_mutex_sem(a: &Args, r: &mut Ret) -> u32 {
    sem_call(a, r, |h| unsafe { DosReleaseMutexSem(h) })
}

/// SysWaitEventSem / SysRequestMutexSem(handle [, timeout]).
fn wait_sem(a: &Args, r: &mut Ret, f: impl FnOnce(u32, u32) -> u32) -> u32 {
    if a.n() < 1 || a.n() > 2 || !a.valid(0) {
        return INVALID;
    }
    let mut timeout: i32 = -1;
    if a.n() == 2 {
        match string2long(a.cstr(1)) {
            Some(t) => timeout = t,
            None => return INVALID,
        }
    }
    let Some(h) = handle(a, 0) else {
        return INVALID;
    };
    r.num(f(h, timeout as u32) as i32);
    0
}

pub fn sys_wait_event_sem(a: &Args, r: &mut Ret) -> u32 {
    wait_sem(a, r, |h, t| unsafe { DosWaitEventSem(h, t) })
}

pub fn sys_request_mutex_sem(a: &Args, r: &mut Ret) -> u32 {
    wait_sem(a, r, |h, t| unsafe { DosRequestMutexSem(h, t) })
}

/// SysSetPriority(class, delta): DosSetPriority's return code (for the
/// calling thread).
pub fn sys_set_priority(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 || !a.valid(0) {
        return INVALID;
    }
    let Some(class) = string2long(a.cstr(0)).filter(|&n| (0..=4).contains(&n)) else {
        return INVALID;
    };
    let Some(delta) = string2long(a.cstr(1)).filter(|&n| (-31..=31).contains(&n)) else {
        return INVALID;
    };
    r.num(unsafe { DosSetPriority(2, class as u32, delta, 0) } as i32);
    0
}

/// SysProcessType(): the process type (0 full screen, 2 window, 3 PM, 4
/// detached).
pub fn sys_process_type(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    let mut tib: *mut Tib = null_mut();
    let mut pib: *mut Pib = null_mut();
    unsafe { DosGetInfoBlocks(&mut tib, &mut pib) };
    r.num(unsafe { (*pib).ultype } as i32);
    0
}

/// SysQueryProcessCodePage(): the process's code page.
pub fn sys_query_process_code_page(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    let (mut cp, mut cb) = (0u32, 0u32);
    unsafe { DosQueryCp(4, &mut cp, &mut cb) };
    r.num(cp as i32);
    0
}

/// SysSetProcessCodePage(cp): DosSetProcessCp's return code.
pub fn sys_set_process_code_page(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let Some(cp) = string2ulong(a.cstr(0)) else {
        return INVALID;
    };
    r.num(unsafe { DosSetProcessCp(cp) } as i32);
    0
}

/// Country and code page from two optional arguments.
fn country(a: &Args, i: usize, need_valid: bool) -> Option<CountryCode> {
    let mut cc = CountryCode::default();
    if a.n() > i && (!need_valid || a.valid(i)) {
        cc.country = string2ulong(a.cstr(i))?;
    }
    if a.n() == i + 2 {
        cc.codepage = string2ulong(a.cstr(i + 1))?;
    }
    Some(cc)
}

/// SysGetCollate([country [, codepage]]): the collating table.
pub fn sys_get_collate(a: &Args, r: &mut Ret) -> u32 {
    if a.n() > 2 {
        return INVALID;
    }
    let Some(cc) = country(a, 0, true) else {
        return INVALID;
    };
    let mut buf = vec![0u8; r.cap().max(1)];
    let mut cb = buf.len() as u32;
    if unsafe { DosQueryCollate(cb, &cc, buf.as_mut_ptr(), &mut cb) } != 0 {
        return INVALID;
    }
    r.set(&buf[..(cb as usize).min(buf.len())]);
    0
}

/// SysMapCase(string [, country [, codepage]]): the string in capitals
/// ("" on failure).
pub fn sys_map_case(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 3 || !a.valid(0) {
        return INVALID;
    }
    let Some(cc) = country(a, 1, true) else {
        return INVALID;
    };
    let mut s = a.bytes(0).to_vec();
    if unsafe { DosMapCase(s.len() as u32, &cc, s.as_mut_ptr()) } != 0 {
        s.clear();
    }
    r.set(&s);
    0
}

/// SysNationalLanguageCompare(s1, s2 [, country [, codepage]]): -1, 0 or
/// 1 by the collating table.
pub fn sys_national_language_compare(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 2 || a.n() > 4 || !a.valid(0) || !a.valid(1) {
        return INVALID;
    }
    let Some(cc) = country(a, 2, true) else {
        return INVALID;
    };
    let mut table = [0u8; 256];
    let mut cb = 0u32;
    if unsafe { DosQueryCollate(256, &cc, table.as_mut_ptr(), &mut cb) } != 0 {
        return INVALID;
    }
    let (x, y) = (a.bytes(0), a.bytes(1));
    let w = |c: u8| table[c as usize] as i8 as i32;
    let mut d = x.iter().zip(y).map(|(&p, &q)| w(p) - w(q)).find(|&d| d != 0).unwrap_or(0);
    if d == 0 {
        d = x.len() as i32 - y.len() as i32;
    }
    r.num(d.signum());
    0
}

/// SysAddRexxMacro(name, file [, 'B'|'A']).
pub fn sys_add_rexx_macro(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 2 || a.n() > 3 || !a.valid(0) || !a.valid(1) {
        return INVALID;
    }
    let mut pos = 1;
    if a.n() == 3 {
        if a.exists(2) && a.len(2) == 0 {
            return INVALID;
        }
        pos = match upper(a.first(2)) {
            b'B' => 1,
            b'A' => 2,
            _ => return INVALID,
        };
    }
    r.num(unsafe { RexxAddMacro(a.ptr(0), a.ptr(1), pos) } as i32);
    0
}

/// SysDropRexxMacro(name).
pub fn sys_drop_rexx_macro(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let n = cz(a.cstr(0));
    r.num(unsafe { RexxDropMacro(n.as_ptr()) } as i32);
    0
}

/// SysReorderRexxMacro(name, 'B'|'A').
pub fn sys_reorder_rexx_macro(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 || !a.valid(0) || (a.exists(1) && a.len(1) == 0) {
        return INVALID;
    }
    let pos = match upper(a.first(1)) {
        b'B' => 1,
        b'A' => 2,
        _ => return INVALID,
    };
    r.num(unsafe { RexxReorderMacro(a.ptr(0), pos) } as i32);
    0
}

/// SysQueryRexxMacro(name): "B" or "A" (its search order), or "".
pub fn sys_query_rexx_macro(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let n = cz(a.cstr(0));
    let mut pos = 0u16;
    if unsafe { RexxQueryMacro(n.as_ptr(), &mut pos) } == 0 {
        r.set(if pos == 1 { b"B" } else { b"A" });
    } else {
        r.set(b"");
    }
    0
}

/// SysClearRexxMacroSpace().
pub fn sys_clear_rexx_macro_space(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 0 {
        return INVALID;
    }
    r.num(unsafe { RexxClearMacroSpace() } as i32);
    0
}

/// SysLoadRexxMacroSpace(file).
pub fn sys_load_rexx_macro_space(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let f = cz(a.cstr(0));
    r.num(unsafe { RexxLoadMacroSpace(0, null(), f.as_ptr()) } as i32);
    0
}

/// SysSaveRexxMacroSpace(file).
pub fn sys_save_rexx_macro_space(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 {
        return INVALID;
    }
    let f = cz(a.cstr(0));
    r.num(unsafe { RexxSaveMacroSpace(0, null(), f.as_ptr()) } as i32);
    0
}

/// IBM's LoadLibraryRoutine: a function package's loader entry (by
/// name, or ordinal 1), which must be 32-bit.
fn package_entry(module: &[u8], name: Option<&[u8]>) -> Result<RexxFunction, u32> {
    let m = cz(module);
    let mut name: Option<Vec<u8>> = name.map(cz);
    let ord = if name.is_some() { 0 } else { 1 };
    let mut h = 0u32;
    let mut rc = unsafe { DosQueryModuleHandle(m.as_ptr(), &mut h) };
    if rc == 126 {
        rc = unsafe { DosLoadModule(null_mut(), 0, m.as_ptr(), &mut h) };
    }
    if rc != 0 {
        return Err(rc);
    }
    let np = |n: &Option<Vec<u8>>| n.as_ref().map_or(null(), |v| v.as_ptr());
    let mut ty = 0u32;
    rc = unsafe { DosQueryProcType(h, ord, np(&name), &mut ty) };
    if rc == 6 {
        h = 0;
        rc = unsafe { DosLoadModule(null_mut(), 0, m.as_ptr(), &mut h) };
        if rc == 0 {
            rc = unsafe { DosQueryProcType(h, ord, np(&name), &mut ty) };
        }
    }
    if rc != 0 {
        if let Some(n) = name.as_mut() {
            n.iter_mut().for_each(|c| *c = upper(*c));
        }
        rc = unsafe { DosQueryProcType(h, ord, np(&name), &mut ty) };
    }
    if rc != 0 {
        return Err(rc);
    }
    if ty != 1 {
        return Err(INVALID);
    }
    let mut p = 0usize;
    rc = unsafe { DosQueryProcAddr(h, ord, np(&name), &mut p) };
    if rc != 0 || p == 0 {
        return Err(rc.max(1));
    }
    Ok(unsafe { core::mem::transmute::<usize, RexxFunction>(p) })
}

/// SysLoadLibrary / SysDropLibrary(dll [, entry]): call a function
/// package's loader as a function named "LOAD"; "0" if it succeeded.
fn library(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 2 || !a.valid(0) {
        return INVALID;
    }
    let name = if a.n() == 2 { Some(a.cstr(1)) } else { None };
    let rc = match package_entry(a.cstr(0), name) {
        Ok(f) => {
            let mut buf = [0u8; 260];
            let mut ret = RxString { len: 256, ptr: buf.as_mut_ptr() };
            unsafe { f(b"LOAD\0".as_ptr(), 0, null(), a.queue(), &mut ret) }
        }
        Err(rc) => rc,
    };
    r.set(if rc == 0 { b"0" } else { b"1" });
    0
}

pub fn sys_load_library(a: &Args, r: &mut Ret) -> u32 {
    library(a, r)
}

pub fn sys_drop_library(a: &Args, r: &mut Ret) -> u32 {
    library(a, r)
}

/// SysQueryExtLIBPATH('B'|'E').
pub fn sys_query_ext_libpath(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 1 || !a.valid(0) {
        return INVALID;
    }
    let flag = match upper(a.first(0)) {
        b'B' => 1,
        b'E' => 2,
        _ => return INVALID,
    };
    let mut p: *mut u8 = null_mut();
    if unsafe { DosAllocMem(&mut p, 0x400, PAG_COMMIT | PAG_WRITE | PAG_READ) } != 0 {
        return INVALID;
    }
    unsafe {
        core::ptr::write_bytes(p, 0, 0x400);
        DosQueryExtLIBPATH(p, flag);
    }
    r.give(p, c_len(p));
    0
}

/// SysSetExtLIBPATH(path, 'B'|'E'): DosSetExtLIBPATH's return code.
pub fn sys_set_ext_libpath(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 2 || !a.valid(1) {
        return INVALID;
    }
    let flag = match upper(a.first(1)) {
        b'B' => 1,
        b'E' => 2,
        _ => return INVALID,
    };
    let path = if a.valid(0) { cz(a.cstr(0)) } else { cz(b"") };
    r.num(unsafe { DosSetExtLIBPATH(path.as_ptr(), flag) } as i32);
    0
}

/// SysGetMessage(num [, file [, insert...]]): the message text.
pub fn sys_get_message(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 11 || !a.valid(0) {
        return INVALID;
    }
    let Some(num) = string2long(a.cstr(0)).filter(|&n| n >= 0) else {
        return INVALID;
    };
    let file = if a.n() >= 2 && a.valid(1) { cz(a.cstr(1)) } else { cz(b"OSO001.MSG") };
    let mut table: Vec<*const u8> = Vec::new();
    for i in 2..a.n() {
        if !a.exists(i) {
            return INVALID;
        }
        table.push(a.ptr(i));
    }
    let mut buf = vec![0u8; 0x800];
    let mut len = 0u32;
    unsafe { DosGetMessage(table.as_ptr(), table.len() as u32, buf.as_mut_ptr(), 0x800, num as u32, file.as_ptr(), &mut len) };
    if !r.set(&buf[..(len as usize).min(0x800)]) {
        r.set(b"2");
    }
    0
}

/// SysElapsedTime(['E'|'R']): seconds since the first call or the last
/// reset ('R' resets).
pub fn sys_elapsed_time(a: &Args, r: &mut Ret) -> u32 {
    static FREQ: AtomicU32 = AtomicU32::new(0);
    static BASE_LO: AtomicU32 = AtomicU32::new(0);
    static BASE_HI: AtomicU32 = AtomicU32::new(0);
    if a.n() > 1 {
        return INVALID;
    }
    let mut reset = false;
    if a.n() == 1 {
        match upper(a.first(0)) {
            b'E' => reset = false,
            b'R' => reset = true,
            _ => return INVALID,
        }
    }
    let mut t = 0u64;
    unsafe { DosTmrQueryTime(&mut t) };
    let (lo, hi) = (t as u32, (t >> 32) as u32);
    if FREQ.load(Ordering::Relaxed) == 0 {
        let mut f = 0u32;
        unsafe { DosTmrQueryFreq(&mut f) };
        FREQ.store(f, Ordering::Relaxed);
        BASE_LO.store(lo, Ordering::Relaxed);
        BASE_HI.store(hi, Ordering::Relaxed);
    }
    let base = BASE_LO.load(Ordering::Relaxed) as f64 + BASE_HI.load(Ordering::Relaxed) as f64 * 4294967296.0;
    let secs = ((lo as f64 + hi as f64 * 4294967296.0) - base) / FREQ.load(Ordering::Relaxed) as f64;
    if reset {
        BASE_LO.store(lo, Ordering::Relaxed);
        BASE_HI.store(hi, Ordering::Relaxed);
    }
    if secs == 0.0 {
        r.set(b"0");
    } else {
        let s = alloc::format!("{secs:.6}");
        r.set(s.as_bytes());
    }
    0
}
