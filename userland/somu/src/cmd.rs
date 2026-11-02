//! SOMDesc's modifier and ownership queries, SOMArgList's text form of an
//! argument list (somutGetArgs/somutSetArgs), SOMCommand (a method call
//! written out as text and run again from it) and the externalization
//! functions built on them, as IBM's SOMU.DLL has them (Ghidra).
//!
//! The text is IBM's: numbers in `%10d` fields. An argument list is its
//! count, then per argument a type letter (as a `%10d` number) and its
//! value: `b` char/octet (the character), `s` short, `S` unsigned short,
//! `l` long, `L` unsigned long, `B` boolean (`%10d`), `f` float, `d`
//! double (`%20f`), `z` string (length, text), `o` object (length, its
//! externalized form). An object is written by the first of three methods
//! its class has (by default somrGetObjName, sompGetPersistentIdString,
//! somrGetState): `R` and the name, `P` and the persistent id, or `L`, the
//! class name, its version and the state. A command is the method's name
//! and the class's name (each `%10d%s`), the target object, then the
//! argument list.

use alloc::format;
use alloc::vec::Vec;

use somsup::{SomId, call};

use crate::desc::*;
use crate::*;

// ---- SOMDesc: modifiers and ownership ----

/// The `{ name, value }` modifier pairs of a repository definition.
fn modifiers_of(def: usize) -> Vec<(*const u8, *const u8)> {
    if def == 0 {
        return Vec::new();
    }
    let mut s = [0usize; 3];
    call(cdp(unsafe { &ContainedClassData }), 8, &[&mut s as *mut _ as usize, def, 0]);
    let n = s[1];
    (0..n).map(|i| unsafe { (*(s[2] as *const usize).add(2 * i) as *const u8, *(s[2] as *const usize).add(2 * i + 1) as *const u8) }).collect()
}

/// somutGetOperationModifier(name): the method's modifier's value ("" for
/// one without a value; the attribute's for an accessor), NULL if none.
pub extern "C" fn get_op_modifier(o: usize, name: *const u8) -> *const u8 {
    let d = ds(o);
    let want = unsafe { somsup::cstr(name) };
    for (k, v) in modifiers_of(d.def) {
        if unsafe { somsup::cstr(k) } == want {
            return if v.is_null() { b"\0".as_ptr() } else { v };
        }
    }
    core::ptr::null()
}

fn listed(list: *const u8, name: &[u8]) -> bool {
    !list.is_null() && unsafe { somsup::cstr(list) }.split(|&c| c == b',' || c == b' ').any(|w| w == name)
}

/// Whether the interface says `memory_management = corba`.
fn corba(o: usize) -> bool {
    let d = ds(o);
    if d.spare == 0 {
        return false;
    }
    let m = modifier(unsafe { somsup::cstr(d.spare as *const u8) }, b"memory_management");
    let r = !m.is_null() && unsafe { somsup::cstr(m) } == b"corba";
    somsup::free(m);
    r
}

/// _get_somutReturnResultOwner: 2 the object owns the result
/// (object_owns_result), 1 the caller (caller_owns_result or CORBA memory
/// management), 3 unspecified.
pub extern "C" fn result_owner(o: usize) -> u32 {
    if !get_op_modifier(o, b"object_owns_result\0".as_ptr()).is_null() {
        2
    } else if corba(o) || !get_op_modifier(o, b"caller_owns_result\0".as_ptr()).is_null() {
        1
    } else {
        3
    }
}

/// somutGetArgOwner(i): as the result's, from object_owns_parameters and
/// caller_owns_parameters (CORBA memory management: the caller's unless
/// the object's).
pub extern "C" fn arg_owner(o: usize, i: u32) -> u32 {
    let Some(a) = arg_info(ds(o), i & 0xffff) else { return 3 };
    let name = unsafe { somsup::cstr(a[0] as *const u8) };
    if listed(get_op_modifier(o, b"object_owns_parameters\0".as_ptr()), name) {
        2
    } else if corba(o) || listed(get_op_modifier(o, b"caller_owns_parameters\0".as_ptr()), name) {
        1
    } else {
        3
    }
}

/// somutGetArgIndex(name, &index).
pub extern "C" fn arg_index(o: usize, name: *const u8, index: *mut u16) -> i32 {
    let d = ds(o);
    if d.def == 0 {
        return -1;
    }
    let want = unsafe { somsup::cstr(name) };
    for i in 0..num_args(d) {
        let Some(a) = arg_info(d, i) else { break };
        // (An accessor's argument is named after its attribute.)
        let n = if a[0] == 0 && d.is_attr != 0 { attr_name(d) } else { unsafe { somsup::cstr(a[0] as *const u8) } };
        if n == want {
            unsafe { *index = i as u16 };
            return 0;
        }
    }
    -1
}

