//! The interface repository classes' methods (callstyle idl: the object,
//! then the environment; methods returning structures get the result's
//! address first).

use alloc::vec::Vec;

use somsup::{Environment, call_parent, data, methods};

use crate::model::*;
use crate::tc::{self, tc_ptr};
use crate::*;

type Ev = *mut Environment;

// ---- Contained ----

extern "C" fn within(ret: *mut Seq, o: usize, _ev: Ev) -> *mut Seq {
    let c = cd(o).container;
    let mut v = Vec::new();
    if c != 0 {
        if is_sub(c) {
            if unsafe { MAIN } != 0 {
                v.push(unsafe { MAIN });
            }
        } else {
            v.push(used(c));
            if is_interface(c) {
                let id = unsafe { somsup::cstr(cd(c).id) }.to_vec();
                subclasses(&id, &mut v);
            }
        }
    }
    unsafe { *ret = Seq::of(&v) };
    ret
}

#[repr(C)]
pub struct Description {
    name: *const u8,
    tc: usize,
    value: usize,
}

extern "C" fn describe(ret: *mut Description, _o: usize, _ev: Ev) -> *mut Description {
    unsafe { *ret = Description { name: core::ptr::null(), tc: 0, value: 0 } };
    ret
}

extern "C" fn get_name(o: usize, _ev: Ev) -> *const u8 {
    cd(o).name
}
extern "C" fn set_name(o: usize, _ev: Ev, s: *const u8) {
    if s.is_null() {
        return;
    }
    let c = cd(o);
    somsup::free(c.name);
    somsup::free(c.id);
    c.name = somsup::dup(unsafe { somsup::cstr(s) });
    let mut v = Vec::new();
    let p = c.container;
    if p != 0 && !is_sub(p) && is_contained(p) {
        v.extend_from_slice(unsafe { somsup::cstr(cd(p).id) });
    }
    v.extend_from_slice(b"::");
    v.extend_from_slice(unsafe { somsup::cstr(s) });
    c.id = somsup::dup(&v);
}
extern "C" fn get_id(o: usize, _ev: Ev) -> *const u8 {
    cd(o).id
}
extern "C" fn set_id(o: usize, _ev: Ev, s: *const u8) {
    if !s.is_null() {
        let c = cd(o);
        somsup::free(c.id);
        c.id = somsup::dup(unsafe { somsup::cstr(s) });
    }
}
extern "C" fn get_defined_in(o: usize, _ev: Ev) -> *const u8 {
    defined_in(o)
}
extern "C" fn set_defined_in(_o: usize, _ev: Ev, _s: *const u8) {}

extern "C" fn get_modifiers(ret: *mut Seq, o: usize, _ev: Ev) -> *mut Seq {
    unsafe { *ret = cd(o).mods };
    ret
}
extern "C" fn set_modifiers(o: usize, _ev: Ev, s: *const Seq) {
    if s.is_null() {
        return;
    }
    let src = unsafe { &*s };
    let mut pairs = Vec::new();
    for i in 0..src.len as usize {
        unsafe {
            pairs.push(somsup::dup(somsup::cstr(*src.buf.add(2 * i) as *const u8)) as usize);
            let v = *src.buf.add(2 * i + 1) as *const u8;
            pairs.push(if v.is_null() { 0 } else { somsup::dup(somsup::cstr(v)) as usize });
        }
    }
    let mut n = Seq::of(&pairs);
    n.max = src.len;
    n.len = src.len;
    cd(o).mods = n;
}
extern "C" fn get_repository(o: usize, _ev: Ev) -> usize {
    cd(o).sub
}
extern "C" fn set_repository(o: usize, _ev: Ev, r: usize) {
    cd(o).sub = r;
}
extern "C" fn get_iid(o: usize, _ev: Ev) -> u32 {
    cd(o).iid
}
extern "C" fn get_defined_in_obj(o: usize, _ev: Ev) -> usize {
    cd(o).container
}
extern "C" fn increment_uses(o: usize, _ev: Ev) {
    cd(o).uses += 1;
}
extern "C" fn decrement_uses(o: usize, _ev: Ev) {
    let c = cd(o);
    c.uses = c.uses.saturating_sub(1);
}
/// somFree: one use fewer (the object stays: the repository keeps it).
extern "C" fn contained_free(o: usize) {
    decrement_uses(o, core::ptr::null_mut());
}

