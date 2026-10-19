//! Self-checks of PMSHAPI's profile calls on real INI files and of the
//! program list. Each check prints PASS or FAIL; the last line is
//! "initest done: N failure(s)" and the exit code N.
//!
//! `INITEST` runs everything (on C:, a second process included: it runs
//! itself with CHILD). `INITEST WRITE` and `INITEST READ` check that a
//! profile survives a reboot: WRITE writes `X:\PERSIST.INI` on the test
//! disk (the drive labelled RS2DISK or RS2HPFS, from `cargo xtask disk
//! [--hpfs]`), READ, run
//! after booting again with the same disk, reads it back.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

os2::entry!(main);

const HINI_PROFILE: u32 = 0;
const HINI_USER: u32 = 0xffff_ffff;
const HINI_SYSTEM: u32 = 0xffff_fffe;
const SGH_ROOT: u32 = 0xffff_ffff;

unsafe extern "C" {
    #[link_name = "PMSHAPI.102"]
    fn PrfOpenProfile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMSHAPI.103"]
    fn PrfCloseProfile(hini: u32) -> u32;
    #[link_name = "PMSHAPI.101"]
    fn PrfQueryProfileSize(hini: u32, app: *const u8, key: *const u8, size: *mut u32) -> u32;
    #[link_name = "PMSHAPI.117"]
    fn PrfQueryProfileData(hini: u32, app: *const u8, key: *const u8, buf: *mut u8, len: *mut u32) -> u32;
    #[link_name = "PMSHAPI.115"]
    fn PrfQueryProfileString(hini: u32, app: *const u8, key: *const u8, def: *const u8, buf: *mut u8, len: u32) -> u32;
    #[link_name = "PMSHAPI.114"]
    fn PrfQueryProfileInt(hini: u32, app: *const u8, key: *const u8, def: i32) -> i32;
    #[link_name = "PMSHAPI.116"]
    fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
    #[link_name = "PMSHAPI.118"]
    fn PrfWriteProfileData(hini: u32, app: *const u8, key: *const u8, data: *const u8, len: u32) -> u32;
    #[link_name = "PMSHAPI.107"]
    fn PrfQueryProfile(hab: u32, prf: *mut PrfProfile) -> u32;
    #[link_name = "PMSHAPI.108"]
    fn PrfReset(hab: u32, prf: *const PrfProfile) -> u32;
    #[link_name = "PMSHAPI.109"]
    fn PrfAddProgram(hini: u32, details: *const ProgDetails, group: u32) -> u32;
    #[link_name = "PMSHAPI.110"]
    fn PrfChangeProgram(hini: u32, hprog: u32, details: *const ProgDetails) -> u32;
    #[link_name = "PMSHAPI.111"]
    fn PrfQueryDefinition(hini: u32, hprog: u32, details: *mut u8, cb: u32) -> u32;
    #[link_name = "PMSHAPI.104"]
    fn PrfRemoveProgram(hini: u32, hprog: u32) -> u32;
    #[link_name = "PMSHAPI.113"]
    fn PrfQueryProgramTitles(hini: u32, group: u32, titles: *mut u8, cb: u32, count: *mut u32) -> u32;
    #[link_name = "PMSHAPI.106"]
    fn PrfDestroyGroup(hini: u32, group: u32) -> u32;
    #[link_name = "PMSHAPI.9000"]
    fn PrfCreateGroup(hini: u32, title: *const u8, vis: u32) -> u32;
    #[link_name = "PMSHAPI.9001"]
    fn PrfQueryProgramHandle(hini: u32, exe: *const u8, handles: *mut u32, cb: u32, count: *mut u32) -> u32;
}

#[repr(C)]
struct PrfProfile {
    cch_user: u32,
    psz_user: *mut u8,
    cch_sys: u32,
    psz_sys: *mut u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ProgDetails {
    length: u32,
    progc: u32,
    visible: u32,
    title: *const u8,
    exe: *const u8,
    params: *const u8,
    dir: *const u8,
    icon: *const u8,
    env: *const u8,
    swp: [u32; 9],
}

static FAILS: AtomicU32 = AtomicU32::new(0);

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {name}");
    } else {
        FAILS.fetch_add(1, Ordering::Relaxed);
        println!("FAIL {name}");
    }
}

fn z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().collect();
    v.push(0);
    v
}

fn ms() -> u32 {
    let mut t = 0;
    unsafe { DosQuerySysInfo(14, 14, &mut t, 4) };
    t
}

/// The value of app/key (PrfQueryProfileSize + PrfQueryProfileData).
fn data(hini: u32, app: &str, key: &str) -> Option<Vec<u8>> {
    let (a, k) = (z(app), z(key));
    let mut n = 0;
    if unsafe { PrfQueryProfileSize(hini, a.as_ptr(), k.as_ptr(), &mut n) } == 0 {
        return None;
    }
    let mut v = vec![0u8; n as usize];
    let ok = unsafe { PrfQueryProfileData(hini, a.as_ptr(), k.as_ptr(), v.as_mut_ptr(), &mut n) };
    (ok != 0 && n as usize == v.len()).then_some(v)
}

