//! WPCONFIG's System and System Clock objects (IBM's WPSystem and
//! WPClock, rs2's own implementations):
//!
//! - **System** (`<WP_SYSTEM>`): the Window page (where the minimize
//!   button puts windows: the Minimized Window Viewer or the desktop;
//!   whether opening an object shows its open window or makes a new one,
//!   `PM_ControlPanel` ConcurrentView) and the Confirmations page (the
//!   flags of wpQueryConfirmations, kept as PMWP reads them; delete.rs).
//! - **System Clock** (`<WP_CLOCK>`): opening it shows a clock window,
//!   analog or digital, with the time, the date or both; its View page
//!   chooses which (kept with the object), its Date/Time page sets the
//!   computer's date and time.

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::delete::{self, CONFIRM_ACTION, CONFIRM_DELETE, CONFIRM_DELETEFOLDER, CONFIRM_PROGRESS, CONFIRM_RENAMEFILESWITHEXT};
use crate::methods;
use crate::obj::{self, OPEN_SETTINGS};
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio, spin, spin_init, spin_value};
use crate::som;
use crate::util::{parse_dec, prf_read, prf_write, z, HINI_USER};
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMGPI.404"]
    fn GpiMove(hps: u32, p: *const PointL) -> u32;
    #[link_name = "PMGPI.398"]
    fn GpiLine(hps: u32, p: *const PointL) -> i32;
    #[link_name = "PMGPI.393"]
    fn GpiFullArc(hps: u32, control: u32, mult: u32) -> i32;
    #[link_name = "PMGPI.517"]
    fn GpiSetColor(hps: u32, c: i32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}

// ---- Object open behavior ----

/// `PM_ControlPanel` ConcurrentView: opening an object makes a new window
/// even when one is open.
pub fn concurrent_view() -> bool {
    prf_read(HINI_USER, b"PM_ControlPanel", b"ConcurrentView").is_some_and(|v| parse_dec(v.strip_suffix(&[0]).unwrap_or(&v)) == Some(1))
}

fn set_concurrent_view(on: bool) {
    prf_write(HINI_USER, b"PM_ControlPanel", b"ConcurrentView", Some(if on { b"1\0" } else { b"0\0" }));
}

// ---- System: the Window page ----

const ID_VIEWER: u16 = 700;
const ID_DESKTOP: u16 = 701;
const ID_EXISTING: u16 = 702;
const ID_NEW: u16 = 703;

extern "C" fn window_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            check(h, if crate::minwin::to_viewer() { ID_VIEWER } else { ID_DESKTOP }, true);
            check(h, if concurrent_view() { ID_NEW } else { ID_EXISTING }, true);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            // BN_CLICKED on a radio button.
            match (a & 0xffff) as u16 {
                ID_VIEWER => crate::minwin::set_to_viewer(true),
                ID_DESKTOP => crate::minwin::set_to_viewer(false),
                ID_EXISTING => set_concurrent_view(false),
                ID_NEW => set_concurrent_view(true),
                _ => {}
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSystemWindowPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, window_page_proc, alloc::vec![
        book::label(b"Minimize button behavior", (6, 106, 220)),
        radio(b"Minimize window to ~viewer", (12, 92, 210), ID_VIEWER, true),
        radio(b"Minimize ~window to desktop", (12, 80, 210), ID_DESKTOP, false),
        book::label(b"Object open behavior", (6, 60, 220)),
        radio(b"Display ~existing window", (12, 46, 210), ID_EXISTING, true),
        radio(b"~Create new window", (12, 34, 210), ID_NEW, false),
    ], b"~Window\0")
}

// ---- System: the Confirmations page ----

/// (check box, flag) as WPCONFIG's page lists them.
const CONFIRMS: [(u16, &[u8], u32); 5] = [
    (710, b"C~onfirm on folder delete", CONFIRM_DELETEFOLDER),
    (711, b"~Confirm on delete", CONFIRM_DELETE),
    (712, b"Confirm on rename of files with ~extensions", CONFIRM_RENAMEFILESWITHEXT),
    (713, b"Confirm on copy, ~move, create shadow", CONFIRM_ACTION),
    (714, b"Di~splay progress indication dialog", CONFIRM_PROGRESS),
];

