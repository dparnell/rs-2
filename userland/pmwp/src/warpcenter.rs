//! The WarpCenter (IBM's SmartCenter class, SCENTER.DLL in Warp 4;
//! `<WP_WARPCENTER>`, in System Setup; IBM's is in the OS/2 System
//! folder, which rs2's desktop does not have): a bar across the top or bottom of the screen.
//! From the left: the Desktop button (a menu of the desktop's objects),
//! the window list (a menu of the windows to switch to), Lockup, Find and
//! Shut down; the monitor (the battery, the processors' activity or a
//! fixed disk's free space; a click goes on to the next one); the tray
//! button (a menu of the trays) and the current tray's objects. At the
//! right end the Assistance button (a menu of the Assistance Center) and
//! the clock (a click goes on from the time to the date, then to the
//! timer, a stopwatch started, stopped and reset from the Timer options
//! button beside it). A resting pointer shows what an item is (bubble
//! help). With SCKILLFEATUREENABLED (or KILLFEATUREENABLED) set, Ctrl
//! and a click on the window list button list the processes instead:
//! one chosen is ended (DosKillProcess) once confirmed, unless
//! SCKILLCONFIRMDISABLED (or KILLCONFIRMDISABLED) is set.
//!
//! IBM's layout, behaviour and setup strings (studied in SCENTER.DLL):
//! the bar is 22 pixels high with small icons; shown on top of maximized
//! windows, it keeps its strip out of the desktop's work area
//! (WinSetDesktopWorkArea), so maximized windows end where it starts;
//! one 333 ms timer does auto-hide, bubble help (after 750 ms) and the
//! monitors (the battery from APM$ every 2 s, the processors from
//! DosPerfSysCall every 500 ms). Up to 16 trays. Setup strings: ADDOBJECT
//! (an object handle, to the current tray), ADDTRAY=name[,objects],
//! ADDTOTRAY=n[,objects] (n from 0), SAVENOW. Its settings pages: Display
//! (on top of maximized windows, auto-hide, top or bottom, small or large
//! icons, bubble help, confirm shutdown), Monitors and Trays. IBM's
//! SmartCenter derives from WPLaunchPad, as this one does.
//!
//! rs2's own: the settings are kept with the object (IBM's go to
//! SCENTER.CFG beside the DLL); large icons are 32 pixels (IBM's bar
//! grows by 4); the battery monitor shows the charge as text too and
//! is offered only when there is a battery (IBM's whenever APM$ opens);
//! the setup strings WCPOSITION=TOP|BOTTOM, WCMONITOR=BATTERY|CPU|d:,
//! WCAUTOHIDE=YES|NO, WCONTOP=YES|NO and WCLARGEICONS=YES|NO; the Desktop
//! and Assistance menus go one folder deep (deeper folders open).

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

/// Flags (0: the defaults, which are IBM's Default button's).
const F_BOTTOM: u32 = 1;
const F_LARGE: u32 = 2;
/// Not on top of maximized windows (they cover it).
const F_NOTONTOP: u32 = 4;
const F_AUTOHIDE: u32 = 8;
/// No bubble help.
const F_NOBUBBLE: u32 = 16;
/// Shut down without asking.
const F_NOCONFIRM: u32 = 32;

/// The monitors offered (bits of `mask`; IBM's resource mask has one for
/// each drive, rs2's one for the fixed disks).
const M_BATTERY: u32 = 1;
const M_CPU: u32 = 2;
const M_DISKS: u32 = 4;
const M_ALL: u32 = 7;

/// The monitor's modes: battery, processors, then drive C: (2) onwards.
const MODE_BATTERY: u32 = 0;
const MODE_CPU: u32 = 1;
const MODE_DRIVE_C: u32 = 2;

const MAX_TRAYS: usize = 16;

/// The clock's modes (a click goes on to the next).
const CLOCK_TIME: u32 = 0;
const CLOCK_DATE: u32 = 1;
const CLOCK_TIMER: u32 = 2;

#[repr(C)]
pub struct WcData {
    trays: Option<Box<Vec<Tray>>>,
    flags: u32,
    /// Monitors switched off (M_*): 0 offers them all.
    off: u32,
    mode: u32,
    current: u32,
    /// What the clock shows: CLOCK_TIME, CLOCK_DATE or CLOCK_TIMER.
    date: u32,
}

#[derive(Clone)]
struct Tray {
    name: Vec<u8>,
    objs: Vec<u32>,
}

fn wd<'a>(obj: usize) -> &'a mut WcData {
    som::data(wps::ccd("SmartCenter"), obj)
}

fn trays<'a>(o: usize) -> &'a mut Vec<Tray> {
    wd(o).trays.get_or_insert_with(|| Box::new(alloc::vec![Tray { name: b"Tray 1".to_vec(), objs: Vec::new() }]))
}

fn current(o: usize) -> usize {
    (wd(o).current as usize).min(trays(o).len().saturating_sub(1))
}

fn tray_name(n: usize) -> Vec<u8> {
    alloc::format!("Tray {}", n + 1).into_bytes()
}

fn add_tray(o: usize, name: &[u8]) -> Option<usize> {
    let t = trays(o);
    if t.len() >= MAX_TRAYS {
        return None;
    }
    let name: Vec<u8> = name.iter().copied().filter(|&c| c >= b' ' && c != b';' && c != 1 && c != 2).take(33).collect();
    let name = if name.trim_ascii().is_empty() { tray_name(t.len()) } else { name };
    t.push(Tray { name, objs: Vec::new() });
    Some(t.len() - 1)
}

fn add_objects(o: usize, n: usize, hs: &[u32]) {
    let Some(t) = trays(o).get_mut(n) else { return };
    for &h in hs {
        if h != 0 && !t.objs.contains(&h) {
            t.objs.push(h);
        }
    }
}

fn yes(v: &[u8]) -> bool {
    v.eq_ignore_ascii_case(b"YES")
}

fn set_flag(o: usize, bit: u32, on: bool) {
    let d = wd(o);
    d.flags = if on { d.flags | bit } else { d.flags & !bit };
}

/// The handles of a comma-separated list of object IDs and paths.
fn handles(v: &[u8]) -> Vec<u32> {
    v.split(|&c| c == b',')
        .map(|s| s.trim_ascii())
        .filter(|s| !s.is_empty())
        .map(|s| wps::object_from_location(s))
        .filter(|&x| x != 0)
        .map(obj::handle)
        .collect()
}

/// Changed: saved soon, and the bar shows it.
fn changed(o: usize) {
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
    refresh(o);
}

extern "C" fn wc_setup(o: usize, setup: *const u8) -> u32 {
    // WPObject's, not the LaunchPad's: its strings are not the WarpCenter's.
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpSetup, &[o, setup as usize]);
    for (k, v) in obj::setup_pairs(cstr(setup)) {
        match k.as_slice() {
            b"ADDOBJECT" => {
                let n = current(o);
                if let Some(h) = parse_dec(v.trim_ascii()) {
                    add_objects(o, n, &[h]);
                }
            }
            b"ADDTRAY" => {
                let (name, rest) = split_first(&v);
                if let Some(n) = add_tray(o, name) {
                    add_objects(o, n, &handles(rest));
                }
            }
            b"ADDTOTRAY" => {
                let (n, rest) = split_first(&v);
                if let Some(n) = parse_dec(n.trim_ascii()) {
                    if (n as usize) < trays(o).len() {
                        wd(o).current = n;
                        add_objects(o, n as usize, &handles(rest));
                    }
                }
            }
            b"SAVENOW" => {
                obj::call(m::WPObject::wpSaveImmediate, &[o]);
            }
            b"WCPOSITION" => set_flag(o, F_BOTTOM, v.eq_ignore_ascii_case(b"BOTTOM")),
            b"WCAUTOHIDE" => set_flag(o, F_AUTOHIDE, yes(&v)),
            b"WCONTOP" => set_flag(o, F_NOTONTOP, !yes(&v)),
            b"WCLARGEICONS" => set_flag(o, F_LARGE, yes(&v)),
            b"WCMONITOR" => {
                let u = v.to_ascii_uppercase();
                wd(o).mode = match u.as_slice() {
                    b"BATTERY" => MODE_BATTERY,
                    b"CPU" => MODE_CPU,
                    [d @ b'C'..=b'Z', b':'] | [d @ b'C'..=b'Z'] => MODE_DRIVE_C + (d - b'C') as u32,
                    _ => wd(o).mode,
                };
            }
            _ => {}
        }
    }
    changed(o);
    1
}

fn split_first(v: &[u8]) -> (&[u8], &[u8]) {
    match v.iter().position(|&c| c == b',') {
        Some(i) => (&v[..i], &v[i + 1..]),
        None => (v, &b""[..]),
    }
}

/// The trays as saved: name 0x01 handles (decimal, commas) 0x02 ...
fn trays_text(o: usize) -> Vec<u8> {
    let mut s = Vec::new();
    for t in trays(o).iter() {
        s.extend_from_slice(&t.name);
        s.push(1);
        for (i, h) in t.objs.iter().enumerate() {
            if i > 0 {
                s.push(b',');
            }
            s.extend_from_slice(alloc::format!("{h}").as_bytes());
        }
        s.push(2);
    }
    s
}

extern "C" fn wc_save_state(o: usize) -> u32 {
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[o]);
    let d = wd(o);
    let key = b"SmartCenter\0".as_ptr();
    obj::wpSaveLong(o, key, 1, d.flags);
    obj::wpSaveLong(o, key, 2, d.off | d.date << 8 | d.current << 16);
    obj::wpSaveLong(o, key, 3, d.mode);
    let s = z(&trays_text(o));
    obj::wpSaveString(o, key, 4, s.as_ptr());
    1
}

extern "C" fn wc_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    if let Some(v) = obj::restored_long(o, b"SmartCenter", 1) {
        wd(o).flags = v;
    }
    if let Some(v) = obj::restored_long(o, b"SmartCenter", 2) {
        wd(o).off = v & 0xff;
        wd(o).date = (v >> 8 & 0xff).min(CLOCK_TIMER);
        wd(o).current = v >> 16 & 0xff;
    }
    if let Some(v) = obj::restored_long(o, b"SmartCenter", 3) {
        wd(o).mode = v;
    }
    if let Some(s) = obj::restored_string(o, b"SmartCenter", 4) {
        let s = s.strip_suffix(&[0]).unwrap_or(&s);
        let mut list = Vec::new();
        for part in s.split(|&c| c == 2).filter(|p| !p.is_empty()) {
            let (name, hs) = match part.iter().position(|&c| c == 1) {
                Some(i) => (&part[..i], &part[i + 1..]),
                None => (part, &b""[..]),
            };
            let objs = hs.split(|&c| c == b',').filter_map(parse_dec).collect();
            list.push(Tray { name: name.to_vec(), objs });
        }
        if !list.is_empty() {
            list.truncate(MAX_TRAYS);
            wd(o).trays = Some(Box::new(list));
        }
    }
    1
}

