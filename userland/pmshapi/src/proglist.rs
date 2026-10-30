//! The program list: groups and programs (PROGDETAILS) kept in a profile
//! (the user profile for HINI_PROFILE), in the layout of
//! `inifile::proglist` (documented in docs/PM.md), so a desktop shell can
//! read it back from OS2.INI.
//!
//! PROGDETAILS (72 bytes): Length, PROGTYPE (progc, fbVisible), pszTitle,
//! pszExecutable, pszParameters, pszStartupDir, pszIcon, pszEnvironment
//! (strings ending with an empty one), SWP swpInitial (9 ULONGs).
//! PROGTITLE (16 bytes): hprog, PROGTYPE, pszTitle. Queries put the
//! strings after the structures in the caller's buffer.

use alloc::vec::Vec;

use inifile::Profile;
use inifile::proglist::{APP, ENVIRONMENT, EXECUTABLE, Entry, NEXT, PROG_GROUP, SGH_ROOT, TITLE, handle, key};

use crate::desktop;
use crate::prf::{self, HINI_PROFILE, HINI_USERPROFILE, cbytes};

const PROGDETAILS: usize = 72;
const PROGTITLE: usize = 16;

fn target(hini: u32) -> u32 {
    if hini == HINI_PROFILE { HINI_USERPROFILE } else { hini }
}

/// Changes to the user profile's list show on the desktop (desktop.rs).
fn on_desktop(hini: u32) -> bool {
    target(hini) == HINI_USERPROFILE
}

/// Entries of the program list in key order.
fn entries(p: &Profile) -> Vec<(u32, Entry)> {
    let Some(a) = p.app(APP) else { return Vec::new() };
    a.keys.iter().filter_map(|k| Some((handle(&k.name)?, Entry::parse(&k.data)?))).collect()
}

fn entry(p: &Profile, h: u32) -> Option<Entry> {
    Entry::parse(p.get(APP, &key(h))?)
}

fn is_group(p: &Profile, h: u32) -> bool {
    entry(p, h).is_some_and(|e| e.is_group())
}

/// A new handle (never reused: NextHandle counts on).
fn new_handle(p: &mut Profile) -> u32 {
    let stored = p.get(APP, NEXT).and_then(|v| v.get(..4)).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(1);
    let max = entries(p).iter().map(|(h, _)| *h).max().unwrap_or(0);
    let mut h = stored.max(max.wrapping_add(1)).max(1);
    if h == SGH_ROOT {
        h = 1;
    }
    let _ = p.set(APP, NEXT, &(h + 1).to_le_bytes());
    h
}

fn put(p: &mut Profile, h: u32, e: &Entry) -> Option<()> {
    p.set(APP, &key(h), &e.to_bytes()).ok()
}

fn ru32(p: *const u8, i: usize) -> u32 {
    unsafe { (p as *const u32).add(i).read_unaligned() }
}

/// The environment list at `p`: strings up to an empty one, each kept
/// with its NUL.
unsafe fn env_bytes(mut p: *const u8) -> Vec<u8> {
    let mut v = Vec::new();
    loop {
        let Some(s) = (unsafe { cbytes(p) }) else { break };
        if s.is_empty() || v.len() > 0xfff0 {
            break;
        }
        v.extend_from_slice(s);
        v.push(0);
        p = unsafe { p.add(s.len() + 1) };
    }
    v
}

/// An entry from a caller's PROGDETAILS.
unsafe fn from_details(d: *const u8, group: u32) -> Entry {
    let mut e = Entry { group, progc: ru32(d, 1), visible: ru32(d, 2), ..Entry::default() };
    for (i, f) in e.fields.iter_mut().enumerate() {
        let p = ru32(d, 3 + i) as *const u8;
        *f = if i == ENVIRONMENT {
            (!p.is_null()).then(|| unsafe { env_bytes(p) })
        } else {
            unsafe { cbytes(p) }.map(|s| s.to_vec())
        };
    }
    for (i, w) in e.swp.iter_mut().enumerate() {
        *w = ru32(d, 9 + i);
    }
    e
}

/// The bytes the strings of `e` take after the structure (each with a
/// NUL; for the environment that NUL ends the list).
fn strings_size(e: &Entry) -> usize {
    e.fields.iter().map(|f| f.as_ref().map_or(0, |s| s.len() + 1)).sum()
}

