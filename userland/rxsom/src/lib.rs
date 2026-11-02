//! Calling SOM methods by name with REXX values, for Object REXX's SOM
//! support (REXXSOM.DLL) and its Workplace Shell server (REXXWPS.DLL).
//!
//! A message name from REXX (upper case) is matched without regard to case
//! against the methods the object's class supports (somGetNthMethodInfo);
//! the method is resolved by name (somResolveByName) and called with
//! `_System` linkage. What its arguments and result are comes from a
//! signature: SOM's own classes', the interface repository's and the
//! Workplace Shell's are in tables here and in the callers'.
//!
//! A signature is `r(aaa)`: the result type, then the argument types:
//! `v` void, `l` long, `L` unsigned long, `h` short, `H` unsigned short,
//! `b` boolean (octet), `c` char, `d` double, `f` float, `s` string, `o`
//! an object, `i` a somId (from a string), `p` a pointer or handle (a
//! number), `e` the CORBA Environment (passed for IDL methods, not by
//! REXX), and `*x` a pointer to an `x` (INOUT and OUT arguments: a DLF
//! object holds the value).

#![no_std]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::vec::Vec;

use somsup::{Environment, SomId, cm, eq_nocase, sc_call, so_call};

unsafe extern "C" {
    #[link_name = "SOM.61"]
    fn somResolveByName(obj: usize, name: *const u8) -> usize;
}

/// A value going to or coming from SOM.
#[derive(Clone, Debug)]
pub enum Val {
    None,
    Str(Vec<u8>),
    Obj(usize),
    Class(usize),
    Nil,
}

/// Why a message failed.
pub enum Error {
    /// The object has no such method.
    NoMethod,
    /// A REXX error: code (major * 1000 + minor) and secondary message.
    Rexx(u32, Vec<u8>),
}

/// SOMObject's, SOMClass's, SOMClassMgr's and the interface repository's
/// methods.
pub const SOM_SIGS: &[(&str, &str)] = &[
    ("somInit", "v()"),
    ("somUninit", "v()"),
    ("somFree", "v()"),
    ("somGetClassName", "s()"),
    ("somGetClass", "o()"),
    ("somIsA", "b(o)"),
    ("somRespondsTo", "b(i)"),
    ("somIsInstanceOf", "b(o)"),
    ("somGetSize", "l()"),
    ("somDumpSelf", "v(l)"),
    ("somDumpSelfInt", "v(l)"),
    ("somPrintSelf", "o()"),
    ("somDefaultInit", "v(p)"),
    ("somDestruct", "v(b,p)"),
    ("somNew", "o()"),
    ("somNewNoInit", "o()"),
    ("somRenew", "o(p)"),
    ("somRenewNoInit", "o(p)"),
    ("somRenewNoZero", "o(p)"),
    ("somRenewNoInitNoZero", "o(p)"),
    ("somGetName", "s()"),
    ("somGetParent", "o()"),
    ("somGetParents", "p()"),
    ("somDescendedFrom", "b(o)"),
    ("somCheckVersion", "b(l,l)"),
    ("somFindMethod", "b(i,p)"),
    ("somFindMethodOk", "b(i,p)"),
    ("somFindSMethod", "p(i)"),
    ("somFindSMethodOk", "p(i)"),
    ("somGetInstanceSize", "l()"),
    ("somGetInstancePartSize", "l()"),
    ("somGetInstanceToken", "p()"),
    ("somGetNumMethods", "l()"),
    ("somGetNumStaticMethods", "l()"),
    ("somGetVersionNumbers", "v(*l,*l)"),
    ("somGetMethodDescriptor", "i(i)"),
    ("somGetMethodIndex", "l(i)"),
    ("somGetMethodToken", "p(i)"),
    ("somGetNthMethodInfo", "i(l,p)"),
    ("somSupportsMethod", "b(i)"),
    ("somLookupMethod", "p(i)"),
    ("somGetRdStub", "p(i)"),
    ("somGetClassMtab", "p()"),
    ("somGetClassData", "p()"),
    ("somAllocate", "p(l)"),
    ("somDeallocate", "v(p)"),
    ("_get_somDataAlignment", "l()"),
    ("somFindClass", "o(i,l,l)"),
    ("somFindClsInFile", "o(i,l,l,s)"),
    ("somClassFromId", "o(i)"),
    ("somLocateClassFile", "s(i,l,l)"),
    ("somLoadClassFile", "o(i,l,l,s)"),
    ("somRegisterClass", "v(o)"),
    ("somUnregisterClass", "l(o)"),
    ("somUnloadClassFile", "l(o)"),
    ("somMergeInto", "v(o)"),
    ("somGetInitFunction", "s()"),
    ("somGetRelatedClasses", "p(o)"),
    ("somSubstituteClass", "l(s,s)"),
    ("_get_somInterfaceRepository", "o()"),
    ("_set_somInterfaceRepository", "v(o)"),
    ("_get_somRegisteredClasses", "p()"),
    // the interface repository (IDL methods)
    ("lookup_id", "o(e,s)"),
    ("lookup_modifier", "s(e,s,s)"),
    ("release_cache", "v(e)"),
    ("_get_name", "s(e)"),
    ("_set_name", "v(e,s)"),
    ("_get_id", "s(e)"),
    ("_set_id", "v(e,s)"),
    ("_get_defined_in", "s(e)"),
    ("_set_defined_in", "v(e,s)"),
    ("within", "p(e)"),
    ("describe", "p(e)"),
    ("contents", "p(e,s,b)"),
    ("lookup_name", "p(e,s,l,s,b)"),
    ("describe_contents", "p(e,s,b,l)"),
    ("_get_instanceData", "p(e)"),
    ("_get_result", "p(e)"),
    ("_get_mode", "l(e)"),
    ("_get_type", "p(e)"),
];

