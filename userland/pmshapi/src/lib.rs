//! PMSHAPI.DLL: the profile calls (Prf*) on real INI files, the program
//! list (PrfAddProgram & co., kept in the user profile) and the task
//! position and title calls.
//!
//! Profiles are OS/2 binary INI files (format: crates/inifile), shared by
//! every process and written through to disk at once (see prf.rs). The
//! user and system profiles (HINI_USERPROFILE, HINI_SYSTEMPROFILE) are
//! the files named by USER_INI and SYSTEM_INI in the environment of the
//! first process that uses PMSHAPI (CONFIG.SYS `SET USER_INI=`), by
//! default \OS2\OS2.INI and \OS2\OS2SYS.INI on the boot drive, created
//! when missing. HINI_PROFILE reads the user profile, then the system
//! profile, and writes to the user profile. Application and key names
//! are case-sensitive, as on OS/2.
//!
//! PMSHAPI is also loaded by text-mode programs (REXXUTIL's SysIni,
//! TELNET), so it does not import PMWIN; WinGetLastError codes are not
//! set.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;
extern crate os2;

mod exports;
mod prf;
mod proglist;
mod switch;

use alloc::vec::Vec;

use inifile::{Profile, name_list};
use prf::cbytes;

/// _DLL_InitTerm(hmod, flag).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// What a query asks for.
enum Query {
    /// The application names (pszApp NULL).
    Apps,
    /// The key names of an application (pszKey NULL).
    Keys(Vec<u8>),
    Value(Vec<u8>, Vec<u8>),
}

impl Query {
    unsafe fn new(app: *const u8, key: *const u8) -> Query {
        match unsafe { (cbytes(app), cbytes(key)) } {
            (None, _) => Query::Apps,
            (Some(a), None) => Query::Keys(a.to_vec()),
            (Some(a), Some(k)) => Query::Value(a.to_vec(), k.to_vec()),
        }
    }

    fn is_list(&self) -> bool {
        !matches!(self, Query::Value(..))
    }
}

/// The answer to `q` from the profiles `hini` searches: a value, or a name
/// list (each name NUL-terminated, then a final NUL). None when there is
/// no such value, or no names to list.
fn lookup(hini: u32, q: &Query) -> Option<Vec<u8>> {
    prf::read(hini, |ps: &[&Profile]| match q {
        Query::Value(a, k) => ps.iter().find_map(|p| p.get(a, k)).map(|v| v.to_vec()),
        Query::Apps => {
            let mut names: Vec<&[u8]> = Vec::new();
            for a in ps.iter().flat_map(|p| p.apps.iter()) {
                if !names.contains(&a.name.as_slice()) {
                    names.push(&a.name);
                }
            }
            (!names.is_empty()).then(|| name_list(names))
        }
        Query::Keys(app) => {
            let mut names: Vec<&[u8]> = Vec::new();
            for k in ps.iter().filter_map(|p| p.app(app)).flat_map(|a| a.keys.iter()) {
                if !names.contains(&k.name.as_slice()) {
                    names.push(&k.name);
                }
            }
            (!names.is_empty()).then(|| name_list(names))
        }
    })
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfOpenProfile(_hab: u32, name: *const u8) -> u32 {
    match unsafe { cbytes(name) } {
        Some(n) if !n.is_empty() => prf::open(n),
        _ => 0,
    }
}

/// PrfCloseProfile: FALSE for HINI_USERPROFILE/HINI_SYSTEMPROFILE and
/// handles that are not open.
#[unsafe(no_mangle)]
pub extern "C" fn PrfCloseProfile(hini: u32) -> u32 {
    prf::close(hini) as u32
}

/// PrfQueryProfileSize: the size of a value, or of the buffer an
/// enumeration needs (its final NUL included).
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfileSize(hini: u32, app: *const u8, key: *const u8, size: *mut u32) -> u32 {
    if size.is_null() {
        return 0;
    }
    match lookup(hini, &unsafe { Query::new(app, key) }) {
        Some(v) => {
            unsafe { *size = v.len() as u32 };
            1
        }
        None => 0,
    }
}

/// PrfQueryProfileData: as much of the value as fits in *pulBufferMax
/// bytes; *pulBufferMax becomes the number of bytes copied, and the
/// result is FALSE if that was not all of it. An enumeration that fits
/// reports its length without the final NUL (which is copied), as OS/2
/// documents.
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32 {
    if len.is_null() {
        return 0;
    }
    let q = unsafe { Query::new(app, key) };
    let Some(v) = lookup(hini, &q) else { return 0 };
    let room = unsafe { *len } as usize;
    let n = if buf.is_null() { 0 } else { v.len().min(room) };
    if n > 0 {
        unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), buf, n) };
    }
    let all = n == v.len();
    unsafe { *len = if all && q.is_list() { n as u32 - 1 } else { n as u32 } };
    all as u32
}

