//! REXXWPS.DLL (OREXXWPS.DLL until SWITCHRX.CMD installs Object REXX):
//! Object REXX programs scripting the Workplace Shell, rs2's own.
//!
//! The DLL has two sides:
//!
//! - In the shell. WPSINST.CMD registers its classes WPZORXINIT and
//!   WPZORX (both WPAbstract subclasses); loading them (SOMInitModule)
//!   starts a server thread on the named pipe `\PIPE\RS2REXXWPS` that
//!   runs messages to the shell's SOM objects (through rxsom, with the
//!   Workplace Shell's method signatures below).
//! - In a REXX program (OREXX.DLL's bridge entries): `.WPS` (the server),
//!   `.WPDESKTOP` (the desktop folder) and the Workplace Shell's classes
//!   by name (`.WPFOLDER`...) are objects whose messages go to the server.
//!   `class~new(title, setup, folder, lock)` is wpclsNew.
//!
//! An object is its address in the shell process; nothing outside the
//! shell uses it but as a handle.

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, static_mut_refs, function_casts_as_integer)]

extern crate alloc;
extern crate os2;

use alloc::vec::Vec;
use core::ptr::null_mut;

use os2::*;
use rxsom::{Error, Val};
use somsup::{ClassSpec, aligned, build_class};

somsup::dll_entry!(init);

extern "C" fn init(_hmod: u32, _term: u32) -> u32 {
    1
}

const PIPE: &[u8] = b"\\PIPE\\RS2REXXWPS\0";

// ---- the classes WPSINST.CMD registers ----

#[unsafe(no_mangle)]
pub static mut WPZORXINITClassData: [usize; 1] = [0; 1];
#[unsafe(no_mangle)]
pub static mut WPZORXINITCClassData: [usize; 8] = [0; 8];
#[unsafe(no_mangle)]
pub static mut WPZORXClassData: [usize; 1] = [0; 1];
#[unsafe(no_mangle)]
pub static mut WPZORXCClassData: [usize; 8] = [0; 8];

macro_rules! p {
    ($a:ident) => {
        (&raw mut $a) as *mut usize
    };
}

pub extern "C" fn WPZORXINITNewClass(major: i32, minor: i32) -> usize {
    let c = build_class(&ClassSpec {
        name: "WPZORXINIT",
        parents: &["WPAbstract"],
        meta: None,
        major,
        minor,
        data: 0,
        align: 0,
        methods: &[],
        overrides: &[],
        cd: p!(WPZORXINITClassData),
        ccd: p!(WPZORXINITCClassData),
    });
    start_server();
    c
}

pub extern "C" fn WPZORXNewClass(major: i32, minor: i32) -> usize {
    let c = build_class(&ClassSpec {
        name: "WPZORX",
        parents: &["WPAbstract"],
        meta: None,
        major,
        minor,
        data: 0,
        align: 0,
        methods: &[],
        overrides: &[],
        cd: p!(WPZORXClassData),
        ccd: p!(WPZORXCClassData),
    });
    start_server();
    c
}

pub extern "C" fn SOMInitModule(_major: i32, _minor: i32, _class: *const u8) {
    WPZORXINITNewClass(1, 2);
    WPZORXNewClass(1, 2);
}

// ---- the Workplace Shell's method signatures (see rxsom) ----

