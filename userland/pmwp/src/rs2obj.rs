//! rs2's own abstract objects for what RS2SHELL's desktop had and IBM's
//! shell keeps elsewhere (the LaunchPad's buttons, WPCONFIG's System
//! object):
//!
//! - RS2Shutdown (`<RS2_SHUTDOWN>`): opening it shuts down, as the
//!   desktop menu's Shut down does;
//! - RS2WindowList (`<RS2_WINLIST>`): opening it shows the window list
//!   (Ctrl+Esc).
//!
//! RS2System, the System object of rs2's earlier desktops, is now IBM's
//! WPSystem (system.rs).

use alloc::vec::Vec;

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

methods! {
    __m_shutdown_open => shutdown_open, 4;
    __m_winlist_open => winlist_open, 4;
}

pub fn impl_shutdown() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_shutdown_open as usize)] }
}

pub fn impl_window_list() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_winlist_open as usize)] }
}

