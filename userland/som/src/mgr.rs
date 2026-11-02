//! SOMClassMgr: the registry of classes, finding classes (loading the
//! DLLs that hold them), unregistering and unloading them, and merging
//! into a replacement manager (the Workplace Shell's WPClassManager takes
//! over this way).
//!
//! As in IBM's SOM, each registered class has an entry that counts the
//! registered classes having it as a parent or as their metaclass (such
//! a class cannot be unregistered), and records the affinity group it
//! was registered in: the classes registered while a class library was
//! being loaded (somLoadClassFile) or initialized (somRegisterClassLibrary),
//! the class that caused the loading being the group's leader.
//! Unregistering the leader unloads the whole group and frees the DLL
//! (SOMDeleteModule). Unregistering a class frees its class object, which
//! clears its ClassData, so that `<Class>NewClass` builds it again.

use alloc::vec::Vec;

use crate::class::{Seq, mm};
use crate::id::{self, SomId};
use crate::rt::*;
use crate::sys::*;

/// Entry flags (IBM's): being unregistered with its affinity group,
/// persistent, the leader of its affinity group.
const E_UNLOADING: u32 = 1;
const E_PERSISTENT: u32 = 2;
const E_LEADER: u32 = 4;

pub struct Entry {
    pub cls: usize,
    pub key: usize,
    /// The class library it came with (an index into `libs` + 1).
    pub lib: usize,
    /// Its affinity group (0: none).
    pub group: u32,
    pub flags: u32,
    /// Registered classes having it as a parent / as their metaclass.
    pub children: u32,
    pub metas: u32,
}

/// A class library (a DLL loaded by somLoadClassFile, or registered with
/// somRegisterClassLibrary).
pub struct Lib {
    /// Its normalized name (no extension).
    pub name: Vec<u8>,
    /// The module handle to free when it is unloaded (0: not loaded by
    /// the class manager).
    pub handle: u32,
    /// The class that caused it to be loaded (its affinity group's
    /// leader).
    pub leader: usize,
    pub live: bool,
}

pub struct Registry {
    /// In the order the classes were registered.
    pub entries: Vec<Entry>,
    /// somSubstituteClass: lookups of the first name give the class.
    pub subs: Vec<(usize, usize)>,
    /// The manager's interface repository (its own copy), the
    /// repository's class, its lookup_modifier, and whether SOMIR was
    /// looked at.
    pub ir: usize,
    pub ir_class: usize,
    pub ir_lookup: usize,
    pub ir_checked: bool,
    /// After somMergeInto, the manager that took over.
    pub merged_into: usize,
    pub libs: Vec<Lib>,
    /// The affinity group and class library classes registered now go
    /// to, and the last group number given out.
    pub group: u32,
    pub lib: usize,
    pub groups: u32,
    /// The next class registered leads its group.
    pub leader_next: bool,
}

