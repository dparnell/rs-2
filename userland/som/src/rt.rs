//! The object model's machinery: class records, method tables, method
//! and data tokens, resolution, and the initializer control structures.
//!
//! Binary interface (what code compiled for IBM's SOM relies on):
//!
//! - An object starts with a pointer to its class's method table, whose
//!   first nine words are `somMethodTab`'s public fields (class object,
//!   class info, class name, instance size, ...); the entries follow.
//! - `<Class>ClassData` holds the class object and then one token per
//!   method in the IDL's release order. A method token is code: calling
//!   it with the method's arguments runs the object's implementation
//!   (`(*WPObjectClassData.wpSaveState)(obj)`). The word before a token
//!   points to its `TokenInfo`.
//! - `<Class>CClassData` holds the class's `somParentMtabStruct` and its
//!   instance data token, which is code too: called with an object, it
//!   returns the address of the class's data in it.
//!
//! The method table holds a block per class in the hierarchy: a marker
//! word (the class record's address) and then the class's methods. The
//! blocks of a class's leftmost ancestors keep their positions in every
//! subclass's table, so a token's thunk checks the marker at the block's
//! usual position and jumps through the slot; otherwise (an ancestor met
//! through a second parent) it looks the block up.

use alloc::vec;
use alloc::vec::Vec;

use crate::id::{self, SomId};
use crate::sys::*;

pub const CLASS_MAGIC: u32 = 0x4d4f_5343; // "CSOM"
pub const TOKEN_MAGIC: u32 = 0x4b4f_5453; // "STOK"

/// `somMethodTab`'s public part (0x24 bytes); the entries follow.
#[repr(C)]
pub struct Mtab {
    pub class_object: usize,
    pub class_info: *mut Class,
    pub class_name: *const u8,
    pub instance_size: i32,
    pub data_alignment: i32,
    pub mtab_size: i32,
    pub protected_data_offset: i32,
    pub protected_data_token: usize,
    pub embedded_objs: usize,
}

pub const MTAB_HDR: usize = 0x24;

impl Mtab {
    pub fn entries(&self) -> *mut usize {
        (self as *const Mtab as usize + MTAB_HDR) as *mut usize
    }
    pub unsafe fn entry(&self, i: usize) -> usize {
        unsafe { *self.entries().add(i) }
    }
    pub unsafe fn set(&self, i: usize, v: usize) {
        unsafe { *self.entries().add(i) = v }
    }
}

/// `somMethodTabList`.
#[repr(C)]
pub struct MtabList {
    pub mtab: *mut Mtab,
    pub next: *mut MtabList,
}

/// somInitCtrl / somDestructCtrl / somAssignCtrl.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ctrl {
    pub mask: *const u8,
    pub info: usize,
    pub info_size: i32,
    pub ctrl_info: usize,
}

impl Ctrl {
    pub const fn empty() -> Ctrl {
        Ctrl { mask: core::ptr::null(), info: 0, info_size: 0, ctrl_info: 0 }
    }
}

/// `somParentMtabStruct` (what `<Class>CClassData.parentMtab` points to).
#[repr(C)]
pub struct ParentMtab {
    pub mtab: *mut Mtab,
    pub next: *mut MtabList,
    pub class_object: usize,
    pub renew_thunk: usize,
    pub instance_size: i32,
    pub initializers: *mut usize,
    pub resolved: *mut usize,
    pub init_ctrl: Ctrl,
    pub destruct_ctrl: Ctrl,
    pub assign_ctrl: Ctrl,
    pub embedded_total: i32,
    pub hierarchy_total: i32,
    pub unused: i32,
}

/// somInitInfo (0x1c bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InitInfo {
    pub cls: usize,
    pub default_init: usize,
    pub default_copy_init: usize,
    pub default_const_copy_init: usize,
    pub default_ncarg_copy_init: usize,
    pub data_offset: i32,
    pub legacy_init: usize,
}

/// somDestructInfo (0x10 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DestructInfo {
    pub cls: usize,
    pub default_destruct: usize,
    pub data_offset: i32,
    pub legacy_uninit: usize,
}

/// somAssignInfo (0x1c bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AssignInfo {
    pub cls: usize,
    pub default_assign: usize,
    pub default_const_assign: usize,
    pub default_ncarg_assign: usize,
    pub uda_assign: usize,
    pub uda_const_assign: usize,
    pub data_offset: i32,
}

/// What the word before a token points to.
#[repr(C)]
pub struct TokenInfo {
    pub magic: u32,
    /// 0: method, 1: data.
    pub kind: u32,
    /// The class that introduced the method or data.
    pub class: *mut Class,
    /// Method: the slot in the class's block (1 = first). Data: the
    /// offset within the class's data.
    pub slot: usize,
    /// Method: the index in the class's `methods`.
    pub index: usize,
}

