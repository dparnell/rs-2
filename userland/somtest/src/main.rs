//! Self-checks of SOM.DLL. The classes are built the way code from the
//! SOM compiler builds them (somBuildClass with a somStaticClassInfo,
//! method tokens called directly, emitted-style initializers and
//! destructors walking the control structures, parent calls through
//! somParentNumResolve, data through the instance data token), so the
//! program also runs on IBM's SOM.DLL, which is how the expectations
//! were checked. Each check prints PASS or FAIL; the last line is
//! "somtest done: N failure(s)" and the exit code N.
//!
//! `SOMTEST LOAD dll class` also loads a class library (IBM's SOMU.DLL,
//! say) with the class manager and makes an instance.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, static_mut_refs, function_casts_as_integer, unused_assignments, unused_unsafe)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use os2::*;

os2::entry!(main);

type SomId = *mut *const u8;

unsafe extern "C" {
    #[link_name = "SOM.22"]
    fn SOMObjectNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOM.21"]
    fn SOMClassNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOM.49"]
    fn somBuildClass(inherit: i32, sci: *const u32, major: i32, minor: i32) -> usize;
    #[link_name = "SOM.50"]
    fn somParentNumResolve(mtabs: usize, n: i32, token: usize) -> usize;
    #[link_name = "SOM.47"]
    fn somDataResolve(obj: usize, token: usize) -> usize;
    #[link_name = "SOM.31"]
    fn somIdFromString(s: *const u8) -> SomId;
    #[link_name = "SOM.40"]
    fn somStringFromId(id: SomId) -> *const u8;
    #[link_name = "SOM.27"]
    fn somCompareIds(a: SomId, b: SomId) -> u32;
    #[link_name = "SOM.61"]
    fn somResolveByName(obj: usize, name: *const u8) -> usize;
    #[link_name = "SOM.37"]
    fn somResolve(obj: usize, token: usize) -> usize;
    #[link_name = "SOM.82"]
    fn somGetClassFromMToken(token: usize) -> usize;
    #[link_name = "SOM.60"]
    fn somIsObj(p: usize) -> u32;
    #[link_name = "SOM.30"]
    fn somEnvironmentNew() -> usize;
    #[link_name = "SOM.14"]
    static SOMObjectClassData: [usize; 33];
    #[link_name = "SOM.3"]
    static SOMClassClassData: [usize; 77];
    #[link_name = "SOM.6"]
    static SOMClassMgrClassData: [usize; 23];
    #[link_name = "SOM.7"]
    static SOMClassMgrObject: usize;
    #[link_name = "SOM.12"]
    static SOMMalloc: extern "C" fn(u32) -> *mut u8;
    #[link_name = "SOM.10"]
    static SOMFree: extern "C" fn(*mut u8);
    #[link_name = "SOM.9"]
    static mut SOMError: usize;
}

unsafe extern "C" {
    #[link_name = "SOM.100"]
    fn somva_dispatch_import();
}

/// Stack-aligning entries for the methods (IBM's SOM calls them with
/// 4-byte alignment).
macro_rules! aligned {
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

// ---- Calls through tokens ----

unsafe fn f<T: Copy>(p: usize) -> T {
    unsafe { core::mem::transmute_copy(&p) }
}

fn tok(cd: *const usize, i: usize) -> usize {
    unsafe { *cd.add(1 + i) }
}

fn m1(cd: *const usize, i: usize, obj: usize) -> usize {
    unsafe { f::<extern "C" fn(usize) -> usize>(tok(cd, i))(obj) }
}

fn m2(cd: *const usize, i: usize, obj: usize, a: usize) -> usize {
    unsafe { f::<extern "C" fn(usize, usize) -> usize>(tok(cd, i))(obj, a) }
}

fn so(i: usize) -> *const usize {
    let _ = i;
    unsafe { &raw const SOMObjectClassData as *const usize }
}

fn sc() -> *const usize {
    unsafe { &raw const SOMClassClassData as *const usize }
}

// SOMObject methods (release order).
const SOM_FREE: usize = 2;
const SOM_GET_CLASS_NAME: usize = 4;
const SOM_GET_CLASS: usize = 5;
const SOM_IS_A: usize = 6;
const SOM_RESPONDS_TO: usize = 7;
const SOM_IS_INSTANCE_OF: usize = 8;
const SOM_GET_SIZE: usize = 9;
const SOM_CAST_OBJ: usize = 20;
const SOM_RESET_OBJ: usize = 21;
const SOM_DEFAULT_INIT: usize = 22;
// SOMClass methods.
const SOM_NEW: usize = 0;
const SOM_GET_NAME: usize = 4;
const SOM_DESCENDED_FROM: usize = 6;
const SOM_GET_PARENTS: usize = 38;
const SOM_DEALLOCATE: usize = 47;
const SOM_GET_INSTANCE_SIZE: usize = 12;
const SOM_GET_INSTANCE_OFFSET: usize = 13;
const SOM_GET_INSTANCE_PART_SIZE: usize = 14;
// IBM's private SOMClass methods (somcPrivate8-11 in the Toolkit).
const SOM_GET_TRUE_CLASS: usize = 58;
const SOM_GET_CASTED_CLASS: usize = 59;
// SOMClassMgr methods.
const SOM_FIND_CLASS: usize = 1;
const SOM_CLASS_FROM_ID: usize = 2;
const SOM_UNREGISTER_CLASS: usize = 4;
const SOM_UNLOAD_CLASS_FILE: usize = 7;
const SOM_GET_RELATED_CLASSES: usize = 10;
const SOM_GET_REGISTERED_CLASSES: usize = 14;
const SOM_BEGIN_PERSISTENT_CLASSES: usize = 15;
const SOM_END_PERSISTENT_CLASSES: usize = 16;
const SOM_JOIN_AFFINITY_GROUP: usize = 20;

// ---- Building classes as emitted code does ----

fn sid(name: &[u8]) -> SomId {
    let mut v = name.to_vec();
    v.push(0);
    let p = Box::leak(v.into_boxed_slice()).as_ptr();
    Box::leak(Box::new(p))
}

struct Spec<'a> {
    name: &'a [u8],
    meta: Option<&'a [u8]>,
    parents: &'a [&'a [u8]],
    data: u32,
    methods: &'a [(&'a [u8], usize)],
    overrides: &'a [(&'a [u8], usize)],
    cd: *mut usize,
    ccd: *mut usize,
}

