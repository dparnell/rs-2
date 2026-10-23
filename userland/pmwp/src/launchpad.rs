//! The LaunchPad (WPLaunchPad, `<WP_LAUNCHPAD>`): a bar of buttons, as
//! on OS/2 Warp 4's desktop. Action buttons (Window list, Find, Shut
//! down: as text, small or normal icons, or none) and the objects put on
//! it; above each object a drawer (another list of objects) opens from
//! its arrow. Clicking an object opens it; the right mouse button gives
//! Open, Remove from LaunchPad and the LaunchPad's settings; objects
//! dropped on it, on an arrow or into an open drawer join that list;
//! dragging the bar's empty space moves it.
//!
//! The setup strings are IBM's (wps1.inf): FPOBJECTS (objects added to
//! the bar), DRAWEROBJECTS (a drawer's number, then its objects),
//! LPCLOSEDRAWER, LPACTIONSTYLE (TEXT, OFF, MINI, NORMAL), LPVERTICAL,
//! LPTEXT, LPDRAWERTEXT, LPSMALLICONS, LPHIDECTLS, LPFLOAT; the settings
//! notebook has IBM's two LaunchPad pages (wpAddLaunchPadPage1, 2). The
//! lists are kept as object handles.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_DEFAULT, OPEN_SETTINGS};
use crate::pm::*;
use crate::som;
use crate::util::{cstr, parse_dec, z};
use crate::wps::{self, Impl};

// ---- The object ----

/// Flags (0: IBM's defaults): keep drawers open, vertical, titles on the
/// bar, no titles in drawers, small icons, show the frame controls, do
/// not float.
const F_KEEPDRAWER: u32 = 1;
const F_VERTICAL: u32 = 2;
const F_TEXT: u32 = 4;
const F_NODRAWERTEXT: u32 = 8;
const F_SMALL: u32 = 16;
const F_SHOWCTLS: u32 = 32;
const F_NOFLOAT: u32 = 64;
/// Action button styles.
const ST_TEXT: u32 = 0;
const ST_OFF: u32 = 1;
const ST_MINI: u32 = 2;
const ST_NORMAL: u32 = 3;

#[repr(C)]
pub struct LpData {
    /// The lists of object handles: 0 the bar, n the drawer above its
    /// n-th object.
    lists: Option<Box<Vec<Vec<u32>>>>,
    flags: u32,
    style: u32,
}

fn ld<'a>(obj: usize) -> &'a mut LpData {
    som::data(wps::ccd("WPLaunchPad"), obj)
}

fn lists<'a>(obj: usize) -> &'a mut Vec<Vec<u32>> {
    ld(obj).lists.get_or_insert_with(|| Box::new(alloc::vec![Vec::new()]))
}

fn list(obj: usize, n: usize) -> Vec<u32> {
    lists(obj).get(n).cloned().unwrap_or_default()
}

fn add_to(obj: usize, n: usize, handles: &[u32]) {
    let l = lists(obj);
    while l.len() <= n {
        l.push(Vec::new());
    }
    for &h in handles {
        if h != 0 && !l[n].contains(&h) {
            l[n].push(h);
        }
    }
}

fn changed(obj: usize) {
    obj::call(m::WPObject::wpSaveDeferred, &[obj]);
    rebuild(obj);
}

fn yes(v: &[u8]) -> bool {
    v.eq_ignore_ascii_case(b"YES")
}

/// The handles of a comma-separated list of object IDs and paths.
fn handles(v: &[u8]) -> Vec<u32> {
    v.split(|&c| c == b',')
        .map(|s| s.trim_ascii())
        .filter(|s| !s.is_empty())
        .map(|s| wps::object_from_location(s))
        .filter(|&o| o != 0)
        .map(obj::handle)
        .collect()
}

extern "C" fn lp_setup(o: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpSetup, &[o, setup as usize]);
    let d = ld(o);
    let flag = |d: &mut LpData, bit: u32, on: bool| d.flags = if on { d.flags | bit } else { d.flags & !bit };
    for (k, v) in obj::setup_pairs(cstr(setup)) {
        match k.as_slice() {
            b"FPOBJECTS" => add_to(o, 0, &handles(&v)),
            b"DRAWEROBJECTS" => {
                let (n, rest) = match v.iter().position(|&c| c == b',') {
                    Some(i) => (&v[..i], &v[i + 1..]),
                    None => (&v[..], &b""[..]),
                };
                if let Some(n) = parse_dec(n.trim_ascii()) {
                    add_to(o, n as usize, &handles(rest));
                }
            }
            b"LPCLOSEDRAWER" => flag(d, F_KEEPDRAWER, !yes(&v)),
            b"LPVERTICAL" => flag(d, F_VERTICAL, yes(&v)),
            b"LPTEXT" => flag(d, F_TEXT, yes(&v)),
            b"LPDRAWERTEXT" => flag(d, F_NODRAWERTEXT, !yes(&v)),
            b"LPSMALLICONS" => flag(d, F_SMALL, yes(&v)),
            b"LPHIDECTLS" => flag(d, F_SHOWCTLS, !yes(&v)),
            b"LPFLOAT" => flag(d, F_NOFLOAT, !yes(&v)),
            b"LPACTIONSTYLE" => {
                d.style = match v.to_ascii_uppercase().as_slice() {
                    b"OFF" => ST_OFF,
                    b"MINI" => ST_MINI,
                    b"NORMAL" => ST_NORMAL,
                    _ => ST_TEXT,
                }
            }
            _ => {}
        }
    }
    rebuild(o);
    1
}

