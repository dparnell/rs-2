//! OREXX.DLL's interface for programs that hold REXX objects: the OSA
//! support calls IBM's OpenDoc scripting component (OREXXSC.DLL) compiles
//! and runs scripts with (RexxCreateMethod, RexxRunMethod, ... ordinals
//! 92-104, RexxSomInitialize 18, RexxTerminate 16) and the native-method
//! API (REXX_SEND, REXX_STRING_NEW, ... ordinals 30-80).
//!
//! The objects live in one Object REXX world per process, made by
//! RexxSomInitialize (or the first call). A REXXOBJECT is a handle into
//! a table of rs2's (never a pointer into the interpreter):
//!
//! - script objects (RexxCreateMethod, RexxLoadMethod, RexxCopyMethod)
//!   hold the script's source and its OSA information, and belong to a
//!   directory by name (RexxCreateDirectory; IBM's component uses
//!   "OPENDOC");
//! - result objects (RexxRunMethod) hold the value a script returned and
//!   its string;
//! - the native-method API makes objects as it is asked to.
//!
//! RexxRunMethod runs the script as an Object REXX program (its
//! directives included) in the process's world; errors fill the error
//! information (9 words): the line (word 0), the error number * 1000 +
//! subcode (1), the message (3) and the secondary message (5), which
//! OSAScriptError gives as 'errn', 'errs' and 'errb'.
//! RexxStoreMethod gives REXXC's form of the script (`/**/` and the
//! source, scrambled), RexxLoadMethod takes that or plain source.

use alloc::rc::Rc;
use alloc::vec::Vec;

use os2::*;
use rexx::oo::World;
use rexx::value::Value;
use rexx::{CallType, Interp};

use crate::RxString;

#[derive(Default)]
struct Entry {
    /// The object (scripts have none: their source is in `src`).
    v: Option<Value>,
    /// Its string, NUL-terminated, once asked for (REXX_STRING_DATA,
    /// RexxResultString).
    s: Option<Vec<u8>>,
    /// A script's source.
    src: Option<Vec<u8>>,
    /// A script's OSA information (RexxSetOSAInfo).
    info: Vec<(u32, u32)>,
    /// The directory a script or result belongs to.
    dir: Vec<u8>,
}

struct Session {
    w: Rc<World>,
    objs: Vec<Option<Entry>>,
    dirs: Vec<Vec<u8>>,
}

static mut SESSION: Option<Session> = None;

fn session() -> &'static mut Session {
    let s = unsafe { &mut *(&raw mut SESSION) };
    if s.is_none() {
        let w = crate::stack::deep(World::new);
        *s = Some(Session { w, objs: Vec::new(), dirs: Vec::new() });
    }
    s.as_mut().unwrap()
}

const BASE: u32 = 0x5258_0000;

fn handle(e: Entry) -> u32 {
    let s = session();
    let i = match s.objs.iter().position(|x| x.is_none()) {
        Some(i) => {
            s.objs[i] = Some(e);
            i
        }
        None => {
            s.objs.push(Some(e));
            s.objs.len() - 1
        }
    };
    BASE + (i as u32) * 4
}

fn entry(h: u32) -> Option<&'static mut Entry> {
    if h < BASE || (h - BASE) % 4 != 0 {
        return None;
    }
    session().objs.get_mut(((h - BASE) / 4) as usize)?.as_mut()
}

fn obj(v: Value) -> u32 {
    handle(Entry { v: Some(v), ..Entry::default() })
}

fn value(h: u32) -> Option<Value> {
    entry(h).and_then(|e| e.v.clone())
}

fn bytes(p: *const u8, n: usize) -> Vec<u8> {
    if p.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(p, n) }.to_vec() }
}

fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    bytes(p, n)
}

/// Text in memory from DosAllocMem (the caller frees it with DosFreeMem).
fn rx_out(out: *mut RxString, s: &[u8]) -> u32 {
    if out.is_null() {
        return 1;
    }
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { DosAllocMem(&mut p, (s.len() + 1).max(1) as u32, 0x13) } != 0 {
        return 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
        (*out).len = s.len() as u32;
        (*out).ptr = p;
    }
    0
}

/// A value's string without running REXX code.
fn fast(v: &Value) -> Vec<u8> {
    v.fast_string()
}

