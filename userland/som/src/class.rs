//! SOMClass's methods: making instances, building classes, and what
//! classes tell about themselves.

use alloc::vec::Vec;

use crate::id::{self, SomId};
use crate::rt::*;
use crate::sys::*;

/// `_IDL_SEQUENCE_x`: maximum, length, buffer.
#[repr(C)]
pub struct Seq {
    pub maximum: u32,
    pub length: u32,
    pub buffer: *mut usize,
}

/// somMethodData.
#[repr(C)]
pub struct MethodData {
    pub id: SomId,
    pub kind: i32,
    pub descriptor: SomId,
    pub token: usize,
    pub method: usize,
    pub shared: usize,
}

fn seq_of(items: &[usize]) -> Seq {
    let buf = crate::malloc((items.len().max(1) * 4) as u32) as *mut usize;
    if !buf.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(items.as_ptr(), buf, items.len()) };
    }
    Seq { maximum: items.len() as u32, length: items.len() as u32, buffer: buf }
}

unsafe fn r<'a>(cls: usize) -> Option<&'a mut Class> {
    unsafe { rec_ok(cls) }
}

// ---- Instances ----

pub fn allocate(cls: usize, size: u32) -> usize {
    crate::call_cm(cls, cm::ALLOCATE, &[cls as u32, size]) as usize
}

pub fn deallocate(cls: usize, mem: usize) {
    crate::call_cm(cls, cm::DEALLOCATE, &[cls as u32, mem as u32]);
}

unsafe fn renew(cls: usize, mem: usize, zero: bool, init: bool) -> usize {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    if mem == 0 {
        return 0;
    }
    unsafe {
        if zero {
            core::ptr::write_bytes(mem as *mut u8, 0, c.instance_size);
        }
        *(mem as *mut usize) = c.mtab as usize;
        if init {
            if !c.ctrl_built {
                build_ctrls(c);
            }
            let mut ctrl = (*c.pmtab).init_ctrl;
            crate::call_om(mem, om::DEFAULT_INIT, &[mem as u32, &mut ctrl as *mut Ctrl as u32]);
        }
    }
    mem
}

pub extern "C" fn somNew(cls: usize) -> usize {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    let size = c.instance_size as u32;
    let mem = allocate(cls, size);
    unsafe { renew(cls, mem, true, true) }
}

pub extern "C" fn somNewNoInit(cls: usize) -> usize {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    let size = c.instance_size as u32;
    let mem = allocate(cls, size);
    unsafe { renew(cls, mem, true, false) }
}

pub extern "C" fn somRenew(cls: usize, mem: usize) -> usize {
    unsafe { renew(cls, mem, true, true) }
}
pub extern "C" fn somRenewNoInit(cls: usize, mem: usize) -> usize {
    unsafe { renew(cls, mem, true, false) }
}
pub extern "C" fn somRenewNoZero(cls: usize, mem: usize) -> usize {
    unsafe { renew(cls, mem, false, true) }
}
pub extern "C" fn somRenewNoInitNoZero(cls: usize, mem: usize) -> usize {
    unsafe { renew(cls, mem, false, false) }
}

pub extern "C" fn somAllocate(cls: usize, size: i32) -> usize {
    let size = size.max(4) as u32;
    if let Some(c) = unsafe { r(cls) } {
        if c.allocate != 0 {
            let f: extern "C" fn(usize, u32) -> usize = unsafe { core::mem::transmute(c.allocate) };
            return f(cls, size);
        }
    }
    crate::malloc(size) as usize
}

pub extern "C" fn somDeallocate(cls: usize, mem: usize) {
    if mem == 0 {
        return;
    }
    unsafe { *(mem as *mut usize) = 0 };
    if let Some(c) = unsafe { r(cls) } {
        if c.deallocate != 0 {
            let f: extern "C" fn(usize, u32) = unsafe { core::mem::transmute(c.deallocate) };
            f(mem, c.instance_size as u32);
            return;
        }
    }
    crate::free(mem as *mut u8);
}

// ---- Building ----

