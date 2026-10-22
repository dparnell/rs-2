//! SOMClassMgr: the registry of classes, finding classes (loading the
//! DLLs that hold them), and merging into a replacement manager (the
//! Workplace Shell's WPClassManager takes over this way).

use alloc::vec::Vec;

use crate::class::{Seq, mm};
use crate::id::{self, SomId};
use crate::rt::*;
use crate::sys::*;

pub struct Registry {
    pub classes: Vec<usize>,
    /// somSubstituteClass: lookups of the first name give the class.
    pub subs: Vec<(usize, usize)>,
    pub ir: usize,
    /// After somMergeInto, the manager that took over.
    pub merged_into: usize,
}

/// The registry of a manager (made on first use).
unsafe fn reg<'a>(mgr: usize) -> Option<&'a mut Registry> {
    unsafe {
        let p = data_in(mgr, (*SOMCLASSMGR).inst_token) as *mut *mut Registry;
        if p.is_null() {
            return None;
        }
        if (*p).is_null() {
            *p = leak(Registry { classes: Vec::new(), subs: Vec::new(), ir: 0, merged_into: 0 });
        }
        Some(&mut **p)
    }
}

/// The manager calls are forwarded to (itself unless merged).
fn target(mgr: usize) -> usize {
    match unsafe { reg(mgr) } {
        Some(r) if r.merged_into != 0 => r.merged_into,
        _ => mgr,
    }
}

fn class_name_key(cls: usize) -> usize {
    unsafe { rec_ok(cls) }.map_or(0, |c| c.key)
}

pub extern "C" fn somClassFromId(mgr: usize, cid: SomId) -> usize {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::CLASS_FROM_ID, &[t as u32, cid as u32]) as usize;
    }
    let _g = lock();
    let key = id::key_of(cid);
    let Some(r) = (unsafe { reg(mgr) }) else { return 0 };
    if let Some(&(_, c)) = r.subs.iter().find(|s| s.0 == key) {
        return c;
    }
    r.classes.iter().copied().find(|&c| class_name_key(c) == key).unwrap_or(0)
}

pub extern "C" fn somRegisterClass(mgr: usize, cls: usize) {
    let t = target(mgr);
    if t != mgr {
        crate::call_mm(t, mm::REGISTER_CLASS, &[t as u32, cls as u32]);
        return;
    }
    let _g = lock();
    let Some(r) = (unsafe { reg(mgr) }) else { return };
    let key = class_name_key(cls);
    // A class built again under the same name replaces the old one.
    r.classes.retain(|&c| c == cls || class_name_key(c) != key);
    if !r.classes.contains(&cls) {
        r.classes.push(cls);
    }
}

pub extern "C" fn somUnregisterClass(mgr: usize, cls: usize) -> i32 {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::UNREGISTER_CLASS, &[t as u32, cls as u32]) as i32;
    }
    let _g = lock();
    if let Some(r) = unsafe { reg(mgr) } {
        r.classes.retain(|&c| c != cls);
    }
    0
}

pub extern "C" fn somFindClass(mgr: usize, cid: SomId, major: i32, minor: i32) -> usize {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::FIND_CLASS, &[t as u32, cid as u32, major as u32, minor as u32]) as usize;
    }
    let c = crate::call_mm(mgr, mm::CLASS_FROM_ID, &[mgr as u32, cid as u32]) as usize;
    if c != 0 && crate::call_cm(c, crate::class::cm::CHECK_VERSION, &[c as u32, major as u32, minor as u32]) & 0xff != 0 {
        return c;
    }
    let file = crate::call_mm(mgr, mm::LOCATE_CLASS_FILE, &[mgr as u32, cid as u32, major as u32, minor as u32]);
    crate::call_mm(mgr, mm::FIND_CLS_IN_FILE, &[mgr as u32, cid as u32, major as u32, minor as u32, file]) as usize
}

