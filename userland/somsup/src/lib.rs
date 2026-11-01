//! What rs2's SOM class libraries (SOMTC.DLL, SOMU.DLL, SOMIR.DLL) share:
//! SOM.DLL's calls and variables, the CORBA environment, memory through
//! SOM's replaceable allocator, classes built the way SOM compiler output
//! builds them (somBuildClass with a static class information structure,
//! methods in release order), and method calls through class data tokens
//! (so that subclasses' overrides run).

#![no_std]
#![allow(non_snake_case, non_upper_case_globals, function_casts_as_integer)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

pub type SomId = *mut *const u8;

/// The CORBA environment (somcorba.h).
#[repr(C)]
pub struct Environment {
    pub major: u32,
    pub exception_name: *const u8,
    pub params: *mut u8,
    pub anchor: usize,
}

pub const NO_EXCEPTION: u32 = 0;
pub const USER_EXCEPTION: u32 = 1;
pub const SYSTEM_EXCEPTION: u32 = 2;

unsafe extern "C" {
    #[link_name = "SOM.49"]
    pub fn somBuildClass(inherit: i32, sci: *const u32, major: i32, minor: i32) -> usize;
    #[link_name = "SOM.31"]
    pub fn somIdFromString(s: *const u8) -> SomId;
    #[link_name = "SOM.40"]
    pub fn somStringFromId(id: SomId) -> *const u8;
    #[link_name = "SOM.27"]
    pub fn somCompareIds(a: SomId, b: SomId) -> u32;
    #[link_name = "SOM.50"]
    pub fn somParentNumResolve(mtabs: usize, n: i32, token: usize) -> usize;
    #[link_name = "SOM.47"]
    pub fn somDataResolve(obj: usize, token: usize) -> usize;
    #[link_name = "SOM.37"]
    pub fn somResolve(obj: usize, token: usize) -> usize;
    #[link_name = "SOM.48"]
    pub fn somClassResolve(cls: usize, token: usize) -> usize;
    #[link_name = "SOM.33"]
    pub fn somParentResolve(mtabs: usize, token: usize) -> usize;
    #[link_name = "SOM.60"]
    pub fn somIsObj(p: usize) -> u32;
    #[link_name = "SOM.30"]
    pub fn somEnvironmentNew() -> usize;
    #[link_name = "SOM.22"]
    pub fn SOMObjectNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOM.21"]
    pub fn SOMClassNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOM.20"]
    pub fn SOMClassMgrNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOM.45"]
    pub fn somVprintf(fmt: *const u8, ap: *const u32) -> i32;
    #[link_name = "SOM.34"]
    pub fn somPrefixLevel(level: i32);
    #[link_name = "SOM.55"]
    pub fn somSetException(ev: *mut Environment, major: u32, name: *const u8, params: *mut u8);
    #[link_name = "SOM.52"]
    pub fn somExceptionId(ev: *mut Environment) -> *const u8;
    #[link_name = "SOM.54"]
    pub fn somExceptionFree(ev: *mut Environment);
    #[link_name = "SOM.58"]
    pub fn somGetGlobalEnvironment() -> *mut Environment;
    #[link_name = "SOM.62"]
    pub fn somStartCriticalSection();
    #[link_name = "SOM.63"]
    pub fn somEndCriticalSection();
    #[link_name = "SOM.69"]
    pub fn somApply(obj: usize, ret: *mut u8, md: *const u8, ap: *const u32) -> u32;
    #[link_name = "SOM.14"]
    pub static SOMObjectClassData: [usize; 33];
    #[link_name = "SOM.3"]
    pub static SOMClassClassData: [usize; 77];
    #[link_name = "SOM.6"]
    pub static SOMClassMgrClassData: [usize; 23];
    #[link_name = "SOM.7"]
    pub static mut SOMClassMgrObject: usize;
    #[link_name = "SOM.12"]
    pub static SOMMalloc: extern "C" fn(u32) -> *mut u8;
    #[link_name = "SOM.1"]
    pub static SOMCalloc: extern "C" fn(u32, u32) -> *mut u8;
    #[link_name = "SOM.16"]
    pub static SOMRealloc: extern "C" fn(*mut u8, u32) -> *mut u8;
    #[link_name = "SOM.10"]
    pub static SOMFree: extern "C" fn(*mut u8);
    #[link_name = "SOM.9"]
    pub static SOMError: extern "C" fn(i32, *const u8, i32);
    #[link_name = "SOM.18"]
    pub static SOM_TraceLevel: i32;
    #[link_name = "SOM.19"]
    pub static SOM_WarnLevel: i32;
}

