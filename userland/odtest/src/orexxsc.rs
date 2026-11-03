//! `ODTEST OREXXSC`: IBM's Object REXX scripting component (OREXXSC.DLL,
//! in `C:\ORX`) on rs2's OPENDOC.DLL, SOMUC.DLL and Object REXX: OREXX.DLL
//! is copied to `C:\ORX\REXX.DLL` (the module REXX that OREXXSC imports)
//! and `C:\ORX` is put in front of LIBPATH. REXX scripts are compiled,
//! run, shown, stored, loaded and copied through OSAScriptingComponent's
//! methods; errors come back through OSAScriptError.

use alloc::vec::Vec;

use os2::*;

use crate::{check, ev, ok_ev};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Desc {
    ty: u32,
    h: usize,
}

unsafe extern "C" {
    #[link_name = "OPENDOC.7"]
    fn AECreateDesc(ty: u32, p: *const u8, n: usize, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.16"]
    fn AEDisposeDesc(d: *mut Desc) -> i32;
    #[link_name = "OPENDOC.24"]
    fn AEGetDescData(d: *const Desc, tyo: *mut u32, p: *mut u8, max: usize, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.33"]
    fn AEGetParamPtr(e: *const Desc, key: u32, ty: u32, tyo: *mut u32, p: *mut u8, max: usize, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.38"]
    fn AEInstallEventHandler(class: u32, id: u32, f: usize, refcon: usize, sys: u32) -> i32;
    #[link_name = "OPENDOC.51"]
    fn AEPutParamDesc(e: *const Desc, key: u32, d: *const Desc) -> i32;
    #[link_name = "OPENDOC.528"]
    static OSAScriptingComponentClassData: [usize; 44];
    #[link_name = "OPENDOC.529"]
    fn OSAScriptingComponentNewClass(major: i32, minor: i32) -> usize;
}

/// Four-character codes as OS/2 OpenDoc has them (memory order; 'long'
/// and 'shor' are reversed).
const fn c(s: &[u8; 4]) -> u32 {
    match s {
        b"long" => 0x6c6f_6e67,
        b"shor" => 0x7368_6f72,
        _ => u32::from_le_bytes(*s),
    }
}

const COMPILE: usize = 7;
const COPY_ID: usize = 9;
const DISPLAY: usize = 10;
const DISPOSE: usize = 11;
const DO_SCRIPT: usize = 13;
const EXECUTE: usize = 14;
const GET_SOURCE: usize = 23;
const LOAD: usize = 24;
const SCRIPT_ERROR: usize = 27;
const NAME: usize = 28;
const STORE: usize = 38;

fn osa(i: usize, args: &[usize]) -> i32 {
    somsup::call(unsafe { (&raw const OSAScriptingComponentClassData) as *const usize }, i, args) as i32
}

fn text(s: &[u8]) -> Desc {
    let mut d = Desc::default();
    unsafe { AECreateDesc(c(b"TEXT"), s.as_ptr(), s.len(), &mut d) };
    d
}

fn data(d: &Desc) -> (u32, Vec<u8>) {
    let (mut ty, mut n) = (0u32, 0u32);
    let mut buf = alloc::vec![0u8; 4096];
    unsafe { AEGetDescData(d, &mut ty, buf.as_mut_ptr(), buf.len(), &mut n) };
    buf.truncate((n as usize).min(4096));
    (ty, buf)
}

fn dp(d: &mut Desc) -> usize {
    d as *mut Desc as usize
}

/// Compile and run a script; its result as text, or the OSA error.
fn run(sc: usize, src: &[u8]) -> Result<Vec<u8>, i32> {
    let e = ev();
    let mut s = text(src);
    let mut id = 0usize;
    let r = osa(COMPILE, &[sc, e, dp(&mut s), 0, &mut id as *mut usize as usize]);
    unsafe { AEDisposeDesc(&mut s) };
    if r != 0 {
        return Err(r);
    }
    let mut res = 0usize;
    let r = osa(EXECUTE, &[sc, e, id, 0, 0, &mut res as *mut usize as usize]);
    osa(DISPOSE, &[sc, e, id]);
    if r != 0 {
        return Err(r);
    }
    let mut t = Desc::default();
    let r = osa(DISPLAY, &[sc, e, res, c(b"TEXT") as usize, 0, dp(&mut t)]);
    osa(DISPOSE, &[sc, e, res]);
    if r != 0 {
        return Err(r);
    }
    let out = data(&t).1;
    unsafe { AEDisposeDesc(&mut t) };
    Ok(out)
}

fn script_error(sc: usize, sel: &[u8; 4], ty: &[u8; 4]) -> Option<(u32, Vec<u8>)> {
    let mut d = Desc::default();
    let r = osa(SCRIPT_ERROR, &[sc, ev(), c(sel) as usize, c(ty) as usize, dp(&mut d)]);
    if r != 0 {
        return None;
    }
    let out = data(&d);
    unsafe { AEDisposeDesc(&mut d) };
    Some(out)
}

// ---- an application to script: this program ----

fn pstr(v: &mut Vec<u8>, s: &[u8]) {
    v.push(s.len() as u8);
    v.extend_from_slice(s);
}

fn align(v: &mut Vec<u8>) {
    if v.len() & 1 != 0 {
        v.push(0);
    }
}

/// An event of the terminology: name, class, ID, reply type, direct
/// parameter type ('null': none).
fn aete_event(v: &mut Vec<u8>, name: &[u8], class: u32, id: u32, reply: u32, direct: u32) {
    pstr(v, name);
    pstr(v, b"");
    align(v);
    v.extend_from_slice(&class.to_le_bytes());
    v.extend_from_slice(&id.to_le_bytes());
    v.extend_from_slice(&reply.to_le_bytes());
    pstr(v, b"");
    align(v);
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&direct.to_le_bytes());
    pstr(v, b"");
    align(v);
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes()); // no other parameters
}

/// ODTEST's terminology ('aete'): a suite with "hello" (reply TEXT) and
/// "twice" (a long in, a long out).
fn aete() -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // one suite
    pstr(&mut v, b"ODTEST suite");
    pstr(&mut v, b"");
    align(&mut v);
    v.extend_from_slice(&c(b"RS2T").to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes()); // events
    aete_event(&mut v, b"hello", c(b"RS2T"), c(b"hllo"), c(b"TEXT"), c(b"null"));
    aete_event(&mut v, b"twice", c(b"RS2T"), c(b"dubl"), c(b"long"), c(b"long"));
    for _ in 0..4 {
        v.extend_from_slice(&0u16.to_le_bytes()); // classes, comparisons, enumerations, ...
    }
    v
}