methods! {
    __m_within => within, 3;
    __m_describe => describe, 3;
    __m_get_name => get_name, 2;
    __m_set_name => set_name, 3;
    __m_get_id => get_id, 2;
    __m_set_id => set_id, 3;
    __m_get_defined_in => get_defined_in, 2;
    __m_set_defined_in => set_defined_in, 3;
    __m_get_modifiers => get_modifiers, 3;
    __m_set_modifiers => set_modifiers, 3;
    __m_get_repository => get_repository, 2;
    __m_set_repository => set_repository, 3;
    __m_get_iid => get_iid, 2;
    __m_get_defined_in_obj => get_defined_in_obj, 2;
    __m_increment_uses => increment_uses, 2;
    __m_decrement_uses => decrement_uses, 2;
    __m_contained_free => contained_free, 1;
}

// ---- Container ----

extern "C" fn contents(ret: *mut Seq, c: usize, _ev: Ev, limit: *const u8, exclude_inherited: u32) -> *mut Seq {
    let mut v = Vec::new();
    unsafe { somsup::somStartCriticalSection() };
    lookup(c, None, &mut v, 1, limit_code(limit), exclude_inherited & 0xff != 0, false);
    unsafe { somsup::somEndCriticalSection() };
    unsafe { *ret = Seq::of(&v) };
    ret
}

extern "C" fn lookup_name(ret: *mut Seq, c: usize, _ev: Ev, name: *const u8, levels: i32, limit: *const u8, exclude_inherited: u32) -> *mut Seq {
    let mut v = Vec::new();
    if !name.is_null() {
        unsafe { somsup::somStartCriticalSection() };
        let n = unsafe { somsup::cstr(name) };
        lookup(c, Some(n), &mut v, if levels == 1 { 1 } else { -1 }, limit_code(limit), exclude_inherited & 0xff != 0, true);
        unsafe { somsup::somEndCriticalSection() };
    }
    unsafe { *ret = Seq::of(&v) };
    ret
}

/// ContainerDescription: { contained_object, name, value }.
extern "C" fn describe_contents(ret: *mut Seq, c: usize, ev: Ev, limit: *const u8, exclude_inherited: u32, max: i32) -> *mut Seq {
    let mut v = Vec::new();
    unsafe { somsup::somStartCriticalSection() };
    lookup(c, None, &mut v, 1, limit_code(limit), exclude_inherited & 0xff != 0, false);
    let n = if max < 0 { v.len() } else { v.len().min(max as usize) };
    let buf = somsup::malloc(n.max(1) * 16) as *mut usize;
    for (i, &o) in v.iter().take(n).enumerate() {
        let mut d = Description { name: core::ptr::null(), tc: 0, value: 0 };
        somsup::call(p!(ContainedClassData), 1, &[&mut d as *mut _ as usize, o, ev as usize]);
        unsafe {
            *buf.add(4 * i) = o;
            *buf.add(4 * i + 1) = d.name as usize;
            *buf.add(4 * i + 2) = d.tc;
            *buf.add(4 * i + 3) = d.value;
        }
    }
    drop_uses(&v[n..]);
    unsafe { somsup::somEndCriticalSection() };
    unsafe { *ret = Seq { max: n as u32, len: n as u32, buf } };
    ret
}

