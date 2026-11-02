//! The repository's objects: made from the file's records when first
//! looked up and kept for the life of the process (a Contained's somFree
//! only counts down its uses, as IBM's does until the last use).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use somsup::{Environment, data, free, malloc, strlen};

use crate::file::{IrFile, files};
use crate::tc;
use crate::*;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Seq {
    pub max: u32,
    pub len: u32,
    pub buf: *mut usize,
}

impl Seq {
    pub const EMPTY: Seq = Seq { max: 0, len: 0, buf: core::ptr::null_mut() };
    /// A sequence in SOMMalloc'd memory (the caller frees the buffer).
    pub fn of(v: &[usize]) -> Seq {
        if v.is_empty() {
            return Seq::EMPTY;
        }
        let b = malloc(v.len() * 4) as *mut usize;
        unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), b, v.len()) };
        Seq { max: v.len() as u32, len: v.len() as u32, buf: b }
    }
    pub fn items(&self) -> &[usize] {
        if self.buf.is_null() { &[] } else { unsafe { core::slice::from_raw_parts(self.buf, self.len as usize) } }
    }
}

/// Contained's instance data (IBM's 64 bytes).
#[repr(C)]
pub struct Cd {
    pub name: *mut u8,
    pub id: *mut u8,
    /// somModifiers: { name, value } string pairs.
    pub mods: Seq,
    pub file: u32,
    pub iid: u32,
    /// The container: a SubRepository for the file's top level.
    pub container: usize,
    pub sub: usize,
    pub def_iid: u32,
    pub ref_iid: u32,
    pub loaded: u32,
    pub uses: u32,
    pub fixed_size: u32,
    pub var_iid: u32,
    pub spare: u32,
}

/// Container's instance data (12 bytes).
#[repr(C)]
pub struct Kd {
    pub contents: u32,
    pub entries: *mut Vec<Entry>,
    pub file: u32,
}

pub struct Entry {
    pub iid: u32,
    pub code: u8,
    pub name: *const u8,
    pub obj: usize,
}

/// SubRepository's instance data (one per file).
#[repr(C)]
pub struct Sd {
    pub file: u32,
    pub main: usize,
}

pub fn cd(o: usize) -> &'static mut Cd {
    data::<Cd>(p!(ContainedCClassData), o)
}
pub fn kd(o: usize) -> &'static mut Kd {
    data::<Kd>(p!(ContainerCClassData), o)
}
pub fn sd(o: usize) -> &'static mut Sd {
    data::<Sd>(p!(SubRepositoryCClassData), o)
}

// ---- Record classes (IBM's codes) ----

pub const CODE_CONSTANT: u8 = 0;
pub const CODE_TYPE: u8 = 1;
pub const CODE_EXCEPTION: u8 = 2;
pub const CODE_INTERFACE: u8 = 3;
pub const CODE_MODULE: u8 = 4;
pub const CODE_ATTRIBUTE: u8 = 5;
pub const CODE_OPERATION: u8 = 6;
pub const CODE_PARAMETER: u8 = 7;

const NAMES: [&[u8]; 8] = [b"ConstantDef", b"TypeDef", b"ExceptionDef", b"InterfaceDef", b"ModuleDef", b"AttributeDef", b"OperationDef", b"ParameterDef"];

/// The code of a class name (limit_type), 0xff for others.
pub fn code_of(name: &[u8]) -> u8 {
    NAMES.iter().position(|&n| n == name).map_or(0xff, |i| i as u8)
}

fn class_for(code: u8) -> usize {
    match code {
        CODE_CONSTANT => ConstantDefNewClass(0, 0),
        CODE_TYPE => TypeDefNewClass(0, 0),
        CODE_EXCEPTION => ExceptionDefNewClass(0, 0),
        CODE_INTERFACE => InterfaceDefNewClass(0, 0),
        CODE_MODULE => ModuleDefNewClass(0, 0),
        CODE_ATTRIBUTE => AttributeDefNewClass(0, 0),
        CODE_OPERATION => OperationDefNewClass(0, 0),
        CODE_PARAMETER => ParameterDefNewClass(0, 0),
        _ => 0,
    }
}

pub fn is_sub(o: usize) -> bool {
    somsup::is_a(o, unsafe { SubRepositoryClassData[0] })
}
pub fn is_repo(o: usize) -> bool {
    somsup::is_a(o, unsafe { RepositoryClassData[0] })
}
pub fn is_contained(o: usize) -> bool {
    somsup::is_a(o, unsafe { ContainedClassData[0] })
}
pub fn is_container(o: usize) -> bool {
    somsup::is_a(o, unsafe { ContainerClassData[0] })
}
pub fn is_interface(o: usize) -> bool {
    somsup::is_a(o, unsafe { InterfaceDefClassData[0] })
}