/// The registry of a manager (made on first use).
unsafe fn reg<'a>(mgr: usize) -> Option<&'a mut Registry> {
    unsafe {
        let p = data_in(mgr, (*SOMCLASSMGR).inst_token) as *mut *mut Registry;
        if p.is_null() {
            return None;
        }
        if (*p).is_null() {
            *p = leak(Registry {
                entries: Vec::new(),
                subs: Vec::new(),
                ir: 0,
                ir_class: 0,
                ir_lookup: 0,
                ir_checked: false,
                merged_into: 0,
                libs: Vec::new(),
                group: 0,
                lib: 0,
                groups: 0,
                leader_next: false,
            });
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

impl Registry {
    fn find(&self, cls: usize) -> Option<usize> {
        if cls == 0 {
            return None;
        }
        self.entries.iter().position(|e| e.cls == cls)
    }

    fn find_mut(&mut self, cls: usize) -> Option<&mut Entry> {
        self.find(cls).map(|i| &mut self.entries[i])
    }

    /// A class library's record by name (made when `make`).
    fn lib_named(&mut self, name: &[u8], make: bool) -> usize {
        let n = normalize_lib(name);
        if let Some(i) = self.libs.iter().position(|l| l.live && l.name.eq_ignore_ascii_case(&n)) {
            return i + 1;
        }
        if !make {
            return 0;
        }
        self.libs.push(Lib { name: n, handle: 0, leader: 0, live: true });
        self.libs.len()
    }

    /// The classes of an affinity group, newest first.
    fn group_members(&self, group: u32) -> Vec<usize> {
        self.entries.iter().rev().filter(|e| e.group == group).map(|e| e.cls).collect()
    }
}

/// A library name as IBM's class manager keeps it: without its extension
/// unless it has a path.
fn normalize_lib(name: &[u8]) -> Vec<u8> {
    let has_path = matches!(name.first(), Some(b'\\') | Some(b'/')) || name.get(1) == Some(&b':');
    match name.iter().rposition(|&c| c == b'.') {
        Some(i) if !has_path => name[..i].to_vec(),
        _ => name.to_vec(),
    }
}

/// somBeginPersistentClasses brackets, per thread.
static mut PERSISTENT: Vec<(u32, u32)> = Vec::new();

fn persistent_depth() -> u32 {
    let tid = thread_id();
    #[allow(static_mut_refs)]
    unsafe {
        PERSISTENT.iter().find(|p| p.0 == tid).map_or(0, |p| p.1)
    }
}

fn parents_of(cls: usize) -> Vec<usize> {
    unsafe { rec_ok(cls) }.map_or(Vec::new(), |c| c.parents.iter().map(|&p| unsafe { (*p).obj }).collect())
}

fn meta_of(cls: usize) -> usize {
    crate::call_om(cls, om::GET_CLASS, &[cls as u32]) as usize
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
    r.entries.iter().rev().find(|e| e.key == key).map_or(0, |e| e.cls)
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
    let persistent = if persistent_depth() != 0 { E_PERSISTENT } else { 0 };
    match r.find(cls) {
        Some(i) => r.entries[i].flags |= persistent,
        None => {
            // A class built again under the same name replaces the old
            // one (IBM's somBuildClass would reuse the class object).
            if let Some(i) = r.entries.iter().position(|e| e.key == key) {
                let old = r.entries.remove(i);
                uncount(r, old.cls);
            }
            let mut flags = persistent;
            if r.leader_next {
                r.leader_next = false;
                flags |= E_LEADER;
                if r.lib != 0 {
                    r.libs[r.lib - 1].leader = cls;
                }
            }
            r.entries.push(Entry { cls, key, lib: r.lib, group: r.group, flags, children: 0, metas: 0 });
        }
    }
    // The parents' and the metaclass's counts (on every registration,
    // as IBM's does).
    for p in parents_of(cls) {
        if let Some(e) = r.find_mut(p) {
            e.children += 1;
        }
    }
    let m = meta_of(cls);
    if let Some(e) = r.find_mut(m) {
        e.metas += 1;
    }
}

/// Take back what registering `cls` counted.
fn uncount(r: &mut Registry, cls: usize) {
    for p in parents_of(cls) {
        if let Some(e) = r.find_mut(p) {
            e.children = e.children.saturating_sub(1);
        }
    }
    let m = meta_of(cls);
    if let Some(e) = r.find_mut(m) {
        e.metas = e.metas.saturating_sub(1);
    }
}

/// somUnregisterClass: 0 when done (the class object freed), 1 when the
/// class is persistent or other registered classes have it as a parent
/// or metaclass. The leader of an affinity group unloads its group.
pub extern "C" fn somUnregisterClass(mgr: usize, cls: usize) -> i32 {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::UNREGISTER_CLASS, &[t as u32, cls as u32]) as i32;
    }
    if cls == 0 {
        return 0;
    }
    let Some(r) = (unsafe { reg(mgr) }) else { return 0 };
    let g = lock();
    let mut counted = None;
    if let Some(i) = r.find(cls) {
        let e = &r.entries[i];
        if e.flags & E_PERSISTENT != 0 {
            return 1;
        }
        if e.flags & E_LEADER != 0 {
            drop(g);
            return crate::call_mm(mgr, mm::UNLOAD_CLASS_FILE, &[mgr as u32, cls as u32]) as i32;
        }
        if e.children > 0 || e.metas > 0 {
            return 1;
        }
        counted = Some((parents_of(cls), meta_of(cls)));
        r.entries.remove(i);
        drop(g);
        // somDestruct(cls, TRUE, NULL): the class object goes.
        crate::call_om(cls, om::DESTRUCT, &[cls as u32, 1, 0]);
    } else {
        drop(g);
    }
    let _g = lock();
    r.entries.retain(|e| e.cls != cls);
    if let Some((parents, m)) = counted {
        for p in parents {
            if let Some(e) = r.find_mut(p) {
                e.children = e.children.saturating_sub(1);
            }
        }
        if let Some(e) = r.find_mut(m) {
            e.metas = e.metas.saturating_sub(1);
        }
    }
    0
}

/// Free a class library's DLL (SOMDeleteModule's result).
fn delete_lib(r: &mut Registry, lib: usize) -> i32 {
    let l = &mut r.libs[lib - 1];
    l.live = false;
    let h = core::mem::take(&mut l.handle);
    if h == 0 {
        return 0;
    }
    let f: extern "C" fn(u32) -> i32 = unsafe { core::mem::transmute(crate::SOMDeleteModule) };
    f(h)
}

/// somUnloadClassFile: unregister the class's affinity group and free
/// its DLL; SOMDeleteModule's result, or -2 when there was no DLL to free
/// (-1: the class is persistent).
pub extern "C" fn somUnloadClassFile(mgr: usize, cls: usize) -> i32 {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::UNLOAD_CLASS_FILE, &[t as u32, cls as u32]) as i32;
    }
    let Some(r) = (unsafe { reg(mgr) }) else { return -1 };
    let g = lock();
    let Some(i) = r.find(cls) else {
        drop(g);
        crate::error(20159, b"SOMCM.C\0", 0x755);
        return -1;
    };
    if r.entries[i].flags & E_PERSISTENT != 0 {
        return -1;
    }
    let group = r.entries[i].group;
    let mut lib = 0;
    if group == 0 {
        let e = &mut r.entries[i];
        let flags = e.flags;
        lib = core::mem::take(&mut e.lib);
        e.flags &= !E_LEADER;
        drop(g);
        let rc = crate::call_mm(mgr, mm::UNREGISTER_CLASS, &[mgr as u32, cls as u32]);
        let _g = lock();
        if rc != 0 {
            lib = 0;
            if let Some(e) = r.find_mut(cls) {
                e.flags = flags;
            }
        }
        return if lib == 0 { -2 } else { delete_lib(r, lib) };
    }
    // The group: its leader holds the library.
    let members = r.group_members(group);
    let leader = r.entries.iter().find(|e| e.group == group && e.flags & E_LEADER != 0).map(|e| e.cls);
    if let Some(e) = leader.and_then(|l| r.find_mut(l)) {
        lib = e.lib;
        e.group = 0;
    }
    drop(g);
    let mut kept = false;
    for &m in &members {
        let saved = {
            let _g = lock();
            r.find_mut(m).map(|e| {
                let s = (e.group, e.flags, e.lib);
                e.flags = (e.flags | E_UNLOADING) & !E_LEADER;
                e.group = 0;
                e.lib = 0;
                s
            })
        };
        let Some((sg, sf, sl)) = saved else { continue };
        if crate::call_mm(mgr, mm::UNREGISTER_CLASS, &[mgr as u32, m as u32]) != 0 {
            let _g = lock();
            if let Some(e) = r.find_mut(m) {
                (e.group, e.flags, e.lib) = (if sg == 0 { group } else { sg }, sf, sl);
            }
            kept = true;
            lib = 0;
        }
    }
    let _g = lock();
    if kept {
        if let Some(e) = leader.and_then(|l| r.find_mut(l)) {
            e.group = group;
        }
    }
    if lib == 0 { -2 } else { delete_lib(r, lib) }
}

