//! The program list on the desktop: groups and programs added to the
//! user profile's program list (PrfCreateGroup, PrfAddProgram ...) show
//! as Workplace Shell objects, as on OS/2 2.x and later, where installers
//! written for OS/2 1.x make their group with these calls (IBM's PMSHAPI
//! posts each change to the shell, ShpxPostChange). A group is a folder
//! on the desktop (object ID `<PRF_GROUP_hhhhhhhh>`, the group's handle),
//! a program a program object in it (`<PRF_PROG_hhhhhhhh>`); removing
//! them from the list deletes the objects. PMWP is loaded when first
//! needed; without a running shell nothing happens (PMWP's
//! WinCreateObject fails for these locations).

use alloc::vec::Vec;

use inifile::proglist::{Entry, TITLE};

/// A PMWP entry point (PMWP is loaded when first needed).
fn wp(ord: u32) -> usize {
    static MODULE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    let mut m = MODULE.load(core::sync::atomic::Ordering::Relaxed);
    if m == 0 {
        let mut fail = [0u8; 32];
        if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), 32, b"PMWP\0".as_ptr(), &mut m) } != 0 {
            return 0;
        }
        MODULE.store(m, core::sync::atomic::Ordering::Relaxed);
    }
    let mut f = 0usize;
    if unsafe { os2::DosQueryProcAddr(m, ord, core::ptr::null(), &mut f) } != 0 {
        return 0;
    }
    f
}

const CO_REPLACEIFEXISTS: u32 = 1;
const CO_UPDATEIFEXISTS: u32 = 2;
const SHE_INVISIBLE: u32 = 1;

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

/// WinCreateObject(class, title, setup, location, flags).
fn create(class: &[u8], title: &[u8], setup: &[u8], location: &[u8], flags: u32) -> u32 {
    let f = wp(281);
    if f == 0 {
        return 0;
    }
    let f: extern "C" fn(*const u8, *const u8, *const u8, *const u8, u32) -> u32 = unsafe { core::mem::transmute(f) };
    let (c, t, s, l) = (z(class), z(title), z(setup), z(location));
    f(c.as_ptr(), t.as_ptr(), s.as_ptr(), l.as_ptr(), flags)
}

/// WinDestroyObject(WinQueryObject(id)).
fn destroy(id: &[u8]) {
    let (q, d) = (wp(252), wp(251));
    if q == 0 || d == 0 {
        return;
    }
    let q: extern "C" fn(*const u8) -> u32 = unsafe { core::mem::transmute(q) };
    let d: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(d) };
    let id = z(id);
    let h = q(id.as_ptr());
    if h != 0 {
        d(h);
    }
}

fn id(kind: &[u8], h: u32) -> Vec<u8> {
    let mut v = [b"<PRF_", kind, b"_"].concat();
    for i in (0..8).rev() {
        v.push(b"0123456789ABCDEF"[(h >> (4 * i) & 15) as usize]);
    }
    v.push(b'>');
    v
}

/// A group's folder on the desktop.
pub fn group(h: u32, e: &Entry) {
    if e.visible & SHE_INVISIBLE != 0 {
        return;
    }
    let title = e.fields[TITLE].as_deref().unwrap_or(b"");
    let setup = [b"OBJECTID=", &id(b"GROUP", h)[..], b";"].concat();
    create(b"WPFolder", title, &setup, b"<WP_DESKTOP>", CO_UPDATEIFEXISTS);
}

/// A program object in its group's folder (made again when the program
/// changes).
pub fn program(h: u32, e: &Entry) {
    if e.visible & SHE_INVISIBLE != 0 {
        destroy(&id(b"PROG", h));
        return;
    }
    let title = e.fields[TITLE].as_deref().unwrap_or(b"");
    let mut setup = Vec::new();
    for (k, f) in [(&b"EXENAME="[..], 1), (b"PARAMETERS=", 2), (b"STARTUPDIR=", 3), (b"ICONFILE=", 4)] {
        if let Some(v) = e.fields[f].as_deref().filter(|v| !v.is_empty()) {
            setup.extend_from_slice(&[k, v, b";"].concat());
        }
    }
    let progtype: &[u8] = match e.progc {
        1 => b"FULLSCREEN",
        2 => b"WINDOWABLEVIO",
        3 => b"PM",
        4 => b"VDM",
        7 => b"WINDOWEDVDM",
        _ => b"",
    };
    if !progtype.is_empty() {
        setup.extend_from_slice(&[b"PROGTYPE=", progtype, b";"].concat());
    }
    setup.extend_from_slice(&[b"OBJECTID=", &id(b"PROG", h)[..], b";"].concat());
    create(b"WPProgram", title, &setup, &id(b"GROUP", e.group), CO_REPLACEIFEXISTS);
}

pub fn remove_program(h: u32) {
    destroy(&id(b"PROG", h));
}

/// A group's folder (with its programs).
pub fn remove_group(h: u32) {
    destroy(&id(b"GROUP", h));
}
