//! SOMD.DLL: DSOM's client library as a stub. rs2 does not do distributed
//! objects (nothing it ships uses them), but programs import SOMD.DLL:
//! IBM's Object REXX (OREXX.DLL) refers to its classes and calls
//! SOMD_Init when a REXX program asks for DSOM. This DLL has IBM's 138
//! exports with IBM's ordinals (studied in IBM's SOMD.DLL with Ghidra):
//!
//! - the 25 classes IBM's exports class data for (SOMDObject,
//!   SOMDClientProxy, ObjectMgr, SOMDObjectMgr, SOMDServer, ORB, BOA,
//!   NVList, Request, Context, ImplRepository, ImplementationDef, ...),
//!   built when the DLL is loaded as IBM's are, with IBM's parents,
//!   metaclasses, versions, instance data sizes and class data order
//!   (`classes.rs`). Their methods, and the procedure methods in their
//!   class data, do nothing but raise the CORBA system exception
//!   `StExcep::NO_IMPLEMENT` in the caller's environment and return 0;
//! - SOMD_Init, which makes the client's objects (SOMD_ObjectMgr,
//!   SOMD_ORBObject, SOMD_ImplRepObject) as IBM's does, and SOMD_Uninit,
//!   which frees them;
//! - the helper functions (string, byte order and environment helpers
//!   work; the communication ones fail).
//!
//! Principal's parent in IBM's DLL is SOMSEC.DLL's
//! CosSecurityAuthn::Principal; here it is SOMObject (rs2 has no SOMSEC).

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, function_casts_as_integer, static_mut_refs)]

extern crate alloc;
extern crate os2;

mod classes;

use alloc::vec::Vec;
use somsup::{ClassSpec, Environment, aligned, build_class, methods, optlink};

somsup::dll_entry!(init);

extern "C" fn init(_hmod: u32, term: u32) -> u32 {
    if term == 0 {
        // IBM's SOMD.DLL builds its classes when it is loaded.
        SOMInitsomd();
    }
    1
}

/// A class as IBM's SOMD.DLL builds it: its class data entries in
/// order ("" where the entry is a procedure method or unused), the
/// entries that are procedure methods.
pub struct ClassDef {
    pub name: &'static str,
    pub parents: &'static [&'static str],
    pub meta: Option<&'static str>,
    pub major: i32,
    pub minor: i32,
    pub data: u32,
    pub methods: &'static [&'static str],
    pub procs: &'static [usize],
}

// ---- Class data (the class object, then IBM's entries) ----

macro_rules! class_data {
    ($($cd:ident, $ccd:ident, $n:literal;)*) => {$(
        #[unsafe(no_mangle)]
        pub static mut $cd: [usize; $n] = [0; $n];
        #[unsafe(no_mangle)]
        pub static mut $ccd: [usize; 8] = [0; 8];
    )*};
}

class_data! {
    CommAddressClassData, CommAddressCClassData, 51;
    CommBindingClassData, CommBindingCClassData, 37;
    ImplDatabaseClassData, ImplDatabaseCClassData, 24;
    LocationServiceClassData, LocationServiceCClassData, 14;
    LocServDClassData, LocServDCClassData, 12;
    RequestMsgClassData, RequestMsgCClassData, 40;
    UuidClassData, UuidCClassData, 3;
    BOAClassData, BOACClassData, 13;
    SOMOAClassData, SOMOACClassData, 9;
    ContextClassData, ContextCClassData, 13;
    ImplementationDefClassData, ImplementationDefCClassData, 22;
    PrincipalClassData, PrincipalCClassData, 5;
    ImplRepositoryClassData, ImplRepositoryCClassData, 18;
    NVListClassData, NVListCClassData, 9;
    ObjectMgrClassData, ObjectMgrCClassData, 6;
    SOMDObjectMgrClassData, SOMDObjectMgrCClassData, 7;
    ORBClassData, ORBCClassData, 8;
    RequestClassData, RequestCClassData, 20;
    SOMDServerClassData, SOMDServerCClassData, 8;
    SOMDServerMgrClassData, SOMDServerMgrCClassData, 8;
    SOMDObjectClassData, SOMDObjectCClassData, 21;
    SOMDClientProxyClassData, SOMDClientProxyCClassData, 14;
    SOMDMetaproxyClassData, SOMDMetaproxyCClassData, 1;
    TSIdentificationClassData, TSIdentificationCClassData, 5;
    ImplRepDistClassData, ImplRepDistCClassData, 9;
}