pub struct MethodDef {
    /// The registered id of the method's simple name.
    pub key: usize,
    pub name: *const u8,
    pub descriptor: SomId,
    pub token: usize,
    pub slot: usize,
    /// The introducing class's implementation.
    pub proc_: usize,
    pub rd: usize,
    pub ap: usize,
    pub aggregate: bool,
    /// The argument list size from somApRdInfo (0: unknown).
    pub va_size: u32,
    pub call_type: u32,
}

pub struct Class {
    pub magic: u32,
    pub obj: usize,
    pub name: *const u8,
    pub key: usize,
    pub parents: Vec<*mut Class>,
    pub mtab: *mut Mtab,
    pub nentries: usize,
    pub blocks: Vec<(*mut Class, usize)>,
    /// This class's block: the marker's index and the slots after it.
    pub base: usize,
    pub block_len: usize,
    pub methods: Vec<MethodDef>,
    pub dynamic: Vec<MethodDef>,
    pub data_size: usize,
    pub align: usize,
    pub own_offset: usize,
    pub instance_size: usize,
    pub data_offsets: Vec<(*mut Class, usize)>,
    pub major: i32,
    pub minor: i32,
    pub cds: *mut usize,
    pub ccds: *mut usize,
    pub pmtab: *mut ParentMtab,
    pub inst_token: usize,
    pub legacy_init: usize,
    pub legacy_uninit: usize,
    pub allocate: usize,
    pub deallocate: usize,
    pub direct_init: Vec<*mut Class>,
    /// Where the SOMClass data (this module's class record pointer) is in
    /// instances of this class, when it is a metaclass.
    pub somclass_off: usize,
    pub ready: bool,
    pub ctrl_built: bool,
    pub num_cds_entries: usize,
}

impl Class {
    pub fn new() -> *mut Class {
        leak(Class {
            magic: CLASS_MAGIC,
            obj: 0,
            name: b"Undefined\0".as_ptr(),
            key: 0,
            parents: Vec::new(),
            mtab: core::ptr::null_mut(),
            nentries: 0,
            blocks: Vec::new(),
            base: 0,
            block_len: 0,
            methods: Vec::new(),
            dynamic: Vec::new(),
            data_size: 0,
            align: 4,
            own_offset: 4,
            instance_size: 4,
            data_offsets: Vec::new(),
            major: 0,
            minor: 0,
            cds: core::ptr::null_mut(),
            ccds: core::ptr::null_mut(),
            pmtab: core::ptr::null_mut(),
            inst_token: 0,
            legacy_init: 0,
            legacy_uninit: 0,
            allocate: 0,
            deallocate: 0,
            direct_init: Vec::new(),
            somclass_off: 0,
            ready: false,
            ctrl_built: false,
            num_cds_entries: 0,
        })
    }

    pub fn name_bytes(&self) -> &[u8] {
        unsafe { cstr(self.name) }
    }

    pub fn mt(&self) -> &Mtab {
        unsafe { &*self.mtab }
    }

    /// Where `x`'s block is in this class's table.
    pub fn block_of(&self, x: *mut Class) -> Option<usize> {
        let xb = unsafe { (*x).base };
        if !self.mtab.is_null() && xb < self.nentries && unsafe { self.mt().entry(xb) } == x as usize {
            return Some(xb);
        }
        self.blocks.iter().find(|b| b.0 == x).map(|b| b.1)
    }

    pub fn data_offset_of(&self, x: *mut Class) -> Option<usize> {
        self.data_offsets.iter().find(|d| d.0 == x).map(|d| d.1)
    }

    pub fn descends_from(&self, x: *const Class) -> bool {
        core::ptr::eq(self, x) || self.blocks.iter().any(|b| b.0 as *const Class == x)
    }
}

// ---- The classes everything starts from ----

pub static mut SOMOBJECT: *mut Class = core::ptr::null_mut();
pub static mut SOMCLASS: *mut Class = core::ptr::null_mut();
pub static mut SOMCLASSMGR: *mut Class = core::ptr::null_mut();