/// Stack-aligning exported entry points: OS/2 compilers keep 4-byte
/// stack alignment, Rust code assumes 16. The arguments (`_System`: the
/// caller removes them) are copied to an aligned frame.
#[macro_export]
macro_rules! aligned {
    ($($tramp:ident => $target:path, $copy:literal;)*) => {$(
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16",
                "sub esp, {pad}",
                "push esi", "push edi", "push ecx",
                "lea esi, [ebp + 8]", "lea edi, [esp + 12]", "mov ecx, {n}", "rep movsd",
                "pop ecx", "pop edi", "pop esi",
                "call {f}", "mov esp, ebp", "pop ebp", "ret",
                pad = const ($copy * 4 + 15) / 16 * 16, n = const $copy, f = sym $target,
            );
        }
    )*};
}

/// The same for method implementations put in method tables.
#[macro_export]
macro_rules! methods {
    ($($tramp:ident => $target:path, $copy:literal;)*) => {$(
        #[unsafe(naked)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16",
                "sub esp, {pad}",
                "push esi", "push edi", "push ecx",
                "lea esi, [ebp + 8]", "lea edi, [esp + 12]", "mov ecx, {n}", "rep movsd",
                "pop ecx", "pop edi", "pop esi",
                "call {f}", "mov esp, ebp", "pop ebp", "ret",
                pad = const ($copy * 4 + 15) / 16 * 16, n = const $copy, f = sym $target,
            );
        }
    )*};
}

/// `_Optlink` entry points (VisualAge C++'s default linkage, which IBM's
/// SOM libraries export some functions with): the first three arguments
/// in EAX, EDX and ECX (the caller still reserves their stack words), the
/// rest on the stack; the caller removes them. They are passed on to a
/// `extern "C"` function on an aligned stack.
#[macro_export]
macro_rules! optlink {
    ($($tramp:ident => $target:path, $n:literal;)*) => {$(
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16",
                "sub esp, {pad}",
                "mov [esp], eax", "mov [esp + 4], edx", "mov [esp + 8], ecx",
                "push esi", "push edi", "push ecx",
                "lea esi, [ebp + 20]", "lea edi, [esp + 24]", "mov ecx, {rest}", "rep movsd",
                "pop ecx", "pop edi", "pop esi",
                "call {f}", "mov esp, ebp", "pop ebp", "ret",
                pad = const (if $n > 3 { $n } else { 3 } * 4 + 15) / 16 * 16,
                rest = const if $n > 3 { $n - 3 } else { 0 },
                f = sym $target,
            );
        }
    )*};
}

/// A DLL's `_DLL_InitTerm` entry, entered with 4-byte stack alignment:
/// `dll_entry!(init)` with `fn init(hmod: u32, term: u32) -> u32`.
#[macro_export]
macro_rules! dll_entry {
    ($init:path) => {
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".text.start")]
        pub extern "C" fn _start() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 16",
                "mov eax, [ebp + 8]", "mov [esp], eax",
                "mov eax, [ebp + 12]", "mov [esp + 4], eax",
                "call {init}",
                "mov esp, ebp", "pop ebp", "ret",
                init = sym $init,
            );
        }
    };
}

// ---- Memory (SOM's replaceable allocator) ----

pub fn malloc(n: usize) -> *mut u8 {
    unsafe { (SOMMalloc)(n as u32) }
}

pub fn calloc(n: usize) -> *mut u8 {
    unsafe { (SOMCalloc)(1, n as u32) }
}

