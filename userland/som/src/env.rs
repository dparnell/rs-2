//! The SOM environment: the three classes everything starts from
//! (SOMObject, SOMClass, SOMClassMgr) and the class manager object, and
//! somBuildClass, which builds a class from the static class information
//! the SOM compiler emits (`somStaticClassInfo`, layout versions 1-4).

use alloc::vec::Vec;

use crate::class::{Seq, cm, mm};
use crate::exports::*;
use crate::id::{self, SomId};
use crate::rt::*;
use crate::sys::*;

/// somStaticClassInfo (the fields of every layout version).
#[repr(C)]
pub struct Sci {
    pub layout_version: u32,
    pub num_static_methods: u32,
    pub num_static_overrides: u32,
    pub num_non_internal_data: u32,
    pub num_proc_methods: u32,
    pub num_varargs_funcs: u32,
    pub major: u32,
    pub minor: u32,
    pub instance_data_size: u32,
    pub max_methods: u32,
    pub num_parents: u32,
    pub class_id: SomId,
    pub explicit_meta_id: SomId,
    pub implicit_parent_meta: i32,
    pub parents: *const SomId,
    pub cds: *mut usize,
    pub ccds: *mut usize,
    pub smt: *const [usize; 6],
    pub omt: *const [usize; 2],
    pub nit_reference_base: usize,
    pub nit: *const [usize; 2],
    pub pmt: *const [usize; 2],
    pub vft: *const [usize; 2],
    pub cif: usize,
    // version 2
    pub data_alignment: i32,
    // version 3
    pub num_direct_init_classes: i32,
    pub direct_init_classes: *const SomId,
    pub num_methods: u32,
    pub mt: *const [usize; 7],
    pub protected_data_offset: u32,
    pub som_sci_version: u32,
    pub num_inherited_methods: u32,
    pub imt: *const [usize; 3],
    pub num_class_data_entries: u32,
    pub class_data_entry_names: *const SomId,
    pub num_migrated_methods: u32,
    pub mmt: *const [usize; 2],
    pub num_initializers: u32,
    pub initializers: *const SomId,
    pub som_dts_class: u32,
    pub dsci: *const [usize; 4],
}

static mut BOOTING: bool = false;

/// somApRdInfo and somMethodInfo for SOM's methods that return
/// structures (sequences).
#[repr(C)]
struct ApRd {
    rd: usize,
    ap: usize,
    info: *const [u32; 3],
}
unsafe impl Sync for ApRd {}
static AGG_INFO: [u32; 3] = [8, 8, 0];
static AGG: ApRd = ApRd { rd: 0, ap: 0, info: &AGG_INFO };