extern "C" fn lp_save_state(o: usize) -> u32 {
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[o]);
    let d = ld(o);
    obj::wpSaveLong(o, b"WPLaunchPad\0".as_ptr(), 1, d.flags | d.style << 16);
    // "n:h,h;n:h,...", decimal handles.
    let mut s = Vec::new();
    for (n, l) in lists(o).iter().enumerate() {
        if l.is_empty() {
            continue;
        }
        s.extend_from_slice(alloc::format!("{n}:").as_bytes());
        for (i, h) in l.iter().enumerate() {
            if i > 0 {
                s.push(b',');
            }
            s.extend_from_slice(alloc::format!("{h}").as_bytes());
        }
        s.push(b';');
    }
    let zs = z(&s);
    obj::wpSaveString(o, b"WPLaunchPad\0".as_ptr(), 2, zs.as_ptr());
    1
}

extern "C" fn lp_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    if let Some(v) = obj::restored_long(o, b"WPLaunchPad", 1) {
        ld(o).flags = v & 0xffff;
        ld(o).style = v >> 16;
    }
    if let Some(s) = obj::restored_string(o, b"WPLaunchPad", 2) {
        let s = s.strip_suffix(&[0]).unwrap_or(&s);
        for part in s.split(|&c| c == b';').filter(|p| !p.is_empty()) {
            let Some(i) = part.iter().position(|&c| c == b':') else { continue };
            let Some(n) = parse_dec(&part[..i]) else { continue };
            let hs: Vec<u32> = part[i + 1..].split(|&c| c == b',').filter_map(parse_dec).collect();
            add_to(o, n as usize, &hs);
        }
    }
    1
}

extern "C" fn lp_uninit(o: usize) {
    ld(o).lists = None;
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[o]);
}

extern "C" fn lp_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32;
    }
    open_bar(o)
}

// ---- The windows ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Action(usize),
    Object(usize),
    Arrow(usize),
}

struct LpWin {
    obj: usize,
    frame: u32,
    client: u32,
    /// 0: the bar; n: drawer n (above the bar's n-th object).
    drawer: usize,
    items: Vec<(RectL, Hit)>,
    pressed: Option<Hit>,
    /// The object the pop-up menu is about.
    menu_on: Option<usize>,
}

static mut WINS: Vec<LpWin> = Vec::new();

#[allow(static_mut_refs)]
fn wins() -> &'static mut Vec<LpWin> {
    unsafe { &mut WINS }
}

const CLASS: &[u8] = b"WPS_LaunchPad\0";
const TID_FLOAT: u32 = 0x5704;
const FCF_BORDER: u32 = 0x0200;
const DP_MINI: u32 = 0x0004;
const DT_CENTER: u32 = 0x0100;
const DT_VCENTER: u32 = 0x0400;
const ARROW: i32 = 10;
const ACTIONS: [&[u8]; 3] = [b"Window list", b"Find", b"Shut down"];
const CMD_OPEN: u32 = 1;
const CMD_REMOVE: u32 = 2;
const CMD_SETTINGS: u32 = 3;
const CMD_CLOSE: u32 = 4;

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.732"]
    fn WinDrawPointer(hps: u32, x: i32, y: i32, h: u32, fs: u32) -> u32;
    #[link_name = "PMWIN.890"]
    fn WinTrackRect(h: u32, hps: u32, pti: *mut i32) -> u32;
}

fn hab() -> u32 {
    unsafe { WinQueryAnchorBlock(HWND_DESKTOP) }
}

fn rect(l: i32, b: i32, r: i32, t: i32) -> RectL {
    RectL { l, b, r, t }
}

/// The objects of a list (the ones still there).
fn objects(o: usize, n: usize) -> Vec<usize> {
    list(o, n).into_iter().map(wps::object_from_handle).filter(|&x| x != 0).collect()
}

fn icon_px(o: usize) -> i32 {
    if ld(o).flags & F_SMALL != 0 { 16 } else { 32 }
}

