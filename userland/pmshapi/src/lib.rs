//! PMSHAPI.DLL: the profile calls (Prf*). Profiles are kept in memory
//! for the life of the process; nothing is read from or written to INI
//! files yet, so queries of settings no one wrote fail and programs use
//! their defaults (TELNET reads its options this way, REXXUTIL's SysIni
//! works on the in-memory data).

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod exports;

use alloc::vec::Vec;
use core::cell::UnsafeCell;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

const HINI_PROFILE: u32 = 0;
const HINI_USERPROFILE: u32 = 0xffff_ffff;
const HINI_SYSTEMPROFILE: u32 = 0xffff_fffe;

struct Entry {
    hini: u32,
    app: Vec<u8>,
    key: Vec<u8>,
    data: Vec<u8>,
}

struct Store {
    entries: Vec<Entry>,
    open: Vec<u32>,
    next: u32,
}

struct Global(UnsafeCell<Store>);
unsafe impl Sync for Global {}

static STORE: Global = Global(UnsafeCell::new(Store { entries: Vec::new(), open: Vec::new(), next: 0x100 }));

/// The process's store (PM programs call Prf* from one thread at a time
/// in practice; DosEnterCritSec keeps two from interleaving).
fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    unsafe {
        os2::DosEnterCritSec();
        let r = f(&mut *STORE.0.get());
        os2::DosExitCritSec();
        r
    }
}

unsafe fn cbytes<'a>(p: *const u8) -> Option<&'a [u8]> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    Some(unsafe { core::slice::from_raw_parts(p, n) })
}

fn eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

fn valid(s: &Store, hini: u32) -> bool {
    matches!(hini, HINI_PROFILE | HINI_USERPROFILE | HINI_SYSTEMPROFILE) || s.open.contains(&hini)
}

fn hits(e: &Entry, hini: u32) -> bool {
    e.hini == hini || hini == HINI_PROFILE && (e.hini == HINI_USERPROFILE || e.hini == HINI_SYSTEMPROFILE)
}

/// The value of app/key, or with app NULL the application names, with
/// key NULL the key names (each NUL-terminated, a final NUL).
fn lookup(hini: u32, app: *const u8, key: *const u8) -> Option<Vec<u8>> {
    let (app, key) = unsafe { (cbytes(app), cbytes(key)) };
    with(|s| {
        if !valid(s, hini) {
            return None;
        }
        let mine = s.entries.iter().filter(|e| hits(e, hini));
        match (app, key) {
            (None, _) => {
                let mut names: Vec<&[u8]> = Vec::new();
                for e in mine {
                    if !names.iter().any(|n| eq(n, &e.app)) {
                        names.push(&e.app);
                    }
                }
                Some(list(&names))
            }
            (Some(a), None) => {
                let names: Vec<&[u8]> = mine.filter(|e| eq(&e.app, a)).map(|e| e.key.as_slice()).collect();
                if names.is_empty() { None } else { Some(list(&names)) }
            }
            (Some(a), Some(k)) => mine.into_iter().find(|e| eq(&e.app, a) && eq(&e.key, k)).map(|e| e.data.clone()),
        }
    })
}

fn list(names: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for n in names {
        v.extend_from_slice(n);
        v.push(0);
    }
    v.push(0);
    v
}