/// somStaticClassInfo (layout version 4, as OS/2 Warp 4's compiler
/// emits it).
fn build(s: &Spec) -> usize {
    // inherit_vars: implementation (data and methods) from every parent.
    build_with(s, (1i32 << s.parents.len()) - 1)
}

fn build_with(s: &Spec, inherit: i32) -> usize {
    let mut smt: Vec<[usize; 6]> = Vec::new();
    for (i, &(n, p)) in s.methods.iter().enumerate() {
        let id: SomId = sid(n);
        // The descriptor ("Class::method"): IBM's SOM requires one.
        let mut d = s.name.to_vec();
        d.extend_from_slice(b"::");
        d.extend_from_slice(n);
        let desc: SomId = sid(&d);
        smt.push([
            s.cd as usize + 4 * (i + 1),
            Box::leak(Box::new(id)) as *mut SomId as usize,
            Box::leak(Box::new(desc)) as *mut SomId as usize,
            p,
            usize::MAX,
            0,
        ]);
    }
    let mut omt: Vec<[usize; 2]> = Vec::new();
    for &(n, p) in s.overrides {
        let id: SomId = sid(n);
        omt.push([Box::leak(Box::new(id)) as *mut SomId as usize, p]);
    }
    let parents: Vec<SomId> = s.parents.iter().map(|p| sid(p)).collect();
    let mut sci = vec![0u32; 48];
    sci[0] = 4;
    sci[1] = s.methods.len() as u32;
    sci[2] = s.overrides.len() as u32;
    sci[6] = 1; // major
    sci[7] = 2; // minor
    sci[8] = s.data;
    sci[9] = s.methods.len() as u32;
    sci[10] = parents.len() as u32;
    sci[11] = sid(s.name) as u32;
    sci[12] = s.meta.map_or(0, |m| sid(m) as u32);
    sci[14] = Box::leak(parents.into_boxed_slice()).as_ptr() as u32;
    sci[15] = s.cd as u32;
    sci[16] = s.ccd as u32;
    sci[17] = Box::leak(smt.into_boxed_slice()).as_ptr() as u32;
    sci[18] = Box::leak(omt.into_boxed_slice()).as_ptr() as u32;
    sci[24] = 4; // dataAlignment
    sci[25] = u32::MAX; // numDirectInitClasses: the parents
    sci[33] = s.methods.len() as u32; // numClassDataEntries
    let sci = Box::leak(sci.into_boxed_slice());
    unsafe { somBuildClass(inherit, sci.as_ptr(), 1, 2) }
}

fn cd(n: usize) -> *mut usize {
    Box::leak(vec![0usize; 1 + n].into_boxed_slice()).as_mut_ptr()
}

// ---- The test classes ----
//
// Animal(SOMObject; metaclass M_Animal): legs; setLegs, getLegs, speak;
//   emitted-style somDefaultInit (legs = 4) and somDestruct.
// Dog(Animal): barks; bark; speak overridden (100 + Animal's), legacy
//   somInit (barks = 7) and somUninit.
// Pet(SOMObject): tag; petTag (1000 + tag); legacy somInit (tag = 5).
// PetDog(Dog, Pet): multiple inheritance, SOMObject reached twice.
// M_Animal(SOMClass): made; makeOne (somNew, counting).
// M_Mixin(SOMClass): mixinHello (42).
// Mixy(Dog; metaclass M_Mixin): a metaclass derived from M_Mixin and
//   M_Animal.

static mut THING: *mut usize = core::ptr::null_mut();
static mut THING_C: *mut usize = core::ptr::null_mut();
static mut ANIMAL: *mut usize = core::ptr::null_mut();
static mut ANIMAL_C: *mut usize = core::ptr::null_mut();
static mut DOG: *mut usize = core::ptr::null_mut();
static mut DOG_C: *mut usize = core::ptr::null_mut();
static mut PET: *mut usize = core::ptr::null_mut();
static mut PET_C: *mut usize = core::ptr::null_mut();
static mut PETDOG: *mut usize = core::ptr::null_mut();
static mut PETDOG_C: *mut usize = core::ptr::null_mut();
static mut M_ANIMAL: *mut usize = core::ptr::null_mut();
static mut M_ANIMAL_C: *mut usize = core::ptr::null_mut();
static mut M_MIXIN: *mut usize = core::ptr::null_mut();
static mut M_MIXIN_C: *mut usize = core::ptr::null_mut();
static mut MIXY: *mut usize = core::ptr::null_mut();
static mut MIXY_C: *mut usize = core::ptr::null_mut();

static mut LOG: Vec<u8> = Vec::new();

fn log(c: u8) {
    unsafe { LOG.push(c) };
}