/// PrfAddProgram(hini, pDetails, hprogGroup): a new program in a group;
/// its handle, or 0.
#[unsafe(no_mangle)]
pub extern "C" fn PrfAddProgram(hini: u32, details: *const u8, group: u32) -> u32 {
    if details.is_null() {
        return 0;
    }
    let e = unsafe { from_details(details, group) };
    if e.progc == PROG_GROUP {
        return 0;
    }
    let h = prf::write(target(hini), |p| {
        if !is_group(p, group) {
            return None;
        }
        let h = new_handle(p);
        put(p, h, &e)?;
        Some(h)
    })
    .unwrap_or(0);
    if h != 0 && on_desktop(hini) {
        desktop::program(h, &e);
    }
    h
}

/// PrfChangeProgram(hini, hprog, pDetails): replace a program's details
/// (a group keeps its kind and place and takes the new title and
/// visibility).
#[unsafe(no_mangle)]
pub extern "C" fn PrfChangeProgram(hini: u32, hprog: u32, details: *const u8) -> u32 {
    if details.is_null() {
        return 0;
    }
    let Some(e) = prf::write(target(hini), |p| {
        let old = entry(p, hprog)?;
        let mut e = unsafe { from_details(details, old.group) };
        if old.is_group() {
            e.progc = PROG_GROUP;
        } else if e.progc == PROG_GROUP {
            return None;
        }
        put(p, hprog, &e)?;
        Some(e)
    }) else {
        return 0;
    };
    if on_desktop(hini) {
        if e.is_group() { desktop::group(hprog, &e) } else { desktop::program(hprog, &e) }
    }
    1
}

/// PrfQueryDefinition(hini, hprog, pDetails, cchBufferMax): the bytes a
/// PROGDETAILS with its strings takes; with a buffer that large, fills
/// it (strings after the structure, NULL for absent ones). 0 for an
/// unknown handle or a buffer too small.
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryDefinition(hini: u32, hprog: u32, details: *mut u8, cb: u32) -> u32 {
    let Some(e) = prf::read(target(hini), |ps| entry(ps.first()?, hprog)).flatten() else { return 0 };
    let need = PROGDETAILS + strings_size(&e);
    if details.is_null() || cb == 0 {
        return need as u32;
    }
    if (cb as usize) < need {
        return 0;
    }
    let mut words = [0u32; 18];
    words[0] = PROGDETAILS as u32;
    words[1] = e.progc;
    words[2] = e.visible;
    words[9..18].copy_from_slice(&e.swp);
    let mut at = PROGDETAILS;
    for (i, f) in e.fields.iter().enumerate() {
        let Some(s) = f else { continue };
        let dst = unsafe { details.add(at) };
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), dst, s.len());
            *dst.add(s.len()) = 0;
        }
        at += s.len() + 1;
        words[3 + i] = dst as u32;
    }
    for (i, w) in words.iter().enumerate() {
        unsafe { (details as *mut u32).add(i).write_unaligned(*w) };
    }
    need as u32
}

/// PrfRemoveProgram(hini, hprog): a program (not a group).
#[unsafe(no_mangle)]
pub extern "C" fn PrfRemoveProgram(hini: u32, hprog: u32) -> u32 {
    let ok = prf::write(target(hini), |p| {
        if entry(p, hprog)?.is_group() {
            return None;
        }
        p.remove_key(APP, &key(hprog)).then_some(())
    })
    .is_some();
    if ok && on_desktop(hini) {
        desktop::remove_program(hprog);
    }
    ok as u32
}