extern "C" fn get_aete(_event: *const Desc, reply: *mut Desc, _refcon: usize) -> i32 {
    let a = aete();
    let mut d = Desc::default();
    unsafe {
        AECreateDesc(c(b"aete"), a.as_ptr(), a.len(), &mut d);
        AEPutParamDesc(reply, c(b"----"), &d);
        AEDisposeDesc(&mut d);
    }
    0
}

extern "C" fn hello(_event: *const Desc, reply: *mut Desc, _refcon: usize) -> i32 {
    let mut d = text(b"hello from ODTEST");
    unsafe {
        AEPutParamDesc(reply, c(b"----"), &d);
        AEDisposeDesc(&mut d);
    }
    0
}

extern "C" fn twice(event: *const Desc, reply: *mut Desc, _refcon: usize) -> i32 {
    let (mut ty, mut n, mut v) = (0u32, 0u32, 0i32);
    let r = unsafe { AEGetParamPtr(event, c(b"----"), c(b"long"), &mut ty, &mut v as *mut i32 as *mut u8, 4, &mut n) };
    if r != 0 {
        return r;
    }
    let v2 = v * 2;
    let mut d = Desc::default();
    unsafe {
        AECreateDesc(c(b"long"), &v2 as *const i32 as *const u8, 4, &mut d);
        AEPutParamDesc(reply, c(b"----"), &d);
        AEDisposeDesc(&mut d);
    }
    0
}

/// Scripts that send Apple events to this program through IBM's
/// ScriptableApp natives and rs2's terminology classes.
fn scriptable_app(sc: usize) {
    unsafe {
        AEInstallEventHandler(c(b"ascr"), c(b"gdte"), get_aete as usize, 0, 0);
        AEInstallEventHandler(c(b"RS2T"), c(b"hllo"), hello as usize, 0, 0);
        AEInstallEventHandler(c(b"RS2T"), c(b"dubl"), twice as usize, 0, 0);
    }
    let r = run(sc, b"app = .ScriptableApp~new~connect('ODTEST')\r\nreturn app~hello");
    check("a script connects to this program and sends it an event (ScriptableApp, its terminology)", r.as_deref() == Ok(&b"hello from ODTEST"[..]));
    let r = run(sc, b"return .ScriptableApp~new~connect('ODTEST')~twice(21)");
    check("an event with a direct parameter and a reply", r.as_deref() == Ok(&b"42"[..]));
    let r = run(sc, b"return .ScriptableApp~new~connect('ODTEST')~nosuchevent");
    check("an event the terminology does not have is an error", r == Err(-1753));
}