pub extern "C" fn somFindClsInFile(mgr: usize, cid: SomId, major: i32, minor: i32, file: *const u8) -> usize {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::FIND_CLS_IN_FILE, &[t as u32, cid as u32, major as u32, minor as u32, file as u32])
            as usize;
    }
    let mut c = crate::call_mm(mgr, mm::CLASS_FROM_ID, &[mgr as u32, cid as u32]) as usize;
    if c == 0 {
        c = crate::call_mm(mgr, mm::LOAD_CLASS_FILE, &[mgr as u32, cid as u32, major as u32, minor as u32, file as u32])
            as usize;
    }
    if c == 0 {
        if unsafe { crate::SOM_WarnLevel } != 0 {
            crate::warn(b"class %s not found\n\0", &[id::name_of(cid) as u32]);
        }
        return 0;
    }
    if crate::call_cm(c, crate::class::cm::CHECK_VERSION, &[c as u32, major as u32, minor as u32]) & 0xff == 0 {
        crate::warn(b"class %s: wrong version\n\0", &[id::name_of(cid) as u32]);
        return 0;
    }
    c
}

pub extern "C" fn somLocateClassFile(_mgr: usize, cid: SomId, _major: i32, _minor: i32) -> *const u8 {
    // No interface repository: a class's DLL has the class's name.
    id::name_of(cid)
}

pub extern "C" fn somLoadClassFile(mgr: usize, cid: SomId, major: i32, minor: i32, file: *const u8) -> usize {
    let init = crate::call_mm(mgr, mm::GET_INIT_FUNCTION, &[mgr as u32]);
    let mut h = 0u32;
    let load: extern "C" fn(*const u8, *const u8, u32, i32, i32, *mut u32) -> i32 =
        unsafe { core::mem::transmute(crate::SOMLoadModule) };
    if load(id::name_of(cid), file, init, major, minor, &mut h) != 0 {
        return 0;
    }
    crate::call_mm(mgr, mm::CLASS_FROM_ID, &[mgr as u32, cid as u32]) as usize
}

pub extern "C" fn somUnloadClassFile(mgr: usize, cls: usize) -> i32 {
    somUnregisterClass(mgr, cls)
}

pub extern "C" fn somGetInitFunction(_mgr: usize) -> *const u8 {
    b"SOMInitModule\0".as_ptr()
}

pub extern "C" fn somMergeInto(mgr: usize, target_obj: usize) {
    let _g = lock();
    let Some(r) = (unsafe { reg(mgr) }) else { return };
    let is_mgr = target_obj != 0
        && unsafe { !class_of_obj(target_obj).is_null() && (*class_of_obj(target_obj)).descends_from(SOMCLASSMGR) };
    if !is_mgr || target_obj == mgr {
        return;
    }
    let Some(t) = (unsafe { reg(target_obj) }) else { return };
    for &c in &r.classes {
        if !t.classes.contains(&c) {
            t.classes.push(c);
        }
    }
    t.subs.extend_from_slice(&r.subs);
    if t.ir == 0 {
        t.ir = r.ir;
    }
    r.merged_into = target_obj;
    unsafe { crate::SOMClassMgrObject = target_obj };
}

pub extern "C" fn somSubstituteClass(mgr: usize, orig: *const u8, new: *const u8) -> i32 {
    let t = target(mgr);
    let new_id = id::static_id(unsafe { cstr(new) });
    let c = crate::call_mm(t, mm::CLASS_FROM_ID, &[t as u32, new_id as u32]) as usize;
    if c == 0 {
        return 1;
    }
    let _g = lock();
    let Some(r) = (unsafe { reg(t) }) else { return 1 };
    let key = id::key_of_name(unsafe { cstr(orig) });
    r.subs.retain(|s| s.0 != key);
    r.subs.push((key, c));
    0
}

pub extern "C" fn get_somRegisteredClasses(ret: *mut Seq, mgr: usize) -> *mut Seq {
    let t = target(mgr);
    let items: Vec<usize> = unsafe { reg(t) }.map_or(Vec::new(), |r| r.classes.clone());
    let buf = crate::malloc((items.len().max(1) * 4) as u32) as *mut usize;
    unsafe {
        core::ptr::copy_nonoverlapping(items.as_ptr(), buf, items.len());
        *ret = Seq { maximum: items.len() as u32, length: items.len() as u32, buffer: buf };
    }
    ret
}

