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
const SOM_DEFAULT_INIT: usize = 22;
// SOMClass methods.
const SOM_NEW: usize = 0;
const SOM_GET_NAME: usize = 4;
const SOM_DESCENDED_FROM: usize = 6;
const SOM_GET_PARENTS: usize = 38;
const SOM_DEALLOCATE: usize = 47;
const SOM_GET_INSTANCE_PART_SIZE: usize = 14;
// SOMClassMgr methods.
const SOM_FIND_CLASS: usize = 1;
const SOM_CLASS_FROM_ID: usize = 2;

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
    let mut smt: Vec<[usize; 6]> = Vec::new();
    for (i, &(n, p)) in s.methods.iter().enumerate() {
        let id: SomId = sid(n);
        smt.push([s.cd as usize + 4 * (i + 1), Box::leak(Box::new(id)) as *mut SomId as usize, 0, p, usize::MAX, 0]);
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
    unsafe { somBuildClass(1, sci.as_ptr(), 1, 2) }
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

aligned! {
    __t_animal_init => animal_init, 2;
    __t_animal_destruct => animal_destruct, 3;
    __t_animal_set_legs => animal_set_legs, 2;
    __t_animal_get_legs => animal_get_legs, 1;
    __t_animal_speak => animal_speak, 1;
    __t_dog_speak => dog_speak, 1;
    __t_dog_bark => dog_bark, 1;
    __t_dog_init => dog_init, 1;
    __t_dog_uninit => dog_uninit, 1;
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
        (ANIMAL, ANIMAL_C) = (cd(3), cd(8));
        build(&Spec {
            name: b"Animal",
            meta: Some(b"M_Animal"),
            parents: &[b"SOMObject"],
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
            parents: &[b"SOMObject"],
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
        check_eq("Animal init log", take_log().first().copied().unwrap_or(0) as i64, b'A' as i64);
        check_eq("Animal legs after somDefaultInit", m1(ANIMAL, 1, a) as i64, 4);
        m2(ANIMAL, 0, a, 6);
        check_eq("setLegs/getLegs", m1(ANIMAL, 1, a) as i64, 6);
        check_eq("Animal speak", m1(ANIMAL, 2, a) as i64, 1);
        check("somGetClassName", cstr(m1(so(0), SOM_GET_CLASS_NAME, a) as *const u8) == b"Animal");
        check("somGetClass", m1(so(0), SOM_GET_CLASS, a) == animal);

        // A subclass: inherited data and methods, an override with a
        // parent call, a legacy somInit after Animal's initializer.
        let d = m1(sc(), SOM_NEW, dog);
        check("Dog init order", take_log() == b"AD");
        check_eq("Dog legs (inherited initializer)", m1(ANIMAL, 1, d) as i64, 4);
        check_eq("Dog bark (legacy somInit)", m1(DOG, 0, d) as i64, 7);
        check_eq("Dog speak (override + parent call)", m1(ANIMAL, 2, d) as i64, 101);
        check("Dog isA Animal", m2(so(0), SOM_IS_A, d, animal) & 0xff != 0);
        check("Animal is not a Dog", m2(so(0), SOM_IS_A, a, dog) & 0xff == 0);
        check("Dog isInstanceOf Dog", m2(so(0), SOM_IS_INSTANCE_OF, d, dog) & 0xff != 0);
        check("Dog descended from Animal", m2(sc(), SOM_DESCENDED_FROM, dog, animal) & 0xff != 0);
        check_eq("Dog size", m1(so(0), SOM_GET_SIZE, d) as i64, 12);
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
        check("PetDog init order", take_log() == b"ADP");
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
        check("PetDog destruct order", take_log() == b"dap");
        m1(so(0), SOM_FREE, d);
        check("Dog destruct order", take_log() == b"da");

        // A derived metaclass: Mixy's class responds to both metaclasses.
        let meta = m1(so(0), SOM_GET_CLASS, mixy);
        check("Mixy's metaclass is derived", meta != m_animal && meta != *M_MIXIN);
        check_eq("Mixy mixinHello (M_Mixin)", m1(M_MIXIN, 0, mixy) as i64, 42);
        let x = m1(M_ANIMAL, 0, mixy);
        check("Mixy makeOne (M_Animal)", x != 0 && m1(so(0), SOM_GET_CLASS, x) == mixy);
        take_log();
        check_eq("Mixy inherits Dog", m1(ANIMAL, 2, x) as i64, 101);

        // Plain default initialization from somNew.
        let dinit: extern "C" fn(usize, usize) = f(tok(so(0), SOM_DEFAULT_INIT));
        let _ = dinit;
        let mem = (SOMMalloc)(64);
        check("SOMMalloc", !mem.is_null());
        (SOMFree)(mem);
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