fn sig_of<'a>(name: &[u8], tables: &[&'a [(&'a str, &'a str)]]) -> Option<&'a str> {
    for t in tables {
        if let Some((_, s)) = t.iter().find(|(n, _)| n.as_bytes() == name) {
            return Some(s);
        }
    }
    None
}

/// The types in a signature: (result, arguments), each type as `x` or
/// `*x`.
fn parse_sig(s: &str) -> (Vec<u8>, Vec<Vec<u8>>) {
    let b = s.as_bytes();
    let open = b.iter().position(|&c| c == b'(').unwrap_or(b.len());
    let ret = b[..open].to_vec();
    let inner = &b[(open + 1).min(b.len())..b.len().saturating_sub(1).max(open + 1).min(b.len())];
    let args = if inner.is_empty() { Vec::new() } else { inner.split(|&c| c == b',').map(|x| x.to_vec()).collect() };
    (ret, args)
}

pub fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn leak_z(s: &[u8]) -> usize {
    alloc::boxed::Box::leak(z(s).into_boxed_slice()).as_ptr() as usize
}

/// A somId kept for good.
pub fn id(name: &[u8]) -> SomId {
    somsup::id(name)
}

pub fn class_of(obj: usize) -> usize {
    somsup::class_of(obj)
}

/// An object's class name.
pub fn class_name_of(obj: usize) -> Vec<u8> {
    let p = so_call(somsup::om::SOM_GET_CLASS_NAME, &[obj]) as *const u8;
    if p.is_null() { Vec::new() } else { unsafe { somsup::cstr(p) }.to_vec() }
}

/// A class's name.
pub fn class_id(cls: usize) -> Vec<u8> {
    let p = somsup::class_name(cls);
    if p.is_null() { Vec::new() } else { unsafe { somsup::cstr(p) }.to_vec() }
}

/// The method of a class whose name matches `upper` (any case).
pub fn method_name(cls: usize, upper: &[u8]) -> Option<Vec<u8>> {
    let n = sc_call(cm::SOM_GET_NUM_METHODS, &[cls]) as i32;
    let mut desc: SomId = core::ptr::null_mut();
    for i in 0..n.max(0) {
        let mid = sc_call(cm::SOM_GET_NTH_METHOD_INFO, &[cls, i as usize, &mut desc as *mut SomId as usize]) as SomId;
        if mid.is_null() {
            continue;
        }
        let s = unsafe { somsup::somStringFromId(mid) };
        if s.is_null() {
            continue;
        }
        let name = unsafe { somsup::cstr(s) };
        if eq_nocase(name, upper) {
            return Some(name.to_vec());
        }
    }
    None
}