// ---- The client's objects and settings (IBM's exported variables) ----

#[unsafe(no_mangle)]
pub static mut SOMD_ObjectMgr: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_ORBObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_ImplRepObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_ImplDefObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_SOMOAObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_ServerObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_LocServObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_SocketObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_MMObject: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_DefaultContext: usize = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_DebugFlag: i32 = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_TraceLevel: i32 = 0;
#[unsafe(no_mangle)]
pub static mut somdCommDebugFlag: i32 = 0;
#[unsafe(no_mangle)]
pub static mut SOMD_RecvBufferSize: i32 = 0x1000;
#[unsafe(no_mangle)]
pub static mut somdLSDAddress: usize = 0;

// ---- Building the classes ----

unsafe extern "C" {
    #[link_name = "SOMU.1"]
    fn SOMUTStringIdNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMU.5"]
    fn SOMMCooperativeSisteredNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "SOMU.42"]
    fn SOMMSingleInstanceNewClass(major: i32, minor: i32) -> usize;
}

macro_rules! p {
    ($a:ident) => {
        (&raw mut $a) as *mut usize
    };
}

/// The class data of a SOMD class, by name (for building parents first).
fn lookup(name: &str) -> Option<(&'static ClassDef, *mut usize, *mut usize)> {
    use classes::*;
    Some(match name {
        "CommAddress" => (&COMMADDRESS, p!(CommAddressClassData), p!(CommAddressCClassData)),
        "CommBinding" => (&COMMBINDING, p!(CommBindingClassData), p!(CommBindingCClassData)),
        "ImplDatabase" => (&IMPLDATABASE, p!(ImplDatabaseClassData), p!(ImplDatabaseCClassData)),
        "LocationService" => (&LOCATIONSERVICE, p!(LocationServiceClassData), p!(LocationServiceCClassData)),
        "LocServD" => (&LOCSERVD, p!(LocServDClassData), p!(LocServDCClassData)),
        "RequestMsg" => (&REQUESTMSG, p!(RequestMsgClassData), p!(RequestMsgCClassData)),
        "Uuid" => (&UUID, p!(UuidClassData), p!(UuidCClassData)),
        "BOA" => (&BOA, p!(BOAClassData), p!(BOACClassData)),
        "SOMOA" => (&SOMOA, p!(SOMOAClassData), p!(SOMOACClassData)),
        "Context" => (&CONTEXT, p!(ContextClassData), p!(ContextCClassData)),
        "ImplementationDef" => (&IMPLEMENTATIONDEF, p!(ImplementationDefClassData), p!(ImplementationDefCClassData)),
        "Principal" => (&PRINCIPAL, p!(PrincipalClassData), p!(PrincipalCClassData)),
        "ImplRepository" => (&IMPLREPOSITORY, p!(ImplRepositoryClassData), p!(ImplRepositoryCClassData)),
        "NVList" => (&NVLIST, p!(NVListClassData), p!(NVListCClassData)),
        "ObjectMgr" => (&OBJECTMGR, p!(ObjectMgrClassData), p!(ObjectMgrCClassData)),
        "SOMDObjectMgr" => (&SOMDOBJECTMGR, p!(SOMDObjectMgrClassData), p!(SOMDObjectMgrCClassData)),
        "ORB" => (&ORB, p!(ORBClassData), p!(ORBCClassData)),
        "Request" => (&REQUEST, p!(RequestClassData), p!(RequestCClassData)),
        "SOMDServer" => (&SOMDSERVER, p!(SOMDServerClassData), p!(SOMDServerCClassData)),
        "SOMDServerMgr" => (&SOMDSERVERMGR, p!(SOMDServerMgrClassData), p!(SOMDServerMgrCClassData)),
        "SOMDObject" => (&SOMDOBJECT, p!(SOMDObjectClassData), p!(SOMDObjectCClassData)),
        "SOMDClientProxy" => (&SOMDCLIENTPROXY, p!(SOMDClientProxyClassData), p!(SOMDClientProxyCClassData)),
        "SOMDMetaproxy" => (&SOMDMETAPROXY, p!(SOMDMetaproxyClassData), p!(SOMDMetaproxyCClassData)),
        "TSIdentification" => (&TSIDENTIFICATION, p!(TSIdentificationClassData), p!(TSIdentificationCClassData)),
        "ImplRepDist" => (&IMPLREPDIST, p!(ImplRepDistClassData), p!(ImplRepDistCClassData)),
        _ => return None,
    })
}