extern "C" fn confirm_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let c = delete::confirmations();
            for (id, _, f) in CONFIRMS {
                check(h, id, c & f != 0);
            }
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let id = (a & 0xffff) as u16;
            if let Some(&(_, _, f)) = CONFIRMS.iter().find(|c| c.0 == id) {
                delete::set_confirmation(f, checked(h, id));
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSystemConfirmationPage(o: usize, nb: u32) -> u32 {
    // BS_AUTOCHECKBOX
    let items = CONFIRMS.iter().enumerate().map(|(i, &(id, t, _))| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if i == 0 { WS_GROUP } else { 0 } | 0x0001, (10, 100 - 14 * i as i16, 220, 10), id)).collect();
    add_page(o, nb, confirm_page_proc, items, b"~Confirmations\0")
}

extern "C" fn system_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPSystem"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSystemConfirmationPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSystemWindowPage, &[o, nb as usize]);
    1
}

/// rs2's earlier System object (RS2System) shows the same pages.
pub extern "C" fn rs2_system_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("RS2System"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wpAddSystemConfirmationPage(o, nb);
    wpAddSystemWindowPage(o, nb);
    1
}

// ---- The System Clock ----

/// What the clock shows (WPClock key 1).
const SHOW_TIME: u32 = 0x01;
const SHOW_DATE: u32 = 0x02;
const ANALOG: u32 = 0x04;
const SECONDS: u32 = 0x08;
const HOUR_MARKS: u32 = 0x10;
const MINUTE_MARKS: u32 = 0x20;
const DEFAULT_MODE: u32 = SHOW_TIME | SHOW_DATE | ANALOG | SECONDS | HOUR_MARKS | MINUTE_MARKS;

#[repr(C)]
pub struct ClockData {
    mode: u32,
}

fn cd(o: usize) -> &'static mut ClockData {
    som::data(wps::ccd("WPClock"), o)
}

/// Open clock windows: (object, frame, client).
static mut CLOCKS: Vec<(usize, u32, u32)> = Vec::new();
const TID_CLOCK: u32 = 0x5710;
static CLOCK_CLASS: &[u8] = b"RS2Clock\0";

fn now() -> os2::DateTime {
    let mut dt = os2::DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    dt
}

/// sin of `deg` degrees (a polynomial: good to a pixel on a clock).
fn sin_deg(deg: i32) -> f64 {
    let d = deg.rem_euclid(360);
    let (d, sign) = if d > 180 { (d - 180, -1.0) } else { (d, 1.0) };
    let d = if d > 90 { 180 - d } else { d };
    let x = d as f64 * core::f64::consts::PI / 180.0;
    let x2 = x * x;
    sign * x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))))
}

fn at(c: PointL, r: f64, deg: i32) -> PointL {
    PointL { x: c.x + (r * sin_deg(deg)) as i32, y: c.y + (r * sin_deg(deg + 90)) as i32 }
}

fn line(hps: u32, a: PointL, b: PointL, width: i32) {
    for k in -(width / 2)..=(width - 1) / 2 {
        let (dx, dy) = if (b.x - a.x).abs() > (b.y - a.y).abs() { (0, k) } else { (k, 0) };
        unsafe {
            GpiMove(hps, &PointL { x: a.x + dx, y: a.y + dy });
            GpiLine(hps, &PointL { x: b.x + dx, y: b.y + dy });
        }
    }
}

fn two(n: u32) -> [u8; 2] {
    [b'0' + (n / 10 % 10) as u8, b'0' + (n % 10) as u8]
}

fn time_text(dt: &os2::DateTime) -> Vec<u8> {
    [&two(dt.hours as u32)[..], b":", &two(dt.minutes as u32), b":", &two(dt.seconds as u32)].concat()
}

fn date_text(dt: &os2::DateTime) -> Vec<u8> {
    alloc::format!("{:02}-{:02}-{}", dt.month, dt.day, dt.year).into_bytes()
}

// CLR_ indices.
const CLR_BLACK: i32 = -1;
const CLR_WHITE: i32 = -2;
const CLR_BLUE: i32 = 1;
const CLR_RED: i32 = 2;
const CLR_DARKBLUE: i32 = 9;
const CLR_PALEGRAY: i32 = 15;

