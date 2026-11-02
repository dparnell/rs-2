//! Help for objects: wpQueryDefaultHelp gives an object's panel in the
//! shell's help library (WPHELP.HLP, rs2's own, userland/help/WPHELP.IPF,
//! with OS/2's panel numbers), wpDisplayHelp shows a panel in a help
//! window. F1 in a view (the container's CN_HELP) and the Help items of
//! the pop-up menus come here. Each help library gets one help instance,
//! made on first use and associated with the frame the request came from
//! (the help window belongs to it).

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::util::{cstr, z};
use crate::wps;

unsafe extern "C" {
    #[link_name = "HELPMGR.51"]
    fn WinCreateHelpInstance(hab: u32, init: *mut u32) -> u32;
    #[link_name = "HELPMGR.54"]
    fn WinAssociateHelpInstance(help: u32, frame: u32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.799"]
    fn WinQueryActiveWindow(desktop: u32) -> u32;
}

/// The shell's help library.
pub const LIBRARY: &[u8] = b"WPHELP.HLP";

pub const HM_DISPLAY_HELP: u32 = 0x0222;
pub const HM_EXT_HELP: u32 = 0x0223;
pub const HM_HELP_INDEX: u32 = 0x022a;
pub const HM_KEYS_HELP: u32 = 0x022c;
pub const HM_ERROR: u32 = 0x022e;
pub const HMERR_HELPTABLE_UNDEFINE: u32 = 0x1009;
const HM_RESOURCEID: u32 = 0;

/// Panels of WPHELP.HLP.
pub const PANEL_HELP: u32 = 0x3e8;
pub const PANEL_KEYS: u32 = 0x9003;
const PANEL_OBJECT: u32 = 0x7d7d;

/// Classes and their panels, most derived first (the first class the
/// object is an instance of wins).
const CLASS_PANELS: &[(&str, u32)] = &[
    ("WPDesktop", 0xfa0),
    ("WPDrives", 0x1f4d),
    ("WPTemplates", 0x3d40),
    ("WPStartup", 0x1f42),
    ("WPMinWinViewer", 0x2446),
    ("WPNetwork", 0x7549),
    ("WPFolder", 0x3d45),
    ("WPProgramFile", 0xfd7),
    ("WPUrl", 0x848d),
    ("WPDataFile", 0x3d46),
    ("WPProgram", 0x3d44),
    ("WPShadow", 0x4b3),
    ("WPDisk", 0x7d8c),
    ("WPShredder", 0x4a6),
    ("WPSystem", 0xfbe),
    ("WPClock", 0x482),
    ("WPKeyboard", 0x36bc),
    ("WPMouse", 0x44c),
    ("WPSound", 0x4a2),
    ("WPColorPalette", 0x3cf1),
    ("WPSchemePalette", 0x17a6),
    ("WPFontPalette", 0x1f82),
    ("WPSpool", 0x3d72),
    ("WPPrinter", 0x3d6c),
    ("WPLaunchPad", 0x7e90),
    ("RS2Shutdown", 0xfa1),
    ("RS2WindowList", 0x36c6),
];

/// Folders known by their object ids.
const ID_PANELS: &[(&[u8], u32)] = &[
    (b"<WP_INFO>", 0x7ea0),
    (b"<WP_ASSISTANCE>", 0x83bb),
    (b"<WP_PROMPTS>", 0x1f48),
    (b"<WP_CONFIG>", 0x4c4),
    (b"<WP_OS2SYS>", 0xfa2),
    (b"<WP_GLOSS>", 0x13a7),
    (b"<WP_MINDEX>", 0x37df),
];

/// The help instances made: (library, instance).
static mut INSTANCES: Vec<(Vec<u8>, u32)> = Vec::new();

/// The help instance for `lib` (made the first time).
fn instance(lib: &[u8]) -> u32 {
    #[allow(static_mut_refs)]
    let list = unsafe { &mut INSTANCES };
    if let Some(&(_, h)) = list.iter().find(|(l, _)| l.eq_ignore_ascii_case(lib)) {
        return h;
    }
    let name = z(lib);
    // HELPINIT: cb, ulReturnCode, pszTutorialName, phtHelpTable,
    // hmodHelpTableModule, hmodAccelActionBarModule, idAccelTable,
    // idActionBar, pszHelpWindowTitle, fShowPanelId, pszHelpLibraryName.
    let mut hi: [u32; 11] = [44, 0, 0, 0, 0, 0, 0, 0, b"Desktop Help\0".as_ptr() as u32, 0, name.as_ptr() as u32];
    let h = unsafe { WinCreateHelpInstance(WinQueryAnchorBlock(HWND_DESKTOP), hi.as_mut_ptr()) };
    if h != 0 {
        list.push((lib.to_vec(), h));
    }
    h
}

/// Give `frame` the shell's help instance: F1 there reaches its client
/// as HM_ERROR (no help table).
pub fn associate(frame: u32) {
    let h = instance(LIBRARY);
    if h != 0 && frame != 0 {
        unsafe { WinAssociateHelpInstance(h, frame) };
    }
}

/// Send a help message for `lib`, the help window belonging to `frame`
/// (or the active window). False when there is no help.
pub fn request(frame: u32, lib: &[u8], msg: u32, a: u32, b: u32) -> bool {
    let h = instance(lib);
    if h == 0 {
        return false;
    }
    let f = if frame != 0 { frame } else { unsafe { WinQueryActiveWindow(HWND_DESKTOP) } };
    if f != 0 {
        unsafe { WinAssociateHelpInstance(h, f) };
    }
    send(h, msg, a, b) == 0
}

/// An object's panel in WPHELP.HLP.
fn class_panel(o: usize) -> u32 {
    let own = obj::d(o).help_panel;
    if own != 0 {
        return own;
    }
    let id = obj::call(m::WPObject::wpQueryObjectID, &[o]);
    if id != 0 {
        let id = cstr(id as *const u8);
        if let Some(&(_, p)) = ID_PANELS.iter().find(|(i, _)| i.eq_ignore_ascii_case(id)) {
            return p;
        }
    }
    CLASS_PANELS.iter().find(|(c, _)| wps::is(o, c)).map_or(PANEL_OBJECT, |&(_, p)| p)
}

/// wpQueryDefaultHelp(pHelpPanelId, pszHelpLibrary).
extern "C" fn wpQueryDefaultHelp(o: usize, panel: *mut u32, lib: *mut u8) -> u32 {
    if !panel.is_null() {
        unsafe { *panel = class_panel(o) };
    }
    if !lib.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(LIBRARY.as_ptr(), lib, LIBRARY.len());
            *lib.add(LIBRARY.len()) = 0;
        }
    }
    1
}