/// Make sure a class this DLL's classes refer to exists.
fn need(name: &str) {
    unsafe {
        match name {
            "SOMObject" | "SOMClass" => {
                somsup::SOMObjectNewClass(1, 4);
                somsup::SOMClassNewClass(1, 4);
            }
            "SOMUTStringId" => {
                SOMUTStringIdNewClass(0, 0);
            }
            "SOMMCooperativeSistered" => {
                SOMMCooperativeSisteredNewClass(0, 0);
            }
            "SOMMSingleInstance" => {
                SOMMSingleInstanceNewClass(0, 0);
            }
            _ => {
                new_class(name);
            }
        }
    }
}

fn new_class(name: &str) -> usize {
    let Some((def, cd, ccd)) = lookup(name) else { return 0 };
    unsafe {
        if *cd != 0 {
            return *cd;
        }
    }
    for p in def.parents {
        need(p);
    }
    if let Some(m) = def.meta {
        need(m);
    }
    let methods: Vec<(&str, usize)> = def
        .methods
        .iter()
        .enumerate()
        .map(|(i, &m)| if def.procs.contains(&i) { ("", 0) } else { (m, __m_no_impl as usize) })
        .collect();
    let cls = build_class(&ClassSpec {
        name: def.name,
        parents: def.parents,
        meta: def.meta,
        major: def.major,
        minor: def.minor,
        data: def.data,
        align: if def.data != 0 { 4 } else { 0 },
        methods: &methods,
        overrides: &[],
        cd,
        ccd,
    });
    if cls != 0 {
        // Procedure methods are called through the class data directly.
        for &i in def.procs {
            unsafe { *cd.add(1 + i) = __m_no_impl as usize };
        }
    }
    cls
}

// ---- What every method does ----

const PAG_WRITE: u32 = 0x2;
const PAG_COMMIT: u32 = 0x10;

/// Whether `ev` is plausibly the caller's Environment (IDL methods take it
/// after the object): writable memory, not an object, a sane major code.
fn env_ok(ev: usize) -> bool {
    if ev < 0x10000 || ev & 3 != 0 || ev >= 0xc000_0000 {
        return false;
    }
    let mut cb = 12u32;
    let mut fl = 0u32;
    if unsafe { os2::DosQueryMem(ev as *mut u8, &mut cb, &mut fl) } != 0 || cb < 12 || fl & (PAG_COMMIT | PAG_WRITE) != PAG_COMMIT | PAG_WRITE {
        return false;
    }
    if unsafe { somsup::somIsObj(ev) } != 0 {
        return false;
    }
    unsafe { (*(ev as *const Environment)).major <= 2 }
}

/// CORBA's StExcep parameters: the minor code and whether the operation
/// completed (COMPLETED_NO).
#[repr(C)]
struct StExcep {
    minor: u32,
    completed: u32,
}

/// IBM's SOMDERROR_NotImplemented.
const SOMDERROR_NOT_IMPLEMENTED: u32 = 30001;

fn no_implement(ev: usize) {
    if !env_ok(ev) {
        return;
    }
    let p = somsup::malloc(core::mem::size_of::<StExcep>()) as *mut StExcep;
    if !p.is_null() {
        unsafe { *p = StExcep { minor: SOMDERROR_NOT_IMPLEMENTED, completed: 1 } };
    }
    somsup::set_exception(ev as *mut Environment, somsup::SYSTEM_EXCEPTION, b"StExcep::NO_IMPLEMENT\0", p as *mut u8);
}

extern "C" fn no_impl(_obj: usize, ev: usize) -> usize {
    no_implement(ev);
    0
}

methods! {
    __m_no_impl => no_impl, 2;
}

// ---- Exported functions ----

macro_rules! new_classes {
    ($($f:ident => $n:literal;)*) => {$(
        pub extern "C" fn $f(_major: i32, _minor: i32) -> usize {
            new_class($n)
        }
    )*};
}