/// Lay out the bar: its items and its size.
fn layout_bar(o: usize) -> (Vec<(RectL, Hit)>, i32, i32) {
    let d = ld(o);
    let vertical = d.flags & F_VERTICAL != 0;
    let text = d.flags & F_TEXT != 0;
    let icon = icon_px(o);
    let bw = if text { (icon + 12).max(72) } else { icon + 12 };
    let bh = icon + 10 + if text { 14 } else { 0 };
    let mut items = Vec::new();
    let objs = objects(o, 0);
    // The action buttons' block.
    let (aw, ah) = match d.style {
        ST_OFF => (0, 0),
        ST_TEXT => (100, 3 * 20),
        ST_MINI => (26, 3 * 22),
        _ => (3 * 44, 44),
    };
    let (mut w, mut h);
    if !vertical {
        h = (bh + ARROW).max(ah) + 8;
        let mut x = 4;
        if d.style != ST_OFF {
            if d.style == ST_NORMAL {
                for i in 0..3 {
                    items.push((rect(x + i as i32 * 44, 4, x + i as i32 * 44 + 42, h - 4), Hit::Action(i)));
                }
            } else {
                let rh = (h - 8) / 3;
                for i in 0..3 {
                    let t = h - 4 - i as i32 * rh;
                    items.push((rect(x, t - rh + 1, x + aw, t), Hit::Action(i)));
                }
            }
            x += aw + 6;
        }
        for i in 0..objs.len() {
            items.push((rect(x, 4 + bh, x + bw, 4 + bh + ARROW), Hit::Arrow(i)));
            items.push((rect(x, 4, x + bw, 4 + bh), Hit::Object(i)));
            x += bw + 2;
        }
        w = x + 2;
    } else {
        w = (bw + ARROW).max(aw) + 8;
        let mut y_top;
        let actions_h = match d.style {
            ST_OFF => 0,
            ST_NORMAL => 44 + 6,
            _ => ah + 6,
        };
        h = 4 + actions_h + objs.len() as i32 * (bh + 2) + 4;
        h = h.max(40);
        y_top = h - 4;
        if d.style != ST_OFF {
            if d.style == ST_NORMAL {
                for i in 0..3 {
                    items.push((rect(4 + i as i32 * 44, y_top - 42, 4 + i as i32 * 44 + 42, y_top), Hit::Action(i)));
                }
                w = w.max(3 * 44 + 8);
                y_top -= 44 + 6;
            } else {
                let rh = ah / 3;
                for i in 0..3 {
                    items.push((rect(4, y_top - rh * (i as i32 + 1) + 1, 4 + aw.max(w - 8), y_top - rh * i as i32), Hit::Action(i)));
                }
                y_top -= ah + 6;
            }
        }
        for i in 0..objs.len() {
            items.push((rect(4, y_top - bh, 4 + bw, y_top), Hit::Object(i)));
            items.push((rect(4 + bw, y_top - bh, 4 + bw + ARROW, y_top), Hit::Arrow(i)));
            y_top -= bh + 2;
        }
    }
    if objs.is_empty() && d.style == ST_OFF {
        w = w.max(60);
        h = h.max(30);
    }
    (items, w, h)
}

/// Lay out drawer `n`: its items and size.
fn layout_drawer(o: usize, n: usize) -> (Vec<(RectL, Hit)>, i32, i32) {
    let icon = icon_px(o);
    let text = ld(o).flags & F_NODRAWERTEXT == 0;
    let objs = objects(o, n);
    let rh = icon + 8;
    let w = if text { icon + 150 } else { icon + 12 };
    let h = (objs.len() as i32 * rh + 8).max(rh + 8);
    let mut items = Vec::new();
    for i in 0..objs.len() {
        let t = h - 4 - i as i32 * rh;
        items.push((rect(4, t - rh, w - 4, t), Hit::Object(i)));
    }
    (items, w, h)
}

fn find_client(client: u32) -> Option<usize> {
    wins().iter().position(|w| w.client == client)
}

/// A frame of `cw` x `ch` client pixels at (x, y).
fn new_frame(o: usize, title: &[u8], cw: i32, ch: i32, x: i32, y: i32) -> (u32, u32) {
    static mut REGISTERED: bool = false;
    unsafe {
        if !REGISTERED {
            WinRegisterClass(hab(), CLASS.as_ptr(), lp_proc, 0, 0);
            REGISTERED = true;
        }
    }
    let ctls = ld(o).flags & F_SHOWCTLS != 0;
    let mut fcf = FCF_BORDER | if ctls { FCF_TITLEBAR | FCF_SYSMENU } else { 0 };
    let mut client = 0;
    let t = z(title);
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), t.as_ptr(), 0, 0, 1, &mut client) };
    if frame == 0 {
        return (0, 0);
    }
    // The frame's size for that client: what the controls take.
    let (mut fr, mut cr) = (RectL::default(), RectL::default());
    unsafe {
        WinSetWindowPos(frame, 0, 0, 0, 200, 200, SWP_SIZE);
        WinQueryWindowRect(frame, &mut fr);
        WinQueryWindowRect(client, &mut cr);
    }
    let (dw, dh) = ((fr.r - fr.l) - (cr.r - cr.l), (fr.t - fr.b) - (cr.t - cr.b));
    let (sw, sh) = screen();
    let (fw, fh) = (cw + dw, ch + dh);
    let x = x.clamp(0, (sw - fw).max(0));
    let y = y.clamp(0, (sh - fh).max(0));
    unsafe { WinSetWindowPos(frame, HWND_TOP, x, y, fw, fh, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER) };
    (frame, client)
}

/// Open the bar (or bring it up).
fn open_bar(o: usize) -> u32 {
    if let Some(w) = wins().iter().find(|w| w.obj == o && w.drawer == 0) {
        unsafe { WinSetWindowPos(w.frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER) };
        return w.frame;
    }
    let (items, w, h) = layout_bar(o);
    let (sw, _) = screen();
    let (frame, client) = new_frame(o, &obj::title(o), w, h, (sw - w) / 2, 4);
    if frame == 0 {
        return 0;
    }
    wins().push(LpWin { obj: o, frame, client, drawer: 0, items, pressed: None, menu_on: None });
    if ld(o).flags & F_NOFLOAT == 0 {
        unsafe { WinStartTimer(hab(), client, TID_FLOAT, 1000) };
    }
    frame
}

