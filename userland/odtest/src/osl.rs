//! Checks of AEResolve with object accessors, and of Apple events to
//! another process (`ODTEST AERECV`, started by these checks, receives
//! them).

use alloc::vec::Vec;

use os2::*;

use crate::check;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Desc {
    ty: u32,
    h: usize,
}

unsafe extern "C" {
    #[link_name = "OPENDOC.7"]
    fn AECreateDesc(ty: u32, p: *const u8, n: usize, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.10"]
    fn AECreateAppleEvent(class: u32, id: u32, target: *const Desc, rid: i32, tid: i32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.11"]
    fn CreateObjSpecifier(class: u32, cont: *mut Desc, form: u32, key: *mut Desc, dispose: u32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.12"]
    fn CreateOffsetDescriptor(offset: i32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.16"]
    fn AEDisposeDesc(d: *mut Desc) -> i32;
    #[link_name = "OPENDOC.17"]
    fn AEDisposeToken(d: *mut Desc) -> i32;
    #[link_name = "OPENDOC.24"]
    fn AEGetDescData(d: *const Desc, tyo: *mut u32, p: *mut u8, max: usize, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.32"]
    fn AEGetParamDesc(e: *const Desc, key: u32, ty: u32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.33"]
    fn AEGetParamPtr(e: *const Desc, key: u32, ty: u32, tyo: *mut u32, p: *mut u8, max: usize, n: *mut u32) -> i32;
    #[link_name = "OPENDOC.38"]
    fn AEInstallEventHandler(class: u32, id: u32, f: usize, refcon: usize, sys: u32) -> i32;
    #[link_name = "OPENDOC.39"]
    fn AEInstallObjectAccessor(class: u32, container: u32, f: usize, refcon: usize, sys: u32) -> i32;
    #[link_name = "OPENDOC.52"]
    fn AEPutParamPtr(e: *const Desc, key: u32, ty: u32, p: *const u8, n: usize) -> i32;
    #[link_name = "OPENDOC.59"]
    fn AEResolve(spec: *const Desc, flags: u32, token: *mut Desc) -> i32;
    #[link_name = "OPENDOC.61"]
    fn AESend(e: *const Desc, reply: *mut Desc, mode: u32, prio: i32, timeout: i32, idle: usize, filter: usize) -> i32;
    #[link_name = "OPENDOC.71"]
    fn AEGetProcessName(pid: u32, buf: *mut u8, info: usize) -> i32;
    #[link_name = "PMWIN.763"]
    fn WinInitialize(opts: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, cmsg: i32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut [u32; 8], hwnd: u32, first: u32, last: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const [u32; 8]) -> u32;
    #[link_name = "PMWIN.902"]
    fn WinPostQueueMsg(hmq: u32, msg: u32, mp1: usize, mp2: usize) -> u32;
}

const fn c(s: &[u8; 4]) -> u32 {
    match s {
        b"long" => 0x6c6f_6e67,
        b"shor" => 0x7368_6f72,
        _ => u32::from_le_bytes(*s),
    }
}

fn data(d: &Desc) -> (u32, Vec<u8>) {
    let (mut ty, mut n) = (0u32, 0u32);
    let mut buf = [0u8; 64];
    unsafe { AEGetDescData(d, &mut ty, buf.as_mut_ptr(), buf.len(), &mut n) };
    (ty, buf[..(n as usize).min(64)].to_vec())
}

fn long_of(d: &Desc) -> i32 {
    let b = data(d).1;
    if b.len() >= 4 { i32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { i32::MIN }
}

// ---- AEResolve ----

/// Parts by absolute position in the application: a 'cpar' token
/// holding the index.
extern "C" fn part_accessor(_class: u32, _ctok: *const Desc, _cclass: u32, form: u32, key: *const Desc, tok: *mut Desc, refcon: usize) -> i32 {
    if form != c(b"indx") {
        return -1708;
    }
    let i = long_of(unsafe { &*key }) + refcon as i32;
    unsafe { AECreateDesc(c(b"cpar"), &i as *const i32 as *const u8, 4, tok) }
}

/// A property of a part: a 'prop' token holding the part index and the
/// property's code.
extern "C" fn prop_accessor(_class: u32, ctok: *const Desc, _cclass: u32, form: u32, key: *const Desc, tok: *mut Desc, _refcon: usize) -> i32 {
    if form != c(b"prop") {
        return -1708;
    }
    let part = long_of(unsafe { &*ctok });
    let code = long_of(unsafe { &*key });
    let v = [part, code];
    unsafe { AECreateDesc(c(b"prop"), v.as_ptr() as *const u8, 8, tok) }
}

pub fn osl_test() {
    unsafe {
        AEInstallObjectAccessor(c(b"cpar"), c(b"null"), part_accessor as usize, 100, 0);
        AEInstallObjectAccessor(c(b"prop"), c(b"cpar"), prop_accessor as usize, 0, 0);
        // "text of part 2"
        let mut null = Desc { ty: c(b"null"), h: 0 };
        let mut two = Desc::default();
        CreateOffsetDescriptor(2, &mut two);
        let mut part = Desc::default();
        CreateObjSpecifier(c(b"cpar"), &mut null, c(b"indx"), &mut two, 1, &mut part);
        let code = c(b"ctxt");
        let mut key = Desc::default();
        AECreateDesc(c(b"type"), &code as *const u32 as *const u8, 4, &mut key);
        let mut text = Desc::default();
        CreateObjSpecifier(c(b"prop"), &mut part, c(b"prop"), &mut key, 1, &mut text);
        let mut tok = Desc::default();
        let r = AEResolve(&text, 0, &mut tok);
        let d = data(&tok).1;
        check(
            "AEResolve: 'text of part 2' through the part and property accessors",
            r == 0 && tok.ty == c(b"prop") && d.len() == 8 && d[..4] == 102i32.to_le_bytes() && d[4..] == code.to_le_bytes(),
        );
        AEDisposeToken(&mut tok);
        // a class nobody accesses
        let mut null2 = Desc { ty: c(b"null"), h: 0 };
        let mut one = Desc::default();
        CreateOffsetDescriptor(1, &mut one);
        let mut win = Desc::default();
        CreateObjSpecifier(c(b"cwin"), &mut null2, c(b"indx"), &mut one, 1, &mut win);
        let r2 = AEResolve(&win, 0, &mut tok);
        check("AEResolve without an accessor: errAEAccessorNotFound", r2 == -1723);
        AEDisposeDesc(&mut text);
        AEDisposeDesc(&mut win);
    }
    cross_process();
}

// ---- another process ----

extern "C" fn twice(event: *const Desc, reply: *mut Desc, _r: usize) -> i32 {
    let (mut ty, mut n, mut v) = (0u32, 0u32, 0i32);
    let r = unsafe { AEGetParamPtr(event, c(b"----"), c(b"long"), &mut ty, &mut v as *mut i32 as *mut u8, 4, &mut n) };
    if r != 0 {
        return r;
    }
    let v2 = v * 2;
    unsafe { AEPutParamPtr(reply, c(b"----"), c(b"long"), &v2 as *const i32 as *const u8, 4) }
}

static mut HMQ: u32 = 0;

extern "C" fn quit(_e: *const Desc, _reply: *mut Desc, _r: usize) -> i32 {
    unsafe { WinPostQueueMsg(HMQ, 0x2a, 0, 0) }; // WM_QUIT
    0
}

/// `ODTEST AERECV`: receive Apple events until 'RS2T'/'quit'.
pub fn receiver() {
    unsafe {
        let hab = WinInitialize(0);
        HMQ = WinCreateMsgQueue(hab, 0);
        // a session with PM: the receiver needs its anchor block
        let s = crate::new_session();
        somsup::call(crate::session_cd(), 2, &[s, crate::ev(), hab as usize]); // SetHAB
        AEInstallEventHandler(c(b"RS2T"), c(b"dubl"), twice as usize, 0, 0);
        AEInstallEventHandler(c(b"RS2T"), c(b"quit"), quit as usize, 0, 0);
        let mut q = [0u32; 8];
        while WinGetMsg(hab, &mut q, 0, 0, 0) != 0 {
            WinDispatchMsg(hab, &q);
        }
    }
}

fn event_to(pid: u32, id: &[u8; 4]) -> Desc {
    let mut target = Desc::default();
    let mut e = Desc::default();
    unsafe {
        AECreateDesc(c(b"PID "), &pid as *const u32 as *const u8, 4, &mut target);
        AECreateAppleEvent(c(b"RS2T"), c(id), &target, -1, 0, &mut e);
        AEDisposeDesc(&mut target);
    }
    e
}

fn cross_process() {
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let args = b"ODTEST.EXE\0AERECV\0\0";
    let r = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 2, args.as_ptr(), core::ptr::null(), &mut res, b"C:\\RS2\\ODTEST.EXE\0".as_ptr()) };
    let pid = res.code_terminate;
    // its receiver is up once it is in the table
    let mut name = [0u8; 64];
    let mut up = false;
    for _ in 0..100 {
        if unsafe { AEGetProcessName(pid, name.as_mut_ptr(), 0) } == 0 {
            up = true;
            break;
        }
        unsafe { DosSleep(100) };
    }
    check("another process receives Apple events (ODTEST AERECV: by PID and name)", r == 0 && up && name.starts_with(b"ODTEST.EXE"));
    if !up {
        return;
    }
    let mut e = event_to(pid, b"dubl");
    let v = 21i32;
    let mut reply = Desc::default();
    let mut ans = Desc::default();
    unsafe {
        AEPutParamPtr(&e, c(b"----"), c(b"long"), &v as *const i32 as *const u8, 4);
        let r = AESend(&e, &mut reply, 0x23, 0, -1, 0, 0);
        AEGetParamDesc(&reply, c(b"----"), c(b"long"), &mut ans);
        check("AESend to another process: its handler's reply comes back", r == 0 && long_of(&ans) == 42);
        AEDisposeDesc(&mut e);
        AEDisposeDesc(&mut reply);
        AEDisposeDesc(&mut ans);
        let mut q = event_to(pid, b"quit");
        let r = AESend(&q, &mut reply, 0x23, 0, -1, 0, 0);
        AEDisposeDesc(&mut q);
        let mut rc = ResultCodes { code_terminate: 0, code_result: 0 };
        let mut got = 0u32;
        let w = DosWaitChild(0, 0, &mut rc, &mut got, pid);
        check("and it ends when asked to ('quit')", r == 0 && w == 0);
    }
}