/// A string value (PrfQueryProfileString, no default).
fn string(hini: u32, app: &str, key: &str) -> Option<String> {
    let (a, k) = (z(app), z(key));
    let mut b = [0u8; 256];
    let n = unsafe { PrfQueryProfileString(hini, a.as_ptr(), k.as_ptr(), null(), b.as_mut_ptr(), b.len() as u32) };
    (n > 0).then(|| b[..n as usize - 1].iter().map(|&c| c as char).collect())
}

fn put(hini: u32, app: &str, key: &str, val: &str) -> bool {
    let (a, k, v) = (z(app), z(key), z(val));
    unsafe { PrfWriteProfileString(hini, a.as_ptr(), k.as_ptr(), v.as_ptr()) != 0 }
}

/// An enumeration through PrfQueryProfileSize + PrfQueryProfileData:
/// (the bytes, the length PrfQueryProfileData reported).
fn names(hini: u32, app: Option<&str>) -> Option<(Vec<u8>, u32)> {
    let a = app.map(z);
    let ap = a.as_ref().map_or(null(), |a| a.as_ptr());
    let mut n = 0;
    if unsafe { PrfQueryProfileSize(hini, ap, null(), &mut n) } == 0 {
        return None;
    }
    let mut v = vec![0u8; n as usize];
    let mut got = n;
    if unsafe { PrfQueryProfileData(hini, ap, null(), v.as_mut_ptr(), &mut got) } == 0 {
        return None;
    }
    Some((v, got))
}

fn split(list: &[u8]) -> Vec<String> {
    let mut v = Vec::new();
    for n in list.split(|&c| c == 0) {
        if n.is_empty() {
            break;
        }
        v.push(n.iter().map(|&c| c as char).collect());
    }
    v
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut act) = (0, 0);
    if unsafe { DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE, null_mut()) } != 0 {
        return None;
    }
    let mut v = vec![0u8; 256 * 1024];
    let mut n = 0;
    unsafe {
        DosRead(hf, v.as_mut_ptr(), v.len() as u32, &mut n);
        DosClose(hf);
    }
    v.truncate(n as usize);
    Some(v)
}

fn write_file(path: &str, data: &[u8]) -> bool {
    let p = z(path);
    let (mut hf, mut act) = (0, 0);
    if unsafe { DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, null_mut()) } != 0 {
        return false;
    }
    let mut n = 0;
    unsafe {
        DosWrite(hf, data.as_ptr(), data.len() as u32, &mut n);
        DosClose(hf);
    }
    n as usize == data.len()
}

fn exists(path: &str) -> bool {
    let p = z(path);
    let mut st = FileStatus3::default();
    unsafe { DosQueryPathInfo(p.as_ptr(), FIL_STANDARD, &mut st as *mut _ as *mut u8, 24) == 0 }
}