pub fn realloc(p: *mut u8, n: usize) -> *mut u8 {
    unsafe { (SOMRealloc)(p, n as u32) }
}

pub fn free(p: *mut u8) {
    if !p.is_null() {
        unsafe { (SOMFree)(p) }
    }
}

/// A copy of a C string in SOMMalloc'd memory.
pub fn dup(s: &[u8]) -> *mut u8 {
    let p = malloc(s.len() + 1);
    if !p.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            *p.add(s.len()) = 0;
        }
    }
    p
}

pub unsafe fn strlen(p: *const u8) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    n
}

pub unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(p, strlen(p)) }
}

/// A NUL-terminated copy of a byte string (Rust heap).
pub fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

pub fn eq_nocase(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

// ---- Output and errors ----

/// somPrintf with its arguments as 32-bit words.
pub fn printf(fmt: &[u8], args: &[u32]) -> i32 {
    unsafe { somVprintf(fmt.as_ptr(), args.as_ptr()) }
}

/// Raise an exception in `ev` (if there is one). `name` ends in a NUL.
pub fn set_exception(ev: *mut Environment, major: u32, name: &'static [u8], params: *mut u8) {
    if !ev.is_null() {
        unsafe { somSetException(ev, major, name.as_ptr(), params) };
    }
}

pub fn error(code: i32, file: &'static [u8], line: i32) {
    unsafe { (SOMError)(code, file.as_ptr(), line) }
}

// ---- Classes ----

/// A somId for a name, kept for good (SOM rewrites ids in place).
pub fn id(name: &[u8]) -> SomId {
    let v = z(name);
    unsafe { somIdFromString(Box::leak(v.into_boxed_slice()).as_ptr()) }
}

fn leak_id(name: &[u8]) -> SomId {
    let p = Box::leak(z(name).into_boxed_slice()).as_ptr();
    Box::leak(Box::new(p))
}

fn leak_id_ptr(name: &[u8]) -> usize {
    Box::leak(Box::new(leak_id(name))) as *mut SomId as usize
}

/// What a class needs to be built (`<Class>NewClass`).
pub struct ClassSpec<'a> {
    pub name: &'a str,
    pub parents: &'a [&'a str],
    pub meta: Option<&'a str>,
    pub major: i32,
    pub minor: i32,
    /// Instance data size and alignment.
    pub data: u32,
    pub align: u32,
    /// The methods the class introduces, in release order (the order of
    /// their tokens in the class data; "" for an entry without a method),
    /// with their implementations (0: one that returns 0).
    pub methods: &'a [(&'a str, usize)],
    pub overrides: &'a [(&'a str, usize)],
    pub cd: *mut usize,
    pub ccd: *mut usize,
}

/// What a method nobody implemented returns: 0 (FALSE, NULL), whatever its
/// arguments (the caller removes them).
#[unsafe(naked)]
pub extern "C" fn ret0() {
    core::arch::naked_asm!("xor eax, eax", "ret");
}