/// Store (or with data None delete) app/key; key NULL deletes the app.
fn store(hini: u32, app: *const u8, key: *const u8, data: Option<&[u8]>) -> u32 {
    let (Some(app), key) = (unsafe { cbytes(app) }, unsafe { cbytes(key) }) else { return 0 };
    let hini = if hini == HINI_PROFILE { HINI_USERPROFILE } else { hini };
    with(|s| {
        if !valid(s, hini) {
            return 0;
        }
        match (key, data) {
            (None, _) => s.entries.retain(|e| !(e.hini == hini && eq(&e.app, app))),
            (Some(k), None) => s.entries.retain(|e| !(e.hini == hini && eq(&e.app, app) && eq(&e.key, k))),
            (Some(k), Some(d)) => match s.entries.iter_mut().find(|e| e.hini == hini && eq(&e.app, app) && eq(&e.key, k)) {
                Some(e) => e.data = d.to_vec(),
                None => s.entries.push(Entry { hini, app: app.to_vec(), key: k.to_vec(), data: d.to_vec() }),
            },
        }
        1
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfOpenProfile(_hab: u32, name: *const u8) -> u32 {
    if unsafe { cbytes(name) }.is_none_or(|n| n.is_empty()) {
        return 0;
    }
    with(|s| {
        s.next += 1;
        let h = s.next;
        s.open.push(h);
        h
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfCloseProfile(hini: u32) -> u32 {
    with(|s| {
        let before = s.open.len();
        s.open.retain(|&h| h != hini);
        if s.open.len() == before {
            return 0;
        }
        s.entries.retain(|e| e.hini != hini);
        1
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfileSize(hini: u32, app: *const u8, key: *const u8, size: *mut u32) -> u32 {
    match lookup(hini, app, key) {
        Some(v) if !size.is_null() => {
            unsafe { *size = v.len() as u32 };
            1
        }
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32 {
    let Some(v) = lookup(hini, app, key) else { return 0 };
    if len.is_null() {
        return 0;
    }
    let room = unsafe { *len } as usize;
    let n = v.len().min(room);
    if n > 0 && !buf.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), buf, n) };
    }
    unsafe { *len = n as u32 };
    (n == v.len()) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, default: *const u8, buf: *mut u8, len: u32) -> u32 {
    if buf.is_null() || len == 0 {
        return 0;
    }
    let mut v = match lookup(hini, app, key) {
        Some(v) => v,
        None => match unsafe { cbytes(default) } {
            Some(d) => d.to_vec(),
            None => return 0,
        },
    };
    if v.last() != Some(&0) {
        v.push(0);
    }
    let n = v.len().min(len as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(v.as_ptr(), buf, n);
        *buf.add(n - 1) = 0;
    }
    n as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfileInt(hini: u32, app: *const u8, key: *const u8, default: i32) -> i32 {
    let Some(v) = lookup(hini, app, key) else { return default };
    let s = v.split(|&c| c == 0).next().unwrap_or(&[]);
    let s = s.trim_ascii();
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut n: i32 = 0;
    for &c in digits.iter().take_while(|c| c.is_ascii_digit()) {
        n = n.wrapping_mul(10).wrapping_add((c - b'0') as i32);
    }
    if neg { -n } else { n }
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32 {
    let d = unsafe { cbytes(data) }.map(|d| {
        let mut v = d.to_vec();
        v.push(0);
        v
    });
    store(hini, app, key, d.as_deref())
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32 {
    let d = (!data.is_null()).then(|| unsafe { core::slice::from_raw_parts(data, len as usize) });
    store(hini, app, key, d)
}

/// PrfQueryProfile / PrfReset: the user and system profile names cannot
/// be queried or changed.
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfile(_hab: u32, _prf: *mut u8) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfReset(_hab: u32, _prf: *const u8) -> u32 {
    0
}

// ---- Task (session) information ----

unsafe extern "C" {
    #[link_name = "DOSCALLS.320"]
    fn DosQueryModuleName(hmod: u32, cb: u32, buf: *mut u8) -> u32;
}

/// Screen size from PMWIN when the process uses PM (PMSHAPI is also
/// loaded by text programs, which must not pull PMWIN in).
fn screen_size() -> (i32, i32) {
    let mut hmod = 0;
    if unsafe { os2::DosQueryModuleHandle(b"PMWIN\0".as_ptr(), &mut hmod) } != 0 {
        return (1024, 768);
    }
    let mut pfn = 0usize;
    // WinQuerySysValue (PMWIN.829).
    if unsafe { os2::DosQueryProcAddr(hmod, 829, core::ptr::null(), &mut pfn) } != 0 || pfn == 0 {
        return (1024, 768);
    }
    let f: extern "C" fn(u32, i32) -> i32 = unsafe { core::mem::transmute(pfn) };
    // HWND_DESKTOP, SV_CXSCREEN / SV_CYSCREEN.
    (f(1, 20), f(1, 21))
}

static NEXT_POS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// WinQueryTaskSizePos(hab, idProcess, pswp): where the shell would put
/// the program's main window (SWP: fl, cy, cx, y, x, hwndInsertBehind,
/// hwnd, ulReserved1, ulReserved2); NO_ERROR.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryTaskSizePos(_hab: u32, _pid: u32, pswp: *mut u32) -> u32 {
    if pswp.is_null() {
        return 87;
    }
    let (w, h) = screen_size();
    let n = NEXT_POS.fetch_add(1, core::sync::atomic::Ordering::Relaxed) % 8;
    let off = 32 + n as i32 * 24;
    let (cx, cy) = (w * 3 / 5, h * 3 / 5);
    let (x, y) = (off, (h / 3 - off / 2).max(0));
    // SWP_SIZE | SWP_MOVE | SWP_ZORDER | SWP_SHOW | SWP_ACTIVATE, on top
    // (HWND_TOP), as the shell starts programs.
    let v = [0x8fu32, cy as u32, cx as u32, y as u32, x as u32, 3, 0, 0, 0];
    for (i, x) in v.into_iter().enumerate() {
        unsafe { pswp.add(i).write_unaligned(x) };
    }
    0
}

/// WinQueryTaskTitle(ulSession, pszTitle, cbTitle): the title of the
/// caller's session: its program name.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryTaskTitle(_session: u32, buf: *mut u8, cb: u32) -> u32 {
    if buf.is_null() || cb == 0 {
        return 87;
    }
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    let mut name = [0u8; 260];
    let mut n = 0;
    let hmod = if pib.is_null() { 0 } else { unsafe { (*pib).hmte } };
    if hmod != 0 && unsafe { DosQueryModuleName(hmod, name.len() as u32, name.as_mut_ptr()) } == 0 {
        n = name.iter().position(|&b| b == 0).unwrap_or(0);
    }
    let full = &name[..n];
    let base = &full[full.iter().rposition(|&b| b == b'\\').map_or(0, |i| i + 1)..];
    let base = &base[..base.iter().position(|&b| b == b'.').unwrap_or(base.len())];
    let k = base.len().min(cb as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(base.as_ptr(), buf, k);
        *buf.add(k) = 0;
    }
    0
}