fn paint(h: u32, mode: u32) {
    let mut r = RectL::default();
    let dt = now();
    unsafe {
        let hps = WinBeginPaint(h, 0, &mut r);
        WinQueryWindowRect(h, &mut r);
        WinFillRect(hps, &r, CLR_PALEGRAY);
        let date_h = if mode & SHOW_DATE != 0 { 16 } else { 0 };
        if mode & ANALOG != 0 && mode & SHOW_TIME != 0 {
            let c = PointL { x: (r.l + r.r) / 2, y: (r.b + date_h + r.t) / 2 };
            let rad = ((r.r - r.l).min(r.t - r.b - date_h) / 2 - 4).max(8);
            GpiMove(hps, &c);
            GpiSetColor(hps, CLR_DARKBLUE);
            // DRO_FILL, then DRO_OUTLINE.
            GpiFullArc(hps, 1, (rad as u32) << 16);
            GpiSetColor(hps, CLR_WHITE);
            GpiFullArc(hps, 1, ((rad - rad / 12 - 1).max(4) as u32) << 16);
            GpiSetColor(hps, CLR_BLACK);
            let face = (rad - rad / 12 - 1).max(4) as f64;
            for k in 0..60 {
                let hour = k % 5 == 0;
                if (hour && mode & HOUR_MARKS == 0) || (!hour && mode & MINUTE_MARKS == 0) {
                    continue;
                }
                let inner = if hour { face * 0.82 } else { face * 0.92 };
                line(hps, at(c, inner, k * 6), at(c, face * 0.97, k * 6), if hour { 2 } else { 1 });
            }
            let (hh, mm, ss) = (dt.hours as i32 % 12, dt.minutes as i32, dt.seconds as i32);
            line(hps, c, at(c, face * 0.5, hh * 30 + mm / 2), 3);
            line(hps, c, at(c, face * 0.78, mm * 6), 2);
            if mode & SECONDS != 0 {
                GpiSetColor(hps, CLR_RED);
                line(hps, c, at(c, face * 0.85, ss * 6), 1);
            }
        } else if mode & SHOW_TIME != 0 {
            let mut tr = RectL { l: r.l, b: r.b + date_h, r: r.r, t: r.t };
            let t = time_text(&dt);
            // DT_CENTER | DT_VCENTER
            WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut tr, CLR_BLUE, CLR_PALEGRAY, 0x0100 | 0x0400);
        }
        if mode & SHOW_DATE != 0 {
            let mut dr = if mode & SHOW_TIME != 0 { RectL { l: r.l, b: r.b, r: r.r, t: r.b + date_h } } else { r };
            let t = date_text(&dt);
            WinDrawText(hps, t.len() as i32, t.as_ptr(), &mut dr, CLR_BLACK, CLR_PALEGRAY, 0x0100 | 0x0400);
        }
        WinEndPaint(hps);
    }
}

fn clock_of_client(h: u32) -> Option<usize> {
    unsafe { CLOCKS.iter().find(|c| c.2 == h).map(|c| c.0) }
}

extern "C" fn clock_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mode = clock_of_client(h).map_or(DEFAULT_MODE, |o| cd(o).mode);
            paint(h, mode);
            0
        }
        WM_TIMER if a == TID_CLOCK => {
            unsafe { WinInvalidateRect(h, core::ptr::null(), 0) };
            0
        }
        WM_SIZE => {
            unsafe { WinInvalidateRect(h, core::ptr::null(), 0) };
            0
        }
        WM_CLOSE => {
            unsafe {
                if let Some(i) = CLOCKS.iter().position(|c| c.2 == h) {
                    let f = CLOCKS.remove(i).1;
                    WinDestroyWindow(f);
                }
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

fn open_clock(o: usize) -> u32 {
    if let Some(&(_, f, _)) = unsafe { CLOCKS.iter().find(|c| c.0 == o) } {
        crate::view::activate(f);
        return f;
    }
    let hab = unsafe { WinQueryAnchorBlock(HWND_DESKTOP) };
    // CS_SIZEREDRAW
    unsafe { WinRegisterClass(hab, CLOCK_CLASS.as_ptr(), clock_proc, 0x0004, 0) };
    let t: Vec<u8> = obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect();
    let t = z(&t);
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLOCK_CLASS.as_ptr(), t.as_ptr(), 0, 0, 1, &mut client) };
    if frame == 0 {
        return 0;
    }
    unsafe { CLOCKS.push((o, frame, client)) };
    send(frame, 0x0047, obj::call(m::WPObject::wpQueryIcon, &[o]) as u32, 0);
    let (sw, sh) = screen();
    unsafe {
        WinSetWindowPos(frame, HWND_TOP, sw - 200, sh - 230, 180, 200, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
        WinStartTimer(hab, client, TID_CLOCK, 1000);
    }
    frame
}

fn repaint_clocks(o: usize) {
    for &(x, _, c) in unsafe { CLOCKS.iter() } {
        if x == o {
            unsafe { WinInvalidateRect(c, core::ptr::null(), 0) };
        }
    }
}

extern "C" fn clock_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd("WPClock"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32;
    }
    open_clock(o)
}

