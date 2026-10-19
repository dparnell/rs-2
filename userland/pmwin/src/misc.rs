//! Initialisation, errors, system values and colours, string resources and
//! message boxes.

#![allow(non_snake_case)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use gpi::Surface;

use crate::color;
use crate::res;
use crate::state::{self, st};
use crate::win;

#[repr(C)]
#[derive(Default)]
struct DisplayInfo {
    address: u32,
    width: u32,
    height: u32,
    stride: u32,
    flags: u32,
}

unsafe extern "C" {
    #[link_name = "RS2API.1"]
    fn Rs2MapDisplay(info: *mut DisplayInfo) -> u32;
    #[link_name = "DOSCALLS.329"]
    fn DosWaitEventSem(hev: u32, ms: u32) -> u32;
    #[link_name = "DOSCALLS.334"]
    fn DosRequestMutexSem(hmtx: u32, ms: u32) -> u32;
    #[link_name = "DOSCALLS.286"]
    fn DosBeep(freq: u32, ms: u32) -> u32;
}

const HAB: u32 = 1;

/// This process has mapped the display.
static MAPPED: AtomicBool = AtomicBool::new(false);
/// WinGetLastError's value (per process).
pub static LAST_ERROR: AtomicU32 = AtomicU32::new(0);

/// WinInitialize(flOptions): the anchor block. The first call in a
/// process maps the display (a PM program joins the Presentation Manager
/// session); the first one creates the shared state.
#[unsafe(no_mangle)]
pub extern "C" fn WinInitialize(_fl: u32) -> u32 {
    if !MAPPED.load(Ordering::Relaxed) {
        let mut info = DisplayInfo::default();
        if unsafe { Rs2MapDisplay(&mut info) } != 0 {
            return 0;
        }
        // Built before anything shared points at it.
        gpi::font::system();
        MAPPED.store(true, Ordering::Relaxed);
        if state::get().is_none() {
            let surface = unsafe {
                Surface::new(info.address as *mut u32, info.width as i32, info.height as i32, info.stride as i32, info.flags & 1 != 0)
            };
            state::init(surface);
            win::register_builtin();
        }
    }
    HAB
}

#[unsafe(no_mangle)]
pub extern "C" fn WinTerminate(_hab: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryAnchorBlock(_h: u32) -> u32 {
    HAB
}

#[unsafe(no_mangle)]
pub extern "C" fn WinGetLastError(_hab: u32) -> u32 {
    LAST_ERROR.swap(0, Ordering::Relaxed)
}

/// No extended error information is kept.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetErrorInfo(_hab: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn WinFreeErrorInfo(_p: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinAlarm(_desk: u32, _style: u32) -> u32 {
    unsafe { DosBeep(880, 80) };
    1
}

