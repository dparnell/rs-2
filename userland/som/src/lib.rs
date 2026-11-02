//! SOM.DLL: the System Object Model runtime (SOM 2.1 as in OS/2 Warp 4),
//! which the Workplace Shell and its classes are built on.
//!
//! Binary compatible with IBM's SOM.DLL: the same ordinals, the same
//! exported variables (SOMMalloc, SOMError, SOMClassMgrObject, ...) and
//! class data structures, method and data tokens that are callable code,
//! method tables with IBM's public header, `somStaticClassInfo` (the
//! layouts the SOM compiler emits), the initializer, destructor and
//! assignment control structures emitted code walks, and registered ids
//! rewritten in place. The machinery is in rt.rs; SOMObject, SOMClass
//! and SOMClassMgr are in object.rs, class.rs and mgr.rs. See
//! docs/WPS.md.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, function_casts_as_integer)]

extern crate alloc;
extern crate os2;

pub mod class;
pub mod env;
pub mod exports;
pub mod fmt;
pub mod id;
pub mod mgr;
pub mod object;
pub mod rt;
pub mod sys;

use alloc::vec::Vec;

use exports::*;
use id::SomId;
use rt::*;
use sys::*;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"SOMDEBUG\0".as_ptr(), &mut v) } == 0 {
        unsafe { SOM_WarnLevel = 2 };
    }
    1
}

// ---- Exported variables ----

#[unsafe(no_mangle)]
pub static mut SOMObjectClassData: [usize; 33] = [0; 33];
#[unsafe(no_mangle)]
pub static mut SOMObjectCClassData: [usize; 8] = [0; 8];
#[unsafe(no_mangle)]
pub static mut SOMClassClassData: [usize; 77] = [0; 77];
#[unsafe(no_mangle)]
pub static mut SOMClassCClassData: [usize; 8] = [0; 8];
#[unsafe(no_mangle)]
pub static mut SOMClassMgrClassData: [usize; 23] = [0; 23];
#[unsafe(no_mangle)]
pub static mut SOMClassMgrCClassData: [usize; 8] = [0; 8];
#[unsafe(no_mangle)]
pub static mut SOMParentDerivedMetaclassClassData: [usize; 8] = [0; 8];
pub static mut SOMParentDerivedMetaclassCClassData: [usize; 8] = [0; 8];

#[unsafe(no_mangle)]
pub static mut SOMClassMgrObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOM_MajorVersion: i32 = 1;
#[unsafe(no_mangle)]
pub static mut SOM_MinorVersion: i32 = 5;
#[unsafe(no_mangle)]
pub static mut SOM_MaxThreads: i32 = 256;
#[unsafe(no_mangle)]
pub static mut SOM_TraceLevel: i32 = 0;
#[unsafe(no_mangle)]
pub static mut SOM_WarnLevel: i32 = 0;
#[unsafe(no_mangle)]
pub static mut SOM_AssertLevel: i32 = 0;
#[unsafe(no_mangle)]
pub static mut SOM_IdTable: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOM_IdTableSize: i32 = 0;

// Replaceable procedures (programs may store their own).
#[unsafe(no_mangle)]
pub static mut SOMMalloc: extern "C" fn() = __a_som_malloc;
#[unsafe(no_mangle)]
pub static mut SOMCalloc: extern "C" fn() = __a_som_calloc;
#[unsafe(no_mangle)]
pub static mut SOMRealloc: extern "C" fn() = __a_som_realloc;
#[unsafe(no_mangle)]
pub static mut SOMFree: extern "C" fn() = __a_som_free;
#[unsafe(no_mangle)]
pub static mut SOMError: extern "C" fn() = __a_som_error;
#[unsafe(no_mangle)]
pub static mut SOMOutCharRoutine: extern "C" fn() = __a_som_out_char;
#[unsafe(no_mangle)]
pub static mut SOMLoadModule: extern "C" fn() = __a_load_module;
#[unsafe(no_mangle)]
pub static mut SOMDeleteModule: extern "C" fn() = __a_delete_module;
#[unsafe(no_mangle)]
pub static mut SOMClassInitFuncName: extern "C" fn() = __a_init_func_name;
#[unsafe(no_mangle)]
pub static mut SOMCreateMutexSem: extern "C" fn() = __a_create_mutex;
#[unsafe(no_mangle)]
pub static mut SOMRequestMutexSem: extern "C" fn() = __a_request_mutex;
#[unsafe(no_mangle)]
pub static mut SOMReleaseMutexSem: extern "C" fn() = __a_release_mutex;
#[unsafe(no_mangle)]
pub static mut SOMDestroyMutexSem: extern "C" fn() = __a_destroy_mutex;
#[unsafe(no_mangle)]
pub static mut SOMGetThreadId: extern "C" fn() = __a_thread_id;