/// somGetRelatedClasses: the class's affinity group (from SOMMalloc,
/// null-terminated): its leader (or -1) and then its members, newest
/// first; a class not in a group gives itself.
pub extern "C" fn somGetRelatedClasses(mgr: usize, cls: usize) -> *mut usize {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::GET_RELATED_CLASSES, &[t as u32, cls as u32]) as *mut usize;
    }
    let _g = lock();
    let Some(r) = (unsafe { reg(mgr) }) else { return core::ptr::null_mut() };
    let Some(i) = r.find(cls) else { return core::ptr::null_mut() };
    let group = r.entries[i].group;
    let mut v: Vec<usize> = Vec::new();
    if group == 0 {
        v.push(cls);
    } else {
        v.push(usize::MAX);
        for e in r.entries.iter().rev().filter(|e| e.group == group) {
            v.push(e.cls);
            if e.flags & E_LEADER != 0 {
                v[0] = e.cls;
            }
        }
    }
    v.push(0);
    let buf = crate::malloc((v.len() * 4) as u32) as *mut usize;
    if !buf.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), buf, v.len()) };
    }
    buf
}

/// somJoinAffinityGroup: `new` (in no group, or already in this one)
/// joins the group whose leader `aff`'s group has.
pub extern "C" fn somJoinAffinityGroup(mgr: usize, new: usize, aff: usize) -> u32 {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::JOIN_AFFINITY_GROUP, &[t as u32, new as u32, aff as u32]);
    }
    let rel = crate::call_mm(mgr, mm::GET_RELATED_CLASSES, &[mgr as u32, aff as u32]) as *mut usize;
    let lead = if rel.is_null() { 0 } else { unsafe { *rel } };
    if !rel.is_null() {
        crate::free(rel as *mut u8);
    }
    let _g = lock();
    let Some(r) = (unsafe { reg(mgr) }) else { return 0 };
    let l = if lead == 0 || lead == usize::MAX { None } else { r.find(lead) };
    let (Some(n), Some(l)) = (r.find(new), l) else { return 0 };
    let (lg, ll) = (r.entries[l].group, r.entries[l].lib);
    let e = &mut r.entries[n];
    if lg == 0 || (e.group != 0 && e.group != lg) {
        return 0;
    }
    e.group = lg;
    if e.lib == 0 {
        e.lib = ll;
    }
    1
}