/// The class record of a class object being built (made on first use).
pub unsafe fn rec_or_new(cls: usize) -> *mut Class {
    unsafe {
        let meta = class_of_obj(cls);
        let off = (*meta).somclass_off;
        if off == 0 {
            return core::ptr::null_mut();
        }
        let slot = (cls + off) as *mut *mut Class;
        if (*slot).is_null() {
            *slot = Class::new();
        }
        *slot
    }
}

pub extern "C" fn somInitMIClass(
    cls: usize,
    _inherit_vars: i32,
    name: *const u8,
    parents: *const Seq,
    data_size: i32,
    align: i32,
    max_methods: i32,
    major: i32,
    minor: i32,
) {
    let c = unsafe { rec_or_new(cls) };
    if c.is_null() {
        return;
    }
    let mut ps: Vec<*mut Class> = Vec::new();
    if !parents.is_null() {
        let s = unsafe { &*parents };
        for i in 0..s.length as usize {
            let p = unsafe { rec(*s.buffer.add(i)) };
            if !p.is_null() {
                ps.push(p);
            }
        }
    }
    if ps.is_empty() && !core::ptr::eq(c, unsafe { SOMOBJECT }) && unsafe { !SOMOBJECT.is_null() } {
        ps.push(unsafe { SOMOBJECT });
    }
    let align = if align <= 0 { 4 } else { align as usize };
    unsafe {
        init_mi(c, cls, cstr(name), &ps, data_size.max(0) as usize, align, max_methods.max(0) as usize, major, minor)
    };
}

pub extern "C" fn somInitClass(
    cls: usize,
    name: *const u8,
    parent: usize,
    data_size: i32,
    max_methods: i32,
    major: i32,
    minor: i32,
) {
    let mut p = [parent];
    let seq = Seq { maximum: 1, length: (parent != 0) as u32, buffer: p.as_mut_ptr() };
    somInitMIClass(cls, 1, name, &seq, data_size, 4, max_methods, major, minor);
}

pub extern "C" fn somClassReady(cls: usize) {
    let Some(c) = (unsafe { r(cls) }) else { return };
    if c.ready {
        return;
    }
    c.ready = true;
    if !c.ctrl_built {
        unsafe { build_ctrls(c) };
    }
    let mgr = crate::class_mgr();
    if mgr != 0 {
        crate::call_mm(mgr, mm::REGISTER_CLASS, &[mgr as u32, cls as u32]);
    }
}

pub extern "C" fn somAddStaticMethod(cls: usize, mid: SomId, desc: SomId, method: usize, rd: usize, ap: usize) -> usize {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    unsafe { add_static(c, mid, desc, method, rd, ap) }
}

pub extern "C" fn somOverrideSMethod(cls: usize, mid: SomId, method: usize) {
    let Some(c) = (unsafe { r(cls) }) else { return };
    if !unsafe { override_method(c, mid, method) } {
        crate::warn(b"somOverrideSMethod: no method %s in class %s\n\0", &[id::name_of(mid) as u32, c.name as u32]);
    }
}

pub extern "C" fn somAddDynamicMethod(cls: usize, mid: SomId, desc: SomId, method: usize, ap: usize) {
    let Some(c) = (unsafe { r(cls) }) else { return };
    unsafe { add_dynamic(c, mid, desc, method, ap) };
}

pub extern "C" fn somSetClassData(cls: usize, cds: *mut usize) {
    if let Some(c) = unsafe { r(cls) } {
        c.cds = cds;
    }
}

pub extern "C" fn somGetClassData(cls: usize) -> *mut usize {
    unsafe { r(cls) }.map_or(core::ptr::null_mut(), |c| c.cds)
}

pub extern "C" fn somOverrideMtab(_cls: usize) {}

// ---- Queries ----

pub extern "C" fn somGetName(cls: usize) -> *const u8 {
    unsafe { r(cls) }.map_or(core::ptr::null(), |c| c.name)
}

pub extern "C" fn somGetParent(cls: usize) -> usize {
    unsafe { r(cls) }.and_then(|c| c.parents.first().map(|&p| unsafe { (*p).obj })).unwrap_or(0)
}