// ---- Helpers ----

pub fn malloc(n: u32) -> *mut u8 {
    let f: extern "C" fn(u32) -> *mut u8 = unsafe { core::mem::transmute(SOMMalloc) };
    f(n)
}

pub fn free(p: *mut u8) {
    let f: extern "C" fn(*mut u8) = unsafe { core::mem::transmute(SOMFree) };
    f(p)
}

pub fn error(code: i32, file: &[u8], line: i32) {
    let f: extern "C" fn(i32, *const u8, i32) = unsafe { core::mem::transmute(SOMError) };
    f(code, file.as_ptr(), line)
}

// ---- Errors raised inside SOM's own operations ----
//
// IBM's SOM catches errors raised while it builds a class (somThrowError
// longjmps to the operation, which cleans up and then reports the error
// through SOMError); elsewhere they go to SOMError at once. rs2 records
// the error for the operation to check.

static mut CATCHER: u32 = 0;
static mut CAUGHT: i32 = 0;

/// Start catching errors on this thread; returns the previous catcher.
pub fn catch_begin() -> (u32, i32) {
    let tid = sys::thread_id();
    unsafe {
        let old = (CATCHER, CAUGHT);
        CATCHER = tid;
        CAUGHT = 0;
        old
    }
}

/// Stop catching; returns the error caught (0: none).
pub fn catch_end(old: (u32, i32)) -> i32 {
    unsafe {
        let code = CAUGHT;
        (CATCHER, CAUGHT) = old;
        code
    }
}

/// somThrowError: to the operation catching errors on this thread, or
/// SOMError.
pub fn throw(code: i32, file: &[u8], line: i32) {
    unsafe {
        if CATCHER != 0 && CATCHER == sys::thread_id() {
            if CAUGHT == 0 {
                CAUGHT = code;
            }
            return;
        }
    }
    error(code, file, line);
}

/// Call a method token with its arguments (the object first).
pub fn call_tok(tok: usize, args: &[u32]) -> u32 {
    if tok == 0 {
        return 0;
    }
    call_args(tok, args.as_ptr(), args.len() as u32) as u32
}

pub fn call_om(_obj: usize, idx: usize, args: &[u32]) -> u32 {
    call_tok(unsafe { SOMObjectClassData[1 + idx] }, args)
}

pub fn call_cm(_cls: usize, idx: usize, args: &[u32]) -> u32 {
    call_tok(unsafe { SOMClassClassData[1 + idx] }, args)
}

pub fn call_mm(_mgr: usize, idx: usize, args: &[u32]) -> u32 {
    call_tok(unsafe { SOMClassMgrClassData[1 + idx] }, args)
}

pub fn call_om_destruct(obj: usize, do_free: u32) {
    call_om(obj, om::DESTRUCT, &[obj as u32, do_free, 0]);
}

pub fn class_mgr() -> usize {
    unsafe { SOMClassMgrObject }
}

/// Each thread's output routine (somSetOutChar), used while
/// SOMOutCharRoutine is the default, as in IBM's somOutputString.
static mut THREAD_OUT: [usize; 257] = [0; 257];

fn output(b: &[u8]) {
    let default = unsafe { SOMOutCharRoutine } as usize == __a_som_out_char as usize;
    let mine = if default { unsafe { THREAD_OUT[(sys::thread_id() as usize).min(256)] } } else { 0 };
    if mine != 0 {
        let f: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(mine) };
        for &c in b {
            f(c as u32);
        }
        // IBM's somOutputString passes the string's terminating 0 too.
        f(0);
    } else if default {
        let mut n = 0;
        unsafe { os2::DosWrite(1, b.as_ptr(), b.len() as u32, &mut n) };
    } else {
        let f: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(SOMOutCharRoutine) };
        for &c in b {
            f(c as u32);
        }
    }
}