/// internalNameLookup(name, seq, levels, limit, exclude_inherited, up):
/// IBM's private lookup, adding to `seq`.
extern "C" fn internal_lookup(c: usize, _ev: Ev, name: *const u8, seq: *mut Seq, levels: i32, limit: *const u8, exclude: u32, up: u32) {
    let mut v: Vec<usize> = if seq.is_null() { Vec::new() } else { unsafe { (*seq).items().to_vec() } };
    let nm = if name.is_null() { None } else { Some(unsafe { somsup::cstr(name) }) };
    lookup(c, nm, &mut v, levels, limit_code(limit), exclude & 0xff != 0, up & 0xff != 0);
    if !seq.is_null() {
        unsafe {
            if !(*seq).buf.is_null() {
                somsup::free((*seq).buf as *mut u8);
            }
            *seq = Seq::of(&v);
        }
    }
}

methods! {
    __m_contents => contents, 5;
    __m_lookup_name => lookup_name, 7;
    __m_describe_contents => describe_contents, 6;
    __m_internal_lookup => internal_lookup, 8;
}

// ---- Repository ----

extern "C" fn repo_lookup_id(_r: usize, _ev: Ev, id: *const u8) -> usize {
    if id.is_null() {
        return 0;
    }
    unsafe { somsup::somStartCriticalSection() };
    let o = lookup_id(unsafe { somsup::cstr(id) });
    unsafe { somsup::somEndCriticalSection() };
    o
}

/// lookup_modifier(id, modifier): a copy of the modifier's value ("" when
/// it has none), NULL when the object or the modifier is not there.
extern "C" fn lookup_modifier(r: usize, ev: Ev, id: *const u8, modifier: *const u8) -> *mut u8 {
    let o = repo_lookup_id(r, ev, id);
    if o == 0 {
        return core::ptr::null_mut();
    }
    let m = unsafe { somsup::cstr(modifier) };
    let mods = cd(o).mods;
    let mut out = core::ptr::null_mut();
    for i in 0..mods.len as usize {
        let (n, v) = unsafe { (*mods.buf.add(2 * i) as *const u8, *mods.buf.add(2 * i + 1) as *const u8) };
        if unsafe { somsup::cstr(n) } == m {
            out = somsup::dup(if v.is_null() { b"" } else { unsafe { somsup::cstr(v) } });
            break;
        }
    }
    decrement_uses(o, ev);
    out
}

extern "C" fn release_cache(_r: usize, _ev: Ev) {}
extern "C" fn save_ir(_r: usize, _ev: Ev) {}

extern "C" fn get_subrepositories(ret: *mut Seq, _r: usize, _ev: Ev) -> *mut Seq {
    unsafe { *ret = Seq::of(&SUBS) };
    ret
}

methods! {
    __m_lookup_id => repo_lookup_id, 3;
    __m_lookup_modifier => lookup_modifier, 4;
    __m_release_cache => release_cache, 2;
    __m_save_ir => save_ir, 2;
    __m_get_subrepositories => get_subrepositories, 3;
}

/// M_Repository::somNew: a repository over the files SOMIR names (read
/// the first time).
extern "C" fn repository_new(cls: usize) -> usize {
    unsafe { somsup::somStartCriticalSection() };
    crate::file::load();
    if unsafe { SUBS.is_empty() } {
        let sc = SubRepositoryNewClass(0, 0);
        for fi in 0..crate::file::files().len() {
            let s = somsup::call_parent(p!(M_RepositoryCClassData), somsup::sc(), somsup::cm::SOM_NEW, &[sc]);
            if s != 0 {
                sd(s).file = fi as u32;
                kd(s).file = fi as u32;
                unsafe { SUBS.push(s) };
            }
        }
    }
    let r = call_parent(p!(M_RepositoryCClassData), somsup::sc(), somsup::cm::SOM_NEW, &[cls]);
    unsafe {
        if MAIN == 0 {
            MAIN = r;
        }
        for &s in SUBS.iter() {
            sd(s).main = MAIN;
        }
        somsup::somEndCriticalSection();
    }
    r
}

extern "C" fn create_repository(cls: usize, _ev: Ev) -> usize {
    somsup::call(somsup::sc(), somsup::cm::SOM_NEW, &[cls])
}

extern "C" fn create_from_pathname(cls: usize, _ev: Ev, _path: *const u8) -> usize {
    somsup::call(somsup::sc(), somsup::cm::SOM_NEW, &[cls])
}

