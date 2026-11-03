//! Scripting ODSHELL's document from REXX: Document/Run script runs
//! `C:\ODSCRIPT.CMD` through the OpenDoc scripting component (IBM's
//! OREXXSC.DLL, REXXScriptingComponent, loaded on first use from
//! `C:\ORX` with rs2's Object REXX as its REXX.DLL), and ODSHELL answers
//! the Apple events a script sends it (`.ScriptableApp~new~connect('ODSHELL')`):
//!
//! - 'ascr'/'gdte': its terminology ('aete'): the class "part" (1: the
//!   root part, then the embedded parts) with "text" and "name", the core
//!   suite's "get" and "set ... to" (object specifiers resolved by
//!   AEResolve through ODSHELL's object accessors), "settext" (the root
//!   part's text, a direct TEXT parameter), "gettext" (replies TEXT),
//!   "play" (the embedded IBM MMAudioPart plays: a click in its facet,
//!   as its Play command needs the play palette its activated frame has).
//!
//! The script's result goes to the status line. ODSHELL installs its
//! handlers at startup, so REXX programs in other processes (run by
//! CMD) can drive it too (ipc.rs in OPENDOC.DLL).

use alloc::vec::Vec;


use crate::*;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Desc {
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
    #[link_name = "OPENDOC.32"]
    fn AEGetParamDesc(e: *const Desc, key: u32, ty: u32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.39"]
    fn AEInstallObjectAccessor(class: u32, container: u32, f: usize, refcon: usize, sys: u32) -> i32;
    #[link_name = "OPENDOC.59"]
    fn AEResolve(spec: *const Desc, flags: u32, token: *mut Desc) -> i32;
    #[link_name = "OPENDOC.10"]
    fn AECreateAppleEvent(class: u32, id: u32, target: *const Desc, rid: i32, tid: i32, r: *mut Desc) -> i32;
    #[link_name = "OPENDOC.61"]
    fn AESend(e: *const Desc, reply: *mut Desc, mode: u32, prio: i32, timeout: i32, idle: usize, filter: usize) -> i32;
    #[link_name = "OPENDOC.51"]
    fn AEPutParamDesc(e: *const Desc, key: u32, d: *const Desc) -> i32;
    #[link_name = "OPENDOC.528"]
    static OSAScriptingComponentClassData: [usize; 44];
    #[link_name = "OPENDOC.529"]
    fn OSAScriptingComponentNewClass(major: i32, minor: i32) -> usize;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(hwnd: u32, id: u32) -> u32;
    #[link_name = "PMWIN.765"]
    fn WinInvalidateRect(hwnd: u32, rect: usize, children: u32) -> u32;
    #[link_name = "OPENDOC.330"]
    static ODFacetIteratorClassData: [usize; 5];
}

/// Four-character codes as OS/2 OpenDoc has them.
const fn c(s: &[u8; 4]) -> u32 {
    match s {
        b"long" => 0x6c6f_6e67,
        b"shor" => 0x7368_6f72,
        _ => u32::from_le_bytes(*s),
    }
}

fn pstr(v: &mut Vec<u8>, s: &[u8]) {
    v.push(s.len() as u8);
    v.extend_from_slice(s);
}

fn align(v: &mut Vec<u8>) {
    if v.len() & 1 != 0 {
        v.push(0);
    }
}

fn u16le(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn u32le(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// An event: name, class and ID, reply type, direct parameter type, other
/// parameters (name, keyword, type).
fn event(v: &mut Vec<u8>, name: &[u8], class: &[u8; 4], id: &[u8; 4], reply: u32, direct: u32, params: &[(&[u8], u32, u32)]) {
    pstr(v, name);
    pstr(v, b"");
    align(v);
    u32le(v, c(class));
    u32le(v, c(id));
    u32le(v, reply);
    pstr(v, b"");
    align(v);
    u16le(v, 0);
    u32le(v, direct);
    pstr(v, b"");
    align(v);
    u16le(v, 0);
    u16le(v, params.len() as u16);
    for &(n, k, t) in params {
        pstr(v, n);
        align(v);
        u32le(v, k);
        u32le(v, t);
        pstr(v, b"");
        align(v);
        u16le(v, 0);
    }
}

/// ODSHELL's terminology: settext, gettext, play; the core suite's get
/// and set (to); the class "part" (by index: 1 the root part, then the
/// embedded ones) with the properties "text" and "name" (its kind).
fn aete() -> Vec<u8> {
    let mut v = Vec::new();
    for x in [1u16, 0, 0, 1] {
        u16le(&mut v, x);
    }
    pstr(&mut v, b"ODSHELL suite");
    pstr(&mut v, b"");
    align(&mut v);
    u32le(&mut v, c(b"RS2S"));
    u16le(&mut v, 1);
    u16le(&mut v, 1);
    u16le(&mut v, 5);
    event(&mut v, b"settext", b"RS2S", b"stxt", c(b"null"), c(b"TEXT"), &[]);
    event(&mut v, b"gettext", b"RS2S", b"gtxt", c(b"TEXT"), c(b"null"), &[]);
    event(&mut v, b"play", b"RS2S", b"play", c(b"null"), c(b"null"), &[]);
    event(&mut v, b"get", b"core", b"getd", c(b"****"), c(b"obj "), &[]);
    event(&mut v, b"set", b"core", b"setd", c(b"null"), c(b"obj "), &[(b"to", c(b"data"), c(b"****"))]);
    u16le(&mut v, 1);
    pstr(&mut v, b"part");
    align(&mut v);
    u32le(&mut v, c(b"cpar"));
    pstr(&mut v, b"");
    align(&mut v);
    u16le(&mut v, 2);
    for (n, code) in [(&b"text"[..], c(b"ctxt")), (&b"name"[..], c(b"pnam"))] {
        pstr(&mut v, n);
        align(&mut v);
        u32le(&mut v, code);
        u32le(&mut v, c(b"TEXT"));
        pstr(&mut v, b"");
        align(&mut v);
        u16le(&mut v, 0);
    }
    u16le(&mut v, 0);
    u16le(&mut v, 0);
    u16le(&mut v, 0);
    v
}

fn reply_text(reply: *mut Desc, s: &[u8]) {
    let mut d = Desc::default();
    unsafe {
        AECreateDesc(c(b"TEXT"), s.as_ptr(), s.len(), &mut d);
        AEPutParamDesc(reply, c(b"----"), &d);
        AEDisposeDesc(&mut d);
    }
}

extern "C" fn get_aete(_e: *const Desc, reply: *mut Desc, _r: usize) -> i32 {
    let a = aete();
    let mut d = Desc::default();
    unsafe {
        AECreateDesc(c(b"aete"), a.as_ptr(), a.len(), &mut d);
        AEPutParamDesc(reply, c(b"----"), &d);
        AEDisposeDesc(&mut d);
    }
    0
}

/// The root part (RS2TextPart).
fn root_part() -> usize {
    let e = ev();
    unsafe {
        if WINDOW == 0 {
            return 0;
        }
        let rf = m(cd!(ODWindowClassData), 1, &[WINDOW, e]);
        let p = m(cd!(ODFrameClassData), 18, &[rf, e]);
        release(rf);
        p
    }
}

extern "C" fn set_text(event: *const Desc, _reply: *mut Desc, _r: usize) -> i32 {
    let mut buf = [0u8; 256];
    let (mut ty, mut n) = (0u32, 0u32);
    let r = unsafe { AEGetParamPtr(event, c(b"----"), c(b"TEXT"), &mut ty, buf.as_mut_ptr(), buf.len(), &mut n) };
    if r != 0 {
        return r;
    }
    let p = root_part();
    if p != 0 {
        let t: Vec<u8> = buf[..(n as usize).min(256)].iter().copied().take_while(|&b| b != 0).collect();
        tp(p).text = t;
        release(p);
        unsafe { WinInvalidateRect(FRAME_HWND, 0, 1) };
    }
    0
}

extern "C" fn get_text(_e: *const Desc, reply: *mut Desc, _r: usize) -> i32 {
    let p = root_part();
    if p == 0 {
        return -1708;
    }
    let t = tp(p).text.clone();
    release(p);
    reply_text(reply, &t);
    0
}

/// The embedded facets of the root facet with their frames and parts:
/// the first whose part's class is `class`.
fn embedded(class: &[u8]) -> Option<(usize, usize, usize)> {
    let e = ev();
    unsafe {
        let rfc = m(cd!(ODWindowClassData), 2, &[WINDOW, e]);
        let it = m(cd!(ODFacetClassData), 6, &[rfc, e, 0, 0]);
        let mut f = m((&raw const ODFacetIteratorClassData) as *const usize, 0, &[it, e]);
        let mut found = None;
        while f != 0 && found.is_none() {
            if f != rfc {
                let fr = m(cd!(ODFacetClassData), 0, &[f, e]);
                let p = m(cd!(ODFrameClassData), 18, &[fr, e]);
                let name = somsup::cstr(somsup::class_name(somsup::class_of(p)));
                if name == class {
                    found = Some((p, fr, f));
                } else {
                    release(p);
                }
            }
            f = m((&raw const ODFacetIteratorClassData) as *const usize, 1, &[it, e]);
        }
        somsup::free_object(it);
        found
    }
}

extern "C" fn play(_e: *const Desc, _reply: *mut Desc, _r: usize) -> i32 {
    let Some((part, _frame, facet)) = embedded(b"MMAudioPart") else { return -1728 };
    release(part);
    // IBM's part plays when it is clicked (its Play command needs the
    // play palette an activated frame has): a click inside its facet
    let e = ev();
    let t = m(cd!(ODFacetClassData), 18, &[facet, e, 0]); // AcquireWindowFrameTransform
    let mut pt = [8i32 << 16, 8i32 << 16];
    m(cd!(ODBaseTransformClassData), 18, &[t, e, pt.as_mut_ptr() as usize]); // TransformPoint
    release(t);
    let (x, y) = ((pt[0] >> 16) as u16 as usize, (pt[1] >> 16) as u16 as usize);
    unsafe {
        let client = WinWindowFromID(FRAME_HWND, 0x8008);
        WinPostMsg(client, 0x71, x | y << 16, 0); // WM_BUTTON1DOWN
        WinPostMsg(client, 0x72, x | y << 16, 0); // WM_BUTTON1UP
    }
    0
}

/// The document's parts by index: 1 the root part, then the embedded
/// parts (their facets in order).
fn parts() -> Vec<usize> {
    let e = ev();
    let mut v = Vec::new();
    let r = root_part();
    if r == 0 {
        return v;
    }
    v.push(r);
    unsafe {
        let rfc = m(cd!(ODWindowClassData), 2, &[WINDOW, e]);
        let it = m(cd!(ODFacetClassData), 6, &[rfc, e, 0, 0]);
        let mut f = m((&raw const ODFacetIteratorClassData) as *const usize, 0, &[it, e]);
        while f != 0 {
            if f != rfc {
                let fr = m(cd!(ODFacetClassData), 0, &[f, e]);
                v.push(m(cd!(ODFrameClassData), 18, &[fr, e]));
            }
            f = m((&raw const ODFacetIteratorClassData) as *const usize, 1, &[it, e]);
        }
        somsup::free_object(it);
    }
    v
}

fn long_of(d: *const Desc) -> i32 {
    let mut b = [0u8; 4];
    let (mut ty, mut n) = (0u32, 0u32);
    unsafe { AEGetDescData(d, &mut ty, b.as_mut_ptr(), 4, &mut n) };
    i32::from_le_bytes(b)
}

/// part (by absolute position) in the application: a 'cpar' token with
/// the index.
extern "C" fn part_accessor(_class: u32, _ctok: *const Desc, _cclass: u32, form: u32, key: *const Desc, tok: *mut Desc, _refcon: usize) -> i32 {
    if form != c(b"indx") {
        return -1708;
    }
    let i = long_of(key);
    let ps = parts();
    let n = ps.len() as i32;
    for p in ps {
        release(p);
    }
    if i < 1 || i > n {
        return -1728; // errAENoSuchObject
    }
    unsafe { AECreateDesc(c(b"cpar"), &i as *const i32 as *const u8, 4, tok) }
}

/// A property of a part: a 'prop' token (index, property).
extern "C" fn prop_accessor(_class: u32, ctok: *const Desc, _cclass: u32, form: u32, key: *const Desc, tok: *mut Desc, _refcon: usize) -> i32 {
    if form != c(b"prop") {
        return -1708;
    }
    let v = [long_of(ctok), long_of(key)];
    unsafe { AECreateDesc(c(b"prop"), v.as_ptr() as *const u8, 8, tok) }
}

/// The direct parameter resolved: (part, property).
fn direct_object(event: *const Desc) -> Result<(usize, u32), i32> {
    let mut spec = Desc::default();
    let mut tok = Desc::default();
    unsafe {
        let r = AEGetParamDesc(event, c(b"----"), c(b"****"), &mut spec);
        if r != 0 {
            return Err(r);
        }
        let r = AEResolve(&spec, 0, &mut tok);
        AEDisposeDesc(&mut spec);
        if r != 0 {
            return Err(r);
        }
    }
    let mut b = [0u8; 8];
    let (mut ty, mut n) = (0u32, 0u32);
    unsafe {
        AEGetDescData(&tok, &mut ty, b.as_mut_ptr(), 8, &mut n);
        AEDisposeDesc(&mut tok);
    }
    let i = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let prop = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    let ps = parts();
    let mut part = 0;
    for (k, p) in ps.into_iter().enumerate() {
        if k as i32 + 1 == i {
            part = p;
        } else {
            release(p);
        }
    }
    if part == 0 { Err(-1728) } else { Ok((part, prop)) }
}

fn is_text_part(p: usize) -> bool {
    somsup::is_a(p, unsafe { RS2TextPartClassData[0] })
}

extern "C" fn get_data(event: *const Desc, reply: *mut Desc, _r: usize) -> i32 {
    let (p, prop) = match direct_object(event) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let v = if prop == c(b"ctxt") && is_text_part(p) {
        tp(p).text.clone()
    } else if prop == c(b"pnam") {
        unsafe { somsup::cstr(somsup::class_name(somsup::class_of(p))) }.to_vec()
    } else {
        release(p);
        return -1728;
    };
    release(p);
    reply_text(reply, &v);
    0
}

extern "C" fn set_data(event: *const Desc, _reply: *mut Desc, _r: usize) -> i32 {
    let (p, prop) = match direct_object(event) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let mut buf = [0u8; 256];
    let (mut ty, mut n) = (0u32, 0u32);
    let r = unsafe { AEGetParamPtr(event, c(b"data"), c(b"TEXT"), &mut ty, buf.as_mut_ptr(), buf.len(), &mut n) };
    let ok = r == 0 && prop == c(b"ctxt") && is_text_part(p);
    if ok {
        tp(p).text = buf[..(n as usize).min(256)].iter().copied().take_while(|&b| b != 0).collect();
        unsafe { WinInvalidateRect(FRAME_HWND, 0, 1) };
    }
    release(p);
    if r != 0 { r } else if ok { 0 } else { -10003 } // errAEWriteDenied
}

/// ODSHELL answers Apple events (from its own scripts and from other
/// processes): its terminology, its events and object accessors.
pub fn install() {
    unsafe {
        AEInstallObjectAccessor(c(b"cpar"), c(b"null"), part_accessor as usize, 0, 0);
        AEInstallObjectAccessor(c(b"prop"), c(b"cpar"), prop_accessor as usize, 0, 0);
        AEInstallEventHandler(c(b"ascr"), c(b"gdte"), get_aete as usize, 0, 0);
        AEInstallEventHandler(c(b"RS2S"), c(b"stxt"), set_text as usize, 0, 0);
        AEInstallEventHandler(c(b"RS2S"), c(b"gtxt"), get_text as usize, 0, 0);
        AEInstallEventHandler(c(b"RS2S"), c(b"play"), play as usize, 0, 0);
        AEInstallEventHandler(c(b"core"), c(b"getd"), get_data as usize, 0, 0);
        AEInstallEventHandler(c(b"core"), c(b"setd"), set_data as usize, 0, 0);
    }
}

static mut COMPONENT: usize = 0;

/// The scripting component, loaded the first time.
fn component() -> usize {
    unsafe {
        if COMPONENT != 0 {
            return COMPONENT;
        }
        DosCopy(b"C:\\OS2\\DLL\\OREXX.DLL\0".as_ptr(), b"C:\\ORX\\REXX.DLL\0".as_ptr(), 1);
        DosSetExtLIBPATH(b"C:\\ORX\0".as_ptr(), 1);
        OSAScriptingComponentNewClass(1, 0);
        let mgr = somsup::SOMClassMgrObject;
        let cls = somsup::call(somsup::scm(), somsup::mm::SOM_FIND_CLS_IN_FILE, &[mgr, somsup::id(b"REXXScriptingComponent") as usize, 0, 0, b"OREXXSC\0".as_ptr() as usize]);
        if cls != 0 {
            COMPONENT = somsup::new_object(cls);
        }
        COMPONENT
    }
}

fn read_script() -> Option<Vec<u8>> {
    let (mut hf, mut action) = (0u32, 0u32);
    if unsafe { DosOpen(b"C:\\ODSCRIPT.CMD\0".as_ptr(), &mut hf, &mut action, 0, 0, 1, 0x40, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut v = alloc::vec![0u8; 16384];
    let mut n = 0u32;
    unsafe {
        DosRead(hf, v.as_mut_ptr(), v.len() as u32, &mut n);
        DosClose(hf);
    }
    v.truncate(n as usize);
    Some(v)
}

// ---- recording ----

static mut RECORDING: Option<usize> = None;
static mut RECORDED: Vec<u8> = Vec::new();

/// 'ascr'/'recd': a line the scripting component recorded.
extern "C" fn recorded(event: *const Desc, _reply: *mut Desc, _r: usize) -> i32 {
    let mut buf = [0u8; 1024];
    let (mut ty, mut n) = (0u32, 0u32);
    let r = unsafe { AEGetParamPtr(event, c(b"----"), c(b"TEXT"), &mut ty, buf.as_mut_ptr(), buf.len(), &mut n) };
    if r == 0 {
        unsafe { (*(&raw mut RECORDED)).extend_from_slice(&buf[..(n as usize).min(1024)]) };
    }
    r
}

/// Document/Play sound: the 'play' event ODSHELL sends itself (so a
/// recording has it).
pub fn play_sound() {
    let mut target = Desc::default();
    let mut e = Desc::default();
    let mut reply = Desc::default();
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { DosGetInfoBlocks(&mut tib, &mut pib) };
    let pid = unsafe { (*pib).pid }; // this process
    unsafe {
        AECreateDesc(c(b"PID "), &pid as *const u32 as *const u8, 4, &mut target);
        AECreateAppleEvent(c(b"RS2S"), c(b"play"), &target, -1, 0, &mut e);
        AESend(&e, &mut reply, 0x23, 0, -1, 0, 0);
        AEDisposeDesc(&mut e);
        AEDisposeDesc(&mut reply);
        AEDisposeDesc(&mut target);
    }
}

/// Document/Record script: OSAStartRecording; again: OSAStopRecording,
/// the recorded REXX goes to C:\RECORDED.CMD and the status line.
pub fn toggle_recording(frame: usize) {
    let e = ev();
    let sc = component();
    if sc == 0 {
        status_hint(e, frame, b"No scripting component (OREXXSC)");
        return;
    }
    let osa = unsafe { (&raw const OSAScriptingComponentClassData) as *const usize };
    unsafe {
        match RECORDING.take() {
            None => {
                AEInstallEventHandler(c(b"ascr"), c(b"recd"), recorded as usize, 0, 0);
                (*(&raw mut RECORDED)).clear();
                let mut id = 0usize;
                let r = somsup::call(osa, 36, &[sc, e, &mut id as *mut usize as usize]) as i32;
                if r == 0 {
                    RECORDING = Some(id);
                    status_hint(e, frame, b"Recording...");
                } else {
                    status_hint(e, frame, alloc::format!("Recording failed: {r}").as_bytes());
                }
            }
            Some(id) => {
                somsup::call(osa, 37, &[sc, e, id]);
                let text = (*(&raw const RECORDED)).clone();
                let mut hf = 0u32;
                let mut action = 0u32;
                if DosOpen(b"C:\\RECORDED.CMD\0".as_ptr(), &mut hf, &mut action, 0, 0, 0x12, 0x41, core::ptr::null_mut()) == 0 {
                    let mut n = 0u32;
                    let head = b"/* recorded by ODSHELL (OSA recording) */\r\n";
                    DosWrite(hf, head.as_ptr(), head.len() as u32, &mut n);
                    let t: Vec<u8> = text.iter().flat_map(|&b| if b == b'\n' { alloc::vec![b'\r', b'\n'] } else { alloc::vec![b] }).collect();
                    DosWrite(hf, t.as_ptr(), t.len() as u32, &mut n);
                    DosClose(hf);
                }
                let mut m = b"Recorded (C:\\RECORDED.CMD): ".to_vec();
                // the lines after the connection line
                let body = text.iter().position(|&b| b == b'\n').map_or(&text[..], |i| &text[i + 1..]);
                m.extend(body.iter().map(|&b| if b == b'\n' || b == b'\r' { b';' } else { b }));
                status_hint(e, frame, &m);
            }
        }
    }
}

/// Document/Run script: C:\ODSCRIPT.CMD's result on the status line.
pub fn run_script(frame: usize) {
    let e = ev();
    let sc = component();
    let msg: Vec<u8> = match (sc, read_script()) {
        (0, _) => b"No scripting component (OREXXSC)".to_vec(),
        (_, None) => b"No script (C:\\ODSCRIPT.CMD)".to_vec(),
        (_, Some(src)) => {
            let osa = unsafe { (&raw const OSAScriptingComponentClassData) as *const usize };
            let mut s = Desc::default();
            let mut out = Desc::default();
            unsafe { AECreateDesc(c(b"TEXT"), src.as_ptr(), src.len(), &mut s) };
            // OSADoScript(source, context, desiredType, modeFlags, *text)
            let r = somsup::call(osa, 13, &[sc, e, &mut s as *mut Desc as usize, 0, c(b"TEXT") as usize, 0, &mut out as *mut Desc as usize]) as i32;
            let mut text = alloc::vec![0u8; 256];
            let (mut ty, mut n) = (0u32, 0u32);
            unsafe {
                AEGetDescData(&out, &mut ty, text.as_mut_ptr(), text.len(), &mut n);
                AEDisposeDesc(&mut s);
                AEDisposeDesc(&mut out);
            }
            text.truncate(n as usize);
            if r == 0 {
                let mut m = b"Script result: ".to_vec();
                m.extend_from_slice(&text);
                m
            } else {
                alloc::format!("Script error {r}").into_bytes()
            }
        }
    };
    status_hint(e, frame, &msg);
}
