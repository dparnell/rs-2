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
        crate::fonts::init();
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

/// Record an error for WinGetLastError.
pub fn set_error(e: u32) {
    LAST_ERROR.store(e, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn WinGetLastError(_hab: u32) -> u32 {
    LAST_ERROR.swap(0, Ordering::Relaxed)
}

/// WinSetErrorInfo(errorid, fsOptions, ...): sets the error
/// WinGetLastError returns (the inserts of SEI_ARGCOUNT and friends are
/// not kept: rs2 keeps no error information block).
#[unsafe(no_mangle)]
pub extern "C" fn WinSetErrorInfo(e: u32, _fs: u32) -> u32 {
    set_error(e);
    e
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
pub extern "C" fn WinAlarm(_desk: u32, style: u32) -> u32 {
    // SV_ALARM off: no sound.
    if WinQuerySysValue(1, 6) == 0 {
        return 1;
    }
    // WA_WARNING, WA_NOTE, WA_ERROR: SV_WARNINGFREQ.. and their durations.
    let (f, d) = match style {
        0..=2 => (WinQuerySysValue(1, 13 + style as i32), WinQuerySysValue(1, 16 + style as i32)),
        _ => (880, 80),
    };
    unsafe { DosBeep(f as u32, d as u32) };
    1
}

/// WinSetSysValue(hwndDesktop, iSysValue, lValue): the values a user
/// sets (the mouse, keyboard and sound objects): double-click time,
/// swapped buttons, the alarm, the cursor rate, scrolling, beeps.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetSysValue(_desk: u32, i: i32, v: i32) -> u32 {
    // SV_SWAPBUTTON, SV_DBLCLKTIME, SV_CX/CYDBLCLK, SV_ALARM,
    // SV_CURSORRATE, SV_FIRST/SCROLLRATE, SV_*FREQ, SV_*DURATION.
    if !matches!(i, 0..=3 | 6 | 9..=11 | 13..=18) {
        set_error(0x1303); // PMERR_INVALID_PARM
        return 0;
    }
    st().sysvals.insert(i, v);
    1
}

/// WinSetSysColors(hwndDesktop, flOptions, flFormat, clrFirst, cclr,
/// pclr): LCOLF_CONSECRGB (the colours of clrFirst on) or LCOLF_INDRGB
/// (index, colour pairs); LCOL_RESET first restores the defaults. Every
/// window is redrawn.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetSysColors(_desk: u32, options: u32, format: u32, first: i32, n: u32, p: *const u32) -> u32 {
    let s = st();
    if options & 0x0001 != 0 {
        s.syscolors.clear();
    }
    if !p.is_null() {
        let at = |k: u32| unsafe { p.add(k as usize).read_unaligned() };
        match format {
            // LCOLF_INDRGB
            1 => (0..n / 2).for_each(|k| _ = s.syscolors.insert(at(2 * k) as i32, at(2 * k + 1) & 0xff_ffff)),
            // LCOLF_CONSECRGB
            2 => (0..n).for_each(|k| _ = s.syscolors.insert(first + k as i32, at(k) & 0xff_ffff)),
            _ => {
                set_error(0x1303); // PMERR_INVALID_PARM
                return 0;
            }
        }
    }
    s.desk.invalidate(1, None, true); // HWND_DESKTOP
    1
}

/// WinQuerySysValue(hwndDesktop, iSysValue)
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysValue(_desk: u32, i: i32) -> i32 {
    let s = st();
    if let Some(&v) = s.sysvals.get(&i) {
        return v;
    }
    match i {
        6 => 1,                      // SV_ALARM
        13 => 1000, 14 => 2000, 15 => 440, // SV_WARNING/NOTE/ERRORFREQ
        16..=18 => 80,               // SV_*DURATION
        1 => 500,                    // SV_DBLCLKTIME
        2 | 3 => 4,                  // SV_CX/CYDBLCLK
        20 | 36 => s.surface.width,  // SV_CXSCREEN, SV_CXFULLSCREEN
        21 => s.surface.height,      // SV_CYSCREEN
        37 => s.surface.height - win::TITLE_H, // SV_CYFULLSCREEN
        4 | 5 => 4,                  // SV_CX/CYSIZEBORDER
        9 => 500,                    // SV_CURSORRATE
        10 => 300,                   // SV_FIRSTSCROLLRATE
        11 => 50,                    // SV_SCROLLRATE
        22 | 23 => 16,               // SV_CXVSCROLL / SV_CYHSCROLL
        24 | 25 => 16,               // SV_CYVSCROLLARROW / SV_CXHSCROLLARROW
        26 | 27 => 1,                // SV_CX/CYBORDER
        28 | 29 => 3,                // SV_CX/CYDLGFRAME
        30 => win::TITLE_H,          // SV_CYTITLEBAR
        31 | 32 => 16,               // SV_CYVSLIDER / SV_CXHSLIDER
        33 | 34 => 18,               // SV_CX/CYMINMAXBUTTON
        35 => 16,                    // SV_CYMENU
        38 | 39 => 32,               // SV_CX/CYICON
        40 | 41 => 32,               // SV_CX/CYPOINTER
        43 => 3,                     // SV_CMOUSEBUTTONS
        44 => -s.ptr_level.min(0),   // SV_POINTERLEVEL
        48 => 1,                     // SV_MOUSEPRESENT
        49 | 50 => 1,                // SV_CX/CYALIGN
        59 => 1,                     // SV_INSERTMODE
        66 => 1,                     // SV_ALTMNEMONIC
        64 | 65 => 0,                // SV_MENUROLLDOWN/UPDELAY
        68 => 75,                    // SV_CXICONTEXTWIDTH
        69 => 3,                     // SV_CICONTEXTLINES
        73 | 74 => 2,                // SV_CX/CYMOTIONSTART
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysColor(_desk: u32, clr: i32, _reserved: i32) -> u32 {
    color::sys(clr)
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
    crate::lock::wait(|| unsafe { DosWaitEventSem(hev, ms) })
}

/// WinRequestMutexSem(hmtx, ulTimeout)
#[unsafe(no_mangle)]
pub extern "C" fn WinRequestMutexSem(hmtx: u32, ms: u32) -> u32 {
    crate::lock::wait(|| unsafe { DosRequestMutexSem(hmtx, ms) })
}

const PP_FOREGROUNDCOLOR: u32 = 1;
const PP_BACKGROUNDCOLOR: u32 = 3;
const PP_FONTNAMESIZE: u32 = 15;
const WM_PRESPARAMCHANGED: u32 = 0x001e;

/// WinSetPresParam(hwnd, id, cbParam, pbParam): the foreground and
/// background colours (PP_FOREGROUNDCOLOR, PP_BACKGROUNDCOLOR, RGB) are
/// kept for the window (the container uses them), and the font
/// (PP_FONTNAMESIZE, "10.Helv": the window and its children draw in
/// it); other presentation parameters are accepted and ignored.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetPresParam(h: u32, id: u32, cb: u32, pv: u32) -> u32 {
    let s = st();
    if !s.desk.exists(h) {
        return 0;
    }
    if id == PP_FONTNAMESIZE && pv != 0 {
        let b = unsafe { core::slice::from_raw_parts(pv as *const u8, cb as usize) };
        let name = b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())].to_vec();
        if crate::fonts::by_name(&name).is_none() {
            return 0;
        }
        s.pres_font.insert(h, name);
        s.desk.invalidate(h, None, true);
        crate::msg::send(h, WM_PRESPARAMCHANGED, id, 0);
        return 1;
    }
    if (id == PP_FOREGROUNDCOLOR || id == PP_BACKGROUNDCOLOR) && cb >= 4 && pv != 0 {
        let v = unsafe { (pv as *const u32).read_unaligned() } & 0xff_ffff;
        let e = s.pres.entry(h).or_insert((crate::state::NO_COLOR, crate::state::NO_COLOR));
        if id == PP_FOREGROUNDCOLOR {
            e.0 = v;
        } else {
            e.1 = v;
        }
        s.desk.invalidate(h, None, true);
    }
    1
}