pub fn printf(fmt: &[u8], args: &[u32]) -> i32 {
    let s = unsafe { fmt::format(fmt.as_ptr(), args.as_ptr()) };
    output(&s);
    s.len() as i32
}

/// A message shown when SOM_WarnLevel is set (or SOMDEBUG is in the
/// environment).
pub fn warn(fmt: &[u8], args: &[u32]) {
    if unsafe { SOM_WarnLevel } != 0 {
        printf(b"SOM: \0", &[]);
        printf(fmt, args);
    }
}

pub fn prefix_level(level: i32) {
    for _ in 0..level.max(0) {
        output(b"  ");
    }
}

static mut PENDING: Vec<(usize, usize)> = Vec::new();

pub fn run_pending_libraries() {
    #[allow(static_mut_refs)]
    let list = unsafe { core::mem::take(&mut PENDING) };
    let mgr = class_mgr();
    for (name, init) in list {
        call_mm(mgr, class::mm::REGISTER_CLASS_LIBRARY, &[mgr as u32, name as u32, init as u32]);
    }
}

// ---- Default replaceable procedures ----

pub extern "C" fn som_error(code: i32, file: *const u8, line: i32) {
    if code % 10 == 9 || unsafe { SOM_WarnLevel } != 0 {
        printf(b"SOM error %d (%s, line %d)\n\0", &[code as u32, file as u32, line as u32]);
    }
    if code % 10 == 9 {
        unsafe { os2::DosExit(1, code as u32) };
    }
}

pub extern "C" fn som_out_char(c: u32) -> u32 {
    let b = [c as u8];
    let mut n = 0;
    unsafe { os2::DosWrite(1, b.as_ptr(), 1, &mut n) };
    1
}

// ---- Exported functions ----

fn new_class(cds: *const usize, major: i32, minor: i32) -> usize {
    env::env_new();
    let cls = unsafe { *cds };
    if (major != 0 || minor != 0) && class::somCheckVersion(cls, major, minor) == 0 {
        warn(b"%s: version %d.%d requested\n\0", &[class::somGetName(cls) as u32, major as u32, minor as u32]);
    }
    cls
}

#[unsafe(no_mangle)]
pub extern "C" fn SOMObjectNewClass(major: i32, minor: i32) -> usize {
    new_class(&raw const SOMObjectClassData as *const usize, major, minor)
}

#[unsafe(no_mangle)]
pub extern "C" fn SOMClassNewClass(major: i32, minor: i32) -> usize {
    new_class(&raw const SOMClassClassData as *const usize, major, minor)
}

#[unsafe(no_mangle)]
pub extern "C" fn SOMClassMgrNewClass(major: i32, minor: i32) -> usize {
    new_class(&raw const SOMClassMgrClassData as *const usize, major, minor)
}

#[unsafe(no_mangle)]
pub extern "C" fn somEnvironmentNew() -> usize {
    env::env_new()
}

#[unsafe(no_mangle)]
pub extern "C" fn somMainProgram() -> usize {
    env::env_new()
}

#[unsafe(no_mangle)]
pub extern "C" fn somEnvironmentEnd() {}

#[unsafe(no_mangle)]
pub extern "C" fn somAbnormalEnd() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn somBuildClass(inherit_vars: i32, sci: *const env::Sci, major: i32, minor: i32) -> usize {
    if sci.is_null() {
        return 0;
    }
    unsafe { env::build_class(inherit_vars, sci, major, minor) }
}

/// SOM 1's class construction: the class init routine does the work.
#[unsafe(no_mangle)]
pub extern "C" fn somConstructClass(init: usize, parent: usize, meta: usize, cds: *mut usize) {
    env::env_new();
    if cds.is_null() || unsafe { *cds } != 0 {
        return;
    }
    let meta = if meta != 0 { meta } else { unsafe { (*SOMCLASS).obj } };
    let cls = call_cm(meta, class::cm::NEW, &[meta as u32]) as usize;
    if init != 0 {
        let f: extern "C" fn(usize, usize) = unsafe { core::mem::transmute(init) };
        f(cls, parent);
    }
    call_cm(cls, class::cm::SET_CLASS_DATA, &[cls as u32, cds as u32]);
    unsafe { *cds = cls };
    call_cm(cls, class::cm::CLASS_READY, &[cls as u32]);
}

