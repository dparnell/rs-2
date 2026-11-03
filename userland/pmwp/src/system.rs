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
use crate::util::{cstr, parse_dec, prf_read, prf_write, z, HINI_USER};
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
    #[link_name = "PMWIN.873"]
    fn WinSetSysValue(desk: u32, i: i32, v: i32) -> u32;
    #[link_name = "PMWIN.9030"]
    fn Rs2SetMinButtonType(hide_button: u32, hide_window: u32) -> u32;
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

// ---- System: the settings (as WPCONFIG keeps them) ----
//
// `PM_ControlPanel` in OS2.INI, under the names WPSystem's
// wpclsQuerySetting and wpclsSetSetting take (the Toolkit's wpsystem.h
// values): Animation (SV_ANIMATION too), MinButtonType (1 hide button,
// 2 minimize button), HiddenMinWindows (1 hide, 2 to the viewer, 3 to
// the desktop), PrintScreen (SV_PRINTSCREEN too), LogoDisplayTime (ms;
// -1 indefinite, 0 none) and NameClash (NAMECLASH_PROMPT, _RENAME,
// _REPLACE).

const SV_ANIMATION: i32 = 93;
const SV_PRINTSCREEN: i32 = 97;

pub const BUTTON_HIDE: u32 = 1;
pub const BUTTON_MINIMIZE: u32 = 2;
pub const MIN_HIDE: u32 = 1;
pub const MIN_VIEWER: u32 = 2;
pub const MIN_DESKTOP: u32 = 3;
pub const LOGO_INDEFINITE: i32 = -1;
const LOGO_DEFAULT_MS: i32 = 2000;
pub const NAMECLASH_RENAME: u32 = 0x02;
pub const NAMECLASH_REPLACE: u32 = 0x08;
pub const NAMECLASH_PROMPT: u32 = 0x10;

const CP: &[u8] = b"PM_ControlPanel";

fn cp_int(key: &[u8]) -> Option<i32> {
    let v = prf_read(HINI_USER, CP, key)?;
    let t = v.strip_suffix(&[0]).unwrap_or(&v).trim_ascii();
    match t.strip_prefix(b"-") {
        Some(n) => parse_dec(n).map(|n| -(n as i32)),
        None => parse_dec(t).map(|n| n as i32),
    }
}

fn set_cp_int(key: &[u8], v: i32) {
    let mut t = alloc::format!("{v}").into_bytes();
    t.push(0);
    prf_write(HINI_USER, CP, key, Some(&t));
}

/// Windows zoom when they are minimized, restored, maximized or hidden.
pub fn animation() -> bool {
    cp_int(b"Animation").is_none_or(|v| v != 0)
}

pub fn set_animation(on: bool) {
    set_cp_int(b"Animation", on as i32);
    unsafe { WinSetSysValue(HWND_DESKTOP, SV_ANIMATION, on as i32) };
}

/// The button windows show: BUTTON_HIDE or BUTTON_MINIMIZE.
pub fn button_appearance() -> u32 {
    match cp_int(b"MinButtonType") {
        Some(1) => BUTTON_HIDE,
        _ => BUTTON_MINIMIZE,
    }
}

pub fn set_button_appearance(b: u32) {
    set_cp_int(b"MinButtonType", b as i32);
    apply_min_buttons();
}

/// What minimizing does: MIN_HIDE, MIN_VIEWER or MIN_DESKTOP (the last
/// two also kept as RS2SHELL's `RS2_Desktop` `MinimizeTo`).
pub fn min_behavior() -> u32 {
    match cp_int(b"HiddenMinWindows") {
        Some(1) => MIN_HIDE,
        Some(3) => MIN_DESKTOP,
        Some(2) => MIN_VIEWER,
        _ if crate::minwin::to_viewer() => MIN_VIEWER,
        _ => MIN_DESKTOP,
    }
}

pub fn set_min_behavior(b: u32) {
    set_cp_int(b"HiddenMinWindows", b as i32);
    match b {
        MIN_VIEWER => crate::minwin::set_to_viewer(true),
        MIN_DESKTOP => crate::minwin::set_to_viewer(false),
        _ => {}
    }
    apply_min_buttons();
}