/// Build a class from its description with a layout-4 somStaticClassInfo
/// (as OS/2 Warp 4's SOM compiler emits it), unless it exists.
pub fn build_class(s: &ClassSpec) -> usize {
    unsafe {
        if *s.cd != 0 {
            return *s.cd;
        }
        SOMObjectNewClass(1, 4);
        SOMClassNewClass(1, 4);
    }
    let mut smt: Vec<[usize; 6]> = Vec::new();
    for (k, &(m, p)) in s.methods.iter().enumerate() {
        if m.is_empty() {
            continue;
        }
        let mut d = s.name.as_bytes().to_vec();
        d.extend_from_slice(b"::");
        d.extend_from_slice(m.as_bytes());
        let p = if p == 0 { ret0 as usize } else { p };
        smt.push([s.cd as usize + 4 * (k + 1), leak_id_ptr(m.as_bytes()), leak_id_ptr(&d), p, usize::MAX, 0]);
    }
    let omt: Vec<[usize; 2]> = s.overrides.iter().map(|&(m, p)| [leak_id_ptr(m.as_bytes()), p]).collect();
    let pids: Vec<SomId> = s.parents.iter().map(|p| leak_id(p.as_bytes())).collect();
    let mut sci = vec![0u32; 48];
    sci[0] = 4;
    sci[1] = smt.len() as u32;
    sci[2] = omt.len() as u32;
    sci[6] = s.major as u32;
    sci[7] = s.minor as u32;
    sci[8] = s.data;
    sci[9] = s.methods.len() as u32;
    sci[10] = pids.len() as u32;
    sci[11] = leak_id(s.name.as_bytes()) as u32;
    sci[12] = s.meta.map_or(0, |m| leak_id(m.as_bytes()) as u32);
    sci[14] = Box::leak(pids.into_boxed_slice()).as_ptr() as u32;
    sci[15] = s.cd as u32;
    sci[16] = s.ccd as u32;
    sci[17] = Box::leak(smt.into_boxed_slice()).as_ptr() as u32;
    sci[18] = Box::leak(omt.into_boxed_slice()).as_ptr() as u32;
    sci[24] = s.align;
    sci[25] = u32::MAX;
    sci[33] = s.methods.len() as u32;
    let sci = Box::leak(sci.into_boxed_slice());
    let inherit = (1i32 << s.parents.len()) - 1;
    unsafe { somBuildClass(inherit, sci.as_ptr(), s.major, s.minor) }
}

/// Call method `i` of class data `cd` (a token) with `args` (the object
/// first).
#[inline(never)]
pub fn call(cd: *const usize, i: usize, args: &[usize]) -> usize {
    let tok = unsafe { *cd.add(1 + i) };
    call_fn(tok, args)
}

/// Call a function (a token or an implementation) with up to 12 words.
#[inline(never)]
pub fn call_fn(f: usize, a: &[usize]) -> usize {
    if f == 0 || a.len() > 12 {
        return 0;
    }
    let mut w = [0usize; 12];
    w[..a.len()].copy_from_slice(a);
    type F = extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize;
    // _System: the callee ignores the words it does not take.
    let g: F = unsafe { core::mem::transmute(f) };
    g(w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7], w[8], w[9], w[10], w[11])
}

/// The parent class's implementation of a method (`parent_` calls).
pub fn parent(ccd: *const usize, cd: *const usize, i: usize) -> usize {
    unsafe { somParentNumResolve(*ccd, 1, *cd.add(1 + i)) }
}

pub fn call_parent(ccd: *const usize, cd: *const usize, i: usize, args: &[usize]) -> usize {
    call_fn(parent(ccd, cd, i), args)
}

/// A class's instance data in an object (its CClassData's data token).
pub fn data<'a, T>(ccd: *const usize, obj: usize) -> &'a mut T {
    unsafe {
        let tok: extern "C" fn(usize) -> *mut T = core::mem::transmute(*ccd.add(1));
        &mut *tok(obj)
    }
}

pub fn so() -> *const usize {
    &raw const SOMObjectClassData as *const usize
}

pub fn sc() -> *const usize {
    &raw const SOMClassClassData as *const usize
}

pub fn scm() -> *const usize {
    &raw const SOMClassMgrClassData as *const usize
}

