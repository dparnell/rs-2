//! Events and menus: ODDispatcher with its dispatch modules (OS/2's
//! ODEvent is a QMSG), the standard dispatch module (mouse to the facet
//! under the pointer, keys to the frame with the key focus, menu commands
//! to the frame with the menu focus, a mouse click activating an inactive
//! frame), the active frame's border, and ODMenuBar as a real PM menu on
//! the window's frame.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::layout;
use crate::obj::{acquire, release};
use crate::rt::*;
use crate::services::{self, sess, session, win, wstate};
use crate::storage::new_of;
use crate::*;

unsafe extern "C" {
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: usize, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32,
                       owner: u32, behind: u32, id: u32, ctl: usize, pres: usize) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(hwnd: u32) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(hwnd: u32, id: u32) -> u32;
    #[link_name = "PMWIN.937"]
    fn WinPopupMenu(parent: u32, owner: u32, menu: u32, x: i32, y: i32, id: i32, fs: u32) -> u32;
    #[link_name = "PMWIN.823"]
    fn WinQueryPointerPos(desktop: u32, pt: *mut [i32; 2]) -> u32;
    #[link_name = "PMWIN.788"]
    fn WinMapWindowPoints(from: u32, to: u32, pts: *mut [i32; 2], n: i32) -> u32;
    #[link_name = "PMWIN.765"]
    fn WinInvalidateRect(hwnd: u32, rect: usize, children: u32) -> u32;
    #[link_name = "PMGPI.592"]
    fn GpiCreateLogColorTable(hps: u32, opts: u32, fmt: i32, start: i32, n: i32, table: *const i32) -> u32;
    #[link_name = "PMGPI.517"]
    fn GpiSetColor(hps: u32, color: i32) -> u32;
    #[link_name = "PMGPI.404"]
    fn GpiMove(hps: u32, pt: *const [i32; 2]) -> u32;
    #[link_name = "PMGPI.520"]
    fn GpiSetDefaultViewMatrix(hps: u32, n: i32, m: *const i32, ty: i32) -> u32;
    #[link_name = "PMGPI.501"]
    fn GpiSavePS(hps: u32) -> i32;
    #[link_name = "PMGPI.499"]
    fn GpiRestorePS(hps: u32, id: i32) -> u32;
    #[link_name = "PMGPI.356"]
    fn GpiBox(hps: u32, ctl: i32, pt: *const [i32; 2], h: i32, v: i32) -> i32;
}

pub const WM_ACTIVATE: u32 = 0x0d;
pub const WM_COMMAND: u32 = 0x20;
pub const WM_INITMENU: u32 = 0x33;
pub const WM_MENUSELECT: u32 = 0x34;
pub const WM_MOUSEMOVE: u32 = 0x70;
pub const WM_BUTTON1DOWN: u32 = 0x71;
pub const WM_BUTTON2DOWN: u32 = 0x74;
pub const WM_BUTTON3DOWN: u32 = 0x77;
pub const WM_CHAR: u32 = 0x7a;
pub const WM_CONTEXTMENU: u32 = 0x424;
pub const WM_BEGINDRAG: u32 = 0x420;

fn is_mouse(msg: u32) -> bool {
    (0x70..=0x79).contains(&msg) || (0x410..=0x42f).contains(&msg)
}

fn is_button_down(msg: u32) -> bool {
    msg == WM_BUTTON1DOWN || msg == WM_BUTTON2DOWN || msg == WM_BUTTON3DOWN
}

// ---- ODDispatcher ----

#[derive(Default)]
pub struct DispatcherState {
    pub session: usize,
    /// Modules by event type (a PM message; 0 for every event), consulted
    /// after the standard module.
    pub modules: BTreeMap<u32, Vec<usize>>,
    pub monitors: BTreeMap<u32, Vec<usize>>,
    pub standard: usize,
    pub exit: bool,
}

pub fn disp(this: usize) -> &'static mut DispatcherState {
    state(cd!(ODDispatcherCClassData), this)
}

pub mod disp_ix {
    pub const INIT_DISPATCHER: usize = 0;
    pub const ADD_DISPATCH_MODULE: usize = 1;
    pub const DISPATCH: usize = 6;
    pub const MODULE_DISPATCH: usize = 1;
}

fn module_dispatch(m: usize, ev: usize, event: usize, info: usize) -> bool {
    call(cd!(ODDispatchModuleClassData), disp_ix::MODULE_DISPATCH, &[m, ev, event, info]) & 0xff != 0
}