/// The instance data of a class in an object (through its CClassData's
/// data token, as emitted <Class>GetData does).
fn data(ccd: *mut usize, obj: usize) -> *mut i32 {
    unsafe { f::<extern "C" fn(usize) -> *mut i32>(*ccd.add(1))(obj) }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Ctrl {
    mask: *const u8,
    info: usize,
    info_size: i32,
    ctrl_info: usize,
}

/// Emitted somDefaultInit: begin (copy the class's control if none),
/// call the parent's initializer as the mask says, then the body.
extern "C" fn animal_init(obj: usize, ctrl: *mut Ctrl) {
    unsafe {
        let mut local: Ctrl = core::mem::zeroed();
        let ctrl = if ctrl.is_null() {
            local = *((*ANIMAL_C as usize + 0x1c) as *const Ctrl);
            &mut local as *mut Ctrl
        } else {
            ctrl
        };
        let mask = (*ctrl).mask;
        (*ctrl).info += (*ctrl).info_size as usize;
        (*ctrl).mask = mask.add(1);
        if *mask & 1 != 0 {
            let next = (*ctrl).info as *const usize;
            f::<extern "C" fn(usize, *mut Ctrl)>(*next.add(1))(obj, ctrl);
        }
        *data(ANIMAL_C, obj) = 4;
        log(b'A');
    }
}

/// Emitted somDestruct: body, parent's destructor, then free.
extern "C" fn animal_destruct(obj: usize, do_free: u32, ctrl: *mut Ctrl) {
    unsafe {
        let mut local: Ctrl = core::mem::zeroed();
        let ctrl = if ctrl.is_null() {
            local = *((*ANIMAL_C as usize + 0x2c) as *const Ctrl);
            &mut local as *mut Ctrl
        } else {
            ctrl
        };
        log(b'a');
        let mask = (*ctrl).mask;
        (*ctrl).info += (*ctrl).info_size as usize;
        (*ctrl).mask = mask.add(1);
        if *mask & 1 != 0 {
            let next = (*ctrl).info as *const usize;
            f::<extern "C" fn(usize, u32, *mut Ctrl)>(*next.add(1))(obj, 0, ctrl);
        }
        if do_free & 0xff != 0 {
            m2(sc(), SOM_DEALLOCATE, *ANIMAL, obj);
        }
    }
}

extern "C" fn animal_set_legs(obj: usize, n: i32) {
    unsafe { *data(ANIMAL_C, obj) = n };
}
extern "C" fn animal_get_legs(obj: usize) -> i32 {
    unsafe { *data(ANIMAL_C, obj) }
}
extern "C" fn animal_speak(_obj: usize) -> i32 {
    1
}
extern "C" fn dog_speak(obj: usize) -> i32 {
    // parent_speak: Animal's implementation through Dog's parent mtabs.
    let p = unsafe { somParentNumResolve(*DOG_C, 1, tok(ANIMAL, 2)) };
    100 + unsafe { f::<extern "C" fn(usize) -> i32>(p)(obj) }
}
extern "C" fn dog_bark(obj: usize) -> i32 {
    unsafe { *data(DOG_C, obj) }
}
extern "C" fn dog_init(obj: usize) {
    unsafe { *data(DOG_C, obj) = 7 };
    log(b'D');
}
extern "C" fn dog_uninit(_obj: usize) {
    log(b'd');
}
extern "C" fn thing_init(_obj: usize) {
    log(b'T');
}
extern "C" fn thing_uninit(_obj: usize) {
    log(b't');
}
extern "C" fn pet_tag(obj: usize) -> i32 {
    1000 + unsafe { *data(PET_C, obj) }
}
extern "C" fn pet_init(obj: usize) {
    unsafe { *data(PET_C, obj) = 5 };
    log(b'P');
}
extern "C" fn pet_uninit(_obj: usize) {
    log(b'p');
}
extern "C" fn m_animal_make_one(cls: usize) -> usize {
    unsafe { *data(M_ANIMAL_C, cls) += 1 };
    m1(sc(), SOM_NEW, cls)
}
extern "C" fn m_mixin_hello(_cls: usize) -> i32 {
    42
}

static mut ERRS: Vec<i32> = Vec::new();

/// A SOMError that records the error and returns.
extern "C" fn err_record(code: i32, _file: *const u8, _line: i32) {
    unsafe { ERRS.push(code) };
}
extern "C" fn abs_speak(_obj: usize) -> i32 {
    55
}

aligned! {
    __t_err_record => err_record, 3;
    __t_abs_speak => abs_speak, 1;
    __t_animal_init => animal_init, 2;
    __t_animal_destruct => animal_destruct, 3;
    __t_animal_set_legs => animal_set_legs, 2;
    __t_animal_get_legs => animal_get_legs, 1;
    __t_animal_speak => animal_speak, 1;
    __t_dog_speak => dog_speak, 1;
    __t_dog_bark => dog_bark, 1;
    __t_dog_init => dog_init, 1;
    __t_dog_uninit => dog_uninit, 1;
    __t_thing_init => thing_init, 1;
    __t_thing_uninit => thing_uninit, 1;
    __t_pet_tag => pet_tag, 1;
    __t_pet_init => pet_init, 1;
    __t_pet_uninit => pet_uninit, 1;
    __t_m_animal_make_one => m_animal_make_one, 1;
    __t_m_mixin_hello => m_mixin_hello, 1;
}

fn build_all() {
    unsafe {
        SOMObjectNewClass(1, 4);
        SOMClassNewClass(1, 4);
        (M_ANIMAL, M_ANIMAL_C) = (cd(1), cd(8));
        build(&Spec {
            name: b"M_Animal",
            meta: None,
            parents: &[b"SOMClass"],
            data: 4,
            methods: &[(b"makeOne", __t_m_animal_make_one as usize)],
            overrides: &[],
            cd: M_ANIMAL,
            ccd: M_ANIMAL_C,
        });
        (THING, THING_C) = (cd(0), cd(8));
        build(&Spec {
            name: b"Thing",
            meta: None,
            parents: &[b"SOMObject"],
            data: 4,
            methods: &[],
            overrides: &[(b"somInit", __t_thing_init as usize), (b"somUninit", __t_thing_uninit as usize)],
            cd: THING,
            ccd: THING_C,
        });
        (ANIMAL, ANIMAL_C) = (cd(3), cd(8));
        build(&Spec {
            name: b"Animal",
            meta: Some(b"M_Animal"),
            parents: &[b"Thing"],
            data: 4,
            methods: &[
                (b"setLegs", __t_animal_set_legs as usize),
                (b"getLegs", __t_animal_get_legs as usize),
                (b"speak", __t_animal_speak as usize),
            ],
            overrides: &[(b"somDefaultInit", __t_animal_init as usize), (b"somDestruct", __t_animal_destruct as usize)],
            cd: ANIMAL,
            ccd: ANIMAL_C,
        });
        (DOG, DOG_C) = (cd(1), cd(8));
        build(&Spec {
            name: b"Dog",
            meta: None,
            parents: &[b"Animal"],
            data: 4,
            methods: &[(b"bark", __t_dog_bark as usize)],
            overrides: &[
                (b"speak", __t_dog_speak as usize),
                (b"somInit", __t_dog_init as usize),
                (b"somUninit", __t_dog_uninit as usize),
            ],
            cd: DOG,
            ccd: DOG_C,
        });
        (PET, PET_C) = (cd(1), cd(8));
        build(&Spec {
            name: b"Pet",
            meta: None,
            parents: &[b"Thing"],
            data: 4,
            methods: &[(b"petTag", __t_pet_tag as usize)],
            overrides: &[(b"somInit", __t_pet_init as usize), (b"somUninit", __t_pet_uninit as usize)],
            cd: PET,
            ccd: PET_C,
        });
        (PETDOG, PETDOG_C) = (cd(0), cd(8));
        build(&Spec {
            name: b"PetDog",
            meta: None,
            parents: &[b"Dog", b"Pet"],
            data: 0,
            methods: &[],
            overrides: &[],
            cd: PETDOG,
            ccd: PETDOG_C,
        });
        (M_MIXIN, M_MIXIN_C) = (cd(1), cd(8));
        build(&Spec {
            name: b"M_Mixin",
            meta: None,
            parents: &[b"SOMClass"],
            data: 0,
            methods: &[(b"mixinHello", __t_m_mixin_hello as usize)],
            overrides: &[],
            cd: M_MIXIN,
            ccd: M_MIXIN_C,
        });
        (MIXY, MIXY_C) = (cd(0), cd(8));
        build(&Spec {
            name: b"Mixy",
            meta: Some(b"M_Mixin"),
            parents: &[b"Dog"],
            data: 0,
            methods: &[],
            overrides: &[],
            cd: MIXY,
            ccd: MIXY_C,
        });
    }
}

// ---- Checks ----

static mut FAILS: u32 = 0;

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS {}", name);
    } else {
        println!("FAIL {}", name);
        unsafe { FAILS += 1 };
    }
}