pub extern "C" fn somBeginPersistentClasses(mgr: usize) {
    let t = target(mgr);
    if t != mgr {
        crate::call_mm(t, mm::BEGIN_PERSISTENT_CLASSES, &[t as u32]);
        return;
    }
    let _g = lock();
    let tid = thread_id();
    #[allow(static_mut_refs)]
    unsafe {
        match PERSISTENT.iter_mut().find(|p| p.0 == tid) {
            Some(p) => p.1 += 1,
            None => PERSISTENT.push((tid, 1)),
        }
    }
}

pub extern "C" fn somEndPersistentClasses(mgr: usize) {
    let t = target(mgr);
    if t != mgr {
        crate::call_mm(t, mm::END_PERSISTENT_CLASSES, &[t as u32]);
        return;
    }
    let _g = lock();
    let tid = thread_id();
    #[allow(static_mut_refs)]
    unsafe {
        if let Some(p) = PERSISTENT.iter_mut().find(|p| p.0 == tid) {
            p.1 = p.1.saturating_sub(1);
        }
    }
}

/// IBM's private somReleaseClasses (does nothing) and
/// somRegisterThreadUsage (TRUE).
pub extern "C" fn somReleaseClasses(_mgr: usize) {}

pub extern "C" fn somRegisterThreadUsage(mgr: usize, cls: usize) -> u32 {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::REGISTER_THREAD_USAGE, &[t as u32, cls as u32]);
    }
    1
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

/// The interface repository's class (SOMIR.DLL's Repository).
fn ir_class(mgr: usize, r: &mut Registry) -> usize {
    if r.ir_class == 0 {
        let rid = id::static_id(b"Repository");
        r.ir_class = crate::call_mm(mgr, mm::FIND_CLS_IN_FILE, &[mgr as u32, rid as u32, 0, 0, b"somir\0".as_ptr() as u32]) as usize;
        r.ir_lookup = 0;
    }
    r.ir_class
}