/// PMWIN's frames follow the button appearance and the hide behavior.
pub fn apply_min_buttons() {
    unsafe { Rs2SetMinButtonType((button_appearance() == BUTTON_HIDE) as u32, (min_behavior() == MIN_HIDE) as u32) };
}

pub fn print_screen() -> bool {
    cp_int(b"PrintScreen").is_none_or(|v| v != 0)
}

pub fn set_print_screen(on: bool) {
    set_cp_int(b"PrintScreen", on as i32);
    unsafe { WinSetSysValue(HWND_DESKTOP, SV_PRINTSCREEN, on as i32) };
}

/// How long programs show their logo (ms; LOGO_INDEFINITE, or 0: not
/// at all).
pub fn logo_time() -> i32 {
    cp_int(b"LogoDisplayTime").unwrap_or(LOGO_DEFAULT_MS)
}

pub fn set_logo_time(ms: i32) {
    set_cp_int(b"LogoDisplayTime", ms);
}

/// What happens when an object comes into a folder that has one with
/// the same name: NAMECLASH_PROMPT (the default), _RENAME or _REPLACE.
pub fn name_clash() -> u32 {
    match cp_int(b"NameClash") {
        Some(2) => NAMECLASH_RENAME,
        Some(8) => NAMECLASH_REPLACE,
        _ => NAMECLASH_PROMPT,
    }
}

pub fn set_name_clash(v: u32) {
    set_cp_int(b"NameClash", v as i32);
}

/// When the shell starts: PMWIN gets the settings OS2.INI keeps.
pub fn apply_saved() {
    unsafe {
        WinSetSysValue(HWND_DESKTOP, SV_ANIMATION, animation() as i32);
        WinSetSysValue(HWND_DESKTOP, SV_PRINTSCREEN, print_screen() as i32);
    }
    apply_min_buttons();
    apply_input();
}

/// M_WPSystem's wpclsQuerySetting(pszSetting, pValue, ulValueLen): one
/// of the settings above (and ConcurrentView) as a ULONG; the size
/// returned, or 0.
extern "C" fn sys_query_setting(_cls: usize, name: *const u8, value: *mut u32, len: u32) -> u32 {
    if name.is_null() || value.is_null() || len < 4 {
        return 0;
    }
    let v = match cstr(name) {
        b"Animation" => animation() as u32,
        b"MinButtonAppearance" => button_appearance(),
        b"MinButtonBehavior" => min_behavior(),
        b"PrintScreen" => print_screen() as u32,
        b"LogoDisplayTime" => logo_time() as u32,
        b"NameClash" => name_clash(),
        // CCVIEW_ON 1, CCVIEW_OFF 2.
        b"ConcurrentView" => if concurrent_view() { 1 } else { 2 },
        _ => return 0,
    };
    unsafe { *value = v };
    4
}

/// M_WPSystem's wpclsSetSetting(pszSetting, pValue): sets one (the
/// *_DEFAULT values of wpsystem.h set the default).
extern "C" fn sys_set_setting(_cls: usize, name: *const u8, value: *const u32) -> u32 {
    if name.is_null() || value.is_null() {
        return 0;
    }
    let v = unsafe { *value };
    match cstr(name) {
        // ANIMATION_DEFAULT, PRINTSCREEN_DEFAULT: on.
        b"Animation" => set_animation(v != 0),
        b"MinButtonAppearance" => set_button_appearance(if v == BUTTON_HIDE { BUTTON_HIDE } else { BUTTON_MINIMIZE }),
        b"MinButtonBehavior" if (1..=3).contains(&v) => set_min_behavior(v),
        b"PrintScreen" => set_print_screen(v != 0),
        b"LogoDisplayTime" => set_logo_time(if v as i32 == -2 { LOGO_DEFAULT_MS } else { v as i32 }),
        b"NameClash" => set_name_clash(match v {
            NAMECLASH_RENAME | NAMECLASH_REPLACE => v,
            _ => NAMECLASH_PROMPT,
        }),
        b"ConcurrentView" => set_concurrent_view(v == 1),
        _ => return 0,
    }
    1
}

// ---- System: the Window page (WPCONFIG's dialog 1200) ----