fn attr_name(d: &Ds) -> &'static [u8] {
    let n = call(cdp(unsafe { &ContainedClassData }), 2, &[d.def, 0]) as *const u8;
    unsafe { somsup::cstr(n) }
}

/// somutFlushCache and somutFlushEntry: the descriptions are not cached
/// here (each SOMDesc asks the repository), so there is nothing to drop.
pub extern "C" fn flush(_o: usize) -> i32 {
    0
}

// ---- Text fields ----

fn d10(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(format!("{:>10}", v).as_bytes());
}

fn f20(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(format!("{:>20.6}", v).as_bytes());
}

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn field(&mut self, n: usize) -> &[u8] {
        let a = self.i.min(self.s.len());
        let b = (self.i + n).min(self.s.len());
        self.i = b;
        &self.s[a..b]
    }
    fn int(&mut self) -> Option<i64> {
        let f = self.field(10);
        core::str::from_utf8(f).ok()?.trim().parse().ok()
    }
    fn float(&mut self) -> Option<f64> {
        let f = self.field(20);
        core::str::from_utf8(f).ok()?.trim().parse().ok()
    }
    fn text(&mut self) -> Option<Vec<u8>> {
        let n = self.int()?;
        if n < 0 || self.i + n as usize > self.s.len() {
            return None;
        }
        Some(self.field(n as usize).to_vec())
    }
}

// ---- Objects ----

fn find_method(cls: usize, name: *const u8) -> usize {
    if cls == 0 || name.is_null() {
        return 0;
    }
    let id = somsup::id(unsafe { somsup::cstr(name) });
    call(somsup::sc(), somsup::cm::SOM_FIND_S_METHOD, &[cls, id as usize])
}

const EXT_NAME: &[u8] = b"somrGetObjName\0";
const EXT_ID: &[u8] = b"sompGetPersistentIdString\0";
const EXT_STATE: &[u8] = b"somrGetState\0";
const INT_NAME: &[u8] = b"softdrGetObjPtrFromName\0";
const INT_ID: &[u8] = b"sompRestoreObjectFromIdString\0";
const INT_STATE: &[u8] = b"somrSetState\0";

fn or(p: *const u8, d: &'static [u8]) -> *const u8 {
    if p.is_null() { d.as_ptr() } else { p }
}

/// An object's external form (by the first of the three methods its
/// class has), or None.
pub fn externalize(obj: usize, m1: *const u8, m2: *const u8, m3: *const u8) -> Option<Vec<u8>> {
    let cls = somsup::class_of(obj);
    if cls == 0 {
        return None;
    }
    let ev = unsafe { somsup::somGetGlobalEnvironment() } as usize;
    let mut out = Vec::new();
    let f = find_method(cls, m1);
    if f != 0 {
        let name = unsafe { somsup::cstr(somsup::call_fn(f, &[obj, ev]) as *const u8) };
        out.push(b'R');
        d10(&mut out, name.len() as i64);
        out.extend_from_slice(name);
        return Some(out);
    }
    let f = find_method(cls, m2);
    if f != 0 {
        let mut buf = [0u8; 512];
        somsup::call_fn(f, &[obj, ev, buf.as_mut_ptr() as usize]);
        let id = unsafe { somsup::cstr(buf.as_ptr()) };
        out.push(b'P');
        d10(&mut out, id.len() as i64);
        out.extend_from_slice(id);
        return Some(out);
    }
    let f = find_method(cls, m3);
    if f == 0 {
        return None;
    }
    // somrGetState(obj, ev, &state): the state (SOMMalloc'd) is its
    // size, these 4 bytes included, then the bytes.
    let mut state: *mut u8 = core::ptr::null_mut();
    somsup::call_fn(f, &[obj, ev, &mut state as *mut *mut u8 as usize]);
    if state.is_null() {
        return None;
    }
    let size = (unsafe { core::ptr::read_unaligned(state as *const i32) } - 4).max(0) as usize;
    let bytes = unsafe { core::slice::from_raw_parts(state.add(4), size) };
    let name = unsafe { somsup::cstr(somsup::class_name(cls)) };
    let (mut major, mut minor) = (0i32, 0i32);
    call(somsup::sc(), 41, &[cls, &mut major as *mut i32 as usize, &mut minor as *mut i32 as usize]);
    out.push(b'L');
    d10(&mut out, name.len() as i64);
    out.extend_from_slice(name);
    d10(&mut out, major as i64);
    d10(&mut out, minor as i64);
    d10(&mut out, size as i64);
    out.extend_from_slice(bytes);
    somsup::free(state);
    Some(out)
}

