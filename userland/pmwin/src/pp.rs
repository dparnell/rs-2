//! Presentation parameters, as IBM's PMWIN keeps them (PMMERGE's
//! GetpPresParam, QueryPresParam, XSetPresParam, XNotifyPresParamChanged,
//! CtlQueryColor).
//!
//! A window has a list of parameters (id, bytes) of any id: the PP_*
//! colours and fonts, PP_USER and up for applications. They come from
//! WinSetPresParam, WinCreateWindow's PRESPARAMS and dialog templates. A
//! window without a parameter takes its owner's (up the owner chain;
//! QPF_NOINHERIT asks the window only). Setting or removing one sends
//! WM_PRESPARAMCHANGED to the window and to the windows below it that take
//! it from there. The built-in controls draw with them (`color`): a
//! colour is the RGB parameter, or its index form (id + 1 for ids below
//! 0x32) as RGB, or the control's system colour.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::msg::send;
use crate::state::st;

pub const PP_FOREGROUNDCOLOR: u32 = 1;
pub const PP_BACKGROUNDCOLOR: u32 = 3;
pub const PP_HILITEFOREGROUNDCOLOR: u32 = 5;
pub const PP_HILITEBACKGROUNDCOLOR: u32 = 7;
pub const PP_DISABLEDFOREGROUNDCOLOR: u32 = 9;
pub const PP_DISABLEDBACKGROUNDCOLOR: u32 = 11;
pub const PP_BORDERCOLOR: u32 = 13;
pub const PP_FONTNAMESIZE: u32 = 15;
pub const PP_ACTIVECOLOR: u32 = 18;
pub const PP_INACTIVECOLOR: u32 = 20;
pub const PP_ACTIVETEXTFGNDCOLOR: u32 = 22;
pub const PP_ACTIVETEXTBGNDCOLOR: u32 = 24;
pub const PP_INACTIVETEXTFGNDCOLOR: u32 = 26;
pub const PP_INACTIVETEXTBGNDCOLOR: u32 = 28;
pub const PP_MENUFOREGROUNDCOLOR: u32 = 31;
pub const PP_MENUBACKGROUNDCOLOR: u32 = 33;
pub const PP_MENUHILITEFGNDCOLOR: u32 = 35;
pub const PP_MENUHILITEBGNDCOLOR: u32 = 37;
pub const PP_MENUDISABLEDFGNDCOLOR: u32 = 39;
pub const PP_BORDERLIGHTCOLOR: u32 = 51;
pub const PP_BORDERDARKCOLOR: u32 = 52;
pub const PP_BORDERDEFAULTCOLOR: u32 = 56;
pub const PP_FIELDBACKGROUNDCOLOR: u32 = 57;
pub const PP_PAGEFOREGROUNDCOLOR: u32 = 68;
pub const PP_PAGEBACKGROUNDCOLOR: u32 = 69;
pub const PP_MAJORTABFOREGROUNDCOLOR: u32 = 70;
pub const PP_MAJORTABBACKGROUNDCOLOR: u32 = 71;
pub const PP_MINORTABFOREGROUNDCOLOR: u32 = 72;
pub const PP_MINORTABBACKGROUNDCOLOR: u32 = 73;

const QPF_NOINHERIT: u32 = 0x0001;
const QPF_ID1COLORINDEX: u32 = 0x0002;
const QPF_ID2COLORINDEX: u32 = 0x0004;
const QPF_PURERGBCOLOR: u32 = 0x0008;

pub const WM_PRESPARAMCHANGED: u32 = 0x002f;

/// A window's own parameters.
fn list(h: u32) -> Option<&'static Vec<(u32, Vec<u8>)>> {
    st().pp.get(&h)
}

/// The first of `id1`/`id2` the window has (in the order they were set),
/// or, unless `own`, its owner, and so on up the owner chain (IBM's
/// GetpPresParam): (the window it was found on, id, bytes).
pub fn find(h: u32, id1: u32, id2: u32, own: bool) -> Option<(u32, u32, Vec<u8>)> {
    let s = st();
    let mut w = h;
    for _ in 0..64 {
        if w == 0 {
            return None;
        }
        if let Some(p) = list(w).and_then(|l| l.iter().find(|(id, _)| *id == id1 || (id2 != 0 && *id == id2))) {
            return Some((w, p.0, p.1.clone()));
        }
        if own {
            return None;
        }
        let x = s.desk.get(w)?;
        w = if x.owner == 0 {
            0
        } else if x.owner == w {
            x.parent
        } else {
            x.owner
        };
    }
    None
}