/// SOMObject's method slots (release order), for the generic
/// initializers every class gets unless it has its own.
pub mod om {
    pub const SOM_INIT: usize = 0;
    pub const SOM_UNINIT: usize = 1;
    pub const SOM_FREE: usize = 2;
    pub const DEFAULT_V_COPY_INIT: usize = 3;
    pub const GET_CLASS_NAME: usize = 4;
    pub const GET_CLASS: usize = 5;
    pub const IS_A: usize = 6;
    pub const RESPONDS_TO: usize = 7;
    pub const IS_INSTANCE_OF: usize = 8;
    pub const GET_SIZE: usize = 9;
    pub const DUMP_SELF: usize = 10;
    pub const DUMP_SELF_INT: usize = 11;
    pub const PRINT_SELF: usize = 12;
    pub const DEFAULT_CONST_V_COPY_INIT: usize = 13;
    pub const DISPATCH: usize = 18;
    pub const CLASS_DISPATCH: usize = 19;
    pub const DEFAULT_INIT: usize = 22;
    pub const DESTRUCT: usize = 23;
    pub const DEFAULT_COPY_INIT: usize = 26;
    pub const DEFAULT_CONST_COPY_INIT: usize = 27;
    pub const DEFAULT_ASSIGN: usize = 28;
    pub const DEFAULT_CONST_ASSIGN: usize = 29;
    pub const DEFAULT_V_ASSIGN: usize = 30;
    pub const DEFAULT_CONST_V_ASSIGN: usize = 31;
    /// Each class has its own of these (SOMObject's generic one unless
    /// it overrides them).
    pub const PER_CLASS: [usize; 10] = [
        DEFAULT_INIT,
        DESTRUCT,
        DEFAULT_COPY_INIT,
        DEFAULT_CONST_COPY_INIT,
        DEFAULT_V_COPY_INIT,
        DEFAULT_CONST_V_COPY_INIT,
        DEFAULT_ASSIGN,
        DEFAULT_CONST_ASSIGN,
        DEFAULT_V_ASSIGN,
        DEFAULT_CONST_V_ASSIGN,
    ];
}

/// The class record of an object's class.
pub unsafe fn class_of_obj(obj: usize) -> *mut Class {
    unsafe { (**(obj as *const *const Mtab)).class_info }
}

/// The class record of a class object.
pub unsafe fn rec(cls: usize) -> *mut Class {
    if cls == 0 {
        return core::ptr::null_mut();
    }
    unsafe {
        let meta = class_of_obj(cls);
        if meta.is_null() || (*meta).magic != CLASS_MAGIC || (*meta).somclass_off == 0 {
            return core::ptr::null_mut();
        }
        *((cls + (*meta).somclass_off) as *const *mut Class)
    }
}

/// The class record of a class object, checked.
pub unsafe fn rec_ok<'a>(cls: usize) -> Option<&'a mut Class> {
    let r = unsafe { rec(cls) };
    if r.is_null() || unsafe { (*r).magic } != CLASS_MAGIC { None } else { Some(unsafe { &mut *r }) }
}

pub fn align_up(v: usize, a: usize) -> usize {
    let a = a.max(1);
    v.div_ceil(a) * a
}

// ---- Tokens ----

pub unsafe fn token_info<'a>(token: usize) -> Option<&'a TokenInfo> {
    if token < 0x10000 {
        return None;
    }
    let p = unsafe { *((token - 4) as *const *const TokenInfo) };
    if p.is_null() || (p as usize) < 0x10000 || unsafe { (*p).magic } != TOKEN_MAGIC {
        return None;
    }
    Some(unsafe { &*p })
}

fn emit(code: &mut Vec<u8>, b: &[u8]) {
    code.extend_from_slice(b);
}
fn emit32(code: &mut Vec<u8>, v: usize) {
    code.extend_from_slice(&(v as u32).to_le_bytes());
}

/// Put `code` in executable memory after a word pointing to `info`;
/// returns the code's address (the token).
fn place(info: *const TokenInfo, code: &[u8]) -> usize {
    let p = exec_alloc(4 + code.len());
    if p.is_null() {
        return 0;
    }
    unsafe {
        *(p as *mut usize) = info as usize;
        core::ptr::copy_nonoverlapping(code.as_ptr(), p.add(4), code.len());
    }
    p as usize + 4
}

/// The size of a method thunk (filled in after the token's address is
/// known).
fn method_code(token: usize, intro: &Class, slot: usize, aggregate: bool) -> Vec<u8> {
    let mut c = Vec::with_capacity(48);
    let k = if aggregate { 8 } else { 4 };
    emit(&mut c, &[0x8b, 0x4c, 0x24, k]); // mov ecx,[esp+k]
    emit(&mut c, &[0x8b, 0x11]); // mov edx,[ecx]
    emit(&mut c, &[0x81, 0xba]); // cmp dword [edx+mark], class
    emit32(&mut c, MTAB_HDR + 4 * intro.base);
    emit32(&mut c, intro as *const Class as usize);
    emit(&mut c, &[0x75, 0x06]); // jne slow
    emit(&mut c, &[0xff, 0xa2]); // jmp [edx+slot]
    emit32(&mut c, MTAB_HDR + 4 * (intro.base + slot));
    emit(&mut c, &[0x68]); // slow: push token
    emit32(&mut c, token);
    emit(&mut c, &[0x51]); // push ecx
    emit(&mut c, &[0xb8]); // mov eax, resolver
    emit32(&mut c, crate::__s_resolve as usize);
    emit(&mut c, &[0xff, 0xd0]); // call eax
    emit(&mut c, &[0x83, 0xc4, 0x08]); // add esp,8
    emit(&mut c, &[0xff, 0xe0]); // jmp eax
    c
}