/// PP_FONTNAMESIZE of the window or the nearest parent that has one.
pub fn pres_font_name(h: u32) -> Option<Vec<u8>> {
    let s = st();
    let mut w = h;
    for _ in 0..64 {
        if let Some(n) = s.pres_font.get(&w) {
            return Some(n.clone());
        }
        w = s.desk.get(w)?.parent;
        if w == 0 {
            break;
        }
    }
    None
}

/// The colour of a presentation parameter set on the window (not
/// inherited).
pub fn pres_color(h: u32, background: bool) -> Option<u32> {
    let (fg, bg) = *st().pres.get(&h)?;
    let v = if background { bg } else { fg };
    (v != crate::state::NO_COLOR).then_some(v)
}

/// WinQueryPresParam(hwnd, id1, id2, pulId, cbBuf, pbBuf, fs): the
/// colours set with WinSetPresParam (the bytes copied, 0 if neither).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryPresParam(h: u32, id1: u32, id2: u32, pid: u32, cb: u32, pv: u32, fs: u32) -> u32 {
    for id in [id1, id2] {
        // The font, inherited from the parents unless QPF_NOINHERIT.
        if id == PP_FONTNAMESIZE {
            let name = if fs & 1 != 0 { st().pres_font.get(&h).cloned() } else { pres_font_name(h) };
            if let Some(n) = name {
                if pv == 0 || (cb as usize) < n.len() + 1 {
                    return 0;
                }
                unsafe {
                    core::ptr::copy_nonoverlapping(n.as_ptr(), pv as *mut u8, n.len());
                    *(pv as *mut u8).add(n.len()) = 0;
                    if pid != 0 {
                        (pid as *mut u32).write_unaligned(id);
                    }
                }
                return n.len() as u32 + 1;
            }
            continue;
        }
        let v = match id {
            PP_FOREGROUNDCOLOR => pres_color(h, false),
            PP_BACKGROUNDCOLOR => pres_color(h, true),
            _ => None,
        };
        if let Some(v) = v {
            if cb < 4 || pv == 0 {
                return 0;
            }
            unsafe {
                (pv as *mut u32).write_unaligned(v);
                if pid != 0 {
                    (pid as *mut u32).write_unaligned(id);
                }
            }
            return 4;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn WinRemovePresParam(h: u32, id: u32) -> u32 {
    let s = st();
    if !s.desk.exists(h) {
        return 0;
    }
    if id == PP_FONTNAMESIZE {
        if s.pres_font.remove(&h).is_some() {
            s.desk.invalidate(h, None, true);
            crate::msg::send(h, WM_PRESPARAMCHANGED, id, 0);
        }
        return 1;
    }
    if let Some(e) = s.pres.get_mut(&h) {
        match id {
            PP_FOREGROUNDCOLOR => e.0 = crate::state::NO_COLOR,
            PP_BACKGROUNDCOLOR => e.1 = crate::state::NO_COLOR,
            _ => return 1,
        }
        s.desk.invalidate(h, None, true);
    }
    1
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
/// The dialog has id idWindow; MB_HELP adds a Help button (MBID_HELP,
/// BS_HELP | BS_NOPOINTERFOCUS, as IBM's MboxInit), which, like F1, asks
/// the help hook for the help of the box (HLPM_WINDOW, idWindow).
#[unsafe(no_mangle)]
pub extern "C" fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32 {
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
    const MB_HELP: u32 = 0x2000;
    const MBID_HELP: u32 = 8;
    let mut buttons: Vec<(u32, &[u8])> = buttons.to_vec();
    if style & MB_HELP != 0 {
        buttons.push((MBID_HELP, b"Help"));
    }
    let font = gpi::font::system();
    let text = cstr(text);
    let title = cstr(title);
    let lines: Vec<Vec<u8>> = text.split(|&b| b == b'\n').map(|l| l.iter().copied().filter(|&b| b != b'\r').collect()).collect();
    let bw = 80;
    let tw = lines.iter().map(|l| font.text_width(l)).max().unwrap_or(0).max(font.text_width(&title) + 40).max(buttons.len() as i32 * (bw + 10)).max(160);
    // Template coordinates are dialog units (2x2 pixels).
    // MB_ICONQUESTION, MB_ICONEXCLAMATION, MB_ICONASTERISK, MB_ICONHAND:
    // a system icon left of the text.
    let icon = match style & 0x70 {
        0x10 => pmwin::sysptr::SPTR_ICONQUESTION,
        0x20 => pmwin::sysptr::SPTR_ICONWARNING,
        0x30 => pmwin::sysptr::SPTR_ICONINFORMATION,
        0x40 => pmwin::sysptr::SPTR_ICONERROR,
        _ => 0,
    };
    let ix = if icon != 0 { 48 } else { 0 };
    let (w, h) = (tw + 40 + ix, (lines.len() as i32 * font.height).max(if icon != 0 { 40 } else { 0 }) + 60);
    let mut items = Vec::new();
    if icon != 0 {
        // SS_SYSICON, visible.
        let t = alloc::format!("#{icon}").into_bytes();
        items.push(res::DlgItemT { class: b"#5".to_vec(), text: t, style: 0x8000_000b, x: 8, y: (h - 50) / 2, cx: 16, cy: 16, id: 0xffff, ctl: core::ptr::null(), children: Vec::new() });
    }
    for (i, l) in lines.iter().enumerate() {
        let y = h - 10 - (i as i32 + 1) * font.height;
        // SS_TEXT | DT_LEFT, visible.
        items.push(res::DlgItemT { class: b"#5".to_vec(), text: l.clone(), style: 0x8000_0001, x: (10 + ix) / 2, y: y / 2, cx: (w - 20 - ix) / 2, cy: font.height / 2, id: 0xffff, ctl: core::ptr::null(), children: Vec::new() });
    }
    let total = buttons.len() as i32 * (bw + 10) - 10;
    for (i, (id, label)) in buttons.iter().enumerate() {
        // BS_PUSHBUTTON (the first one BS_DEFAULT), WS_TABSTOP, the first
        // one WS_GROUP, visible.
        let mut bs = 0x8000_0000 | 0x0002_0000 | if i == 0 { 0x0001_0400 } else { 0 };
        if *id == MBID_HELP {
            // BS_HELP | BS_NOPOINTERFOCUS.
            bs |= 0x0100 | 0x0800;
        }
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
        id: id & 0xffff,
        ctl: &fcf as *const u32 as *const u8,
        children: items,
    };
    let parent = if parent == 0 { pmwin::HWND_DESKTOP } else { parent };
    let dlg = crate::dlg::create_from(parent, owner, 0, &t, 0);
    if dlg == 0 {
        return 0xffff;
    }
    let r = crate::dlg::process(dlg, crate::hooks::MSGF_MESSAGEBOX);
    win::WinDestroyWindow(dlg);
    r
}

// ---- Version, code pages and strings ----

/// WinQueryVersion: PM 20.45 (OS/2 Warp 4.5x): major version in the low
/// byte, minor in the next.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryVersion(_hab: u32) -> u32 {
    20 | 45 << 8
}

/// WinQueryCp(hmq): the queue's code page (HMQ_CURRENT: the caller's).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryCp(hmq: u32) -> u32 {
    let s = st();
    let q = if hmq == 1 || hmq == 0 { s.my_queue() } else { hmq };
    s.queues.get(&q).map_or(850, |x| x.cp)
}

/// WinSetCp(hmq, idCodePage): one of the system's code pages; the code
/// page changed hooks are told.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetCp(hmq: u32, cp: u32) -> u32 {
    if cp != 850 && cp != 437 {
        return 0;
    }
    let s = st();
    let q = if hmq == 1 || hmq == 0 { s.my_queue() } else { hmq };
    let Some(x) = s.queues.get_mut(&q) else { return 0 };
    let old = core::mem::replace(&mut x.cp, cp);
    if old != cp {
        crate::hooks::codepage_changed(q, old, cp);
    }
    1
}