/// SOMObject's, SOMClass's and SOMClassMgr's methods (release order).
pub mod om {
    pub const SOM_INIT: usize = 0;
    pub const SOM_UNINIT: usize = 1;
    pub const SOM_FREE: usize = 2;
    pub const SOM_GET_CLASS_NAME: usize = 4;
    pub const SOM_GET_CLASS: usize = 5;
    pub const SOM_IS_A: usize = 6;
    pub const SOM_RESPONDS_TO: usize = 7;
    pub const SOM_IS_INSTANCE_OF: usize = 8;
    pub const SOM_GET_SIZE: usize = 9;
    pub const SOM_DUMP_SELF: usize = 10;
    pub const SOM_DUMP_SELF_INT: usize = 11;
    pub const SOM_PRINT_SELF: usize = 12;
    pub const SOM_DISPATCH: usize = 18;
    pub const SOM_CLASS_DISPATCH: usize = 19;
    pub const SOM_DEFAULT_INIT: usize = 22;
    pub const SOM_DESTRUCT: usize = 23;
}
pub mod cm {
    pub const SOM_NEW: usize = 0;
    pub const SOM_RENEW: usize = 1;
    pub const SOM_INIT_CLASS: usize = 2;
    pub const SOM_CLASS_READY: usize = 3;
    pub const SOM_GET_NAME: usize = 4;
    pub const SOM_GET_PARENT: usize = 5;
    pub const SOM_DESCENDED_FROM: usize = 6;
    pub const SOM_FIND_METHOD: usize = 8;
    pub const SOM_GET_NUM_METHODS: usize = 11;
    pub const SOM_GET_INSTANCE_SIZE: usize = 12;
    pub const SOM_GET_METHOD_INDEX: usize = 15;
    pub const SOM_GET_NUM_STATIC_METHODS: usize = 16;
    pub const SOM_GET_CLASS_MTAB: usize = 18;
    pub const SOM_OVERRIDE_S_METHOD: usize = 20;
    pub const SOM_FIND_S_METHOD: usize = 24;
    pub const SOM_GET_METHOD_DESCRIPTOR: usize = 26;
    pub const SOM_GET_NTH_METHOD_INFO: usize = 27;
    pub const SOM_GET_CLASS_DATA: usize = 29;
    pub const SOM_NEW_NO_INIT: usize = 30;
    pub const SOM_GET_METHOD_DATA: usize = 35;
    pub const SOM_OVERRIDE_MTAB: usize = 36;
    pub const SOM_GET_METHOD_TOKEN: usize = 37;
    pub const SOM_GET_PARENTS: usize = 38;
    pub const SOM_INIT_MI_CLASS: usize = 40;
    pub const SOM_LOOKUP_METHOD: usize = 42;
    pub const SOM_DEALLOCATE: usize = 47;
    pub const SOM_GET_RD_STUB: usize = 48;
    pub const SOM_GET_NTH_METHOD_DATA: usize = 49;
}
pub mod mm {
    pub const SOM_FIND_CLS_IN_FILE: usize = 0;
    pub const SOM_FIND_CLASS: usize = 1;
    pub const SOM_CLASS_FROM_ID: usize = 2;
    pub const SOM_GET_INTERFACE_REPOSITORY: usize = 12;
    pub const SOM_SET_INTERFACE_REPOSITORY: usize = 13;
}

pub fn so_call(i: usize, args: &[usize]) -> usize {
    call(so(), i, args)
}

pub fn sc_call(i: usize, args: &[usize]) -> usize {
    call(sc(), i, args)
}

pub fn class_of(obj: usize) -> usize {
    so_call(om::SOM_GET_CLASS, &[obj])
}

pub fn class_name(cls: usize) -> *const u8 {
    sc_call(cm::SOM_GET_NAME, &[cls]) as *const u8
}

pub fn is_a(obj: usize, cls: usize) -> bool {
    obj != 0 && cls != 0 && so_call(om::SOM_IS_A, &[obj, cls]) & 0xff != 0
}

pub fn new_object(cls: usize) -> usize {
    if cls == 0 { 0 } else { sc_call(cm::SOM_NEW, &[cls]) }
}

pub fn free_object(obj: usize) {
    if obj != 0 {
        so_call(om::SOM_FREE, &[obj]);
    }
}

/// Find a class by name through the class manager.
pub fn find_class(name: &[u8], major: i32, minor: i32) -> usize {
    let mgr = unsafe { SOMClassMgrObject };
    call(scm(), mm::SOM_FIND_CLASS, &[mgr, id(name) as usize, major as usize, minor as usize])
}

/// The class data of a class (the address its `<Class>ClassData` is at).
pub fn class_data_of(cls: usize) -> *const usize {
    sc_call(cm::SOM_GET_CLASS_DATA, &[cls]) as *const usize
}