pub const WPS_SIGS: &[(&str, &str)] = &[
    ("wpQueryTitle", "s()"),
    ("wpSetTitle", "b(s)"),
    ("wpSetup", "b(s)"),
    ("wpDelete", "L(L)"),
    ("wpFree", "b()"),
    ("wpQueryHandle", "L()"),
    ("wpQueryFolder", "o()"),
    ("wpOpen", "L(L,L,L)"),
    ("wpViewObject", "L(L,L,L)"),
    ("wpSwitchTo", "b(L)"),
    ("wpClose", "b()"),
    ("wpHide", "b()"),
    ("wpRestore", "b()"),
    ("wpSaveImmediate", "b()"),
    ("wpSaveDeferred", "b()"),
    ("wpQueryStyle", "L()"),
    ("wpSetStyle", "b(L)"),
    ("wpQueryIcon", "L()"),
    ("wpSetIcon", "b(L)"),
    ("wpQueryDefaultView", "L()"),
    ("wpSetDefaultView", "b(L)"),
    ("wpQueryConfirmations", "L()"),
    ("wpCopyObject", "o(o,b)"),
    ("wpMoveObject", "b(o)"),
    ("wpCreateShadowObject", "o(o,b)"),
    ("wpCreateAnother", "o(s,s,o)"),
    ("wpCreateFromTemplate", "o(o,b)"),
    ("wpQueryObjectID", "s()"),
    ("wpSetObjectID", "b(s)"),
    ("wpLockObject", "v()"),
    ("wpUnlockObject", "b()"),
    ("wpIsLocked", "b()"),
    ("wpIsObjectInitialized", "b()"),
    ("wpIsDeleteable", "b()"),
    ("wpConfirmDelete", "L(L)"),
    ("wpQueryNameClashOptions", "L()"),
    ("wpQueryMinWindow", "L()"),
    ("wpSetMinWindow", "b(L)"),
    ("wpQueryConcurrentView", "L()"),
    ("wpSetConcurrentView", "b(L)"),
    ("wpQueryButtonAppearance", "L()"),
    ("wpSetButtonAppearance", "v(L)"),
    ("wpQueryCoreRecord", "p()"),
    ("wpQueryTrueStyle", "L()"),
    ("wpDisplayHelp", "b(L,s)"),
    ("wpMenuItemSelected", "b(L,L)"),
    ("wpQueryContent", "o(o,L)"),
    ("wpPopulate", "b(L,s,b)"),
    ("wpAddToContent", "b(o)"),
    ("wpDeleteFromContent", "b(o)"),
    ("wpDeleteContents", "L(L)"),
    ("wpQueryFldrFlags", "L()"),
    ("wpSetFldrFlags", "b(L)"),
    ("wpQueryFldrFont", "s(L)"),
    ("wpSetFldrFont", "b(s,L)"),
    ("wpQueryType", "s()"),
    ("wpSetType", "b(s,p)"),
    ("wpQueryAttr", "L()"),
    ("wpSetAttr", "b(L)"),
    ("wpRefresh", "b(L,p)"),
    ("wpQueryShadowedObject", "o(b)"),
    ("wpSetLinkToObject", "b(o)"),
    ("wpQueryLogicalDrive", "L()"),
    ("wpQueryRootFolder", "o()"),
    ("wpclsNew", "o(s,s,o,b)"),
    ("wpclsQueryTitle", "s()"),
    ("wpclsQueryIcon", "L()"),
    ("wpclsQueryDefaultView", "L()"),
    ("wpclsQueryStyle", "L()"),
    ("wpclsQueryObject", "o(L)"),
    ("wpclsObjectFromHandle", "o(L)"),
    ("wpclsQueryFolder", "o(s,b)"),
    ("wpclsQueryButtonAppearance", "L()"),
    ("wpclsQueryInstanceType", "s()"),
    ("wpclsQueryInstanceFilter", "s()"),
    ("wpclsQueryObjectFromPath", "o(s)"),
];

// ---- the server (in the shell) ----

static mut STARTED: bool = false;

fn start_server() {
    unsafe {
        if STARTED {
            return;
        }
        STARTED = true;
        let mut tid = 0u32;
        DosCreateThread(&mut tid, server, 0, 0, 0x10000);
    }
}

type WinInitialize = extern "C" fn(u32) -> u32;
type WinCreateMsgQueue = extern "C" fn(u32, i32) -> u32;
type WinQueryObject = extern "C" fn(*const u8) -> u32;
type WinEnumObjectClasses = extern "C" fn(*mut u8, *mut u32) -> u32;