/// An object from its external form: the `L` form makes an instance of
/// the class and gives it the state (somrSetState); the `R` and `P` forms
/// need the replication and persistence frameworks, which rs2 does not
/// have.
fn internalize(r: &mut Reader, m3: *const u8) -> Option<usize> {
    let tag = *r.field(1).first()?;
    let name = r.text()?;
    if tag != b'L' {
        return None;
    }
    let major = r.int()? as i32;
    let minor = r.int()? as i32;
    let state = r.text()?;
    let cls = somsup::find_class(&name, major, minor);
    if cls == 0 {
        return None;
    }
    let obj = somsup::new_object(cls);
    let f = find_method(cls, m3);
    if obj == 0 || f == 0 {
        return None;
    }
    let buf = somsup::malloc(state.len() + 4);
    unsafe {
        core::ptr::write_unaligned(buf as *mut i32, state.len() as i32 + 4);
        core::ptr::copy_nonoverlapping(state.as_ptr(), buf.add(4), state.len());
    }
    let ev = unsafe { somsup::somGetGlobalEnvironment() } as usize;
    somsup::call_fn(f, &[obj, ev, buf as usize]);
    somsup::free(buf);
    Some(obj)
}

// ---- SOMArgList: the text form ----

/// The first argument of the list that is an operation's (callstyle idl:
/// after the environment and the context).
fn first_arg(d: &Ds) -> u32 {
    let mut i = 0;
    if d.callstyle == 2 {
        i += 1;
        if !d.op.is_null() && unsafe { *d.op.add(6) } != 0 {
            i += 1;
        }
    }
    i
}

fn args_text(o: usize, m1: *const u8, m2: *const u8, m3: *const u8) -> Option<Vec<u8>> {
    let desc = al(o).desc;
    if desc == 0 {
        return None;
    }
    let d = ds(desc);
    if d.def == 0 || d.callstyle == 3 {
        return None;
    }
    let n = num_args(d);
    let mut out = Vec::new();
    d10(&mut out, n as i64);
    let base = first_arg(d);
    for i in 0..n {
        let a = arg_info(d, i)?;
        let p = arg_at(o, base + i);
        let w = unsafe { core::ptr::read_unaligned(p as *const u32) };
        match unsafe { tcKind(a[3], 0) } {
            10 | 11 => {
                d10(&mut out, b'b' as i64);
                out.push(w as u8);
            }
            3 => {
                d10(&mut out, b's' as i64);
                d10(&mut out, w as i16 as i64);
            }
            5 => {
                d10(&mut out, b'S' as i64);
                d10(&mut out, w as u16 as i64);
            }
            4 => {
                d10(&mut out, b'l' as i64);
                d10(&mut out, w as i32 as i64);
            }
            6 => {
                d10(&mut out, b'L' as i64);
                // (%10d of an unsigned long, as IBM's.)
                d10(&mut out, w as i32 as i64);
            }
            9 => {
                d10(&mut out, b'B' as i64);
                d10(&mut out, (w & 0xff) as i64);
            }
            7 | 8 => {
                let v = unsafe { core::ptr::read_unaligned(p as *const f64) };
                d10(&mut out, if unsafe { tcKind(a[3], 0) } == 7 { b'f' } else { b'd' } as i64);
                f20(&mut out, v);
            }
            19 => {
                let s = unsafe { somsup::cstr(w as *const u8) };
                d10(&mut out, b'z' as i64);
                d10(&mut out, s.len() as i64);
                out.extend_from_slice(s);
            }
            15 => {
                let e = externalize(w as usize, m1, m2, m3)?;
                d10(&mut out, b'o' as i64);
                d10(&mut out, e.len() as i64);
                out.extend_from_slice(&e);
            }
            _ => return None,
        }
    }
    Some(out)
}

fn put(text: &[u8], buf: *mut u8, size: u32) -> i32 {
    if text.len() + 1 > size as usize || buf.is_null() {
        return -2;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), buf, text.len());
        *buf.add(text.len()) = 0;
    }
    0
}