fn whole(v: &Value) -> Option<i64> {
    let s = fast(v);
    let t = core::str::from_utf8(&s).ok()?.trim();
    t.parse::<i64>().ok().or_else(|| t.parse::<f64>().ok().filter(|&f| f == (f as i64) as f64).map(|f| f as i64))
}

// ---- the interpreter for a run ----

fn with_interp<R>(f: impl FnOnce(&mut Interp) -> R) -> R {
    let w = session().w.clone();
    crate::stack::deep(move || {
        let mut h = crate::host::OsHost { exits: Default::default(), locals: Vec::new(), tid: crate::stack::tid() };
        crate::oohost::lock();
        let r = {
            let mut it = Interp::new_activity(&mut h, w, 1);
            f(&mut it)
        };
        crate::oohost::unlock();
        r
    })
}

fn error_info(err: *mut u32, code: u32, line: u32) {
    if err.is_null() {
        return;
    }
    let (major, minor) = if code >= 1000 { (code / 1000, code % 1000) } else { (code, 0) };
    let leak = |v: Option<Vec<u8>>| -> u32 {
        let mut v = v.unwrap_or_default();
        v.push(0);
        alloc::boxed::Box::leak(v.into_boxed_slice()).as_ptr() as u32
    };
    let msg = leak(crate::host::message_text(major));
    let sec = if minor != 0 { leak(rexx::msgs::number(major * 1000 + minor).and_then(crate::host::message_text)) } else { leak(None) };
    // IBM's layout (36 bytes; OREXXSC copies words 0, 1, 3, 5, 6 and 8)
    let info = [line, major * 1000 + minor, 0, msg, 0, sec, 0, 0, 0];
    unsafe { core::ptr::copy_nonoverlapping(info.as_ptr(), err, 9) };
}

// ---- the OSA support calls ----

pub extern "C" fn RexxTerminate() -> u32 {
    0
}

pub extern "C" fn RexxSomInitialize() -> u32 {
    session();
    0
}

pub extern "C" fn RexxCreateDirectory(name: *const u8) -> u32 {
    let n = cstr(name);
    let s = session();
    if !s.dirs.contains(&n) {
        s.dirs.push(n);
    }
    0
}

pub extern "C" fn RexxRemoveDirectory(name: *const u8) -> u32 {
    let n = cstr(name);
    let s = session();
    s.dirs.retain(|d| *d != n);
    for e in s.objs.iter_mut() {
        if e.as_ref().is_some_and(|x| x.dir == n) {
            *e = None;
        }
    }
    0
}

fn script(dir: *const u8, src: Vec<u8>) -> u32 {
    handle(Entry { src: Some(src), dir: cstr(dir), ..Entry::default() })
}

/// RexxCreateMethod(dir, source, *method, *error information).
pub extern "C" fn RexxCreateMethod(dir: *const u8, src: *const RxString, out: *mut u32, err: *mut u32) -> u32 {
    if src.is_null() || out.is_null() {
        return 1;
    }
    let s = unsafe { bytes((*src).ptr, (*src).len as usize) };
    let s: Vec<u8> = s.into_iter().take_while(|&c| c != 0).collect();
    match crate::stack::deep(|| rexx::embed::check_program(&s, b"OSA")) {
        Ok(()) => {
            unsafe { *out = script(dir, s) };
            0
        }
        Err((code, line)) => {
            error_info(err, code, line);
            code.max(1)
        }
    }
}

/// RexxRunMethod(dir, method, *result, *error information).
pub extern "C" fn RexxRunMethod(dir: *const u8, method: u32, out: *mut u32, err: *mut u32) -> u32 {
    let Some(src) = entry(method).and_then(|e| e.src.clone()) else { return 1 };
    let r = with_interp(|it| {
        let r = it.run_program_v(&src, b"OSA", CallType::Subroutine, Vec::new(), b"CMD");
        r.map(|v| v.map(|v| {
            let s = it.fast_str(&v);
            (v, s)
        }))
    });
    match r {
        Ok(v) => {
            let (v, s) = v.unwrap_or((Value::S(Vec::new()), Vec::new()));
            let mut s = s;
            s.push(0);
            let h = handle(Entry { v: Some(v), s: Some(s), dir: cstr(dir), ..Entry::default() });
            if !out.is_null() {
                unsafe { *out = h };
            }
            0
        }
        Err(e) => {
            let code = (if e < -0x0fff_ffff { -(e + 0x1000_0000) } else { -e }) as u32;
            error_info(err, code, 0);
            code.max(1)
        }
    }
}