/// WinQuerySysValue(hwndDesktop, iSysValue)
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysValue(_desk: u32, i: i32) -> i32 {
    let s = st();
    match i {
        20 => s.surface.width,  // SV_CXSCREEN
        21 => s.surface.height, // SV_CYSCREEN
        4 | 5 => 4,             // SV_CX/CYSIZEBORDER
        22 | 23 => 16,          // SV_CXVSCROLL / SV_CYHSCROLL
        26 | 27 => 1,           // SV_CX/CYBORDER
        28 | 29 => 3,           // SV_CX/CYDLGFRAME
        30 => win::TITLE_H,     // SV_CYTITLEBAR
        32 => 16,               // SV_CYMENU
        38 | 39 => 32,          // SV_CX/CYICON
        40 | 41 => 32,          // SV_CX/CYPOINTER
        43 => 3,                // SV_CMOUSEBUTTONS
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysColor(_desk: u32, clr: i32, _reserved: i32) -> u32 {
    color::sys(clr)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysPointer(_desk: u32, _id: i32, _copy: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetPointer(_desk: u32, _hptr: u32) -> u32 {
    1
}

/// CLASSINFO: flClassStyle, pfnWindowProc, cbWindowData.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryClassInfo(_hab: u32, name: *const u8, info: *mut u32) -> u32 {
    if name.is_null() || info.is_null() {
        return 0;
    }
    let v = name as u32;
    let n: Vec<u8> = if v >> 16 == 0xffff {
        alloc::format!("#{}", v & 0xffff).into_bytes()
    } else {
        (0..).map(|i| unsafe { *name.add(i) }).take_while(|&b| b != 0).collect()
    };
    let Some(c) = st().class(&n) else { return 0 };
    unsafe {
        info.write_unaligned(c.style);
        info.add(1).write_unaligned(c.proc_);
        info.add(2).write_unaligned(c.words);
    }
    1
}

/// WinSubclassWindow(hwnd, pfnwp): returns the previous procedure.
#[unsafe(no_mangle)]
pub extern "C" fn WinSubclassWindow(h: u32, proc_: u32) -> u32 {
    let Some(w) = st().desk.get_mut(h) else { return 0 };
    core::mem::replace(&mut w.proc_, proc_)
}

/// WinLoadString(hab, hmod, id, cchMax, pchBuffer)
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadString(_hab: u32, hmod: u32, id: u32, cch: i32, buf: *mut u8) -> i32 {
    load_string(res::RT_STRING, hmod, id, cch, buf)
}

/// WinLoadMessage(hab, hmod, id, cchMax, pchBuffer): message tables have
/// the string table layout.
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadMessage(_hab: u32, hmod: u32, id: u32, cch: i32, buf: *mut u8) -> i32 {
    load_string(res::RT_MESSAGE, hmod, id, cch, buf)
}

fn load_string(ty: u32, hmod: u32, id: u32, cch: i32, buf: *mut u8) -> i32 {
    if buf.is_null() || cch <= 0 {
        return 0;
    }
    let text = res::string(hmod, ty, id).unwrap_or_default();
    crate::ctl::copy_out(&text, cch as usize, buf) as i32
}

/// WinWaitEventSem(hev, ulTimeout): waits without holding PMWIN.
#[unsafe(no_mangle)]
pub extern "C" fn WinWaitEventSem(hev: u32, ms: u32) -> u32 {
    crate::lock::unlocked(|| unsafe { DosWaitEventSem(hev, ms) })
}

/// WinRequestMutexSem(hmtx, ulTimeout)
#[unsafe(no_mangle)]
pub extern "C" fn WinRequestMutexSem(hmtx: u32, ms: u32) -> u32 {
    crate::lock::unlocked(|| unsafe { DosRequestMutexSem(hmtx, ms) })
}

/// Presentation parameters are accepted and ignored (controls use the
/// system colours and font).
#[unsafe(no_mangle)]
pub extern "C" fn WinSetPresParam(h: u32, _id: u32, _cb: u32, _pv: u32) -> u32 {
    st().desk.exists(h) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryPresParam(_h: u32, _id1: u32, _id2: u32, _pid: u32, _cb: u32, _pv: u32, _fs: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn WinRemovePresParam(h: u32, _id: u32) -> u32 {
    st().desk.exists(h) as u32
}

fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

/// WinMessageBox(hwndParent, hwndOwner, pszText, pszCaption, idWindow,
/// flStyle): a modal dialog with the text and the buttons of the style
/// (MB_OK .. MB_ENTERCANCEL); returns the MBID_* of the button pressed.
#[unsafe(no_mangle)]
pub extern "C" fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, _id: u32, style: u32) -> u32 {
    const MBID_OK: u32 = 1;
    const MBID_CANCEL: u32 = 2;
    const MBID_ABORT: u32 = 3;
    const MBID_RETRY: u32 = 4;
    const MBID_IGNORE: u32 = 5;
    const MBID_YES: u32 = 6;
    const MBID_NO: u32 = 7;
    const MBID_ENTER: u32 = 9;
    let buttons: &[(u32, &[u8])] = match style & 0xf {
        1 => &[(MBID_OK, b"OK"), (MBID_CANCEL, b"Cancel")],
        2 => &[(MBID_RETRY, b"~Retry"), (MBID_CANCEL, b"Cancel")],
        3 => &[(MBID_ABORT, b"~Abort"), (MBID_RETRY, b"~Retry"), (MBID_IGNORE, b"~Ignore")],
        4 => &[(MBID_YES, b"~Yes"), (MBID_NO, b"~No")],
        5 => &[(MBID_YES, b"~Yes"), (MBID_NO, b"~No"), (MBID_CANCEL, b"Cancel")],
        6 => &[(MBID_CANCEL, b"Cancel")],
        7 => &[(MBID_ENTER, b"Enter")],
        8 => &[(MBID_ENTER, b"Enter"), (MBID_CANCEL, b"Cancel")],
        _ => &[(MBID_OK, b"OK")],
    };
    let font = gpi::font::system();
    let text = cstr(text);
    let title = cstr(title);
    let lines: Vec<Vec<u8>> = text.split(|&b| b == b'\n').map(|l| l.iter().copied().filter(|&b| b != b'\r').collect()).collect();
    let bw = 80;
    let tw = lines.iter().map(|l| font.text_width(l)).max().unwrap_or(0).max(font.text_width(&title) + 40).max(buttons.len() as i32 * (bw + 10)).max(160);
    // Template coordinates are dialog units (2x2 pixels).
    let (w, h) = (tw + 40, lines.len() as i32 * font.height + 60);
    let mut items = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let y = h - 10 - (i as i32 + 1) * font.height;
        // SS_TEXT | DT_LEFT, visible.
        items.push(res::DlgItemT { class: b"#5".to_vec(), text: l.clone(), style: 0x8000_0001, x: 5, y: y / 2, cx: (w - 20) / 2, cy: font.height / 2, id: 0xffff, ctl: core::ptr::null(), children: Vec::new() });
    }
    let total = buttons.len() as i32 * (bw + 10) - 10;
    for (i, (id, label)) in buttons.iter().enumerate() {
        // BS_PUSHBUTTON (the first one BS_DEFAULT), WS_TABSTOP, the first
        // one WS_GROUP, visible.
        let bs = 0x8000_0000 | 0x0002_0000 | if i == 0 { 0x0001_0400 } else { 0 };
        let x = (w - total) / 2 + i as i32 * (bw + 10);
        items.push(res::DlgItemT { class: b"#3".to_vec(), text: label.to_vec(), style: bs, x: x / 2, y: 4, cx: bw / 2, cy: 12, id: *id, ctl: core::ptr::null(), children: Vec::new() });
    }
    // FCF_TITLEBAR (control data); FS_DLGBORDER | FS_SCREENALIGN.
    let fcf: u32 = 0x0001;
    let (sw, sh) = (st().surface.width, st().surface.height);
    let t = res::DlgItemT {
        class: b"#1".to_vec(),
        text: title,
        style: 0x0080 | 0x0200,
        x: (sw - w) / 4,
        y: (sh - h) / 4,
        cx: w / 2,
        cy: h / 2,
        id: 0,
        ctl: &fcf as *const u32 as *const u8,
        children: items,
    };
    let parent = if parent == 0 { pmwin::HWND_DESKTOP } else { parent };
    let dlg = crate::dlg::create_from(parent, owner, 0, &t, 0);
    if dlg == 0 {
        return 0xffff;
    }
    let r = crate::dlg::WinProcessDlg(dlg);
    win::WinDestroyWindow(dlg);
    r
}