/// Close the LaunchPad's windows and open the bar again (its settings or
/// lists changed).
fn rebuild(o: usize) {
    let open = wins().iter().any(|w| w.obj == o && w.drawer == 0);
    close_all(o);
    if open {
        open_bar(o);
    }
}

fn close_drawers(o: usize) {
    let gone: Vec<u32> = wins().iter().filter(|w| w.obj == o && w.drawer != 0).map(|w| w.frame).collect();
    wins().retain(|w| !(w.obj == o && w.drawer != 0));
    for f in gone {
        unsafe { WinDestroyWindow(f) };
    }
}

fn close_all(o: usize) {
    let gone: Vec<u32> = wins().iter().filter(|w| w.obj == o).map(|w| w.frame).collect();
    wins().retain(|w| w.obj != o);
    for f in gone {
        unsafe { WinDestroyWindow(f) };
    }
}

/// Toggle drawer `n` above the bar's item rectangle `at` (bar client
/// coordinates).
fn toggle_drawer(o: usize, bar: usize, n: usize, at: RectL) {
    if wins().iter().any(|w| w.obj == o && w.drawer == n) {
        close_drawers(o);
        return;
    }
    close_drawers(o);
    let (items, w, h) = layout_drawer(o, n);
    let bw = &wins()[bar];
    let mut p = [PointL { x: at.l, y: at.t }, PointL { x: at.r, y: at.b }];
    unsafe { WinMapWindowPoints(bw.client, HWND_DESKTOP, p.as_mut_ptr(), 2) };
    let vertical = ld(o).flags & F_VERTICAL != 0;
    let (x, y) = if vertical { (p[1].x + 2, p[0].y - h) } else { (p[0].x, p[0].y + 2) };
    let mut title = obj::title(o);
    title.extend_from_slice(b" - drawer");
    let (frame, client) = new_frame(o, &title, w, h, x, y);
    if frame != 0 {
        wins().push(LpWin { obj: o, frame, client, drawer: n, items, pressed: None, menu_on: None });
    }
}

fn hit(w: usize, x: i32, y: i32) -> Option<Hit> {
    wins()[w].items.iter().find(|(r, _)| x >= r.l && x < r.r && y >= r.b && y < r.t).map(|&(_, h)| h)
}

fn item_rect(w: usize, h: Hit) -> RectL {
    wins()[w].items.iter().find(|&&(_, x)| x == h).map_or(RectL::default(), |&(r, _)| r)
}

/// The object an item stands for.
fn item_obj(w: usize, h: Hit) -> usize {
    let lw = &wins()[w];
    match h {
        Hit::Object(i) => objects(lw.obj, lw.drawer).get(i).copied().unwrap_or(0),
        _ => 0,
    }
}

fn act(w: usize, h: Hit) {
    let o = wins()[w].obj;
    match h {
        Hit::Action(0) => {
            post(HWND_DESKTOP, WM_RS2_WINLIST, 0, 0);
        }
        Hit::Action(1) => {
            let d = crate::fs::desktop();
            if d != 0 {
                crate::find::ask(d);
            }
        }
        Hit::Action(_) => crate::shutdown::ask(),
        Hit::Object(_) => {
            let x = item_obj(w, h);
            let in_drawer = wins()[w].drawer != 0;
            if x != 0 {
                obj::call(m::WPObject::wpViewObject, &[x, 0, OPEN_DEFAULT as usize, 0]);
            }
            if in_drawer && ld(o).flags & F_KEEPDRAWER == 0 {
                close_drawers(o);
            }
        }
        Hit::Arrow(i) => {
            let r = item_rect(w, h);
            toggle_drawer(o, w, i + 1, r);
        }
    }
}

/// Remove the n-th object of window `w`'s list (and the drawer above it
/// on the bar).
fn remove(w: usize, i: usize) {
    let (o, n) = (wins()[w].obj, wins()[w].drawer);
    let keep: Vec<u32> = list(o, n);
    let live: Vec<u32> = keep.iter().copied().filter(|&h| wps::object_from_handle(h) != 0).collect();
    let Some(&gone) = live.get(i) else { return };
    let l = lists(o);
    if let Some(v) = l.get_mut(n) {
        v.retain(|&h| h != gone);
    }
    // On the bar, the drawers move along with their objects.
    if n == 0 && i + 1 < l.len() {
        l.remove(i + 1);
    }
    changed(o);
}