/// somCreateDynamicClass(className, parentName, file, major, minor) (an
/// _Optlink function in IBM's SOM.DLL: see `__o_somCreateDynamicClass`):
/// a subclass of the named parent (found, or loaded from `file`, by the
/// class manager) without data or methods of its own, made by the
/// parent's metaclass, its methods redispatched (somOverrideMtab), and
/// registered.
pub extern "C" fn somCreateDynamicClass(name: *const u8, parent: *const u8, file: *const u8, major: i32, minor: i32) -> usize {
    let mgr = env::env_new();
    if mgr == 0 {
        error(20149, b"SOMCLS.C\0", 0xee3);
        return 0;
    }
    let pid = id::static_id(unsafe { cstr(parent) });
    let p = call_mm(mgr, class::mm::FIND_CLS_IN_FILE, &[mgr as u32, pid as u32, major as u32, major as u32, file as u32]) as usize;
    if p == 0 {
        error(20219, b"SOMCLS.C\0", 0xeeb);
        return 0;
    }
    let meta = object::somGetClass(p);
    let cls = call_cm(meta, class::cm::NEW, &[meta as u32]) as usize;
    call_cm(cls, class::cm::INIT_CLASS, &[cls as u32, name as u32, p as u32, 0, 0, major as u32, minor as u32]);
    call_cm(cls, class::cm::OVERRIDE_MTAB, &[cls as u32]);
    call_cm(cls, class::cm::CLASS_READY, &[cls as u32]);
    cls
}

#[unsafe(no_mangle)]
pub extern "C" fn somResolve(obj: usize, token: usize) -> usize {
    if obj == 0 {
        return 0;
    }
    let c = unsafe { class_of_obj(obj) };
    let p = unsafe { resolve_in(&*c, token) };
    if p == 0 {
        error(20101, b"SOM.C\0", 3);
    }
    p
}

#[unsafe(no_mangle)]
pub extern "C" fn somParentNumResolve(mtabs: usize, n: i32, token: usize) -> usize {
    let mut l = mtabs as *const MtabList;
    for _ in 0..n.max(0) {
        if l.is_null() {
            break;
        }
        l = unsafe { (*l).next };
    }
    if l.is_null() || unsafe { (*l).mtab.is_null() } {
        error(20101, b"SOM.C\0", 4);
        return 0;
    }
    let c = unsafe { (*(*l).mtab).class_info };
    unsafe { resolve_in(&*c, token) }
}

#[unsafe(no_mangle)]
pub extern "C" fn somParentResolve(mtabs: usize, token: usize) -> usize {
    somParentNumResolve(mtabs, 1, token)
}