/// A copy of an interface repository object (somDefaultCopyInit into
/// memory from SOMMalloc, as IBM's class manager keeps and hands out).
fn ir_copy(cls: usize, ir: usize) -> usize {
    let size = crate::call_cm(cls, crate::class::cm::GET_INSTANCE_SIZE, &[cls as u32]);
    let p = crate::malloc(size.max(4)) as usize;
    if p == 0 {
        return 0;
    }
    unsafe { *(p as *mut usize) = crate::call_cm(cls, crate::class::cm::GET_CLASS_MTAB, &[cls as u32]) as usize };
    crate::call_om(p, om::DEFAULT_COPY_INIT, &[p as u32, 0, ir as u32]);
    p
}

/// somLocateClassFile: the class's DLL as the interface repository's
/// `dllname` modifier gives it (the repository is used once there is
/// one, or when SOMIR is set), else the class's name, as IBM's does.
pub extern "C" fn somLocateClassFile(mgr: usize, cid: SomId, major: i32, minor: i32) -> *const u8 {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::LOCATE_CLASS_FILE, &[t as u32, cid as u32, major as u32, minor as u32]) as *const u8;
    }
    let name = id::name_of(cid);
    let Some(r) = (unsafe { reg(mgr) }) else { return name };
    let _g = lock();
    if r.ir == 0 && !r.ir_checked {
        let mut v: *const u8 = core::ptr::null();
        if unsafe { os2::DosScanEnv(b"SOMIR\0".as_ptr(), &mut v) } == 0 && !v.is_null() && unsafe { *v } != 0 {
            ir_class(mgr, r);
        }
    }
    r.ir_checked = true;
    if r.ir_class != 0 && r.ir == 0 {
        r.ir = crate::call_cm(r.ir_class, crate::class::cm::NEW, &[r.ir_class as u32]) as usize;
    }
    if r.ir == 0 {
        return name;
    }
    if r.ir_lookup == 0 {
        r.ir_lookup = crate::somResolveByName(r.ir, b"lookup_modifier\0".as_ptr());
    }
    if r.ir_lookup == 0 {
        return name;
    }
    let f: extern "C" fn(usize, *mut crate::Environment, *const u8, *const u8) -> *const u8 =
        unsafe { core::mem::transmute(r.ir_lookup) };
    let s = f(r.ir, crate::somGetGlobalEnvironment(), name, b"dllname\0".as_ptr());
    if s.is_null() || unsafe { *s } == 0 {
        return name;
    }
    let v = unsafe { cstr(s) };
    let v = if v.len() >= 3 && v[0] == b'"' { &v[1..v.len() - 1] } else { v };
    let mut out = v.to_vec();
    out.push(0);
    leak_vec(out)
}