fn data_code(token: usize, intro: &Class, extra: usize) -> Vec<u8> {
    let mut c = Vec::with_capacity(48);
    emit(&mut c, &[0x8b, 0x44, 0x24, 0x04]); // mov eax,[esp+4]
    emit(&mut c, &[0x85, 0xc0]); // test eax,eax
    emit(&mut c, &[0x74, 36]); // jz end
    emit(&mut c, &[0x8b, 0x10]); // mov edx,[eax]
    emit(&mut c, &[0x81, 0xba]); // cmp dword [edx+mark], class
    emit32(&mut c, MTAB_HDR + 4 * intro.base);
    emit32(&mut c, intro as *const Class as usize);
    emit(&mut c, &[0x75, 0x06]); // jne slow
    emit(&mut c, &[0x05]); // add eax, offset
    emit32(&mut c, intro.own_offset + extra);
    emit(&mut c, &[0xc3]); // ret
    emit(&mut c, &[0x68]); // slow: push token
    emit32(&mut c, token);
    emit(&mut c, &[0x50]); // push eax
    emit(&mut c, &[0xb8]); // mov eax, resolver
    emit32(&mut c, crate::__s_data_resolve as usize);
    emit(&mut c, &[0xff, 0xd0]); // call eax
    emit(&mut c, &[0x83, 0xc4, 0x08]); // add esp,8
    emit(&mut c, &[0xc3]); // end: ret
    c
}

fn make_token(info: TokenInfo, code_of: impl Fn(usize) -> Vec<u8>) -> usize {
    let info = leak(info);
    let len = code_of(0).len();
    let p = exec_alloc(4 + len);
    if p.is_null() {
        return 0;
    }
    let token = p as usize + 4;
    let code = code_of(token);
    unsafe {
        *(p as *mut usize) = info as usize;
        core::ptr::copy_nonoverlapping(code.as_ptr(), p.add(4), code.len());
    }
    token
}

pub fn method_token(intro: *mut Class, slot: usize, index: usize, aggregate: bool) -> usize {
    let info = TokenInfo { magic: TOKEN_MAGIC, kind: 0, class: intro, slot, index };
    make_token(info, |t| method_code(t, unsafe { &*intro }, slot, aggregate))
}

pub fn data_token(intro: *mut Class, extra: usize) -> usize {
    let info = TokenInfo { magic: TOKEN_MAGIC, kind: 1, class: intro, slot: extra, index: 0 };
    make_token(info, |t| data_code(t, unsafe { &*intro }, extra))
}

/// `void somRenewNoInitNoZeroThunk(void *)`: store the method table.
pub fn renew_thunk(mtab: usize) -> usize {
    let mut c = Vec::new();
    emit(&mut c, &[0x8b, 0x44, 0x24, 0x04, 0xc7, 0x00]);
    emit32(&mut c, mtab);
    emit(&mut c, &[0xc3]);
    place(core::ptr::null(), &c)
}

// ---- Resolution ----

/// A method's implementation in the method table of class `c`.
pub unsafe fn resolve_in(c: &Class, token: usize) -> usize {
    let Some(ti) = (unsafe { token_info(token) }) else { return 0 };
    if ti.kind != 0 {
        return 0;
    }
    match c.block_of(ti.class) {
        Some(b) => unsafe { c.mt().entry(b + ti.slot) },
        None => 0,
    }
}

/// The address of a data token's data in an object.
pub unsafe fn data_in(obj: usize, token: usize) -> usize {
    if obj == 0 {
        return 0;
    }
    let Some(ti) = (unsafe { token_info(token) }) else { return 0 };
    let c = unsafe { &*class_of_obj(obj) };
    match c.data_offset_of(ti.class) {
        Some(off) => obj + off + ti.slot,
        None => 0,
    }
}

/// The thunks' slow path: the method for `obj`'s class.
pub extern "C" fn slow_resolve(obj: usize, token: usize) -> usize {
    let c = unsafe { class_of_obj(obj) };
    let m = if c.is_null() { 0 } else { unsafe { resolve_in(&*c, token) } };
    if m != 0 {
        return m;
    }
    crate::error(20101, b"rt.rs\0", 1);
    crate::__a_no_method as usize
}

