//! rs2's own abstract objects for what RS2SHELL's desktop had and IBM's
//! shell keeps elsewhere (the LaunchPad's buttons, WPCONFIG's System
//! object):
//!
//! - RS2Shutdown (`<RS2_SHUTDOWN>`): opening it shuts down, as the
//!   desktop menu's Shut down does;
//! - RS2WindowList (`<RS2_WINLIST>`): opening it shows the window list
//!   (Ctrl+Esc);
//! - RS2System (`<RS2_SYSTEM>`, in System Setup): its settings add a
//!   Window page, where minimized windows go (the Minimized Window Viewer
//!   or the desktop).

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj::OPEN_SETTINGS;
use crate::pm::*;
use crate::som;
use crate::wps::{self, Impl};

fn parent_open(class: &str, obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    som::call_parent(wps::ccd(class), wps::cd("WPObject"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize]) as u32
}

extern "C" fn shutdown_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return parent_open("RS2Shutdown", obj, cnr, view, param);
    }
    crate::shutdown::ask();
    1
}

extern "C" fn winlist_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return parent_open("RS2WindowList", obj, cnr, view, param);
    }
    post(HWND_DESKTOP, WM_RS2_WINLIST, 0, 0);
    1
}

// ---- System: the Window page ----

const ID_VIEWER: u16 = 420;
const ID_DESKTOP: u16 = 421;

extern "C" fn window_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let id = if crate::minwin::to_viewer() { ID_VIEWER } else { ID_DESKTOP };
            unsafe { WinSendDlgItemMsg(h, id as u32, BM_SETCHECK, 1, 0) };
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            // BN_CLICKED on a radio button.
            match (a & 0xffff) as u16 {
                ID_VIEWER => crate::minwin::set_to_viewer(true),
                ID_DESKTOP => crate::minwin::set_to_viewer(false),
                _ => {}
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn system_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("RS2System"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    // BS_AUTORADIOBUTTON
    let radio = |t: &[u8], y: i16, id: u16, first: bool| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (10, y, 216, 10), id);
    let tpl = template(book::page_frame(), alloc::vec![
        book::label(b"Minimized windows go to:", (6, 104, 220)),
        radio(b"the Minimized ~Window Viewer", 88, ID_VIEWER, true),
        radio(b"the ~desktop (as icons)", 74, ID_DESKTOP, false),
    ]);
    let page = book::make_page(nb, window_page_proc, &tpl, o);
    if page != 0 {
        book::insert(o, nb, page, b"~Window\0");
    }
    1
}

methods! {
    __m_shutdown_open => shutdown_open, 4;
    __m_winlist_open => winlist_open, 4;
    __m_system_add_pages => system_add_pages, 2;
}

pub fn impl_shutdown() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_shutdown_open as usize)] }
}

pub fn impl_window_list() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_winlist_open as usize)] }
}

pub fn impl_system() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpAddSettingsPages", __m_system_add_pages as usize)] }
}