extern "C" fn wc_uninit(o: usize) {
    wd(o).trays = None;
    som::call_parent(wps::ccd("SmartCenter"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[o]);
}

extern "C" fn wc_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32;
    }
    set_closed(false);
    open_bar(o)
}

/// The WarpCenter closed (or opened) by the user: it stays so after a
/// restart (its shadow leaves the Startup folder, or comes back).
fn set_closed(closed: bool) {
    use crate::util::{prf_read, prf_write, HINI_USER};
    let was = prf_read(HINI_USER, b"PM_Workplace", CLOSED_KEY).is_some();
    if closed == was {
        return;
    }
    if closed {
        prf_write(HINI_USER, b"PM_Workplace", CLOSED_KEY, Some(b"1\0"));
        if let Some(sh) = wps::handle_of_id(b"<RS2_WARPCENTERSTART>").map(wps::object_from_handle).filter(|&x| x != 0) {
            obj::call(m::WPObject::wpFree, &[sh]);
        }
    } else {
        prf_write(HINI_USER, b"PM_Workplace", CLOSED_KEY, None);
        crate::install::install_object(START.0, START.1);
    }
}

/// PM_Workplace's key for a WarpCenter the user closed.
pub const CLOSED_KEY: &[u8] = b"Rs2WarpCenterClosed";

/// The WarpCenter and its shadow in the Startup folder.
pub const OBJECT: (&[u8], &[u8]) = (b"WarpCenter;SmartCenter;<WP_CONFIG>;PRESERVEOLD", b"OBJECTID=<WP_WARPCENTER>");
pub const START: (&[u8], &[u8]) = (b"WarpCenter;WPShadow;<WP_START>;PRESERVEOLD", b"SHADOWID=<WP_WARPCENTER>;OBJECTID=<RS2_WARPCENTERSTART>");

/// IBM's first tray: these objects, where they are.
pub const FIRST_TRAY: [&[u8]; 6] = [b"<WP_DRIVES>", b"<WP_PRINTERSFOLDER>", b"<MMPM2_MMVOLUME>", b"<WP_CONFIG>", b"<WP_PROMPTS>", b"<WP_CONNECTIONSFOLDER>"];

/// The setup string that fills the first tray of a new WarpCenter.
pub fn first_tray_setup() -> Vec<u8> {
    let mut s = b"ADDTOTRAY=0".to_vec();
    for id in FIRST_TRAY {
        s.push(b',');
        s.extend_from_slice(id);
    }
    s
}

// ---- The bar ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Desktop,
    WinList,
    Lockup,
    Find,
    Shutdown,
    Monitor,
    TrayButton,
    Tray(usize),
    Help,
    /// The timer's options button, beside the clock in timer mode.
    Timer,
    Clock,
}

struct Bar {
    obj: usize,
    hwnd: u32,
    items: Vec<(RectL, Hit)>,
    pressed: Option<Hit>,
    /// The object of the current tray a pop-up menu is about.
    menu_on: Option<usize>,
    /// Menu commands: the objects of the Desktop, Assistance and window
    /// list menus (by command - CMD_LIST).
    list: Vec<usize>,
    switch: Vec<u32>,
    /// A pop-up menu of the bar is up.
    menu_up: bool,
    hidden: bool,
    idle: u32,
    /// Bubble help: the item the pointer rests on, since when (ms), and
    /// the bubble window.
    rest: Option<Hit>,
    rest_ms: u32,
    bubble: u32,
    mon: Monitor,
    /// The timer: milliseconds counted, when last counted, running.
    timer_ms: u32,
    timer_at: u32,
    timer_on: bool,
    /// The process list's processes (by command - CMD_KILL).
    kill: Vec<u32>,
}

#[derive(Default)]
struct Monitor {
    battery: Option<os2::power::ApmStatus>,
    battery_at: u32,
    /// Each processor's busy percentages, newest last.
    cpu: Vec<Vec<u8>>,
    cpu_last: Vec<[u64; 4]>,
    cpu_at: u32,
    /// (drive 1 = A:, label, MB free, MB total)
    disk: Option<(u32, Vec<u8>, u32, u32)>,
    disk_at: u32,
    clock: Vec<u8>,
}

static mut BARS: Vec<Bar> = Vec::new();

#[allow(static_mut_refs)]
fn bars() -> &'static mut Vec<Bar> {
    unsafe { &mut BARS }
}

const CLASS: &[u8] = b"WPS_WarpCenter\0";
const BUBBLE_CLASS: &[u8] = b"WPS_WarpCenterBubble\0";
const TID_TICK: u32 = 1;
const TICK: u32 = 333;
const BUBBLE_AFTER: u32 = 750;
const HIDE_AFTER: u32 = 1000;
/// What stays on the screen of a hidden bar.
const STRIP: i32 = 2;
const DP_MINI: u32 = 0x0004;
const DT_CENTER: u32 = 0x0100;
const DT_VCENTER: u32 = 0x0400;
const WS_CLIPSIBLINGS: u32 = 0x1000_0000;
const PP_FONTNAMESIZE: u32 = 1;
const PALEGRAY: u32 = 0xcccccc;
const DARKBLUE: u32 = 0x000080;

// Menu commands.
const CMD_OPEN: u32 = 1;
const CMD_REMOVE: u32 = 2;
const CMD_SETTINGS: u32 = 3;
const CMD_CLOSE: u32 = 4;
const CMD_ADDTRAY: u32 = 5;
const CMD_RENAMETRAY: u32 = 6;
const CMD_DELETETRAY: u32 = 7;
const CMD_MONITOR: u32 = 8;
// IBM's ids for the timer's menu.
const CMD_TSTART: u32 = 0x23;
const CMD_TSTOP: u32 = 0x24;
const CMD_TRESET: u32 = 0x25;
const CMD_TRAY: u32 = 0x100; // + tray
const CMD_MON: u32 = 0x180; // + mode
const CMD_LIST: u32 = 0x1000; // + object
const CMD_SWITCH: u32 = 0x3000; // + switch entry
const CMD_KILL: u32 = 0x4000; // + process

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.732"]
    fn WinDrawPointer(hps: u32, x: i32, y: i32, h: u32, fs: u32) -> u32;
    #[link_name = "PMWIN.804"]
    fn WinQueryCapture(desk: u32) -> u32;
    #[link_name = "PMWIN.1068"]
    fn WinSetDesktopWorkArea(desk: u32, r: *const RectL) -> u32;
    #[link_name = "PMSHAPI.126"]
    fn WinQuerySwitchList(hab: u32, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "PMSHAPI.131"]
    fn WinSwitchToProgram(h: u32) -> u32;
    #[link_name = "DOSCALLS.976"]
    fn DosPerfSysCall(cmd: u32, a1: u32, a2: u32, a3: u32) -> u32;
    #[link_name = "DOSCALLS.348"]
    fn DosQuerySysInfo(first: u32, last: u32, buf: *mut u32, cb: u32) -> u32;
    #[link_name = "DOSCALLS.278"]
    fn DosQueryFSInfo(disk: u32, level: u32, buf: *mut u8, cb: u32) -> u32;
    #[link_name = "DOSCALLS.275"]
    fn DosQueryCurrentDisk(disk: *mut u32, map: *mut u32) -> u32;
    #[link_name = "DOSCALLS.209"]
    fn DosError(fl: u32) -> u32;
    #[link_name = "DOSCALLS.368"]
    fn DosQuerySysState(list: u32, level: u32, pid: u32, tid: u32, buf: *mut u8, cb: u32) -> u32;
}

fn hab() -> u32 {
    unsafe { WinQueryAnchorBlock(HWND_DESKTOP) }
}

fn rect(l: i32, b: i32, r: i32, t: i32) -> RectL {
    RectL { l, b, r, t }
}

fn now() -> u32 {
    let mut ms = 0u32;
    unsafe { DosQuerySysInfo(14, 14, &mut ms, 4) };
    ms
}

fn large(o: usize) -> bool {
    wd(o).flags & F_LARGE != 0
}

/// The icon size and the bar's height.
fn icon_px(o: usize) -> i32 {
    if large(o) { 32 } else { 16 }
}

fn bar_h(o: usize) -> i32 {
    icon_px(o) + 6
}

/// The objects of the current tray (the ones still there).
fn tray_objects(o: usize) -> Vec<usize> {
    let n = current(o);
    trays(o).get(n).map_or(Vec::new(), |t| t.objs.iter().map(|&h| wps::object_from_handle(h)).filter(|&x| x != 0).collect())
}

/// Lay out the bar for a screen `sw` wide.
fn layout(o: usize, sw: i32) -> Vec<(RectL, Hit)> {
    let h = bar_h(o);
    let sq = icon_px(o) + 7;
    let mut items = Vec::new();
    let mut x = 0;
    for it in [Hit::Desktop, Hit::WinList, Hit::Lockup, Hit::Find, Hit::Shutdown] {
        items.push((rect(x, 0, x + sq, h), it));
        x += sq - 1;
    }
    x += 3;
    let mw = 8 * (16 + 7);
    items.push((rect(x, 0, x + mw, h), Hit::Monitor));
    x += mw + 3;
    items.push((rect(x, 0, x + sq, h), Hit::TrayButton));
    x += sq - 1;
    // The right end: the clock, the Assistance button left of it.
    let cw = if large(o) { 128 } else { 104 };
    let clock = rect(sw - cw, 0, sw, h);
    // In timer mode the Timer options button, narrow, left of the clock.
    let left = if wd(o).date == CLOCK_TIMER {
        let t = rect(clock.l - 13, 0, clock.l + 1, h);
        items.push((t, Hit::Timer));
        t.l
    } else {
        clock.l
    };
    let help = rect(left - sq + 1, 0, left + 1, h);
    for i in 0..tray_objects(o).len() {
        if x + sq > help.l - 2 {
            break;
        }
        items.push((rect(x, 0, x + sq, h), Hit::Tray(i)));
        x += sq - 1;
    }
    items.push((help, Hit::Help));
    items.push((clock, Hit::Clock));
    items
}

