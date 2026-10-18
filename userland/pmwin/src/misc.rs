//! Initialisation, errors, system values and colours, string resources and
//! message boxes.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use gpi::{Point, Rect, Surface};

use crate::color;
use crate::pointer;
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

#[repr(C)]
#[derive(Default)]
struct Input {
    kind: u16,
    flags: u16,
    a: u32,
    b: u32,
    time: u32,
}

unsafe extern "C" {
    #[link_name = "RS2API.1"]
    fn Rs2MapDisplay(info: *mut DisplayInfo) -> u32;
    #[link_name = "RS2API.2"]
    fn Rs2ReadInput(ev: *mut Input, ms: u32) -> u32;
    #[link_name = "DOSCALLS.352"]
    fn DosGetResource(hmod: u32, ty: u32, id: u32, ppb: *mut *const u8) -> u32;
    #[link_name = "DOSCALLS.353"]
    fn DosFreeResource(pb: *const u8) -> u32;
    #[link_name = "DOSCALLS.286"]
    fn DosBeep(freq: u32, ms: u32) -> u32;
}

const HAB: u32 = 1;

/// WinInitialize(flOptions): the anchor block. The first call maps the
/// display and joins the Presentation Manager session.
#[unsafe(no_mangle)]
pub extern "C" fn WinInitialize(_fl: u32) -> u32 {
    if state::get().is_some() {
        return HAB;
    }
    let mut info = DisplayInfo::default();
    if unsafe { Rs2MapDisplay(&mut info) } != 0 {
        return 0;
    }
    let surface = unsafe {
        Surface::new(info.address as *mut u32, info.width as i32, info.height as i32, info.stride as i32, info.flags & 1 != 0)
    };
    state::init(surface);
    win::register_builtin();
    pointer::draw(st());
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
    state::get().map_or(0, |s| core::mem::take(&mut s.last_error))
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

/// WinLoadString(hab, hmod, id, cchMax, pchBuffer): string tables are
/// bundles of 16 strings (resource id = id / 16 + 1): a code page word,
/// then for each string a length byte (including the NUL) and the text.
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadString(_hab: u32, hmod: u32, id: u32, cch: i32, buf: *mut u8) -> i32 {
    if buf.is_null() || cch <= 0 {
        return 0;
    }
    let mut p: *const u8 = core::ptr::null();
    if unsafe { DosGetResource(hmod, 5, id / 16 + 1, &mut p) } != 0 || p.is_null() {
        unsafe { *buf = 0 };
        return 0;
    }
    let mut at = unsafe { p.add(2) };
    for _ in 0..id % 16 {
        let len = unsafe { *at } as usize;
        at = unsafe { at.add(1 + len) };
    }
    let len = unsafe { *at } as usize;
    let text = unsafe { core::slice::from_raw_parts(at.add(1), len.saturating_sub(1)) };
    let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
    let n = text.len().min(cch as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), buf, n);
        *buf.add(n) = 0;
        DosFreeResource(p);
    }
    n as i32
}

fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

/// WinMessageBox(hwndParent, hwndOwner, pszText, pszCaption, idWindow,
/// flStyle): a modal box drawn over the desktop, answered with the mouse
/// (its button) or Enter/Esc.
#[unsafe(no_mangle)]
pub extern "C" fn WinMessageBox(_parent: u32, _owner: u32, text: *const u8, title: *const u8, _id: u32, style: u32) -> u32 {
    const MB_OKCANCEL: u32 = 1;
    const MB_CANCEL: u32 = 6;
    const MBID_OK: u32 = 1;
    const MBID_CANCEL: u32 = 2;
    let s = st();
    let font = gpi::font::system();
    let text = cstr(text);
    let title = cstr(title);
    let lines: Vec<&[u8]> = text.split(|&b| b == b'\n').collect();
    let tw = lines.iter().map(|l| font.text_width(l)).max().unwrap_or(0).max(font.text_width(&title)).max(160);
    let (w, h) = (tw + 40, lines.len() as i32 * font.height + 90);
    let (sw, sh) = (s.surface.width, s.surface.height);
    let r = Rect::new((sw - w) / 2, (sh - h) / 2, (sw + w) / 2, (sh + h) / 2);
    let button = Rect::new(r.left + w / 2 - 40, r.bottom + 12, r.left + w / 2 + 40, r.bottom + 38);
    let kind = style & 0xf;
    let label: &[u8] = if kind == MB_CANCEL { b"Cancel" } else { b"OK" };
    pointer::hide(s);
    let mut ps = gpi::Ps::new(Point::new(0, 0), alloc::vec![r]);
    let mut surf = s.surface;
    ps.fill_rect(&mut surf, r, 0xcccccc);
    ps.move_to(Point::new(r.left, r.bottom));
    ps.draw_box(&mut surf, Point::new(r.right - 1, r.top - 1), true, false);
    let tbar = Rect::new(r.left + 3, r.top - 3 - win::TITLE_H, r.right - 3, r.top - 3);
    ps.fill_rect(&mut surf, tbar, color::sys(color::SYSCLR_ACTIVETITLE));
    ps.color = 0xffffff;
    ps.char_string_at(&mut surf, Point::new(tbar.left + (tbar.width() - font.text_width(&title)) / 2, tbar.bottom + 3 + font.descender), &title);
    ps.color = 0x000000;
    for (i, l) in lines.iter().enumerate() {
        let y = tbar.bottom - 10 - (i as i32 + 1) * font.height + font.descender;
        ps.char_string_at(&mut surf, Point::new(r.left + 20, y), l);
    }
    ps.fill_rect(&mut surf, button, 0xe0e0e0);
    ps.move_to(Point::new(button.left, button.bottom));
    ps.draw_box(&mut surf, Point::new(button.right - 1, button.top - 1), true, false);
    ps.char_string_at(&mut surf, Point::new(button.left + (button.width() - font.text_width(label)) / 2, button.bottom + 5 + font.descender), label);
    pointer::show(s);
    let result = loop {
        let mut ev = Input::default();
        if unsafe { Rs2ReadInput(&mut ev, u32::MAX) } != 0 {
            break MBID_CANCEL;
        }
        match ev.kind {
            2 => {
                let p = Point::new(ev.a as i32, ev.b as i32);
                pointer::move_to(st(), p);
                let down = ev.flags & 1 != 0 && st().buttons & 1 == 0;
                st().buttons = ev.flags as u8;
                if down && button.contains(p) {
                    break if kind == MB_CANCEL { MBID_CANCEL } else { MBID_OK };
                }
            }
            1 => match ev.a & 0xff {
                0x0d => break if kind == MB_CANCEL { MBID_CANCEL } else { MBID_OK },
                0x1b if kind == MB_OKCANCEL || kind == MB_CANCEL => break MBID_CANCEL,
                _ => {}
            },
            _ => {}
        }
    };
    st().desk.invalidate_screen(r);
    result
}