methods! {
    __m_repository_new => repository_new, 1;
    __m_create_repository => create_repository, 2;
    __m_create_from_pathname => create_from_pathname, 3;
}

// ---- The definitions ----

macro_rules! field_methods {
    ($($get:ident, $set:ident => $ccd:ident [$i:literal];)*) => {$(
        extern "C" fn $get(o: usize, _ev: Ev) -> usize {
            data::<[usize; 2]>(p!($ccd), o)[$i]
        }
        extern "C" fn $set(o: usize, _ev: Ev, v: usize) {
            data::<[usize; 2]>(p!($ccd), o)[$i] = v;
        }
    )*};
}

field_methods! {
    attr_get_mode, attr_set_mode => AttributeDefCClassData[0];
    attr_get_type, attr_set_type => AttributeDefCClassData[1];
    param_get_mode, param_set_mode => ParameterDefCClassData[0];
    param_get_type, param_set_type => ParameterDefCClassData[1];
    const_get_type, const_set_type => ConstantDefCClassData[0];
}

extern "C" fn typedef_get_type(o: usize, _ev: Ev) -> usize {
    data::<[usize; 1]>(p!(TypeDefCClassData), o)[0]
}
extern "C" fn typedef_set_type(o: usize, _ev: Ev, v: usize) {
    data::<[usize; 1]>(p!(TypeDefCClassData), o)[0] = v;
}
extern "C" fn except_get_type(o: usize, _ev: Ev) -> usize {
    data::<[usize; 1]>(p!(ExceptionDefCClassData), o)[0]
}
extern "C" fn except_set_type(o: usize, _ev: Ev, v: usize) {
    data::<[usize; 1]>(p!(ExceptionDefCClassData), o)[0] = v;
}

#[repr(C)]
pub struct Any {
    tc: usize,
    value: usize,
}

/// ConstantDef::_get_value: the value's text (IBM keeps it so).
extern "C" fn const_get_value(ret: *mut Any, o: usize, _ev: Ev) -> *mut Any {
    let d = data::<[usize; 2]>(p!(ConstantDefCClassData), o);
    unsafe { *ret = Any { tc: tc_ptr(&tc::TC__string), value: &mut d[1] as *mut usize as usize } };
    ret
}
extern "C" fn const_set_value(o: usize, _ev: Ev, v: *const Any) {
    if v.is_null() {
        return;
    }
    let d = data::<[usize; 2]>(p!(ConstantDefCClassData), o);
    unsafe {
        if (*v).tc == tc_ptr(&tc::TC__string) && (*v).value != 0 {
            d[1] = somsup::dup(somsup::cstr(*((*v).value as *const *const u8))) as usize;
        }
    }
}

fn op(o: usize) -> &'static mut Op {
    data::<Op>(p!(OperationDefCClassData), o)
}
extern "C" fn op_get_result(o: usize, _ev: Ev) -> usize {
    op(o).result
}
extern "C" fn op_set_result(o: usize, _ev: Ev, t: usize) {
    op(o).result = t;
}
extern "C" fn op_get_mode(o: usize, _ev: Ev) -> u32 {
    op(o).mode
}
extern "C" fn op_set_mode(o: usize, _ev: Ev, m: u32) {
    op(o).mode = m;
}
extern "C" fn op_get_contexts(ret: *mut Seq, o: usize, _ev: Ev) -> *mut Seq {
    unsafe { *ret = op(o).contexts };
    ret
}
extern "C" fn op_set_contexts(o: usize, _ev: Ev, s: *const Seq) {
    if !s.is_null() {
        op(o).contexts = copy_strings(unsafe { &*s });
    }
}
extern "C" fn op_get_raises(ret: *mut Seq, o: usize, _ev: Ev) -> *mut Seq {
    unsafe { *ret = op(o).raises };
    ret
}
extern "C" fn op_set_raises(o: usize, _ev: Ev, s: *const Seq) {
    if !s.is_null() {
        op(o).raises = copy_strings(unsafe { &*s });
    }
}