fn number(s: &[u8]) -> Option<f64> {
    let t = core::str::from_utf8(s).ok()?.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

fn whole(s: &[u8]) -> Option<i64> {
    let f = number(s)?;
    if f != (f as i64) as f64 {
        return None;
    }
    Some(f as i64)
}

/// A double as REXX shows numbers.
pub fn fmt_double(d: f64) -> Vec<u8> {
    if d.abs() < 1e15 && d == (d as i64) as f64 {
        return alloc::format!("{}", d as i64).into_bytes();
    }
    alloc::format!("{d}").into_bytes()
}

fn bad_arg(n: usize, ty: &[u8]) -> Error {
    let t = core::str::from_utf8(ty).unwrap_or("?");
    Error::Rexx(98932, alloc::format!("SOM methods cannot take arguments of the type \"{t}\" (argument {})", n + 1).into_bytes())
}

/// Call `name` (the method's real name) on `obj` with the signature
/// `sig`. `dlf`: the address of a DLF object's value (INOUT and OUT
/// arguments), if the object is one.
pub fn call(obj: usize, name: &[u8], sig: &str, args: &[Val], dlf: &dyn Fn(usize) -> Option<usize>) -> Result<Val, Error> {
    let (ret, types) = parse_sig(sig);
    let nz = z(name);
    let f = unsafe { somResolveByName(obj, nz.as_ptr()) };
    if f == 0 {
        return Err(Error::NoMethod);
    }
    let mut ev = Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 };
    let mut words: Vec<usize> = alloc::vec![obj];
    let mut ai = 0;
    for t in &types {
        if t.as_slice() == b"e" {
            words.push(&mut ev as *mut Environment as usize);
            continue;
        }
        let a = args.get(ai).cloned().unwrap_or(Val::None);
        let n = ai;
        ai += 1;
        if t.first() == Some(&b'*') {
            match a {
                Val::Obj(o) => match dlf(o) {
                    Some(p) => words.push(p),
                    None => words.push(o),
                },
                Val::None | Val::Nil => words.push(0),
                Val::Str(s) => {
                    // a plain value for an INOUT argument: a copy
                    let cell = alloc::boxed::Box::leak(alloc::boxed::Box::new([0usize; 2]));
                    if let Some(v) = whole(&s) {
                        cell[0] = v as usize;
                    }
                    words.push(cell.as_ptr() as usize);
                }
                Val::Class(c) => words.push(c),
            }
            continue;
        }
        let ty = t.first().copied().unwrap_or(b'p');
        match (ty, a) {
            (b'o', Val::Obj(o)) | (b'o', Val::Class(o)) => words.push(o),
            (b'o', Val::None) | (b'o', Val::Nil) => words.push(0),
            (b'o', Val::Str(_)) => return Err(bad_arg(n, b"object")),
            (b's', Val::Str(s)) => words.push(leak_z(&s)),
            (b's', Val::None) | (b's', Val::Nil) => words.push(0),
            (b'i', Val::Str(s)) => words.push(id(&s) as usize),
            (b'd', Val::Str(s)) => {
                let d = number(&s).ok_or_else(|| Error::Rexx(98902, alloc::format!("\"{}\" cannot be made a double-float value", core::str::from_utf8(&s).unwrap_or("")).into_bytes()))?;
                let bits = d.to_bits();
                words.push(bits as u32 as usize);
                words.push((bits >> 32) as u32 as usize);
            }
            (b'f', Val::Str(s)) => {
                let d = number(&s).ok_or_else(|| bad_arg(n, b"float"))? as f32;
                words.push(d.to_bits() as usize);
            }
            (b'l' | b'L' | b'h' | b'H' | b'c' | b'p', Val::Str(s)) => {
                let v = if ty == b'c' && s.len() == 1 && !s[0].is_ascii_digit() { s[0] as i64 } else { whole(&s).ok_or_else(|| bad_arg(n, b"long"))? };
                words.push(v as usize);
            }
            (b'b', Val::Str(s)) => {
                let v = match s.as_slice() {
                    b"1" => 1,
                    b"0" | b"" => 0,
                    _ => whole(&s).ok_or_else(|| bad_arg(n, b"boolean"))? as usize,
                };
                words.push(v);
            }
            (_, Val::None) | (_, Val::Nil) => words.push(0),
            (b'p' | b'l' | b'L', Val::Obj(o)) | (b'p' | b'l' | b'L', Val::Class(o)) => words.push(o),
            (_, _) => return Err(bad_arg(n, t)),
        }
    }
    if words.len() > 12 {
        return Err(Error::Rexx(98934, b"Too many arguments for a SOM method".to_vec()));
    }
    let r = match ret.first().copied().unwrap_or(b'v') {
        b'd' | b'f' => {
            let d = call_f64(f, &words);
            Val::Str(fmt_double(d))
        }
        rt => {
            let v = somsup::call_fn(f, &words);
            match rt {
                b'v' => Val::None,
                b's' => {
                    if v == 0 {
                        Val::Nil
                    } else {
                        Val::Str(unsafe { somsup::cstr(v as *const u8) }.to_vec())
                    }
                }
                b'o' => {
                    if v == 0 {
                        Val::Nil
                    } else {
                        Val::Obj(v)
                    }
                }
                b'b' => Val::Str(if v & 0xff != 0 { b"1".to_vec() } else { b"0".to_vec() }),
                b'c' => Val::Str(alloc::vec![v as u8]),
                b'h' => Val::Str(alloc::format!("{}", v as u16 as i16).into_bytes()),
                b'H' => Val::Str(alloc::format!("{}", v as u16).into_bytes()),
                b'l' => Val::Str(alloc::format!("{}", v as u32 as i32).into_bytes()),
                b'i' => {
                    let s = if v == 0 { core::ptr::null() } else { unsafe { somsup::somStringFromId(v as SomId) } };
                    if s.is_null() { Val::Nil } else { Val::Str(unsafe { somsup::cstr(s) }.to_vec()) }
                }
                _ => Val::Str(alloc::format!("{}", v as u32).into_bytes()),
            }
        }
    };
    if ev.major != 0 {
        let id = unsafe { somsup::somExceptionId(&mut ev) };
        let s = if id.is_null() { b"SOM exception".to_vec() } else { unsafe { somsup::cstr(id) }.to_vec() };
        unsafe { somsup::somExceptionFree(&mut ev) };
        let mut m = b"The SOM method raised the exception ".to_vec();
        m.extend_from_slice(&s);
        return Err(Error::Rexx(98904, m));
    }
    Ok(r)
}