type Def = (&'static [u8], usize, bool);

macro_rules! defs {
    ($($name:literal => $proc:expr $(, $agg:literal)?;)*) => {
        [$(($name as &[u8], $proc as usize, false $(|| $agg)?)),*]
    };
}

fn object_defs() -> [Def; 32] {
    defs! {
        b"somInit" => __m_somInit;
        b"somUninit" => __m_somUninit;
        b"somFree" => __m_somFree;
        b"somDefaultVCopyInit" => __m_somDefaultVCopyInit;
        b"somGetClassName" => __m_somGetClassName;
        b"somGetClass" => __m_somGetClass;
        b"somIsA" => __m_somIsA;
        b"somRespondsTo" => __m_somRespondsTo;
        b"somIsInstanceOf" => __m_somIsInstanceOf;
        b"somGetSize" => __m_somGetSize;
        b"somDumpSelf" => __m_somDumpSelf;
        b"somDumpSelfInt" => __m_somDumpSelfInt;
        b"somPrintSelf" => __m_somPrintSelf;
        b"somDefaultConstVCopyInit" => __m_somDefaultConstVCopyInit;
        b"somDispatchV" => __m_somDispatchV;
        b"somDispatchL" => __m_somDispatchL;
        b"somDispatchA" => __m_somDispatchA;
        b"somDispatchD" => __m_somDispatchD;
        b"somDispatch" => __m_somDispatch;
        b"somClassDispatch" => __m_somClassDispatch;
        b"somCastObj" => __m_somCastObj;
        b"somResetObj" => __m_somResetObj;
        b"somDefaultInit" => __m_somDefaultInit;
        b"somDestruct" => __m_somDestruct;
        b"somPrivate1" => __m_somPrivate;
        b"somPrivate2" => __m_somPrivate;
        b"somDefaultCopyInit" => __m_somDefaultCopyInit;
        b"somDefaultConstCopyInit" => __m_somDefaultConstCopyInit;
        b"somDefaultAssign" => __m_somDefaultAssign;
        b"somDefaultConstAssign" => __m_somDefaultConstAssign;
        b"somDefaultVAssign" => __m_somDefaultVAssign;
        b"somDefaultConstVAssign" => __m_somDefaultConstVAssign;
    }
}

fn class_defs() -> [Def; 76] {
    defs! {
        b"somNew" => __m_somNew;
        b"somRenew" => __m_somRenew;
        b"somInitClass" => __m_somInitClass;
        b"somClassReady" => __m_somClassReady;
        b"somGetName" => __m_somGetName;
        b"somGetParent" => __m_somGetParent;
        b"somDescendedFrom" => __m_somDescendedFrom;
        b"somCheckVersion" => __m_somCheckVersion;
        b"somFindMethod" => __m_somFindMethod;
        b"somFindMethodOk" => __m_somFindMethodOk;
        b"somSupportsMethod" => __m_somSupportsMethod;
        b"somGetNumMethods" => __m_somGetNumMethods;
        b"somGetInstanceSize" => __m_somGetInstanceSize;
        b"somGetInstanceOffset" => __m_somGetInstanceOffset;
        b"somGetInstancePartSize" => __m_somGetInstancePartSize;
        b"somGetMethodIndex" => __m_somGetMethodIndex;
        b"somGetNumStaticMethods" => __m_somGetNumStaticMethods;
        b"somGetPClsMtab" => __m_somGetPClsMtab;
        b"somGetClassMtab" => __m_somGetClassMtab;
        b"somAddStaticMethod" => __m_somAddStaticMethod;
        b"somOverrideSMethod" => __m_somOverrideSMethod;
        b"somAddDynamicMethod" => __m_somAddDynamicMethod;
        b"somcPrivate0" => __m_class_private;
        b"somGetApplyStub" => __m_somGetApplyStub;
        b"somFindSMethod" => __m_somFindSMethod;
        b"somFindSMethodOk" => __m_somFindSMethodOk;
        b"somGetMethodDescriptor" => __m_somGetMethodDescriptor;
        b"somGetNthMethodInfo" => __m_somGetNthMethodInfo;
        b"somSetClassData" => __m_somSetClassData;
        b"somGetClassData" => __m_somGetClassData;
        b"somNewNoInit" => __m_somNewNoInit;
        b"somRenewNoInit" => __m_somRenewNoInit;
        b"somGetInstanceToken" => __m_somGetInstanceToken;
        b"somGetMemberToken" => __m_somGetMemberToken;
        b"somcPrivate1" => __m_class_private;
        b"somGetMethodData" => __m_somGetMethodData;
        b"somOverrideMtab" => __m_somOverrideMtab;
        b"somGetMethodToken" => __m_somGetMethodToken;
        b"somGetParents" => __m_somGetParents, true;
        b"somGetPClsMtabs" => __m_somGetPClsMtabs;
        b"somInitMIClass" => __m_somInitMIClass;
        b"somGetVersionNumbers" => __m_somGetVersionNumbers;
        b"somLookupMethod" => __m_somLookupMethod;
        b"_get_somInstanceDataOffsets" => __m_get_somInstanceDataOffsets, true;
        b"somRenewNoZero" => __m_somRenewNoZero;
        b"somRenewNoInitNoZero" => __m_somRenewNoInitNoZero;
        b"somAllocate" => __m_somAllocate;
        b"somDeallocate" => __m_somDeallocate;
        b"somGetRdStub" => __m_somGetRdStub;
        b"somGetNthMethodData" => __m_somGetNthMethodData;
        b"somcPrivate2" => __m_class_private;
        b"somcPrivate3" => __m_class_private;
        b"_get_somDirectInitClasses" => __m_get_somDirectInitClasses, true;
        b"somcPrivate5" => __m_class_private;
        b"somcPrivate6" => __m_class_private;
        b"somcPrivate7" => __m_class_private;
        b"somcPrivate8" => __m_class_private;
        b"somcPrivate9" => __m_class_private;
        b"somcPrivate10" => __m_class_private;
        b"somcPrivate11" => __m_class_private;
        b"somcPrivate12" => __m_class_private;
        b"somDefinedMethod" => __m_somDefinedMethod;
        b"somcPrivate13" => __m_class_private;
        b"somcPrivate14" => __m_class_private;
        b"somcPrivate15" => __m_class_private;
        b"somcPrivate16" => __m_class_private;
        b"somcPrivate17" => __m_class_private;
        b"somcPrivate18" => __m_class_private;
        b"somcPrivate19" => __m_class_private;
        b"somcPrivate20" => __m_class_private;
        b"_get_somDataAlignment" => __m_get_somDataAlignment;
        b"somcPrivate21" => __m_class_private;
        b"somcPrivate22" => __m_class_private;
        b"somcPrivate23" => __m_class_private;
        b"somcPrivate24" => __m_class_private;
        b"somcPrivate25" => __m_class_private;
    }
}

fn mgr_defs() -> [Def; 22] {
    defs! {
        b"somFindClsInFile" => __m_somFindClsInFile;
        b"somFindClass" => __m_somFindClass;
        b"somClassFromId" => __m_somClassFromId;
        b"somRegisterClass" => __m_somRegisterClass;
        b"somUnregisterClass" => __m_somUnregisterClass;
        b"somLocateClassFile" => __m_somLocateClassFile;
        b"somLoadClassFile" => __m_somLoadClassFile;
        b"somUnloadClassFile" => __m_somUnloadClassFile;
        b"somGetInitFunction" => __m_somGetInitFunction;
        b"somMergeInto" => __m_somMergeInto;
        b"somGetRelatedClasses" => __m_somGetRelatedClasses;
        b"somSubstituteClass" => __m_somSubstituteClass;
        b"_get_somInterfaceRepository" => __m_get_somInterfaceRepository;
        b"_set_somInterfaceRepository" => __m_set_somInterfaceRepository;
        b"_get_somRegisteredClasses" => __m_get_somRegisteredClasses, true;
        b"somBeginPersistentClasses" => __m_somBeginPersistentClasses;
        b"somEndPersistentClasses" => __m_somEndPersistentClasses;
        b"somcmPrivate1" => __m_mgr_private;
        b"somcmPrivate2" => __m_mgr_private;
        b"somRegisterClassLibrary" => __m_somRegisterClassLibrary;
        b"somJoinAffinityGroup" => __m_somJoinAffinityGroup;
        b"somUnregisterClassLibrary" => __m_somUnregisterClassLibrary;
    }
}

unsafe fn add_defs(c: *mut Class, defs: &[Def], cds: *mut usize) {
    for (i, &(name, p, agg)) in defs.iter().enumerate() {
        let mid = id::static_id(name);
        let tok = if agg {
            unsafe { add_static(c, mid, core::ptr::null_mut(), p, usize::MAX, &AGG as *const ApRd as usize) }
        } else {
            unsafe { add_static(c, mid, core::ptr::null_mut(), p, 0, 0) }
        };
        unsafe { *cds.add(1 + i) = tok };
    }
}

unsafe fn override_name(c: *mut Class, name: &[u8], p: usize) {
    unsafe { override_method(c, id::static_id(name), p) };
}

/// A class object for one of SOM's own classes (an instance of SOMClass).
unsafe fn class_object(c: *mut Class, cds: *mut usize, ccds: *mut usize) {
    unsafe {
        let sc = &*SOMCLASS;
        let obj = crate::malloc(sc.instance_size as u32) as usize;
        core::ptr::write_bytes(obj as *mut u8, 0, sc.instance_size);
        *(obj as *mut usize) = sc.mtab as usize;
        *((obj + sc.somclass_off) as *mut *mut Class) = c;
        let cr = &mut *c;
        cr.obj = obj;
        (*cr.mtab).class_object = obj;
        (*cr.pmtab).class_object = obj;
        cr.cds = cds;
        cr.ccds = ccds;
        *cds = obj;
        if !ccds.is_null() {
            *ccds = cr.pmtab as usize;
            *ccds.add(1) = cr.inst_token;
        }
    }
}

/// Make the SOM environment (once): returns the class manager.
pub fn env_new() -> usize {
    unsafe {
        if crate::SOMClassMgrObject != 0 {
            return crate::SOMClassMgrObject;
        }
        let _g = lock();
        if crate::SOMClassMgrObject != 0 || BOOTING {
            return crate::SOMClassMgrObject;
        }
        BOOTING = true;
        let so = Class::new();
        let sc = Class::new();
        let scm = Class::new();
        let pdm = Class::new();
        SOMOBJECT = so;
        SOMCLASS = sc;
        SOMCLASSMGR = scm;
        init_mi(so, 0, b"SOMObject", &[], 0, 4, 32, 1, 5);
        add_defs(so, &object_defs(), (&raw mut crate::SOMObjectClassData) as *mut usize);
        init_mi(sc, 0, b"SOMClass", &[so], 4, 4, 76, 1, 5);
        add_defs(sc, &class_defs(), (&raw mut crate::SOMClassClassData) as *mut usize);
        override_name(sc, b"somFree", __m_class_somFree as usize);
        override_name(sc, b"somDumpSelfInt", __m_class_somDumpSelfInt as usize);
        init_mi(scm, 0, b"SOMClassMgr", &[so], 4, 4, 22, 1, 5);
        add_defs(scm, &mgr_defs(), (&raw mut crate::SOMClassMgrClassData) as *mut usize);
        override_name(scm, b"somDumpSelfInt", __m_mgr_somDumpSelfInt as usize);
        init_mi(pdm, 0, b"SOMParentDerivedMetaclass", &[sc], 0, 4, 0, 1, 5);
        class_object(so, (&raw mut crate::SOMObjectClassData) as *mut usize, (&raw mut crate::SOMObjectCClassData) as *mut usize);
        class_object(sc, (&raw mut crate::SOMClassClassData) as *mut usize, (&raw mut crate::SOMClassCClassData) as *mut usize);
        class_object(scm, (&raw mut crate::SOMClassMgrClassData) as *mut usize, (&raw mut crate::SOMClassMgrCClassData) as *mut usize);
        class_object(
            pdm,
            (&raw mut crate::SOMParentDerivedMetaclassClassData) as *mut usize,
            (&raw mut crate::SOMParentDerivedMetaclassCClassData) as *mut usize,
        );
        for c in [so, sc, scm, pdm] {
            build_ctrls(c);
            (*c).ready = true;
        }
        let mgr = crate::call_cm((*scm).obj, cm::NEW, &[(*scm).obj as u32]) as usize;
        crate::SOMClassMgrObject = mgr;
        for c in [so, sc, scm, pdm] {
            crate::call_mm(mgr, mm::REGISTER_CLASS, &[mgr as u32, (*c).obj as u32]);
        }
        BOOTING = false;
        // Class libraries registered before the environment existed.
        crate::run_pending_libraries();
        mgr
    }
}

/// The metaclass for a new class: the explicit one and the parents'
/// metaclasses, less those another of them descends from; a new class
/// derived from all of them when more than one remains.
pub unsafe fn metaclass_for(name: &[u8], parents: &[usize], explicit: usize) -> usize {
    let mut cands: Vec<usize> = Vec::new();
    if explicit != 0 {
        cands.push(explicit);
    }
    for &p in parents {
        let m = unsafe { (**(p as *const *const Mtab)).class_object };
        if m != 0 && !cands.contains(&m) {
            cands.push(m);
        }
    }
    let keep: Vec<usize> = cands
        .iter()
        .copied()
        .filter(|&c| {
            !cands.iter().any(|&d| {
                d != c && unsafe { !rec(d).is_null() && !rec(c).is_null() && (*rec(d)).descends_from(rec(c)) }
            })
        })
        .collect();
    match keep.len() {
        0 => unsafe { (*SOMCLASS).obj },
        1 => keep[0],
        _ => unsafe { derived_metaclass(name, &keep) },
    }
}

unsafe fn derived_metaclass(name: &[u8], parents: &[usize]) -> usize {
    let mut dname: Vec<u8> = b"SOMDerivedMeta_".to_vec();
    dname.extend_from_slice(name);
    let did = id::static_id(&dname);
    let mgr = crate::class_mgr();
    let existing = crate::call_mm(mgr, mm::CLASS_FROM_ID, &[mgr as u32, did as u32]) as usize;
    if existing != 0 {
        return existing;
    }
    let meta = unsafe { metaclass_for(&dname, parents, 0) };
    let cls = crate::call_cm(meta, cm::NEW, &[meta as u32]) as usize;
    let mut ps = parents.to_vec();
    let seq = Seq { maximum: ps.len() as u32, length: ps.len() as u32, buffer: ps.as_mut_ptr() };
    dname.push(0);
    crate::call_cm(cls, cm::INIT_MI_CLASS, &[
        cls as u32,
        u32::MAX,
        dname.as_ptr() as u32,
        &seq as *const Seq as u32,
        0,
        4,
        0,
        0,
        0,
    ]);
    crate::call_cm(cls, cm::CLASS_READY, &[cls as u32]);
    cls
}

fn find_class(cid: SomId) -> usize {
    let mgr = crate::class_mgr();
    crate::call_mm(mgr, mm::FIND_CLASS, &[mgr as u32, cid as u32, 0, 0]) as usize
}

/// somBuildClass.
pub unsafe fn build_class(inherit_vars: i32, sci: *const Sci, major: i32, minor: i32) -> usize {
    env_new();
    let _g = lock();
    let s = unsafe { &*sci };
    let v = s.layout_version;
    let name = id::name_bytes(s.class_id);
    if (major != 0 && s.major as i32 != major) || (minor != 0 && (s.minor as i32) < minor) {
        crate::warn(b"class %s: version %d.%d requested\n\0", &[id::name_of(s.class_id) as u32, major as u32, minor as u32]);
        crate::error(20079, b"SOM.C\0", 1);
    }
    if !s.cds.is_null() && unsafe { *s.cds } != 0 {
        return unsafe { *s.cds };
    }
    let mut parents: Vec<usize> = Vec::new();
    for i in 0..s.num_parents as usize {
        let p = find_class(unsafe { *s.parents.add(i) });
        if p == 0 {
            crate::warn(b"class %s: parent %s not found\n\0", &[
                id::name_of(s.class_id) as u32,
                id::name_of(unsafe { *s.parents.add(i) }) as u32,
            ]);
            crate::error(20219, b"SOM.C\0", 2);
            return 0;
        }
        parents.push(p);
    }
    let explicit = if s.explicit_meta_id.is_null() { 0 } else { find_class(s.explicit_meta_id) };
    let meta = unsafe { metaclass_for(name, &parents, explicit) };
    let cls = crate::call_cm(meta, cm::NEW, &[meta as u32]) as usize;
    if cls == 0 {
        return 0;
    }
    let seq = Seq { maximum: parents.len() as u32, length: parents.len() as u32, buffer: parents.as_mut_ptr() };
    let align = if v >= 2 { s.data_alignment } else { 0 };
    crate::call_cm(cls, cm::INIT_MI_CLASS, &[
        cls as u32,
        inherit_vars as u32,
        id::name_of(s.class_id) as u32,
        &seq as *const Seq as u32,
        s.instance_data_size,
        align as u32,
        s.max_methods.max(s.num_static_methods + if v >= 3 { s.num_methods } else { 0 }),
        s.major,
        s.minor,
    ]);
    let Some(c) = (unsafe { rec_ok(cls) }) else { return 0 };
    c.cds = s.cds;
    c.ccds = s.ccds;
    let inst = crate::call_cm(cls, cm::GET_INSTANCE_TOKEN, &[cls as u32]) as usize;
    unsafe {
        if !s.ccds.is_null() {
            *s.ccds = c.pmtab as usize;
            *s.ccds.add(1) = inst;
        }
        if v >= 3 {
            let mt = &*c.mtab;
            let pdo = s.protected_data_offset;
            (*c.mtab).protected_data_offset = pdo as i32;
            (*c.mtab).protected_data_token =
                crate::call_cm(cls, cm::GET_MEMBER_TOKEN, &[cls as u32, pdo, inst as u32]) as usize;
            let _ = mt;
            if !s.dsci.is_null() {
                (*c.mtab).embedded_objs = (*s.dsci)[3];
            }
            let n = s.num_direct_init_classes;
            if n > 0 && !s.direct_init_classes.is_null() {
                c.direct_init.clear();
                for i in 0..n as usize {
                    let d = find_class(*s.direct_init_classes.add(i));
                    if d != 0 {
                        c.direct_init.push(rec(d));
                    }
                }
            }
            c.num_cds_entries = s.num_class_data_entries as usize;
        }
        if c.num_cds_entries == 0 {
            c.num_cds_entries = (s.num_static_methods + s.num_proc_methods + s.num_varargs_funcs) as usize;
        }
        let resolved = leak_vec(alloc::vec![0usize; c.num_cds_entries.max(1)]);
        (*c.pmtab).resolved = resolved;
        let ncde = c.num_cds_entries;
        let cds_index = |slot: *mut usize| -> Option<usize> {
            if s.cds.is_null() || (slot as usize) < s.cds as usize + 4 {
                return None;
            }
            let i = (slot as usize - s.cds as usize - 4) / 4;
            (i < ncde).then_some(i)
        };
        // Non-internal data: tokens for members.
        for i in 0..s.num_non_internal_data as usize {
            let e = &*s.nit.add(i);
            let off = e[1].wrapping_sub(s.nit_reference_base);
            *(e[0] as *mut usize) = crate::call_cm(cls, cm::GET_MEMBER_TOKEN, &[cls as u32, off as u32, inst as u32]) as usize;
        }
        // Procedures in the class data.
        for i in 0..s.num_proc_methods as usize {
            let e = &*s.pmt.add(i);
            *(e[0] as *mut usize) = e[1];
        }
        // Static methods.
        for i in 0..s.num_static_methods as usize {
            let e = &*s.smt.add(i);
            let mid = *(e[1] as *const SomId);
            let desc = if e[2] == 0 { core::ptr::null_mut() } else { *(e[2] as *const SomId) };
            let tok = crate::call_cm(cls, cm::ADD_STATIC_METHOD, &[
                cls as u32,
                mid as u32,
                desc as u32,
                e[3] as u32,
                e[4] as u32,
                e[5] as u32,
            ]) as usize;
            if e[0] != 0 {
                *(e[0] as *mut usize) = tok;
                if let Some(k) = cds_index(e[0] as *mut usize) {
                    *resolved.add(k) = e[3];
                }
            }
        }
        // Overrides.
        for i in 0..s.num_static_overrides as usize {
            let e = &*s.omt.add(i);
            let mid = *(e[0] as *const SomId);
            crate::call_cm(cls, cm::OVERRIDE_S_METHOD, &[cls as u32, mid as u32, e[1] as u32]);
        }
        // General methods (layout 3).
        if v >= 3 {
            for i in 0..s.num_methods as usize {
                let e = &*s.mt.add(i);
                let action = e[0] & 0xff;
                let cd = e[1] as *mut usize;
                let mid = if e[2] == 0 { core::ptr::null_mut() } else { *(e[2] as *const SomId) };
                let desc = if e[3] == 0 { core::ptr::null_mut() } else { *(e[3] as *const SomId) };
                match action {
                    0 | 2 => {
                        let tok = crate::call_cm(cls, cm::ADD_STATIC_METHOD, &[
                            cls as u32,
                            mid as u32,
                            desc as u32,
                            e[4] as u32,
                            e[5] as u32,
                            e[6] as u32,
                        ]) as usize;
                        if !cd.is_null() {
                            *cd = tok;
                            if let Some(k) = cds_index(cd) {
                                *resolved.add(k) = e[4];
                            }
                        }
                    }
                    1 => {
                        crate::call_cm(cls, cm::ADD_DYNAMIC_METHOD, &[cls as u32, mid as u32, desc as u32, e[4] as u32, e[6] as u32]);
                    }
                    5 => {
                        // The implementation another class (*classData) has.
                        if !cd.is_null() {
                            if let Some(o) = rec_ok(*cd) {
                                if let Some((p, _)) = lookup(o, id::name_bytes(mid)) {
                                    override_method(c, mid, p);
                                }
                            }
                        }
                    }
                    6 => {
                        // methodId points to a method token.
                        let tok = if e[2] == 0 { 0 } else { *(e[2] as *const usize) };
                        if let Some(ti) = token_info(tok) {
                            let intro = &*ti.class;
                            if let Some(m) = intro.methods.get(ti.index) {
                                let mut d: Vec<u8> = intro.name_bytes().to_vec();
                                d.extend_from_slice(b"::");
                                d.extend_from_slice(cstr(m.name));
                                override_method(c, id::static_id(&d), e[4]);
                            }
                        }
                    }
                    7 => c.allocate = if cd.is_null() { e[4] } else { *cd },
                    8 => c.deallocate = if cd.is_null() { e[4] } else { *cd },
                    _ => {}
                }
            }
            // Implementations inherited from a given parent.
            for i in 0..s.num_inherited_methods as usize {
                let e = &*s.imt.add(i);
                let mid = *(e[0] as *const SomId);
                let tok = *(e[2] as *const usize);
                let p = crate::somParentNumResolve((*c.pmtab).next as usize, e[1] as i32 - 1, tok);
                if p != 0 {
                    override_method(c, mid, p);
                }
            }
            for i in 0..s.num_migrated_methods as usize {
                let e = &*s.mmt.add(i);
                *(e[0] as *mut usize) = *(e[1] as *const usize);
            }
            if s.num_initializers > 0 && !s.initializers.is_null() {
                let mut v = Vec::new();
                for i in 0..s.num_initializers as usize {
                    v.push(crate::class::somFindSMethod(cls, *s.initializers.add(i)));
                }
                (*c.pmtab).initializers = leak_vec(v);
            }
        }
        // Varargs stubs.
        for i in 0..s.num_varargs_funcs as usize {
            let e = &*s.vft.add(i);
            *(e[0] as *mut usize) = e[1];
        }
        build_ctrls(c);
        crate::call_cm(cls, cm::SET_CLASS_DATA, &[cls as u32, s.cds as u32]);
        if s.cif != 0 {
            let f: extern "C" fn(usize) = core::mem::transmute(s.cif);
            f(cls);
        }
        if !s.cds.is_null() {
            *s.cds = cls;
        }
        crate::call_cm(cls, cm::CLASS_READY, &[cls as u32]);
    }
    cls
}