const ID_HIDEBUTTON: u16 = 1224;
const ID_MINBUTTON: u16 = 1225;
const ID_HIDEWINDOW: u16 = 1301;
const ID_VIEWER: u16 = 1222;
const ID_DESKTOP: u16 = 1221;
const ID_ANIM_ON: u16 = 1203;
const ID_ANIM_OFF: u16 = 1204;
const ID_EXISTING: u16 = 1218;
const ID_NEW: u16 = 1219;
const ID_DEFAULT: u16 = 103;
const SPBN_CHANGE: u32 = 0x20d;

fn enable(h: u32, id: u16, on: bool) {
    unsafe { WinEnableWindow(WinWindowFromID(h, id as u32), on as u32) };
}

fn group(t: &[u8], (x, y, cx, cy): (i16, i16, i16, i16)) -> Item {
    // SS_GROUPBOX
    item(WC_STATIC, t, WS_VISIBLE | 0x0002, (x, y, cx, cy), 0xffff)
}

fn push(t: &[u8], (x, y): (i16, i16), id: u16) -> Item {
    item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP, (x, y, 40, 14), id)
}

fn window_page_init(h: u32) {
    let hide = button_appearance() == BUTTON_HIDE;
    check(h, if hide { ID_HIDEBUTTON } else { ID_MINBUTTON }, true);
    check(h, match min_behavior() {
        MIN_HIDE => ID_HIDEWINDOW,
        MIN_VIEWER => ID_VIEWER,
        _ => ID_DESKTOP,
    }, true);
    // A hide button always hides.
    for id in [ID_HIDEWINDOW, ID_VIEWER, ID_DESKTOP] {
        enable(h, id, !hide);
    }
    check(h, if animation() { ID_ANIM_ON } else { ID_ANIM_OFF }, true);
    check(h, if concurrent_view() { ID_NEW } else { ID_EXISTING }, true);
}

extern "C" fn window_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            window_page_init(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            // BN_CLICKED on a radio button.
            match (a & 0xffff) as u16 {
                ID_HIDEBUTTON => set_button_appearance(BUTTON_HIDE),
                ID_MINBUTTON => set_button_appearance(BUTTON_MINIMIZE),
                ID_HIDEWINDOW => set_min_behavior(MIN_HIDE),
                ID_VIEWER => set_min_behavior(MIN_VIEWER),
                ID_DESKTOP => set_min_behavior(MIN_DESKTOP),
                ID_ANIM_ON => set_animation(true),
                ID_ANIM_OFF => set_animation(false),
                ID_EXISTING => set_concurrent_view(false),
                ID_NEW => set_concurrent_view(true),
                _ => return 0,
            }
            let hide = button_appearance() == BUTTON_HIDE;
            for id in [ID_HIDEWINDOW, ID_VIEWER, ID_DESKTOP] {
                enable(h, id, !hide);
            }
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                set_button_appearance(BUTTON_MINIMIZE);
                set_min_behavior(MIN_VIEWER);
                set_animation(true);
                set_concurrent_view(false);
                window_page_init(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSystemWindowPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, window_page_proc, alloc::vec![
        group(b"Button appearance for windows", (4, 74, 110, 42)),
        radio(b"Hide ~button", (8, 96, 100), ID_HIDEBUTTON, true),
        radio(b"~Minimize button", (8, 84, 100), ID_MINBUTTON, false),
        group(b"Minimize button behavior", (120, 62, 114, 54)),
        radio(b"~Hide window", (124, 96, 106), ID_HIDEWINDOW, true),
        radio(b"Minimize window to ~viewer", (124, 84, 106), ID_VIEWER, false),
        radio(b"Minimize ~window to desktop", (124, 72, 106), ID_DESKTOP, false),
        group(b"Animation", (4, 22, 110, 42)),
        radio(b"~Enabled", (8, 44, 100), ID_ANIM_ON, true),
        radio(b"D~isabled", (8, 32, 100), ID_ANIM_OFF, false),
        group(b"Object open behavior", (120, 22, 114, 38)),
        radio(b"Display ~existing window", (124, 42, 106), ID_EXISTING, true),
        radio(b"~Create new window", (124, 30, 106), ID_NEW, false),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ], b"~Window\0")
}

// ---- System: the Logo page (dialog 1201) ----

const ID_LOGO_INDEF: u16 = 1209;
const ID_LOGO_NONE: u16 = 1210;
const ID_LOGO_TIMED: u16 = 1211;
const ID_LOGO_SECS: u16 = 1212;
const ID_LOGO_TENTHS: u16 = 1213;