fn find_bar(h: u32) -> Option<usize> {
    bars().iter().position(|b| b.hwnd == h)
}

fn bar_of(o: usize) -> Option<usize> {
    bars().iter().position(|b| b.obj == o)
}

/// Where the bar goes: (y shown, y hidden).
fn place(o: usize) -> (i32, i32) {
    let (_, sh) = screen();
    let h = bar_h(o);
    if wd(o).flags & F_BOTTOM != 0 { (0, STRIP - h) } else { (sh - h, sh - STRIP) }
}

/// The work area: the screen without the bar, while it is shown on top
/// of maximized windows and does not hide.
fn set_work_area(o: usize, open: bool) {
    let (sw, sh) = screen();
    let f = wd(o).flags;
    if !open || f & (F_NOTONTOP | F_AUTOHIDE) != 0 {
        unsafe { WinSetDesktopWorkArea(HWND_DESKTOP, core::ptr::null()) };
        return;
    }
    let h = bar_h(o);
    let r = if f & F_BOTTOM != 0 { rect(0, h, sw, sh) } else { rect(0, 0, sw, sh - h) };
    unsafe { WinSetDesktopWorkArea(HWND_DESKTOP, &r) };
}

/// Open the bar (or bring it up).
fn open_bar(o: usize) -> u32 {
    if let Some(b) = bar_of(o) {
        let h = bars()[b].hwnd;
        unsafe { WinSetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER) };
        return h;
    }
    static mut REGISTERED: bool = false;
    unsafe {
        if !REGISTERED {
            WinRegisterClass(hab(), CLASS.as_ptr(), wc_proc, 0, 0);
            WinRegisterClass(hab(), BUBBLE_CLASS.as_ptr(), bubble_proc, 0, 0);
            REGISTERED = true;
        }
    }
    let (sw, _) = screen();
    let (y, _) = place(o);
    let title = z(&obj::title(o));
    let h = unsafe { WinCreateWindow(HWND_DESKTOP, CLASS.as_ptr() as u32, title.as_ptr(), WS_CLIPSIBLINGS, 0, y, sw, bar_h(o), 0, HWND_TOP, 0x555, core::ptr::null(), core::ptr::null()) };
    if h == 0 {
        return 0;
    }
    let font = b"8.Helv\0";
    unsafe { WinSetPresParam(h, PP_FONTNAMESIZE, font.len() as u32, font.as_ptr() as *const u32) };
    bars().push(Bar {
        obj: o,
        hwnd: h,
        items: layout(o, sw),
        pressed: None,
        menu_on: None,
        list: Vec::new(),
        switch: Vec::new(),
        menu_up: false,
        hidden: false,
        idle: 0,
        rest: None,
        rest_ms: 0,
        bubble: 0,
        mon: Monitor::default(),
        timer_ms: 0,
        timer_at: 0,
        timer_on: false,
        kill: Vec::new(),
    });
    let b = bars().len() - 1;
    // The monitor starts on one there is.
    let mode = wd(o).mode;
    if !available(b, mode) {
        wd(o).mode = next_mode(b, mode);
    }
    poll(b, true);
    set_work_area(o, true);
    unsafe {
        WinSetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER);
        WinStartTimer(hab(), h, TID_TICK, TICK);
    }
    h
}

fn close_bar(o: usize) {
    let Some(b) = bar_of(o) else { return };
    let (h, bubble) = (bars()[b].hwnd, bars()[b].bubble);
    bars().remove(b);
    unsafe {
        if bubble != 0 {
            WinDestroyWindow(bubble);
        }
        WinDestroyWindow(h);
    }
    set_work_area(o, false);
}

/// The settings or trays changed: lay the bar out again, put it where it
/// now goes.
fn refresh(o: usize) {
    let Some(b) = bar_of(o) else { return };
    let (sw, _) = screen();
    bars()[b].items = layout(o, sw);
    let (y, _) = place(o);
    let h = bars()[b].hwnd;
    bars()[b].hidden = false;
    let mode = wd(o).mode;
    if !available(b, mode) {
        wd(o).mode = next_mode(b, mode);
        poll(b, true);
    }
    unsafe {
        WinSetWindowPos(h, if wd(o).flags & F_NOTONTOP != 0 { HWND_BOTTOM } else { HWND_TOP }, 0, y, sw, bar_h(o), SWP_MOVE | SWP_SIZE | SWP_ZORDER);
        WinInvalidateRect(h, core::ptr::null(), 0);
    }
    set_work_area(o, true);
}

fn hit(b: usize, x: i32, y: i32) -> Option<Hit> {
    bars()[b].items.iter().find(|(r, _)| x >= r.l && x < r.r && y >= r.b && y < r.t).map(|&(_, h)| h)
}

fn item_rect(b: usize, h: Hit) -> RectL {
    bars()[b].items.iter().find(|&&(_, x)| x == h).map_or(RectL::default(), |&(r, _)| r)
}

fn invalidate_item(b: usize, h: Hit) {
    let r = item_rect(b, h);
    unsafe { WinInvalidateRect(bars()[b].hwnd, &r, 0) };
}

// ---- The monitors ----

/// Whether monitor mode `mode` can be shown.
fn available(b: usize, mode: u32) -> bool {
    let o = bars()[b].obj;
    let off = wd(o).off;
    match mode {
        MODE_BATTERY => off & M_BATTERY == 0 && os2::power::apm_status(0).is_some_and(|a| a.has_battery()),
        MODE_CPU => off & M_CPU == 0,
        m => off & M_DISKS == 0 && m >= MODE_DRIVE_C && m < MODE_DRIVE_C + 24 && fixed_disk(m - MODE_DRIVE_C + 3),
    }
}

/// The next mode there is after `mode` (the processors when no other).
fn next_mode(b: usize, mode: u32) -> u32 {
    for k in 1..=26 {
        let m = (mode + k) % 26;
        if available(b, m) {
            return m;
        }
    }
    MODE_CPU
}

/// Drive `d` (1 = A:) is a fixed disk: a local file system that is not a
/// CD's.
fn fixed_disk(d: u32) -> bool {
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { DosQueryCurrentDisk(&mut cur, &mut map) };
    if d < 3 || map & 1 << (d - 1) == 0 {
        return false;
    }
    match crate::icons::fs_name(d) {
        Some(fs) => [&b"FAT"[..], b"FAT32", b"HPFS", b"JFS", b"EXT2", b"NTFS"].iter().any(|n| fs.eq_ignore_ascii_case(n)),
        None => false,
    }
}

/// Read the current monitor's value (when it is due, or `now`).
fn poll(b: usize, force: bool) {
    let o = bars()[b].obj;
    let t = now();
    let mode = wd(o).mode;
    let mon = &mut bars()[b].mon;
    let mut redraw = false;
    match mode {
        MODE_BATTERY => {
            if force || t.wrapping_sub(mon.battery_at) >= 2000 {
                mon.battery_at = t;
                let a = os2::power::apm_status(0);
                let key = |x: &Option<os2::power::ApmStatus>| x.map(|a| (a.ac, a.status, a.life, a.flags, a.time));
                if key(&a) != key(&mon.battery) || force {
                    mon.battery = a;
                    redraw = true;
                }
            }
        }
        MODE_CPU => {
            if force || t.wrapping_sub(mon.cpu_at) >= 500 {
                mon.cpu_at = t;
                cpu_sample(mon);
                redraw = true;
            }
        }
        m => {
            if force || t.wrapping_sub(mon.disk_at) >= 5000 {
                mon.disk_at = t;
                let d = m - MODE_DRIVE_C + 3;
                let new = disk_info(d);
                if new != mon.disk || force {
                    mon.disk = new;
                    redraw = true;
                }
            }
        }
    }
    if redraw {
        invalidate_item(b, Hit::Monitor);
        if bars()[b].bubble != 0 && bars()[b].rest == Some(Hit::Monitor) {
            show_bubble(b, Hit::Monitor);
        }
    }
}

/// The processors' activity (DosPerfSysCall CMD_KI_RDCNT: time, idle,
/// busy and interrupt time for each).
fn cpu_sample(mon: &mut Monitor) {
    let mut n = 1u32;
    unsafe { DosQuerySysInfo(26, 26, &mut n, 4) };
    let n = n.clamp(1, 64) as usize;
    let mut buf = alloc::vec![0u64; n * 4];
    if unsafe { DosPerfSysCall(0x63, buf.as_mut_ptr() as u32, 0, 0) } != 0 {
        return;
    }
    let width = 8 * (16 + 7) as usize;
    mon.cpu.resize(n, Vec::new());
    for i in 0..n {
        let cur = [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]];
        let pct = match mon.cpu_last.get(i) {
            Some(last) => {
                let dt = cur[0].wrapping_sub(last[0]);
                let busy = cur[2].wrapping_sub(last[2]) + cur[3].wrapping_sub(last[3]);
                if dt == 0 { 0 } else { (busy.min(dt) * 100 / dt) as u8 }
            }
            None => 0,
        };
        if mon.cpu_last.len() > i {
            mon.cpu_last[i] = cur;
            let h = &mut mon.cpu[i];
            h.push(pct);
            if h.len() > width {
                h.remove(0);
            }
        } else {
            mon.cpu_last.push(cur);
        }
    }
}

/// Drive `d`'s label, free and total megabytes.
fn disk_info(d: u32) -> Option<(u32, Vec<u8>, u32, u32)> {
    let mut a = [0u8; 18];
    unsafe { DosError(0) };
    let rc = unsafe { DosQueryFSInfo(d, 1, a.as_mut_ptr(), 18) };
    let mut l = [0u8; 20];
    let rc2 = unsafe { DosQueryFSInfo(d, 2, l.as_mut_ptr(), 20) };
    unsafe { DosError(1) };
    if rc != 0 {
        return None;
    }
    let u = |i: usize| u32::from_le_bytes([a[i], a[i + 1], a[i + 2], a[i + 3]]) as u64;
    let unit = u(4) * u16::from_le_bytes([a[16], a[17]]) as u64;
    let mb = |n: u64| (n * unit / (1024 * 1024)) as u32;
    let label = if rc2 == 0 { l[5..5 + (l[4] as usize).min(12)].to_vec() } else { Vec::new() };
    Some((d, label, mb(u(12)), mb(u(8))))
}