fn delete(path: &str) {
    let p = z(path);
    unsafe { DosDelete(p.as_ptr()) };
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// A file looks like an OS/2 profile: signature, size word.
fn is_ini(b: &[u8]) -> bool {
    b.len() >= 20 && u32_at(b, 0) == 0xffff_ffff && u32_at(b, 8) as usize == b.len() && u32_at(b, 4) == 0x14
}

fn basic_checks() {
    let file = "C:\\INITEST.INI";
    delete(file);
    delete("C:\\INITEST.!!!");
    let f = z(file);
    let h = unsafe { PrfOpenProfile(0, f.as_ptr()) };
    check("PrfOpenProfile creates a profile", h != 0 && read_file(file).is_some_and(|b| is_ini(&b) && b.len() == 20));
    let (a1, a2, k1, kb) = (z("App1"), z("App2"), z("Key1"), z("Bin"));
    unsafe {
        check("PrfWriteProfileString", PrfWriteProfileString(h, a1.as_ptr(), k1.as_ptr(), b"Hello\0".as_ptr()) != 0);
        check("PrfWriteProfileData", PrfWriteProfileData(h, a1.as_ptr(), kb.as_ptr(), [1u8, 2, 0, 3].as_ptr(), 4) != 0);
    }
    check("PrfWriteProfileString (number)", put(h, "App2", "Num", "  -123abc") && put(h, "App2", "Word", "abc"));
    let mut n = 0;
    unsafe {
        check("PrfQueryProfileSize string", PrfQueryProfileSize(h, a1.as_ptr(), k1.as_ptr(), &mut n) != 0 && n == 6);
        check("PrfQueryProfileSize data", PrfQueryProfileSize(h, a1.as_ptr(), kb.as_ptr(), &mut n) != 0 && n == 4);
        check("PrfQueryProfileSize missing key", PrfQueryProfileSize(h, a1.as_ptr(), b"Nope\0".as_ptr(), &mut n) == 0);
    }
    check("PrfQueryProfileData", data(h, "App1", "Bin").as_deref() == Some(&[1, 2, 0, 3][..]));
    let mut b = [0xeeu8; 8];
    let mut n = 2;
    let ok = unsafe { PrfQueryProfileData(h, a1.as_ptr(), kb.as_ptr(), b.as_mut_ptr(), &mut n) };
    check("PrfQueryProfileData too small: FALSE, part copied", ok == 0 && n == 2 && b[..3] == [1, 2, 0xee]);
    let mut b = [0u8; 16];
    let n = unsafe { PrfQueryProfileString(h, a1.as_ptr(), k1.as_ptr(), b"dflt\0".as_ptr(), b.as_mut_ptr(), 16) };
    check("PrfQueryProfileString", n == 6 && &b[..6] == b"Hello\0");
    let mut b = [0xeeu8; 8];
    let n = unsafe { PrfQueryProfileString(h, a1.as_ptr(), k1.as_ptr(), null(), b.as_mut_ptr(), 3) };
    check("PrfQueryProfileString truncates with a NUL", n == 3 && &b[..4] == b"He\0\xee");
    let n = unsafe { PrfQueryProfileString(h, a1.as_ptr(), b"Nope\0".as_ptr(), b"dflt\0".as_ptr(), b.as_mut_ptr(), 8) };
    check("PrfQueryProfileString default", n == 5 && &b[..5] == b"dflt\0");
    let n = unsafe { PrfQueryProfileString(h, a1.as_ptr(), b"Nope\0".as_ptr(), null(), b.as_mut_ptr(), 8) };
    check("PrfQueryProfileString no default: 0", n == 0);
    unsafe {
        check("PrfQueryProfileInt", PrfQueryProfileInt(h, a2.as_ptr(), b"Num\0".as_ptr(), 5) == -123);
        check("PrfQueryProfileInt not a number: 0", PrfQueryProfileInt(h, a2.as_ptr(), b"Word\0".as_ptr(), 5) == 0);
        check("PrfQueryProfileInt default", PrfQueryProfileInt(h, a2.as_ptr(), b"Nope\0".as_ptr(), 77) == 77);
    }
    check("names are case-sensitive", data(h, "app1", "Key1").is_none() && data(h, "App1", "KEY1").is_none() && put(h, "app1", "Key1", "lower") && string(h, "App1", "Key1").as_deref() == Some("Hello"));
    // Enumerations.
    let apps = names(h, None);
    check("enumerate applications", apps.as_ref().is_some_and(|(v, n)| v == b"App1\0App2\0app1\0\0" && *n as usize == v.len() - 1));
    let keys = names(h, Some("App1"));
    check("enumerate keys", keys.as_ref().is_some_and(|(v, _)| v == b"Key1\0Bin\0\0"));
    check("enumerate keys of a missing application", names(h, Some("Nope")).is_none());
    let mut b = [0xeeu8; 32];
    let n = unsafe { PrfQueryProfileString(h, null(), null(), b"dflt\0".as_ptr(), b.as_mut_ptr(), 32) };
    check("enumerate with PrfQueryProfileString", n == 15 && &b[..16] == b"App1\0App2\0app1\0\0");
    let mut b = [0xeeu8; 32];
    let mut n = 7;
    let ok = unsafe { PrfQueryProfileData(h, null(), null(), b.as_mut_ptr(), &mut n) };
    check("enumeration into a small buffer: FALSE, cut", ok == 0 && n == 7 && &b[..8] == b"App1\0Ap\xee");
    // Deleting.
    unsafe {
        check("delete a key (string NULL)", PrfWriteProfileString(h, a1.as_ptr(), k1.as_ptr(), null()) != 0 && data(h, "App1", "Key1").is_none() && data(h, "App1", "Bin").is_some());
        check("delete an application (key NULL)", PrfWriteProfileData(h, a2.as_ptr(), null(), null(), 0) != 0 && names(h, Some("App2")).is_none());
        check("delete what is not there", PrfWriteProfileString(h, a2.as_ptr(), k1.as_ptr(), null()) != 0);
        check("the last key takes its application", PrfWriteProfileString(h, b"app1\0".as_ptr(), k1.as_ptr(), null()) != 0 && names(h, None).is_some_and(|(v, _)| v == b"App1\0\0"));
    }
    // Sizes.
    let big = vec![0x5au8; 65535];
    unsafe {
        check("a 65535-byte value", PrfWriteProfileData(h, a1.as_ptr(), b"Big\0".as_ptr(), big.as_ptr(), 65535) != 0 && data(h, "App1", "Big").is_some_and(|v| v == big));
        check("a 65536-byte value is refused", PrfWriteProfileData(h, a1.as_ptr(), b"Big2\0".as_ptr(), big.as_ptr().cast(), 65536) == 0);
        check("an empty value", PrfWriteProfileData(h, a1.as_ptr(), b"Empty\0".as_ptr(), b"".as_ptr(), 0) != 0 && data(h, "App1", "Empty").is_some_and(|v| v.is_empty()));
        PrfWriteProfileData(h, a1.as_ptr(), b"Big\0".as_ptr(), null(), 0);
    }
    // On disk at once, as an INI file, no temporary file left.
    let disk = read_file(file);
    check("written through to the file", disk.as_ref().is_some_and(|b| is_ini(b) && b.windows(5).any(|w| w == b"App1\0") && !b.windows(4).any(|w| w == b"App2")));
    check("no temporary file left", !exists("C:\\INITEST.!!!"));
    unsafe {
        check("PrfCloseProfile", PrfCloseProfile(h) != 0);
        check("PrfCloseProfile again fails", PrfCloseProfile(h) == 0);
        check("a closed handle is invalid", PrfQueryProfileSize(h, a1.as_ptr(), kb.as_ptr(), &mut n) == 0 && put(h, "x", "y", "z") == false);
        check("PrfCloseProfile(HINI_USERPROFILE) fails", PrfCloseProfile(HINI_USER) == 0 && PrfCloseProfile(HINI_SYSTEM) == 0);
    }
    let h = unsafe { PrfOpenProfile(0, f.as_ptr()) };
    let h2 = unsafe { PrfOpenProfile(0, b"c:\\initest.ini\0".as_ptr()) };
    check("reopened: the data is there", h != 0 && data(h, "App1", "Bin").as_deref() == Some(&[1, 2, 0, 3][..]));
    check("the same file opened twice", h2 != 0 && h2 != h && put(h2, "Twice", "k", "v") && string(h, "Twice", "k").as_deref() == Some("v"));
    unsafe {
        PrfCloseProfile(h2);
        check("one of two handles closed: the other works", data(h, "Twice", "k").is_some());
        PrfCloseProfile(h);
    }
    // Not a profile: refused, left alone.
    write_file("C:\\INITEST4.INI", b"[section]\r\nkey=value\r\n");
    let h = unsafe { PrfOpenProfile(0, b"C:\\INITEST4.INI\0".as_ptr()) };
    check("a file that is not a profile is not opened", h == 0 && read_file("C:\\INITEST4.INI").as_deref() == Some(&b"[section]\r\nkey=value\r\n"[..]));
    delete("C:\\INITEST4.INI");
    // A crash between deleting the profile and renaming its replacement.
    let good = read_file(file).unwrap_or_default();
    delete("C:\\INITEST3.INI");
    write_file("C:\\INITEST3.!!!", &good);
    let h = unsafe { PrfOpenProfile(0, b"C:\\INITEST3.INI\0".as_ptr()) };
    check("recovered from the temporary file", h != 0 && data(h, "App1", "Bin").is_some() && exists("C:\\INITEST3.INI") && !exists("C:\\INITEST3.!!!"));
    unsafe { PrfCloseProfile(h) };
    // A stale temporary file next to a good profile is ignored.
    write_file("C:\\INITEST3.!!!", b"junk");
    let h = unsafe { PrfOpenProfile(0, b"C:\\INITEST3.INI\0".as_ptr()) };
    check("a stale temporary file is ignored", h != 0 && data(h, "App1", "Bin").is_some() && put(h, "After", "k", "v") && !exists("C:\\INITEST3.!!!"));
    unsafe { PrfCloseProfile(h) };
    delete("C:\\INITEST3.INI");
    // Speed: many writes, each one written through.
    let h = unsafe { PrfOpenProfile(0, f.as_ptr()) };
    let t0 = ms();
    let mut ok = true;
    for i in 0..200 {
        ok &= put(h, "Many", &format!("Key{i}"), "a value of some length, like those in OS2.INI");
    }
    let t = ms().wrapping_sub(t0);
    check("200 writes", ok && names(h, Some("Many")).is_some_and(|(v, _)| split(&v).len() == 200));
    println!("  200 writes took {t} ms");
    unsafe { PrfCloseProfile(h) };
}

fn profile_names() -> (String, String) {
    let mut p = PrfProfile { cch_user: 0, psz_user: null_mut(), cch_sys: 0, psz_sys: null_mut() };
    unsafe { PrfQueryProfile(0, &mut p) };
    let (mut u, mut s) = (vec![0u8; p.cch_user as usize], vec![0u8; p.cch_sys as usize]);
    p.psz_user = u.as_mut_ptr();
    p.psz_sys = s.as_mut_ptr();
    unsafe { PrfQueryProfile(0, &mut p) };
    let t = |v: &[u8]| v.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
    (t(&u), t(&s))
}

fn system_checks() {
    // PrfQueryProfile.
    let mut p = PrfProfile { cch_user: 0, psz_user: null_mut(), cch_sys: 0, psz_sys: null_mut() };
    let ok = unsafe { PrfQueryProfile(0, &mut p) };
    check("PrfQueryProfile lengths", ok != 0 && p.cch_user > 4 && p.cch_sys > 4);
    let (user, system) = profile_names();
    println!("  user profile {user}, system profile {system}");
    check("PrfQueryProfile names", user.to_ascii_uppercase().ends_with("\\OS2.INI") && system.to_ascii_uppercase().ends_with("\\OS2SYS.INI"));
    let mut small = [0u8; 4];
    let mut p = PrfProfile { cch_user: 4, psz_user: small.as_mut_ptr(), cch_sys: 0, psz_sys: null_mut() };
    let ok = unsafe { PrfQueryProfile(0, &mut p) };
    check("PrfQueryProfile small buffer: FALSE, cut", ok == 0 && small[3] == 0 && p.cch_user as usize == user.len() + 1);
    let u = z(&user);
    check("PrfOpenProfile of the user profile fails", unsafe { PrfOpenProfile(0, u.as_ptr()) } == 0);
    // HINI_PROFILE: the user profile first, then the system profile;
    // writes go to the user profile.
    check("write user and system", put(HINI_SYSTEM, "IniTest", "Where", "system") && put(HINI_SYSTEM, "IniTest", "OnlySys", "s") && put(HINI_USER, "IniTest", "Where", "user"));
    check("OS2.INI and OS2SYS.INI created on first use", exists(&user) && exists(&system) && read_file(&user).is_some_and(|b| is_ini(&b)));
    check("HINI_PROFILE reads the user profile first", string(HINI_PROFILE, "IniTest", "Where").as_deref() == Some("user"));
    check("HINI_PROFILE reads the system profile next", string(HINI_PROFILE, "IniTest", "OnlySys").as_deref() == Some("s"));
    check("HINI_PROFILE writes to the user profile", put(HINI_PROFILE, "IniTest", "Via", "p") && string(HINI_USER, "IniTest", "Via").as_deref() == Some("p") && string(HINI_SYSTEM, "IniTest", "Via").is_none());
    let keys = names(HINI_PROFILE, Some("IniTest")).map(|(v, _)| split(&v));
    check("HINI_PROFILE enumerates both", keys.as_deref() == Some(&["Where".into(), "Via".into(), "OnlySys".into()][..]));
    check("HINI_USERPROFILE / HINI_SYSTEMPROFILE alone", string(HINI_SYSTEM, "IniTest", "Where").as_deref() == Some("system") && string(HINI_USER, "IniTest", "OnlySys").is_none());
    check("on disk in OS2.INI and OS2SYS.INI", read_file(&user).is_some_and(|b| b.windows(5).any(|w| w == b"user\0")) && read_file(&system).is_some_and(|b| b.windows(7).any(|w| w == b"system\0")));
    unsafe {
        PrfWriteProfileString(HINI_SYSTEM, b"IniTest\0".as_ptr(), null(), null());
        PrfWriteProfileString(HINI_USER, b"IniTest\0".as_ptr(), null(), null());
    }
    check("cleaned up", names(HINI_PROFILE, Some("IniTest")).is_none());
    // PrfReset.
    delete("C:\\INITEST2.INI");
    let (nu, ns) = (z("C:\\INITEST2.INI"), z(&system));
    let p = PrfProfile { cch_user: nu.len() as u32, psz_user: nu.as_ptr() as *mut u8, cch_sys: ns.len() as u32, psz_sys: ns.as_ptr() as *mut u8 };
    check("PrfReset", unsafe { PrfReset(0, &p) } != 0);
    check("PrfQueryProfile after PrfReset", profile_names().0.eq_ignore_ascii_case("C:\\INITEST2.INI") && exists("C:\\INITEST2.INI"));
    check("the new user profile is used", put(HINI_USER, "IniTest", "Reset", "yes") && string(HINI_PROFILE, "IniTest", "Reset").as_deref() == Some("yes"));
    let bad = z("C:\\OTHER.INI");
    let p2 = PrfProfile { cch_user: u.len() as u32, psz_user: u.as_ptr() as *mut u8, cch_sys: bad.len() as u32, psz_sys: bad.as_ptr() as *mut u8 };
    check("PrfReset cannot change the system profile", unsafe { PrfReset(0, &p2) } == 0);
    let p3 = PrfProfile { cch_user: u.len() as u32, psz_user: u.as_ptr() as *mut u8, cch_sys: ns.len() as u32, psz_sys: ns.as_ptr() as *mut u8 };
    check("PrfReset back", unsafe { PrfReset(0, &p3) } != 0 && profile_names().0 == user && string(HINI_USER, "IniTest", "Reset").is_none());
    let h = unsafe { PrfOpenProfile(0, b"C:\\INITEST2.INI\0".as_ptr()) };
    check("the other profile kept its data", h != 0 && string(h, "IniTest", "Reset").as_deref() == Some("yes"));
    unsafe { PrfCloseProfile(h) };
    delete("C:\\INITEST2.INI");
}

/// The other process's side: reads what the parent wrote, writes back.
fn child() -> u32 {
    let mut bad = 0;
    if string(HINI_USER, "IniTest:Child", "FromParent").as_deref() != Some("p1") {
        bad |= 1;
    }
    let h = unsafe { PrfOpenProfile(0, b"C:\\INITEST.INI\0".as_ptr()) };
    if h == 0 || data(h, "App1", "Bin").as_deref() != Some(&[1, 2, 0, 3][..]) {
        bad |= 2;
    }
    if !put(h, "App1", "Child", "c2") {
        bad |= 4;
    }
    unsafe { PrfCloseProfile(h) };
    if !put(HINI_USER, "IniTest:Child", "FromChild", "c1") {
        bad |= 8;
    }
    if !profile_names().0.to_ascii_uppercase().ends_with("\\OS2.INI") {
        bad |= 16;
    }
    bad
}

fn process_checks() {
    let h = unsafe { PrfOpenProfile(0, b"C:\\INITEST.INI\0".as_ptr()) };
    // Cache the values in this process first.
    let _ = string(h, "App1", "Child");
    let _ = string(HINI_USER, "IniTest:Child", "FromChild");
    check("write for the other process", put(HINI_USER, "IniTest:Child", "FromParent", "p1"));
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, EXEC_SYNC, b"INITEST\0CHILD\0\0".as_ptr(), null(), &mut res, b"C:\\RS2\\INITEST.EXE\0".as_ptr()) };
    check("another process reads this process's writes", rc == 0 && res.code_result == 0);
    if res.code_result != 0 {
        println!("  child result {}", res.code_result);
    }
    check("this process reads the other's writes (user profile)", string(HINI_USER, "IniTest:Child", "FromChild").as_deref() == Some("c1"));
    check("this process reads the other's writes (open profile)", string(h, "App1", "Child").as_deref() == Some("c2"));
    unsafe {
        PrfCloseProfile(h);
        PrfWriteProfileString(HINI_USER, b"IniTest:Child\0".as_ptr(), null(), null());
    }
}