pub extern "C" fn somGetParents(ret: *mut Seq, cls: usize) -> *mut Seq {
    let items: Vec<usize> = unsafe { r(cls) }.map_or(Vec::new(), |c| c.parents.iter().map(|&p| unsafe { (*p).obj }).collect());
    unsafe { *ret = seq_of(&items) };
    ret
}

pub extern "C" fn somDescendedFrom(cls: usize, other: usize) -> u32 {
    let o = unsafe { rec(other) };
    (unsafe { r(cls) }.is_some_and(|c| !o.is_null() && c.descends_from(o))) as u32
}

pub extern "C" fn somCheckVersion(cls: usize, major: i32, minor: i32) -> u32 {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    ((major == 0 || major == c.major) && (minor == 0 || minor <= c.minor)) as u32
}

pub extern "C" fn somGetVersionNumbers(cls: usize, major: *mut i32, minor: *mut i32) {
    let (a, b) = unsafe { r(cls) }.map_or((0, 0), |c| (c.major, c.minor));
    unsafe {
        if !major.is_null() {
            *major = a;
        }
        if !minor.is_null() {
            *minor = b;
        }
    }
}

fn lookup_static(cls: usize, mid: SomId) -> Option<(usize, &'static MethodDef)> {
    let c = unsafe { r(cls) }?;
    let (scope, simple) = split_descriptor(id::name_bytes(mid));
    let key = id::key_of_name(simple);
    let (_, m, xb) = find(c, key, scope)?;
    Some((unsafe { c.mt().entry(xb + m.slot) }, m))
}

fn lookup_any(cls: usize, mid: SomId) -> Option<(usize, Option<&'static MethodDef>)> {
    let c = unsafe { r(cls) }?;
    lookup(c, id::name_bytes(mid))
}

pub extern "C" fn somFindMethod(cls: usize, mid: SomId, m: *mut usize) -> u32 {
    let p = lookup_any(cls, mid).map_or(0, |x| x.0);
    if !m.is_null() {
        unsafe { *m = p };
    }
    (p != 0) as u32
}

pub extern "C" fn somFindMethodOk(cls: usize, mid: SomId, m: *mut usize) -> u32 {
    let ok = somFindMethod(cls, mid, m);
    if ok == 0 {
        not_found(cls, mid);
    }
    ok
}

fn not_found(cls: usize, mid: SomId) {
    crate::warn(b"method %s not found in class %s\n\0", &[id::name_of(mid) as u32, somGetName(cls) as u32]);
    crate::error(20109, b"SOMCLS.C\0", 1);
}

pub extern "C" fn somFindSMethod(cls: usize, mid: SomId) -> usize {
    lookup_static(cls, mid).map_or(0, |x| x.0)
}

pub extern "C" fn somFindSMethodOk(cls: usize, mid: SomId) -> usize {
    let p = somFindSMethod(cls, mid);
    if p == 0 {
        not_found(cls, mid);
    }
    p
}

pub extern "C" fn somLookupMethod(cls: usize, mid: SomId) -> usize {
    lookup_any(cls, mid).map_or(0, |x| x.0)
}

pub extern "C" fn somSupportsMethod(cls: usize, mid: SomId) -> u32 {
    lookup_any(cls, mid).is_some() as u32
}

pub extern "C" fn somGetMethodToken(cls: usize, mid: SomId) -> usize {
    lookup_static(cls, mid).map_or(0, |x| x.1.token)
}

pub extern "C" fn somGetMethodDescriptor(cls: usize, mid: SomId) -> SomId {
    lookup_any(cls, mid).and_then(|x| x.1).map_or(core::ptr::null_mut(), |m| m.descriptor)
}

pub extern "C" fn somGetApplyStub(cls: usize, mid: SomId) -> usize {
    lookup_any(cls, mid).and_then(|x| x.1).map_or(0, |m| m.ap)
}

pub extern "C" fn somGetRdStub(cls: usize, mid: SomId) -> usize {
    lookup_any(cls, mid).and_then(|x| x.1).map_or(0, |m| m.rd)
}