/// PrfQueryProgramTitles(hini, hprogGroup, pTitles, cchBufferMax,
/// pulCount): the programs of a group (SGH_ROOT: the groups) as
/// PROGTITLEs followed by their titles. Returns the bytes that takes
/// (and the count) when pTitles is NULL or cchBufferMax 0; else fills
/// the buffer and returns the same, or 0 if it is too small.
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProgramTitles(hini: u32, group: u32, titles: *mut u8, cb: u32, count: *mut u32) -> u32 {
    let list = prf::read(target(hini), |ps| {
        let p = ps.first()?;
        if group != SGH_ROOT && !is_group(p, group) {
            return None;
        }
        Some(entries(p).into_iter().filter(|(_, e)| if group == SGH_ROOT { e.is_group() } else { e.group == group }).collect::<Vec<_>>())
    })
    .flatten();
    let Some(list) = list else {
        if !count.is_null() {
            unsafe { count.write_unaligned(0) };
        }
        return 0;
    };
    if !count.is_null() {
        unsafe { count.write_unaligned(list.len() as u32) };
    }
    let need = list.len() * PROGTITLE + list.iter().map(|(_, e)| e.fields[TITLE].as_ref().map_or(1, |t| t.len() + 1)).sum::<usize>();
    if titles.is_null() || cb == 0 {
        return need as u32;
    }
    if (cb as usize) < need {
        return 0;
    }
    let mut at = list.len() * PROGTITLE;
    for (i, (h, e)) in list.iter().enumerate() {
        let t = e.fields[TITLE].as_deref().unwrap_or(b"");
        let s = unsafe { titles.add(at) };
        unsafe {
            core::ptr::copy_nonoverlapping(t.as_ptr(), s, t.len());
            *s.add(t.len()) = 0;
        }
        at += t.len() + 1;
        for (j, w) in [*h, e.progc, e.visible, s as u32].into_iter().enumerate() {
            unsafe { (titles as *mut u32).add(i * 4 + j).write_unaligned(w) };
        }
    }
    need as u32
}

/// PrfCreateGroup(hini, pszTitle, chVisibility): a new group, or the
/// handle of the group with that title if there is one.
#[unsafe(no_mangle)]
pub extern "C" fn PrfCreateGroup(hini: u32, title: *const u8, visibility: u32) -> u32 {
    let Some(t) = (unsafe { cbytes(title) }).filter(|t| !t.is_empty()) else { return 0 };
    let Some((h, e)) = prf::write(target(hini), |p| {
        if let Some(found) = entries(p).into_iter().find(|(_, e)| e.is_group() && e.fields[TITLE].as_deref() == Some(t)) {
            return Some(found);
        }
        let mut e = Entry { group: SGH_ROOT, progc: PROG_GROUP, visible: visibility & 0xff, ..Entry::default() };
        e.fields[TITLE] = Some(t.to_vec());
        let h = new_handle(p);
        put(p, h, &e)?;
        Some((h, e))
    }) else {
        return 0;
    };
    if on_desktop(hini) {
        desktop::group(h, &e);
    }
    h
}

/// PrfDestroyGroup(hini, hprogGroup): a group and its programs.
#[unsafe(no_mangle)]
pub extern "C" fn PrfDestroyGroup(hini: u32, group: u32) -> u32 {
    let ok = prf::write(target(hini), |p| {
        if !is_group(p, group) {
            return None;
        }
        for (h, e) in entries(p) {
            if h == group || e.group == group {
                p.remove_key(APP, &key(h));
            }
        }
        Some(())
    })
    .is_some();
    if ok && on_desktop(hini) {
        desktop::remove_group(group);
    }
    ok as u32
}

fn same_exe(stored: &[u8], wanted: &[u8]) -> bool {
    let base = |s: &[u8]| -> Vec<u8> { s[s.iter().rposition(|&c| c == b'\\' || c == b':').map_or(0, |i| i + 1)..].to_ascii_uppercase() };
    if wanted.iter().any(|&c| c == b'\\' || c == b':') {
        stored.eq_ignore_ascii_case(wanted)
    } else {
        base(stored) == base(wanted)
    }
}

/// PrfQueryProgramHandle(hini, pszExe, phprogArray, cchBufferMax,
/// pulCount): the handles of the programs with that executable (a name
/// without a path matches any directory). TRUE if there were any and
/// they all fit.
#[unsafe(no_mangle)]
pub extern "C" fn PrfQueryProgramHandle(hini: u32, exe: *const u8, handles: *mut u32, cb: u32, count: *mut u32) -> u32 {
    let Some(x) = (unsafe { cbytes(exe) }).filter(|x| !x.is_empty()) else { return 0 };
    let found: Vec<u32> = prf::read(target(hini), |ps| {
        ps.first().map_or(Vec::new(), |p| {
            entries(p).into_iter().filter(|(_, e)| !e.is_group() && e.fields[EXECUTABLE].as_deref().is_some_and(|s| same_exe(s, x))).map(|(h, _)| h).collect()
        })
    })
    .unwrap_or_default();
    if !count.is_null() {
        unsafe { count.write_unaligned(found.len() as u32) };
    }
    let room = if handles.is_null() { 0 } else { cb as usize / 4 };
    for (i, h) in found.iter().take(room).enumerate() {
        unsafe { handles.add(i).write_unaligned(*h) };
    }
    (!found.is_empty() && found.len() <= room) as u32
}