/// One of two processes writing to the same profiles at once.
fn writer(who: &[u8]) -> u32 {
    let who = core::str::from_utf8(who).unwrap_or("?");
    let h = unsafe { PrfOpenProfile(0, b"C:\\INITEST.INI\0".as_ptr()) };
    let mut bad = 0;
    for i in 0..60 {
        if !put(HINI_USER, "IniTest:Conc", &format!("{who}{i}"), "x") || !put(h, "Conc", &format!("{who}{i}"), "y") {
            bad += 1;
        }
    }
    unsafe { PrfCloseProfile(h) };
    bad
}

fn concurrent_checks() {
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, EXEC_ASYNCRESULT, b"INITEST\0WRITER\0\0".as_ptr(), null(), &mut res, b"C:\\RS2\\INITEST.EXE\0".as_ptr()) };
    let mine = writer(b"P");
    let mut pid = 0;
    let child = res.code_terminate;
    let wrc = unsafe { DosWaitChild(DCWA_PROCESS, DCWW_WAIT, &mut res, &mut pid, child) };
    check("two processes write at once", rc == 0 && wrc == 0 && mine == 0 && res.code_result == 0);
    let user = names(HINI_USER, Some("IniTest:Conc")).map_or(0, |(v, _)| split(&v).len());
    let h = unsafe { PrfOpenProfile(0, b"C:\\INITEST.INI\0".as_ptr()) };
    let file = names(h, Some("Conc")).map_or(0, |(v, _)| split(&v).len());
    unsafe { PrfCloseProfile(h) };
    println!("  keys after both: {user} in OS2.INI, {file} in INITEST.INI");
    check("no write of either process lost", user == 120 && file == 120);
    unsafe { PrfWriteProfileString(HINI_USER, b"IniTest:Conc\0".as_ptr(), null(), null()) };
}