fn copy_strings(s: &Seq) -> Seq {
    let v: Vec<usize> = s.items().iter().map(|&p| somsup::dup(unsafe { somsup::cstr(p as *const u8) }) as usize).collect();
    Seq::of(&v)
}

fn iface(o: usize) -> &'static mut Iface {
    data::<Iface>(p!(InterfaceDefCClassData), o)
}
extern "C" fn get_bases(ret: *mut Seq, o: usize, _ev: Ev) -> *mut Seq {
    unsafe { *ret = iface(o).bases };
    ret
}
extern "C" fn set_bases(o: usize, _ev: Ev, s: *const Seq) {
    if !s.is_null() {
        iface(o).bases = copy_strings(unsafe { &*s });
    }
}
extern "C" fn get_instance_data(o: usize, _ev: Ev) -> usize {
    iface(o).instance_data
}
extern "C" fn set_instance_data(o: usize, _ev: Ev, t: usize) {
    iface(o).instance_data = t;
}
extern "C" fn iface_subclasses(o: usize, _ev: Ev, seq: *mut Seq) {
    let mut v: Vec<usize> = if seq.is_null() { Vec::new() } else { unsafe { (*seq).items().to_vec() } };
    let id = unsafe { somsup::cstr(cd(o).id) }.to_vec();
    subclasses(&id, &mut v);
    if !seq.is_null() {
        unsafe { *seq = Seq::of(&v) };
    }
}
/// InterfaceDef::within: the module it is in (none at the top level).
extern "C" fn iface_within(ret: *mut Seq, o: usize, _ev: Ev) -> *mut Seq {
    let c = cd(o).container;
    let v: Vec<usize> = if c != 0 && !is_sub(c) { alloc::vec![used(c)] } else { Vec::new() };
    unsafe { *ret = Seq::of(&v) };
    ret
}

methods! {
    __m_attr_get_type => attr_get_type, 2;
    __m_attr_set_type => attr_set_type, 3;
    __m_attr_get_mode => attr_get_mode, 2;
    __m_attr_set_mode => attr_set_mode, 3;
    __m_param_get_type => param_get_type, 2;
    __m_param_set_type => param_set_type, 3;
    __m_param_get_mode => param_get_mode, 2;
    __m_param_set_mode => param_set_mode, 3;
    __m_const_get_type => const_get_type, 2;
    __m_const_set_type => const_set_type, 3;
    __m_const_get_value => const_get_value, 3;
    __m_const_set_value => const_set_value, 3;
    __m_typedef_get_type => typedef_get_type, 2;
    __m_typedef_set_type => typedef_set_type, 3;
    __m_except_get_type => except_get_type, 2;
    __m_except_set_type => except_set_type, 3;
    __m_op_get_result => op_get_result, 2;
    __m_op_set_result => op_set_result, 3;
    __m_op_get_mode => op_get_mode, 2;
    __m_op_set_mode => op_set_mode, 3;
    __m_op_get_contexts => op_get_contexts, 3;
    __m_op_set_contexts => op_set_contexts, 3;
    __m_op_get_raises => op_get_raises, 3;
    __m_op_set_raises => op_set_raises, 3;
    __m_get_bases => get_bases, 3;
    __m_set_bases => set_bases, 3;
    __m_get_instance_data => get_instance_data, 2;
    __m_set_instance_data => set_instance_data, 3;
    __m_iface_subclasses => iface_subclasses, 3;
    __m_iface_within => iface_within, 3;
}

// ---- Descriptions ----

fn described(ret: *mut Description, desc_tc: &'static u32, value: usize) -> *mut Description {
    // The structure's name (the TypeCode's parameter 0).
    let t = tc_ptr(desc_tc);
    let name = unsafe { *((t + 12) as *const *const u8) };
    unsafe { *ret = Description { name, tc: t, value } };
    ret
}