fn logo_init(h: u32) {
    let t = logo_time();
    check(h, match t {
        LOGO_INDEFINITE => ID_LOGO_INDEF,
        0 => ID_LOGO_NONE,
        _ => ID_LOGO_TIMED,
    }, true);
    let ms = if t > 0 { t } else { LOGO_DEFAULT_MS };
    spin_init(h, ID_LOGO_SECS, 0, 32, ms / 1000);
    spin_init(h, ID_LOGO_TENTHS, 0, 9, ms % 1000 / 100);
    enable(h, ID_LOGO_SECS, t > 0);
    enable(h, ID_LOGO_TENTHS, t > 0);
}

extern "C" fn logo_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            logo_init(h);
            0
        }
        WM_CONTROL => {
            let id = (a & 0xffff) as u16;
            match (id, a >> 16) {
                (ID_LOGO_INDEF, 1) => set_logo_time(LOGO_INDEFINITE),
                (ID_LOGO_NONE, 1) => set_logo_time(0),
                (ID_LOGO_TIMED, 1) | (ID_LOGO_SECS | ID_LOGO_TENTHS, SPBN_CHANGE) => {
                    let (s, t) = (spin_value(h, ID_LOGO_SECS).unwrap_or(2), spin_value(h, ID_LOGO_TENTHS).unwrap_or(0));
                    let ms = (s.clamp(0, 32) * 1000 + t.clamp(0, 9) * 100).max(100);
                    if checked(h, ID_LOGO_TIMED) {
                        set_logo_time(ms);
                    }
                }
                _ => return 0,
            }
            let t = logo_time();
            enable(h, ID_LOGO_SECS, t > 0);
            enable(h, ID_LOGO_TENTHS, t > 0);
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                set_logo_time(LOGO_DEFAULT_MS);
                logo_init(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSystemLogoPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, logo_page_proc, alloc::vec![
        group(b"Logo display time", (4, 40, 150, 76)),
        radio(b"~Indefinite", (8, 96, 100), ID_LOGO_INDEF, true),
        radio(b"~None", (8, 84, 100), ID_LOGO_NONE, false),
        radio(b"~Timed", (8, 72, 100), ID_LOGO_TIMED, false),
        book::label(b"Seconds", (20, 62, 40)),
        spin((20, 48, 40), ID_LOGO_SECS),
        book::label(b"Tenths", (70, 62, 40)),
        spin((70, 48, 30), ID_LOGO_TENTHS),
        book::label(b"How long programs show their logo window.", (4, 26, 220)),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ], b"~Logo\0")
}

// ---- System: the Print Screen page (dialog 1300) ----

const ID_PRTSC_ON: u16 = 1203;
const ID_PRTSC_OFF: u16 = 1204;

extern "C" fn print_screen_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            check(h, if print_screen() { ID_PRTSC_ON } else { ID_PRTSC_OFF }, true);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            match (a & 0xffff) as u16 {
                ID_PRTSC_ON => set_print_screen(true),
                ID_PRTSC_OFF => set_print_screen(false),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                set_print_screen(true);
                check(h, ID_PRTSC_ON, true);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSystemPrintScreenPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, print_screen_page_proc, alloc::vec![
        group(b"Print Screen key", (4, 50, 170, 66)),
        radio(b"~Enable", (8, 96, 100), ID_PRTSC_ON, true),
        radio(b"Dis~able", (8, 84, 100), ID_PRTSC_OFF, false),
        book::label(b"Print Screen prints the active window", (8, 68, 160)),
        book::label(b"(or the desktop) on the default printer.", (8, 58, 160)),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ], b"~Print Screen\0")
}

// ---- System: the Title page (dialog 3052) ----

const ID_CLASH_PROMPT: u16 = 1345;
const ID_CLASH_RENAME: u16 = 1346;
const ID_CLASH_REPLACE: u16 = 523;

fn title_init(h: u32) {
    check(h, match name_clash() {
        NAMECLASH_RENAME => ID_CLASH_RENAME,
        NAMECLASH_REPLACE => ID_CLASH_REPLACE,
        _ => ID_CLASH_PROMPT,
    }, true);
}