extern "C" fn clock_init(o: usize) {
    som::call_parent(wps::ccd("WPClock"), wps::cd("WPObject"), m::WPObject::wpInitData, &[o]);
    cd(o).mode = DEFAULT_MODE;
}

extern "C" fn clock_save_state(o: usize) -> u32 {
    som::call_parent(wps::ccd("WPClock"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[o]);
    obj::wpSaveLong(o, b"WPClock\0".as_ptr(), 1, cd(o).mode);
    1
}

extern "C" fn clock_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPClock"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    if let Some(v) = obj::restored_long(o, b"WPClock", 1) {
        cd(o).mode = v;
    }
    1
}

// The View page: what the clock shows.
const ID_TIMEONLY: u16 = 720;
const ID_DATEONLY: u16 = 721;
const ID_BOTH: u16 = 722;
const ID_DIGITAL: u16 = 723;
const ID_ANALOG: u16 = 724;
const ID_SECOND: u16 = 725;
const ID_HOURMARKS: u16 = 726;
const ID_MINMARKS: u16 = 727;

fn page_obj(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

extern "C" fn clock_view_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let mode = cd(b as usize).mode;
            let info = match mode & (SHOW_TIME | SHOW_DATE) {
                SHOW_TIME => ID_TIMEONLY,
                SHOW_DATE => ID_DATEONLY,
                _ => ID_BOTH,
            };
            check(h, info, true);
            check(h, if mode & ANALOG != 0 { ID_ANALOG } else { ID_DIGITAL }, true);
            check(h, ID_SECOND, mode & SECONDS != 0);
            check(h, ID_HOURMARKS, mode & HOUR_MARKS != 0);
            check(h, ID_MINMARKS, mode & MINUTE_MARKS != 0);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = page_obj(h);
            let d = cd(o);
            match (a & 0xffff) as u16 {
                ID_TIMEONLY => d.mode = d.mode & !SHOW_DATE | SHOW_TIME,
                ID_DATEONLY => d.mode = d.mode & !SHOW_TIME | SHOW_DATE,
                ID_BOTH => d.mode |= SHOW_TIME | SHOW_DATE,
                ID_DIGITAL => d.mode &= !ANALOG,
                ID_ANALOG => d.mode |= ANALOG,
                id @ (ID_SECOND | ID_HOURMARKS | ID_MINMARKS) => {
                    let bit = match id {
                        ID_SECOND => SECONDS,
                        ID_HOURMARKS => HOUR_MARKS,
                        _ => MINUTE_MARKS,
                    };
                    if checked(h, id) {
                        d.mode |= bit;
                    } else {
                        d.mode &= !bit;
                    }
                }
                _ => return 0,
            }
            obj::call(m::WPObject::wpSaveDeferred, &[o]);
            repaint_clocks(o);
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddClockView1Page(o: usize, nb: u32) -> u32 {
    // BS_AUTOCHECKBOX
    let cb = |t: &[u8], y: i16, id: u16| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | 0x0001, (124, y, 110, 10), id);
    add_page(o, nb, clock_view_proc, alloc::vec![
        book::label(b"Information", (6, 106, 110)),
        radio(b"~Time only", (12, 92, 104), ID_TIMEONLY, true),
        radio(b"D~ate only", (12, 80, 104), ID_DATEONLY, false),
        radio(b"~Both date and time", (12, 68, 104), ID_BOTH, false),
        book::label(b"Mode", (124, 106, 110)),
        radio(b"D~igital", (130, 92, 100), ID_DIGITAL, true),
        radio(b"A~nalog", (130, 80, 100), ID_ANALOG, false),
        cb(b"~Second hand", 60, ID_SECOND),
        cb(b"~Hour marks", 48, ID_HOURMARKS),
        cb(b"~Minute marks", 36, ID_MINMARKS),
    ], b"~View\0")
}

// The Date/Time page: the computer's clock.
const ID_HOURS: u16 = 730;
const ID_MINUTES: u16 = 731;
const ID_SECONDS: u16 = 732;
const ID_MONTH: u16 = 733;
const ID_DAY: u16 = 734;
const ID_YEAR: u16 = 735;