new_classes! {
    BOANewClass => "BOA";
    SOMOANewClass => "SOMOA";
    SOMDObjectNewClass => "SOMDObject";
    SOMDMetaproxyNewClass => "SOMDMetaproxy";
    SOMDClientProxyNewClass => "SOMDClientProxy";
    ObjectMgrNewClass => "ObjectMgr";
    SOMDObjectMgrNewClass => "SOMDObjectMgr";
    SOMDServerNewClass => "SOMDServer";
    ImplRepositoryNewClass => "ImplRepository";
    ImplementationDefNewClass => "ImplementationDef";
    ImplDatabaseNewClass => "ImplDatabase";
    LocationServiceNewClass => "LocationService";
    LocServDNewClass => "LocServD";
    NVListNewClass => "NVList";
    ORBNewClass => "ORB";
    RequestMsgNewClass => "RequestMsg";
    RequestNewClass => "Request";
    UuidNewClass => "Uuid";
    PrincipalNewClass => "Principal";
    CommAddressNewClass => "CommAddress";
    CommBindingNewClass => "CommBinding";
    ContextNewClass => "Context";
    SOMDServerMgrNewClass => "SOMDServerMgr";
    TSIdentificationNewClass => "TSIdentification";
    ImplRepDistNewClass => "ImplRepDist";
}

/// SOMInitsomd: builds every class, in IBM's order.
pub extern "C" fn SOMInitsomd() {
    for n in [
        "BOA", "SOMOA", "SOMDObject", "SOMDMetaproxy", "SOMDClientProxy", "ObjectMgr", "SOMDObjectMgr", "SOMDServer",
        "ImplRepository", "ImplementationDef", "ImplDatabase", "LocationService", "LocServD", "NVList", "ORB",
        "RequestMsg", "Request", "Uuid", "Principal", "CommAddress", "CommBinding", "Context", "SOMDServerMgr",
        "TSIdentification", "ImplRepDist",
    ] {
        new_class(n);
    }
}

fn instance(name: &str) -> usize {
    let cls = new_class(name);
    if cls == 0 { 0 } else { somsup::new_object(cls) }
}

/// SOMD_Init(ev): the client's objects (the object manager, the ORB, the
/// implementation repository), once.
pub extern "C" fn SOMD_Init(_ev: *mut Environment) {
    SOMInitsomd();
    unsafe {
        if SOMD_ObjectMgr == 0 {
            SOMD_ObjectMgr = instance("SOMDObjectMgr");
        }
        if SOMD_ORBObject == 0 {
            SOMD_ORBObject = instance("ORB");
        }
        if SOMD_ImplRepObject == 0 {
            SOMD_ImplRepObject = instance("ImplRepository");
        }
    }
}

/// SOMD_Uninit(ev): frees what SOMD_Init made.
pub extern "C" fn SOMD_Uninit(_ev: *mut Environment) {
    unsafe {
        for o in [&raw mut SOMD_MMObject, &raw mut SOMD_ImplRepObject, &raw mut SOMD_LocServObject, &raw mut SOMD_ORBObject, &raw mut SOMD_ObjectMgr] {
            if *o != 0 {
                somsup::free_object(*o);
                *o = 0;
            }
        }
    }
}

/// somdExceptionFree(ev): frees an exception (the global environment's
/// for NULL) as somExceptionFree does.
pub extern "C" fn somdExceptionFree(ev: *mut Environment) {
    let ev = if ev.is_null() { unsafe { somsup::somGetGlobalEnvironment() } } else { ev };
    if !ev.is_null() {
        unsafe { somsup::somExceptionFree(ev) };
    }
}

/// somdExceptionCodeName(ev, buffer): the exception's name.
pub extern "C" fn somdExceptionCodeName(ev: *mut Environment, buf: *mut u8) -> *mut u8 {
    if buf.is_null() {
        return buf;
    }
    let id = unsafe { somsup::cstr(somsup::somExceptionId(ev)) };
    unsafe {
        core::ptr::copy_nonoverlapping(id.as_ptr(), buf, id.len());
        *buf.add(id.len()) = 0;
    }
    buf
}

/// What the functions that would talk to the DSOM daemon or another
/// process return: nothing (0).
pub extern "C" fn fail() -> usize {
    0
}

pub extern "C" fn nothing() {}

pub extern "C" fn somd_htonl(v: u32) -> u32 {
    v.swap_bytes()
}

pub extern "C" fn somd_htons(v: u32) -> u32 {
    (v as u16).swap_bytes() as u32
}