/// AttributeDescription / ParameterDescription: names, type, mode.
fn typed_mode(b: &mut Block, o: usize, ty: usize, mode: usize) -> usize {
    let s = b.alloc(20, 4);
    put_names(b, s, o);
    let t = b.tc(ty);
    b.put(s + 12, t);
    b.put(s + 16, mode);
    s
}

/// TypeDescription / ExceptionDescription: names, type.
fn typed(b: &mut Block, o: usize, ty: usize) -> usize {
    let s = b.alloc(16, 4);
    put_names(b, s, o);
    let t = b.tc(ty);
    b.put(s + 12, t);
    s
}

fn names_only(b: &mut Block, o: usize) -> usize {
    let s = b.alloc(12, 4);
    put_names(b, s, o);
    s
}

extern "C" fn describe_attribute(ret: *mut Description, o: usize, _ev: Ev) -> *mut Description {
    let d = data::<[usize; 2]>(p!(AttributeDefCClassData), o);
    let (mode, ty) = (d[0], d[1]);
    described(ret, unsafe { &tc::TC__AttributeDescription }, block(|b| typed_mode(b, o, ty, mode)))
}
extern "C" fn describe_parameter(ret: *mut Description, o: usize, _ev: Ev) -> *mut Description {
    let d = data::<[usize; 2]>(p!(ParameterDefCClassData), o);
    let (mode, ty) = (d[0], d[1]);
    described(ret, unsafe { &tc::TC__ParameterDescription }, block(|b| typed_mode(b, o, ty, mode)))
}
extern "C" fn describe_typedef(ret: *mut Description, o: usize, ev: Ev) -> *mut Description {
    let ty = typedef_get_type(o, ev);
    described(ret, unsafe { &tc::TC__TypeDescription }, block(|b| typed(b, o, ty)))
}
extern "C" fn describe_exception(ret: *mut Description, o: usize, ev: Ev) -> *mut Description {
    let ty = except_get_type(o, ev);
    described(ret, unsafe { &tc::TC__ExceptionDescription }, block(|b| typed(b, o, ty)))
}
/// ConstantDescription: names, type, value (an any holding the value's
/// text).
extern "C" fn describe_constant(ret: *mut Description, o: usize, _ev: Ev) -> *mut Description {
    let d = data::<[usize; 2]>(p!(ConstantDefCClassData), o);
    let (ty, val) = (d[0], d[1]);
    let v = block(|b| {
        let s = b.alloc(24, 4);
        put_names(b, s, o);
        let t = b.tc(ty);
        b.put(s + 12, t);
        b.put(s + 16, tc_ptr(unsafe { &tc::TC__string }));
        let sp = b.alloc(4, 4);
        let text = b.str(val as *const u8);
        b.put(sp, text);
        b.put(s + 20, sp);
        s
    });
    described(ret, unsafe { &tc::TC__ConstDescription }, v)
}
extern "C" fn describe_module(ret: *mut Description, o: usize, _ev: Ev) -> *mut Description {
    described(ret, unsafe { &tc::TC__ModuleDescription }, block(|b| names_only(b, o)))
}
extern "C" fn describe_interface_brief(ret: *mut Description, o: usize, _ev: Ev) -> *mut Description {
    described(ret, unsafe { &tc::TC__InterfaceDescription }, block(|b| names_only(b, o)))
}