pub extern "C" fn slow_data_resolve(obj: usize, token: usize) -> usize {
    let p = unsafe { data_in(obj, token) };
    if p == 0 {
        crate::error(20102, b"rt.rs\0", 2);
    }
    p
}

// ---- Finding methods ----

/// A method by name: (introducing class, its definition, block index).
pub fn find<'a>(c: &'a Class, key: usize, scope: Option<&[u8]>) -> Option<(&'a Class, &'a MethodDef, usize)> {
    for &(x, xb) in &c.blocks {
        let x = unsafe { &*x };
        if let Some(s) = scope {
            if !name_eq(s, x.name_bytes()) {
                continue;
            }
        }
        if let Some(m) = x.methods.iter().find(|m| m.key == key) {
            return Some((x, m, xb));
        }
    }
    None
}

/// A dynamic method by name, in the class or its ancestors.
pub fn find_dynamic<'a>(c: &'a Class, key: usize) -> Option<&'a MethodDef> {
    if let Some(m) = c.dynamic.iter().find(|m| m.key == key) {
        return Some(m);
    }
    for &p in &c.parents {
        if let Some(m) = find_dynamic(unsafe { &*p }, key) {
            return Some(m);
        }
    }
    None
}

/// Split "Class::method" into its scope and name.
pub fn split_descriptor(s: &[u8]) -> (Option<&[u8]>, &[u8]) {
    let s = s.strip_prefix(b"::").unwrap_or(s);
    let mut i = s.len();
    while i >= 2 {
        if s[i - 1] == b':' && s[i - 2] == b':' {
            let scope = &s[..i - 2];
            let scope = match scope.iter().rposition(|&c| c == b':') {
                Some(j) => &scope[j + 1..],
                None => scope,
            };
            return (Some(scope), &s[i..]);
        }
        i -= 1;
    }
    (None, s)
}

/// A method's implementation for instances of `c`, by name or
/// descriptor (static first, then dynamic).
pub fn lookup<'a>(c: &'a Class, name: &[u8]) -> Option<(usize, Option<&'a MethodDef>)> {
    let (scope, simple) = split_descriptor(name);
    let key = id::key_of_name(simple);
    if let Some((_, m, xb)) = find(c, key, scope) {
        return Some((unsafe { c.mt().entry(xb + m.slot) }, Some(m)));
    }
    find_dynamic(c, key).map(|m| (m.proc_, Some(m)))
}

// ---- Building a class ----