/// The colour a control draws with: the RGB parameter `id`, its index
/// form (id + 1, for ids below 0x32), inherited; else `default` (a
/// SYSCLR_* index, or an RGB value when not negative).
pub fn color(h: u32, id: u32, default: i32) -> u32 {
    let idx = if id < 0x32 { id + 1 } else { 0 };
    if let Some((_, found, b)) = find(h, id, idx, false) {
        if b.len() >= 4 {
            let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            return if found == idx { crate::color::resolve(v as i32) } else { v & 0xff_ffff };
        }
    }
    if default < 0 { crate::color::resolve(default) } else { default as u32 & 0xff_ffff }
}

/// A colour parameter of the window or its owners, if there is one.
pub fn color_set(h: u32, id: u32) -> Option<u32> {
    let idx = if id < 0x32 { id + 1 } else { 0 };
    find(h, id, idx, false).filter(|(_, _, b)| b.len() >= 4).map(|(_, found, b)| {
        let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if found == idx { crate::color::resolve(v as i32) } else { v & 0xff_ffff }
    })
}

/// The colours of a text control: background, foreground, selection
/// background and foreground; the disabled ones when it is disabled
/// (IBM's EditQueryPresParams and LboxQueryPresParams).
#[derive(Clone, Copy)]
pub struct TextColors {
    pub bg: u32,
    pub fg: u32,
    pub hilite_bg: u32,
    pub hilite_fg: u32,
}

pub fn text_colors(h: u32) -> TextColors {
    let enabled = crate::ctl::enabled(h);
    let (fg, bg) = if enabled {
        (or(h, PP_FOREGROUNDCOLOR, crate::draw::text()), or(h, PP_BACKGROUNDCOLOR, crate::draw::window()))
    } else {
        (
            color_set(h, PP_DISABLEDFOREGROUNDCOLOR).unwrap_or(crate::draw::GRAY_TEXT),
            color_set(h, PP_DISABLEDBACKGROUNDCOLOR).or_else(|| color_set(h, PP_BACKGROUNDCOLOR)).unwrap_or(crate::draw::window()),
        )
    };
    TextColors { bg, fg, hilite_bg: or(h, PP_HILITEBACKGROUNDCOLOR, crate::draw::hilite()), hilite_fg: or(h, PP_HILITEFOREGROUNDCOLOR, crate::draw::hilite_text()) }
}

/// `color_set`, or `default` (an RGB value) when there is none.
pub fn or(h: u32, id: u32, default: u32) -> u32 {
    color_set(h, id).unwrap_or(default)
}

/// The window's font name (PP_FONTNAMESIZE, inherited).
pub fn font_name(h: u32) -> Option<Vec<u8>> {
    let (_, _, b) = find(h, PP_FONTNAMESIZE, 0, false)?;
    Some(b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())].to_vec())
}

/// Set a parameter without telling anyone (creation).
fn store(h: u32, id: u32, data: &[u8]) {
    let l = st().pp.entry(h).or_default();
    match l.iter_mut().find(|(i, _)| *i == id) {
        Some(e) => e.1 = data.to_vec(),
        None => l.push((id, data.to_vec())),
    }
}

/// WM_PRESPARAMCHANGED to `h` and the windows that take parameter `id`
/// from it: its children and owned windows that do not have it
/// themselves (IBM's XNotifyPresParamChanged), and a repaint.
fn notify(h: u32, id: u32) {
    let s = st();
    let mut todo = alloc::vec![h];
    let mut done = Vec::new();
    while let Some(w) = todo.pop() {
        if done.contains(&w) || !st().desk.exists(w) {
            continue;
        }
        done.push(w);
        st().desk.invalidate(w, None, false);
        send(w, WM_PRESPARAMCHANGED, id, 0);
        // The windows that inherit through `w`.
        let kids: Vec<u32> = s.desk.all().filter(|x| x.owner == w || (x.owner == x.hwnd && x.parent == w)).map(|x| x.hwnd).collect();
        for k in kids {
            if find(k, id, if id < 0x32 { id + 1 } else { 0 }, true).is_none() {
                todo.push(k);
            }
        }
    }
}