fn mode_name(mode: u32) -> Vec<u8> {
    match mode {
        MODE_BATTERY => b"~Battery".to_vec(),
        MODE_CPU => b"~Processor activity".to_vec(),
        m => alloc::format!("Drive ~{}:", (b'A' + (m - MODE_DRIVE_C + 2) as u8) as char).into_bytes(),
    }
}

// ---- Painting ----

fn fill(hps: u32, r: RectL, c: u32) {
    unsafe { WinFillRect(hps, &r, c as i32) };
}

/// A raised (or, pressed, sunken) square.
fn bevel(hps: u32, r: RectL, sunk: bool) {
    let (hi, lo) = (0xffffff, 0x808080);
    let (a, b) = if sunk { (lo, hi) } else { (hi, lo) };
    fill(hps, rect(r.l, r.t - 1, r.r, r.t), a);
    fill(hps, rect(r.l, r.b, r.l + 1, r.t), a);
    fill(hps, rect(r.l, r.b, r.r, r.b + 1), b);
    fill(hps, rect(r.r - 1, r.b, r.r, r.t), b);
}

fn text(hps: u32, t: &[u8], mut r: RectL, fg: u32, fl: u32) {
    unsafe { WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut r, fg as i32, -1, fl | DT_VCENTER) };
}

fn icon_at(hps: u32, r: RectL, icon: u32, px: i32) {
    let fs = if px == 16 { DP_MINI } else { 0 };
    unsafe { WinDrawPointer(hps, r.l + (r.r - r.l - px) / 2, r.b + (r.t - r.b - px) / 2, icon, fs) };
}

fn paint(h: u32) {
    let Some(b) = find_bar(h) else { return };
    let o = bars()[b].obj;
    let px = icon_px(o);
    let mut upd = RectL::default();
    unsafe {
        let hps = WinBeginPaint(h, 0, &mut upd);
        // LCOLF_RGB
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
        let mut all = RectL::default();
        WinQueryWindowRect(h, &mut all);
        fill(hps, all, PALEGRAY);
        // The edge towards the screen's middle.
        if wd(o).flags & F_BOTTOM != 0 {
            fill(hps, rect(all.l, all.t - 1, all.r, all.t), 0xffffff);
        } else {
            fill(hps, rect(all.l, all.b, all.r, all.b + 1), 0x808080);
        }
        let objs = tray_objects(o);
        let items = bars()[b].items.clone();
        for (r, it) in items {
            let pressed = bars()[b].pressed == Some(it);
            let ic = |id: u32| crate::icons::shell(id);
            match it {
                Hit::Monitor => paint_monitor(hps, b, r),
                Hit::Clock => {
                    bevel(hps, r, true);
                    let t = bars()[b].mon.clock.clone();
                    text(hps, &t, rect(r.l + 2, r.b + 1, r.r - 2, r.t - 1), 0, DT_CENTER);
                }
                Hit::Timer => {
                    bevel(hps, r, pressed);
                    // An arrow towards the screen's middle (where the menu
                    // comes up).
                    let cx = (r.l + r.r) / 2;
                    let cy = (r.b + r.t) / 2;
                    let down = wd(o).flags & F_BOTTOM == 0;
                    for k in 0..4 {
                        let y = if down { cy - 2 + k } else { cy + 1 - k };
                        fill(hps, rect(cx - k, y, cx + k + 1, y + 1), 0);
                    }
                }
                Hit::Tray(i) => {
                    if pressed {
                        bevel(hps, r, true);
                    }
                    if let Some(&x) = objs.get(i) {
                        icon_at(hps, r, crate::templates::view_icon(x), px);
                    }
                }
                _ => {
                    bevel(hps, r, pressed);
                    let id = match it {
                        Hit::Desktop => crate::icons::WARPCENTER,
                        Hit::WinList => crate::icons::WINDOW_LIST,
                        Hit::Lockup => crate::icons::LOCKUP,
                        Hit::Find => crate::icons::FIND,
                        Hit::Shutdown => crate::icons::SHUTDOWN,
                        Hit::TrayButton => crate::icons::TRAY,
                        _ => crate::icons::ASSISTANCE,
                    };
                    icon_at(hps, r, ic(id), px);
                }
            }
        }
        WinEndPaint(hps);
    }
}

fn paint_monitor(hps: u32, b: usize, r: RectL) {
    let o = bars()[b].obj;
    bevel(hps, r, true);
    let inner = rect(r.l + 1, r.b + 1, r.r - 1, r.t - 1);
    let mode = wd(o).mode;
    let mon = &bars()[b].mon;
    // A gauge: black outline, the filled part dark blue, its text on it.
    let gauge = |x0: i32, pct: u32, t: &[u8]| {
        let g = rect(x0, inner.b + 3, inner.r - 3, inner.t - 3);
        fill(hps, g, 0);
        let gi = rect(g.l + 1, g.b + 1, g.r - 1, g.t - 1);
        fill(hps, gi, PALEGRAY);
        let w = (gi.r - gi.l) * pct.min(100) as i32 / 100;
        fill(hps, rect(gi.l, gi.b, gi.l + w, gi.t), DARKBLUE);
        let fg = if pct >= 55 { 0xffffff } else { 0 };
        text(hps, t, gi, fg, DT_CENTER);
    };
    match mode {
        MODE_BATTERY => {
            let a = mon.battery;
            let ac = a.is_some_and(|a| a.ac == 1);
            let icon = crate::icons::shell(if ac { crate::icons::PLUG } else { crate::icons::BATTERY });
            let sq = rect(inner.l, inner.b, inner.l + 16 + 6, inner.t);
            icon_at(hps, sq, icon, 16);
            let life = a.map_or(0xff, |a| a.life);
            let pct = if life <= 100 { life as u32 } else { 0 };
            let mut t = if life <= 100 { alloc::format!("{life}%") } else { alloc::string::String::from("?") };
            if a.is_some_and(|a| a.charging()) {
                t += " +";
            }
            gauge(sq.r + 2, pct, t.as_bytes());
        }
        MODE_CPU => {
            let g = rect(inner.l + 2, inner.b + 2, inner.r - 2, inner.t - 2);
            fill(hps, g, 0);
            let gh = g.t - g.b;
            // One line for each processor, as IBM's colours 1, 2, ...
            const COLOURS: [u32; 8] = [0x00ff00, 0xffff00, 0x00ffff, 0xff00ff, 0xff8040, 0x8080ff, 0xffffff, 0xff4040];
            for (i, hist) in mon.cpu.iter().enumerate() {
                let c = COLOURS[i % COLOURS.len()];
                let n = hist.len() as i32;
                let w = g.r - g.l;
                let mut prev: Option<i32> = None;
                for (k, &p) in hist.iter().enumerate() {
                    let x = g.r - (n - k as i32);
                    if x < g.l || x >= g.l + w {
                        continue;
                    }
                    let y = g.b + (gh - 1) * p as i32 / 100;
                    let (lo, hi) = match prev {
                        Some(py) => (py.min(y), py.max(y)),
                        None => (y, y),
                    };
                    fill(hps, rect(x, lo, x + 1, hi + 1), c);
                    prev = Some(y);
                }
            }
        }
        m => {
            let letter = (b'A' + (m - MODE_DRIVE_C + 2) as u8) as char;
            match &mon.disk {
                Some((_, label, free, total)) => {
                    let lab = alloc::format!("{letter}:");
                    let lw = 24;
                    text(hps, lab.as_bytes(), rect(inner.l + 3, inner.b, inner.l + lw, inner.t), 0, 0);
                    let pct = if *total == 0 { 0 } else { (*free as u64 * 100 / *total as u64) as u32 };
                    let _ = label;
                    gauge(inner.l + lw, pct, alloc::format!("{free} MB free").as_bytes());
                }
                None => text(hps, alloc::format!("{letter}: not ready").as_bytes(), inner, 0, DT_CENTER),
            }
        }
    }
}

// ---- Bubble help ----

/// What a resting pointer shows over an item.
fn bubble_text(b: usize, it: Hit) -> Vec<u8> {
    let o = bars()[b].obj;
    let t: alloc::string::String = match it {
        Hit::Desktop => "Desktop: its objects".into(),
        Hit::WinList => "Window list".into(),
        Hit::Lockup => "Lockup now".into(),
        Hit::Find => "Find".into(),
        Hit::Shutdown => "Shut down".into(),
        Hit::TrayButton => alloc::format!("Trays (now {})", core::str::from_utf8(&trays(o)[current(o)].name).unwrap_or("?")),
        Hit::Help => "Assistance".into(),
        // IBM's hints: Time, Date, Timer, Timer options.
        Hit::Clock => match wd(o).date {
            CLOCK_DATE => "Date (click for the timer)".into(),
            CLOCK_TIMER => if bars()[b].timer_on { "Timer, running (click for the time)".into() } else { "Timer (click for the time)".into() },
            _ => "Time (click for the date)".into(),
        },
        Hit::Timer => "Timer options".into(),
        Hit::Tray(i) => {
            let x = tray_objects(o).get(i).copied().unwrap_or(0);
            let t: Vec<u8> = obj::title(x).into_iter().map(|c| if c == b'\n' || c == b'\r' { b' ' } else { c }).collect();
            return t;
        }
        Hit::Monitor => {
            let mon = &bars()[b].mon;
            let s = match wd(o).mode {
                MODE_BATTERY => match mon.battery {
                    Some(a) if a.ac == 1 => alloc::format!("External power; battery {}", a.battery_text()),
                    Some(a) => alloc::format!("Battery: {}", a.battery_text()),
                    None => "Battery: unknown".into(),
                },
                MODE_CPU => {
                    let now: Vec<alloc::string::String> = mon.cpu.iter().map(|h| alloc::format!("{}%", h.last().copied().unwrap_or(0))).collect();
                    alloc::format!("Processor activity: {}", now.join(", "))
                }
                m => match &mon.disk {
                    Some((d, label, free, total)) => {
                        let l: alloc::string::String = label.iter().map(|&c| c as char).collect();
                        alloc::format!("Drive {}: {} - {} MB free of {} MB", (b'@' + *d as u8) as char, l.trim(), free, total)
                    }
                    None => alloc::format!("Drive {}: not ready", (b'A' + (m - MODE_DRIVE_C + 2) as u8) as char),
                },
            };
            s + " (click for the next monitor)"
        }
    };
    t.into_bytes()
}