fn proc_of(module: &[u8], ord: u32) -> usize {
    let mut fail = [0u8; 32];
    let mut h = 0u32;
    if unsafe { DosLoadModule(fail.as_mut_ptr(), fail.len() as u32, module.as_ptr(), &mut h) } != 0 {
        return 0;
    }
    let mut p = 0usize;
    if unsafe { DosQueryProcAddr(h, ord, null_mut(), &mut p) } != 0 { 0 } else { p }
}

unsafe extern "C" fn server(_p: u32) {
    // a message queue, for the PM calls the shell's methods make
    let wi = proc_of(b"PMWIN\0", 763);
    let wq = proc_of(b"PMWIN\0", 716);
    if wi != 0 && wq != 0 {
        let (fi, fq) = unsafe { (core::mem::transmute::<usize, WinInitialize>(wi), core::mem::transmute::<usize, WinCreateMsgQueue>(wq)) };
        let hab = fi(0);
        fq(hab, 0);
    }
    let mut buf = alloc::vec![0u8; 0x10000];
    loop {
        let mut h = 0u32;
        // message mode, one instance: clients wait their turn
        let rc = unsafe { DosCreateNPipe(PIPE.as_ptr(), &mut h, NP_ACCESS_DUPLEX, NP_TYPE_MESSAGE | NP_READMODE_MESSAGE | 1, 0x10000, 0x10000, 0) };
        if rc != 0 {
            unsafe { DosSleep(1000) };
            continue;
        }
        loop {
            if unsafe { DosConnectNPipe(h) } != 0 {
                break;
            }
            let mut n = 0u32;
            if unsafe { DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } == 0 && n > 0 {
                let reply = handle(&buf[..n as usize]);
                let mut w = 0u32;
                unsafe { DosWrite(h, reply.as_ptr(), reply.len() as u32, &mut w) };
                // wait for the client to read the reply and close its end:
                // disconnecting would throw the reply away
                let mut m = 0u32;
                unsafe { DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut m) };
            }
            unsafe { DosDisConnectNPipe(h) };
        }
        unsafe { DosClose(h) };
    }
}

/// The server's `.WPS` object.
const ROOT: u32 = 1;

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> u8 {
        let v = self.b.get(self.i).copied().unwrap_or(0);
        self.i += 1;
        v
    }
    fn u16(&mut self) -> u16 {
        let v = u16::from_le_bytes([self.u8(), self.u8()]);
        v
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes([self.u8(), self.u8(), self.u8(), self.u8()])
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        let e = (self.i + n).min(self.b.len());
        let v = self.b[self.i.min(e)..e].to_vec();
        self.i = e;
        v
    }
}

/// A reply: rc, then the value (kind, handle, string).
fn reply(rc: u32, kind: u8, h: u32, s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(13 + s.len());
    v.extend_from_slice(&rc.to_le_bytes());
    v.push(kind);
    v.extend_from_slice(&h.to_le_bytes());
    v.extend_from_slice(&(s.len() as u32).to_le_bytes());
    v.extend_from_slice(s);
    v
}

fn reply_val(v: Val) -> Vec<u8> {
    match v {
        Val::None => reply(0, 0, 0, b""),
        Val::Nil => reply(0, 5, 0, b""),
        Val::Str(s) => reply(0, 1, 0, &s),
        Val::Obj(o) => reply(0, 2, o as u32, &rxsom::class_name_of(o)),
        Val::Class(c) => reply(0, 3, c as u32, &rxsom::class_id(c)),
    }
}

/// A Workplace Shell class by name in any case.
fn wps_class(upper: &[u8]) -> usize {
    let f = proc_of(b"PMWP\0", 205);
    if f == 0 {
        return 0;
    }
    let enumc = unsafe { core::mem::transmute::<usize, WinEnumObjectClasses>(f) };
    let mut size = 0u32;
    enumc(null_mut(), &mut size);
    let mut buf = alloc::vec![0u8; size as usize + 16];
    size = buf.len() as u32;
    if enumc(buf.as_mut_ptr(), &mut size) == 0 {
        return 0;
    }
    let mut e = buf.as_ptr() as *const u32;
    while !e.is_null() {
        let name = unsafe { somsup::cstr(*e.add(1) as *const u8) };
        if somsup::eq_nocase(name, upper) {
            return rxsom::find_class(name);
        }
        e = unsafe { *e } as *const u32;
    }
    // classes of SOM itself
    0
}