fn paint(h: u32) {
    let Some(w) = find_client(h) else { return };
    let mut r = RectL::default();
    unsafe {
        let hps = WinBeginPaint(h, 0, &mut r);
        // LCOLF_RGB
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
        let mut all = RectL::default();
        WinQueryWindowRect(h, &mut all);
        WinFillRect(hps, &all, 0xcccccc);
        let lw = &wins()[w];
        let o = lw.obj;
        let small = ld(o).flags & F_SMALL != 0;
        let text = if lw.drawer == 0 { ld(o).flags & F_TEXT != 0 } else { ld(o).flags & F_NODRAWERTEXT == 0 };
        let objs = objects(o, lw.drawer);
        let style = ld(o).style;
        for &(r, hit) in &lw.items {
            let pressed = lw.pressed == Some(hit);
            let (light, dark) = if pressed { (0x808080, 0xffffff) } else { (0xffffff, 0x808080) };
            let edge = |x0: i32, y0: i32, x1: i32, y1: i32, c: u32| WinFillRect(hps, &rect(x0, y0, x1, y1), c as i32);
            edge(r.l, r.t - 1, r.r, r.t, light);
            edge(r.l, r.b, r.l + 1, r.t, light);
            edge(r.l, r.b, r.r, r.b + 1, dark);
            edge(r.r - 1, r.b, r.r, r.t, dark);
            match hit {
                Hit::Action(i) => {
                    let icon = crate::icons::shell([crate::icons::WINDOW_LIST, crate::icons::FIND, crate::icons::SHUTDOWN][i]);
                    if style == ST_TEXT {
                        let mut tr = rect(r.l + 2, r.b + 1, r.r - 2, r.t - 1);
                        WinDrawText(hps, ACTIONS[i].len() as i32, ACTIONS[i].as_ptr(), &mut tr, 0, -1, DT_CENTER | DT_VCENTER);
                    } else if style == ST_MINI {
                        WinDrawPointer(hps, r.l + (r.r - r.l - 16) / 2, r.b + (r.t - r.b - 16) / 2, icon, DP_MINI);
                    } else {
                        WinDrawPointer(hps, r.l + (r.r - r.l - 32) / 2, r.b + (r.t - r.b - 32) / 2, icon, 0);
                    }
                }
                Hit::Arrow(i) => {
                    // An arrow towards where the drawer opens; brighter
                    // when the drawer has something in it.
                    let full = !list(o, i + 1).is_empty();
                    let c: u32 = if full { 0x000080 } else { 0x909090 };
                    let (cx, cy) = ((r.l + r.r) / 2, (r.b + r.t) / 2);
                    for k in 0..4 {
                        if ld(o).flags & F_VERTICAL != 0 {
                            WinFillRect(hps, &rect(cx - 2 + k, cy - 3 + k, cx - 1 + k, cy + 4 - k), c as i32);
                        } else {
                            WinFillRect(hps, &rect(cx - 3 + k, cy - 2 + k, cx + 4 - k, cy - 1 + k), c as i32);
                        }
                    }
                }
                Hit::Object(i) => {
                    let Some(&x) = objs.get(i) else { continue };
                    let icon = crate::templates::view_icon(x);
                    let px = if small { 16 } else { 32 };
                    let fs = if small { DP_MINI } else { 0 };
                    let mut t: Vec<u8> = obj::title(x).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect();
                    if lw.drawer != 0 {
                        // A drawer's row: the icon, its title beside it.
                        WinDrawPointer(hps, r.l + 4, r.b + (r.t - r.b - px) / 2, icon, fs);
                        if text {
                            let mut tr = rect(r.l + px + 10, r.b, r.r - 2, r.t);
                            WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut tr, 0, -1, DT_VCENTER);
                        }
                    } else {
                        let top = if text { r.t - 4 - px } else { r.b + (r.t - r.b - px) / 2 };
                        WinDrawPointer(hps, r.l + (r.r - r.l - px) / 2, top, icon, fs);
                        if text {
                            t.truncate(12);
                            let mut tr = rect(r.l + 1, r.b + 1, r.r - 1, r.b + 15);
                            WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut tr, 0, -1, DT_CENTER);
                        }
                    }
                }
            }
        }
        WinEndPaint(hps);
    }
}

fn pos(b: u32) -> (i32, i32) {
    ((b & 0xffff) as i16 as i32, (b >> 16) as i16 as i32)
}

/// Move the window by dragging (WinTrackRect, TF_MOVE).
fn track(w: usize) {
    let f = wins()[w].frame;
    let mut r = RectL::default();
    let mut p = [PointL { x: 0, y: 0 }, PointL { x: 0, y: 0 }];
    unsafe {
        WinQueryWindowRect(f, &mut r);
        p[0] = PointL { x: r.l, y: r.b };
        p[1] = PointL { x: r.r, y: r.t };
        WinMapWindowPoints(f, HWND_DESKTOP, p.as_mut_ptr(), 2);
    }
    let (sw, sh) = screen();
    // TRACKINFO: borders, grid, keyboard steps, rclTrack, rclBoundary,
    // minimum and maximum size, fs (TF_MOVE).
    let mut ti: [i32; 19] = [1, 1, 1, 1, 8, 8, p[0].x, p[0].y, p[1].x, p[1].y, 0, 0, sw, sh, 1, 1, sw, sh, 0x000f];
    if unsafe { WinTrackRect(HWND_DESKTOP, 0, ti.as_mut_ptr()) } != 0 {
        unsafe { WinSetWindowPos(f, 0, ti[6], ti[7], 0, 0, SWP_MOVE) };
    }
}

fn context_menu(w: usize, x: i32, y: i32) {
    let h = hit(w, x, y);
    let on = match h {
        Some(Hit::Object(i)) => Some(i),
        _ => None,
    };
    wins()[w].menu_on = on;
    let mut items: Vec<Menu> = Vec::new();
    if on.is_some() {
        items.push(Menu::Item(CMD_OPEN as u16, b"~Open", true));
        items.push(Menu::Item(CMD_REMOVE as u16, b"~Remove from LaunchPad", true));
        items.push(Menu::Sep);
    }
    items.push(Menu::Item(CMD_SETTINGS as u16, b"LaunchPad ~settings", true));
    items.push(Menu::Item(CMD_CLOSE as u16, b"~Close LaunchPad", true));
    let menu = popup_menu(&items);
    if menu != 0 {
        let p = pointer_pos();
        unsafe { WinPopupMenu(HWND_DESKTOP, wins()[w].client, menu, p.x, p.y, 0, PU_HCONSTRAIN | PU_VCONSTRAIN | PU_KEYBOARD | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2) };
    }
}