/// PrfQueryProfileString: the value (NUL-terminated, truncated to the
/// buffer with a NUL in its last byte) or else the default; the number
/// of bytes copied including the NUL; 0 when neither exists. An
/// enumeration returns the list's length without its final NUL, or the
/// bytes copied if it was cut short.
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, default: *const u8, buf: *mut u8, len: u32) -> u32 {
    if buf.is_null() || len == 0 {
        return 0;
    }
    let q = unsafe { Query::new(app, key) };
    let found = lookup(hini, &q);
    if q.is_list() {
        // The default is not used for enumerations.
        let Some(v) = found else { return 0 };
        let n = v.len().min(len as usize);
        unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), buf, n) };
        return if n == v.len() { n as u32 - 1 } else { n as u32 };
    }
    let mut v = match found {
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

/// PrfQueryProfileInt: the leading decimal number of the value (0 if it
/// does not start with one), or the default if there is no such key.
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfileInt(hini: u32, app: *const u8, key: *const u8, default: i32) -> i32 {
    if app.is_null() || key.is_null() {
        return default;
    }
    let Some(v) = lookup(hini, &unsafe { Query::new(app, key) }) else { return default };
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
    if neg { n.wrapping_neg() } else { n }
}

/// Store `data` at app/key; data None deletes the key, key None the
/// application (deleting what is not there succeeds); app None with key
/// and data None empties the profile.
fn store(hini: u32, app: *const u8, key: *const u8, data: Option<&[u8]>) -> u32 {
    let (app, key) = unsafe { (cbytes(app), cbytes(key)) };
    prf::write(hini, |p| {
        match (app, key, data) {
            (None, None, None) => p.apps.clear(),
            (None, _, _) => return None,
            (Some(a), None, _) => {
                p.remove_app(a);
            }
            (Some(a), Some(k), None) => {
                p.remove_key(a, k);
            }
            (Some(a), Some(k), Some(d)) => p.set(a, k, d).ok()?,
        }
        Some(())
    })
    .is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32 {
    if app.is_null() {
        return 0;
    }
    let d = unsafe { cbytes(data) }.map(|d| {
        let mut v = d.to_vec();
        v.push(0);
        v
    });
    store(hini, app, key, d.as_deref())
}

#[unsafe(no_mangle)]
pub extern "C" fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32 {
    if len as usize > inifile::MAX_DATA {
        return 0;
    }
    let d = (!data.is_null()).then(|| unsafe { core::slice::from_raw_parts(data, len as usize) });
    store(hini, app, key, d)
}

/// Copy `name` (no NUL) into a PRFPROFILE name field: `cch` is the
/// buffer size, then the name's length with its NUL. False if it did not
/// fit.
unsafe fn put_name(pcch: *mut u32, psz: *mut u8, name: &[u8]) -> bool {
    let cch = unsafe { pcch.read_unaligned() } as usize;
    unsafe { pcch.write_unaligned(name.len() as u32 + 1) };
    if cch == 0 {
        return true;
    }
    if psz.is_null() {
        return false;
    }
    let n = name.len().min(cch - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), psz, n);
        *psz.add(n) = 0;
    }
    n == name.len()
}

/// PrfQueryProfile(hab, PRFPROFILE*): the user and system profile names
/// (cchUserName, pszUserName, cchSysName, pszSysName). A zero length
/// only asks for the length.
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProfile(_hab: u32, prf: *mut u32) -> u32 {
    if prf.is_null() {
        return 0;
    }
    let (user, system) = prf::names();
    unsafe {
        let u = put_name(prf, prf.add(1).read_unaligned() as *mut u8, &user);
        let s = put_name(prf.add(2), prf.add(3).read_unaligned() as *mut u8, &system);
        (u && s) as u32
    }
}

/// PrfReset(hab, PRFPROFILE*): switch every process to another user
/// profile (created if missing); the system profile cannot change.
/// PL_ALTERED is not broadcast.
#[unsafe(no_mangle)]
pub extern "C" fn PrfReset(_hab: u32, prf: *const u32) -> u32 {
    if prf.is_null() {
        return 0;
    }
    let (user, system) = unsafe { (cbytes(prf.add(1).read_unaligned() as *const u8), cbytes(prf.add(3).read_unaligned() as *const u8)) };
    match user {
        Some(u) if !u.is_empty() => prf::reset(u, system) as u32,
        _ => 0,
    }
}

// ---- Task (session) information ----

unsafe extern "C" {
    #[link_name = "DOSCALLS.320"]
    fn DosQueryModuleName(hmod: u32, cb: u32, buf: *mut u8) -> u32;
}

/// A PMWIN entry point when the process uses PM (PMSHAPI is also loaded
/// by text programs, which must not pull PMWIN in).
pub(crate) fn pmwin(ordinal: u32) -> Option<usize> {
    let mut hmod = 0;
    if unsafe { os2::DosQueryModuleHandle(b"PMWIN\0".as_ptr(), &mut hmod) } != 0 {
        return None;
    }
    let mut pfn = 0usize;
    (unsafe { os2::DosQueryProcAddr(hmod, ordinal, core::ptr::null(), &mut pfn) } == 0 && pfn != 0).then_some(pfn)
}

/// The next cascade position for a program's window (left, bottom,
/// right, top), from PMWIN.
fn shell_position() -> Option<[i32; 4]> {
    // Rs2ShellPosition (PMWIN 9001).
    let f = pmwin(9001)?;
    let f: extern "C" fn(*mut [i32; 4]) -> u32 = unsafe { core::mem::transmute(f) };
    let mut r = [0i32; 4];
    (f(&mut r) != 0).then_some(r)
}

/// WinQueryTaskSizePos(hab, idProcess, pswp): where the shell would put
/// the program's main window (SWP: fl, cy, cx, y, x, hwndInsertBehind,
/// hwnd, ulReserved1, ulReserved2); NO_ERROR.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryTaskSizePos(_hab: u32, _pid: u32, pswp: *mut u32) -> u32 {
    if pswp.is_null() {
        return 87;
    }
    let [l, b, r, t] = shell_position().unwrap_or([32, 240, 646, 700]);
    let (x, y, cx, cy) = (l, b, r - l, t - b);
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