/// All methods of a class: its blocks' static methods in table order,
/// then the dynamic ones.
fn all_methods(c: &Class) -> Vec<(usize, &MethodDef)> {
    let mut v = Vec::new();
    for &(x, xb) in &c.blocks {
        for m in unsafe { &(*x).methods } {
            v.push((unsafe { c.mt().entry(xb + m.slot) }, m));
        }
    }
    let mut cur: Vec<*mut Class> = Vec::from([c as *const Class as *mut Class]);
    while let Some(x) = cur.pop() {
        for m in unsafe { &(*x).dynamic } {
            if !v.iter().any(|e| e.1.key == m.key) {
                v.push((m.proc_, m));
            }
        }
        cur.extend_from_slice(unsafe { &(*x).parents });
    }
    v
}

pub extern "C" fn somGetNumMethods(cls: usize) -> i32 {
    unsafe { r(cls) }.map_or(0, |c| all_methods(c).len() as i32)
}

pub extern "C" fn somGetNumStaticMethods(cls: usize) -> i32 {
    unsafe { r(cls) }.map_or(0, |c| c.blocks.iter().map(|b| unsafe { (*b.0).methods.len() }).sum::<usize>() as i32)
}

pub extern "C" fn somGetMethodIndex(cls: usize, mid: SomId) -> i32 {
    let Some(c) = (unsafe { r(cls) }) else { return -1 };
    let key = id::key_of_name(split_descriptor(id::name_bytes(mid)).1);
    all_methods(c).iter().position(|m| m.1.key == key).map_or(-1, |i| i as i32)
}

fn fill(md: *mut MethodData, p: usize, m: &MethodDef) {
    if md.is_null() {
        return;
    }
    unsafe {
        *md = MethodData {
            id: id::static_id(cstr(m.name)),
            kind: if m.token == 0 { 1 } else { 0 },
            descriptor: m.descriptor,
            token: m.token,
            method: p,
            shared: 0,
        };
    }
}

pub extern "C" fn somGetMethodData(cls: usize, mid: SomId, md: *mut MethodData) -> u32 {
    match lookup_any(cls, mid) {
        Some((p, Some(m))) => {
            fill(md, p, m);
            1
        }
        _ => 0,
    }
}

pub extern "C" fn somGetNthMethodData(cls: usize, n: i32, md: *mut MethodData) -> u32 {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    match all_methods(c).get(n.max(0) as usize) {
        Some(&(p, m)) if n >= 0 => {
            fill(md, p, m);
            1
        }
        _ => 0,
    }
}

pub extern "C" fn somGetNthMethodInfo(cls: usize, n: i32, desc: *mut SomId) -> SomId {
    let Some(c) = (unsafe { r(cls) }) else { return core::ptr::null_mut() };
    match all_methods(c).get(n.max(0) as usize) {
        Some(&(_, m)) if n >= 0 => {
            if !desc.is_null() {
                unsafe { *desc = m.descriptor };
            }
            id::static_id(unsafe { cstr(m.name) })
        }
        _ => core::ptr::null_mut(),
    }
}

pub extern "C" fn somDefinedMethod(cls: usize, token: usize) -> usize {
    unsafe { r(cls) }.map_or(0, |c| unsafe { resolve_in(c, token) })
}

pub extern "C" fn somGetInstanceSize(cls: usize) -> i32 {
    unsafe { r(cls) }.map_or(0, |c| c.instance_size as i32)
}

pub extern "C" fn somGetInstanceOffset(cls: usize) -> i32 {
    unsafe { r(cls) }.map_or(0, |c| c.own_offset as i32)
}

pub extern "C" fn somGetInstancePartSize(cls: usize) -> i32 {
    unsafe { r(cls) }.map_or(0, |c| c.data_size as i32)
}

pub extern "C" fn somGetInstanceToken(cls: usize) -> usize {
    unsafe { r(cls) }.map_or(0, |c| c.inst_token)
}

