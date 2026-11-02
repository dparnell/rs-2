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
    inherit_vars: i32,
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
        init_mi(c, cls, cstr(name), &ps, inherit_vars, data_size.max(0) as usize, align, max_methods.max(0) as usize, major, minor)
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
        crate::throw(20169, b"SOMCLS.C\0", 0x7c1);
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

/// somOverrideMtab: every static method but somDispatch goes through its
/// redispatch stub (so that somDispatch sees every call).
pub extern "C" fn somOverrideMtab(cls: usize) {
    let Some(c) = (unsafe { r(cls) }) else { return };
    let _g = lock();
    let dispatch = id::key_of_name(b"somDispatch");
    for &(x, xb) in &c.blocks {
        for m in unsafe { (&mut (*x).methods).iter_mut() } {
            if m.key == dispatch {
                continue;
            }
            let stub = rd_stub_of(m);
            unsafe { c.mt().set(xb + m.slot, stub) };
        }
    }
}

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

/// somGetRdStub: the method's redispatch stub (made from its signature
/// when the compiler gave none, as IBM's does).
pub extern "C" fn somGetRdStub(cls: usize, mid: SomId) -> usize {
    let Some(m) = lookup_any(cls, mid).and_then(|x| x.1) else { return 0 };
    let _g = lock();
    match unsafe { token_info(m.token) } {
        Some(ti) => match unsafe { (&mut (*ti.class).methods).get_mut(ti.index) } {
            Some(md) => rd_stub_of(md),
            None => 0,
        },
        None => m.rd,
    }
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

/// IBM's SOM returns 0 (the field it reads is never set).
pub extern "C" fn somGetInstanceOffset(_cls: usize) -> i32 {
    0
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

/// SOMClass's somUninit: a class object going away (somUnregisterClass)
/// clears its ClassData, so that `<Class>NewClass` builds the class
/// again, as IBM's does; its record stops being valid.
pub extern "C" fn class_somUninit(cls: usize) {
    let Some(c) = (unsafe { r(cls) }) else { return };
    unsafe {
        if !c.cds.is_null() && *c.cds == cls {
            *c.cds = 0;
        }
        if !c.true_class.is_null() {
            let cp = c as *mut Class;
            (*c.true_class).casts.retain(|&x| x != cp);
        }
        c.magic = 0;
        let meta = class_of_obj(cls);
        if !meta.is_null() && (*meta).somclass_off != 0 {
            *((cls + (*meta).somclass_off) as *mut *mut Class) = core::ptr::null_mut();
        }
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

// ---- Casting objects (IBM's private SOMClass methods) ----

/// somCloneClass: a copy of the class under another name, made by its
/// metaclass and then given the class's state (as IBM's copies the class
/// object), with a method table of its own.
pub extern "C" fn somCloneClass(cls: usize, name: *const u8) -> usize {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    let meta = unsafe { (*(*(cls as *const *const Mtab))).class_object };
    let new = crate::call_cm(meta, cm::NEW, &[meta as u32]) as usize;
    if new == 0 {
        return 0;
    }
    unsafe {
        let mr = &*class_of_obj(cls);
        core::ptr::copy_nonoverlapping(cls as *const u8, new as *mut u8, mr.instance_size);
        let rec = clone_record(c, new, cstr(name));
        *((new + mr.somclass_off) as *mut *mut Class) = rec;
    }
    new
}

/// somCastObjCls: make `obj` (an instance of this class or of a cast of
/// it) behave as an instance of its ancestor `to`: its method table
/// becomes that of a cast class ("Dog->Animal") with `to`'s
/// implementations of the methods `to` has, made once per ancestor.
/// Casting to the true class resets the object.
pub extern "C" fn somCastObjCls(cls: usize, to: usize, obj: *mut usize) -> u32 {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    let t = unsafe { &mut *c.true_class() };
    if t.obj == to {
        unsafe { *obj = t.mtab as usize };
        return 1;
    }
    if crate::call_cm(t.obj, cm::DESCENDED_FROM, &[t.obj as u32, to as u32]) & 0xff == 0 {
        return 0;
    }
    let _g = lock();
    let tr = unsafe { rec(to) };
    if let Some(&x) = t.casts.iter().find(|&&x| unsafe { (*x).casted } == tr) {
        unsafe { *obj = (*x).mtab as usize };
        return 1;
    }
    let mut name = t.name_bytes().to_vec();
    name.extend_from_slice(b"->");
    name.extend_from_slice(unsafe { (*tr).name_bytes() });
    name.push(0);
    let clone = crate::call_cm(t.obj, cm::CLONE_CLASS, &[t.obj as u32, name.as_ptr() as u32]) as usize;
    let Some(cr) = (unsafe { r(clone) }) else { return 0 };
    // `to`'s implementations of its (static) methods.
    let tor = unsafe { &*tr };
    for &(x, xb) in &tor.blocks {
        let Some(cb) = cr.block_of(x) else { continue };
        for m in unsafe { &(*x).methods } {
            unsafe { cr.mt().set(cb + m.slot, tor.mt().entry(xb + m.slot)) };
        }
    }
    cr.casted = tr;
    t.casts.push(cr);
    unsafe { *obj = cr.mtab as usize };
    1
}

/// somResetObjCls: give `obj` its true class's method table again.
pub extern "C" fn somResetObjCls(cls: usize, obj: *mut usize) -> u32 {
    if let Some(c) = unsafe { r(cls) } {
        unsafe { *obj = (*c.true_class()).mtab as usize };
    }
    1
}

pub extern "C" fn get_somTrueClass(cls: usize) -> usize {
    unsafe { r(cls) }.map_or(0, |c| unsafe { (*c.true_class()).obj })
}

pub extern "C" fn get_somCastedClass(cls: usize) -> usize {
    unsafe { r(cls) }.map_or(0, |c| if c.casted.is_null() { 0 } else { unsafe { (*c.casted).obj } })
}

// ---- IBM's other private SOMClass methods ----

/// somGetMethodOffset: where a static method's slot is in the class's
/// method table (bytes from its start; 0 for a method it does not have
/// or a dynamic one). SOMU.DLL's cooperative metaclasses (SOMMBeforeAfter)
/// patch method tables through it.
pub extern "C" fn somGetMethodOffset(cls: usize, mid: SomId) -> i32 {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    let (scope, simple) = split_descriptor(id::name_bytes(mid));
    match find(c, id::key_of_name(simple), scope) {
        Some((_, m, xb)) => (MTAB_HDR + 4 * (xb + m.slot)) as i32,
        None => 0,
    }
}

/// somSetMethodDescriptor: the descriptor of a method the class has.
pub extern "C" fn somSetMethodDescriptor(cls: usize, mid: SomId, desc: SomId) -> u32 {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    let (scope, simple) = split_descriptor(id::name_bytes(mid));
    let key = id::key_of_name(simple);
    let Some((x, _, _)) = find(c, key, scope) else { return 0 };
    let x = x as *const Class as *mut Class;
    if let Some(m) = unsafe { (*x).methods.iter_mut().find(|m| m.key == key) } {
        m.descriptor = desc;
    }
    1
}

/// somLookupTerminal: the class's own implementation of a static method
/// (somResolveTerminal), or somLookupMethod's for a dynamic one.
pub extern "C" fn somLookupTerminal(cls: usize, mid: SomId) -> usize {
    let tok = somGetMethodToken(cls, mid);
    if tok != 0 {
        return crate::somResolveTerminal(cls, tok);
    }
    somLookupMethod(cls, mid)
}

/// set_somDirectInitClasses: the classes whose initializers the class's
/// initializers call (those it inherits the implementation of).
pub extern "C" fn set_somDirectInitClasses(cls: usize, seq: *const Seq) {
    let Some(c) = (unsafe { r(cls) }) else { return };
    let mut v = Vec::new();
    if !seq.is_null() {
        let s = unsafe { &*seq };
        for i in 0..s.length as usize {
            let x = unsafe { rec(*s.buffer.add(i)) };
            if let Some(k) = c.blocks.iter().position(|b| b.0 == x) {
                if c.flags.get(k).is_none_or(|f| f & BLOCK_IMPL != 0) {
                    v.push(x);
                }
            }
        }
    }
    c.direct_init = v;
}

/// somGetInstanceInitMask, somGetInstanceDestructionMask,
/// somGetInstanceAssignmentMask: the class's control (copied to `ctrl`
/// when given); the mask.
fn instance_mask(cls: usize, ctrl: *mut Ctrl, which: u32) -> *const u8 {
    let Some(c) = (unsafe { r(cls) }) else { return core::ptr::null() };
    {
        let _g = lock();
        if !c.ctrl_built {
            unsafe { build_ctrls(c) };
        }
    }
    let pm = unsafe { &*c.pmtab };
    let x = match which {
        0 => pm.init_ctrl,
        1 => pm.destruct_ctrl,
        _ => pm.assign_ctrl,
    };
    if !ctrl.is_null() {
        unsafe { *ctrl = x };
    }
    x.mask
}

pub extern "C" fn somGetInstanceInitMask(cls: usize, ctrl: *mut Ctrl) -> *const u8 {
    instance_mask(cls, ctrl, 0)
}
pub extern "C" fn somGetInstanceDestructionMask(cls: usize, ctrl: *mut Ctrl) -> *const u8 {
    instance_mask(cls, ctrl, 1)
}
pub extern "C" fn somGetInstanceAssignmentMask(cls: usize, ctrl: *mut Ctrl) -> *const u8 {
    instance_mask(cls, ctrl, 2)
}

/// somRegLPMToken: nothing to do (rs2's method tokens already resolve
/// any method).
pub extern "C" fn somRegLPMToken(_cls: usize, _token: *mut usize) {}

/// somAddMethod: by kind, a static method (0, 2; its token), a dynamic
/// one (1), the class's allocator (3) or deallocator (4).
pub extern "C" fn somAddMethod(cls: usize, kind: i32, mid: SomId, desc: SomId, method: usize, rd: usize, ap: usize) -> usize {
    match kind {
        0 | 2 => somAddStaticMethod(cls, mid, desc, method, rd, ap),
        1 => {
            somAddDynamicMethod(cls, mid, desc, method, ap);
            0
        }
        3 | 4 => {
            if let Some(c) = unsafe { r(cls) } {
                if kind == 3 {
                    c.allocate = method;
                } else {
                    c.deallocate = method;
                }
            }
            0
        }
        _ => kind as usize,
    }
}

pub extern "C" fn get_somCClassData(cls: usize) -> *mut usize {
    unsafe { r(cls) }.map_or(core::ptr::null_mut(), |c| c.ccds)
}

pub extern "C" fn set_somCClassData(cls: usize, ccds: *mut usize) {
    if let Some(c) = unsafe { r(cls) } {
        c.ccds = ccds;
    }
}

pub extern "C" fn set_somClassDataOrder(cls: usize, seq: *const Seq) {
    let Some(c) = (unsafe { r(cls) }) else { return };
    c.cd_order.clear();
    if !seq.is_null() {
        let s = unsafe { &*seq };
        for i in 0..s.length as usize {
            c.cd_order.push(unsafe { *s.buffer.add(i) } as SomId);
        }
    }
}

pub extern "C" fn get_somClassDataOrder(ret: *mut Seq, cls: usize) -> *mut Seq {
    let items: Vec<usize> = unsafe { r(cls) }.map_or(Vec::new(), |c| c.cd_order.iter().map(|&i| i as usize).collect());
    unsafe { *ret = seq_of(&items) };
    ret
}

/// somGetClassDataEntry / somSetClassDataEntry: the ClassData entry named
/// in the class data order. IBM's index the ClassData at (n+1)*32 bytes
/// (not *4); kept, as code may depend on where it lands.
fn class_data_entry(cls: usize, name: SomId) -> *mut usize {
    let Some(c) = (unsafe { r(cls) }) else { return core::ptr::null_mut() };
    if c.cds.is_null() {
        return core::ptr::null_mut();
    }
    match c.cd_order.iter().position(|&i| id::somCompareIds(i, name) != 0) {
        Some(n) => (c.cds as usize + (n + 1) * 32) as *mut usize,
        None => core::ptr::null_mut(),
    }
}

pub extern "C" fn somGetClassDataEntry(cls: usize, name: SomId) -> usize {
    let p = class_data_entry(cls, name);
    if p.is_null() { 0 } else { unsafe { *p } }
}

pub extern "C" fn somSetClassDataEntry(cls: usize, name: SomId, value: usize) -> u32 {
    let p = class_data_entry(cls, name);
    if p.is_null() {
        return 0;
    }
    unsafe { *p = value };
    1
}

/// _get_somDirectAssignClasses: the direct init classes, or {1, 0}
/// when the class has its own allocator or deallocator (as IBM's).
pub extern "C" fn get_somDirectAssignClasses(ret: *mut Seq, cls: usize) -> *mut Seq {
    let Some(c) = (unsafe { r(cls) }) else {
        unsafe { *ret = Seq { maximum: 0, length: 0, buffer: core::ptr::null_mut() } };
        return ret;
    };
    if c.allocate != 0 || c.deallocate != 0 {
        unsafe { *ret = Seq { maximum: 1, length: 0, buffer: core::ptr::null_mut() } };
        return ret;
    }
    let items: Vec<usize> = c.direct_init.iter().map(|&p| unsafe { (*p).obj }).collect();
    unsafe { *ret = seq_of(&items) };
    ret
}

pub extern "C" fn setUserPCallDispatch(cls: usize, old: usize, new: usize) -> u32 {
    let Some(c) = (unsafe { r(cls) }) else { return 0 };
    if c.pcall_dispatch != old {
        return 0;
    }
    c.pcall_dispatch = new;
    1
}

pub extern "C" fn get_somClassAllocate(cls: usize) -> usize {
    unsafe { r(cls) }.map_or(0, |c| c.allocate)
}

pub extern "C" fn get_somClassDeallocate(cls: usize) -> usize {
    unsafe { r(cls) }.map_or(0, |c| c.deallocate)
}

pub extern "C" fn private_stub(_cls: usize) -> usize {
    0
}

/// SOMClass's method slots (release order).
pub mod cm {
    pub const NEW: usize = 0;
    pub const INIT_MI_CLASS: usize = 40;
    pub const CLASS_READY: usize = 3;
    pub const DESCENDED_FROM: usize = 6;
    pub const CHECK_VERSION: usize = 7;
    pub const ADD_STATIC_METHOD: usize = 19;
    pub const OVERRIDE_S_METHOD: usize = 20;
    pub const ADD_DYNAMIC_METHOD: usize = 21;
    pub const SET_CLASS_DATA: usize = 28;
    pub const GET_INSTANCE_TOKEN: usize = 32;
    pub const GET_METHOD_DATA: usize = 35;
    pub const GET_MEMBER_TOKEN: usize = 33;
    pub const ALLOCATE: usize = 46;
    pub const DEALLOCATE: usize = 47;
    pub const CLONE_CLASS: usize = 50;
    pub const CAST_OBJ_CLS: usize = 56;
    pub const RESET_OBJ_CLS: usize = 57;
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
    pub const UNLOAD_CLASS_FILE: usize = 7;
    pub const GET_INIT_FUNCTION: usize = 8;
    pub const GET_RELATED_CLASSES: usize = 10;
    pub const BEGIN_PERSISTENT_CLASSES: usize = 15;
    pub const END_PERSISTENT_CLASSES: usize = 16;
    pub const REGISTER_THREAD_USAGE: usize = 18;
    pub const REGISTER_CLASS_LIBRARY: usize = 19;
    pub const JOIN_AFFINITY_GROUP: usize = 20;
    pub const UNREGISTER_CLASS_LIBRARY: usize = 21;
}

