//! The bidirectional language calls of IBM's PMMERGE (WinSetLangInfo and
//! friends, ordinals 3500-3505; PMBIDI forwards its Win32 entries here).
//! rs2's code pages are not bidirectional (WinQueryCpType says so), so
//! nothing reorders text: the calls keep what they are given, per window
//! (LI_BD_WND_ATTR, LI_BD_CLIP_ATTR) or for the process (LI_BD_PRC_*),
//! and tell the window as IBM's do.

use alloc::collections::BTreeMap;

unsafe extern "C" {
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.919"]
    fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.772"]
    fn WinIsWindow(hab: u32, h: u32) -> u32;
    #[link_name = "PMWIN.817"]
    fn WinQueryFocus(desk: u32) -> u32;
}

/// LI_BD_PRC_ATTR, LI_BD_PRC_STAT (the process's), LI_BD_WND_ATTR,
/// LI_BD_CLIP_ATTR (a window's).
const LI_BD_PRC_ATTR: u32 = 0x10;
const LI_BD_PRC_STAT: u32 = 0x11;
const LI_BD_WND_ATTR: u32 = 0x20;
const LI_BD_CLIP_ATTR: u32 = 0x21;
/// IBM's messages: set/query a window's bidirectional attributes, the
/// keyboard layer changed, to the language viewer.
const WM_SETBIDIATTR: u32 = 0x0bd0;
const WM_QUERYBIDIATTR: u32 = 0x0bd1;
const WM_SETBIDISTAT: u32 = 0x0bd2;
const WM_QUERYBIDISTAT: u32 = 0x0bd3;
const WM_KBDLAYERCHANGED: u32 = 0x0bd4;
const WM_LANGVIEWERNOTIFY: u32 = 0x0be0;
const SLI_NOTIFY: u32 = 0x0001;

struct Bidi {
    process: [u32; 2],
    windows: BTreeMap<(u32, u32), u32>,
    layers: BTreeMap<u32, u32>,
    viewer: u32,
}

static mut BIDI: Option<Bidi> = None;

fn bidi() -> &'static mut Bidi {
    let b = unsafe { &mut *core::ptr::addr_of_mut!(BIDI) };
    b.get_or_insert_with(|| Bidi { process: [0; 2], windows: BTreeMap::new(), layers: BTreeMap::new(), viewer: 0 })
}

/// WinSetLangInfo(hwnd, ulEffect, ulData, ulMask, ulFlags): the window
/// first gets the change as a message (it may take it: TRUE); else the
/// bits of ulMask become ulData's.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetLangInfo(h: u32, effect: u32, data: u32, mask: u32, fl: u32) -> u32 {
    match effect {
        LI_BD_PRC_ATTR | LI_BD_PRC_STAT => {
            let v = &mut bidi().process[(effect - LI_BD_PRC_ATTR) as usize];
            *v = *v & !mask | data & mask;
            1
        }
        LI_BD_WND_ATTR | LI_BD_CLIP_ATTR => {
            if h == 0 || unsafe { WinIsWindow(0, h) } == 0 {
                return 0;
            }
            let msg = if effect == LI_BD_WND_ATTR { WM_SETBIDIATTR } else { WM_SETBIDISTAT };
            if fl & 1 == 0 && unsafe { WinSendMsg(h, msg, data, mask) } == 1 {
                return 1;
            }
            let v = bidi().windows.entry((h, effect)).or_insert(0);
            *v = *v & !mask | data & mask;
            1
        }
        _ => 0,
    }
}

/// WinQueryLangInfo(hwnd, ulEffect, ulFlags): flag 1 asks rs2's record,
/// otherwise the window is asked.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryLangInfo(h: u32, effect: u32, fl: u32) -> u32 {
    match effect {
        LI_BD_PRC_ATTR | LI_BD_PRC_STAT => bidi().process[(effect - LI_BD_PRC_ATTR) as usize],
        LI_BD_WND_ATTR | LI_BD_CLIP_ATTR => {
            let kept = bidi().windows.get(&(h, effect)).copied().unwrap_or(0);
            if fl & 1 != 0 || h == 0 {
                return kept;
            }
            let msg = if effect == LI_BD_WND_ATTR { WM_QUERYBIDIATTR } else { WM_QUERYBIDISTAT };
            let r = unsafe { WinSendMsg(h, msg, 0, 0) };
            if r != 0 { r } else { kept }
        }
        _ => 0,
    }
}

/// WinSetKbdLayer(hwnd, idKbdLayer, ulFlags): the keyboard layer (0
/// Latin, 1 national) of a window (HWND_DESKTOP 1: the focus window).
/// rs2's keyboards have one layer: it is kept and announced.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetKbdLayer(h: u32, layer: u32, fl: u32) -> u32 {
    let w = if h == 1 { unsafe { WinQueryFocus(1) } } else { h };
    bidi().layers.insert(w, layer);
    if fl & SLI_NOTIFY != 0 && w != 0 {
        unsafe { WinPostMsg(w, WM_KBDLAYERCHANGED, layer, 0) };
    }
    let v = bidi().viewer;
    if v != 0 {
        unsafe { WinPostMsg(v, WM_LANGVIEWERNOTIFY, 4, layer) };
    }
    1
}

/// WinQueryKbdLayer(hwnd).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryKbdLayer(h: u32) -> u32 {
    let w = if h == 1 { unsafe { WinQueryFocus(1) } } else { h };
    bidi().layers.get(&w).copied().unwrap_or(0)
}

/// WinSetLangViewer(hab, hwnd): the window told about layer changes.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetLangViewer(_hab: u32, h: u32) -> u32 {
    bidi().viewer = h;
    1
}

/// WinQueryLangViewer(hab, cp).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryLangViewer(_hab: u32, _cp: u32) -> u32 {
    bidi().viewer
}
