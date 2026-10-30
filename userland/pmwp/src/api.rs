//! The Workplace Shell's API for programs: objects (WinCreateObject,
//! WinQueryObject, WinSetObjectData, WinDestroyObject, WinOpenObject,
//! WinSaveObject, WinMoveObject, WinCopyObject, WinCreateShadow,
//! WinQueryObjectPath) and classes (WinRegisterObjectClass,
//! WinDeregisterObjectClass, WinEnumObjectClasses,
//! WinReplaceObjectClass).
//!
//! They work on the objects of the Workplace Shell running in the calling
//! process (Rs2WpsStart; the shell process calls it). Without one,
//! WinCreateObject keeps adding WPProgram objects to RS2SHELL's desktop
//! (see lib.rs), WinQueryObject reads OS2.INI, and the class calls edit
//! the class table. With a shell in another process, the calls go to it
//! (ipc.rs).

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::som;
use crate::util::*;
use crate::{clsmgr, ipc, obj, wps};

/// A call for the shell in another process (None: no shell there, or
/// the shell is this process).
fn remote(op: u32, nums: &[u32], strs: &[&[u8]]) -> Option<u32> {
    if wps::active() {
        return None;
    }
    ipc::call(op, nums, strs)
}

const CO_FAILIFEXISTS: u32 = 0;
const CO_REPLACEIFEXISTS: u32 = 1;
const CO_UPDATEIFEXISTS: u32 = 2;

/// The object ID in a setup string.
fn setup_object_id(setup: &[u8]) -> Option<Vec<u8>> {
    obj::setup_pairs(setup).into_iter().find(|(k, _)| k == b"OBJECTID").map(|(_, v)| v)
}

/// Start the Workplace Shell's objects in this process (rs2-private).
#[unsafe(no_mangle)]
pub extern "C" fn Rs2WpsStart() -> u32 {
    wps::start() as u32
}