/// somutGetArgs(buffer, size, m1, m2, m3): the list as text; -2 when the
/// buffer is too small, -1 when an argument cannot be written.
pub extern "C" fn get_args(o: usize, buf: *mut u8, size: u32, m1: *const u8, m2: *const u8, m3: *const u8) -> i32 {
    match args_text(o, or(m1, EXT_NAME), or(m2, EXT_ID), or(m3, EXT_STATE)) {
        Some(t) => put(&t, buf, size),
        None => -1,
    }
}

fn set_args_from(o: usize, desc: usize, r: &mut Reader, m3: *const u8) -> i32 {
    let a = al(o);
    unsafe {
        (*a.list).clear();
        (*a.offsets).clear();
    }
    a.desc = desc;
    let d = ds(desc);
    if d.def == 0 || d.callstyle == 3 {
        return -1;
    }
    if d.callstyle == 2 {
        let ev = unsafe { somsup::somGetGlobalEnvironment() } as u32;
        add(o, &ev.to_le_bytes());
        if !d.op.is_null() && unsafe { *d.op.add(6) } != 0 {
            add(o, &0u32.to_le_bytes());
        }
    }
    let Some(n) = r.int() else { return -1 };
    for _ in 0..n {
        let Some(t) = r.int() else { return -1 };
        let ok = match t as u8 {
            b'b' => r.field(1).first().map(|&c| add(o, &(c as u32).to_le_bytes())),
            b's' | b'S' | b'l' | b'L' | b'B' => r.int().map(|v| add(o, &(v as u32).to_le_bytes())),
            b'f' | b'd' => r.float().map(|v| add(o, &v.to_le_bytes())),
            b'z' => r.text().map(|s| add(o, &(somsup::dup(&s) as u32).to_le_bytes())),
            b'o' => r.text().and_then(|e| internalize(&mut Reader { s: &e, i: 0 }, m3)).map(|x| add(o, &(x as u32).to_le_bytes())),
            _ => None,
        };
        if ok.is_none() {
            return -1;
        }
    }
    0
}

/// somutSetArgs(desc, text, m1, m2, m3): the list from text.
pub extern "C" fn set_args(o: usize, desc: usize, text: *const u8, _m1: *const u8, _m2: *const u8, m3: *const u8) -> i32 {
    let s = unsafe { somsup::cstr(text) };
    set_args_from(o, desc, &mut Reader { s, i: 0 }, or(m3, INT_STATE))
}

// ---- SOMCommand ----

/// What a SOMCommand keeps (IBM's 20 bytes).
#[repr(C)]
pub struct Cm {
    args: usize,
    method: SomId,
    target: usize,
    class: usize,
    desc: usize,
}

fn cm(o: usize) -> &'static mut Cm {
    somsup::data::<Cm>(p!(SOMCommandCClassData), o)
}

fn new_desc() -> usize {
    somsup::new_object(SOMDescNewClass(2, 1))
}

fn new_arglist() -> usize {
    somsup::new_object(SOMArgListNewClass(2, 1))
}

/// somutSetCommand(target, methodId, argList).
pub extern "C" fn set_command(o: usize, target: usize, method: SomId, args: usize) {
    *cm(o) = Cm { args, method, target, class: 0, desc: 0 };
}

/// somutSetClassCommand(target, class, methodId, ap): the call with its
/// arguments (the class the method is taken from: the target's if 0).
pub extern "C" fn set_class_command(o: usize, target: usize, class: usize, method: SomId, ap: *const u32) -> i32 {
    if target == 0 {
        return -1;
    }
    let c = cm(o);
    c.target = target;
    c.class = if class == 0 { somsup::class_of(target) } else { class };
    c.method = method;
    c.desc = new_desc();
    if desc::set_som_desc_for(c.desc, target, unsafe { somsup::somStringFromId(method) }) != 0 {
        return -3;
    }
    c.args = new_arglist();
    if set_arg_list(c.args, c.desc, ap) != 0 {
        return -4;
    }
    0
}

fn command_text(o: usize, m1: *const u8, m2: *const u8, m3: *const u8) -> Option<Vec<u8>> {
    let c = cm(o);
    let name = unsafe { somsup::cstr(somsup::somStringFromId(c.method)) };
    let mut out = Vec::new();
    d10(&mut out, name.len() as i64);
    out.extend_from_slice(name);
    let cls = if c.class != 0 { c.class } else { somsup::class_of(c.target) };
    let cname = unsafe { somsup::cstr(somsup::class_name(cls)) };
    d10(&mut out, cname.len() as i64);
    out.extend_from_slice(cname);
    out.extend_from_slice(&externalize(c.target, m1, m2, m3)?);
    if c.args != 0 {
        out.extend_from_slice(&args_text(c.args, m1, m2, m3)?);
    }
    Some(out)
}