extern "C" fn title_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            title_init(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            match (a & 0xffff) as u16 {
                ID_CLASH_PROMPT => set_name_clash(NAMECLASH_PROMPT),
                ID_CLASH_RENAME => set_name_clash(NAMECLASH_RENAME),
                ID_CLASH_REPLACE => set_name_clash(NAMECLASH_REPLACE),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                set_name_clash(NAMECLASH_PROMPT);
                title_init(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddTitleConfirmationPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, title_page_proc, alloc::vec![
        group(b"Duplicate title", (4, 30, 210, 86)),
        book::label(b"In the event that an object is added to a folder", (8, 98, 200)),
        book::label(b"that contains an object of the same title:", (8, 88, 200)),
        radio(b"~Prompt for appropriate action", (8, 72, 180), ID_CLASH_PROMPT, true),
        radio(b"~Auto-rename object", (8, 58, 180), ID_CLASH_RENAME, false),
        radio(b"~Replace existing object", (8, 44, 180), ID_CLASH_REPLACE, false),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ], b"~Title\0")
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

// ---- System: the Input page (WPCONFIG's PmInputDlgProc, dialog 1302) ----
//
// Each check box sets a system value at once and WPCONFIG keeps it in
// `PM_ControlPanel` as a ULONG: FullWindowDrag (99, off), AltTabWindow
// (100, on), VioMouseActions (101, on), AsyncFocusChange (102, on),
// AFCSensitivity (103, tenths of a second, 20) and LockStartInput
// (SV_LOCKSTARTINPUT 98, the Type-ahead box, off).

const ID_FULLDRAG: u16 = 1510;
const ID_ALTTAB: u16 = 1511;
const ID_VIOMOUSE: u16 = 1512;
const ID_ASYNCFOCUS: u16 = 1513;
const ID_AFCSENS: u16 = 1514;
const ID_TYPEAHEAD: u16 = 1515;
const WM_SYSVALUECHANGED: u32 = 0x002d;

/// (check box or spin button, key, system value, default).
const INPUT: [(u16, &[u8], i32, u32); 6] = [
    (ID_FULLDRAG, b"FullWindowDrag", 99, 0),
    (ID_ALTTAB, b"AltTabWindow", 100, 1),
    (ID_VIOMOUSE, b"VioMouseActions", 101, 1),
    (ID_ASYNCFOCUS, b"AsyncFocusChange", 102, 1),
    (ID_AFCSENS, b"AFCSensitivity", 103, 20),
    (ID_TYPEAHEAD, b"LockStartInput", SV_LOCKSTARTINPUT, 0),
];
const SV_LOCKSTARTINPUT: i32 = 98;

unsafe extern "C" {
    #[link_name = "PMWIN.708"]
    fn WinBroadcastMsg(h: u32, msg: u32, a: u32, b: u32, flags: u32) -> u32;
}

/// An Input setting (its ULONG in `PM_ControlPanel`, else the default).
pub fn input_setting(key: &[u8]) -> u32 {
    let def = INPUT.iter().find(|i| i.1 == key).map_or(0, |i| i.3);
    match prf_read(HINI_USER, CP, key) {
        Some(v) if v.len() == 4 => u32::from_le_bytes([v[0], v[1], v[2], v[3]]),
        _ => def,
    }
}

pub fn set_input_setting(key: &[u8], v: u32) {
    let Some(&(_, _, sv, _)) = INPUT.iter().find(|i| i.1 == key) else { return };
    prf_write(HINI_USER, CP, key, Some(&v.to_le_bytes()));
    unsafe {
        WinSetSysValue(HWND_DESKTOP, sv, v as i32);
        // BMSG_POST | BMSG_DESCENDANTS: programs hear of the change.
        WinBroadcastMsg(HWND_DESKTOP, WM_SYSVALUECHANGED, sv as u32, sv as u32, 0x0001 | 0x0008);
    }
}

fn input_init(h: u32) {
    for &(id, key, _, _) in &INPUT {
        if id == ID_AFCSENS {
            spin_init(h, id, 0, 99, input_setting(key) as i32);
        } else {
            check(h, id, input_setting(key) != 0);
        }
    }
    enable(h, ID_AFCSENS, input_setting(b"AsyncFocusChange") != 0);
}

extern "C" fn input_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            input_init(h);
            0
        }
        WM_CONTROL => {
            let id = (a & 0xffff) as u16;
            match (id, a >> 16) {
                (ID_AFCSENS, SPBN_CHANGE) => {
                    if let Some(v) = spin_value(h, ID_AFCSENS).filter(|v| (0..=99).contains(v)) {
                        set_input_setting(b"AFCSensitivity", v as u32);
                    }
                }
                (_, 1) => {
                    let Some(&(_, key, _, _)) = INPUT.iter().find(|i| i.0 == id) else { return 0 };
                    set_input_setting(key, checked(h, id) as u32);
                    enable(h, ID_AFCSENS, input_setting(b"AsyncFocusChange") != 0);
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                for &(_, key, _, def) in &INPUT {
                    set_input_setting(key, def);
                }
                input_init(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn checkbox(t: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    // BS_AUTOCHECKBOX
    item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (x, y, cx, 10), id)
}

extern "C" fn wpAddSystemInputPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, input_page_proc, alloc::vec![
        group(b"Window manipulation", (4, 50, 112, 66)),
        checkbox(b"Full window ~dragging", (8, 94, 104), ID_FULLDRAG),
        checkbox(b"~Alt+Tab switch window", (8, 80, 104), ID_ALTTAB),
        checkbox(b"~VIO mouse actions", (8, 66, 104), ID_VIOMOUSE),
        group(b"User input", (120, 22, 114, 94)),
        checkbox(b"Asynchronous ~focus change", (124, 94, 106), ID_ASYNCFOCUS),
        book::label(b"Focus change delay:", (124, 80, 106)),
        spin((124, 66, 36), ID_AFCSENS),
        book::label(b"tenths of a second", (164, 68, 66)),
        checkbox(b"~Type-ahead", (124, 50, 106), ID_TYPEAHEAD),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ], b"~Input\0")
}

// ---- System: the folder pages (WPCONFIG's dialogs 1307, 1303, 2107) ----

const ID_LONGMENUS: u16 = 3;
const ID_SHORTMENUS: u16 = 4;
const ID_MENUBARS: u16 = 1;

fn sys_menu_init(h: u32) {
    let short = crate::usermenu::system_menu_style() == crate::usermenu::MENUS_SHORT;
    check(h, if short { ID_SHORTMENUS } else { ID_LONGMENUS }, true);
    check(h, ID_MENUBARS, crate::usermenu::system_menu_bar());
}

extern "C" fn sys_menu_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            sys_menu_init(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            match (a & 0xffff) as u16 {
                ID_LONGMENUS => crate::usermenu::set_system_menu_style(crate::usermenu::MENUS_LONG),
                ID_SHORTMENUS => crate::usermenu::set_system_menu_style(crate::usermenu::MENUS_SHORT),
                ID_MENUBARS => crate::usermenu::set_system_menu_bar(checked(h, ID_MENUBARS)),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                crate::usermenu::set_system_menu_style(crate::usermenu::MENUS_LONG);
                crate::usermenu::set_system_menu_bar(true);
                sys_menu_init(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// The Folder tab's first page: the menus of every folder that has no
/// setting of its own.
extern "C" fn wpAddSysFdrMenuPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, sys_menu_page_proc, alloc::vec![
        group(b"Menu style", (4, 58, 160, 58)),
        radio(b"Display ~long menus", (10, 94, 150), ID_LONGMENUS, true),
        radio(b"Display ~short menus", (10, 80, 150), ID_SHORTMENUS, false),
        checkbox(b"Display ~folder menu bars", (8, 42, 160), ID_MENUBARS),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ], b"~Folder\0")
}

const ID_DEFICON: u16 = 1304;
const ID_DEFTREE: u16 = 1305;
const ID_DEFDETAILS: u16 = 1306;

/// The view folders open in when they have no default of their own
/// (`PM_Workplace` `DefFolderView`: ICON, TREE or DETAILS).
pub fn default_folder_view() -> u32 {
    let v: Vec<u8> = prf_read(HINI_USER, b"PM_Workplace", b"DefFolderView").unwrap_or_default().into_iter().take_while(|&c| c != 0).collect();
    match v.as_slice() {
        b"TREE" => obj::OPEN_TREE,
        b"DETAILS" => obj::OPEN_DETAILS,
        _ => obj::OPEN_CONTENTS,
    }
}

pub fn set_default_folder_view(view: u32) {
    let t: &[u8] = match view {
        obj::OPEN_TREE => b"TREE\0",
        obj::OPEN_DETAILS => b"DETAILS\0",
        _ => b"ICON\0",
    };
    prf_write(HINI_USER, b"PM_Workplace", b"DefFolderView", Some(t));
}

fn defview_init(h: u32) {
    check(h, match default_folder_view() {
        obj::OPEN_TREE => ID_DEFTREE,
        obj::OPEN_DETAILS => ID_DEFDETAILS,
        _ => ID_DEFICON,
    }, true);
}

extern "C" fn defview_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            defview_init(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            match (a & 0xffff) as u16 {
                ID_DEFICON => set_default_folder_view(obj::OPEN_CONTENTS),
                ID_DEFTREE => set_default_folder_view(obj::OPEN_TREE),
                ID_DEFDETAILS => set_default_folder_view(obj::OPEN_DETAILS),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                set_default_folder_view(obj::OPEN_CONTENTS);
                defview_init(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSysFdrDefViewPage(o: usize, nb: u32) -> u32 {
    let tpl = crate::pm::template(book::page_frame(), alloc::vec![
        group(b"Default folder view", (4, 58, 120, 58)),
        radio(b"~Icon", (10, 94, 100), ID_DEFICON, true),
        radio(b"~Tree", (10, 80, 100), ID_DEFTREE, false),
        radio(b"~Details", (10, 66, 100), ID_DEFDETAILS, false),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ]);
    let page = book::make_page(nb, defview_page_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert_after(o, nb, page, b"Folder - View\0", b"~Folder")
}

const ID_CLOSE_NEVER: u16 = 2102;
const ID_CLOSE_SUBFOLDERS: u16 = 2101;
const ID_CLOSE_ALL: u16 = 2103;

fn selfclose_init(h: u32) {
    check(h, match crate::usermenu::system_self_close() {
        crate::usermenu::SELFCLOSE_SUBFOLDERS => ID_CLOSE_SUBFOLDERS,
        crate::usermenu::SELFCLOSE_ALL => ID_CLOSE_ALL,
        _ => ID_CLOSE_NEVER,
    }, true);
}

extern "C" fn selfclose_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            selfclose_init(h);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            match (a & 0xffff) as u16 {
                ID_CLOSE_NEVER => crate::usermenu::set_system_self_close(crate::usermenu::SELFCLOSE_NEVER),
                ID_CLOSE_SUBFOLDERS => crate::usermenu::set_system_self_close(crate::usermenu::SELFCLOSE_SUBFOLDERS),
                ID_CLOSE_ALL => crate::usermenu::set_system_self_close(crate::usermenu::SELFCLOSE_ALL),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                crate::usermenu::set_system_self_close(crate::usermenu::SELFCLOSE_NEVER);
                selfclose_init(h);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSysFdrSelfClosePage(o: usize, nb: u32) -> u32 {
    let tpl = crate::pm::template(book::page_frame(), alloc::vec![
        group(b"Folder automatic close", (4, 44, 200, 72)),
        book::label(b"Close a folder's window when an object in it is opened:", (8, 98, 192)),
        radio(b"~Never", (10, 82, 150), ID_CLOSE_NEVER, true),
        radio(b"~Subfolders only", (10, 68, 150), ID_CLOSE_SUBFOLDERS, false),
        radio(b"~All objects", (10, 54, 150), ID_CLOSE_ALL, false),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ]);
    let page = book::make_page(nb, selfclose_page_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert_after(o, nb, page, b"Folder - Automatic close\0", b"~Folder")
}

// ---- System: the 256-colour page (dialog 1226) ----
//
// Only on a display of 256 colours, as IBM's: the Workplace Shell
// realizes a palette for its icons and backgrounds
// (`PM_WorkPlace:Options` `WPSPaletteAware`).

const ID_PALETTEAWARE: u16 = 1010;

/// Whether the screen has 256 colours (CAPS_COLOR_BITCOUNT of the
/// desktop's device).
fn screen_256() -> bool {
    unsafe extern "C" {
        #[link_name = "PMGPI.606"]
        fn DevQueryCaps(hdc: u32, first: i32, n: i32, out: *mut i32) -> u32;
        #[link_name = "PMWIN.795"]
        fn WinOpenWindowDC(h: u32) -> u32;
    }
    let mut bits = 0i32;
    unsafe {
        let hdc = WinOpenWindowDC(HWND_DESKTOP);
        DevQueryCaps(hdc, 40, 1, &mut bits);
    }
    bits == 8
}

pub fn palette_aware() -> bool {
    prf_read(HINI_USER, b"PM_WorkPlace:Options", b"WPSPaletteAware").and_then(|v| v.first().copied()).is_some_and(|c| c != 0 && c != b'0')
}

fn set_palette_aware(on: bool) {
    prf_write(HINI_USER, b"PM_WorkPlace:Options", b"WPSPaletteAware", Some(&(on as u32).to_le_bytes()));
}

extern "C" fn color256_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            check(h, ID_PALETTEAWARE, palette_aware());
            0
        }
        WM_CONTROL if (a >> 16) == 1 && (a & 0xffff) as u16 == ID_PALETTEAWARE => {
            set_palette_aware(checked(h, ID_PALETTEAWARE));
            0
        }
        WM_COMMAND => {
            if (a & 0xffff) as u16 == ID_DEFAULT {
                set_palette_aware(false);
                check(h, ID_PALETTEAWARE, false);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSystem256ColorPage(o: usize, nb: u32) -> u32 {
    if !screen_256() {
        return 1;
    }
    add_page(o, nb, color256_page_proc, alloc::vec![
        checkbox(b"Workplace Shell ~palette aware", (8, 100, 160), ID_PALETTEAWARE),
        push(b"~Default", (4, 2), ID_DEFAULT),
    ], b"~Screen\0")
}

/// The Input page's system values, set when the shell starts.
fn apply_input() {
    for &(_, key, sv, _) in &INPUT {
        unsafe { WinSetSysValue(HWND_DESKTOP, sv, input_setting(key) as i32) };
    }
}

/// IBM's order of the tabs: Confirmations, Title, Window, Print Screen,
/// Logo (each page goes in front of those before it); rs2 puts Folder
/// and Input after Logo.
extern "C" fn system_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPSystem"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSystem256ColorPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSystemInputPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSysFdrMenuPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSysFdrDefViewPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSysFdrSelfClosePage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSystemLogoPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSystemPrintScreenPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSystemWindowPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddTitleConfirmationPage, &[o, nb as usize]);
    wps::call("WPSystem", m::WPSystem::wpAddSystemConfirmationPage, &[o, nb as usize]);
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
    __m_wpAddSystemLogoPage => wpAddSystemLogoPage, 2;
    __m_wpAddSystemPrintScreenPage => wpAddSystemPrintScreenPage, 2;
    __m_wpAddTitleConfirmationPage => wpAddTitleConfirmationPage, 2;
    __m_wpAddSystemInputPage => wpAddSystemInputPage, 2;
    __m_wpAddSysFdrMenuPage => wpAddSysFdrMenuPage, 2;
    __m_wpAddSysFdrDefViewPage => wpAddSysFdrDefViewPage, 2;
    __m_wpAddSysFdrSelfClosePage => wpAddSysFdrSelfClosePage, 2;
    __m_wpAddSystem256ColorPage => wpAddSystem256ColorPage, 2;
    __m_sys_query_setting => sys_query_setting, 4;
    __m_sys_set_setting => sys_set_setting, 3;
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
            ("wpAddSystemLogoPage", __m_wpAddSystemLogoPage as usize),
            ("wpAddSystemPrintScreenPage", __m_wpAddSystemPrintScreenPage as usize),
            ("wpAddTitleConfirmationPage", __m_wpAddTitleConfirmationPage as usize),
            ("wpAddSystemInputPage", __m_wpAddSystemInputPage as usize),
            ("wpAddSysFdrMenuPage", __m_wpAddSysFdrMenuPage as usize),
            ("wpAddSysFdrDefViewPage", __m_wpAddSysFdrDefViewPage as usize),
            ("wpAddSysFdrSelfClosePage", __m_wpAddSysFdrSelfClosePage as usize),
            ("wpAddSystem256ColorPage", __m_wpAddSystem256ColorPage as usize),
        ],
        overrides: alloc::vec![("wpAddSettingsPages", __m_system_add_pages as usize), crate::pages::no_window_page_override()],
    }
}

pub fn impl_m_system() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![("wpclsQuerySetting", __m_sys_query_setting as usize), ("wpclsSetSetting", __m_sys_set_setting as usize)],
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
