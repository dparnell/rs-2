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

/// The handle of an object: the same one for the same object (and for
/// equal strings), as native code compares REXXOBJECTs (with REXX_NIL(),
/// REXX_TRUE()...).
fn obj(v: Value) -> u32 {
    let s = session();
    for (i, e) in s.objs.iter().enumerate() {
        if let Some(Entry { v: Some(x), src: None, .. }) = e {
            if x.same(&v) {
                return BASE + (i as u32) * 4;
            }
        }
    }
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
    let w = w();
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
    let fresh = unsafe { (*(&raw const SESSION)).is_none() };
    session();
    if fresh || !unsafe { CLASSES_IN } {
        unsafe { CLASSES_IN = true };
        install_classes();
    }
    0
}

static mut CLASSES_IN: bool = false;

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

/// The world of the native method running now (its program's), else
/// the OSA session's.
static mut CURRENT: Option<Rc<World>> = None;

fn w() -> Rc<World> {
    match unsafe { (*(&raw const CURRENT)).clone() } {
        Some(w) => w,
        None => session().w.clone(),
    }
}

/// `.ScriptableApp` and the other scripting classes in a program that
/// did not get them from RexxSomInitialize (a REXX program run by CMD):
/// rs2's REXX code for them.
pub fn environment_source(name: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    const NAMES: &[&[u8]] = &[b"SCRIPTABLEAPP", b"OBJECTSPECIFIER", b"SCREFERENCE", b"SCRELATIVEPOSITION", b"SCLIST", b"SCRANGE", b"SCCLASS"];
    if NAMES.contains(&name) { Some((OPENDOC_CLASSES.to_vec(), b"OPENDOC".to_vec())) } else { None }
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

/// REXX_INTEGER(object): its value as a long (OREXXSC puts it straight
/// into an AEDesc).
pub extern "C" fn REXX_INTEGER(h: u32) -> i32 {
    value(h).and_then(|v| whole(&v)).unwrap_or(0) as i32
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

// ---- native methods ----

/// The routine of `EXTERNAL 'LIBRARY dll [entry]'` (the entry defaults to
/// the method's name).
fn routine(spec: &[u8], name: &[u8]) -> Option<usize> {
    let words: Vec<&[u8]> = spec.split(|&c| c == b' ').filter(|w| !w.is_empty()).collect();
    if words.len() < 2 || !words[0].eq_ignore_ascii_case(b"LIBRARY") {
        return None;
    }
    let mut dll = words[1].to_vec();
    dll.push(0);
    let mut entry = words.get(2).map_or(name.to_vec(), |e| e.to_vec());
    entry.push(0);
    let mut h = 0u32;
    let mut fail = [0u8; 64];
    if unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, dll.as_ptr(), &mut h) } != 0 {
        // REXXSC is the module of OREXXSC.DLL
        if !words[1].eq_ignore_ascii_case(b"REXXSC") || unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, b"OREXXSC\0".as_ptr(), &mut h) } != 0 {
            return None;
        }
    }
    let mut f = 0usize;
    if unsafe { DosQueryProcAddr(h, 0, entry.as_ptr(), &mut f) } != 0 || f == 0 {
        return None;
    }
    Some(f)
}

/// Call a native method: `f(NULL)` gives its signature (the result's type,
/// then the arguments': 2 object, 3 int, 4 long, 5 double, 6 C string,
/// 7 the receiver, 8 the argument array, 9 the message name, 0x10 an
/// optional object), `f(args)` runs it: `args[0]` points to the result,
/// `args[n]` to the nth argument's value.
pub fn native_call(world: &Rc<World>, spec: &[u8], recv: &Value, name: &[u8], args: &[Option<Value>]) -> Option<Result<Option<Value>, (u32, Vec<u8>)>> {
    let saved = unsafe { core::mem::replace(&mut *(&raw mut CURRENT), Some(world.clone())) };
    let r = native_call_in(spec, recv, name, args);
    unsafe { *(&raw mut CURRENT) = saved };
    r
}

