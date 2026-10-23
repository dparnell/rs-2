//! The Minimized Window Viewer (WPMinWinViewer, `<WP_VIEWER>`) and its
//! objects (WPMinWindow, transient: one per minimized window, as in
//! IBM's shell). PMWIN hides a minimized frame and posts
//! WM_RS2_MINWINDOW (frame, on) to the window set with
//! Rs2SetMinimizeViewer: the desktop's, while windows are to go to the
//! viewer (OS2.INI `RS2_Desktop` `MinimizeTo` is not DESKTOP, as for
//! RS2SHELL; the System object's Window page sets it). Opening a
//! WPMinWindow restores its window.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_SETTINGS};
use crate::pm::*;
use crate::som;
use crate::util::*;
use crate::util::z;
use crate::wps::{self, Impl};

pub const TID_MINWATCH: u32 = 0x5702;
const APP: &[u8] = b"RS2_Desktop";
const KEY: &[u8] = b"MinimizeTo";

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}

#[repr(C)]
pub struct MinData {
    pub frame: u32,
}

fn md<'a>(obj: usize) -> &'a mut MinData {
    som::data(wps::ccd("WPMinWindow"), obj)
}

/// The minimized frames and their objects.
static mut LIST: Vec<(u32, usize)> = Vec::new();

#[allow(static_mut_refs)]
fn list() -> &'static mut Vec<(u32, usize)> {
    unsafe { &mut LIST }
}

/// Whether minimized windows go to the viewer (the default).
pub fn to_viewer() -> bool {
    prf_read(HINI_USER, APP, KEY).is_none_or(|v| v.strip_suffix(&[0]).unwrap_or(&v) != b"DESKTOP")
}

pub fn set_to_viewer(on: bool) {
    prf_write(HINI_USER, APP, KEY, Some(if on { b"VIEWER\0" } else { b"DESKTOP\0" }));
    apply();
}

/// Tell PMWIN where minimized windows go, and watch for those that end.
pub fn apply() {
    let client = crate::view::desktop_client();
    unsafe { Rs2SetMinimizeViewer(if to_viewer() { client } else { 0 }) };
    if client != 0 {
        unsafe { WinStartTimer(WinQueryAnchorBlock(HWND_DESKTOP), client, TID_MINWATCH, 2000) };
    }
}

fn viewer() -> usize {
    wps::object_from_location(b"<WP_VIEWER>")
}

/// WM_RS2_MINWINDOW: a frame was minimized to the viewer (`on`) or
/// came back.
pub fn changed(frame: u32, on: bool) {
    debug(&alloc::format!("minwin: frame {:x} {}", frame, if on { "minimized" } else { "back" }));
    if let Some(i) = list().iter().position(|&(f, _)| f == frame) {
        let (_, o) = list().remove(i);
        drop_object(o);
    }
    if on {
        let o = make_object(frame);
        list().push((frame, o));
    }
}

/// Windows that ended while minimized leave the viewer.
pub fn check() {
    let gone: Vec<u32> = list()
        .iter()
        .map(|&(f, _)| f)
        .filter(|&f| unsafe { WinIsWindow(0, f) == 0 || WinQueryWindowULong(f, QWL_STYLE) & WS_MINIMIZED == 0 })
        .collect();
    for f in gone {
        debug(&alloc::format!("minwin: frame {:x} gone", f));
        changed(f, false);
    }
}

fn make_object(frame: u32) -> usize {
    let cls = wps::class("WPMinWindow");
    let v = viewer();
    let mut t = frame_title(frame);
    if t.is_empty() {
        t = b"(untitled)".to_vec();
    }
    let t = z(&t);
    let style = obj::OBJSTYLE_NOMOVE | obj::OBJSTYLE_NOCOPY | obj::OBJSTYLE_NOLINK | obj::OBJSTYLE_NODELETE | obj::OBJSTYLE_NORENAME | obj::OBJSTYLE_NOSETTINGS | obj::OBJSTYLE_NODRAG;
    let o = wps::call("M_WPObject", m::M_WPObject::wpclsMakeAwake, &[cls, t.as_ptr() as usize, style as usize, 0, 0, v, 0]);
    if o == 0 {
        debug(&alloc::format!("minwin: no object (class {:x}, viewer {:x})", cls, v));
        return 0;
    }
    md(o).frame = frame;
    debug(&alloc::format!("minwin: object {:x} in viewer {:x}", o, v));
    if v != 0 {
        wps::call("WPFolder", m::WPFolder::wpAddToContent, &[v, o]);
    }
    o
}

fn drop_object(o: usize) {
    if o == 0 {
        return;
    }
    let v = obj::d(o).folder;
    if v != 0 {
        wps::call("WPFolder", m::WPFolder::wpDeleteFromContent, &[v, o]);
    }
    som::so_call(som::so::SOM_FREE, &[o]);
}

// ---- WPMinWindow ----

/// Opening a minimized window restores it.
extern "C" fn min_open(obj: usize, _cnr: u32, view: u32, _param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return 0;
    }
    let f = md(obj).frame;
    unsafe { WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER | SWP_RESTORE) };
    f
}

/// The window's own icon.
extern "C" fn min_icon(obj: usize) -> u32 {
    let ic = send(md(obj).frame, WM_QUERYICON, 0, 0);
    if ic != 0 {
        return ic;
    }
    som::call_parent(wps::ccd("WPMinWindow"), wps::cd("WPObject"), m::WPObject::wpQueryIcon, &[obj]) as u32
}

// ---- WPMinWinViewer ----

/// The viewer's content: its directory's and the minimized windows.
extern "C" fn viewer_populate(obj: usize, r: u32, p: *const u8, folders_only: u32) -> u32 {
    som::call_parent(wps::ccd("WPMinWinViewer"), wps::cd("WPFolder"), m::WPFolder::wpPopulate, &[obj, r as usize, p as usize, folders_only as usize]);
    let objs: Vec<usize> = list().iter().map(|&(_, o)| o).filter(|&o| o != 0).collect();
    for o in objs {
        obj::d(o).folder = obj;
        wps::call("WPFolder", m::WPFolder::wpAddToContent, &[obj, o]);
    }
    1
}

methods! {
    __m_min_open => min_open, 4;
    __m_min_icon => min_icon, 1;
    __m_viewer_populate => viewer_populate, 4;
}

pub fn impl_min_window() -> Impl {
    Impl {
        data: core::mem::size_of::<MinData>() as u32,
        methods: Vec::new(),
        overrides: alloc::vec![("wpOpen", __m_min_open as usize), ("wpQueryIcon", __m_min_icon as usize)],
    }
}

pub fn impl_viewer() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpPopulate", __m_viewer_populate as usize)] }
}