#[unsafe(no_mangle)]
pub extern "C" fn somClassResolve(cls: usize, token: usize) -> usize {
    match unsafe { rec_ok(cls) } {
        Some(c) => unsafe { resolve_in(c, token) },
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn somResolveTerminal(cls: usize, token: usize) -> usize {
    somClassResolve(cls, token)
}

#[unsafe(no_mangle)]
pub extern "C" fn somAncestorResolve(_obj: usize, ccds: *const usize, token: usize) -> usize {
    if ccds.is_null() {
        return 0;
    }
    let pm = unsafe { *ccds } as *const ParentMtab;
    if pm.is_null() {
        return 0;
    }
    let c = unsafe { (*(*pm).mtab).class_info };
    unsafe { resolve_in(&*c, token) }
}

/// A parent call from `calling` (the first parent that has the method).
#[unsafe(no_mangle)]
pub extern "C" fn somPCallResolve(_obj: usize, calling: usize, token: usize) -> usize {
    let Some(c) = (unsafe { rec_ok(calling) }) else { return 0 };
    let Some(ti) = (unsafe { token_info(token) }) else { return 0 };
    for &p in &c.parents {
        let pr = unsafe { &*p };
        if pr.descends_from(ti.class) {
            return unsafe { resolve_in(pr, token) };
        }
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn somResolveByName(obj: usize, name: *const u8) -> usize {
    if obj == 0 {
        return 0;
    }
    let c = unsafe { &*class_of_obj(obj) };
    lookup(c, unsafe { cstr(name) }).map_or(0, |x| x.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn somDataResolve(obj: usize, token: usize) -> usize {
    data_resolve(obj, token, b"SOM.C\0")
}

#[unsafe(no_mangle)]
pub extern "C" fn somDataResolveChk(obj: usize, token: usize) -> usize {
    unsafe { data_in(obj, token) }
}

#[unsafe(no_mangle)]
pub extern "C" fn somGetClassFromMToken(token: usize) -> usize {
    unsafe { token_info(token) }.map_or(0, |ti| unsafe { (*ti.class).obj })
}

fn readable(p: usize) -> bool {
    if p < 0x10000 {
        return false;
    }
    let (mut cb, mut fl) = (4u32, 0u32);
    let rc = unsafe { os2::DosQueryMem(p as *mut u8, &mut cb, &mut fl) };
    rc == 0 && fl & 0x10 != 0 && fl & 1 != 0
}

#[unsafe(no_mangle)]
pub extern "C" fn somIsObj(p: usize) -> u32 {
    if p & 3 != 0 || !readable(p) {
        return 0;
    }
    let mt = unsafe { *(p as *const usize) };
    if mt & 3 != 0 || !readable(mt) {
        return 0;
    }
    let ci = unsafe { (*(mt as *const Mtab)).class_info };
    if ci.is_null() || !readable(ci as usize) {
        return 0;
    }
    (unsafe { (*ci).magic == CLASS_MAGIC && (*ci).mtab as usize == mt }) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn somTestCls(obj: usize, cls: usize, file: *const u8, line: i32) -> usize {
    if somIsObj(obj) == 0 || object::somIsA(obj, cls) == 0 {
        if unsafe { SOM_WarnLevel } != 0 {
            printf(b"SOM: object %08X is not a %s\n\0", &[obj as u32, class::somGetName(cls) as u32]);
        }
        let f: extern "C" fn(i32, *const u8, i32) = unsafe { core::mem::transmute(SOMError) };
        f(20049, file, line);
    }
    obj
}

#[unsafe(no_mangle)]
pub extern "C" fn somTest(cond: i32, severity: i32, file: *const u8, line: i32, msg: *const u8) {
    if cond == 0 {
        printf(b"SOM test failed: %s\n\0", &[msg as u32]);
        let f: extern "C" fn(i32, *const u8, i32) = unsafe { core::mem::transmute(SOMError) };
        f(severity, file, line);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn somAssert(cond: i32, ecode: i32, file: *const u8, line: i32, msg: *const u8) {
    if cond == 0 {
        printf(b"SOM assertion failed: %s\n\0", &[msg as u32]);
        let f: extern "C" fn(i32, *const u8, i32) = unsafe { core::mem::transmute(SOMError) };
        f(ecode, file, line);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn somCheckArgs() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn somVprintf(fmt: *const u8, ap: *const u32) -> i32 {
    let s = unsafe { fmt::format(fmt, ap) };
    output(&s);
    s.len() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn somPrefixLevel(level: i32) {
    prefix_level(level);
}

#[unsafe(no_mangle)]
/// somSetOutChar(routine) (_Optlink in IBM's SOM.DLL: see
/// `__o_somSetOutChar`) sets the calling thread's output routine, which
/// somPrintf & co. use while SOMOutCharRoutine is the default.
pub extern "C" fn somSetOutChar(f: usize) {
    unsafe { THREAD_OUT[(sys::thread_id() as usize).min(256)] = f };
}

#[unsafe(no_mangle)]
pub extern "C" fn somStartCriticalSection() {
    core::mem::forget(lock());
}

#[unsafe(no_mangle)]
pub extern "C" fn somEndCriticalSection() {
    drop(Guard);
}

#[unsafe(no_mangle)]
pub extern "C" fn somRegisterClassLibrary(name: *const u8, init: usize) {
    let mgr = class_mgr();
    if mgr == 0 {
        #[allow(static_mut_refs)]
        unsafe {
            PENDING.push((name as usize, init))
        };
        return;
    }
    call_mm(mgr, class::mm::REGISTER_CLASS_LIBRARY, &[mgr as u32, name as u32, init as u32]);
}

#[unsafe(no_mangle)]
pub extern "C" fn somUnregisterClassLibrary(name: *const u8) {
    let mgr = class_mgr();
    if mgr != 0 {
        call_mm(mgr, 21, &[mgr as u32, name as u32]);
    }
}

/// somApply: call the method `md` describes with arguments from a
/// va_list that starts with the object.
#[unsafe(no_mangle)]
pub extern "C" fn somApply(_obj: usize, ret: *mut u8, md: *const class::MethodData, ap: *const u32) -> u32 {
    if md.is_null() || unsafe { (*md).method } == 0 {
        return 0;
    }
    let m = unsafe { token_info((*md).token) }.and_then(|ti| unsafe { (&*ti.class).methods.get(ti.index) });
    unsafe { object::apply((*md).method, m, ret, ap) };
    1
}

// ---- The CORBA environment ----

#[repr(C)]
pub struct Environment {
    pub major: u32,
    pub exception_name: *const u8,
    pub params: *mut u8,
    pub anchor: usize,
}

static mut GLOBAL_ENV: Environment =
    Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 };

#[unsafe(no_mangle)]
pub extern "C" fn somGetGlobalEnvironment() -> *mut Environment {
    &raw mut GLOBAL_ENV
}

#[unsafe(no_mangle)]
pub extern "C" fn somSetException(ev: *mut Environment, major: u32, name: *const u8, params: *mut u8) {
    if ev.is_null() {
        return;
    }
    unsafe {
        (*ev).major = major;
        (*ev).exception_name = name;
        (*ev).params = params;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn somExceptionId(ev: *mut Environment) -> *const u8 {
    if ev.is_null() { core::ptr::null() } else { unsafe { (*ev).exception_name } }
}

#[unsafe(no_mangle)]
pub extern "C" fn somExceptionValue(ev: *mut Environment) -> *mut u8 {
    if ev.is_null() { core::ptr::null_mut() } else { unsafe { (*ev).params } }
}

#[unsafe(no_mangle)]
pub extern "C" fn somExceptionFree(ev: *mut Environment) {
    if ev.is_null() {
        return;
    }
    unsafe {
        (*ev).major = 0;
        (*ev).exception_name = core::ptr::null();
        (*ev).params = core::ptr::null_mut();
    }
}

// ---- Rarely used ----

#[unsafe(no_mangle)]
pub extern "C" fn somSaveMetrics() {}
#[unsafe(no_mangle)]
pub extern "C" fn somWriteMetrics(_a: i32, _b: i32) {}
#[unsafe(no_mangle)]
pub extern "C" fn somFreeThreadData() {}
#[unsafe(no_mangle)]
pub extern "C" fn somIdMarshal(_a: usize, _b: usize) -> usize {
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn somMakeUserRdStub(a: usize) -> usize {
    a
}
#[unsafe(no_mangle)]
/// somfixMsgTemplate(template) (_Optlink): turns a message template's
/// escapes (`\n`, `\t`, `\v`, `\r`, `\a`, `\f`, `\"`) into their
/// characters and `%#` into `%0`, in place, as IBM's does.
pub extern "C" fn somfixMsgTemplate(a: *mut u8) {
    if a.is_null() {
        return;
    }
    unsafe {
        let mut p = a;
        while *p != 0 {
            if *p == b'%' && *p.add(1) == b'#' {
                *p.add(1) = b'0';
            } else if *p == b'\\' {
                let c = match *p.add(1) {
                    b'n' => 10,
                    b't' => 9,
                    b'v' => 11,
                    b'r' => 13,
                    b'a' => 7,
                    b'f' => 12,
                    b'"' => b'"',
                    _ => b'\\',
                };
                if c != b'\\' {
                    *p = c;
                    let mut q = p.add(1);
                    while *q != 0 {
                        *q = *q.add(1);
                        q = q.add(1);
                    }
                }
            }
            p = p.add(1);
        }
    }
}

/// What the methods of a class whose implementation is not inherited
/// (abstract inheritance) run: SOMError 20059, as IBM's SOM does.
pub extern "C" fn abstract_method() -> usize {
    error(20059, b"SOMCLS.C\0", 0xecb);
    0
}

/// What a token that resolves to nothing runs.
pub extern "C" fn no_method() -> usize {
    0
}

pub type _Unused = SomId;