fn cp_or_default(cp: u32) -> u16 {
    if cp == 0 { 850 } else { cp as u16 }
}

/// A character of code page `from` in code page `to` (through Unicode,
/// with rs2's NLS tables); '?' when `to` has no such character.
fn translate(from: u16, to: u16, c: u8) -> u8 {
    if from == to {
        return c;
    }
    uls::ucs_to_cp(to, uls::cp_to_ucs(from, c)).unwrap_or(b'?')
}

/// WinCpTranslateChar(hab, cpSrc, chSrc, cpDst) (0: the PM code page).
#[unsafe(no_mangle)]
pub extern "C" fn WinCpTranslateChar(_hab: u32, from: u32, c: u32, to: u32) -> u32 {
    translate(cp_or_default(from), cp_or_default(to), c as u8) as u32
}

/// WinCpTranslateString(hab, cpSrc, pszSrc, cpDst, cchDestMax, pchDest):
/// the string with its NUL, cut to fit.
#[unsafe(no_mangle)]
pub extern "C" fn WinCpTranslateString(_hab: u32, from: u32, src: *const u8, to: u32, max: u32, dst: *mut u8) -> u32 {
    if src.is_null() || dst.is_null() || max == 0 {
        return 0;
    }
    let (f, t) = (cp_or_default(from), cp_or_default(to));
    let mut i = 0usize;
    unsafe {
        while i + 1 < max as usize && *src.add(i) != 0 {
            *dst.add(i) = translate(f, t, *src.add(i));
            i += 1;
        }
        *dst.add(i) = 0;
    }
    1
}