/// wpDisplayHelp(HelpPanelId, HelpLibrary): panel 0 is the object's
/// default help.
extern "C" fn wpDisplayHelp(o: usize, panel: u32, lib: *const u8) -> u32 {
    let (mut p, mut l) = (panel, cstr(lib).to_vec());
    if p == 0 || l.is_empty() {
        let mut buf = [0u8; 260];
        let mut dp = 0u32;
        obj::call(m::WPObject::wpQueryDefaultHelp, &[o, &mut dp as *mut u32 as usize, buf.as_mut_ptr() as usize]);
        if p == 0 {
            p = dp;
        }
        if l.is_empty() {
            l = cstr(buf.as_ptr()).to_vec();
        }
    }
    if l.is_empty() {
        l = LIBRARY.to_vec();
    }
    request(0, &l, HM_DISPLAY_HELP, p, HM_RESOURCEID) as u32
}

/// Help for `o` (F1 in a view, General help in its menu).
pub fn show(o: usize) {
    obj::call(m::WPObject::wpDisplayHelp, &[o, 0, 0]);
}

methods! {
    __m_wpQueryDefaultHelp => wpQueryDefaultHelp, 3;
    __m_wpDisplayHelp => wpDisplayHelp, 3;
}

pub fn object_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpQueryDefaultHelp", __m_wpQueryDefaultHelp as usize), ("wpDisplayHelp", __m_wpDisplayHelp as usize)]
}