fn native_call_in(spec: &[u8], recv: &Value, name: &[u8], args: &[Option<Value>]) -> Option<Result<Option<Value>, (u32, Vec<u8>)>> {
    let f = routine(spec, name)?;
    let call: extern "C" fn(*mut u32) -> *const u8 = unsafe { core::mem::transmute(f) };
    let sig = cstr(call(core::ptr::null_mut()));
    if sig.is_empty() {
        return None;
    }
    let mark = session().objs.len();
    let mut result = [0u32; 2];
    // each argument is passed by its address: the values first
    let mut words: Vec<u32> = Vec::new();
    let mut next = args.iter();
    let mut keep: Vec<Vec<u8>> = Vec::new();
    for &t in &sig[1..] {
        match t & 0x7f {
            7 => words.push(obj(recv.clone())),
            8 => words.push(obj(w().array_opt(args.to_vec()))),
            9 => words.push(obj(Value::S(name.to_vec()))),
            3 | 4 => words.push(next.next().cloned().flatten().and_then(|v| whole(&v)).unwrap_or(0) as u32),
            5 => {
                let d = next.next().cloned().flatten().and_then(|v| core::str::from_utf8(&fast(&v)).ok().and_then(|s| s.trim().parse::<f64>().ok())).unwrap_or(0.0);
                let b = d.to_bits();
                words.push(b as u32);
                words.push((b >> 32) as u32);
            }
            6 => {
                let mut s = next.next().cloned().flatten().map(|v| fast(&v)).unwrap_or_default();
                s.push(0);
                words.push(s.as_ptr() as u32);
                keep.push(s);
            }
            _ => words.push(next.next().cloned().flatten().map_or(0, obj)),
        }
    }
    // the argument list: the result's address, then each value's (a
    // double takes two words)
    let mut list: Vec<u32> = alloc::vec![result.as_mut_ptr() as u32];
    let mut i = 0;
    for &t in &sig[1..] {
        list.push(unsafe { words.as_ptr().add(i) } as u32);
        i += if t & 0x7f == 5 { 2 } else { 1 };
    }
    unsafe { RAISED = None };
    call(list.as_mut_ptr());
    let out = match sig[0] & 0x7f {
        2 | 0x10 => value(result[0]),
        3 | 4 => Some(Value::S(alloc::format!("{}", result[0] as i32).into_bytes())),
        6 => Some(Value::S(cstr(result[0] as *const u8))),
        _ => None,
    };
    // the handles the call made are done with
    let s = session();
    for e in s.objs.iter_mut().skip(mark) {
        *e = None;
    }
    if let Some((code, a)) = unsafe { (*(&raw mut RAISED)).take() } {
        let text = a.and_then(|a| World::array_items(&a)).map(|v| v.iter().flatten().map(fast).collect::<Vec<_>>().join(&b' ')).unwrap_or_default();
        return Some(Err((code, text)));
    }
    drop(keep);
    Some(Ok(out))
}

// ---- the REXX classes for scripting OpenDoc ----