/// Lay out class `c` (its object `obj`) with `parents`: method table,
/// instance data, parent method tables, tokens for the instance data.
pub unsafe fn init_mi(
    c: *mut Class,
    obj: usize,
    name: &[u8],
    parents: &[*mut Class],
    data_size: usize,
    align: usize,
    max_methods: usize,
    major: i32,
    minor: i32,
) {
    let c = unsafe { &mut *c };
    let (e, _) = id::entry(name);
    c.key = e as usize;
    c.name = unsafe { (*e).name };
    c.obj = obj;
    c.parents = parents.to_vec();
    c.major = major;
    c.minor = minor;
    c.data_size = data_size;
    c.align = align.clamp(1, 16).max(if data_size >= 4 { 4 } else { 1 });

    // Method table: the first parent's table (its blocks keep their
    // places), this class's block, then the blocks the other parents add.
    let mut ents: Vec<usize> = Vec::new();
    let mut blocks: Vec<(*mut Class, usize)> = Vec::new();
    if let Some(&p1) = parents.first() {
        let p1 = unsafe { &*p1 };
        ents.extend_from_slice(unsafe { core::slice::from_raw_parts(p1.mt().entries(), p1.nentries) });
        blocks.extend_from_slice(&p1.blocks);
    }
    c.base = ents.len();
    c.block_len = max_methods + 8;
    ents.push(c as *mut Class as usize);
    ents.extend(core::iter::repeat_n(0, c.block_len));
    blocks.push((c as *mut Class, c.base));
    for &p in parents.iter().skip(1) {
        let p = unsafe { &*p };
        for &(x, xb) in &p.blocks {
            let xr = unsafe { &*x };
            let theirs = unsafe { core::slice::from_raw_parts(p.mt().entries().add(xb), 1 + xr.block_len) };
            match blocks.iter().find(|b| b.0 == x) {
                Some(&(_, mb)) => {
                    // Reached twice: an implementation that overrides the
                    // introducer's wins over one that does not.
                    for (k, m) in xr.methods.iter().enumerate() {
                        let slot = 1 + k;
                        if ents[mb + slot] == m.proc_ && theirs[slot] != m.proc_ {
                            ents[mb + slot] = theirs[slot];
                        }
                    }
                }
                None => {
                    let nb = ents.len();
                    ents.extend_from_slice(theirs);
                    blocks.push((x, nb));
                }
            }
        }
    }
    // Every class has its own initializers, destructor and assignment
    // operators: SOMObject's generic ones unless it overrides them.
    let so = unsafe { SOMOBJECT };
    if !so.is_null() && !core::ptr::eq(so, c) {
        let so = unsafe { &*so };
        for &i in &om::PER_CLASS {
            if let Some(m) = so.methods.get(i) {
                ents[1 + i] = m.proc_;
            }
        }
    }
    c.nentries = ents.len();
    c.blocks = blocks;

    // Instance data: the first parent's, this class's, then the data of
    // classes the other parents add.
    let mut offsets: Vec<(*mut Class, usize)> = Vec::new();
    let mut size = 4;
    if let Some(&p1) = parents.first() {
        let p1 = unsafe { &*p1 };
        offsets.extend_from_slice(&p1.data_offsets);
        size = p1.instance_size;
    }
    c.own_offset = align_up(size, c.align);
    offsets.push((c as *mut Class, c.own_offset));
    size = c.own_offset + data_size;
    for &p in parents.iter().skip(1) {
        let p = unsafe { &*p };
        for &(x, _) in &p.data_offsets {
            if offsets.iter().any(|d| d.0 == x) {
                continue;
            }
            let xr = unsafe { &*x };
            let off = align_up(size, xr.align);
            offsets.push((x, off));
            size = off + xr.data_size;
        }
    }
    c.instance_size = align_up(size, 4);
    c.data_offsets = offsets;

    let mut words = vec![0usize; MTAB_HDR / 4 + c.nentries];
    words[MTAB_HDR / 4..].copy_from_slice(&ents);
    let mt = leak_vec(words) as *mut Mtab;
    unsafe {
        (*mt).class_object = obj;
        (*mt).class_info = c;
        (*mt).class_name = c.name;
        (*mt).instance_size = c.instance_size as i32;
        (*mt).data_alignment = c.align as i32;
        (*mt).mtab_size = (MTAB_HDR + 4 * c.nentries) as i32;
    }
    c.mtab = mt;

    // SOMClass's data (the class record) in this class's instances.
    let sc = unsafe { SOMCLASS };
    c.somclass_off = if sc.is_null() { 0 } else { c.data_offset_of(sc).unwrap_or(0) };

    // The parent method tables: this class's, then its parents'.
    let mut next: *mut MtabList = core::ptr::null_mut();
    for &p in parents.iter().rev() {
        next = leak(MtabList { mtab: unsafe { (*p).mtab }, next });
    }
    c.pmtab = leak(ParentMtab {
        mtab: mt,
        next,
        class_object: obj,
        renew_thunk: renew_thunk(mt as usize),
        instance_size: c.instance_size as i32,
        initializers: core::ptr::null_mut(),
        resolved: core::ptr::null_mut(),
        init_ctrl: Ctrl::empty(),
        destruct_ctrl: Ctrl::empty(),
        assign_ctrl: Ctrl::empty(),
        embedded_total: 0,
        hierarchy_total: 0,
        unused: 0,
    });
    c.inst_token = data_token(c, 0);
    c.direct_init = parents.to_vec();
}

/// Add a static method; returns its token (the existing one if the class
/// already has the method).
pub unsafe fn add_static(c: *mut Class, mid: SomId, desc: SomId, proc_: usize, rd: usize, ap: usize) -> usize {
    let cr = unsafe { &mut *c };
    let key = id::key_of(mid);
    if let Some(i) = cr.methods.iter().position(|m| m.key == key) {
        if proc_ != 0 {
            cr.methods[i].proc_ = proc_;
            unsafe { cr.mt().set(cr.base + cr.methods[i].slot, proc_) };
        }
        return cr.methods[i].token;
    }
    // Reintroduced (an ancestor has the method): the ancestor's token,
    // with this implementation.
    if let Some(&(x, xb)) = cr.blocks.iter().find(|b| b.0 != c && unsafe { (*b.0).methods.iter().any(|m| m.key == key) }) {
        let m = unsafe { (*x).methods.iter().find(|m| m.key == key).unwrap() };
        if proc_ != 0 {
            unsafe { cr.mt().set(xb + m.slot, proc_) };
        }
        return m.token;
    }
    let (mut aggregate, mut va_size, mut call_type) = (false, 0, 0);
    let (mut rd, mut ap) = (rd, ap);
    if rd == usize::MAX {
        // ap points to a somApRdInfo (or is null: no signature known).
        if ap != 0 {
            let info = ap as *const [usize; 3];
            let si = unsafe { (*info)[2] } as *const [u32; 2];
            rd = unsafe { (*info)[0] };
            ap = unsafe { (*info)[1] };
            if !si.is_null() {
                call_type = unsafe { (*si)[0] };
                va_size = unsafe { (*si)[1] };
                aggregate = call_type & !1 == 8;
            }
        } else {
            rd = 0;
        }
    }
    let slot = cr.methods.len() + 1;
    if slot > cr.block_len {
        crate::error(20111, b"rt.rs\0", 3);
        return 0;
    }
    let index = cr.methods.len();
    let token = method_token(c, slot, index, aggregate);
    unsafe { cr.mt().set(cr.base + slot, proc_) };
    cr.methods.push(MethodDef {
        key,
        name: id::name_of(mid),
        descriptor: desc,
        token,
        slot,
        proc_,
        rd,
        ap,
        aggregate,
        va_size,
        call_type,
    });
    token
}