/// The classes derived from a class or having it as their metaclass
/// (a null-terminated array from SOMMalloc).
pub extern "C" fn somGetRelatedClasses(mgr: usize, cls: usize) -> *mut usize {
    let t = target(mgr);
    let base = unsafe { rec(cls) };
    let mut v: Vec<usize> = Vec::new();
    if let Some(r) = unsafe { reg(t) } {
        for &c in &r.classes {
            let cr = unsafe { rec(c) };
            let meta = unsafe { class_of_obj(c) };
            if c != cls
                && !base.is_null()
                && ((!cr.is_null() && unsafe { (*cr).descends_from(base) }) || (!meta.is_null() && unsafe { (*meta).descends_from(base) }))
            {
                v.push(c);
            }
        }
    }
    v.push(0);
    let buf = crate::malloc((v.len() * 4) as u32) as *mut usize;
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), buf, v.len()) };
    buf
}

pub extern "C" fn get_somInterfaceRepository(mgr: usize) -> usize {
    unsafe { reg(target(mgr)) }.map_or(0, |r| r.ir)
}

pub extern "C" fn set_somInterfaceRepository(mgr: usize, ir: usize) {
    if let Some(r) = unsafe { reg(target(mgr)) } {
        r.ir = ir;
    }
}

pub extern "C" fn somBeginPersistentClasses(_mgr: usize) {}
pub extern "C" fn somEndPersistentClasses(_mgr: usize) {}

pub extern "C" fn somJoinAffinityGroup(_mgr: usize, _new: usize, _aff: usize) -> u32 {
    0
}

pub extern "C" fn somRegisterClassLibrary(_mgr: usize, _name: *const u8, init: usize) {
    if init != 0 {
        let f: extern "C" fn(i32, i32, *const u8) = unsafe { core::mem::transmute(init) };
        f(0, 0, core::ptr::null());
    }
}

pub extern "C" fn somUnregisterClassLibrary(_mgr: usize, _name: *const u8) {}

pub extern "C" fn mgr_private(_mgr: usize) -> usize {
    0
}

pub extern "C" fn mgr_somDumpSelfInt(mgr: usize, level: i32) {
    let n = unsafe { reg(target(mgr)) }.map_or(0, |r| r.classes.len());
    crate::prefix_level(level);
    crate::printf(b"%d classes registered\n\0", &[n as u32]);
}

// ---- Loading class libraries ----

/// The default SOMLoadModule: load the DLL and call its SOMInitModule
/// (or, without one, `<className>NewClass`).
pub extern "C" fn load_module(class_name: *const u8, file: *const u8, func: *const u8, major: i32, minor: i32, h: *mut u32) -> i32 {
    let mut hmod = 0u32;
    let mut fail = [0u8; 64];
    let rc = unsafe { os2::DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, file, &mut hmod) };
    if rc != 0 {
        return rc as i32;
    }
    if !h.is_null() {
        unsafe { *h = hmod };
    }
    let mut p: usize = 0;
    if !func.is_null() && unsafe { os2::DosQueryProcAddr(hmod, 0, func, &mut p) } == 0 && p != 0 {
        let f: extern "C" fn(i32, i32, *const u8) = unsafe { core::mem::transmute(p) };
        f(major, minor, class_name);
        return 0;
    }
    let mut name: Vec<u8> = unsafe { cstr(class_name) }.to_vec();
    name.extend_from_slice(b"NewClass\0");
    if unsafe { os2::DosQueryProcAddr(hmod, 0, name.as_ptr(), &mut p) } == 0 && p != 0 {
        let f: extern "C" fn(i32, i32) -> usize = unsafe { core::mem::transmute(p) };
        f(major, minor);
        return 0;
    }
    0
}

pub extern "C" fn delete_module(h: u32) -> i32 {
    unsafe { os2::DosFreeModule(h) as i32 }
}

pub extern "C" fn init_func_name() -> *const u8 {
    b"SOMInitModule\0".as_ptr()
}