pub fn create_object(class: &[u8], title: &[u8], setup: &[u8], location: &[u8], flags: u32) -> u32 {
    let cls = som::find_class(class);
    if cls == 0 {
        debug(&alloc::format!("WinCreateObject: no class {:?}", core::str::from_utf8(class)));
        return 0;
    }
    let folder = wps::object_from_location(location);
    if folder == 0 || !wps::is(folder, "WPFolder") {
        debug(&alloc::format!("WinCreateObject: no folder {:?} ({:x})", core::str::from_utf8(location), folder));
        return 0;
    }
    if let Some(id) = setup_object_id(setup) {
        if let Some(h) = wps::handle_of_id(&id) {
            let old = wps::object_from_handle(h);
            if old != 0 {
                match flags & 3 {
                    CO_FAILIFEXISTS => return 0,
                    CO_UPDATEIFEXISTS => {
                        let s = z(setup);
                        obj::call(m::WPObject::wpSetup, &[old, s.as_ptr() as usize]);
                        obj::call(m::WPObject::wpSaveImmediate, &[old]);
                        return h;
                    }
                    CO_REPLACEIFEXISTS => {
                        obj::call(m::WPObject::wpFree, &[old]);
                    }
                    _ => {}
                }
            }
        }
    }
    let (t, s) = (z(title), z(setup));
    let o = wps::call("M_WPObject", m::M_WPObject::wpclsNew, &[cls, t.as_ptr() as usize, s.as_ptr() as usize, folder, 0]);
    debug(&alloc::format!("WinCreateObject: class {:x} folder {:x} -> {:x}", cls, folder, o));
    if o == 0 { 0 } else { obj::handle(o) }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryObject(id: *const u8) -> u32 {
    let id = cstr(id);
    if let Some(r) = remote(ipc::OP_QUERY, &[], &[id]) {
        return r;
    }
    if wps::active() {
        let o = wps::object_from_location(id);
        return if o == 0 { 0 } else { obj::handle(o) };
    }
    if id.starts_with(b"<") {
        return wps::handle_of_id(id).unwrap_or(0);
    }
    if id.len() >= 2 && id[1] == b':' && file_exists(id) {
        return wps::fs_handle(id);
    }
    0
}

fn object(h: u32) -> usize {
    if !wps::active() {
        return 0;
    }
    wps::object_from_handle(h)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetObjectData(h: u32, setup: *const u8) -> u32 {
    if let Some(r) = remote(ipc::OP_SETDATA, &[h], &[cstr(setup)]) {
        return r;
    }
    let o = object(h);
    if o == 0 {
        return 0;
    }
    let r = obj::call(m::WPObject::wpSetup, &[o, setup as usize]) as u32;
    obj::call(m::WPObject::wpSaveImmediate, &[o]);
    r
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyObject(h: u32) -> u32 {
    if let Some(r) = remote(ipc::OP_DESTROY, &[h], &[]) {
        return r;
    }
    let o = object(h);
    if o == 0 {
        return 0;
    }
    (obj::call(m::WPObject::wpDelete, &[o, 0]) != 0) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinOpenObject(h: u32, view: u32, _flag: u32) -> u32 {
    if let Some(r) = remote(ipc::OP_OPEN, &[h, view, _flag], &[]) {
        return r;
    }
    let o = object(h);
    if o == 0 {
        return 0;
    }
    obj::call(m::WPObject::wpViewObject, &[o, 0, view as usize, 0]);
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSaveObject(h: u32, _async: u32) -> u32 {
    if let Some(r) = remote(ipc::OP_SAVE, &[h, _async], &[]) {
        return r;
    }
    let o = object(h);
    if o == 0 {
        return 0;
    }
    obj::call(m::WPObject::wpSaveImmediate, &[o]) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinMoveObject(h: u32, dest: u32, _flags: u32) -> u32 {
    if let Some(r) = remote(ipc::OP_MOVE, &[h, dest, _flags], &[]) {
        return r;
    }
    let (o, f) = (object(h), object(dest));
    if o == 0 || f == 0 || obj::call(m::WPObject::wpMoveObject, &[o, f]) == 0 {
        return 0;
    }
    obj::handle(o)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinCopyObject(h: u32, dest: u32, _flags: u32) -> u32 {
    if let Some(r) = remote(ipc::OP_COPY, &[h, dest, _flags], &[]) {
        return r;
    }
    let (o, f) = (object(h), object(dest));
    if o == 0 || f == 0 {
        return 0;
    }
    let n = obj::call(m::WPObject::wpCopyObject, &[o, f, 0]);
    if n == 0 { 0 } else { obj::handle(n) }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinCreateShadow(h: u32, dest: u32, _flags: u32) -> u32 {
    if let Some(r) = remote(ipc::OP_SHADOW, &[h, dest, _flags], &[]) {
        return r;
    }
    let (o, f) = (object(h), object(dest));
    if o == 0 || f == 0 {
        return 0;
    }
    let n = obj::call(m::WPObject::wpCreateShadowObject, &[o, f, 0]);
    if n == 0 { 0 } else { obj::handle(n) }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryObjectPath(h: u32, buf: *mut u8, cb: u32) -> u32 {
    let p = match wps::path_of_handle(h) {
        Some(p) => p,
        None => return 0,
    };
    if buf.is_null() || (cb as usize) < p.len() + 1 {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(p.as_ptr(), buf, p.len());
        *buf.add(p.len()) = 0;
    }
    1
}

/// WinQueryActiveDesktopPathname(pszPathName, ulSize): the directory of
/// the desktop folder (<WP_DESKTOP>, else the boot drive's DESKTOP).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryActiveDesktopPathname(buf: *mut u8, cb: u32) -> u32 {
    let p = match wps::handle_of_id(b"<WP_DESKTOP>").and_then(wps::path_of_handle) {
        Some(p) => p,
        None => {
            let mut boot = 3u32;
            unsafe { os2::DosQuerySysInfo(5, 5, &mut boot, 4) };
            alloc::vec![b'A' + boot as u8 - 1, b':', b'\\', b'D', b'E', b'S', b'K', b'T', b'O', b'P']
        }
    };
    if buf.is_null() || (cb as usize) < p.len() + 1 {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(p.as_ptr(), buf, p.len());
        *buf.add(p.len()) = 0;
    }
    1
}

// ---- Classes ----

/// WinRegisterObjectClass: the class must load from the DLL.
#[unsafe(no_mangle)]
pub extern "C" fn WinRegisterObjectClass(class: *const u8, module: *const u8) -> u32 {
    if let Some(r) = remote(ipc::OP_REGISTER, &[], &[cstr(class), cstr(module)]) {
        return r;
    }
    let (c, mname) = (cstr(class).to_vec(), cstr(module).to_vec());
    if c.is_empty() || mname.is_empty() {
        return 0;
    }
    let mut t = clsmgr::table();
    t.retain(|(n, _)| !eq_nocase(n, &c));
    t.push((c.clone(), mname));
    if !clsmgr::set_table(&t) {
        return 0;
    }
    if wps::active() && som::find_class(&c) == 0 {
        t.pop();
        clsmgr::set_table(&t);
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDeregisterObjectClass(class: *const u8) -> u32 {
    if let Some(r) = remote(ipc::OP_DEREGISTER, &[], &[cstr(class)]) {
        return r;
    }
    let c = cstr(class);
    let mut t = clsmgr::table();
    let n = t.len();
    t.retain(|(x, _)| !eq_nocase(x, c));
    (t.len() != n && clsmgr::set_table(&t)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinReplaceObjectClass(old: *const u8, new: *const u8, replace: u32) -> u32 {
    let (o, n) = (cstr(old).to_vec(), cstr(new).to_vec());
    let key = b"PM_Objects:ReplaceList";
    if replace != 0 {
        prf_write(HINI_SYSTEM, key, &o, Some(&z(&n)));
    } else {
        prf_write(HINI_SYSTEM, key, &o, None);
    }
    if wps::active() && replace != 0 {
        let (zo, zn) = (z(&o), z(&n));
        som::scm_call(11, &[zo.as_ptr() as usize, zn.as_ptr() as usize]);
    }
    1
}

/// The classes PMWP.DLL itself provides, which are always there.
const BUILT_IN: &[&str] = &[
    "WPObject", "WPAbstract", "WPTransient", "WPFileSystem", "WPDataFile", "WPProgramFile", "WPCommandFile", "WPFolder", "WPRootFolder",
    "WPDesktop", "WPDrives", "WPDisk", "WPProgram", "WPShadow", "WPIcon", "WPPointer", "WPBitmap", "WPMet", "WPPif",
];

/// WinEnumObjectClasses(pObjClass, pulSize): a list of OBJCLASS
/// (pNext, pszClassName, pszModName) with the strings after it.
#[unsafe(no_mangle)]
pub extern "C" fn WinEnumObjectClasses(buf: *mut u8, size: *mut u32) -> u32 {
    let mut list: Vec<(Vec<u8>, Vec<u8>)> = BUILT_IN.iter().map(|c| (c.as_bytes().to_vec(), b"PMWP".to_vec())).collect();
    for (c, d) in clsmgr::table() {
        if !list.iter().any(|(x, _)| eq_nocase(x, &c)) {
            list.push((c, d));
        }
    }
    let need = list.len() * 12 + list.iter().map(|(c, d)| c.len() + d.len() + 2).sum::<usize>();
    if size.is_null() {
        return 0;
    }
    unsafe {
        if buf.is_null() {
            *size = need as u32;
            return 1;
        }
        if (*size as usize) < need {
            *size = need as u32;
            return 0;
        }
        let mut s = buf.add(list.len() * 12);
        for (i, (c, d)) in list.iter().enumerate() {
            let e = buf.add(i * 12) as *mut u32;
            *e = if i + 1 < list.len() { buf.add((i + 1) * 12) as u32 } else { 0 };
            for (k, str_) in [c, d].iter().enumerate() {
                *e.add(1 + k) = s as u32;
                core::ptr::copy_nonoverlapping(str_.as_ptr(), s, str_.len());
                *s.add(str_.len()) = 0;
                s = s.add(str_.len() + 1);
            }
        }
    }
    1
}