static mut BUBBLE_TEXT: Vec<u8> = Vec::new();

fn show_bubble(b: usize, it: Hit) {
    let o = bars()[b].obj;
    let t = bubble_text(b, it);
    unsafe { *(&raw mut BUBBLE_TEXT) = t.clone() };
    let w = (t.len() as i32 * 6 + 12).clamp(40, 600);
    let h = 16;
    let r = item_rect(b, it);
    let mut p = [PointL { x: r.l, y: r.b }, PointL { x: r.r, y: r.t }];
    unsafe { WinMapWindowPoints(bars()[b].hwnd, HWND_DESKTOP, p.as_mut_ptr(), 2) };
    let (sw, _) = screen();
    let x = p[0].x.clamp(0, (sw - w).max(0));
    let y = if wd(o).flags & F_BOTTOM != 0 { p[1].y + 2 } else { p[0].y - h - 2 };
    let bub = bars()[b].bubble;
    unsafe {
        if bub == 0 {
            let nb = WinCreateWindow(HWND_DESKTOP, BUBBLE_CLASS.as_ptr() as u32, b"\0".as_ptr(), WS_CLIPSIBLINGS, x, y, w, h, 0, HWND_TOP, 0x556, core::ptr::null(), core::ptr::null());
            let font = b"8.Helv\0";
            WinSetPresParam(nb, PP_FONTNAMESIZE, font.len() as u32, font.as_ptr() as *const u32);
            bars()[b].bubble = nb;
            WinSetWindowPos(nb, HWND_TOP, x, y, w, h, SWP_SHOW | SWP_ZORDER | SWP_MOVE | SWP_SIZE);
        } else {
            WinSetWindowPos(bub, HWND_TOP, x, y, w, h, SWP_SHOW | SWP_ZORDER | SWP_MOVE | SWP_SIZE);
            WinInvalidateRect(bub, core::ptr::null(), 0);
        }
    }
}

fn hide_bubble(b: usize) {
    let bub = core::mem::replace(&mut bars()[b].bubble, 0);
    if bub != 0 {
        unsafe { WinDestroyWindow(bub) };
    }
}

extern "C" fn bubble_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            unsafe {
                let hps = WinBeginPaint(h, 0, &mut r);
                GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
                let mut all = RectL::default();
                WinQueryWindowRect(h, &mut all);
                fill(hps, all, 0);
                fill(hps, rect(all.l + 1, all.b + 1, all.r - 1, all.t - 1), 0xffffe0);
                let t = (*(&raw const BUBBLE_TEXT)).clone();
                text(hps, &t, rect(all.l + 4, all.b, all.r - 4, all.t), 0, 0);
                WinEndPaint(hps);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

// ---- The timer ----

fn tick(b: usize) {
    let o = bars()[b].obj;
    let h = bars()[b].hwnd;
    // The timer counts while it runs (in every clock mode, as IBM's).
    if bars()[b].timer_on {
        let t = now();
        let at = core::mem::replace(&mut bars()[b].timer_at, t);
        bars()[b].timer_ms = bars()[b].timer_ms.wrapping_add(t.wrapping_sub(at));
    }
    // The clock: repainted when its text changes.
    let c = clock_now(b);
    if c != bars()[b].mon.clock {
        bars()[b].mon.clock = c;
        invalidate_item(b, Hit::Clock);
    }
    poll(b, false);
    let p = pointer_pos();
    let s = window_pos(h);
    let over = p.x >= s.x && p.x < s.x + s.cx && p.y >= s.y && p.y < s.y + s.cy;
    let busy = bars()[b].pressed.is_some() || bars()[b].menu_up || unsafe { WinQueryCapture(HWND_DESKTOP) } != 0 || crate::desktop::locked();
    // Bubble help.
    if over && !busy && wd(o).flags & F_NOBUBBLE == 0 {
        let mut q = [PointL { x: p.x, y: p.y }];
        unsafe { WinMapWindowPoints(HWND_DESKTOP, h, q.as_mut_ptr(), 1) };
        let it = hit(b, q[0].x, q[0].y);
        if it != bars()[b].rest {
            bars()[b].rest = it;
            bars()[b].rest_ms = 0;
            hide_bubble(b);
        } else if let Some(it) = it {
            bars()[b].rest_ms += TICK;
            if bars()[b].rest_ms >= BUBBLE_AFTER && bars()[b].bubble == 0 {
                show_bubble(b, it);
            }
        }
    } else if bars()[b].rest.is_some() || bars()[b].bubble != 0 {
        bars()[b].rest = None;
        hide_bubble(b);
    }
    // Auto-hide.
    let (shown_y, hidden_y) = place(o);
    if wd(o).flags & F_AUTOHIDE != 0 {
        if bars()[b].hidden {
            let (sw, _) = screen();
            let edge = if hidden_y < shown_y { p.y < STRIP } else { p.y >= hidden_y };
            if edge && p.x >= 0 && p.x < sw {
                bars()[b].hidden = false;
                bars()[b].idle = 0;
                unsafe { WinSetWindowPos(h, HWND_TOP, 0, shown_y, 0, 0, SWP_MOVE | SWP_ZORDER) };
            }
        } else if over || busy {
            bars()[b].idle = 0;
        } else {
            bars()[b].idle += TICK;
            if bars()[b].idle >= HIDE_AFTER {
                bars()[b].hidden = true;
                hide_bubble(b);
                unsafe { WinSetWindowPos(h, 0, 0, hidden_y, 0, 0, SWP_MOVE) };
            }
        }
    }
    // On top of the other windows (not over its own menus or the lockup).
    if wd(o).flags & F_NOTONTOP == 0 && !busy && unsafe { WinQueryWindow(HWND_DESKTOP, QW_TOP) } != h {
        let bub = bars()[b].bubble;
        unsafe {
            WinSetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER);
            if bub != 0 {
                WinSetWindowPos(bub, HWND_TOP, 0, 0, 0, 0, SWP_ZORDER);
            }
        }
    }
}

/// What bar `b`'s clock shows now.
fn clock_now(b: usize) -> Vec<u8> {
    match wd(bars()[b].obj).date {
        CLOCK_TIMER => timer_text(bars()[b].timer_ms),
        m => clock_text(m == CLOCK_DATE),
    }
}

/// The timer's text: hours, minutes and seconds counted, with the
/// country's time separator.
fn timer_text(ms: u32) -> Vec<u8> {
    let n = crate::country::load();
    let s = ms / 1000;
    let mut out = alloc::format!("{}", s / 3600).into_bytes();
    out.extend_from_slice(&n.s_time);
    out.extend_from_slice(alloc::format!("{:02}", s / 60 % 60).as_bytes());
    out.extend_from_slice(&n.s_time);
    out.extend_from_slice(alloc::format!("{:02}", s % 60).as_bytes());
    out
}

/// The clock's text: the time (hours and minutes), or the date, as the
/// country conventions write them.
fn clock_text(date: bool) -> Vec<u8> {
    let mut dt = os2::DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    let n = crate::country::load();
    if date {
        return crate::country::format_date(&n, dt.year as u32, dt.month as u32, dt.day as u32);
    }
    let h = dt.hours as u32;
    let mut out = Vec::new();
    if n.time == 0 {
        let h12 = if h % 12 == 0 { 12 } else { h % 12 };
        out.extend_from_slice(alloc::format!("{h12}").as_bytes());
    } else {
        out.extend_from_slice(alloc::format!("{h:02}").as_bytes());
    }
    out.extend_from_slice(&n.s_time);
    out.extend_from_slice(alloc::format!("{:02}", dt.minutes).as_bytes());
    if n.time == 0 {
        out.push(b' ');
        out.extend_from_slice(if h < 12 { &n.s1159 } else { &n.s2359 });
    }
    out
}

// ---- Menus ----

fn menu_title(x: usize) -> Vec<u8> {
    let mut t: Vec<u8> = obj::title(x).into_iter().filter(|&c| c != b'~').map(|c| if c == b'\n' || c == b'\r' { b' ' } else { c }).collect();
    t.truncate(60);
    t
}

/// A folder's objects (populated first), at most `max`.
fn folder_objects(f: usize, max: usize) -> Vec<usize> {
    wps::call("WPFolder", m::WPFolder::wpPopulate, &[f, 0, 0, 0]);
    let mut v = Vec::new();
    // QC_FIRST, QC_NEXT
    let mut x = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, 0, 0]);
    while x != 0 && v.len() < max {
        v.push(x);
        x = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[f, x, 1]);
    }
    v
}

/// Folders a menu lists one level into (the others open).
fn cascades(x: usize) -> bool {
    wps::is(x, "WPFolder") && !wps::is(x, "WPDrives") && !wps::is(x, "WPNetwork") && !wps::is(x, "WPDesktop")
}

/// Pop up a menu of folder `f`'s objects above (or below) item `it`.
fn folder_menu(b: usize, f: usize, it: Hit) {
    bars()[b].list.clear();
    let mut titles: Vec<(Vec<u8>, u16, Option<Vec<(Vec<u8>, u16)>>)> = Vec::new();
    let mut objs = folder_objects(f, 60);
    objs.sort_by_key(|&x| menu_title(x).to_ascii_uppercase());
    for x in objs {
        let id = (CMD_LIST as usize + bars()[b].list.len()) as u16;
        bars()[b].list.push(x);
        let sub = if cascades(x) {
            let mut inner = folder_objects(x, 40);
            inner.sort_by_key(|&y| menu_title(y).to_ascii_uppercase());
            let mut v = Vec::new();
            for y in inner {
                let id = (CMD_LIST as usize + bars()[b].list.len()) as u16;
                bars()[b].list.push(y);
                v.push((menu_title(y), id));
            }
            Some(v)
        } else {
            None
        };
        titles.push((menu_title(x), id, sub));
    }
    let mut items: Vec<Menu> = Vec::new();
    // Folders: the folder itself first in its submenu (it opens).
    let opens: Vec<Vec<u8>> = titles.iter().map(|(t, _, _)| [&b"Open "[..], t].concat()).collect();
    for (k, (t, id, sub)) in titles.iter().enumerate() {
        match sub {
            Some(v) => {
                let mut s = alloc::vec![Menu::Item(*id, &opens[k], true), Menu::Sep];
                for (st, sid) in v {
                    s.push(Menu::Item(*sid, st, true));
                }
                if v.is_empty() {
                    s.pop();
                }
                items.push(Menu::Sub(t, s));
            }
            None => items.push(Menu::Item(*id, t, true)),
        }
    }
    if items.is_empty() {
        items.push(Menu::Check(0xffff, b"(empty)", false));
    }
    popup(b, &items, it);
}