/// An OperationDescription at `s`: names, result, mode, contexts, the
/// parameters (its contents) and the exceptions it raises.
fn operation_at(b: &mut Block, s: usize, o: usize) {
    put_names(b, s, o);
    let d = op(o);
    let t = b.tc(d.result);
    b.put(s + 12, t);
    b.put(s + 16, d.mode as usize);
    let ctx: Vec<usize> = d.contexts.items().to_vec();
    let cb = b.alloc(ctx.len() * 4, 4);
    for (i, &c) in ctx.iter().enumerate() {
        let p = b.str(c as *const u8);
        b.put(cb + 4 * i, p);
    }
    b.put(s + 20, ctx.len());
    b.put(s + 24, ctx.len());
    b.put(s + 28, if ctx.is_empty() { 0 } else { cb });
    let mut params = Vec::new();
    lookup(o, None, &mut params, 1, Some(CODE_PARAMETER), true, false);
    drop_uses(&params);
    let pb = b.alloc(params.len() * 20, 4);
    for (i, &p) in params.iter().enumerate() {
        let pd = data::<[usize; 2]>(p!(ParameterDefCClassData), p);
        let at = pb + 20 * i;
        put_names(b, at, p);
        let t = b.tc(pd[1]);
        b.put(at + 12, t);
        b.put(at + 16, pd[0]);
    }
    b.put(s + 32, params.len());
    b.put(s + 36, params.len());
    b.put(s + 40, if params.is_empty() { 0 } else { pb });
    let raises: Vec<usize> = d.raises.items().to_vec();
    let mut ex = Vec::new();
    for r in raises {
        let e = lookup_id(unsafe { somsup::cstr(r as *const u8) });
        if e != 0 {
            decrement_uses(e, core::ptr::null_mut());
            ex.push(e);
        }
    }
    let eb = b.alloc(ex.len() * 16, 4);
    for (i, &e) in ex.iter().enumerate() {
        let at = eb + 16 * i;
        put_names(b, at, e);
        let t = b.tc(except_get_type(e, core::ptr::null_mut()));
        b.put(at + 12, t);
    }
    b.put(s + 44, ex.len());
    b.put(s + 48, ex.len());
    b.put(s + 52, if ex.is_empty() { 0 } else { eb });
}

extern "C" fn describe_operation(ret: *mut Description, o: usize, _ev: Ev) -> *mut Description {
    let v = block(|b| {
        let s = b.alloc(56, 4);
        operation_at(b, s, o);
        s
    });
    described(ret, unsafe { &tc::TC__OperationDescription }, v)
}

/// describe_interface: FullInterfaceDescription (names, the operations
/// and the attributes, inherited ones included).
extern "C" fn describe_interface(ret: *mut [usize; 9], o: usize, _ev: Ev) -> *mut [usize; 9] {
    let mut ops = Vec::new();
    lookup(o, None, &mut ops, 1, Some(CODE_OPERATION), false, false);
    drop_uses(&ops);
    let mut attrs = Vec::new();
    lookup(o, None, &mut attrs, 1, Some(CODE_ATTRIBUTE), false, false);
    drop_uses(&attrs);
    // One block: the structure, the two arrays, then what they point to
    // (freeing the block frees it all).
    let base = block(|b| {
        let s = b.alloc(12, 4);
        let ob = b.alloc(ops.len() * 56, 4);
        let ab = b.alloc(attrs.len() * 20, 4);
        put_names(b, s, o);
        for (i, &x) in ops.iter().enumerate() {
            operation_at(b, ob + 56 * i, x);
        }
        for (i, &a) in attrs.iter().enumerate() {
            let d = data::<[usize; 2]>(p!(AttributeDefCClassData), a);
            let at = ab + 20 * i;
            put_names(b, at, a);
            let t = b.tc(d[1]);
            b.put(at + 12, t);
            b.put(at + 16, d[0]);
        }
        s
    });
    let w = |i: usize| unsafe { *((base + 4 * i) as *const usize) };
    let ob = base + 12;
    let ab = ob + ops.len() * 56;
    unsafe {
        *ret = [w(0), w(1), w(2), ops.len(), ops.len(), if ops.is_empty() { 0 } else { ob }, attrs.len(), attrs.len(), if attrs.is_empty() { 0 } else { ab }];
    }
    ret
}

methods! {
    __m_describe_attribute => describe_attribute, 3;
    __m_describe_parameter => describe_parameter, 3;
    __m_describe_typedef => describe_typedef, 3;
    __m_describe_exception => describe_exception, 3;
    __m_describe_constant => describe_constant, 3;
    __m_describe_module => describe_module, 3;
    __m_describe_interface_brief => describe_interface_brief, 3;
    __m_describe_operation => describe_operation, 3;
    __m_describe_interface => describe_interface, 3;
}