/// RexxResultString(object, *text): the object's string.
pub extern "C" fn RexxResultString(h: u32, out: *mut RxString) -> u32 {
    let Some(e) = entry(h) else { return 1 };
    let s = match (&e.s, &e.v, &e.src) {
        (Some(s), _, _) => s[..s.len().saturating_sub(1)].to_vec(),
        (None, Some(v), _) => fast(v),
        (None, None, Some(src)) => src.clone(),
        _ => Vec::new(),
    };
    rx_out(out, &s)
}

pub extern "C" fn RexxGetSource(h: u32, out: *mut RxString) -> u32 {
    match entry(h).and_then(|e| e.src.clone()) {
        Some(s) => rx_out(out, &s),
        None => 1,
    }
}

pub extern "C" fn RexxStoreMethod(h: u32, out: *mut RxString) -> u32 {
    match entry(h).and_then(|e| e.src.clone()) {
        Some(s) => rx_out(out, &crate::oohost::tokenize(&s)),
        None => 1,
    }
}

pub extern "C" fn RexxLoadMethod(dir: *const u8, data: *const RxString, out: *mut u32) -> u32 {
    if data.is_null() || out.is_null() {
        return 1;
    }
    let d = unsafe { bytes((*data).ptr, (*data).len as usize) };
    let src = crate::oohost::untokenize(&d).unwrap_or(d);
    if crate::stack::deep(|| rexx::embed::check_program(&src, b"OSA")).is_err() {
        return 1;
    }
    unsafe { *out = script(dir, src) };
    0
}

pub extern "C" fn RexxCopyMethod(dir: *const u8, h: u32, out: *mut u32) -> u32 {
    let Some(e) = entry(h) else { return 1 };
    let n = Entry { v: e.v.clone(), s: e.s.clone(), src: e.src.clone(), info: e.info.clone(), dir: cstr(dir) };
    if !out.is_null() {
        unsafe { *out = handle(n) };
    }
    0
}

/// RexxDispose(dir, object): TRUE when it was one.
pub extern "C" fn RexxDispose(_dir: *const u8, h: u32) -> u32 {
    if entry(h).is_none() {
        return 0;
    }
    session().objs[((h - BASE) / 4) as usize] = None;
    1
}

/// RexxValidObject(dir, object): TRUE for an object of the directory.
pub extern "C" fn RexxValidObject(dir: *const u8, h: u32) -> u32 {
    let d = cstr(dir);
    entry(h).is_some_and(|e| e.dir == d || e.dir.is_empty()) as u32
}

pub extern "C" fn RexxSetOSAInfo(h: u32, sel: u32, val: u32) -> u32 {
    let Some(e) = entry(h) else { return 1 };
    match e.info.iter_mut().find(|x| x.0 == sel) {
        Some(x) => x.1 = val,
        None => e.info.push((sel, val)),
    }
    0
}

pub extern "C" fn RexxGetOSAInfo(h: u32, sel: u32, out: *mut u32) -> u32 {
    let Some(e) = entry(h) else { return 1 };
    let v = e.info.iter().find(|x| x.0 == sel).map_or(0, |x| x.1);
    if !out.is_null() {
        unsafe { *out = v };
    }
    0
}

// ---- the native-method API ----

fn w() -> Rc<World> {
    session().w.clone()
}

fn opt(h: u32) -> Option<Value> {
    if h == 0 { None } else { value(h) }
}

pub extern "C" fn REXX_ARRAY_NEW(n: u32) -> u32 {
    obj(w().array_opt(alloc::vec![None; n as usize]))
}

pub extern "C" fn REXX_ARRAY_NEW1(a: u32) -> u32 {
    obj(w().array_opt(alloc::vec![opt(a)]))
}

pub extern "C" fn REXX_ARRAY_NEW2(a: u32, b: u32) -> u32 {
    obj(w().array_opt(alloc::vec![opt(a), opt(b)]))
}