/// Objects dropped: they join the list under the pointer.
fn dropped(w: usize, pdinfo: u32) -> u32 {
    let list = crate::dnd::items(pdinfo);
    let objs: Vec<usize> = list.iter().map(|&it| crate::dnd::item_object(it)).collect();
    if objs.is_empty() || objs.contains(&0) {
        crate::dnd::finish_drop(pdinfo, &list, false);
        return 0;
    }
    let (o, drawer, client) = (wins()[w].obj, wins()[w].drawer, wins()[w].client);
    // DRAGINFO's drop point (desktop coordinates): over an arrow, into
    // that drawer.
    let mut p = [PointL { x: (rd(pdinfo, 12) & 0xffff) as i16 as i32, y: (rd(pdinfo, 12) >> 16) as i16 as i32 }];
    unsafe { WinMapWindowPoints(HWND_DESKTOP, client, p.as_mut_ptr(), 1) };
    let n = match (drawer, hit(w, p[0].x, p[0].y)) {
        (0, Some(Hit::Arrow(i))) => i + 1,
        (d, _) => d,
    };
    let hs: Vec<u32> = objs.iter().map(|&x| obj::handle(x)).collect();
    add_to(o, n, &hs);
    crate::dnd::finish_drop(pdinfo, &list, true);
    changed(o);
    0
}