// ---- The objects ----

static mut CACHE: BTreeMap<(u32, u32), usize> = BTreeMap::new();
pub static mut SUBS: Vec<usize> = Vec::new();
pub static mut MAIN: usize = 0;

fn dup_c(s: *const u8) -> *mut u8 {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    somsup::dup(unsafe { somsup::cstr(s) })
}

/// The id of something named `name` in `container` ("::A::B").
fn make_id(container: usize, name: *const u8) -> *mut u8 {
    let mut v = Vec::new();
    if container != 0 && !is_sub(container) && is_contained(container) {
        v.extend_from_slice(unsafe { somsup::cstr(cd(container).id) });
    }
    v.extend_from_slice(b"::");
    v.extend_from_slice(unsafe { somsup::cstr(name) });
    somsup::dup(&v)
}

/// A sequence of strings (`count`, then the strings) from the file.
fn read_strings(f: &IrFile, at: u32) -> (Seq, u32) {
    let n = f.int(at).max(0) as usize;
    let mut at = at + 4;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let (s, q) = f.str_at(at);
        v.push(dup_c(s) as usize);
        at = q;
    }
    (Seq::of(&v), at)
}

/// The object of record `iid` in file `fi`, defined in `container`.
pub fn object(fi: u32, iid: u32, container: usize) -> usize {
    if let Some(&o) = unsafe { CACHE.get(&(fi, iid)) } {
        return o;
    }
    let f = &files()[fi as usize];
    let code = f.int(iid + 4) as u8;
    let cls = class_for(code);
    if cls == 0 {
        return 0;
    }
    let o = somsup::new_object(cls);
    if o == 0 {
        return 0;
    }
    unsafe { CACHE.insert((fi, iid), o) };
    let (name, at) = f.str_at(iid + 8);
    let c = cd(o);
    c.file = fi;
    c.iid = iid;
    c.fixed_size = f.int(iid) as u32;
    c.def_iid = f.int(at + 4) as u32;
    c.ref_iid = f.int(at + 8) as u32;
    c.var_iid = f.int(at + 12) as u32;
    c.container = if container != 0 {
        container
    } else if c.def_iid == 0 {
        unsafe { SUBS.get(fi as usize).copied().unwrap_or(0) }
    } else {
        object(fi, c.def_iid, 0)
    };
    c.sub = unsafe { SUBS.get(fi as usize).copied().unwrap_or(0) };
    c.name = dup_c(name);
    c.id = make_id(c.container, name);
    c.loaded = 1;
    let fixed = at + 16;
    // The variable record: the modifiers, then the class's own data.
    let mut var = 0;
    if f.is(c.var_iid, 11) {
        let n = f.int(c.var_iid + 16).max(0) as usize;
        let mut q = c.var_iid + 20;
        let mut pairs = Vec::with_capacity(2 * n);
        for _ in 0..n {
            let (a, x) = f.str_at(q);
            let (b, y) = f.str_at(x);
            pairs.push(dup_c(a) as usize);
            pairs.push(dup_c(b) as usize);
            q = y;
        }
        let mut s = Seq::of(&pairs);
        s.max = n as u32;
        s.len = n as u32;
        c.mods = s;
        var = q;
    }
    let tc_at = |at: u32| -> (usize, u32) {
        let (t, q) = buf::get_tc(f.ptr(at));
        (t, f.off(q))
    };
    match code {
        CODE_ATTRIBUTE => {
            let d = data::<[usize; 2]>(p!(AttributeDefCClassData), o);
            d[0] = f.int(fixed) as usize;
            if var != 0 {
                d[1] = tc_at(var).0;
            }
        }
        CODE_PARAMETER => {
            let d = data::<[usize; 2]>(p!(ParameterDefCClassData), o);
            d[0] = f.int(fixed) as usize;
            if var != 0 {
                d[1] = tc_at(var).0;
            }
        }
        CODE_TYPE => {
            if var != 0 {
                data::<[usize; 1]>(p!(TypeDefCClassData), o)[0] = tc_at(var).0;
            }
        }
        CODE_EXCEPTION => {
            if var != 0 {
                data::<[usize; 1]>(p!(ExceptionDefCClassData), o)[0] = tc_at(var).0;
            }
        }
        CODE_CONSTANT => {
            if var != 0 {
                let d = data::<[usize; 2]>(p!(ConstantDefCClassData), o);
                let (t, q) = tc_at(var);
                d[0] = t;
                d[1] = dup_c(f.str_at(q).0) as usize;
            }
        }
        CODE_MODULE => {
            kd(o).contents = f.int(fixed) as u32;
            kd(o).file = fi;
        }
        CODE_INTERFACE => {
            kd(o).contents = f.int(fixed) as u32;
            kd(o).file = fi;
            if var != 0 {
                let d = data::<Iface>(p!(InterfaceDefCClassData), o);
                let (b, q) = read_strings(f, var);
                d.bases = b;
                d.instance_data = tc_at(q).0;
            }
        }
        CODE_OPERATION => {
            kd(o).contents = f.int(fixed) as u32;
            kd(o).file = fi;
            let d = data::<Op>(p!(OperationDefCClassData), o);
            d.mode = f.int(fixed + 8) as u32;
            if var != 0 {
                let (t, q) = tc_at(var);
                d.result = t;
                let (c1, q) = read_strings(f, q);
                let (c2, _) = read_strings(f, q);
                d.contexts = c1;
                d.raises = c2;
            }
        }
        _ => {}
    }
    o
}