/// WinSetPresParam(hwnd, id, cbParam, pbParam).
#[unsafe(no_mangle)]
pub extern "C" fn WinSetPresParam(h: u32, id: u32, cb: u32, pv: u32) -> u32 {
    if !st().desk.exists(h) || (pv == 0 && cb != 0) {
        return 0;
    }
    let data = if cb == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(pv as *const u8, cb as usize) }.to_vec() };
    if id == PP_FONTNAMESIZE {
        let name = &data[..data.iter().position(|&c| c == 0).unwrap_or(data.len())];
        if crate::fonts::by_name(name).is_none() {
            return 0;
        }
    }
    // An RGB colour and its index form replace each other.
    if id < 0x32 && id != PP_FONTNAMESIZE && id != PP_FONTNAMESIZE + 1 {
        let other = if id % 2 == 1 { id + 1 } else { id - 1 };
        if let Some(l) = st().pp.get_mut(&h) {
            l.retain(|(i, _)| *i != other);
        }
    }
    store(h, id, &data);
    notify(h, id);
    1
}

/// WinQueryPresParam(hwnd, id1, id2, pulId, cbBuf, pbBuf, fs): the bytes
/// of the first of id1/id2 found (QPF_NOINHERIT: on the window only);
/// QPF_ID1COLORINDEX/QPF_ID2COLORINDEX turn that id's colour index into
/// RGB, QPF_PURERGBCOLOR gives the nearest pure colour (rs2's display is
/// true colour: the same). Returns the number of bytes copied.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryPresParam(h: u32, id1: u32, id2: u32, pid: u32, cb: u32, pv: u32, fs: u32) -> u32 {
    let Some((_, found, b)) = find(h, id1, id2, fs & QPF_NOINHERIT != 0) else { return 0 };
    if pid != 0 {
        unsafe { (pid as *mut u32).write_unaligned(found) };
    }
    let out = if fs & (QPF_ID1COLORINDEX | QPF_ID2COLORINDEX | QPF_PURERGBCOLOR) == 0 {
        b
    } else {
        let mut v = [0u8; 4];
        let n = b.len().min(4);
        v[..n].copy_from_slice(&b[..n]);
        let mut c = u32::from_le_bytes(v);
        if (found == id1 && fs & QPF_ID1COLORINDEX != 0) || (found == id2 && fs & QPF_ID2COLORINDEX != 0) {
            c = crate::color::resolve(c as i32);
        }
        c.to_le_bytes().to_vec()
    };
    let n = out.len().min(cb as usize);
    if pv != 0 {
        unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), pv as *mut u8, n) };
    }
    n as u32
}

/// WinRemovePresParam(hwnd, id).
#[unsafe(no_mangle)]
pub extern "C" fn WinRemovePresParam(h: u32, id: u32) -> u32 {
    if !st().desk.exists(h) {
        return 0;
    }
    let had = st().pp.get_mut(&h).is_some_and(|l| {
        let n = l.len();
        l.retain(|(i, _)| *i != id);
        l.len() != n
    });
    if had {
        notify(h, id);
    }
    1
}

/// The PRESPARAMS (cb, then PARAMs: id, cb, bytes) a window being made
/// gets before WM_CREATE (`take_pending`).
static PENDING: AtomicU32 = AtomicU32::new(0);

/// WinCreateWindow's pPresParams for the next window `win::create`
/// makes on this thread.
pub fn set_pending(p: u32) {
    PENDING.store(p, Ordering::Relaxed);
}

/// Give the window being made the pending PRESPARAMS.
pub fn take_pending(h: u32) {
    let p = PENDING.swap(0, Ordering::Relaxed);
    if p == 0 {
        return;
    }
    let total = unsafe { (p as *const u32).read_unaligned() } as usize;
    let mut at = 4usize;
    while at + 8 <= total + 4 {
        let (id, cb) = unsafe { (((p as usize + at) as *const u32).read_unaligned(), ((p as usize + at + 4) as *const u32).read_unaligned() as usize) };
        if at + 8 + cb > total + 4 {
            break;
        }
        let data = unsafe { core::slice::from_raw_parts((p as usize + at + 8) as *const u8, cb) };
        let ok = id != PP_FONTNAMESIZE || crate::fonts::by_name(&data[..data.iter().position(|&c| c == 0).unwrap_or(data.len())]).is_some();
        if ok {
            store(h, id, data);
        }
        at += 8 + cb;
    }
}

/// A window went: its parameters go.
pub fn forget(h: u32) {
    st().pp.remove(&h);
}