/// Pop up `items` at item `it`'s edge away from the screen's.
fn popup(b: usize, items: &[Menu], it: Hit) {
    let menu = popup_menu(items);
    if menu == 0 {
        return;
    }
    let o = bars()[b].obj;
    let r = item_rect(b, it);
    let mut p = [PointL { x: r.l, y: if wd(o).flags & F_BOTTOM != 0 { r.t } else { r.b } }];
    unsafe { WinMapWindowPoints(bars()[b].hwnd, HWND_DESKTOP, p.as_mut_ptr(), 1) };
    // Below a bar at the top: the menu's top at the bar's bottom.
    if wd(o).flags & F_BOTTOM == 0 {
        let s = window_pos(menu);
        p[0].y -= s.cy.max(0);
    }
    hide_bubble(b);
    bars()[b].menu_up = true;
    unsafe { WinPopupMenu(HWND_DESKTOP, bars()[b].hwnd, menu, p[0].x, p[0].y.max(0), 0, PU_HCONSTRAIN | PU_VCONSTRAIN | PU_KEYBOARD | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2) };
}

/// The window list: the switch list's visible, jumpable entries.
fn window_menu(b: usize) {
    let hab = hab();
    let n = unsafe { WinQuerySwitchList(hab, core::ptr::null_mut(), 0) } as usize;
    let cb = 4 + n.max(1) * 100;
    let mut buf = alloc::vec![0u8; cb];
    let n = unsafe { WinQuerySwitchList(hab, buf.as_mut_ptr(), cb as u32) } as usize;
    let rd = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    bars()[b].switch.clear();
    let mut names = Vec::new();
    for i in 0..n {
        // SWENTRY: hswitch, then SWCNTRL: hwnd, hwndIcon, hprog, pid,
        // session, uchVisibility, fbJump, szSwtitle[64], bProgType.
        let e = 4 + i * 100;
        if e + 100 > buf.len() {
            break;
        }
        let (hsw, vis, jump) = (rd(e), rd(e + 24) & 0xff, rd(e + 28));
        if vis & 4 == 0 || jump & 2 == 0 {
            continue;
        }
        let t = &buf[e + 32..e + 96];
        let t: Vec<u8> = t.iter().copied().take_while(|&c| c != 0).filter(|&c| c != b'~').map(|c| if c < b' ' { b' ' } else { c }).collect();
        bars()[b].switch.push(hsw);
        names.push(t);
    }
    let mut items: Vec<Menu> = names.iter().enumerate().map(|(i, t)| Menu::Item((CMD_SWITCH as usize + i) as u16, t, true)).collect();
    if items.is_empty() {
        items.push(Menu::Check(0xffff, b"(no windows)", false));
    }
    popup(b, &items, Hit::WinList);
}

/// The timer's options: Start or Stop, and Reset.
fn timer_menu(b: usize) {
    let items = [
        if bars()[b].timer_on { Menu::Item(CMD_TSTOP as u16, b"Stop timer", true) } else { Menu::Item(CMD_TSTART as u16, b"Start timer", true) },
        Menu::Item(CMD_TRESET as u16, b"Reset timer", true),
    ];
    popup(b, &items, Hit::Timer);
}

/// An environment variable is set (IBM's tests only that it is there).
fn env_set(name: &[u8]) -> bool {
    let mut v: *const u8 = core::ptr::null();
    unsafe { os2::DosScanEnv(name.as_ptr(), &mut v) == 0 }
}

/// IBM's process list: every process, by its program's file name, with
/// the title of its window list entry (if it has one) after it.
fn process_menu(b: usize) {
    let mut size = 64 * 1024;
    let buf = loop {
        let mut buf = alloc::vec![0u8; size];
        // QS_PROCESS | QS_MTE
        match unsafe { DosQuerySysState(5, 0, 0, 0, buf.as_mut_ptr(), size as u32) } {
            0 => break buf,
            111 if size < 1024 * 1024 => size *= 2,
            _ => return,
        }
    };
    let base = buf.as_ptr() as usize;
    let end = base + buf.len();
    let ok = |a: usize, n: usize| a >= base && a + n <= end;
    let u16_at = |a: usize| if ok(a, 2) { unsafe { (a as *const u16).read_unaligned() } } else { 0 };
    let u32_at = |a: usize| if ok(a, 4) { unsafe { (a as *const u32).read_unaligned() } } else { 0 };
    // Modules: handle -> full name.
    let mut mods: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut m = u32_at(base + 20) as usize;
    while m != 0 && ok(m, 24) && mods.len() < 4096 {
        let name = u32_at(m + 20) as usize;
        let mut v = Vec::new();
        let mut a = name;
        while ok(a, 1) && v.len() < 260 {
            let c = unsafe { *(a as *const u8) };
            if c == 0 {
                break;
            }
            v.push(c);
            a += 1;
        }
        mods.push((u16_at(m + 4), v));
        m = u32_at(m) as usize;
    }
    // Window list titles by process.
    let hab = hab();
    let n = unsafe { WinQuerySwitchList(hab, core::ptr::null_mut(), 0) } as usize;
    let cb = 4 + n.max(1) * 100;
    let mut sw = alloc::vec![0u8; cb];
    let n = (unsafe { WinQuerySwitchList(hab, sw.as_mut_ptr(), cb as u32) } as usize).min((cb - 4) / 100);
    let title_of = |pid: u32| -> Option<Vec<u8>> {
        (0..n).map(|i| 4 + i * 100).find(|&e| u32::from_le_bytes([sw[e + 16], sw[e + 17], sw[e + 18], sw[e + 19]]) == pid).map(|e| {
            sw[e + 32..e + 96].iter().copied().take_while(|&c| c != 0).filter(|&c| c != b'~').map(|c| if c < b' ' { b' ' } else { c }).collect()
        })
    };
    bars()[b].kill.clear();
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut p = u32_at(base + 4) as usize;
    while p != 0 && ok(p, 28) && u32_at(p) == 1 && names.len() < 0x1000 {
        let pid = u16_at(p + 8) as u32;
        let hmte = u16_at(p + 24);
        if let Some((_, full)) = mods.iter().find(|(h, _)| *h == hmte) {
            let file = full.rsplit(|&c| c == b'\\').next().unwrap_or(full);
            let mut t: Vec<u8> = file.iter().copied().filter(|&c| c != b'~').collect();
            if let Some(title) = title_of(pid) {
                t.extend_from_slice(b" (");
                t.extend_from_slice(&title);
                t.push(b')');
            }
            t.truncate(80);
            bars()[b].kill.push(pid);
            names.push(t);
        }
        // The next process record follows its thread records.
        p = u32_at(p + 4) as usize + u16_at(p + 26) as usize * 28;
    }
    let items: Vec<Menu> = names.iter().enumerate().map(|(i, t)| Menu::Item((CMD_KILL as usize + i) as u16, t, true)).collect();
    popup(b, &items, Hit::WinList);
}

/// End process `pid` from the process list, once confirmed.
fn kill(b: usize, pid: u32) {
    if !(env_set(b"KILLCONFIRMDISABLED\0") || env_set(b"SCKILLCONFIRMDISABLED\0"))
        && message(bars()[b].hwnd, b"Are you sure that you want to kill this process?", b"WarpCenter", MB_YESNO | MB_WARNING) != MBID_YES
    {
        return;
    }
    // DKP_PROCESS
    unsafe { os2::DosKillProcess(1, pid) };
}

fn tray_menu(b: usize) {
    let o = bars()[b].obj;
    let names: Vec<Vec<u8>> = trays(o).iter().map(|t| t.name.iter().copied().filter(|&c| c != b'~').collect()).collect();
    let cur = current(o);
    let mut items: Vec<Menu> = names.iter().enumerate().map(|(i, t)| Menu::Check((CMD_TRAY as usize + i) as u16, t, i == cur)).collect();
    items.push(Menu::Sep);
    items.push(Menu::Item(CMD_ADDTRAY as u16, b"~Add tray", trays(o).len() < MAX_TRAYS));
    items.push(Menu::Item(CMD_RENAMETRAY as u16, b"~Rename tray...", true));
    items.push(Menu::Item(CMD_DELETETRAY as u16, b"~Delete tray", trays(o).len() > 1));
    popup(b, &items, Hit::TrayButton);
}

fn context_menu(b: usize, x: i32, y: i32) {
    let o = bars()[b].obj;
    let h = hit(b, x, y);
    bars()[b].menu_on = match h {
        Some(Hit::Tray(i)) => Some(i),
        _ => None,
    };
    let names: Vec<Vec<u8>> = (0..26).map(mode_name).collect();
    let mut items: Vec<Menu> = Vec::new();
    if bars()[b].menu_on.is_some() {
        items.push(Menu::Item(CMD_OPEN as u16, b"~Open", true));
        items.push(Menu::Item(CMD_REMOVE as u16, b"Remove from ~tray", true));
        items.push(Menu::Sep);
    }
    if h == Some(Hit::Monitor) {
        let mut mons = Vec::new();
        for m in 0..26 {
            if available(b, m) {
                mons.push(Menu::Check((CMD_MON + m) as u16, &names[m as usize], m == wd(o).mode));
            }
        }
        items.push(Menu::SubId(CMD_MONITOR as u16, b"~Monitor", mons));
        items.push(Menu::Sep);
    }
    items.push(Menu::Item(CMD_ADDTRAY as u16, b"~Add tray", trays(o).len() < MAX_TRAYS));
    items.push(Menu::Item(CMD_RENAMETRAY as u16, b"~Rename tray...", true));
    items.push(Menu::Item(CMD_DELETETRAY as u16, b"~Delete tray", trays(o).len() > 1));
    items.push(Menu::Sep);
    items.push(Menu::Item(CMD_SETTINGS as u16, b"~Properties", true));
    items.push(Menu::Item(CMD_CLOSE as u16, b"~Close WarpCenter", true));
    let menu = popup_menu(&items);
    if menu != 0 {
        hide_bubble(b);
        bars()[b].menu_up = true;
        let p = pointer_pos();
        unsafe { WinPopupMenu(HWND_DESKTOP, bars()[b].hwnd, menu, p.x, p.y, 0, PU_HCONSTRAIN | PU_VCONSTRAIN | PU_KEYBOARD | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2) };
    }
}