/// rs2's own REXX code for the classes OREXXSC's native methods work with
/// (IBM keeps its in REXX.IMG): ScriptableApp (CONNECT, DISCONNECT and
/// messages it does not know become Apple events to the application,
/// natives of REXXSC, OREXXSC's module) and the object-specifier
/// classes OREXXSC makes from Apple-event replies. They go into
/// `.environment` when RexxSomInitialize makes the world.
const OPENDOC_CLASSES: &[u8] = b"\
/* rs2's OpenDoc scripting classes */\r\n\
.environment~setentry('SCRIPTABLEAPP', .ScriptableApp)\r\n\
.environment~setentry('OBJECTSPECIFIER', .ObjectSpecifier)\r\n\
.environment~setentry('SCREFERENCE', .SCReference)\r\n\
.environment~setentry('SCRELATIVEPOSITION', .SCRelativePosition)\r\n\
.environment~setentry('SCLIST', .SCList)\r\n\
.environment~setentry('SCRANGE', .SCRange)\r\n\
.environment~setentry('SCCLASS', .SCClass)\r\n\
return\r\n\
::class ScriptableApp public\r\n\
::method init\r\n\
  expose aete name currapp dirparm\r\n\
  aete = 0; name = ''; currapp = .nil; dirparm = .nil\r\n\
::method getaete;      expose aete;    return aete\r\n\
::method setaete;      expose aete;    use arg aete\r\n\
::method getname;      expose name;    return name\r\n\
::method setname;      expose name;    use arg name\r\n\
::method getcurrapp;   expose currapp; return currapp\r\n\
::method setcurrapp;   expose currapp; use arg currapp\r\n\
::method getdirparm;   expose dirparm; return dirparm\r\n\
::method setdirparm;   expose dirparm; use arg dirparm\r\n\
::method resetdirparm; expose dirparm; dirparm = .nil\r\n\
::method connect class; use arg name; return self~new~connect(name)\r\n\
::method connect    external 'LIBRARY REXXSC ScriptableApp_Connect'\r\n\
::method disconnect external 'LIBRARY REXXSC ScriptableApp_Disconnect'\r\n\
::method novalue    external 'LIBRARY REXXSC ScriptableApp_Novalue'\r\n\
::method sendevent  external 'LIBRARY REXXSC ScriptableApp_Unknown'\r\n\
/* app~event([direct parameter] [, .directory of named parameters]) */\r\n\
::method unknown\r\n\
  expose dirparm\r\n\
  use arg msg, args\r\n\
  named = .array~new\r\n\
  if args~items > 0 then do\r\n\
    if args[1]~class \\== .directory then do\r\n\
      dirparm = args[1]\r\n\
      if args~items > 1 then named[1] = args[2]\r\n\
    end\r\n\
    else named[1] = args[1]\r\n\
  end\r\n\
  return self~sendevent(msg, named)\r\n\
::class 'ObjectSpecifier' public\r\n\
::method init; expose class reference container; use arg class, reference; container = .nil\r\n\
::method setcontainer; expose container; use arg container\r\n\
::method getcontainer; expose container; return container\r\n\
::method getclass; expose class; return class\r\n\
::method getreference; expose reference; return reference\r\n\
/* text~of(part~index(1)): the specifier with that container */\r\n\
::method of; expose container; use arg container; return self\r\n\
::method makeaedesc external 'LIBRARY REXXSC ObjectSpecifier_MakeAEDesc'\r\n\
::class SCReference public\r\n\
::method init; expose form data type; use arg form, data, type; if arg(3, 'o') then type = 0\r\n\
::method getkeyform; expose form; return form\r\n\
::method getkeydata; expose data; return data\r\n\
::method gettype; expose type; return type\r\n\
::method makeaedesc external 'LIBRARY REXXSC SCReference_MakeAEDesc'\r\n\
::class SCRelativePosition public\r\n\
::method init; expose pos; use arg pos\r\n\
::method getposition; expose pos; return pos\r\n\
::class 'SCList' public subclass array\r\n\
::method makeaedesc external 'LIBRARY REXXSC SCList_MakeAEDesc'\r\n\
::class SCRange public\r\n\
::method init; expose start stop; use arg start, stop\r\n\
::method getstart; expose start; return start\r\n\
::method getstop; expose stop; return stop\r\n\
::method makeaedesc external 'LIBRARY REXXSC SCRange_MakeAEDesc'\r\n\
/* a class of the application's terminology (what NOVALUE gives for its\r\n\
   name): part~index(1), part~named('x') */\r\n\
::class SCClass public\r\n\
::method init; expose name code; use arg name, code\r\n\
::method getname; expose name; return name\r\n\
::method getclass; expose code; return code\r\n\
::method index\r\n\
  expose code; use arg n\r\n\
  return .ObjectSpecifier~new(code, .SCReference~new(2019847785, n, 2))  /* 'indx' */\r\n\
::method named\r\n\
  expose code; use arg s\r\n\
  return .ObjectSpecifier~new(code, .SCReference~new(1701667182, s))  /* 'name' */\r\n\
";

fn install_classes() {
    let _ = with_interp(|it| it.run_program_v(OPENDOC_CLASSES, b"OPENDOC", CallType::Subroutine, Vec::new(), b"CMD").map(|_| ()));
}