/// Call a function returning a double (or float) in ST0.
fn call_f64(f: usize, a: &[usize]) -> f64 {
    let mut w = [0usize; 12];
    w[..a.len()].copy_from_slice(a);
    type F = extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize) -> f64;
    let g: F = unsafe { core::mem::transmute(f) };
    g(w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7], w[8], w[9], w[10], w[11])
}

/// Send a message (REXX's upper-case name) to a SOM object (or class
/// object), the signature found in `tables`.
pub fn send(obj: usize, msg: &[u8], args: &[Val], tables: &[&[(&str, &str)]], dlf: &dyn Fn(usize) -> Option<usize>) -> Result<Val, Error> {
    if obj == 0 || unsafe { somsup::somIsObj(obj) } == 0 {
        return Err(Error::Rexx(98901, b"The SOM object is no longer there".to_vec()));
    }
    let cls = class_of(obj);
    let Some(name) = method_name(cls, msg) else { return Err(Error::NoMethod) };
    // "Class::method" entries first (methods of the same name differ)
    let cname = class_name_of(obj);
    let mut qual = cname.clone();
    qual.extend_from_slice(b"::");
    qual.extend_from_slice(&name);
    let Some(sig) = sig_of(&qual, tables).or_else(|| sig_of(&name, tables)) else {
        let mut m = b"There is no description of the method \"".to_vec();
        m.extend_from_slice(&name);
        m.extend_from_slice(b"\" of the class \"");
        m.extend_from_slice(&cname);
        m.push(b'"');
        return Err(Error::Rexx(98931, m));
    };
    call(obj, &name, sig, args, dlf)
}

/// A SOM class by name (the class manager loads its DLL if need be).
pub fn find_class(name: &[u8]) -> usize {
    if name.is_empty() {
        return 0;
    }
    unsafe {
        if somsup::SOMClassMgrObject == 0 {
            somsup::somEnvironmentNew();
        }
    }
    let mgr = unsafe { somsup::SOMClassMgrObject };
    let cid = id(name);
    let c = somsup::call(somsup::scm(), somsup::mm::SOM_CLASS_FROM_ID, &[mgr, cid as usize]);
    if c != 0 {
        return c;
    }
    somsup::call(somsup::scm(), somsup::mm::SOM_FIND_CLASS, &[mgr, cid as usize, 0, 0])
}