pub extern "C" fn somLoadClassFile(mgr: usize, cid: SomId, major: i32, minor: i32, file: *const u8) -> usize {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::LOAD_CLASS_FILE, &[t as u32, cid as u32, major as u32, minor as u32, file as u32])
            as usize;
    }
    let Some(r) = (unsafe { reg(mgr) }) else { return 0 };
    let init = crate::call_mm(mgr, mm::GET_INIT_FUNCTION, &[mgr as u32]);
    // The classes registered while the DLL loads form a new affinity
    // group.
    let (old_group, old_lib, old_leader) = {
        let _g = lock();
        let saved = (r.group, r.lib, r.leader_next);
        r.groups += 1;
        r.group = r.groups;
        r.leader_next = false;
        r.lib = if file.is_null() { 0 } else { r.lib_named(unsafe { cstr(file) }, true) };
        saved
    };
    let lib = r.lib;
    let mut h = 0u32;
    let load: extern "C" fn(*const u8, *const u8, u32, i32, i32, *mut u32) -> i32 =
        unsafe { core::mem::transmute(crate::SOMLoadModule) };
    let rc = load(id::name_of(cid), file, init, major, minor, &mut h);
    let _g = lock();
    if lib != 0 {
        if h == 0 {
            r.libs[lib - 1].live = false;
        } else {
            r.libs[lib - 1].handle = h;
        }
    }
    let mut cls = 0;
    if rc == 0 {
        let key = id::key_of(cid);
        if let Some(e) = r.entries.iter_mut().rev().find(|e| e.key == key) {
            let c = e.cls;
            if crate::call_cm(c, crate::class::cm::CHECK_VERSION, &[c as u32, major as u32, minor as u32]) & 0xff != 0 {
                e.flags |= E_LEADER;
                if e.lib != 0 {
                    r.libs[e.lib - 1].leader = c;
                }
                cls = c;
            }
        }
    }
    (r.group, r.lib, r.leader_next) = (old_group, old_lib, old_leader);
    cls
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
    let base = t.libs.len();
    t.libs.append(&mut r.libs);
    for mut e in core::mem::take(&mut r.entries) {
        if t.find(e.cls).is_none() {
            if e.lib != 0 {
                e.lib += base;
            }
            t.entries.push(e);
        }
    }
    t.subs.extend_from_slice(&r.subs);
    if t.ir == 0 {
        (t.ir, t.ir_class, t.ir_lookup, t.ir_checked) = (r.ir, r.ir_class, r.ir_lookup, r.ir_checked);
    }
    t.groups = t.groups.max(r.groups);
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

/// The registered classes, newest first (as IBM's lists them).
pub extern "C" fn get_somRegisteredClasses(ret: *mut Seq, mgr: usize) -> *mut Seq {
    let t = target(mgr);
    let items: Vec<usize> = unsafe { reg(t) }.map_or(Vec::new(), |r| r.entries.iter().rev().map(|e| e.cls).collect());
    let buf = if items.is_empty() { core::ptr::null_mut() } else { crate::malloc((items.len() * 4) as u32) as *mut usize };
    unsafe {
        if !buf.is_null() {
            core::ptr::copy_nonoverlapping(items.as_ptr(), buf, items.len());
        }
        *ret = Seq { maximum: items.len() as u32, length: items.len() as u32, buffer: buf };
    }
    ret
}

/// _get_somInterfaceRepository: a copy of the manager's repository (the
/// caller owns it), made from SOMIR.DLL's Repository on first use.
pub extern "C" fn get_somInterfaceRepository(mgr: usize) -> usize {
    let t = target(mgr);
    if t != mgr {
        return crate::call_mm(t, mm::GET_INTERFACE_REPOSITORY, &[t as u32]) as usize;
    }
    let Some(r) = (unsafe { reg(mgr) }) else { return 0 };
    let (copy, cls) = {
        let _g = lock();
        let cls = ir_class(mgr, r);
        if cls == 0 {
            return 0;
        }
        if r.ir == 0 {
            r.ir = crate::call_cm(cls, crate::class::cm::NEW, &[cls as u32]) as usize;
        }
        if r.ir == 0 {
            return 0;
        }
        (ir_copy(cls, r.ir), cls)
    };
    if copy != 0 {
        crate::call_mm(mgr, mm::REGISTER_THREAD_USAGE, &[mgr as u32, cls as u32]);
    }
    copy
}

/// _set_somInterfaceRepository: the manager keeps its own copy (the old
/// one is freed).
pub extern "C" fn set_somInterfaceRepository(mgr: usize, ir: usize) {
    let t = target(mgr);
    if t != mgr {
        crate::call_mm(t, mm::SET_INTERFACE_REPOSITORY, &[t as u32, ir as u32]);
        return;
    }
    let Some(r) = (unsafe { reg(mgr) }) else { return };
    let _g = lock();
    if ir == r.ir {
        return;
    }
    let old = core::mem::take(&mut r.ir);
    if old != 0 {
        crate::call_om(old, om::SOM_FREE, &[old as u32]);
    }
    if ir != 0 {
        let cls = crate::call_om(ir, om::GET_CLASS, &[ir as u32]) as usize;
        r.ir_class = cls;
        r.ir = ir_copy(cls, ir);
    }
    r.ir_lookup = 0;
}