/// somutGetExtRep(buffer, size, m1, m2, m3): the command as text (-2: the
/// buffer is too small).
pub extern "C" fn get_ext_rep(o: usize, buf: *mut u8, size: u32, m1: *const u8, m2: *const u8, m3: *const u8) -> i32 {
    match command_text(o, or(m1, EXT_NAME), or(m2, EXT_ID), or(m3, EXT_STATE)) {
        Some(t) => put(&t, buf, size),
        None => -1,
    }
}

/// somutSetExtRep(text, ..., m1, m2, m3): the command from its text (the
/// target made again from its external form).
pub extern "C" fn set_ext_rep(o: usize, text: *const u8, _x: usize, _y: usize, _m1: *const u8, _m2: *const u8, m3: *const u8) -> i32 {
    let s = unsafe { somsup::cstr(text) };
    let mut r = Reader { s, i: 0 };
    let Some(method) = r.text() else { return -1 };
    let Some(cname) = r.text() else { return -1 };
    let m3 = or(m3, INT_STATE);
    let Some(target) = internalize(&mut r, m3) else { return -1 };
    let c = cm(o);
    c.method = somsup::id(&method);
    let cid = somsup::id(&cname);
    c.class = call(somsup::scm(), somsup::mm::SOM_CLASS_FROM_ID, &[unsafe { somsup::SOMClassMgrObject }, cid as usize]);
    c.target = target;
    c.desc = new_desc();
    let mz = somsup::z(&method);
    let rc = desc::set_som_desc_for(c.desc, target, mz.as_ptr());
    c.args = new_arglist();
    if r.i < s.len() {
        return set_args_from(c.args, c.desc, &mut r, m3);
    }
    rc
}

/// somutDoCommand(&result): the call (somDispatch, or somClassDispatch
/// when the command names a class).
pub extern "C" fn do_command(o: usize, ret: *mut u8) -> u32 {
    let c = cm(o);
    if c.target == 0 {
        return 0;
    }
    // somDispatch's va_list starts with the receiver.
    let mut list = (c.target as u32).to_le_bytes().to_vec();
    if c.args != 0 {
        list.extend_from_slice(unsafe { &*al(c.args).list });
    }
    let ap = list.as_ptr();
    if c.class == 0 || c.class == somsup::class_of(c.target) {
        call(somsup::so(), somsup::om::SOM_DISPATCH, &[c.target, ret as usize, c.method as usize, ap as usize]) as u32
    } else {
        call(somsup::so(), somsup::om::SOM_CLASS_DISPATCH, &[c.target, c.class, ret as usize, c.method as usize, ap as usize]) as u32
    }
}

// ---- The externalization functions (_Optlink) ----

/// somutExternalizeObject(obj, buffer, size, m1, m2, m3, &length): the
/// object's external form into the buffer.
pub extern "C" fn somutExternalizeObject(obj: usize, buf: *mut u8, size: u32, m1: *const u8, m2: *const u8, m3: *const u8, len: *mut u32) -> i32 {
    let Some(e) = externalize(obj, or(m1, EXT_NAME), or(m2, EXT_ID), or(m3, EXT_STATE)) else { return -1 };
    if !len.is_null() {
        unsafe { *len = e.len() as u32 };
    }
    put(&e, buf, size)
}

/// somutInternalizeObject(text, &used, ..., m3): the object made again
/// from its external form.
pub extern "C" fn somutInternalizeObject(text: *const u8, used: *mut u32, _a: usize, _m1: *const u8, _m2: *const u8, m3: *const u8) -> usize {
    let s = unsafe { somsup::cstr(text) };
    let mut r = Reader { s, i: 0 };
    let o = internalize(&mut r, or(m3, INT_STATE)).unwrap_or(0);
    if !used.is_null() {
        unsafe { *used = r.i as u32 };
    }
    o
}