fn details(title: &[u8], exe: &[u8], params: &[u8], env: &[u8]) -> ProgDetails {
    ProgDetails {
        length: 72,
        progc: 2, // PROG_WINDOWABLEVIO
        visible: 0,
        title: title.as_ptr(),
        exe: exe.as_ptr(),
        params: params.as_ptr(),
        dir: b"C:\\\0".as_ptr(),
        icon: null(),
        env: env.as_ptr(),
        swp: [0x8f, 300, 400, 10, 20, 3, 0, 0, 0],
    }
}

fn cstr(p: u32) -> Option<Vec<u8>> {
    (p != 0).then(|| (0..).map(|i| unsafe { *(p as *const u8).add(i) }).take_while(|&c| c != 0).collect())
}

fn titles(group: u32) -> Option<Vec<(u32, u32, Vec<u8>)>> {
    let mut count = 0;
    let need = unsafe { PrfQueryProgramTitles(HINI_USER, group, null_mut(), 0, &mut count) };
    if need == 0 {
        return None;
    }
    let mut b = vec![0u32; need as usize / 4 + 1];
    let got = unsafe { PrfQueryProgramTitles(HINI_USER, group, b.as_mut_ptr().cast(), need, &mut count) };
    if got != need {
        return None;
    }
    Some((0..count as usize).map(|i| (b[i * 4], b[i * 4 + 1], cstr(b[i * 4 + 3]).unwrap_or_default())).collect())
}