/// The desktop folder.
fn desktop() -> usize {
    let f = proc_of(b"PMWP\0", 252);
    if f == 0 {
        return 0;
    }
    let h = unsafe { core::mem::transmute::<usize, WinQueryObject>(f) }(b"<WP_DESKTOP>\0".as_ptr());
    if h == 0 {
        return 0;
    }
    let cls = rxsom::find_class(b"WPObject");
    match rxsom::call(cls, b"wpclsQueryObject", "o(L)", &[Val::Str(alloc::format!("{h}").into_bytes())], &|_| None) {
        Ok(Val::Obj(o)) => o,
        _ => 0,
    }
}

fn handle(req: &[u8]) -> Vec<u8> {
    let mut r = Rd { b: req, i: 0 };
    let op = r.u8();
    let h = r.u32();
    let nl = r.u16() as usize;
    let name = r.bytes(nl);
    let argc = r.u16() as usize;
    let mut args = Vec::new();
    for _ in 0..argc {
        let k = r.u8();
        let ah = r.u32();
        let n = r.u32() as usize;
        let s = r.bytes(n);
        args.push(match k {
            1 => Val::Str(s),
            2 => Val::Obj(ah as usize),
            3 => Val::Class(ah as usize),
            _ => Val::None,
        });
    }
    match op {
        // lookup an environment symbol
        1 => match name.as_slice() {
            b"WPS" => reply(0, 2, ROOT, b"WPS"),
            b"WPDESKTOP" => match desktop() {
                0 => reply(1, 0, 0, b""),
                o => reply_val(Val::Obj(o)),
            },
            n => match wps_class(n) {
                0 => reply(1, 0, 0, b""),
                c => reply_val(Val::Class(c)),
            },
        },
        // a class by name (EXTERNAL 'WPS name')
        3 => {
            let mut c = rxsom::find_class(&name);
            if c == 0 {
                c = wps_class(&name.to_ascii_uppercase());
            }
            if c == 0 { reply(1, 0, 0, b"") } else { reply_val(Val::Class(c)) }
        }
        // a message
        _ => {
            let res: Result<Val, Error> = if h == ROOT {
                Err(Error::NoMethod)
            } else if name == b"NEW" && somsup::is_a(h as usize, unsafe { *somsup::sc() }) {
                if args.is_empty() {
                    let o = somsup::new_object(h as usize);
                    Ok(if o == 0 { Val::Nil } else { Val::Obj(o) })
                } else {
                    rxsom::call(h as usize, b"wpclsNew", "o(s,s,o,b)", &args, &|_| None)
                }
            } else {
                rxsom::send(h as usize, &name, &args, &[WPS_SIGS, rxsom::SOM_SIGS], &|_| None)
            };
            match res {
                Ok(v) => reply_val(v),
                Err(Error::NoMethod) => reply(1, 0, 0, b""),
                Err(Error::Rexx(code, m)) => reply(2, 1, code, &m),
            }
        }
    }
}

// ---- the bridge (in a REXX program) ----

#[repr(C)]
pub struct ExtVal {
    pub kind: u32,
    pub len: u32,
    pub ptr: *mut u8,
    pub handle: u32,
}