#[repr(C)]
pub struct Iface {
    pub bases: Seq,
    pub instance_data: usize,
}

#[repr(C)]
pub struct Op {
    pub result: usize,
    pub mode: u32,
    pub contexts: Seq,
    pub raises: Seq,
}

/// A container's entries (read from its contents record when first
/// needed).
pub fn entries(c: usize) -> &'static mut Vec<Entry> {
    let k = kd(c);
    if k.entries.is_null() {
        let mut v = Vec::new();
        let (fi, at) = if is_sub(c) {
            let fi = sd(c).file;
            (fi, files()[fi as usize].root())
        } else {
            (k.file, k.contents)
        };
        if let Some(f) = files().get(fi as usize) {
            if f.is(at, 8) {
                let n = f.int(at + 8).max(0) as usize;
                let mut q = at + 12;
                for _ in 0..n {
                    let iid = f.int(q) as u32;
                    let code = f.int(q + 4) as u8;
                    let (name, r) = f.str_at(q + 8);
                    v.push(Entry { iid, code, name, obj: 0 });
                    q = r;
                }
            }
        }
        k.file = fi;
        k.entries = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(v));
    }
    unsafe { &mut *k.entries }
}

pub fn entry_object(c: usize, i: usize) -> usize {
    let es = entries(c);
    if es[i].obj == 0 {
        let fi = kd(c).file;
        es[i].obj = object(fi, es[i].iid, c);
    }
    es[i].obj
}

/// A use of an object handed to a caller.
pub fn used(o: usize) -> usize {
    if o != 0 && is_contained(o) {
        cd(o).uses += 1;
    }
    o
}

// ---- Looking things up ----

/// IBM's internalNameLookup: `container`'s objects called `name` (all
/// for none) of class `limit` (None for all), searching nested modules,
/// interfaces and operations when `levels` is -1, the base interfaces
/// unless `exclude_inherited`, and the enclosing scopes when `up`.
pub fn lookup(c: usize, name: Option<&[u8]>, out: &mut Vec<usize>, levels: i32, limit: Option<u8>, exclude_inherited: bool, up: bool) {
    if is_repo(c) {
        for &s in unsafe { SUBS.iter() } {
            lookup(s, name, out, levels, limit, exclude_inherited, up);
        }
        return;
    }
    if !is_container(c) {
        return;
    }
    let n = entries(c).len();
    for i in 0..n {
        let (code, ename) = {
            let e = &entries(c)[i];
            (e.code, e.name)
        };
        let matches = name.is_none_or(|nm| unsafe { somsup::cstr(ename) } == nm);
        if matches && limit.is_none_or(|l| l == code) {
            let o = entry_object(c, i);
            if o != 0 && !out.contains(&o) {
                out.push(used(o));
            }
            if name.is_some() && levels == 1 {
                break;
            }
        }
        if name.is_some() && levels == -1 && matches!(code, CODE_MODULE | CODE_INTERFACE | CODE_OPERATION) {
            let o = entry_object(c, i);
            if o != 0 {
                lookup(o, name, out, -1, limit, true, false);
            }
        }
    }
    if is_interface(c) && !exclude_inherited {
        let bases: Vec<usize> = data::<Iface>(p!(InterfaceDefCClassData), c).bases.items().to_vec();
        for b in bases {
            let base = lookup_id(unsafe { somsup::cstr(b as *const u8) });
            if base != 0 {
                lookup(base, name, out, levels, limit, false, false);
            }
        }
    }
    if up && name.is_some() && is_contained(c) {
        let p = cd(c).container;
        if p != 0 && !is_sub(p) {
            lookup(p, name, out, 1, limit, exclude_inherited, true);
        }
    }
}