fn program_list_checks() {
    let g = unsafe { PrfCreateGroup(HINI_USER, b"IniTest Group\0".as_ptr(), 0) };
    check("PrfCreateGroup", g != 0 && g != SGH_ROOT);
    check("PrfCreateGroup with the same title: the same group", unsafe { PrfCreateGroup(HINI_USER, b"IniTest Group\0".as_ptr(), 0) } == g);
    let d = details(b"IniTest CMD\0", b"C:\\OS2\\CMD.EXE\0", b"/K VER\0", b"A=1\0B=2\0\0");
    let p = unsafe { PrfAddProgram(HINI_USER, &d, g) };
    check("PrfAddProgram", p != 0 && p != g);
    check("PrfAddProgram to a group that does not exist fails", unsafe { PrfAddProgram(HINI_USER, &d, 0x7777) } == 0);
    let t = titles(g);
    check("PrfQueryProgramTitles of the group", t.as_ref().is_some_and(|t| t.len() == 1 && t[0].0 == p && t[0].1 == 2 && t[0].2 == b"IniTest CMD"));
    let root = titles(SGH_ROOT);
    check("PrfQueryProgramTitles of SGH_ROOT lists the group", root.as_ref().is_some_and(|t| t.iter().any(|(h, c, n)| *h == g && *c == 5 && n == b"IniTest Group")));
    let mut count = 0;
    let mut small = [0u8; 8];
    check("PrfQueryProgramTitles small buffer: 0", unsafe { PrfQueryProgramTitles(HINI_USER, g, small.as_mut_ptr(), 8, &mut count) } == 0 && count == 1);
    let need = unsafe { PrfQueryDefinition(HINI_USER, p, null_mut(), 0) };
    check("PrfQueryDefinition size", need as usize == 72 + 12 + 15 + 7 + 4 + 9);
    let mut b = vec![0u32; need as usize / 4 + 1];
    let got = unsafe { PrfQueryDefinition(HINI_USER, p, b.as_mut_ptr().cast(), need) };
    let env = (b[8] != 0).then(|| unsafe { core::slice::from_raw_parts(b[8] as *const u8, 9) }.to_vec());
    check(
        "PrfQueryDefinition",
        got == need
            && b[0] == 72
            && b[1] == 2
            && cstr(b[3]).as_deref() == Some(b"IniTest CMD")
            && cstr(b[4]).as_deref() == Some(b"C:\\OS2\\CMD.EXE")
            && cstr(b[5]).as_deref() == Some(b"/K VER")
            && cstr(b[6]).as_deref() == Some(b"C:\\")
            && b[7] == 0
            && env.as_deref() == Some(b"A=1\0B=2\0\0")
            && b[9..18] == [0x8f, 300, 400, 10, 20, 3, 0, 0, 0]
            && (b[3] as usize) >= b.as_ptr() as usize + 72,
    );
    check("PrfQueryDefinition small buffer: 0", unsafe { PrfQueryDefinition(HINI_USER, p, b.as_mut_ptr().cast(), 72) } == 0);
    let d2 = details(b"IniTest Shell\0", b"C:\\OS2\\CMD.EXE\0", b"\0", b"\0");
    check("PrfChangeProgram", unsafe { PrfChangeProgram(HINI_USER, p, &d2) } != 0 && titles(g).is_some_and(|t| t[0].2 == b"IniTest Shell"));
    let mut hs = [0u32; 8];
    let mut n = 0;
    let ok = unsafe { PrfQueryProgramHandle(HINI_USER, b"CMD.EXE\0".as_ptr(), hs.as_mut_ptr(), 32, &mut n) };
    check("PrfQueryProgramHandle", ok != 0 && hs[..n as usize].contains(&p));
    check("PrfQueryProgramHandle of an unknown program", unsafe { PrfQueryProgramHandle(HINI_USER, b"NOSUCH.EXE\0".as_ptr(), hs.as_mut_ptr(), 32, &mut n) } == 0 && n == 0);
    let user = profile_names().0;
    check("the program list is in OS2.INI", read_file(&user).is_some_and(|f| f.windows(18).any(|w| w == b"PM_RS2_ProgramList") && f.windows(13).any(|w| w == b"IniTest Shell")));
    check("PrfRemoveProgram", unsafe { PrfRemoveProgram(HINI_USER, p) } != 0 && unsafe { PrfQueryDefinition(HINI_USER, p, null_mut(), 0) } == 0);
    let p2 = unsafe { PrfAddProgram(HINI_USER, &d, g) };
    check("a removed handle is not reused", p2 != 0 && p2 != p);
    check("PrfRemoveProgram does not remove groups", unsafe { PrfRemoveProgram(HINI_USER, g) } == 0);
    check("PrfDestroyGroup", unsafe { PrfDestroyGroup(HINI_USER, g) } != 0 && titles(g).is_none() && unsafe { PrfQueryDefinition(HINI_USER, p2, null_mut(), 0) } == 0);
    check("PrfDestroyGroup again fails", unsafe { PrfDestroyGroup(HINI_USER, g) } == 0);
}