extern "C" fn lp_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    let Some(w) = find_client(h) else { return unsafe { WinDefWindowProc(h, msg, a, b) } };
    match msg {
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN => {
            let (x, y) = pos(a);
            match hit(w, x, y) {
                Some(it) => {
                    wins()[w].pressed = Some(it);
                    unsafe {
                        WinSetCapture(HWND_DESKTOP, h);
                        WinInvalidateRect(h, core::ptr::null(), 0);
                    }
                }
                None => track(w),
            }
            1
        }
        WM_BUTTON1UP => {
            let (x, y) = pos(a);
            let was = wins()[w].pressed.take();
            unsafe {
                WinSetCapture(HWND_DESKTOP, 0);
                WinInvalidateRect(h, core::ptr::null(), 0);
            }
            if let Some(it) = was {
                if hit(w, x, y) == Some(it) {
                    act(w, it);
                }
            }
            1
        }
        WM_CONTEXTMENU => {
            let (x, y) = pos(a);
            context_menu(w, x, y);
            1
        }
        WM_COMMAND => {
            let o = wins()[w].obj;
            match a & 0xffff {
                CMD_OPEN => {
                    if let Some(i) = wins()[w].menu_on {
                        let x = item_obj(w, Hit::Object(i));
                        if x != 0 {
                            obj::call(m::WPObject::wpViewObject, &[x, 0, OPEN_DEFAULT as usize, 0]);
                        }
                    }
                }
                CMD_REMOVE => {
                    if let Some(i) = wins()[w].menu_on {
                        remove(w, i);
                    }
                }
                CMD_SETTINGS => {
                    obj::call(m::WPObject::wpViewObject, &[o, 0, OPEN_SETTINGS as usize, 0]);
                }
                CMD_CLOSE => close_all(o),
                _ => {}
            }
            0
        }
        WM_TIMER if a == TID_FLOAT => {
            // Floating on top of the other windows.
            unsafe { WinSetWindowPos(wins()[w].frame, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER) };
            0
        }
        DM_DRAGOVER => {
            let pdinfo = a;
            if unsafe { DrgAccessDraginfo(pdinfo) } == 0 {
                return DOR_NEVERDROP;
            }
            let ok = crate::dnd::items(pdinfo).iter().all(|&it| crate::dnd::item_object(it) != 0);
            unsafe { DrgFreeDraginfo(pdinfo) };
            if ok { mp(DOR_DROP, DO_LINK) } else { mp(DOR_NEVERDROP, 0) }
        }
        DM_DROP => {
            if unsafe { DrgAccessDraginfo(a) } == 0 {
                return 0;
            }
            dropped(w, a)
        }
        WM_CLOSE => {
            let o = wins()[w].obj;
            if wins()[w].drawer == 0 { close_all(o) } else { close_drawers(o) }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

// ---- Settings pages ----

const ID_OPTS: u16 = 580; // .. 586
const ID_STYLE: u16 = 590; // .. 593
/// The first page's check boxes: (text, flag, flag set means checked).
const OPTS: [(&[u8], u32, bool); 7] = [
    (b"Display ~vertically", F_VERTICAL, true),
    (b"~Small icons", F_SMALL, true),
    (b"Show ~titles on the LaunchPad", F_TEXT, true),
    (b"Show titles in ~drawers", F_NODRAWERTEXT, false),
    (b"~Close a drawer after opening an object", F_KEEPDRAWER, false),
    (b"~Float on top", F_NOFLOAT, false),
    (b"Show the title ~bar", F_SHOWCTLS, true),
];

extern "C" fn page1_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let f = ld(b as usize).flags;
            for (i, &(_, bit, set)) in OPTS.iter().enumerate() {
                let on = (f & bit != 0) == set;
                unsafe { WinSendDlgItemMsg(h, (ID_OPTS + i as u16) as u32, BM_SETCHECK, on as u32, 0) };
            }
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let id = (a & 0xffff) as u16;
            if o != 0 && (ID_OPTS..ID_OPTS + OPTS.len() as u16).contains(&id) {
                let (_, bit, set) = OPTS[(id - ID_OPTS) as usize];
                let on = unsafe { WinSendDlgItemMsg(h, id as u32, BM_QUERYCHECK, 0, 0) } != 0;
                let d = ld(o);
                d.flags = if on == set { d.flags | bit } else { d.flags & !bit };
                changed(o);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddLaunchPadPage1(o: usize, nb: u32) -> u32 {
    let mut v = alloc::vec![book::label(b"The LaunchPad:", (6, 106, 200))];
    for (i, &(t, _, _)) in OPTS.iter().enumerate() {
        // BS_AUTOCHECKBOX
        v.push(item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (10, 92 - i as i16 * 12, 220, 10), ID_OPTS + i as u16));
    }
    let tpl = template(book::page_frame(), v);
    let page = book::make_page(nb, page1_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, b"~Options\0")
}

extern "C" fn page2_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe {
                WinSetWindowULong(h, QWL_USER, b);
                WinSendDlgItemMsg(h, (ID_STYLE + ld(b as usize).style.min(3) as u16) as u32, BM_SETCHECK, 1, 0);
            }
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let id = (a & 0xffff) as u16;
            if o != 0 && (ID_STYLE..ID_STYLE + 4).contains(&id) {
                ld(o).style = (id - ID_STYLE) as u32;
                changed(o);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddLaunchPadPage2(o: usize, nb: u32) -> u32 {
    // BS_AUTORADIOBUTTON; the order of the styles' values.
    let radio = |t: &[u8], y: i16, id: u16, first: bool| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (10, y, 200, 10), id);
    let tpl = template(book::page_frame(), alloc::vec![
        book::label(b"Action buttons (Window list, Find, Shut down):", (6, 106, 220)),
        radio(b"As ~text", 92, ID_STYLE, true),
        radio(b"~Off", 80, ID_STYLE + 1, false),
        radio(b"As ~small icons", 68, ID_STYLE + 2, false),
        radio(b"As ~normal icons", 56, ID_STYLE + 3, false),
    ]);
    let page = book::make_page(nb, page2_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, b"~Buttons\0")
}

extern "C" fn lp_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPLaunchPad", m::WPLaunchPad::wpAddLaunchPadPage2, &[o, nb as usize]);
    wps::call("WPLaunchPad", m::WPLaunchPad::wpAddLaunchPadPage1, &[o, nb as usize]);
    1
}

// ---- IBM's query and set methods ----

extern "C" fn wpQueryCloseDrawer(o: usize) -> u32 {
    (ld(o).flags & F_KEEPDRAWER == 0) as u32
}
extern "C" fn wpQueryDisplayVertical(o: usize) -> u32 {
    (ld(o).flags & F_VERTICAL != 0) as u32
}
extern "C" fn wpQueryDisplayText(o: usize) -> u32 {
    (ld(o).flags & F_TEXT != 0) as u32
}
extern "C" fn wpQueryDisplayTextInDrawers(o: usize) -> u32 {
    (ld(o).flags & F_NODRAWERTEXT == 0) as u32
}
extern "C" fn wpQueryDisplaySmallIcons(o: usize) -> u32 {
    (ld(o).flags & F_SMALL != 0) as u32
}
extern "C" fn wpQueryHideLaunchPadFrameCtls(o: usize) -> u32 {
    (ld(o).flags & F_SHOWCTLS == 0) as u32
}
extern "C" fn wpQueryFloatOnTop(o: usize) -> u32 {
    (ld(o).flags & F_NOFLOAT == 0) as u32
}
fn set_bit(o: usize, bit: u32, on: bool) {
    let d = ld(o);
    d.flags = if on { d.flags | bit } else { d.flags & !bit };
    changed(o);
}
extern "C" fn wpSetCloseDrawer(o: usize, on: u32) {
    set_bit(o, F_KEEPDRAWER, on == 0)
}
extern "C" fn wpSetDisplayVertical(o: usize, on: u32) {
    set_bit(o, F_VERTICAL, on != 0)
}
extern "C" fn wpSetDisplayText(o: usize, on: u32) {
    set_bit(o, F_TEXT, on != 0)
}
extern "C" fn wpSetDisplayTextInDrawers(o: usize, on: u32) {
    set_bit(o, F_NODRAWERTEXT, on == 0)
}
extern "C" fn wpSetDisplaySmallIcons(o: usize, on: u32) {
    set_bit(o, F_SMALL, on != 0)
}
extern "C" fn wpSetHideLaunchPadFrameCtls(o: usize, on: u32) {
    set_bit(o, F_SHOWCTLS, on == 0)
}
extern "C" fn wpSetFloatOnTop(o: usize, on: u32) {
    set_bit(o, F_NOFLOAT, on == 0)
}
extern "C" fn wpQueryActionButtonStyle(o: usize) -> u32 {
    // IBM's ACTION_BUTTON_STYLE values follow LPACTIONSTYLE's order.
    ld(o).style
}
extern "C" fn wpSetActionButtonStyle(o: usize, s: u32) -> u32 {
    ld(o).style = s.min(3);
    changed(o);
    1
}
extern "C" fn wpSetObjectListFromStrings(o: usize, drawer: u32, setup: *const u8, _after: u32) -> u32 {
    add_to(o, drawer as usize, &handles(cstr(setup)));
    changed(o);
    1
}
extern "C" fn wpRefreshDrawer(o: usize, _drawer: u32) {
    rebuild(o);
}

methods! {
    __m_lp_setup => lp_setup, 2;
    __m_lp_save_state => lp_save_state, 1;
    __m_lp_restore_state => lp_restore_state, 2;
    __m_lp_uninit => lp_uninit, 1;
    __m_lp_open => lp_open, 4;
    __m_lp_add_pages => lp_add_pages, 2;
    __m_wpAddLaunchPadPage1 => wpAddLaunchPadPage1, 2;
    __m_wpAddLaunchPadPage2 => wpAddLaunchPadPage2, 2;
    __m_wpQueryCloseDrawer => wpQueryCloseDrawer, 1;
    __m_wpQueryDisplayVertical => wpQueryDisplayVertical, 1;
    __m_wpQueryDisplayText => wpQueryDisplayText, 1;
    __m_wpQueryDisplayTextInDrawers => wpQueryDisplayTextInDrawers, 1;
    __m_wpQueryDisplaySmallIcons => wpQueryDisplaySmallIcons, 1;
    __m_wpQueryHideLaunchPadFrameCtls => wpQueryHideLaunchPadFrameCtls, 1;
    __m_wpQueryFloatOnTop => wpQueryFloatOnTop, 1;
    __m_wpSetCloseDrawer => wpSetCloseDrawer, 2;
    __m_wpSetDisplayVertical => wpSetDisplayVertical, 2;
    __m_wpSetDisplayText => wpSetDisplayText, 2;
    __m_wpSetDisplayTextInDrawers => wpSetDisplayTextInDrawers, 2;
    __m_wpSetDisplaySmallIcons => wpSetDisplaySmallIcons, 2;
    __m_wpSetHideLaunchPadFrameCtls => wpSetHideLaunchPadFrameCtls, 2;
    __m_wpSetFloatOnTop => wpSetFloatOnTop, 2;
    __m_wpQueryActionButtonStyle => wpQueryActionButtonStyle, 1;
    __m_wpSetActionButtonStyle => wpSetActionButtonStyle, 2;
    __m_wpSetObjectListFromStrings => wpSetObjectListFromStrings, 4;
    __m_wpRefreshDrawer => wpRefreshDrawer, 2;
}

pub fn impl_launchpad() -> Impl {
    Impl {
        data: core::mem::size_of::<LpData>() as u32,
        methods: alloc::vec![
            ("wpAddLaunchPadPage1", __m_wpAddLaunchPadPage1 as usize),
            ("wpAddLaunchPadPage2", __m_wpAddLaunchPadPage2 as usize),
            ("wpQueryCloseDrawer", __m_wpQueryCloseDrawer as usize),
            ("wpQueryDisplayVertical", __m_wpQueryDisplayVertical as usize),
            ("wpQueryDisplayText", __m_wpQueryDisplayText as usize),
            ("wpQueryDisplayTextInDrawers", __m_wpQueryDisplayTextInDrawers as usize),
            ("wpQueryDisplaySmallIcons", __m_wpQueryDisplaySmallIcons as usize),
            ("wpQueryHideLaunchPadFrameCtls", __m_wpQueryHideLaunchPadFrameCtls as usize),
            ("wpQueryFloatOnTop", __m_wpQueryFloatOnTop as usize),
            ("wpSetCloseDrawer", __m_wpSetCloseDrawer as usize),
            ("wpSetDisplayVertical", __m_wpSetDisplayVertical as usize),
            ("wpSetDisplayText", __m_wpSetDisplayText as usize),
            ("wpSetDisplayTextInDrawers", __m_wpSetDisplayTextInDrawers as usize),
            ("wpSetDisplaySmallIcons", __m_wpSetDisplaySmallIcons as usize),
            ("wpSetHideLaunchPadFrameCtls", __m_wpSetHideLaunchPadFrameCtls as usize),
            ("wpSetFloatOnTop", __m_wpSetFloatOnTop as usize),
            ("wpQueryActionButtonStyle", __m_wpQueryActionButtonStyle as usize),
            ("wpSetActionButtonStyle", __m_wpSetActionButtonStyle as usize),
            ("wpSetObjectListFromStrings", __m_wpSetObjectListFromStrings as usize),
            ("wpRefreshDrawer", __m_wpRefreshDrawer as usize),
        ],
        overrides: alloc::vec![
            ("wpSetup", __m_lp_setup as usize),
            ("wpSaveState", __m_lp_save_state as usize),
            ("wpRestoreState", __m_lp_restore_state as usize),
            ("wpUnInitData", __m_lp_uninit as usize),
            ("wpOpen", __m_lp_open as usize),
            ("wpAddSettingsPages", __m_lp_add_pages as usize),
        ],
    }
}