pub extern "C" fn REXX_ARRAY_PUT(arr: u32, item: u32, index: u32) -> u32 {
    if let Some(a) = value(arr) {
        World::array_put(&a, opt(item), index as usize);
    }
    0
}

pub extern "C" fn REXX_ARRAY_AT(arr: u32, index: u32) -> u32 {
    let it = value(arr).and_then(|a| World::array_items(&a)).and_then(|v| v.get((index as usize).wrapping_sub(1)).cloned().flatten());
    it.map_or(0, obj)
}

pub extern "C" fn REXX_ARRAY_SIZE(arr: u32) -> u32 {
    value(arr).and_then(|a| World::array_items(&a)).map_or(0, |v| v.len() as u32)
}

pub extern "C" fn REXX_BUFFER_NEW(n: u32) -> u32 {
    obj(Value::S(alloc::vec![0; n as usize]))
}

pub extern "C" fn REXX_STRING_NEW(p: *const u8, n: u32) -> u32 {
    obj(Value::S(bytes(p, n as usize)))
}

/// The object's string, NUL-terminated, while the object lives.
pub extern "C" fn REXX_STRING_DATA(h: u32) -> *const u8 {
    let Some(e) = entry(h) else { return b"\0".as_ptr() };
    if e.s.is_none() {
        let mut s = e.v.as_ref().map(fast).unwrap_or_default();
        s.push(0);
        e.s = Some(s);
    }
    e.s.as_ref().unwrap().as_ptr()
}

pub extern "C" fn REXX_INTEGER_NEW(v: i32) -> u32 {
    obj(Value::S(alloc::format!("{v}").into_bytes()))
}

pub extern "C" fn REXX_INTEGER_VALUE(h: u32) -> i32 {
    value(h).and_then(|v| whole(&v)).unwrap_or(0) as i32
}

pub extern "C" fn REXX_UNSIGNED_INTEGER(h: u32) -> u32 {
    value(h).and_then(|v| whole(&v)).unwrap_or(0) as u32
}

pub extern "C" fn REXX_INTEGER(h: u32) -> u32 {
    match value(h).and_then(|v| whole(&v)) {
        Some(n) => REXX_INTEGER_NEW(n as i32),
        None => 0,
    }
}

pub extern "C" fn REXX_DOUBLE(h: u32) -> f64 {
    value(h).and_then(|v| core::str::from_utf8(&fast(&v)).ok().and_then(|s| s.trim().parse::<f64>().ok())).unwrap_or(0.0)
}

pub extern "C" fn REXX_ISINTEGER(h: u32) -> u32 {
    value(h).is_some_and(|v| World::is_string(&v) && whole(&v).is_some()) as u32
}

pub extern "C" fn REXX_ISASTRING(h: u32) -> u32 {
    value(h).is_some_and(|v| World::is_string(&v)) as u32
}

pub extern "C" fn REXX_ISDIRECTORY(h: u32) -> u32 {
    value(h).is_some_and(|v| World::is_directory(&v)) as u32
}

pub extern "C" fn REXX_NIL() -> u32 {
    obj(w().nil_value())
}

pub extern "C" fn REXX_TRUE() -> u32 {
    obj(Value::S(b"1".to_vec()))
}

pub extern "C" fn REXX_ENVIRONMENT() -> u32 {
    obj(w().environment())
}

pub extern "C" fn REXX_LOCAL() -> u32 {
    obj(w().local_dir())
}

/// The error a native method raised (REXX_EXCEPT), for its caller.
pub static mut RAISED: Option<(u32, Option<Value>)> = None;

pub extern "C" fn REXX_EXCEPT(code: u32, args: u32) -> u32 {
    unsafe { RAISED = Some((code, opt(args))) };
    0
}

/// REXX_SEND(receiver, message, arguments array): the result (0 for
/// none).
pub extern "C" fn REXX_SEND(recv: u32, msg: *const u8, args: u32) -> u32 {
    let Some(r) = value(recv) else { return 0 };
    let name = cstr(msg);
    let a = opt(args).and_then(|a| World::array_items(&a)).unwrap_or_default();
    match with_interp(|it| it.send_value(&r, &name, a)) {
        Ok(Some(v)) => obj(v),
        Ok(None) => 0,
        Err(code) => {
            unsafe { RAISED = Some((code, None)) };
            0
        }
    }
}