/// The drive of the test disk (label RS2DISK or RS2HPFS).
fn test_drive() -> Option<char> {
    (b'D'..=b'Z').find_map(|d| {
        let mut fsi: FsInfo = unsafe { core::mem::zeroed() };
        let rc = unsafe { DosQueryFSInfo((d - b'A' + 1) as u32, FSIL_VOLSER, &mut fsi as *mut _ as *mut u8, 17) };
        let label = &fsi.label[..(fsi.cch_label as usize).min(11)];
        (rc == 0 && (label == b"RS2DISK" || label == b"RS2HPFS")).then_some(d as char)
    })
}

const PERSIST_BIN: [u8; 6] = [0, 1, 2, 0xfe, 0xff, 0];

fn persist(write: bool) {
    let Some(d) = test_drive() else {
        check("test disk (label RS2DISK or RS2HPFS) present", false);
        return;
    };
    let file = format!("{d}:\\PERSIST.INI");
    println!("  profile {file}");
    let f = z(&file);
    if write {
        delete(&file);
        let h = unsafe { PrfOpenProfile(0, f.as_ptr()) };
        let mut ok = h != 0 && put(h, "Persist", "Text", "written before the reboot") && put(h, "Persist", "Number", "4711");
        ok &= unsafe { PrfWriteProfileData(h, b"Binary\0".as_ptr(), b"Bytes\0".as_ptr(), PERSIST_BIN.as_ptr(), 6) } != 0;
        let g = unsafe { PrfCreateGroup(h, b"Persisted Group\0".as_ptr(), 0) };
        let d = details(b"Persisted CMD\0", b"C:\\OS2\\CMD.EXE\0", b"\0", b"\0");
        ok &= g != 0 && unsafe { PrfAddProgram(h, &d, g) } != 0;
        check("write a profile on the test disk", ok);
        check("PrfCloseProfile", unsafe { PrfCloseProfile(h) } != 0);
        check("the file is a profile", read_file(&file).is_some_and(|b| is_ini(&b)));
    } else {
        check("the profile is still there", read_file(&file).is_some_and(|b| is_ini(&b)));
        let h = unsafe { PrfOpenProfile(0, f.as_ptr()) };
        check("read text after the reboot", h != 0 && string(h, "Persist", "Text").as_deref() == Some("written before the reboot"));
        check("read a number after the reboot", unsafe { PrfQueryProfileInt(h, b"Persist\0".as_ptr(), b"Number\0".as_ptr(), 0) } == 4711);
        check("read binary data after the reboot", data(h, "Binary", "Bytes").as_deref() == Some(&PERSIST_BIN[..]));
        let mut count = 0;
        let need = unsafe { PrfQueryProgramTitles(h, SGH_ROOT, null_mut(), 0, &mut count) };
        let mut b = vec![0u32; need as usize / 4 + 1];
        let got = unsafe { PrfQueryProgramTitles(h, SGH_ROOT, b.as_mut_ptr().cast(), need, &mut count) };
        let g = b[0];
        check("read the program list after the reboot", need != 0 && got == need && count == 1 && cstr(b[3]).as_deref() == Some(b"Persisted Group"));
        let mut n2 = 0;
        let need2 = unsafe { PrfQueryProgramTitles(h, g, null_mut(), 0, &mut n2) };
        let mut b2 = vec![0u32; need2 as usize / 4 + 1];
        unsafe { PrfQueryProgramTitles(h, g, b2.as_mut_ptr().cast(), need2, &mut n2) };
        check("read the program in the group", n2 == 1 && cstr(b2[3]).as_deref() == Some(b"Persisted CMD"));
        unsafe { PrfCloseProfile(h) };
    }
}

fn main(s: &Startup) -> u32 {
    let args: Vec<u8> = s.args().iter().copied().skip_while(|&c| c == b' ').collect();
    let mode: Vec<u8> = args.iter().take_while(|&&c| c != b' ').map(|c| c.to_ascii_uppercase()).collect();
    let name = match mode.as_slice() {
        b"CHILD" => return child(),
        b"WRITER" => return writer(b"C"),
        b"WRITE" => {
            persist(true);
            "initest write"
        }
        b"READ" => {
            persist(false);
            "initest read"
        }
        _ => {
            basic_checks();
            system_checks();
            process_checks();
            concurrent_checks();
            program_list_checks();
            "initest"
        }
    };
    let f = FAILS.load(Ordering::Relaxed);
    println!("{name} done: {f} failure(s)");
    f
}