unsafe fn case_copy(src: *const u8, dst: *mut u8, upper: bool) -> *mut u8 {
    if src.is_null() || dst.is_null() {
        return dst;
    }
    let mut i = 0;
    loop {
        let c = unsafe { *src.add(i) };
        unsafe { *dst.add(i) = if upper { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() } };
        if c == 0 {
            return dst;
        }
        i += 1;
    }
}

/// somduppercase(source, destination) (_Optlink): destination.
pub extern "C" fn somduppercase(src: *const u8, dst: *mut u8) -> *mut u8 {
    unsafe { case_copy(src, dst, true) }
}

pub extern "C" fn somdlowercase(src: *const u8, dst: *mut u8) -> *mut u8 {
    unsafe { case_copy(src, dst, false) }
}

/// somdstrcmp(a, b) (_Optlink): compares without regard to case.
pub extern "C" fn somdstrcmp(a: *const u8, b: *const u8) -> i32 {
    let (a, b) = unsafe { (somsup::cstr(a), somsup::cstr(b)) };
    let mut i = 0;
    loop {
        let x = a.get(i).map_or(0, |c| c.to_ascii_lowercase()) as i32;
        let y = b.get(i).map_or(0, |c| c.to_ascii_lowercase()) as i32;
        if x != y || x == 0 {
            return x - y;
        }
        i += 1;
    }
}

/// somdGetenv(name) (_Optlink): the environment variable's value or NULL.
pub extern "C" fn somdGetenv(name: *const u8) -> *const u8 {
    let mut v: *const u8 = core::ptr::null();
    if name.is_null() || unsafe { os2::DosScanEnv(name, &mut v) } != 0 {
        return core::ptr::null();
    }
    v
}

/// somdisDbcs(c) (_Optlink): whether c is a DBCS lead byte (none here:
/// only values above 255 count, as in IBM's).
pub extern "C" fn somdisDbcs(c: i32) -> u32 {
    (c > 0xff) as u32
}

pub extern "C" fn somddbcsStrchr(s: *const u8, c: u32) -> *const u8 {
    let t = unsafe { somsup::cstr(s) };
    match t.iter().position(|&x| x == c as u8) {
        Some(i) => unsafe { s.add(i) },
        None if c as u8 == 0 => unsafe { s.add(t.len()) },
        None => core::ptr::null(),
    }
}

pub extern "C" fn somddbcsStrrchr(s: *const u8, c: u32) -> *const u8 {
    let t = unsafe { somsup::cstr(s) };
    match t.iter().rposition(|&x| x == c as u8) {
        Some(i) => unsafe { s.add(i) },
        None if c as u8 == 0 => unsafe { s.add(t.len()) },
        None => core::ptr::null(),
    }
}

pub extern "C" fn somddbcsStrstr(s: *const u8, w: *const u8) -> *const u8 {
    let (t, n) = unsafe { (somsup::cstr(s), somsup::cstr(w)) };
    if n.is_empty() {
        return s;
    }
    match t.windows(n.len()).position(|x| x == n) {
        Some(i) => unsafe { s.add(i) },
        None => core::ptr::null(),
    }
}

/// somdMaxPathLen(ev, &length) (_Optlink): the longest path (QSV_MAX_PATH_LENGTH).
pub extern "C" fn somdMaxPathLen(_ev: usize, len: *mut u32) -> u32 {
    if !len.is_null() {
        unsafe { *len = 260 };
    }
    0
}

/// SOMD_QueryORBfree(): whether the ORB frees client memory (IBM's default: no).
pub extern "C" fn SOMD_QueryORBfree() -> u32 {
    0
}

/// ORBfree(p): frees memory the ORB gave out.
pub extern "C" fn ORBfree(p: *mut u8) {
    somsup::free(p);
}

/// somdPrintf(fmt, ...): somPrintf.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn __v_somdPrintf() {
    core::arch::naked_asm!(
        "lea eax, [esp + 8]",
        "push eax",
        "push dword ptr [esp + 8]",
        "call {f}",
        "add esp, 8",
        "ret",
        f = sym somsup::somVprintf,
    );
}