// ---- Actions ----

fn act(b: usize, it: Hit) {
    let o = bars()[b].obj;
    match it {
        Hit::Desktop => {
            let d = crate::fs::desktop();
            if d != 0 {
                folder_menu(b, d, it);
            }
        }
        Hit::Help => {
            let a = wps::handle_of_id(b"<WP_ASSISTANCE>").map(wps::object_from_handle).unwrap_or(0);
            if a != 0 {
                folder_menu(b, a, it);
            }
        }
        Hit::WinList => {
            if (env_set(b"KILLFEATUREENABLED\0") || env_set(b"SCKILLFEATUREENABLED\0")) && unsafe { WinGetKeyState(HWND_DESKTOP, VK_CTRL) } & 0x8000 != 0 {
                process_menu(b);
            } else {
                window_menu(b);
            }
        }
        Hit::Timer => timer_menu(b),
        Hit::Lockup => {
            crate::desktop::WinLockupSystem(0);
        }
        Hit::Find => {
            let d = crate::fs::desktop();
            if d != 0 {
                crate::find::ask(d);
            }
        }
        Hit::Shutdown => {
            if wd(o).flags & F_NOCONFIRM != 0 {
                crate::shutdown::begin(false);
            } else {
                crate::shutdown::ask();
            }
        }
        Hit::Monitor => {
            let next = next_mode(b, wd(o).mode);
            set_mode(b, next);
        }
        Hit::TrayButton => tray_menu(b),
        Hit::Tray(i) => {
            if let Some(&x) = tray_objects(o).get(i) {
                obj::call(m::WPObject::wpViewObject, &[x, 0, OPEN_DEFAULT as usize, 0]);
            }
        }
        Hit::Clock => {
            // Time, date, timer (the timer's button comes and goes).
            wd(o).date = if wd(o).date >= CLOCK_TIMER { CLOCK_TIME } else { wd(o).date + 1 };
            let (sw, _) = screen();
            bars()[b].items = layout(o, sw);
            bars()[b].mon.clock = clock_now(b);
            hide_bubble(b);
            bars()[b].rest_ms = 0;
            unsafe { WinInvalidateRect(bars()[b].hwnd, core::ptr::null(), 0) };
            obj::call(m::WPObject::wpSaveDeferred, &[o]);
        }
    }
}

fn set_mode(b: usize, mode: u32) {
    let o = bars()[b].obj;
    wd(o).mode = mode;
    bars()[b].mon.cpu.clear();
    bars()[b].mon.cpu_last.clear();
    poll(b, true);
    hide_bubble(b);
    bars()[b].rest_ms = 0;
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
}

fn select_tray(o: usize, n: usize) {
    if n < trays(o).len() {
        wd(o).current = n as u32;
        changed(o);
    }
}

fn remove_tray_object(o: usize, i: usize) {
    let objs = tray_objects(o);
    let Some(&x) = objs.get(i) else { return };
    let h = obj::handle(x);
    let n = current(o);
    trays(o)[n].objs.retain(|&k| k != h);
    changed(o);
}

fn delete_tray(o: usize, n: usize) {
    if trays(o).len() <= 1 || n >= trays(o).len() {
        return;
    }
    trays(o).remove(n);
    if wd(o).current as usize >= trays(o).len() || wd(o).current as usize > n {
        wd(o).current = wd(o).current.saturating_sub(1);
    }
    changed(o);
}