extern "C" fn clock_time_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    const SPBN_CHANGE: u32 = 0x20d;
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let dt = now();
            spin_init(h, ID_HOURS, 0, 23, dt.hours as i32);
            spin_init(h, ID_MINUTES, 0, 59, dt.minutes as i32);
            spin_init(h, ID_SECONDS, 0, 59, dt.seconds as i32);
            spin_init(h, ID_MONTH, 1, 12, dt.month as i32);
            spin_init(h, ID_DAY, 1, 31, dt.day as i32);
            spin_init(h, ID_YEAR, 1980, 2079, dt.year as i32);
            0
        }
        WM_CONTROL if a >> 16 == SPBN_CHANGE => {
            let id = (a & 0xffff) as u16;
            if !(ID_HOURS..=ID_YEAR).contains(&id) {
                return 0;
            }
            let mut dt = now();
            let v = |i: u16| spin_value(h, i);
            if let (Some(hh), Some(mi), Some(ss), Some(mo), Some(dd), Some(yy)) = (v(ID_HOURS), v(ID_MINUTES), v(ID_SECONDS), v(ID_MONTH), v(ID_DAY), v(ID_YEAR)) {
                if (0..24).contains(&hh) && (0..60).contains(&mi) && (0..60).contains(&ss) && (1..=12).contains(&mo) && (1..=31).contains(&dd) && (1980..2080).contains(&yy) {
                    dt.hours = hh as u8;
                    dt.minutes = mi as u8;
                    dt.seconds = ss as u8;
                    dt.hundredths = 0;
                    dt.month = mo as u8;
                    dt.day = dd as u8;
                    dt.year = yy as u16;
                    unsafe { os2::DosSetDateTime(&dt) };
                    for &(_, _, c) in unsafe { CLOCKS.iter() } {
                        unsafe { WinInvalidateRect(c, core::ptr::null(), 0) };
                    }
                }
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddClockDateTimePage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, clock_time_proc, alloc::vec![
        book::label(b"Time", (6, 106, 220)),
        book::label(b"Hours", (12, 92, 60)),
        spin((80, 90, 40), ID_HOURS),
        book::label(b"Minutes", (12, 78, 60)),
        spin((80, 76, 40), ID_MINUTES),
        book::label(b"Seconds", (12, 64, 60)),
        spin((80, 62, 40), ID_SECONDS),
        book::label(b"Date", (6, 46, 220)),
        book::label(b"Month", (12, 32, 60)),
        spin((80, 30, 40), ID_MONTH),
        book::label(b"Day", (12, 18, 60)),
        spin((80, 16, 40), ID_DAY),
        book::label(b"Year", (12, 4, 60)),
        spin((80, 2, 50), ID_YEAR),
    ], b"~Date/Time\0")
}

extern "C" fn clock_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPClock"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPClock", m::WPClock::wpAddClockDateTimePage, &[o, nb as usize]);
    wps::call("WPClock", m::WPClock::wpAddClockView1Page, &[o, nb as usize]);
    1
}

methods! {
    __m_wpAddSystemWindowPage => wpAddSystemWindowPage, 2;
    __m_wpAddSystemConfirmationPage => wpAddSystemConfirmationPage, 2;
    __m_system_add_pages => system_add_pages, 2;
    __m_rs2_system_add_pages => rs2_system_add_pages, 2;
    __m_wpAddClockView1Page => wpAddClockView1Page, 2;
    __m_wpAddClockDateTimePage => wpAddClockDateTimePage, 2;
    __m_clock_add_pages => clock_add_pages, 2;
    __m_clock_open => clock_open, 4;
    __m_clock_init => clock_init, 1;
    __m_clock_save_state => clock_save_state, 1;
    __m_clock_restore_state => clock_restore_state, 2;
}

pub fn impl_system() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![
            ("wpAddSystemWindowPage", __m_wpAddSystemWindowPage as usize),
            ("wpAddSystemConfirmationPage", __m_wpAddSystemConfirmationPage as usize),
        ],
        overrides: alloc::vec![("wpAddSettingsPages", __m_system_add_pages as usize), crate::pages::no_window_page_override()],
    }
}

pub fn impl_rs2_system() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpAddSettingsPages", __m_rs2_system_add_pages as usize), crate::pages::no_window_page_override()] }
}

pub fn impl_clock() -> Impl {
    Impl {
        data: core::mem::size_of::<ClockData>() as u32,
        methods: alloc::vec![
            ("wpAddClockView1Page", __m_wpAddClockView1Page as usize),
            ("wpAddClockDateTimePage", __m_wpAddClockDateTimePage as usize),
        ],
        overrides: alloc::vec![
            ("wpAddSettingsPages", __m_clock_add_pages as usize),
            ("wpOpen", __m_clock_open as usize),
            ("wpInitData", __m_clock_init as usize),
            ("wpSaveState", __m_clock_save_state as usize),
            ("wpRestoreState", __m_clock_restore_state as usize),
            crate::pages::no_window_page_override(),
        ],
    }
}