/// Override a method by name or descriptor ("Class::method"). Overrides
/// of somInit and somUninit become the class's legacy initializer and
/// uninitializer, which its generic somDefaultInit and somDestruct call
/// (IBM's SOM does the same).
pub unsafe fn override_method(c: *mut Class, mid: SomId, proc_: usize) -> bool {
    let cr = unsafe { &mut *c };
    let full = id::name_bytes(mid);
    let (scope, simple) = split_descriptor(full);
    if name_eq(simple, b"somInit") && cr.legacy_init == 0 && !core::ptr::eq(c, unsafe { SOMOBJECT }) {
        cr.legacy_init = proc_;
        return true;
    }
    if name_eq(simple, b"somUninit") && cr.legacy_uninit == 0 && !core::ptr::eq(c, unsafe { SOMOBJECT }) {
        cr.legacy_uninit = proc_;
        return true;
    }
    let key = id::key_of_name(simple);
    let mut found = false;
    for i in 0..cr.blocks.len() {
        let (x, xb) = cr.blocks[i];
        let xr = unsafe { &*x };
        if let Some(s) = scope {
            if !name_eq(s, xr.name_bytes()) {
                continue;
            }
        }
        if let Some(m) = xr.methods.iter().find(|m| m.key == key) {
            unsafe { cr.mt().set(xb + m.slot, proc_) };
            found = true;
        }
    }
    if !found {
        if let Some(m) = cr.dynamic.iter_mut().find(|m| m.key == key) {
            m.proc_ = proc_;
            found = true;
        }
    }
    found
}

pub unsafe fn add_dynamic(c: *mut Class, mid: SomId, desc: SomId, proc_: usize, ap: usize) {
    let cr = unsafe { &mut *c };
    let key = id::key_of(mid);
    if let Some(m) = cr.dynamic.iter_mut().find(|m| m.key == key) {
        m.proc_ = proc_;
        return;
    }
    cr.dynamic.push(MethodDef {
        key,
        name: id::name_of(mid),
        descriptor: desc,
        token: 0,
        slot: 0,
        proc_,
        rd: 0,
        ap,
        aggregate: false,
        va_size: 0,
        call_type: 0,
    });
}

/// A class's implementation of one of SOMObject's methods.
pub unsafe fn so_method(c: &Class, idx: usize) -> usize {
    unsafe { c.mt().entry(1 + idx) }
}

// ---- Initializer, destructor and assignment controls ----