/// Ask for a tray's new name (None: cancelled).
fn ask_name(title: &[u8], old: &[u8]) -> Option<Vec<u8>> {
    let tpl = template(dialog_frame(title, 180, 56), alloc::vec![
        item(WC_STATIC, b"~Name:", WS_VISIBLE | 0x0001, (6, 36, 30, 8), 0xffff),
        // ES_MARGIN | ES_AUTOSCROLL
        item(WC_ENTRYFIELD, old, WS_VISIBLE | WS_TABSTOP | 0x0008 | 0x0004, (40, 36, 130, 8), 10),
        item(WC_BUTTON, b"OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 6, 50, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (62, 6, 50, 14), DID_CANCEL as u16),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, HWND_DESKTOP, name_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return None;
    }
    center(dlg);
    unsafe { WinSendDlgItemMsg(dlg, 10, EM_SETTEXTLIMIT, 33, 0) };
    let r = unsafe { WinProcessDlg(dlg) };
    let t = item_text(dlg, 10);
    unsafe { WinDestroyWindow(dlg) };
    (r == DID_OK).then_some(t)
}

extern "C" fn name_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    unsafe { WinDefDlgProc(h, msg, a, b) }
}

fn rename_tray(o: usize, n: usize) {
    let Some(old) = trays(o).get(n).map(|t| t.name.clone()) else { return };
    if let Some(new) = ask_name(b"Rename tray", &old) {
        let new: Vec<u8> = new.into_iter().filter(|&c| c >= b' ' && c != 1 && c != 2).take(33).collect();
        if !new.trim_ascii().is_empty() {
            trays(o)[n].name = new;
            changed(o);
        }
    }
}

fn command(b: usize, id: u32) {
    let o = bars()[b].obj;
    match id {
        CMD_OPEN => {
            if let Some(i) = bars()[b].menu_on {
                if let Some(&x) = tray_objects(o).get(i) {
                    obj::call(m::WPObject::wpViewObject, &[x, 0, OPEN_DEFAULT as usize, 0]);
                }
            }
        }
        CMD_REMOVE => {
            if let Some(i) = bars()[b].menu_on {
                remove_tray_object(o, i);
            }
        }
        CMD_SETTINGS => {
            obj::call(m::WPObject::wpViewObject, &[o, 0, OPEN_SETTINGS as usize, 0]);
        }
        CMD_CLOSE => {
            close_bar(o);
            set_closed(true);
        }
        CMD_ADDTRAY => {
            let name = tray_name(trays(o).len());
            if let Some(n) = add_tray(o, &name) {
                select_tray(o, n);
            }
        }
        CMD_TSTART => {
            bars()[b].timer_on = true;
            bars()[b].timer_at = now();
        }
        CMD_TSTOP => {
            bars()[b].timer_on = false;
            invalidate_item(b, Hit::Clock);
        }
        CMD_TRESET => {
            bars()[b].timer_ms = 0;
            bars()[b].timer_at = now();
            bars()[b].mon.clock = clock_now(b);
            invalidate_item(b, Hit::Clock);
        }
        CMD_RENAMETRAY => rename_tray(o, current(o)),
        CMD_DELETETRAY => delete_tray(o, current(o)),
        id if (CMD_TRAY..CMD_TRAY + MAX_TRAYS as u32).contains(&id) => select_tray(o, (id - CMD_TRAY) as usize),
        id if (CMD_MON..CMD_MON + 26).contains(&id) => set_mode(b, id - CMD_MON),
        id if (CMD_LIST..CMD_SWITCH).contains(&id) => {
            if let Some(&x) = bars()[b].list.get((id - CMD_LIST) as usize) {
                obj::call(m::WPObject::wpViewObject, &[x, 0, OPEN_DEFAULT as usize, 0]);
            }
        }
        id if id >= CMD_KILL => {
            if let Some(&pid) = bars()[b].kill.get((id - CMD_KILL) as usize) {
                kill(b, pid);
            }
        }
        id if id >= CMD_SWITCH => {
            if let Some(&hsw) = bars()[b].switch.get((id - CMD_SWITCH) as usize) {
                unsafe { WinSwitchToProgram(hsw) };
            }
        }
        _ => {}
    }
}

/// Objects dropped on the tray: they join it.
fn dropped(b: usize, pdinfo: u32) -> u32 {
    let list = crate::dnd::items(pdinfo);
    let objs: Vec<usize> = list.iter().map(|&it| crate::dnd::item_object(it)).collect();
    if objs.is_empty() || objs.contains(&0) {
        crate::dnd::finish_drop(pdinfo, &list, false);
        return 0;
    }
    let o = bars()[b].obj;
    let hs: Vec<u32> = objs.iter().map(|&x| obj::handle(x)).collect();
    let n = current(o);
    add_objects(o, n, &hs);
    crate::dnd::finish_drop(pdinfo, &list, true);
    changed(o);
    0
}

fn pos(v: u32) -> (i32, i32) {
    ((v & 0xffff) as i16 as i32, (v >> 16) as i16 as i32)
}

const WM_MENUEND: u32 = 0x0035;
const DM_DRAGOVER: u32 = 0x032e;
const DM_DROP: u32 = 0x032f;

extern "C" fn wc_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    let Some(w) = find_bar(h) else { return unsafe { WinDefWindowProc(h, msg, a, b) } };
    match msg {
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN => {
            let (x, y) = pos(a);
            hide_bubble(w);
            if let Some(it) = hit(w, x, y) {
                bars()[w].pressed = Some(it);
                unsafe { WinSetCapture(HWND_DESKTOP, h) };
                invalidate_item(w, it);
            }
            1
        }
        WM_BUTTON1UP => {
            let (x, y) = pos(a);
            let was = bars()[w].pressed.take();
            unsafe { WinSetCapture(HWND_DESKTOP, 0) };
            if let Some(it) = was {
                invalidate_item(w, it);
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
        WM_MENUEND => {
            bars()[w].menu_up = false;
            unsafe { WinDefWindowProc(h, msg, a, b) }
        }
        WM_COMMAND => {
            bars()[w].menu_up = false;
            command(w, a & 0xffff);
            0
        }
        WM_TIMER if a == TID_TICK => {
            tick(w);
            0
        }
        DM_DRAGOVER => {
            let pdinfo = a;
            if unsafe { DrgAccessDraginfo(pdinfo) } == 0 {
                return mp(DOR_NEVERDROP, 0);
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
            let o = bars()[w].obj;
            close_bar(o);
            set_closed(true);
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

// ---- Settings pages ----

const ID_DISPLAY: u16 = 1400; // ..
const ID_TOP: u16 = 1410;
const ID_BOTTOM: u16 = 1411;
const ID_SMALL: u16 = 1412;
const ID_LARGE: u16 = 1413;
const ID_MON: u16 = 1420; // .. 1422
const ID_TRAYS: u16 = 1430;
const ID_TADD: u16 = 1431;
const ID_TRENAME: u16 = 1432;
const ID_TDELETE: u16 = 1433;
const ID_UNDO: u16 = 1440;
const ID_DEFAULT: u16 = 1441;

/// The Display page's check boxes: (text, flag, flag set means checked).
const OPTS: [(&[u8], u32, bool); 4] = [
    (b"Show on top of ~maximized windows", F_NOTONTOP, false),
    (b"~Hide until the pointer reaches its edge", F_AUTOHIDE, true),
    (b"Show ~bubble help", F_NOBUBBLE, false),
    (b"~Confirm shutdown", F_NOCONFIRM, false),
];
const MONS: [(&[u8], u32); 3] = [(b"~Battery", M_BATTERY), (b"~Processor activity", M_CPU), (b"~Fixed disks", M_DISKS)];

fn page_obj(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

fn display_init(h: u32, o: usize) {
    let f = wd(o).flags;
    for (i, &(_, bit, set)) in OPTS.iter().enumerate() {
        let on = (f & bit != 0) == set;
        unsafe { WinSendDlgItemMsg(h, (ID_DISPLAY + i as u16) as u32, BM_SETCHECK, on as u32, 0) };
    }
    let r = |id: u16, on: bool| unsafe { WinSendDlgItemMsg(h, id as u32, BM_SETCHECK, on as u32, 0) };
    r(ID_TOP, f & F_BOTTOM == 0);
    r(ID_BOTTOM, f & F_BOTTOM != 0);
    r(ID_SMALL, f & F_LARGE == 0);
    r(ID_LARGE, f & F_LARGE != 0);
}

extern "C" fn display_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            // Undo goes back to what the page started with.
            unsafe { WinSetWindowULong(h, 4, wd(b as usize).flags) };
            display_init(h, b as usize);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = page_obj(h);
            let id = (a & 0xffff) as u16;
            if o == 0 {
                return 0;
            }
            let d = wd(o);
            if (ID_DISPLAY..ID_DISPLAY + OPTS.len() as u16).contains(&id) {
                let (_, bit, set) = OPTS[(id - ID_DISPLAY) as usize];
                let on = unsafe { WinSendDlgItemMsg(h, id as u32, BM_QUERYCHECK, 0, 0) } != 0;
                d.flags = if on == set { d.flags | bit } else { d.flags & !bit };
            } else {
                match id {
                    ID_TOP => d.flags &= !F_BOTTOM,
                    ID_BOTTOM => d.flags |= F_BOTTOM,
                    ID_SMALL => d.flags &= !F_LARGE,
                    ID_LARGE => d.flags |= F_LARGE,
                    _ => return 0,
                }
            }
            changed(o);
            0
        }
        WM_COMMAND => {
            let o = page_obj(h);
            match (a & 0xffff) as u16 {
                ID_UNDO if o != 0 => {
                    wd(o).flags = unsafe { WinQueryWindowULong(h, 4) };
                    display_init(h, o);
                    changed(o);
                }
                ID_DEFAULT if o != 0 => {
                    wd(o).flags &= !(F_BOTTOM | F_LARGE | F_NOTONTOP | F_AUTOHIDE | F_NOBUBBLE | F_NOCONFIRM);
                    display_init(h, o);
                    changed(o);
                }
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn page_buttons() -> [Item; 2] {
    [
        item(WC_BUTTON, b"~Undo", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (6, 4, 50, 14), ID_UNDO),
        item(WC_BUTTON, b"~Default", WS_VISIBLE | WS_TABSTOP, (60, 4, 50, 14), ID_DEFAULT),
    ]
}

extern "C" fn wc_add_display_page(o: usize, nb: u32) -> u32 {
    let mut v = alloc::vec![book::label(b"The bar:", (6, 110, 200))];
    for (i, &(t, _, _)) in OPTS.iter().enumerate() {
        // BS_AUTOCHECKBOX
        v.push(item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (10, 98 - i as i16 * 11, 220, 10), ID_DISPLAY + i as u16));
    }
    // BS_AUTORADIOBUTTON
    let radio = |t: &[u8], x: i16, y: i16, id: u16, first: bool| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (x, y, 100, 10), id);
    v.push(book::label(b"Position:", (6, 50, 100)));
    v.push(radio(b"~Top of the screen", 10, 39, ID_TOP, true));
    v.push(radio(b"B~ottom of the screen", 10, 28, ID_BOTTOM, false));
    v.push(book::label(b"Icons:", (120, 50, 100)));
    v.push(radio(b"~Small", 124, 39, ID_SMALL, true));
    v.push(radio(b"~Large", 124, 28, ID_LARGE, false));
    v.extend(page_buttons());
    let tpl = template(book::page_frame(), v);
    let page = book::make_page(nb, display_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, b"~Display\0")
}

extern "C" fn monitors_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    let init = |h: u32, o: usize| {
        for (i, &(_, bit)) in MONS.iter().enumerate() {
            unsafe { WinSendDlgItemMsg(h, (ID_MON + i as u16) as u32, BM_SETCHECK, (wd(o).off & bit == 0) as u32, 0) };
        }
    };
    match msg {
        WM_INITDLG => {
            unsafe {
                WinSetWindowULong(h, QWL_USER, b);
                WinSetWindowULong(h, 4, wd(b as usize).off);
            }
            init(h, b as usize);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = page_obj(h);
            let id = (a & 0xffff) as u16;
            if o != 0 && (ID_MON..ID_MON + MONS.len() as u16).contains(&id) {
                let bit = MONS[(id - ID_MON) as usize].1;
                let on = unsafe { WinSendDlgItemMsg(h, id as u32, BM_QUERYCHECK, 0, 0) } != 0;
                let d = wd(o);
                d.off = if on { d.off & !bit } else { d.off | bit };
                // The processors' monitor stays when all are off.
                if d.off & M_ALL == M_ALL {
                    d.off &= !M_CPU;
                    init(h, o);
                }
                changed(o);
            }
            0
        }
        WM_COMMAND => {
            let o = page_obj(h);
            match (a & 0xffff) as u16 {
                ID_UNDO if o != 0 => {
                    wd(o).off = unsafe { WinQueryWindowULong(h, 4) };
                    init(h, o);
                    changed(o);
                }
                ID_DEFAULT if o != 0 => {
                    wd(o).off = 0;
                    init(h, o);
                    changed(o);
                }
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wc_add_monitors_page(o: usize, nb: u32) -> u32 {
    let mut v = alloc::vec![book::label(b"The monitor shows, a click going on to the next:", (6, 110, 226))];
    for (i, &(t, _)) in MONS.iter().enumerate() {
        v.push(item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (10, 96 - i as i16 * 12, 200, 10), ID_MON + i as u16));
    }
    v.push(book::label(b"(the battery when there is one)", (22, 56, 200)));
    v.extend(page_buttons());
    let tpl = template(book::page_frame(), v);
    let page = book::make_page(nb, monitors_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, b"~Monitors\0")
}

fn trays_fill(h: u32, o: usize) {
    unsafe { WinSendDlgItemMsg(h, ID_TRAYS as u32, LM_DELETEALL, 0, 0) };
    for t in trays(o).iter() {
        let s = z(&t.name);
        unsafe { WinSendDlgItemMsg(h, ID_TRAYS as u32, LM_INSERTITEM, LIT_END, s.as_ptr() as u32) };
    }
    unsafe { WinSendDlgItemMsg(h, ID_TRAYS as u32, LM_SELECTITEM, current(o) as u32, 1) };
}

extern "C" fn trays_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            trays_fill(h, b as usize);
            0
        }
        WM_COMMAND => {
            let o = page_obj(h);
            if o == 0 {
                return 0;
            }
            let sel = unsafe { WinSendDlgItemMsg(h, ID_TRAYS as u32, LM_QUERYSELECTION, LIT_END, 0) };
            let sel = if sel == LIT_NONE || sel as i32 == -1 { current(o) } else { sel as usize };
            match (a & 0xffff) as u16 {
                ID_TADD => {
                    let name = tray_name(trays(o).len());
                    match ask_name(b"Add tray", &name) {
                        Some(n) => {
                            if let Some(k) = add_tray(o, &n) {
                                wd(o).current = k as u32;
                            }
                        }
                        None => return 0,
                    }
                    changed(o);
                }
                ID_TRENAME => rename_tray(o, sel),
                ID_TDELETE => delete_tray(o, sel),
                _ => return 0,
            }
            trays_fill(h, o);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wc_add_trays_page(o: usize, nb: u32) -> u32 {
    let v = alloc::vec![
        book::label(b"Trays (up to 16; drop objects on the bar to add them):", (6, 110, 226)),
        item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (10, 26, 140, 80), ID_TRAYS),
        item(WC_BUTTON, b"~Add...", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (158, 92, 60, 14), ID_TADD),
        item(WC_BUTTON, b"~Rename...", WS_VISIBLE | WS_TABSTOP, (158, 74, 60, 14), ID_TRENAME),
        item(WC_BUTTON, b"~Delete", WS_VISIBLE | WS_TABSTOP, (158, 56, 60, 14), ID_TDELETE),
    ];
    let tpl = template(book::page_frame(), v);
    let page = book::make_page(nb, trays_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, b"~Trays\0")
}

extern "C" fn wc_add_pages(o: usize, nb: u32) -> u32 {
    // The WPObject pages, not the LaunchPad's.
    som::call_parent(wps::ccd("WPLaunchPad"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    // Inserted first: the last inserted is the first page.
    wc_add_trays_page(o, nb);
    wc_add_monitors_page(o, nb);
    wc_add_display_page(o, nb);
    1
}

methods! {
    __m_wc_setup => wc_setup, 2;
    __m_wc_save_state => wc_save_state, 1;
    __m_wc_restore_state => wc_restore_state, 2;
    __m_wc_uninit => wc_uninit, 1;
    __m_wc_open => wc_open, 4;
    __m_wc_add_pages => wc_add_pages, 2;
}

pub fn impl_warpcenter() -> Impl {
    Impl {
        data: core::mem::size_of::<WcData>() as u32,
        methods: Vec::new(),
        overrides: alloc::vec![
            ("wpSetup", __m_wc_setup as usize),
            ("wpSaveState", __m_wc_save_state as usize),
            ("wpRestoreState", __m_wc_restore_state as usize),
            ("wpUnInitData", __m_wc_uninit as usize),
            ("wpOpen", __m_wc_open as usize),
            ("wpAddSettingsPages", __m_wc_add_pages as usize),
        ],
    }
}