/// WinQueryCpList(hab, ccpMax, prgcp): the code pages PM can use.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryCpList(_hab: u32, max: u32, list: *mut u32) -> u32 {
    let cps = [850u32, 437];
    if !list.is_null() {
        for (i, cp) in cps.iter().take(max as usize).enumerate() {
            unsafe { list.add(i).write_unaligned(*cp) };
        }
    }
    cps.len() as u32
}

/// Upper case in code page 850.
pub fn upper850(c: u8) -> u8 {
    const PAIRS: [(u8, u8); 31] = [
        (0x87, 0x80), (0x81, 0x9a), (0x82, 0x90), (0x83, 0xb6), (0x84, 0x8e), (0x85, 0xb7), (0x86, 0x8f), (0x88, 0xd2),
        (0x89, 0xd3), (0x8a, 0xd4), (0x8b, 0xd8), (0x8c, 0xd7), (0x8d, 0xde), (0x91, 0x92), (0x93, 0xe2), (0x94, 0x99),
        (0x95, 0xe3), (0x96, 0xea), (0x97, 0xeb), (0x9b, 0x9d), (0xa0, 0xb5), (0xa1, 0xd6), (0xa2, 0xe0), (0xa3, 0xe9),
        (0xa4, 0xa5), (0xc6, 0xc7), (0xd0, 0xd1), (0xe4, 0xe5), (0xe7, 0xe8), (0xec, 0xed), (0x9c, 0x9c),
    ];
    if c.is_ascii_lowercase() {
        return c.to_ascii_uppercase();
    }
    PAIRS.iter().find(|p| p.0 == c).map_or(c, |p| p.1)
}