/// somRegisterClassLibrary: a class library loaded by the system (its
/// _DLL_InitTerm calls this) is initialized now; the classes it
/// registers form an affinity group, the first one leading it.
pub extern "C" fn somRegisterClassLibrary(mgr: usize, name: *const u8, init: usize) {
    let t = target(mgr);
    if t != mgr {
        crate::call_mm(t, mm::REGISTER_CLASS_LIBRARY, &[t as u32, name as u32, init as u32]);
        return;
    }
    let Some(r) = (unsafe { reg(mgr) }) else { return };
    if init == 0 {
        return;
    }
    let saved = {
        let _g = lock();
        let saved = (r.group, r.lib, r.leader_next);
        r.groups += 1;
        r.group = r.groups;
        r.leader_next = true;
        r.lib = if name.is_null() { 0 } else { r.lib_named(unsafe { cstr(name) }, true) };
        saved
    };
    let f: extern "C" fn(i32, i32, *const u8) = unsafe { core::mem::transmute(init) };
    f(0, 0, b"\0".as_ptr());
    let _g = lock();
    (r.group, r.lib, r.leader_next) = saved;
}

/// somUnregisterClassLibrary: the library is being unloaded by the
/// system (from its _DLL_InitTerm): its affinity group goes, without
/// freeing the DLL again.
pub extern "C" fn somUnregisterClassLibrary(mgr: usize, name: *const u8) {
    let t = target(mgr);
    if t != mgr {
        crate::call_mm(t, mm::UNREGISTER_CLASS_LIBRARY, &[t as u32, name as u32]);
        return;
    }
    let Some(r) = (unsafe { reg(mgr) }) else { return };
    let leader = {
        let _g = lock();
        let lib = if name.is_null() { 0 } else { r.lib_named(unsafe { cstr(name) }, false) };
        if lib == 0 {
            return;
        }
        let l = &mut r.libs[lib - 1];
        let leader = l.leader;
        if leader == 0 || r.find(leader).is_none() {
            return;
        }
        r.libs[lib - 1].handle = 0;
        leader
    };
    crate::call_mm(mgr, mm::UNLOAD_CLASS_FILE, &[mgr as u32, leader as u32]);
}

pub extern "C" fn mgr_somDumpSelfInt(mgr: usize, level: i32) {
    let n = unsafe { reg(target(mgr)) }.map_or(0, |r| r.entries.len());
    crate::prefix_level(level);
    crate::printf(b"%d classes registered\n\0", &[n as u32]);
}

// ---- Loading class libraries ----

/// Is a class of that name known to the class manager?
fn class_exists(name: &[u8]) -> bool {
    let mgr = unsafe { crate::SOMClassMgrObject };
    if mgr == 0 {
        return false;
    }
    let cid = id::static_id(name);
    crate::call_mm(mgr, mm::CLASS_FROM_ID, &[mgr as u32, cid as u32]) != 0
}

/// The default SOMLoadModule: load the DLL and call its SOMInitModule;
/// when there is none, or the class is still not there after it (a DLL
/// whose SOMInitModule makes only some of its classes: MMPARTS's
/// MMBitmap), its `<className>NewClass` ("::" as "_"), as IBM's does.
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
    if class_name.is_null() && func.is_null() {
        return 0;
    }
    let mut p: usize = 0;
    if !func.is_null() && unsafe { os2::DosQueryProcAddr(hmod, 0, func, &mut p) } == 0 && p != 0 {
        let f: extern "C" fn(i32, i32, *const u8) = unsafe { core::mem::transmute(p) };
        f(major, minor, class_name);
        if class_name.is_null() || class_exists(unsafe { cstr(class_name) }) {
            return 0;
        }
    }
    if class_name.is_null() {
        return 0;
    }
    let mut name: Vec<u8> = Vec::new();
    let cn = unsafe { cstr(class_name) };
    let mut i = 0;
    while i < cn.len() {
        if cn[i] == b':' && cn.get(i + 1) == Some(&b':') {
            name.push(b'_');
            i += 2;
        } else {
            name.push(cn[i]);
            i += 1;
        }
    }
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