methods! {
    fn InitDispatcher(this: usize, ev: usize, session: usize) -> usize {
        let d = disp(this);
        d.session = session;
        if d.standard == 0 {
            let m = new_of(cd!(ODDispatchModuleClassData), "ODDispatchModule");
            if m != 0 {
                InitDispatchModule(m, ev, session);
            }
            d.standard = m;
        }
        0
    }
    fn AddDispatchModule(this: usize, ev: usize, ty: usize, m: usize) -> usize {
        disp(this).modules.entry(ty as u32).or_default().push(m);
        0
    }
    fn GetDispatchModule(this: usize, ev: usize, ty: usize) -> usize {
        disp(this).modules.get(&(ty as u32)).and_then(|v| v.last().copied()).unwrap_or(0)
    }
    fn RemoveDispatchModule(this: usize, ev: usize, ty: usize) -> usize {
        disp(this).modules.remove(&(ty as u32));
        0
    }
    fn AddMonitor(this: usize, ev: usize, ty: usize, m: usize) -> usize {
        disp(this).monitors.entry(ty as u32).or_default().push(m);
        0
    }
    fn RemoveMonitor(this: usize, ev: usize, ty: usize, m: usize) -> usize {
        if let Some(v) = disp(this).monitors.get_mut(&(ty as u32)) {
            v.retain(|&x| x != m);
        }
        0
    }
    fn Dispatch(this: usize, ev: usize, event: usize) -> usize {
        if event == 0 {
            return 0;
        }
        let msg = read::<[u32; 8]>(event)[1];
        let info = [0u32; 8];
        let ip = info.as_ptr() as usize;
        let d = disp(this);
        let mons: Vec<usize> = d.monitors.get(&msg).into_iter().chain(d.monitors.get(&0)).flatten().copied().collect();
        for m in mons {
            module_dispatch(m, ev, event, ip);
        }
        let std = disp(this).standard;
        if std != 0 && module_dispatch(std, ev, event, ip) {
            return 1;
        }
        let d = disp(this);
        let mods: Vec<usize> = d.modules.get(&msg).into_iter().chain(d.modules.get(&0)).flatten().copied().collect();
        for m in mods.iter().rev() {
            if module_dispatch(*m, ev, event, ip) {
                return 1;
            }
        }
        0
    }
    fn Exit(this: usize, ev: usize) -> usize {
        disp(this).exit = true;
        0
    }
    fn ShouldExit(this: usize, ev: usize) -> usize { disp(this).exit as usize }
    fn Redispatch(this: usize, ev: usize, event: usize, info: usize) -> usize { Dispatch(this, ev, event) }
    fn DispSomUninit(this: usize) -> usize {
        drop_state::<DispatcherState>(cd!(ODDispatcherCClassData), this);
        call_parent(cd!(ODDispatcherCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
    // ODDispatchModule: the standard module's behaviour is its default.
    fn InitDispatchModule(this: usize, ev: usize, session: usize) -> usize {
        *somsup::data::<usize>(cd!(ODDispatchModuleCClassData), this) = session;
        0
    }
    fn ModuleDispatch(this: usize, ev: usize, event: usize, info: usize) -> usize { std_dispatch(ev, event, info) as usize }
}

pub static DISPATCHER_M: &[(&str, M)] = &[
    ("InitDispatcher", InitDispatcher::t),
    ("AddDispatchModule", AddDispatchModule::t),
    ("GetDispatchModule", GetDispatchModule::t),
    ("RemoveDispatchModule", RemoveDispatchModule::t),
    ("AddMonitor", AddMonitor::t),
    ("RemoveMonitor", RemoveMonitor::t),
    ("Dispatch", Dispatch::t),
    ("Exit", Exit::t),
    ("ShouldExit", ShouldExit::t),
    ("Redispatch", Redispatch::t),
];
pub static DISPATCHER_O: &[(&str, M)] = &[("SOMObject::somUninit", DispSomUninit::t)];
pub static DISPATCH_MODULE_M: &[(&str, M)] =
    &[("InitDispatchModule", InitDispatchModule::t), ("Dispatch", ModuleDispatch::t)];

// ---- The standard dispatch module ----

fn tok(name: &[u8]) -> usize {
    services::tokenize(session(), name)
}

/// The ODWindow whose client window is `hwnd` (with no client: the last
/// window registered, so programs without PM windows can dispatch).
pub fn window_for(hwnd: u32) -> usize {
    let s = session();
    if s == 0 || sess(s).window_state == 0 {
        return 0;
    }
    let ws = wstate(sess(s).window_state);
    ws.windows.iter().copied().find(|&w| win(w).client == hwnd && hwnd != 0)
        .or_else(|| if hwnd == 0 { ws.windows.last().copied() } else { None })
        .unwrap_or(0)
}

/// The deepest facet under a point (window coordinates, fixed point),
/// the last embedded facet (the frontmost) first.
pub fn hit(f: usize, p: Point) -> usize {
    if f == 0 || layout::FcContainsPoint(f, 0, &p as *const Point as usize, 0) & 0xff == 0 {
        return 0;
    }
    for &c in layout::facet(f).children.clone().iter().rev() {
        let h = hit(c, p);
        if h != 0 {
            return h;
        }
    }
    f
}

fn owner(focus: &[u8]) -> usize {
    let s = session();
    let a = if s == 0 { 0 } else { sess(s).arbitrator };
    if a == 0 { 0 } else { services::focus_owner(a, tok(focus)) }
}

fn first_facet(frame: usize) -> usize {
    if frame == 0 { 0 } else { layout::frame(frame).facets.first().copied().unwrap_or(0) }
}

/// Make a frame the active one: the selection, menu and key foci (the
/// previous owner's part commits to giving them up), each announced to
/// the frame's part with FocusAcquired, as an OpenDoc part activating
/// itself does.
pub fn activate(ev: usize, frame: usize) {
    let s = session();
    if s == 0 || frame == 0 {
        return;
    }
    let a = sess(s).arbitrator;
    let before = menu_shown();
    let part = layout::FAcquirePart(frame, ev);
    for f in [&b"Selection"[..], b"Menu", b"Key"] {
        let t = tok(f);
        if services::focus_owner(a, t) == frame {
            continue;
        }
        services::grant(a, ev, t, frame);
        if part != 0 {
            call(cd!(ODPartClassData), 28, &[part, ev, t, frame]);
        }
    }
    release(part, ev);
    // A part that shows no menu bar of its own gets the window's base one.
    if menu_shown() == before {
        let ws = sess(s).window_state;
        let base = if ws == 0 { 0 } else { wstate(ws).base_menu };
        if base != 0 {
            MDisplay(base, ev);
        }
    }
    invalidate_window(layout::frame(frame).window);
}

fn invalidate_window(w: usize) {
    let w = if w == 0 { window_for(0) } else { w };
    if w != 0 && win(w).client != 0 {
        unsafe { WinInvalidateRect(win(w).client, 0, 1) };
    }
}

fn send(ev: usize, frame: usize, facet: usize, event: usize, info: usize) -> bool {
    if frame == 0 {
        return false;
    }
    let part = layout::FAcquirePart(frame, ev);
    if part == 0 {
        return false;
    }
    let r = call(cd!(ODPartClassData), crate::part::ix::HANDLE_EVENT, &[part, ev, event, frame, facet, info]) & 0xff != 0;
    release(part, ev);
    r
}

/// Send an event to the part of a frame, then up the frame tree while the
/// frames propagate events and the parts do not handle it.
fn send_up(ev: usize, mut frame: usize, mut facet: usize, event: usize, info: usize) -> bool {
    loop {
        if send(ev, frame, facet, event, info) {
            return true;
        }
        if frame == 0 || !layout::frame(frame).propagate {
            return false;
        }
        facet = layout::facet(facet).containing;
        frame = if facet != 0 { layout::facet(facet).frame } else { 0 };
    }
}

pub fn std_dispatch(ev: usize, event: usize, info: usize) -> bool {
    let q = read::<[u32; 8]>(event);
    let (hwnd, msg, mp1) = (q[0], q[1], q[2]);
    let w = window_for(hwnd);
    if w == 0 {
        return false;
    }
    let root_frame = win(w).root_frame;
    let root_facet = win(w).root_facet;
    if is_mouse(msg) {
        let p = Point { x: ((mp1 & 0xffff) as i16 as i32) << 16, y: ((mp1 >> 16) as i16 as i32) << 16 };
        let captured = owner(b"Mouse");
        let facet = if captured != 0 && first_facet(captured) != 0 { first_facet(captured) } else { hit(root_facet, p) };
        if facet == 0 {
            return false;
        }
        let frame = layout::facet(facet).frame;
        if is_button_down(msg) && owner(b"Selection") != frame {
            activate(ev, frame);
        }
        let handled = send_up(ev, frame, facet, event, info);
        if !handled && msg == WM_BEGINDRAG && frame != root_frame {
            crate::dnd::drag_frame(frame, win(w).client);
            return true;
        }
        if !handled && msg == WM_CONTEXTMENU {
            base_popup(ev, w);
            return true;
        }
        return handled;
    }
    match msg {
        WM_CHAR => {
            let f = match owner(b"Key") {
                0 => root_frame,
                f => f,
            };
            send_up(ev, f, first_facet(f), event, info)
        }
        WM_COMMAND | WM_MENUSELECT | WM_INITMENU => {
            let f = match owner(b"Menu") {
                0 => root_frame,
                f => f,
            };
            if msg == WM_INITMENU {
                let part = layout::FAcquirePart(f, ev);
                if part != 0 {
                    call(cd!(ODPartClassData), 33, &[part, ev, f]);
                    release(part, ev);
                }
            }
            send(ev, f, first_facet(f), event, info)
        }
        WM_ACTIVATE => {
            if mp1 & 0xffff != 0 && owner(b"Selection") == 0 {
                activate(ev, root_frame);
            }
            let sel = owner(b"Selection");
            send(ev, root_frame, root_facet, event, info);
            if sel != 0 && sel != root_frame {
                send(ev, sel, first_facet(sel), event, info);
            }
            false
        }
        _ => false,
    }
}

/// The default view matrix back to identity (parts focusing on a facet
/// set it to their content transform).
fn reset_view(hps: u32) {
    let m = [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 1];
    unsafe { GpiSetDefaultViewMatrix(hps, 9, m.as_ptr(), 0) };
}

/// Draw a facet and the facets embedded in it, each with the paint PS as
/// it was (parts change its view matrix, colours and clipping).
pub fn draw_tree(f: usize, ev: usize, hps: u32) {
    let id = unsafe { GpiSavePS(hps) };
    layout::Draw(f, ev, 0, 0);
    if id > 0 {
        unsafe { GpiRestorePS(hps, id) };
    }
    reset_view(hps);
    for c in layout::facet(f).children.clone() {
        draw_tree(c, ev, hps);
    }
}

/// The active frame's border: drawn round its facets when it is not the
/// window's root frame.
pub fn draw_active_border(w: usize, hps: u32) {
    let f = owner(b"Selection");
    if f == 0 || f == win(w).root_frame || layout::frame(f).window != w && layout::frame(f).window != 0 {
        return;
    }
    for fc in layout::frame(f).facets.clone() {
        let r = layout::window_frame_xf(fc).rect(layout::shown_rect(fc));
        let (l, b, rt, t) = (r.left >> 16, r.bottom >> 16, (r.right >> 16) - 1, (r.top >> 16) - 1);
        unsafe {
            GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
            for (i, c) in [(1, 0x40_40_40), (2, 0x90_90_90), (3, 0x40_40_40)] {
                GpiSetColor(hps, c);
                GpiMove(hps, &[l - i, b - i]);
                GpiBox(hps, 2, &[rt + i, t + i], 0, 0);
            }
        }
    }
}

/// A PM message on a document window's client, as an ODEvent: true when
/// a part (or a dispatch module) handled it.
pub fn client_message(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> bool {
    let s = session();
    if s == 0 || sess(s).dispatcher == 0 {
        return false;
    }
    let q = [hwnd, msg, mp1 as u32, mp2 as u32, 0, 0, 0, 0];
    let mut e = somsup::Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 };
    let r = Dispatch(sess(s).dispatcher, &raw mut e as usize, q.as_ptr() as usize) != 0;
    if e.major != 0 {
        unsafe { somsup::somExceptionFree(&raw mut e) };
    }
    r
}

// ---- ODMenuBar: menus of items, shown as the window's PM menu ----

#[derive(Default, Clone)]
pub struct Item {
    pub id: u16,
    pub text: Vec<u8>,
    pub style: u16,
    pub disabled: bool,
    pub checked: bool,
}

#[derive(Default, Clone)]
pub struct Menu {
    pub id: u16,
    pub title: Vec<u8>,
    pub items: Vec<Item>,
}

#[derive(Default, Clone)]
pub struct MenuState {
    pub menus: Vec<Menu>,
    pub displayed: bool,
    /// A popup menu (the window state's base popup and its copies): its
    /// menus are the popup's items (those with items of their own are
    /// submenus), shown at the pointer.
    pub popup: bool,
}

pub fn menu(this: usize) -> &'static mut MenuState {
    state(cd!(ODMenuBarCClassData), this)
}

/// The menu bar shown now, and on which frame window.
static mut SHOWN: usize = 0;
static mut SHOWN_ON: u32 = 0;
static mut SHOW_COUNT: u32 = 0;

fn menu_shown() -> u32 {
    unsafe { SHOW_COUNT }
}

pub fn shown_menu_bar() -> usize {
    unsafe { SHOWN }
}

const MIS_TEXT: u16 = 0x1;
const MIS_SEPARATOR: u16 = 0x4;
const MIS_SUBMENU: u16 = 0x10;
const MIA_CHECKED: u16 = 0x2000;
const MIA_DISABLED: u16 = 0x4000;
const MM_INSERTITEM: u32 = 0x180;
const MM_SETITEMTEXT: u32 = 0x18e;
const MM_SETITEMATTR: u32 = 0x192;
const FID_MENU: u32 = 0x8005;
const WC_MENU: usize = 0xffff_0004;
const WM_UPDATEFRAME: u32 = 0x42;

/// MENUITEM.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MenuItem {
    pub pos: i16,
    pub style: u16,
    pub attr: u16,
    pub id: u16,
    pub submenu: u32,
    pub handle: u32,
}

fn insert_items(menu: u32, items: &[Item]) {
    for it in items {
        let style = if it.style & MIS_SEPARATOR != 0 { it.style } else { it.style | MIS_TEXT };
        let attr = if it.disabled { MIA_DISABLED } else { 0 } | if it.checked { MIA_CHECKED } else { 0 };
        let mi = MenuItem { pos: -1, style, attr, id: it.id, submenu: 0, handle: 0 };
        let t = z(&it.text);
        unsafe { WinSendMsg(menu, MM_INSERTITEM, &mi as *const MenuItem as usize, t.as_ptr() as usize) };
    }
}

/// Show a popup menu bar at the pointer over a window's client.
fn popup_pm(this: usize, client: u32) {
    unsafe {
        let pm = WinCreateWindow(1, WC_MENU, b"\0".as_ptr(), 0, 0, 0, 0, 0, client, 3, 0x7fff, 0, 0);
        if pm == 0 {
            return;
        }
        for m in menu(this).menus.clone() {
            if m.items.is_empty() {
                // An item of the popup itself.
                let style = if m.title.is_empty() { MIS_SEPARATOR } else { MIS_TEXT };
                let mi = MenuItem { pos: -1, style, attr: 0, id: m.id, submenu: 0, handle: 0 };
                let t = z(&m.title);
                WinSendMsg(pm, MM_INSERTITEM, &mi as *const MenuItem as usize, t.as_ptr() as usize);
            } else {
                let sub = WinCreateWindow(2, WC_MENU, b"\0".as_ptr(), 0, 0, 0, 0, 0, pm, 3, m.id as u32, 0, 0);
                insert_items(sub, &m.items);
                let mi = MenuItem { pos: -1, style: MIS_TEXT | MIS_SUBMENU, attr: 0, id: m.id, submenu: sub, handle: 0 };
                let t = z(&m.title);
                WinSendMsg(pm, MM_INSERTITEM, &mi as *const MenuItem as usize, t.as_ptr() as usize);
            }
        }
        let mut pt = [0i32; 2];
        WinQueryPointerPos(1, &mut pt);
        WinMapWindowPoints(1, client, &mut pt, 1);
        // PU_HCONSTRAIN | PU_VCONSTRAIN | PU_KEYBOARD | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2
        WinPopupMenu(client, client, pm, pt[0], pt[1], 0, 0x2000 | 0x4000 | 0x0200 | 0x0040 | 0x0080);
    }
}

/// Show the window state's base popup for a frame whose part shows none.
pub fn base_popup(ev: usize, w: usize) {
    let s = session();
    let ws = if s == 0 { 0 } else { sess(s).window_state };
    let p = if ws == 0 { 0 } else { wstate(ws).base_popup };
    if p != 0 && w != 0 && win(w).client != 0 {
        popup_pm(p, win(w).client);
    }
}

fn build_pm(this: usize, frame: u32) {
    unsafe {
        let old = WinWindowFromID(frame, FID_MENU);
        if old != 0 {
            WinDestroyWindow(old);
        }
        let bar = WinCreateWindow(frame, WC_MENU, b"\0".as_ptr(), 0x8000_0001, 0, 0, 0, 0, frame, 3, FID_MENU, 0, 0);
        if bar == 0 {
            return;
        }
        SHAPE = shape(this);
        for m in menu(this).menus.clone() {
            let sub = WinCreateWindow(2, WC_MENU, b"\0".as_ptr(), 0, 0, 0, 0, 0, bar, 3, m.id as u32, 0, 0);
            for it in &m.items {
                let style = if it.style & MIS_SEPARATOR != 0 { it.style } else { it.style | MIS_TEXT };
                let attr = if it.disabled { MIA_DISABLED } else { 0 } | if it.checked { MIA_CHECKED } else { 0 };
                let mi = MenuItem { pos: -1, style, attr, id: it.id, submenu: 0, handle: 0 };
                let t = z(&it.text);
                WinSendMsg(sub, MM_INSERTITEM, &mi as *const MenuItem as usize, t.as_ptr() as usize);
            }
            let mi = MenuItem { pos: -1, style: MIS_TEXT | MIS_SUBMENU, attr: 0, id: m.id, submenu: sub, handle: 0 };
            let t = z(&m.title);
            WinSendMsg(bar, MM_INSERTITEM, &mi as *const MenuItem as usize, t.as_ptr() as usize);
        }
        WinSendMsg(frame, WM_UPDATEFRAME, 0x4, 0); // FCF_MENU
    }
}

/// The menus and their items' IDs, as last built into PM menus.
static mut SHAPE: Vec<(u16, Vec<u16>)> = Vec::new();

fn shape(this: usize) -> Vec<(u16, Vec<u16>)> {
    menu(this).menus.iter().map(|m| (m.id, m.items.iter().map(|i| i.id).collect())).collect()
}

/// Show the menu bar again if it is the one on the screen. When only
/// items' states or texts changed (AdjustMenus while PM opens a menu:
/// WM_INITMENU), the PM menus are updated in place, so the menu being
/// opened stays open.
fn refresh(this: usize) {
    unsafe {
        if SHOWN != this || SHOWN_ON == 0 {
            return;
        }
        let bar = WinWindowFromID(SHOWN_ON, FID_MENU);
        if bar == 0 || shape(this) != *(&raw const SHAPE) {
            build_pm(this, SHOWN_ON);
            return;
        }
        for m in &menu(this).menus {
            for it in &m.items {
                let attr = if it.disabled { MIA_DISABLED } else { 0 } | if it.checked { MIA_CHECKED } else { 0 };
                let mask = (MIA_DISABLED | MIA_CHECKED) as usize;
                WinSendMsg(bar, MM_SETITEMATTR, it.id as usize | 1 << 16, mask | (attr as usize) << 16);
                if it.style & MIS_SEPARATOR == 0 && !it.text.is_empty() {
                    let t = z(&it.text);
                    WinSendMsg(bar, MM_SETITEMTEXT, it.id as usize, t.as_ptr() as usize);
                }
            }
        }
    }
}

fn find_item(this: usize, m: usize, i: usize) -> Option<&'static mut Item> {
    menu(this).menus.iter_mut().find(|x| x.id as usize == m)?.items.iter_mut().find(|x| x.id as usize == i)
}