fn check_eq(name: &str, got: i64, want: i64) {
    if got == want {
        println!("PASS {} = {}", name, got);
    } else {
        println!("FAIL {}: {} (expected {})", name, got, want);
        unsafe { FAILS += 1 };
    }
}

fn cstr(p: *const u8) -> &'static [u8] {
    if p.is_null() {
        return b"(null)";
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

fn check_log(name: &str, want: &[u8]) {
    let got = take_log();
    if got == want {
        println!("PASS {} {}", name, core::str::from_utf8(&got).unwrap_or("?"));
    } else {
        println!("FAIL {}: {} (expected {})", name, core::str::from_utf8(&got).unwrap_or("?"), core::str::from_utf8(want).unwrap_or("?"));
        unsafe { FAILS += 1 };
    }
}

fn take_log() -> Vec<u8> {
    unsafe { core::mem::take(&mut LOG) }
}

fn run() {
    unsafe {
        let mgr = somEnvironmentNew();
        check("environment", mgr != 0 && mgr == SOMClassMgrObject);
        build_all();
        let (animal, dog, pet, petdog, m_animal, mixy) = (*ANIMAL, *DOG, *PET, *PETDOG, *M_ANIMAL, *MIXY);
        check("classes built", animal != 0 && dog != 0 && pet != 0 && petdog != 0 && m_animal != 0 && mixy != 0);
        check("class names", cstr(m1(sc(), SOM_GET_NAME, dog) as *const u8) == b"Dog");

        // Instances through the metaclass's class method.
        let a = m1(M_ANIMAL, 0, animal);
        check("Animal made by M_Animal::makeOne", a != 0 && somIsObj(a) != 0);
        check_eq("M_Animal data (instances made)", *data(M_ANIMAL_C, animal) as i64, 1);
        check_log("Animal init order", b"TA");
        check_eq("Animal legs after somDefaultInit", m1(ANIMAL, 1, a) as i64, 4);
        m2(ANIMAL, 0, a, 6);
        check_eq("setLegs/getLegs", m1(ANIMAL, 1, a) as i64, 6);
        check_eq("Animal speak", m1(ANIMAL, 2, a) as i64, 1);
        check("somGetClassName", cstr(m1(so(0), SOM_GET_CLASS_NAME, a) as *const u8) == b"Animal");
        check("somGetClass", m1(so(0), SOM_GET_CLASS, a) == animal);

        // A subclass: inherited data and methods, an override with a
        // parent call, a legacy somInit after Animal's initializer.
        let d = m1(sc(), SOM_NEW, dog);
        check_log("Dog init order", b"TAD");
        check_eq("Dog legs (inherited initializer)", m1(ANIMAL, 1, d) as i64, 4);
        check_eq("Dog bark (legacy somInit)", m1(DOG, 0, d) as i64, 7);
        check_eq("Dog speak (override + parent call)", m1(ANIMAL, 2, d) as i64, 101);
        check("Dog isA Animal", m2(so(0), SOM_IS_A, d, animal) & 0xff != 0);
        check("Animal is not a Dog", m2(so(0), SOM_IS_A, a, dog) & 0xff == 0);
        check("Dog isInstanceOf Dog", m2(so(0), SOM_IS_INSTANCE_OF, d, dog) & 0xff != 0);
        check("Dog descended from Animal", m2(sc(), SOM_DESCENDED_FROM, dog, animal) & 0xff != 0);
        check_eq("Dog size", m1(so(0), SOM_GET_SIZE, d) as i64, 16);
        check_eq("Dog instance part size", m1(sc(), SOM_GET_INSTANCE_PART_SIZE, dog) as i64, 4);
        check_eq("Dog's metaclass is M_Animal", (m1(so(0), SOM_GET_CLASS, dog) == m_animal) as i64, 1);
        check("somResolve", somResolve(d, tok(ANIMAL, 2)) == __t_dog_speak as usize);
        check("somGetClassFromMToken", somGetClassFromMToken(tok(ANIMAL, 2)) == animal);

        // Ids: registered in place, without regard to case.
        let i1 = somIdFromString(b"speak\0".as_ptr());
        let i2 = somIdFromString(b"SPEAK\0".as_ptr());
        check("ids compare without case", somCompareIds(i1, i2) != 0);
        check("somStringFromId", cstr(somStringFromId(i1)) == b"speak");
        check("somRespondsTo", m2(so(0), SOM_RESPONDS_TO, d, i1 as usize) & 0xff != 0);

        // By name.
        let p = somResolveByName(d, b"getLegs\0".as_ptr());
        check_eq("somResolveByName", if p == 0 { -1 } else { f::<extern "C" fn(usize) -> i32>(p)(d) as i64 }, 4);
        let mut ret = 0usize;
        let disp: extern "C" fn(usize, *mut usize, SomId, usize) -> u32 = core::mem::transmute(somva_dispatch_import as usize);
        let ok = disp(d, &mut ret, i1, d);
        check_eq("somva_SOMObject_somDispatch speak", if ok & 0xff != 0 { ret as i64 } else { -1 }, 101);

        // The class manager.
        let mgrf: extern "C" fn(usize, SomId, i32, i32) -> usize = f(tok(&raw const SOMClassMgrClassData as *const usize, SOM_FIND_CLASS));
        check("somFindClass Dog", mgrf(SOMClassMgrObject, sid(b"Dog"), 0, 0) == dog);
        let fromid: extern "C" fn(usize, SomId) -> usize = f(tok(&raw const SOMClassMgrClassData as *const usize, SOM_CLASS_FROM_ID));
        check("somClassFromId pet", fromid(SOMClassMgrObject, sid(b"pet")) == pet);

        // Multiple inheritance: Pet's methods and data through the slow
        // paths, both parents' initializers once, SOMObject's last when
        // destroying.
        let pd = m1(sc(), SOM_NEW, petdog);
        check_log("PetDog init order", b"TADP");
        check_eq("PetDog petTag (second parent)", m1(PET, 0, pd) as i64, 1005);
        check_eq("PetDog speak (first parent)", m1(ANIMAL, 2, pd) as i64, 101);
        check_eq("PetDog Pet data via somDataResolve", *(somDataResolve(pd, *PET_C.add(1)) as *const i32) as i64, 5);
        check("PetDog isA Pet", m2(so(0), SOM_IS_A, pd, pet) & 0xff != 0);
        #[repr(C)]
        struct Seq {
            max: u32,
            len: u32,
            buf: *const usize,
        }
        let mut seq = Seq { max: 0, len: 0, buf: core::ptr::null() };
        f::<extern "C" fn(*mut Seq, usize) -> *mut Seq>(tok(sc(), SOM_GET_PARENTS))(&mut seq, petdog);
        check(
            "somGetParents PetDog",
            seq.len == 2 && *seq.buf == dog && *seq.buf.add(1) == pet,
        );
        m1(so(0), SOM_FREE, pd);
        check_log("PetDog destruct order", b"pdat");
        m1(so(0), SOM_FREE, d);
        check_log("Dog destruct order", b"dat");

        // A derived metaclass: Mixy's class responds to both metaclasses.
        let meta = m1(so(0), SOM_GET_CLASS, mixy);
        check("Mixy's metaclass is derived", meta != m_animal && meta != *M_MIXIN);
        check_eq("Mixy mixinHello (M_Mixin)", m1(M_MIXIN, 0, mixy) as i64, 42);
        let x = m1(M_ANIMAL, 0, mixy);
        check("Mixy makeOne (M_Animal)", x != 0 && m1(so(0), SOM_GET_CLASS, x) == mixy);
        take_log();
        check_eq("Mixy inherits Dog", m1(ANIMAL, 2, x) as i64, 101);

        abstract_checks();
        cast_checks();
        unregister_checks();

        // Plain default initialization from somNew.
        let dinit: extern "C" fn(usize, usize) = f(tok(so(0), SOM_DEFAULT_INIT));
        let _ = dinit;
        let mem = (SOMMalloc)(64);
        check("SOMMalloc", !mem.is_null());
        (SOMFree)(mem);
    }
}

/// The errors SOMError was called with since the last call.
fn take_errs() -> Vec<i32> {
    unsafe { core::mem::take(&mut ERRS) }
}

fn check_errs(name: &str, want: &[i32]) {
    let got = take_errs();
    if got == want {
        println!("PASS {} errors {:?}", name, got);
    } else {
        println!("FAIL {}: errors {:?} (expected {:?})", name, got, want);
        unsafe { FAILS += 1 };
    }
}

fn size_of_class(c: usize) -> i64 {
    m1(sc(), SOM_GET_INSTANCE_SIZE, c) as i64
}

/// A class with Dog's and Pet's interfaces, built with `inherit` as its
/// inherit_vars (a bit per parent whose implementation it inherits).
fn build_abstract(name: &[u8], parents: &[&[u8]], inherit: i32, cd: *mut usize, ccd: *mut usize) -> usize {
    build_with(
        &Spec {
            name,
            meta: None,
            parents,
            data: 4,
            methods: &[(b"absM", __t_abs_speak as usize)],
            overrides: &[(b"speak", __t_abs_speak as usize)],
            cd,
            ccd,
        },
        inherit,
    )
}

/// Abstract inheritance: a parent without its inherit_vars bit gives
/// only its interface (IBM's SOM: none of its data, its methods raise
/// SOMError 20059, its initializers and destructor are not run).
fn abstract_checks() {
    unsafe {
        let old = SOMError;
        SOMError = __t_err_record as usize;
        take_errs();

        let (c, cc) = (cd(1), cd(8));
        let cls = build_abstract(b"AbsDog", &[b"Dog"], 0, c, cc);
        check("AbsDog (Dog's interface only) built", cls != 0);
        check_eq("AbsDog size (no inherited data)", size_of_class(cls), 8);
        take_log();
        let o = m1(sc(), SOM_NEW, cls);
        check_log("AbsDog init (no ancestor initializers)", b"");
        check_eq("AbsDog data offset", data(cc, o) as i64 - o as i64, 4);
        check_eq("AbsDog speak (its override)", m1(ANIMAL, 2, o) as i32 as i64, 55);
        check_errs("AbsDog speak", &[]);
        m1(ANIMAL, 1, o);
        check_errs("AbsDog getLegs (abstract)", &[20059]);
        m1(so(0), SOM_GET_SIZE, o);
        check_errs("AbsDog somGetSize (abstract)", &[20059]);
        somDataResolve(o, *ANIMAL_C.add(1));
        check_errs("AbsDog Animal data (not inherited)", &[20109, 20109]);
        somDataResolve(o, *PET_C.add(1));
        check_errs("AbsDog Pet data (not an ancestor)", &[20109]);
        check_eq("AbsDog absM", m1(c, 0, o) as i32 as i64, 55);
        m2(so(0), SOM_IS_A, o, *ANIMAL);
        check_errs("AbsDog somIsA (SOMObject's methods are abstract too)", &[20059]);

        let (c, cc) = (cd(1), cd(8));
        let cls = build_abstract(b"HalfPD1", &[b"Dog", b"Pet"], 1, c, cc);
        check_eq("HalfPD1 (Dog's implementation, Pet's interface) size", size_of_class(cls), 20);
        take_log();
        let o = m1(sc(), SOM_NEW, cls);
        check_log("HalfPD1 init", b"TAD");
        check_eq("HalfPD1 data offset", data(cc, o) as i64 - o as i64, 16);
        check_eq("HalfPD1 getLegs", m1(ANIMAL, 1, o) as i64, 4);
        m1(PET, 0, o);
        check_errs("HalfPD1 petTag (abstract)", &[20059]);
        somDataResolve(o, *PET_C.add(1));
        check_errs("HalfPD1 Pet data", &[20109, 20109]);
        m1(so(0), SOM_FREE, o);
        check_log("HalfPD1 destruct", b"dat");

        let (c, cc) = (cd(1), cd(8));
        let cls = build_abstract(b"HalfPD2", &[b"Dog", b"Pet"], 2, c, cc);
        check_eq("HalfPD2 (Pet's implementation, Dog's interface) size", size_of_class(cls), 16);
        take_log();
        let o = m1(sc(), SOM_NEW, cls);
        check_log("HalfPD2 init", b"TP");
        check_eq("HalfPD2 data offset", data(cc, o) as i64 - o as i64, 4);
        check_eq("HalfPD2 Pet data offset", somDataResolve(o, *PET_C.add(1)) as i64 - o as i64, 12);
        check_eq("HalfPD2 petTag", m1(PET, 0, o) as i64, 1005);
        m1(DOG, 0, o);
        check_errs("HalfPD2 bark (abstract)", &[20059]);
        m1(so(0), SOM_FREE, o);
        check_log("HalfPD2 destruct", b"pt");

        // Overriding a method the class does not have fails the class.
        let cls = build_abstract(b"NoSpeak", &[b"SOMObject"], 1, cd(1), cd(8));
        check("override of an unknown method fails the class", cls == 0);
        check_errs("override of an unknown method", &[20169]);

        // Instance sizes are not rounded up; somGetInstanceOffset is 0.
        let cls = build(&Spec { name: b"Odd3", meta: None, parents: &[b"Thing"], data: 3, methods: &[], overrides: &[], cd: cd(0), ccd: cd(8) });
        check_eq("instance size with 3 bytes of data", size_of_class(cls), 11);
        check_eq("somGetInstanceOffset", m1(sc(), SOM_GET_INSTANCE_OFFSET, *DOG) as i64, 0);
        SOMError = old;
    }
}

/// somCastObj and somResetObj: an object cast to an ancestor gets a cast
/// class ("Dog->Animal", made once per ancestor, not registered) whose
/// methods are the ancestor's.
fn cast_checks() {
    unsafe {
        let name = |o: usize| cstr(m1(so(0), SOM_GET_CLASS_NAME, o) as *const u8);
        let (dog, animal) = (*DOG, *ANIMAL);
        let d = m1(sc(), SOM_NEW, dog);
        check("Dog's true class is Dog", m1(sc(), SOM_GET_TRUE_CLASS, dog) == dog && m1(sc(), SOM_GET_CASTED_CLASS, dog) == 0);
        check("somCastObj Dog to Animal", m2(so(0), SOM_CAST_OBJ, d, animal) & 0xff != 0);
        let c1 = m1(so(0), SOM_GET_CLASS, d);
        check("cast object's class name", name(d) == b"Dog->Animal" && cstr(m1(sc(), SOM_GET_NAME, c1) as *const u8) == b"Dog->Animal");
        check_eq("cast object's speak (Animal's)", m1(ANIMAL, 2, d) as i64, 1);
        check_eq("cast object's bark (Dog's)", m1(DOG, 0, d) as i64, 7);
        check_eq("cast object's legs", m1(ANIMAL, 1, d) as i64, 4);
        check("cast class: true class Dog, casted class Animal", m1(sc(), SOM_GET_TRUE_CLASS, c1) == dog && m1(sc(), SOM_GET_CASTED_CLASS, c1) == animal);
        check("cast object isA Dog, not an instance of Dog", m2(so(0), SOM_IS_A, d, dog) & 0xff != 0 && m2(so(0), SOM_IS_INSTANCE_OF, d, dog) & 0xff == 0);
        check("cast class's metaclass", m1(so(0), SOM_GET_CLASS, c1) == m1(so(0), SOM_GET_CLASS, dog));
        let fromid: extern "C" fn(usize, SomId) -> usize = f(tok(&raw const SOMClassMgrClassData as *const usize, SOM_CLASS_FROM_ID));
        check("cast class not registered", fromid(SOMClassMgrObject, sid(b"Dog->Animal")) == 0);
        check("somCastObj to a class not an ancestor fails", m2(so(0), SOM_CAST_OBJ, d, *PET) & 0xff == 0 && name(d) == b"Dog->Animal");
        check("somCastObj to Thing", m2(so(0), SOM_CAST_OBJ, d, *THING) & 0xff != 0 && name(d) == b"Dog->Thing");
        check_eq("Dog->Thing speak (Dog's)", m1(ANIMAL, 2, d) as i64, 101);
        check("cast classes are reused", m2(so(0), SOM_CAST_OBJ, d, animal) & 0xff != 0 && m1(so(0), SOM_GET_CLASS, d) == c1);
        check("somResetObj", m1(so(0), SOM_RESET_OBJ, d) & 0xff != 0 && m1(so(0), SOM_GET_CLASS, d) == dog);
        check_eq("reset object's speak", m1(ANIMAL, 2, d) as i64, 101);
        m2(so(0), SOM_CAST_OBJ, d, animal);
        check("somCastObj to the true class resets", m2(so(0), SOM_CAST_OBJ, d, dog) & 0xff != 0 && m1(so(0), SOM_GET_CLASS, d) == dog);
        let pd = m1(sc(), SOM_NEW, *PETDOG);
        check("somCastObj PetDog to Pet (second parent)", m2(so(0), SOM_CAST_OBJ, pd, *PET) & 0xff != 0 && name(pd) == b"PetDog->Pet");
        check_eq("PetDog->Pet petTag", m1(PET, 0, pd) as i64, 1005);
        check_eq("PetDog->Pet speak (PetDog's)", m1(ANIMAL, 2, pd) as i64, 101);
        take_log();
        m1(so(0), SOM_FREE, pd);
        check_log("cast PetDog destruct", b"pdat");
        m1(so(0), SOM_FREE, d);
        take_log();
    }
}

fn mgr_tok(i: usize) -> usize {
    tok(&raw const SOMClassMgrClassData as *const usize, i)
}

fn mgr1(i: usize, a: usize) -> usize {
    unsafe { f::<extern "C" fn(usize, usize) -> usize>(mgr_tok(i))(SOMClassMgrObject, a) }
}

fn class_from_id(name: &[u8]) -> usize {
    unsafe { f::<extern "C" fn(usize, SomId) -> usize>(mgr_tok(SOM_CLASS_FROM_ID))(SOMClassMgrObject, sid(name)) }
}

/// somGetRelatedClasses as a list (None: NULL).
fn related(cls: usize) -> Option<Vec<usize>> {
    let p = mgr1(SOM_GET_RELATED_CLASSES, cls) as *mut usize;
    if p.is_null() {
        return None;
    }
    let mut v = Vec::new();
    let mut i = 0;
    while unsafe { *p.add(i) } != 0 {
        v.push(unsafe { *p.add(i) });
        i += 1;
    }
    unsafe { (SOMFree)(p as *mut u8) };
    Some(v)
}

fn simple(name: &[u8], parent: &[u8], c: *mut usize) -> usize {
    build(&Spec { name, meta: None, parents: &[parent], data: 4, methods: &[], overrides: &[], cd: c, ccd: cd(8) })
}

/// Unregistering classes: the class object goes and its ClassData is
/// cleared (so it can be built again); classes with registered
/// subclasses or instances that are classes, and persistent ones, stay.
fn unregister_checks() {
    unsafe {
        let mgr = SOMClassMgrObject;
        let tc = cd(0);
        let t = simple(b"Temp", b"Thing", tc);
        check("Temp built and registered", t != 0 && class_from_id(b"Temp") == t);
        check_eq("somUnregisterClass of a class with subclasses", mgr1(SOM_UNREGISTER_CLASS, *THING) as i64, 1);
        check_eq("somUnregisterClass of a metaclass in use", mgr1(SOM_UNREGISTER_CLASS, *M_ANIMAL) as i64, 1);
        check("still registered", class_from_id(b"Thing") == *THING && class_from_id(b"M_Animal") == *M_ANIMAL);
        check("somGetRelatedClasses of a class not loaded from a file", related(t) == Some(vec![t]));
        let mut seq = [0usize; 3];
        f::<extern "C" fn(*mut usize, usize)>(mgr_tok(SOM_GET_REGISTERED_CLASSES))(seq.as_mut_ptr(), mgr);
        check("registered classes, newest first", seq[1] != 0 && *(seq[2] as *const usize) == t);
        (SOMFree)(seq[2] as *mut u8);
        check_eq("somUnregisterClass Temp", mgr1(SOM_UNREGISTER_CLASS, t) as i64, 0);
        check("Temp unregistered, its ClassData cleared", class_from_id(b"Temp") == 0 && *tc == 0);
        check("somGetRelatedClasses of a class not registered", related(t).is_none());
        check_eq("somUnregisterClass of NULL", mgr1(SOM_UNREGISTER_CLASS, 0) as i64, 0);
        let t2 = simple(b"Temp", b"Thing", tc);
        check("Temp built again", t2 != 0 && class_from_id(b"Temp") == t2 && *tc == t2);
        let o = m1(sc(), SOM_NEW, t2);
        check("an instance of it", o != 0 && m1(so(0), SOM_GET_CLASS, o) == t2);
        m1(so(0), SOM_FREE, o);
        check_eq("somUnloadClassFile of a class not loaded from a file", mgr1(SOM_UNLOAD_CLASS_FILE, t2) as i32 as i64, -2);
        check("it is unregistered", class_from_id(b"Temp") == 0 && *tc == 0);

        // A parent goes once its subclass has.
        let (bc, kc) = (cd(0), cd(0));
        let b = simple(b"TempBase", b"Thing", bc);
        let k = simple(b"TempKid", b"TempBase", kc);
        check_eq("unregister TempBase (TempKid registered)", mgr1(SOM_UNREGISTER_CLASS, b) as i64, 1);
        check_eq("unregister TempKid", mgr1(SOM_UNREGISTER_CLASS, k) as i64, 0);
        check_eq("unregister TempBase then", mgr1(SOM_UNREGISTER_CLASS, b) as i64, 0);
        check("both gone", class_from_id(b"TempBase") == 0 && class_from_id(b"TempKid") == 0 && *bc == 0 && *kc == 0);

        // Persistent classes stay.
        f::<extern "C" fn(usize)>(mgr_tok(SOM_BEGIN_PERSISTENT_CLASSES))(mgr);
        let p = simple(b"Pers", b"Thing", cd(0));
        f::<extern "C" fn(usize)>(mgr_tok(SOM_END_PERSISTENT_CLASSES))(mgr);
        check_eq("somUnregisterClass of a persistent class", mgr1(SOM_UNREGISTER_CLASS, p) as i64, 1);
        check_eq("somUnloadClassFile of a persistent class", mgr1(SOM_UNLOAD_CLASS_FILE, p) as i32 as i64, -1);
        check("persistent class registered", class_from_id(b"Pers") == p);
        let join: extern "C" fn(usize, usize, usize) -> u32 = f(mgr_tok(SOM_JOIN_AFFINITY_GROUP));
        check("somJoinAffinityGroup without a group", join(mgr, p, *DOG) & 0xff == 0);
    }
}

fn load(args: &[u8]) {
    let mut it = args.split(|&c| c == b' ').filter(|s| !s.is_empty());
    let _ = it.next();
    let (Some(dll), Some(class)) = (it.next(), it.next()) else {
        println!("usage: SOMTEST LOAD dll class");
        return;
    };
    unsafe {
        somEnvironmentNew();
        let mut dname = dll.to_vec();
        dname.push(0);
        let id = sid(class);
        let fic: extern "C" fn(usize, SomId, i32, i32, *const u8) -> usize =
            f(tok(&raw const SOMClassMgrClassData as *const usize, 0));
        let cls = fic(SOMClassMgrObject, id, 0, 0, dname.as_ptr());
        check("class loaded", cls != 0);
        if cls == 0 {
            return;
        }
        println!("class {} at {:08x}", core::str::from_utf8(cstr(m1(sc(), SOM_GET_NAME, cls) as *const u8)).unwrap_or("?"), cls);
        let o = m1(sc(), SOM_NEW, cls);
        check("instance made", o != 0 && somIsObj(o) != 0);
        if o != 0 {
            check("instance's class", m1(so(0), SOM_GET_CLASS, o) == cls);
            m1(so(0), SOM_FREE, o);
        }
    }
}

fn main(s: &Startup) -> u32 {
    let args = s.args();
    let word = args.split(|&c| c == b' ').find(|w| !w.is_empty()).unwrap_or(b"");
    if word.eq_ignore_ascii_case(b"LOAD") {
        load(args);

    } else {
        run();
    }
    let n = unsafe { FAILS };
    println!("somtest done: {} failure(s)", n);
    n
}
