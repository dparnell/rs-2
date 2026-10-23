//! SOM for the Workplace Shell classes: SOM.DLL's calls, method calls
//! through class data tokens (so subclasses' overrides run), instance
//! data through the class's data token, and stack-aligning entries for
//! the methods (code compiled for OS/2 calls them with 4-byte alignment).

use alloc::vec::Vec;

pub type SomId = *mut *const u8;

unsafe extern "C" {
    #[link_name = "SOM.49"]
    pub fn somBuildClass(inherit: i32, sci: *const u32, major: i32, minor: i32) -> usize;
    #[link_name = "SOM.31"]
    pub fn somIdFromString(s: *const u8) -> SomId;
    #[link_name = "SOM.40"]
    pub fn somStringFromId(id: SomId) -> *const u8;
    #[link_name = "SOM.50"]
    pub fn somParentNumResolve(mtabs: usize, n: i32, token: usize) -> usize;
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
    #[link_name = "SOM.10"]
    pub static SOMFree: extern "C" fn(*mut u8);
}

/// SOMObject's, SOMClass's and SOMClassMgr's methods used here.
pub mod so {
    pub const SOM_FREE: usize = 2;
    pub const SOM_GET_CLASS_NAME: usize = 4;
    pub const SOM_GET_CLASS: usize = 5;
    pub const SOM_IS_A: usize = 6;
    pub const SOM_RESPONDS_TO: usize = 7;
}
pub mod sc {
    pub const SOM_NEW: usize = 0;
    pub const SOM_GET_NAME: usize = 4;
    pub const SOM_GET_PARENT: usize = 5;
    pub const SOM_DESCENDED_FROM: usize = 6;
    pub const SOM_FIND_S_METHOD: usize = 24;
}
pub mod scm {
    pub const SOM_FIND_CLS_IN_FILE: usize = 0;
    pub const SOM_FIND_CLASS: usize = 1;
    pub const SOM_CLASS_FROM_ID: usize = 2;
    pub const SOM_REGISTER_CLASS: usize = 3;
    pub const SOM_UNREGISTER_CLASS: usize = 4;
    pub const SOM_LOCATE_CLASS_FILE: usize = 5;
    pub const SOM_MERGE_INTO: usize = 9;
}

/// Call `cd`'s method `i` (a token) with `args` (the object first).
#[inline(never)]
pub fn call(cd: *const usize, i: usize, args: &[usize]) -> usize {
    let tok = unsafe { *cd.add(1 + i) };
    if tok == 0 {
        return 0;
    }
    unsafe {
        match args.len() {
            1 => core::mem::transmute::<usize, extern "C" fn(usize) -> usize>(tok)(args[0]),
            2 => core::mem::transmute::<usize, extern "C" fn(usize, usize) -> usize>(tok)(args[0], args[1]),
            3 => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize) -> usize>(tok)(args[0], args[1], args[2]),
            4 => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize, usize) -> usize>(tok)(
                args[0], args[1], args[2], args[3],
            ),
            5 => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize, usize, usize) -> usize>(tok)(
                args[0], args[1], args[2], args[3], args[4],
            ),
            6 => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize, usize, usize, usize) -> usize>(tok)(
                args[0], args[1], args[2], args[3], args[4], args[5],
            ),
            7 => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize, usize, usize, usize, usize) -> usize>(tok)(
                args[0], args[1], args[2], args[3], args[4], args[5], args[6],
            ),
            8 => core::mem::transmute::<usize, extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> usize>(tok)(
                args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
            ),
            _ => 0,
        }
    }
}

/// The parent class's implementation of a method (`parent_` calls).
pub fn parent(ccd: *const usize, cd: *const usize, i: usize) -> usize {
    let pm = unsafe { *ccd };
    unsafe { somParentNumResolve(pm, 1, *cd.add(1 + i)) }
}

pub fn call_parent(ccd: *const usize, cd: *const usize, i: usize, args: &[usize]) -> usize {
    let p = parent(ccd, cd, i);
    if p == 0 {
        return 0;
    }
    // A one-entry table holding the implementation stands in for a token.
    let t = [0usize, p];
    call(t.as_ptr(), 0, args)
}

/// A class's instance data in an object (its CClassData's data token).
pub fn data<'a, T>(ccd: *const usize, obj: usize) -> &'a mut T {
    unsafe {
        let tok: extern "C" fn(usize) -> *mut T = core::mem::transmute(*ccd.add(1));
        &mut *tok(obj)
    }
}

pub fn so_call(i: usize, args: &[usize]) -> usize {
    call(unsafe { &raw const SOMObjectClassData } as *const usize, i, args)
}
pub fn sc_call(i: usize, args: &[usize]) -> usize {
    call(unsafe { &raw const SOMClassClassData } as *const usize, i, args)
}
pub fn scm_call(i: usize, args: &[usize]) -> usize {
    let mgr = unsafe { SOMClassMgrObject };
    let mut a = Vec::with_capacity(args.len() + 1);
    a.push(mgr);
    a.extend_from_slice(args);
    call(unsafe { &raw const SOMClassMgrClassData } as *const usize, i, &a)
}

pub fn class_of(obj: usize) -> usize {
    so_call(so::SOM_GET_CLASS, &[obj])
}

pub fn is_a(obj: usize, cls: usize) -> bool {
    obj != 0 && cls != 0 && so_call(so::SOM_IS_A, &[obj, cls]) & 0xff != 0
}

pub fn class_name(cls: usize) -> &'static [u8] {
    crate::util::cstr_static(sc_call(sc::SOM_GET_NAME, &[cls]) as *const u8)
}

/// A somId for a name, kept for good (SOM rewrites ids in place).
pub fn id(name: &[u8]) -> SomId {
    let mut v = name.to_vec();
    v.push(0);
    unsafe { somIdFromString(alloc::boxed::Box::leak(v.into_boxed_slice()).as_ptr()) }
}

/// Find a class by name (loading its DLL if registered).
pub fn find_class(name: &[u8]) -> usize {
    scm_call(scm::SOM_FIND_CLASS, &[id(name) as usize, 0, 0])
}

/// What a method nobody implemented yet returns: 0 (FALSE, NULL),
/// whatever its arguments (the caller removes them).
#[unsafe(naked)]
pub extern "C" fn ret0() {
    core::arch::naked_asm!("xor eax, eax", "ret");
}

/// Stack-aligning entries for method implementations.
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