/// Fill in the class's init, destruct and assign controls (in its
/// parent mtab structure): which ancestors' initializers each
/// initializer calls (the mask) and the information they use.
pub unsafe fn build_ctrls(c: *mut Class) {
    let cr = unsafe { &mut *c };
    // Initialization: depth first, each class at its first occurrence.
    let mut order: Vec<*mut Class> = vec![c];
    let mut mask: Vec<u8> = Vec::new();
    fn visit_first(x: *mut Class, order: &mut Vec<*mut Class>, mask: &mut Vec<u8>) {
        let parents = unsafe { (*x).parents.clone() };
        let start = mask.len();
        mask.resize(start + parents.len(), 0);
        for (i, &p) in parents.iter().enumerate() {
            if !order.contains(&p) {
                mask[start + i] = 1;
                order.push(p);
                visit_first(p, order, mask);
            }
        }
    }
    visit_first(c, &mut order, &mut mask);
    let mut init: Vec<InitInfo> = Vec::new();
    let mut assign: Vec<AssignInfo> = Vec::new();
    for &x in &order {
        let xr = unsafe { &*x };
        let off = cr.data_offset_of(x).unwrap_or(0) as i32;
        let ci = unsafe { so_method(xr, om::DEFAULT_COPY_INIT) };
        let cci = unsafe { so_method(xr, om::DEFAULT_CONST_COPY_INIT) };
        init.push(InitInfo {
            cls: xr.obj,
            default_init: unsafe { so_method(xr, om::DEFAULT_INIT) },
            default_copy_init: ci,
            default_const_copy_init: cci,
            default_ncarg_copy_init: if ci != 0 { ci } else { cci },
            data_offset: off,
            legacy_init: xr.legacy_init,
        });
        let a = unsafe { so_method(xr, om::DEFAULT_ASSIGN) };
        let ca = unsafe { so_method(xr, om::DEFAULT_CONST_ASSIGN) };
        assign.push(AssignInfo {
            cls: xr.obj,
            default_assign: a,
            default_const_assign: ca,
            default_ncarg_assign: if a != 0 { a } else { ca },
            uda_assign: 0,
            uda_const_assign: 0,
            data_offset: off,
        });
    }
    let n = init.len();
    init.push(InitInfo {
        cls: 0,
        default_init: 0,
        default_copy_init: 0,
        default_const_copy_init: 0,
        default_ncarg_copy_init: 0,
        data_offset: 0,
        legacy_init: 0,
    });
    assign.push(AssignInfo {
        cls: 0,
        default_assign: 0,
        default_const_assign: 0,
        default_ncarg_assign: 0,
        uda_assign: 0,
        uda_const_assign: 0,
        data_offset: 0,
    });
    let mask_init = leak_vec(mask.clone());
    let pm = unsafe { &mut *cr.pmtab };
    pm.init_ctrl = Ctrl { mask: mask_init, info: leak_vec(init) as usize, info_size: 0x1c, ctrl_info: n };
    pm.assign_ctrl = Ctrl { mask: leak_vec(mask), info: leak_vec(assign) as usize, info_size: 0x1c, ctrl_info: n };

    // Destruction: the mirror of initialization (as IBM's SOM does it):
    // depth first with the parents right to left, each class at its last
    // occurrence (after every class that has it as an ancestor). The mask
    // has a byte per parent in the order they are visited.
    fn size(x: *mut Class) -> usize {
        1 + unsafe { (*x).parents.iter().map(|&p| size(p)).sum::<usize>() }
    }
    fn last(x: *mut Class, pos: usize, out: &mut Vec<(*mut Class, usize)>) {
        match out.iter_mut().find(|e| e.0 == x) {
            Some(e) => e.1 = pos,
            None => out.push((x, pos)),
        }
        let mut q = pos + 1;
        for &p in unsafe { (*x).parents.iter().rev() } {
            last(p, q, out);
            q += size(p);
        }
    }
    let mut lasts = Vec::new();
    last(c, 0, &mut lasts);
    let mut dorder: Vec<*mut Class> = vec![c];
    let mut dmask: Vec<u8> = Vec::new();
    fn visit_last(x: *mut Class, pos: usize, lasts: &[(*mut Class, usize)], order: &mut Vec<*mut Class>, mask: &mut Vec<u8>) {
        let parents: Vec<*mut Class> = unsafe { (*x).parents.iter().rev().copied().collect() };
        let start = mask.len();
        mask.resize(start + parents.len(), 0);
        let mut q = pos + 1;
        for (i, &p) in parents.iter().enumerate() {
            let lp = lasts.iter().find(|e| e.0 == p).map_or(q, |e| e.1);
            if lp == q && !order.contains(&p) {
                mask[start + i] = 1;
                order.push(p);
                visit_last(p, q, lasts, order, mask);
            }
            q += size(p);
        }
    }
    visit_last(c, 0, &lasts, &mut dorder, &mut dmask);
    let mut dest: Vec<DestructInfo> = dorder
        .iter()
        .map(|&x| {
            let xr = unsafe { &*x };
            DestructInfo {
                cls: xr.obj,
                default_destruct: unsafe { so_method(xr, om::DESTRUCT) },
                data_offset: cr.data_offset_of(x).unwrap_or(0) as i32,
                legacy_uninit: xr.legacy_uninit,
            }
        })
        .collect();
    let dn = dest.len();
    dest.push(DestructInfo { cls: 0, default_destruct: 0, data_offset: 0, legacy_uninit: 0 });
    pm.destruct_ctrl = Ctrl { mask: leak_vec(dmask), info: leak_vec(dest) as usize, info_size: 0x10, ctrl_info: dn };
    pm.hierarchy_total = order.len() as i32;
    cr.ctrl_built = true;
}