fn request(op: u8, h: u32, name: &[u8], args: &[(u8, u32, Vec<u8>)]) -> Option<(u32, u8, u32, Vec<u8>)> {
    let mut q = Vec::new();
    q.push(op);
    q.extend_from_slice(&h.to_le_bytes());
    q.extend_from_slice(&(name.len() as u16).to_le_bytes());
    q.extend_from_slice(name);
    q.extend_from_slice(&(args.len() as u16).to_le_bytes());
    for (k, ah, s) in args {
        q.push(*k);
        q.extend_from_slice(&ah.to_le_bytes());
        q.extend_from_slice(&(s.len() as u32).to_le_bytes());
        q.extend_from_slice(s);
    }
    let mut out = alloc::vec![0u8; 0x10000];
    // the server may be starting (WPSINST.CMD just registered the classes)
    for _ in 0..40 {
        let mut n = 0u32;
        let rc = unsafe { DosCallNPipe(PIPE.as_ptr(), q.as_ptr(), q.len() as u32, out.as_mut_ptr(), out.len() as u32, &mut n, 10000) };
        if rc == 0 && n >= 13 {
            let mut r = Rd { b: &out[..n as usize], i: 0 };
            let rc = r.u32();
            let k = r.u8();
            let vh = r.u32();
            let len = r.u32() as usize;
            let s = r.bytes(len);
            return Some((rc, k, vh, s));
        }
        unsafe { DosSleep(50) };
    }
    None
}

fn put_str(dst: *mut u8, cb: u32, s: &[u8]) {
    if dst.is_null() || cb == 0 {
        return;
    }
    let n = s.len().min(cb as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), dst, n);
        *dst.add(n) = 0;
    }
}

extern "C" fn ext_lookup(name: *const u8, h: *mut u32, is_class: *mut u32, id: *mut u8, cbid: u32) -> u32 {
    let n = unsafe { somsup::cstr(name) }.to_vec();
    match request(1, 0, &n, &[]) {
        Some((0, k, vh, s)) if k == 2 || k == 3 => {
            unsafe {
                *h = vh;
                *is_class = (k == 3) as u32;
            }
            put_str(id, cbid, &s);
            1
        }
        _ => 0,
    }
}

extern "C" fn ext_class(spec: *const u8, h: *mut u32, id: *mut u8, cbid: u32) -> u32 {
    let n: Vec<u8> = unsafe { somsup::cstr(spec) }.iter().copied().take_while(|&c| c != b' ').collect();
    match request(3, 0, &n, &[]) {
        Some((0, 3, vh, s)) => {
            unsafe { *h = vh };
            put_str(id, cbid, &s);
            1
        }
        _ => 0,
    }
}

extern "C" fn ext_send(h: u32, msg: *const u8, argc: u32, args: *const ExtVal, res: *mut ExtVal) -> u32 {
    let m = unsafe { somsup::cstr(msg) }.to_vec();
    let mut av = Vec::new();
    for i in 0..argc as usize {
        let a = unsafe { &*args.add(i) };
        let s = if a.ptr.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(a.ptr, a.len as usize).to_vec() } };
        av.push((a.kind as u8, a.handle, s));
    }
    let Some((rc, k, vh, s)) = request(2, h, &m, &av) else {
        let t = b"The Workplace Shell's REXX server does not answer (run WPSINST.CMD)";
        unsafe {
            (*res).kind = 1;
            (*res).len = t.len() as u32;
            (*res).ptr = somsup::dup(t);
            (*res).handle = 98903;
        }
        return 2;
    };
    unsafe {
        (*res).kind = k as u32;
        (*res).len = s.len() as u32;
        (*res).ptr = if s.is_empty() && k != 1 { null_mut() } else { somsup::dup(&s) };
        (*res).handle = vh;
    }
    rc
}

extern "C" fn ext_free(p: *mut u8) {
    if !p.is_null() {
        somsup::free(p);
    }
}

aligned! {
    __a_SOMInitModule => SOMInitModule, 3;
    __a_WPZORXINITNewClass => WPZORXINITNewClass, 2;
    __a_WPZORXNewClass => WPZORXNewClass, 2;
    rs2_ext_lookup => ext_lookup, 5;
    rs2_ext_class => ext_class, 4;
    rs2_ext_send => ext_send, 5;
    rs2_ext_free => ext_free, 1;
}
