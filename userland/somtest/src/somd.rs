//! `SOMTEST SOMD`: SOMD.DLL (DSOM's client library). Its classes exist once
//! it is loaded, with IBM's parents, metaclasses and instance sizes;
//! SOMD_Init makes the client's objects. With rs2's SOMD (a stub) the
//! methods raise StExcep::NO_IMPLEMENT; IBM's needs SOMEM, SOMS and SOMSEC
//! in the directory and does the real thing.

use os2::*;

use crate::libs::{Env, load, proc_addr};
use crate::{check, check_eq, cstr, m1, sc, so};

const SOM_GET_NAME: usize = 4;
const SOM_GET_PARENT: usize = 5;
const SOM_GET_INSTANCE_SIZE: usize = 12;
const SOM_NEW: usize = 0;
const SOM_GET_CLASS: usize = 5;
const SOM_FREE: usize = 2;

fn name_of(cls: usize) -> &'static [u8] {
    if cls == 0 {
        return b"(none)";
    }
    cstr(m1(sc(), SOM_GET_NAME, cls) as *const u8)
}

pub fn run() {
    let h = load(b"SOMD");
    if h == 0 {
        check("SOMD.DLL loaded", false);
        return;
    }
    let cd = |n: &[u8]| -> *const usize {
        let mut v = n.to_vec();
        v.extend_from_slice(b"ClassData");
        proc_addr(h, &v) as *const usize
    };
    // (class, parent, metaclass, instance size)
    let classes: &[(&[u8], &[u8], &[u8], i64)] = &[
        (b"SOMDObject", b"SOMObject", b"SOMClass", 32),
        (b"SOMDClientProxy", b"SOMDObject", b"SOMDMetaproxy", 52),
        (b"ObjectMgr", b"SOMObject", b"SOMMSingleInstance", 4),
        (b"SOMDObjectMgr", b"ObjectMgr", b"SOMMSingleInstance", 12),
        (b"SOMDServer", b"SOMObject", b"SOMMSingleInstance", 4),
        (b"ORB", b"SOMObject", b"SOMMSingleInstance", 4),
        (b"BOA", b"SOMObject", b"SOMMSingleInstance", 4),
        (b"NVList", b"SOMObject", b"SOMClass", 16),
        (b"Request", b"SOMObject", b"SOMClass", 40),
        (b"Context", b"SOMObject", b"SOMClass", 16),
        (b"ImplRepository", b"SOMObject", b"SOMMSingleInstance", 8),
    ];
    for &(n, parent, meta, size) in classes {
        let d = cd(n);
        let cls = if d.is_null() { 0 } else { unsafe { *d } };
        let what = core::str::from_utf8(n).unwrap_or("?");
        check(&alloc::format!("{} built at load", what), cls != 0 && name_of(cls) == n);
        if cls == 0 {
            continue;
        }
        let p = m1(sc(), SOM_GET_PARENT, cls);
        let m = m1(so(0), SOM_GET_CLASS, cls);
        println!("  {}: parent {}, metaclass {}, size {}", what, core::str::from_utf8(name_of(p)).unwrap_or("?"),
            core::str::from_utf8(name_of(m)).unwrap_or("?"), m1(sc(), SOM_GET_INSTANCE_SIZE, cls));
        check(&alloc::format!("  its parent is {}", core::str::from_utf8(parent).unwrap_or("?")), name_of(p) == parent);
        check(&alloc::format!("  its metaclass descends from {}", core::str::from_utf8(meta).unwrap_or("?")), descends(m, meta));
        check_eq(&alloc::format!("  {} instance size", what), m1(sc(), SOM_GET_INSTANCE_SIZE, cls) as i64, size);
    }
    // SOMD_Init: the object manager and the ORB.
    let init = proc_addr(h, b"SOMD_Init");
    let om = proc_addr(h, b"SOMD_ObjectMgr") as *const usize;
    let orb = proc_addr(h, b"SOMD_ORBObject") as *const usize;
    check("SOMD_Init exported", init != 0 && !om.is_null() && !orb.is_null());
    if init == 0 || om.is_null() || orb.is_null() {
        return;
    }
    let mut ev = Env::new();
    let f: extern "C" fn(*mut Env) = unsafe { core::mem::transmute(init) };
    f(&mut ev);
    let (om, orb) = unsafe { (*om, *orb) };
    println!("  SOMD_Init: exception {}, object manager {:08x} ({}), ORB {:08x} ({})", ev.major, om,
        core::str::from_utf8(name_of(if om != 0 { m1(so(0), SOM_GET_CLASS, om) } else { 0 })).unwrap_or("?"), orb,
        core::str::from_utf8(name_of(if orb != 0 { m1(so(0), SOM_GET_CLASS, orb) } else { 0 })).unwrap_or("?"));
    check("SOMD_Init: SOMD_ObjectMgr is a SOMDObjectMgr", om != 0 && name_of(m1(so(0), SOM_GET_CLASS, om)) == b"SOMDObjectMgr");
    check("SOMD_Init: SOMD_ORBObject is an ORB", orb != 0 && name_of(m1(so(0), SOM_GET_CLASS, orb)) == b"ORB");
    // An NVList: rs2's stub raises NO_IMPLEMENT, IBM's counts 0 items.
    let nvd = cd(b"NVList");
    if nvd.is_null() || unsafe { *nvd } == 0 {
        return;
    }
    let nv = m1(sc(), SOM_NEW, unsafe { *nvd });
    check("an NVList", nv != 0);
    let mut ev = Env::new();
    let mut count: i32 = -1;
    let get_count: extern "C" fn(usize, *mut Env, *mut i32) -> u32 = unsafe { core::mem::transmute(*nvd.add(5)) };
    let r = get_count(nv, &mut ev, &mut count);
    let ex = if ev.major != 0 { cstr(ev.name) } else { b"" };
    println!("  NVList get_count: {} count {} exception {} {}", r, count, ev.major, core::str::from_utf8(ex).unwrap_or("?"));
    check("NVList get_count: 0 items or NO_IMPLEMENT", (ev.major == 0 && count == 0) || (ev.major == 2 && ex == b"StExcep::NO_IMPLEMENT"));
    m1(so(0), SOM_FREE, nv);
}

fn descends(cls: usize, name: &[u8]) -> bool {
    let mut c = cls;
    let mut n = 0;
    while c != 0 && n < 32 {
        if name_of(c) == name {
            return true;
        }
        c = m1(sc(), SOM_GET_PARENT, c);
        n += 1;
    }
    false
}