fn title_of(platform: usize) -> Vec<u8> {
    // rs2: a menu's title string where IBM's takes a PM menu window.
    if platform >= 0x10000 { cstr(platform).to_vec() } else { Vec::new() }
}

/// The ID of an item being added: the MENUITEM's (OS/2's
/// AddMenuItemLast/Before take the menu, the submenu item to add to (0:
/// the menu itself; rs2 adds to the menu either way) and the MENUITEM,
/// as IBM's MMODPTS calls them), else the ID argument.
fn item_id(i: usize, mi: usize) -> usize {
    let m: MenuItem = if mi != 0 { read(mi) } else { MenuItem::default() };
    if m.id != 0 { m.id as usize } else { i }
}

fn new_item(id: usize, mi: usize) -> Item {
    let m: MenuItem = if mi != 0 { read(mi) } else { MenuItem::default() };
    Item { id: id as u16, text: Vec::new(), style: m.style, disabled: m.attr & MIA_DISABLED != 0, checked: m.attr & MIA_CHECKED != 0 }
}

methods! {
    fn MDisplay(this: usize, ev: usize) -> usize {
        if menu(this).popup {
            let f = owner(b"Menu");
            let w = if f != 0 && layout::frame(f).window != 0 { layout::frame(f).window } else { window_for(0) };
            if w != 0 && win(w).client != 0 {
                popup_pm(this, win(w).client);
            }
            return 0;
        }
        let w = window_for(0);
        let w = {
            // The window of the frame with the menu focus, else the last one.
            let f = owner(b"Menu");
            let fw = if f != 0 { layout::frame(f).window } else { 0 };
            if fw != 0 { fw } else { w }
        };
        let frame = if w != 0 { win(w).hwnd } else { 0 };
        unsafe {
            SHOWN = this;
            SHOWN_ON = frame;
            SHOW_COUNT += 1;
        }
        menu(this).displayed = true;
        if frame != 0 {
            build_pm(this, frame);
        }
        0
    }
    fn MCopy(this: usize, ev: usize) -> usize {
        let m = new_of(cd!(ODMenuBarClassData), "ODMenuBar");
        if m != 0 {
            *menu(m) = menu(this).clone();
            menu(m).displayed = false;
        }
        m
    }
    fn AddMenuLast(this: usize, ev: usize, id: usize, platform: usize, part: usize) -> usize {
        let id = id & 0xffff;
        menu(this).menus.push(Menu { id: id as u16, title: title_of(platform), items: Vec::new() });
        refresh(this);
        0
    }
    fn AddMenuBefore(this: usize, ev: usize, id: usize, platform: usize, part: usize, before: usize) -> usize {
        let id = id & 0xffff;
        let before = before & 0xffff;
        let ms = &mut menu(this).menus;
        let at = ms.iter().position(|m| m.id as usize == before).unwrap_or(ms.len());
        ms.insert(at, Menu { id: id as u16, title: title_of(platform), items: Vec::new() });
        refresh(this);
        0
    }
    fn RemoveMenu(this: usize, ev: usize, id: usize) -> usize {
        let id = id & 0xffff;
        menu(this).menus.retain(|m| m.id as usize != id);
        refresh(this);
        0
    }
    fn GetMenu(this: usize, ev: usize, id: usize) -> usize {
        let id = id & 0xffff; menu(this).menus.iter().any(|m| m.id as usize == id) as usize }
    fn MIsValid(this: usize, ev: usize) -> usize { 1 }
    fn SetMenuItemText(this: usize, ev: usize, m: usize, i: usize, t: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        if m == 0 {
            if let Some(x) = menu(this).menus.iter_mut().find(|x| x.id as usize == i) {
                x.title = cstr(t).to_vec();
            }
        } else if let Some(x) = find_item(this, m, i) {
            x.text = cstr(t).to_vec();
        } else {
            return 0;
        }
        refresh(this);
        1
    }
    fn GetMenuItemText(this: usize, ev: usize, m: usize, i: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        if m == 0 {
            return dup(menu(this).menus.iter().find(|x| x.id as usize == i).map_or(&b""[..], |x| &x.title));
        }
        dup(find_item(this, m, i).map_or(&b""[..], |x| &x.text))
    }
    fn AddSubMenuLast(this: usize, ev: usize, m: usize, sub: usize, platform: usize) -> usize {
        let m = m & 0xffff; 0 }
    fn AddSubMenuBefore(this: usize, ev: usize, m: usize, sub: usize, platform: usize, before: usize) -> usize {
        let m = m & 0xffff;
        let before = before & 0xffff; 0 }
    fn CheckMenuItem(this: usize, ev: usize, m: usize, i: usize, b: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        match find_item(this, m, i) {
            Some(x) => x.checked = b & 0xff != 0,
            None => return 0,
        }
        refresh(this);
        1
    }
    fn EnableMenuItem(this: usize, ev: usize, m: usize, i: usize, b: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        match find_item(this, m, i) {
            Some(x) => x.disabled = b & 0xff == 0,
            None => return 0,
        }
        refresh(this);
        1
    }
    fn AddMenuItemBefore(this: usize, ev: usize, m: usize, i: usize, mi: usize, before: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        let before = before & 0xffff;
        let i = item_id(i, mi);
        if m == 0 {
            let ms = &mut menu(this).menus;
            let at = ms.iter().position(|x| x.id as usize == before).unwrap_or(ms.len());
            ms.insert(at, Menu { id: i as u16, title: Vec::new(), items: Vec::new() });
        } else {
            let Some(mm) = menu(this).menus.iter_mut().find(|x| x.id as usize == m) else { return 0 };
            let at = mm.items.iter().position(|x| x.id as usize == before).unwrap_or(mm.items.len());
            mm.items.insert(at, new_item(i, mi));
        }
        refresh(this);
        1
    }
    fn AddMenuItemLast(this: usize, ev: usize, m: usize, i: usize, mi: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        let i = item_id(i, mi);
        if m == 0 {
            menu(this).menus.push(Menu { id: i as u16, title: Vec::new(), items: Vec::new() });
        } else {
            let Some(mm) = menu(this).menus.iter_mut().find(|x| x.id as usize == m) else { return 0 };
            mm.items.push(new_item(i, mi));
        }
        refresh(this);
        1
    }
    fn GetMenuItem(this: usize, ev: usize, m: usize, i: usize, out: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        match find_item(this, m, i) {
            Some(x) => {
                let attr = if x.disabled { MIA_DISABLED } else { 0 } | if x.checked { MIA_CHECKED } else { 0 };
                write(out, MenuItem { pos: 0, style: x.style, attr, id: x.id, submenu: 0, handle: 0 });
                1
            }
            None => 0,
        }
    }
    fn MenuItemExists(this: usize, ev: usize, m: usize, i: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        if m == 0 {
            return menu(this).menus.iter().any(|x| x.id as usize == i) as usize;
        }
        find_item(this, m, i).is_some() as usize
    }
    fn IsItemEnabled(this: usize, ev: usize, m: usize, i: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff; find_item(this, m, i).is_some_and(|x| !x.disabled) as usize }
    fn IsItemChecked(this: usize, ev: usize, m: usize, i: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff; find_item(this, m, i).is_some_and(|x| x.checked) as usize }
    fn GetPlatformMenuBar(this: usize, ev: usize) -> usize {
        unsafe { if SHOWN == this && SHOWN_ON != 0 { WinWindowFromID(SHOWN_ON, FID_MENU) as usize } else { 0 } }
    }
    fn ModifyMenuItemAttributes(this: usize, ev: usize, m: usize, i: usize, mask: usize, attr: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        match find_item(this, m, i) {
            Some(x) => {
                if mask as u16 & MIA_DISABLED != 0 {
                    x.disabled = attr as u16 & MIA_DISABLED != 0;
                }
                if mask as u16 & MIA_CHECKED != 0 {
                    x.checked = attr as u16 & MIA_CHECKED != 0;
                }
            }
            None => return 0,
        }
        refresh(this);
        1
    }
    fn RemoveMenuItem(this: usize, ev: usize, m: usize, i: usize) -> usize {
        let m = m & 0xffff;
        let i = i & 0xffff;
        if m == 0 {
            menu(this).menus.retain(|x| x.id as usize != i);
        } else if let Some(mm) = menu(this).menus.iter_mut().find(|x| x.id as usize == m) {
            mm.items.retain(|x| x.id as usize != i);
        }
        refresh(this);
        1
    }
    fn InitMenuBar(this: usize, ev: usize, session: usize, platform: usize) -> usize { 0 }
    fn MSomUninit(this: usize) -> usize {
        unsafe {
            if SHOWN == this {
                SHOWN = 0;
            }
        }
        drop_state::<MenuState>(cd!(ODMenuBarCClassData), this);
        call_parent(cd!(ODMenuBarCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static MENU_BAR_M: &[(&str, M)] = &[
    ("Display", MDisplay::t),
    ("Copy", MCopy::t),
    ("AddMenuLast", AddMenuLast::t),
    ("AddMenuBefore", AddMenuBefore::t),
    ("RemoveMenu", RemoveMenu::t),
    ("GetMenu", GetMenu::t),
    ("IsValid", MIsValid::t),
    ("SetMenuItemText", SetMenuItemText::t),
    ("GetMenuItemText", GetMenuItemText::t),
    ("AddSubMenuLast", AddSubMenuLast::t),
    ("AddSubMenuBefore", AddSubMenuBefore::t),
    ("CheckMenuItem", CheckMenuItem::t),
    ("EnableMenuItem", EnableMenuItem::t),
    ("AddMenuItemBefore", AddMenuItemBefore::t),
    ("AddMenuItemLast", AddMenuItemLast::t),
    ("GetMenuItem", GetMenuItem::t),
    ("MenuItemExists", MenuItemExists::t),
    ("IsItemEnabled", IsItemEnabled::t),
    ("IsItemChecked", IsItemChecked::t),
    ("GetPlatformMenuBar", GetPlatformMenuBar::t),
    ("ModifyMenuItemAttributes", ModifyMenuItemAttributes::t),
    ("RemoveMenuItem", RemoveMenuItem::t),
    ("InitMenuBar", InitMenuBar::t),
];
pub static MENU_BAR_O: &[(&str, M)] = &[("SOMObject::somUninit", MSomUninit::t)];

#[allow(dead_code)]
fn unused() -> usize {
    acquire(0, 0)
}