aligned! {
    __a_SOMD_Init => SOMD_Init, 1;
    __a_SOMD_Uninit => SOMD_Uninit, 1;
    __a_SOMInitsomd => SOMInitsomd, 0;
    __a_somdExceptionFree => somdExceptionFree, 1;
    __a_SOMD_QueryORBfree => SOMD_QueryORBfree, 0;
    __a_ORBfree => ORBfree, 1;
    __a_SOMD_RegisterCallback => nothing, 0;
    __a_SOMD_FlushInterfaceCache => nothing, 0;
    __a_SOMD_YesORBfree => nothing, 0;
    __a_SOMD_NoORBfree => nothing, 0;
    __a_authenticate_clients => nothing, 0;
    __a_somdLSDSetup => nothing, 0;
    __a_somdPgmSetup => nothing, 0;
    __a_somdLSDExitHandler => nothing, 0;
    __a_somdPgmExitHandler => nothing, 0;
    __a_somdDefineExitHandler => nothing, 0;
    __a_somdVDMQueues => fail, 0;
    __a_somdCreateDynProxyClass => fail, 0;
    __a_somdGetLSDPort => fail, 0;
    __a_somdMakeSocketObj => fail, 0;
    __a_somdGetAnchorBlock => fail, 0;
    __a_somdMsg => fail, 0;
    __a_somdCheckSOMSOCKETS => fail, 0;
    __a_BOANewClass => BOANewClass, 2;
    __a_SOMOANewClass => SOMOANewClass, 2;
    __a_SOMDObjectNewClass => SOMDObjectNewClass, 2;
    __a_SOMDMetaproxyNewClass => SOMDMetaproxyNewClass, 2;
    __a_SOMDClientProxyNewClass => SOMDClientProxyNewClass, 2;
    __a_ObjectMgrNewClass => ObjectMgrNewClass, 2;
    __a_SOMDObjectMgrNewClass => SOMDObjectMgrNewClass, 2;
    __a_SOMDServerNewClass => SOMDServerNewClass, 2;
    __a_ImplRepositoryNewClass => ImplRepositoryNewClass, 2;
    __a_ImplementationDefNewClass => ImplementationDefNewClass, 2;
    __a_ImplDatabaseNewClass => ImplDatabaseNewClass, 2;
    __a_LocationServiceNewClass => LocationServiceNewClass, 2;
    __a_LocServDNewClass => LocServDNewClass, 2;
    __a_NVListNewClass => NVListNewClass, 2;
    __a_ORBNewClass => ORBNewClass, 2;
    __a_RequestMsgNewClass => RequestMsgNewClass, 2;
    __a_RequestNewClass => RequestNewClass, 2;
    __a_UuidNewClass => UuidNewClass, 2;
    __a_PrincipalNewClass => PrincipalNewClass, 2;
    __a_CommAddressNewClass => CommAddressNewClass, 2;
    __a_CommBindingNewClass => CommBindingNewClass, 2;
    __a_ContextNewClass => ContextNewClass, 2;
    __a_SOMDServerMgrNewClass => SOMDServerMgrNewClass, 2;
    __a_TSIdentificationNewClass => TSIdentificationNewClass, 2;
    __a_ImplRepDistNewClass => ImplRepDistNewClass, 2;
}

optlink! {
    __o_somdExceptionCodeName => somdExceptionCodeName, 2;
    __o_somd_htonl => somd_htonl, 1;
    __o_somd_htons => somd_htons, 1;
    __o_somduppercase => somduppercase, 2;
    __o_somdlowercase => somdlowercase, 2;
    __o_somddbcsuppercase => somduppercase, 2;
    __o_somddbcslowercase => somdlowercase, 2;
    __o_somdstrcmp => somdstrcmp, 2;
    __o_somdGetenv => somdGetenv, 1;
    __o_somdisDbcs => somdisDbcs, 1;
    __o_somddbcsStrchr => somddbcsStrchr, 2;
    __o_somddbcsStrrchr => somddbcsStrrchr, 2;
    __o_somddbcsStrstr => somddbcsStrstr, 2;
    __o_somdMaxPathLen => somdMaxPathLen, 2;
    __o_somddbcsScan => fail, 0;
    __o_somddbcsPreincr => fail, 0;
    __o_somddbcsPostincr => fail, 0;
    __o_somddbcsLastChar => fail, 0;
    __o_somdPathGet => fail, 0;
    __o_somdError => fail, 0;
    __o_somdImplIdBinToStr => fail, 0;
    __o_somdLSRsp => fail, 0;
    __o_get_next_response => fail, 0;
    __o_send_multiple_requests => fail, 0;
}