/// The object with a repository id ("::A::B" or "A::B").
pub fn lookup_id(id: &[u8]) -> usize {
    let id = id.strip_prefix(b"::").unwrap_or(id);
    if id.is_empty() {
        return 0;
    }
    for &s in unsafe { SUBS.iter() } {
        let mut c = s;
        let parts: Vec<&[u8]> = id.split(|&b| b == b':').filter(|p| !p.is_empty()).collect();
        let mut found = 0;
        for (k, part) in parts.iter().enumerate() {
            let mut v = Vec::new();
            lookup(c, Some(part), &mut v, 1, None, true, false);
            for &o in &v {
                cd(o).uses -= 1;
            }
            let Some(&o) = v.first() else { break };
            if k + 1 == parts.len() {
                found = o;
            } else if is_container(o) {
                c = o;
            } else {
                break;
            }
        }
        if found != 0 {
            return used(found);
        }
    }
    0
}

/// The interfaces whose base interfaces include `id`, and theirs.
pub fn subclasses(id: &[u8], out: &mut Vec<usize>) {
    for fi in 0..files().len() {
        let mut all = Vec::new();
        let s = unsafe { SUBS[fi] };
        lookup(s, None, &mut all, 1, Some(CODE_INTERFACE), true, false);
        for o in all {
            cd(o).uses -= 1;
            let bases = &data::<Iface>(p!(InterfaceDefCClassData), o).bases;
            let derived = bases.items().iter().any(|&b| {
                let b = unsafe { somsup::cstr(b as *const u8) };
                b == id || b.strip_prefix(b"::") == id.strip_prefix(b"::")
            });
            if derived && !out.contains(&o) {
                out.push(used(o));
                let oid = unsafe { somsup::cstr(cd(o).id) }.to_vec();
                subclasses(&oid, out);
            }
        }
    }
}

pub fn limit_code(limit: *const u8) -> Option<u8> {
    let l = unsafe { somsup::cstr(limit) };
    if limit.is_null() || l == b"all" { None } else { Some(code_of(l)) }
}

// ---- Descriptions in one block (one SOMFree frees one) ----

pub struct Block {
    base: usize,
    at: usize,
    dry: bool,
}

impl Block {
    pub fn alloc(&mut self, n: usize, align: usize) -> usize {
        self.at = (self.at + align - 1) & !(align - 1);
        let p = self.base + self.at;
        self.at += n;
        p
    }
    pub fn put(&mut self, p: usize, v: usize) {
        if !self.dry {
            unsafe { *(p as *mut usize) = v };
        }
    }
    pub fn str(&mut self, s: *const u8) -> usize {
        if s.is_null() {
            return 0;
        }
        let n = unsafe { strlen(s) } + 1;
        let p = self.alloc(n, 1);
        if !self.dry {
            unsafe { core::ptr::copy_nonoverlapping(s, p as *mut u8, n) };
        }
        p
    }
    pub fn tc(&mut self, t: usize) -> usize {
        if t == 0 {
            return 0;
        }
        let n = tc::buffer_length(t);
        let p = self.alloc(n, 4);
        if !self.dry {
            tc::copy_to_buffer(t, p);
        }
        p
    }
}

/// Build a block: `f` runs once to size it and once to fill it; the
/// block's address.
pub fn block(f: impl Fn(&mut Block) -> usize) -> usize {
    let mut b = Block { base: 0, at: 0, dry: true };
    f(&mut b);
    let size = b.at.max(4);
    let mem = malloc(size) as usize;
    if mem == 0 {
        return 0;
    }
    let mut b = Block { base: mem, at: 0, dry: false };
    f(&mut b)
}

/// The first three fields of every description: name, id, defined_in.
pub fn put_names(b: &mut Block, at: usize, o: usize) {
    let c = cd(o);
    let n = b.str(c.name);
    b.put(at, n);
    let i = b.str(c.id);
    b.put(at + 4, i);
    let d = b.str(defined_in(o));
    b.put(at + 8, d);
}

pub fn defined_in(o: usize) -> *const u8 {
    let c = cd(o).container;
    if c == 0 || is_sub(c) || !is_contained(c) { b"\0".as_ptr() } else { cd(c).id }
}

/// Free a sequence's buffer the caller does not get.
pub fn drop_uses(v: &[usize]) {
    for &o in v {
        if is_contained(o) {
            let c = cd(o);
            c.uses = c.uses.saturating_sub(1);
        }
    }
}

pub fn release(p: *mut u8) {
    free(p)
}

pub fn ev_none(ev: *mut Environment) -> *mut Environment {
    ev
}