pub extern "C" fn somGetMemberToken(cls: usize, offset: i32, inst_token: usize) -> usize {
    let intro = match unsafe { token_info(inst_token) } {
        Some(ti) if ti.kind == 1 => ti.class,
        _ => match unsafe { r(cls) } {
            Some(c) => c,
            None => return 0,
        },
    };
    let base = unsafe { token_info(inst_token) }.map_or(0, |ti| ti.slot);
    data_token(intro, base + offset.max(0) as usize)
}

pub extern "C" fn somGetPClsMtab(cls: usize) -> *mut ParentMtab {
    unsafe { r(cls) }.map_or(core::ptr::null_mut(), |c| c.pmtab)
}

pub extern "C" fn somGetPClsMtabs(cls: usize) -> *mut MtabList {
    unsafe { r(cls) }.map_or(core::ptr::null_mut(), |c| unsafe { (*c.pmtab).next })
}

pub extern "C" fn somGetClassMtab(cls: usize) -> *mut Mtab {
    unsafe { r(cls) }.map_or(core::ptr::null_mut(), |c| c.mtab)
}

pub extern "C" fn get_somInstanceDataOffsets(ret: *mut Seq, cls: usize) -> *mut Seq {
    let mut items = Vec::new();
    if let Some(c) = unsafe { r(cls) } {
        for &(x, off) in &c.data_offsets {
            items.push(unsafe { (*x).obj });
            items.push(off);
        }
    }
    let mut s = seq_of(&items);
    s.length /= 2;
    s.maximum /= 2;
    unsafe { *ret = s };
    ret
}

pub extern "C" fn get_somDirectInitClasses(ret: *mut Seq, cls: usize) -> *mut Seq {
    let items: Vec<usize> = unsafe { r(cls) }.map_or(Vec::new(), |c| c.direct_init.iter().map(|&p| unsafe { (*p).obj }).collect());
    unsafe { *ret = seq_of(&items) };
    ret
}

pub extern "C" fn get_somDataAlignment(cls: usize) -> i32 {
    unsafe { r(cls) }.map_or(4, |c| c.align as i32)
}

/// SOMClass's own somFree: classes stay (they are in use by their
/// instances and other classes); it only unregisters them.
pub extern "C" fn class_somFree(cls: usize) {
    let mgr = crate::class_mgr();
    if mgr != 0 {
        crate::call_mm(mgr, mm::UNREGISTER_CLASS, &[mgr as u32, cls as u32]);
    }
}

pub extern "C" fn class_somDumpSelfInt(cls: usize, level: i32) {
    crate::prefix_level(level);
    crate::printf(b"Class: %s (%d methods, instance size %d)\n\0", &[
        somGetName(cls) as u32,
        somGetNumMethods(cls) as u32,
        somGetInstanceSize(cls) as u32,
    ]);
}

pub extern "C" fn private_stub(_cls: usize) -> usize {
    0
}

/// SOMClass's method slots (release order).
pub mod cm {
    pub const NEW: usize = 0;
    pub const INIT_MI_CLASS: usize = 40;
    pub const CLASS_READY: usize = 3;
    pub const CHECK_VERSION: usize = 7;
    pub const ADD_STATIC_METHOD: usize = 19;
    pub const OVERRIDE_S_METHOD: usize = 20;
    pub const ADD_DYNAMIC_METHOD: usize = 21;
    pub const SET_CLASS_DATA: usize = 28;
    pub const GET_INSTANCE_TOKEN: usize = 32;
    pub const GET_MEMBER_TOKEN: usize = 33;
    pub const ALLOCATE: usize = 46;
    pub const DEALLOCATE: usize = 47;
}

/// SOMClassMgr's method slots.
pub mod mm {
    pub const FIND_CLS_IN_FILE: usize = 0;
    pub const FIND_CLASS: usize = 1;
    pub const CLASS_FROM_ID: usize = 2;
    pub const REGISTER_CLASS: usize = 3;
    pub const UNREGISTER_CLASS: usize = 4;
    pub const LOCATE_CLASS_FILE: usize = 5;
    pub const LOAD_CLASS_FILE: usize = 6;
    pub const GET_INIT_FUNCTION: usize = 8;
    pub const REGISTER_CLASS_LIBRARY: usize = 19;
}

