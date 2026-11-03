//! Scripting ODSHELL's document from REXX: Document/Run script runs
//! `C:\ODSCRIPT.CMD` through the OpenDoc scripting component (IBM's
//! OREXXSC.DLL, REXXScriptingComponent, loaded on first use from
//! `C:\ORX` with rs2's Object REXX as its REXX.DLL), and ODSHELL answers
//! the Apple events a script sends it (`.ScriptableApp~new~connect('ODSHELL')`):
//!
//! - 'ascr'/'gdte': its terminology ('aete'): "settext" (the root
//!   part's text, a direct TEXT parameter), "gettext" (replies TEXT),
//!   "play" (the embedded IBM MMAudioPart plays: a click in its facet,
//!   as its Play command needs the play palette its activated frame has).
//!
//! The script's result goes to the status line.

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

fn event(v: &mut Vec<u8>, name: &[u8], id: &[u8; 4], reply: u32, direct: u32) {
    pstr(v, name);
    pstr(v, b"");
    align(v);
    v.extend_from_slice(&c(b"RS2S").to_le_bytes());
    v.extend_from_slice(&c(id).to_le_bytes());
    v.extend_from_slice(&reply.to_le_bytes());
    pstr(v, b"");
    align(v);
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&direct.to_le_bytes());
    pstr(v, b"");
    align(v);
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
}

fn aete() -> Vec<u8> {
    let mut v = Vec::new();
    for x in [1u16, 0, 0, 1] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    pstr(&mut v, b"ODSHELL suite");
    pstr(&mut v, b"");
    align(&mut v);
    v.extend_from_slice(&c(b"RS2S").to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes());
    event(&mut v, b"settext", b"stxt", c(b"null"), c(b"TEXT"));
    event(&mut v, b"gettext", b"gtxt", c(b"TEXT"), c(b"null"));
    event(&mut v, b"play", b"play", c(b"null"), c(b"null"));
    for _ in 0..4 {
        v.extend_from_slice(&0u16.to_le_bytes());
    }
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
            AEInstallEventHandler(c(b"ascr"), c(b"gdte"), get_aete as usize, 0, 0);
            AEInstallEventHandler(c(b"RS2S"), c(b"stxt"), set_text as usize, 0, 0);
            AEInstallEventHandler(c(b"RS2S"), c(b"gtxt"), get_text as usize, 0, 0);
            AEInstallEventHandler(c(b"RS2S"), c(b"play"), play as usize, 0, 0);
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