pub fn orexxsc() {
    let e = ev();
    unsafe {
        DosCopy(b"C:\\OS2\\DLL\\OREXX.DLL\0".as_ptr(), b"C:\\ORX\\REXX.DLL\0".as_ptr(), 1);
        DosSetExtLIBPATH(b"C:\\ORX\0".as_ptr(), 1);
        OSAScriptingComponentNewClass(1, 0);
    }
    let mgr = unsafe { somsup::SOMClassMgrObject };
    let file = b"OREXXSC\0";
    let cls = somsup::call(somsup::scm(), somsup::mm::SOM_FIND_CLS_IN_FILE, &[mgr, somsup::id(b"REXXScriptingComponent") as usize, 0, 0, file.as_ptr() as usize]);
    check("OREXXSC loads: REXXScriptingComponent, an OSAScriptingComponent", cls != 0 && unsafe { somsup::is_a(somsup::new_object(cls), OSAScriptingComponentClassData[0]) });
    if cls == 0 {
        return;
    }
    let sc = somsup::new_object(cls);
    let mut name = Desc::default();
    let r = osa(NAME, &[sc, e, dp(&mut name)]);
    check("OSAScriptingComponentName", r == 0 && data(&name).1 == b"Object REXX");
    let r = run(sc, b"return 6*7");
    check("OSACompile, OSAExecute, OSADisplay: a script's result", r.as_deref() == Ok(&b"42"[..]));
    let r = run(sc, b"parse version v\r\nreturn word(v, 1)");
    check("the script runs on Object REXX", r.as_deref() == Ok(&b"OBJREXX"[..]));
    let r = run(sc, b"return .twice~new~of(21)\r\n::class twice\r\n::method of\r\n  return arg(1) * 2");
    check("with directives (::CLASS, ::METHOD)", r.as_deref() == Ok(&b"42"[..]));
    // OSADoScript: compile, execute and display at once
    let mut s = text(b"return 'Hello' || ' ' || 'OpenDoc'");
    let mut out = Desc::default();
    let r = osa(DO_SCRIPT, &[sc, e, dp(&mut s), 0, c(b"TEXT") as usize, 0, dp(&mut out)]);
    check("OSADoScript", r == 0 && data(&out).1 == b"Hello OpenDoc");
    // the source, a copy, storing and loading
    let mut id = 0usize;
    osa(COMPILE, &[sc, e, dp(&mut s), 0, &mut id as *mut usize as usize]);
    let mut src = Desc::default();
    let r1 = osa(GET_SOURCE, &[sc, e, id, c(b"TEXT") as usize, dp(&mut src)]);
    let mut copy = 0usize;
    let r2 = osa(COPY_ID, &[sc, e, id, &mut copy as *mut usize as usize]);
    check("OSAGetSource and OSACopyID", r1 == 0 && data(&src).1 == b"return 'Hello' || ' ' || 'OpenDoc'" && r2 == 0 && copy != 0 && copy != id);
    let mut stored = Desc::default();
    let r3 = osa(STORE, &[sc, e, id, c(b"orex") as usize, 0, dp(&mut stored)]);
    let mut loaded = 0usize;
    let r4 = osa(LOAD, &[sc, e, dp(&mut stored), 0, &mut loaded as *mut usize as usize]);
    let mut res = 0usize;
    let r5 = osa(EXECUTE, &[sc, e, loaded, 0, 0, &mut res as *mut usize as usize]);
    let mut shown = Desc::default();
    osa(DISPLAY, &[sc, e, res, c(b"TEXT") as usize, 0, dp(&mut shown)]);
    check(
        "OSAStore ('orex', not the source) and OSALoad run again",
        r3 == 0 && stored.ty == c(b"orex") && data(&stored).1 != data(&src).1 && r4 == 0 && r5 == 0 && data(&shown).1 == b"Hello OpenDoc",
    );
    for x in [id, copy, loaded, res] {
        osa(DISPOSE, &[sc, e, x]);
    }
    // errors
    let r = run(sc, b"say 'unfinished");
    let n = script_error(sc, b"errn", b"shor");
    let msg = script_error(sc, b"errs", b"TEXT");
    check("a syntax error: errOSAScriptError, its number and message (OSAScriptError)", r == Err(-1753) && n.as_ref().map(|x| &x.1[..2]) == Some(&6i16.to_le_bytes()[..]) && msg.is_some_and(|m| !m.1.is_empty()));
    let r = run(sc, b"x = 1\r\nreturn x / 0");
    let n = script_error(sc, b"errn", b"shor");
    check("a run-time error (division by zero: 42)", r == Err(-1753) && n.map(|x| x.1[..2].to_vec()) == Some(42i16.to_le_bytes().to_vec()));
    scriptable_app(sc);
    for d in [&mut name, &mut s, &mut out, &mut src, &mut stored, &mut shown] {
        unsafe { AEDisposeDesc(d) };
    }
    ok_ev("OREXXSC");
}