/// WinUpper(hab, idcp, idcc, psz): upper-cases the string in place;
/// returns its length.
#[unsafe(no_mangle)]
pub extern "C" fn WinUpper(_hab: u32, _cp: u32, _cc: u32, psz: *mut u8) -> u32 {
    if psz.is_null() {
        return 0;
    }
    let mut n = 0;
    loop {
        let p = unsafe { psz.add(n) };
        let c = unsafe { *p };
        if c == 0 {
            return n as u32;
        }
        unsafe { *p = upper850(c) };
        n += 1;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinUpperChar(_hab: u32, _cp: u32, _cc: u32, c: u32) -> u32 {
    if c > 0xff { c } else { upper850(c as u8) as u32 }
}

/// WinNextChar(hab, idcp, idcc, psz): single-byte code pages only.
#[unsafe(no_mangle)]
pub extern "C" fn WinNextChar(_hab: u32, _cp: u32, _cc: u32, psz: *const u8) -> u32 {
    if psz.is_null() || unsafe { *psz } == 0 { psz as u32 } else { psz as u32 + 1 }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinPrevChar(_hab: u32, _cp: u32, _cc: u32, start: *const u8, psz: *const u8) -> u32 {
    if psz as u32 <= start as u32 { start as u32 } else { psz as u32 - 1 }
}

/// WinCompareStrings(hab, idcp, idcc, psz1, psz2, reserved): WCS_EQ 1,
/// WCS_LT 2, WCS_GT 3 (case-insensitive).
#[unsafe(no_mangle)]
pub extern "C" fn WinCompareStrings(_hab: u32, _cp: u32, _cc: u32, a: *const u8, b: *const u8, _r: u32) -> u32 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    let (a, b) = (cstr(a), cstr(b));
    let ka: Vec<u8> = a.iter().map(|&c| upper850(c)).collect();
    let kb: Vec<u8> = b.iter().map(|&c| upper850(c)).collect();
    match ka.cmp(&kb) {
        core::cmp::Ordering::Equal => 1,
        core::cmp::Ordering::Less => 2,
        core::cmp::Ordering::Greater => 3,
    }
}

// ---- Shutdown, window updates, owners ----

#[unsafe(no_mangle)]
pub extern "C" fn WinCancelShutdown(_hmq: u32, _always: u32) -> u32 {
    1
}

/// WinEnableWindowUpdate(hwnd, fEnable): drawing is not held back, so
/// turning updates on again only repaints the window.
#[unsafe(no_mangle)]
pub extern "C" fn WinEnableWindowUpdate(h: u32, on: u32) -> u32 {
    if !st().desk.exists(h) {
        return 0;
    }
    if on != 0 {
        st().desk.invalidate(h, None, true);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinLockWindowUpdate(_desk: u32, _h: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinIsWindowShowing(h: u32) -> u32 {
    st().desk.is_showing(h) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetOwner(h: u32, owner: u32) -> u32 {
    match st().desk.get_mut(h) {
        Some(w) => {
            w.owner = owner;
            1
        }
        None => 0,
    }
}

/// WinQueryUpdateRect(hwnd, prcl): the bounds of the update region;
/// FALSE when nothing needs painting.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryUpdateRect(h: u32, prcl: *mut crate::gpiapi::RectL) -> u32 {
    let Some(w) = st().desk.get(h) else { return 0 };
    let b = w.update.bounds();
    if !prcl.is_null() {
        unsafe { prcl.write_unaligned(crate::gpiapi::RectL::from(b)) };
    }
    (!w.update.is_empty()) as u32
}

/// WinInvertRect(hps, prcl)
#[unsafe(no_mangle)]
pub extern "C" fn WinInvertRect(hps: u32, prcl: *const crate::gpiapi::RectL) -> u32 {
    if prcl.is_null() {
        return 0;
    }
    let r = unsafe { prcl.read_unaligned() }.rect();
    let s = st();
    crate::gpiapi::refresh(s, hps);
    let mut surface = s.surface;
    crate::pointer::hide(s);
    // DSTINVERT.
    let ok = crate::gpiapi::entry(s, hps).map(|e| e.ps.blit(&mut surface, r, None, 0x55, false, None)).is_some();
    crate::pointer::show(s);
    ok as u32
}

/// WinGetScreenPS(hwndDesktop): a presentation space on the whole screen.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetScreenPS(_desk: u32) -> u32 {
    let s = st();
    let r = gpi::Rect::new(0, 0, s.surface.width, s.surface.height);
    crate::gpiapi::create(s, 0, gpi::Point::new(0, 0), alloc::vec![r])
}

/// Invert the outline of `r` (window coordinates) `b` pels wide.
fn invert_frame(hps: u32, r: &crate::gpiapi::RectL, b: i32) {
    use crate::gpiapi::RectL;
    if r.right <= r.left || r.top <= r.bottom {
        return;
    }
    let b = b.max(1);
    let edges = [
        RectL { left: r.left, bottom: r.bottom, right: r.right, top: (r.bottom + b).min(r.top) },
        RectL { left: r.left, bottom: (r.top - b).max(r.bottom + b), right: r.right, top: r.top },
        RectL { left: r.left, bottom: r.bottom + b, right: (r.left + b).min(r.right), top: r.top - b },
        RectL { left: (r.right - b).max(r.left + b), bottom: r.bottom + b, right: r.right, top: r.top - b },
    ];
    for e in edges.iter().filter(|e| e.right > e.left && e.top > e.bottom) {
        WinInvertRect(hps, e);
    }
}

/// WinTrackRect(hwnd, hps, ptrackinfo): track a rectangle with the mouse
/// (button 1 down; the TF_LEFT/TF_TOP/TF_RIGHT/TF_BOTTOM edges, or all
/// of them for TF_MOVE, follow the pointer) or the keyboard (arrows,
/// Enter, Esc), drawn as an inverted frame. TRACKINFO: cxBorder,
/// cyBorder, cxGrid, cyGrid, cxKeyboard, cyKeyboard, rclTrack,
/// rclBoundary, ptlMinTrackSize, ptlMaxTrackSize, fs. TRUE when it
/// ended with the button released or Enter; rclTrack holds the result.
#[unsafe(no_mangle)]
pub extern "C" fn WinTrackRect(h: u32, hps: u32, pti: *mut i32) -> u32 {
    use crate::gpiapi::RectL;
    const TF_LEFT: i32 = 1;
    const TF_TOP: i32 = 2;
    const TF_RIGHT: i32 = 4;
    const TF_BOTTOM: i32 = 8;
    const TF_ALLINBOUNDARY: i32 = 0x0080;
    const TF_PARTINBOUNDARY: i32 = 0x0200;
    if pti.is_null() || !st().desk.exists(h) {
        return 0;
    }
    let rd = |i: usize| unsafe { pti.add(i).read_unaligned() };
    let border = rd(0).max(rd(1)).max(1);
    let orig = RectL { left: rd(6), bottom: rd(7), right: rd(8), top: rd(9) };
    let bound = RectL { left: rd(10), bottom: rd(11), right: rd(12), top: rd(13) };
    let (min, max) = ((rd(14), rd(15)), (rd(16), rd(17)));
    let fs = rd(18);
    let edges = fs & 0xf;
    let own_ps = hps == 0;
    let hps = if own_ps { win::WinGetPS(h) } else { hps };
    let start = st().pointer;
    let o = st().desk.origin(h);
    let (sx, sy) = (start.x - o.x, start.y - o.y);
    // Keep the rectangle inside the boundary and within the sizes.
    let fit = |mut r: RectL| -> RectL {
        if edges == 0xf {
            let (w, hh) = (r.right - r.left, r.top - r.bottom);
            if fs & TF_ALLINBOUNDARY != 0 {
                let dx = (bound.left - r.left).max(0) - (r.right - bound.right).max(0);
                let dy = (bound.bottom - r.bottom).max(0) - (r.top - bound.top).max(0);
                r = RectL { left: r.left + dx, bottom: r.bottom + dy, right: r.left + dx + w, top: r.bottom + dy + hh };
            }
            return r;
        }
        if fs & (TF_ALLINBOUNDARY | TF_PARTINBOUNDARY) != 0 {
            r.left = r.left.clamp(bound.left, bound.right);
            r.right = r.right.clamp(bound.left, bound.right);
            r.bottom = r.bottom.clamp(bound.bottom, bound.top);
            r.top = r.top.clamp(bound.bottom, bound.top);
        }
        let (w, hh) = ((r.right - r.left).clamp(min.0.min(max.0), max.0.max(min.0)), (r.top - r.bottom).clamp(min.1.min(max.1), max.1.max(min.1)));
        if edges & TF_LEFT != 0 { r.left = r.right - w } else { r.right = r.left + w }
        if edges & TF_BOTTOM != 0 { r.bottom = r.top - hh } else { r.top = r.bottom + hh }
        r
    };
    // The rectangle for a pointer at (x, y) (window coordinates).
    let at = |x: i32, y: i32| -> RectL {
        let (dx, dy) = (x - sx, y - sy);
        let mut r = orig;
        if edges & TF_LEFT != 0 {
            r.left = orig.left + dx;
        }
        if edges & TF_RIGHT != 0 {
            r.right = orig.right + dx;
        }
        if edges & TF_BOTTOM != 0 {
            r.bottom = orig.bottom + dy;
        }
        if edges & TF_TOP != 0 {
            r.top = orig.top + dy;
        }
        // Dragging an edge past the opposite one turns the rectangle over.
        let (l, rr) = (r.left.min(r.right), r.left.max(r.right));
        let (b, t) = (r.bottom.min(r.top), r.bottom.max(r.top));
        fit(RectL { left: l, bottom: b, right: rr, top: t })
    };
    let mut cur = orig;
    invert_frame(hps, &cur, border);
    let old_capture = st().capture;
    st().capture = h;
    let mouse = st().buttons & 1 != 0;
    let mut ok = false;
    let mut m = state::QMsg::default();
    state::modal_begin("WinTrackRect", h);
    loop {
        if crate::msg::WinGetMsg(1, &mut m, 0, 0, 0) == 0 {
            crate::msg::WinPostQueueMsg(st().my_queue(), win::WM_QUIT, 0, 0);
            break;
        }
        if crate::hooks::msg_filter(&mut m, crate::hooks::MSGF_TRACK) {
            continue;
        }
        match m.msg {
            win::WM_MOUSEMOVE if m.hwnd == h => {
                let (x, y) = (m.mp1 as i16 as i32, (m.mp1 >> 16) as i16 as i32);
                let r = at(x, y);
                if (r.left, r.bottom, r.right, r.top) != (cur.left, cur.bottom, cur.right, cur.top) {
                    invert_frame(hps, &cur, border);
                    cur = r;
                    invert_frame(hps, &cur, border);
                }
            }
            win::WM_BUTTON1UP if mouse => {
                ok = true;
                break;
            }
            win::WM_BUTTON1DOWN | win::WM_BUTTON1UP => {}
            win::WM_CHAR => {
                let k = crate::ctl::Key::from(m.mp1, m.mp2);
                if k.up() {
                    continue;
                }
                let step = |d: i32| d * rd(4).max(1);
                let (dx, dy) = match k.vk {
                    crate::ctl::VK_LEFT => (step(-1), 0),
                    crate::ctl::VK_RIGHT => (step(1), 0),
                    crate::ctl::VK_UP => (0, step(1)),
                    crate::ctl::VK_DOWN => (0, step(-1)),
                    crate::ctl::VK_ENTER | crate::ctl::VK_NEWLINE => {
                        ok = true;
                        break;
                    }
                    crate::ctl::VK_ESC => break,
                    _ => (0, 0),
                };
                if dx != 0 || dy != 0 {
                    let mut r = cur;
                    if edges & TF_LEFT != 0 || edges == 0xf { r.left += dx }
                    if edges & TF_RIGHT != 0 || edges == 0xf { r.right += dx }
                    if edges & TF_BOTTOM != 0 || edges == 0xf { r.bottom += dy }
                    if edges & TF_TOP != 0 || edges == 0xf { r.top += dy }
                    invert_frame(hps, &cur, border);
                    cur = fit(r);
                    invert_frame(hps, &cur, border);
                }
            }
            _ => {
                crate::msg::WinDispatchMsg(1, &m);
            }
        }
    }
    state::modal_end();
    invert_frame(hps, &cur, border);
    if st().capture == h {
        st().capture = if old_capture == h { 0 } else { old_capture };
    }
    if own_ps {
        win::WinReleasePS(hps);
    }
    let r = if ok { cur } else { orig };
    for (i, v) in [r.left, r.bottom, r.right, r.top].into_iter().enumerate() {
        unsafe { pti.add(6 + i).write_unaligned(v) };
    }
    ok as u32
}