/// somutExtCommand(m1, m2, m3, &buffer, &size, target, className,
/// methodName, ap): a call written out (SOMMalloc'd buffer, grown until
/// it fits); the target's class must have m1 (somrGetObjName), as IBM's.
pub extern "C" fn somutExtCommand(m1: *const u8, m2: *const u8, m3: *const u8, out: *mut *mut u8, size: *mut u32, target: usize, cname: *const u8, mname: *const u8, ap: *const u32) -> i32 {
    let (m1, m2, m3) = (or(m1, EXT_NAME), or(m2, EXT_ID), or(m3, EXT_STATE));
    if target == 0 || out.is_null() || size.is_null() || find_method(somsup::class_of(target), m1) == 0 {
        return -1;
    }
    let cmd = somsup::new_object(SOMCommandNewClass(2, 1));
    let cls = if cname.is_null() { 0 } else { somsup::find_class(unsafe { somsup::cstr(cname) }, 0, 0) };
    let mid = somsup::id(unsafe { somsup::cstr(mname) });
    let mut rc = set_class_command(cmd, target, cls, mid, ap);
    if rc == 0 {
        rc = match command_text(cmd, m1, m2, m3) {
            Some(t) => {
                let n = t.len() + 1;
                let b = somsup::malloc(n);
                put(&t, b, n as u32);
                unsafe {
                    *out = b;
                    *size = n as u32;
                }
                0
            }
            None => -1,
        };
    }
    somsup::free_object(cmd);
    rc
}

/// somutIntCommand(m1, m2, m3, target, text, ...): a written-out call run
/// again on target (whose class must have m1, softdrGetObjPtrFromName).
pub extern "C" fn somutIntCommand(m1: *const u8, m2: *const u8, m3: *const u8, target: usize, text: *const u8, x: usize) -> i32 {
    let m1 = or(m1, INT_NAME);
    if target == 0 || find_method(somsup::class_of(target), m1) == 0 {
        return -1;
    }
    let cmd = somsup::new_object(SOMCommandNewClass(2, 1));
    let rc = set_ext_rep(cmd, text, x, target, m1, or(m2, INT_ID), or(m3, INT_STATE));
    if rc == 0 {
        let mut ret = [0u8; 8];
        do_command(cmd, ret.as_mut_ptr());
    }
    somsup::free_object(cmd);
    rc
}

somsup::optlink! {
    __o_somutExternalizeObject => somutExternalizeObject, 7;
    __o_somutInternalizeObject => somutInternalizeObject, 6;
    __o_somutExtCommand => somutExtCommand, 9;
    __o_somutIntCommand => somutIntCommand, 6;
}

/// somExternalizeCommandInit(m1, m2, m3, &buffer, &size, target,
/// className, methodName, ...): somutExtCommand with the call's arguments
/// following.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_somExternalizeCommandInit() {
    core::arch::naked_asm!(
        "lea eax, [esp + 36]",
        "push eax",
        "push dword ptr [esp + 36]",
        "push dword ptr [esp + 36]",
        "push dword ptr [esp + 36]",
        "push dword ptr [esp + 36]",
        "push dword ptr [esp + 36]",
        "push dword ptr [esp + 36]",
        "push dword ptr [esp + 36]",
        "push dword ptr [esp + 36]",
        "call {f}",
        "add esp, 36",
        "ret",
        f = sym __a_ext_command,
    );
}

somsup::aligned! {
    __a_ext_command => somutExtCommand, 9;
}

somsup::methods! {
    __m_get_op_modifier => get_op_modifier, 2;
    __m_result_owner => result_owner, 1;
    __m_arg_owner => arg_owner, 2;
    __m_arg_index => arg_index, 3;
    __m_flush => flush, 1;
    __m_get_args => get_args, 6;
    __m_set_args => set_args, 6;
    __m_set_command => set_command, 4;
    __m_set_class_command => set_class_command, 5;
    __m_get_ext_rep => get_ext_rep, 6;
    __m_set_ext_rep => set_ext_rep, 7;
    __m_do_command => do_command, 2;
}

/// The varargs forms (somva_SOMArgList_somutSetArgs takes the rest of
/// its arguments as the call's: here they are passed on as they are).
#[unsafe(naked)]
pub extern "C" fn __v_set_args() {
    core::arch::naked_asm!("jmp {f}", f = sym __m_set_args);
}

/// somva_SOMCommand_somutSetClassCommand(target, class, methodId, ...).
#[unsafe(naked)]
pub extern "C" fn __v_set_command() {
    core::arch::naked_asm!(
        "lea eax, [esp + 20]",
        "push eax",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "call {f}",
        "add esp, 20",
        "ret",
        f = sym __m_set_class_command,
    );
}
