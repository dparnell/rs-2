//! The System Setup folder's settings objects, as IBM's WPCONFIG.DLL
//! classes:
//!
//! - WPMouse: the Timing page (the double-click time) and the Setup
//!   page (right- or left-handed: SV_SWAPBUTTON);
//! - WPKeyboard: the Timing page (the key repeat rate and delay, through
//!   KBD$'s KBD_SETTYPAMATICRATE, and the cursor blink rate);
//! - WPSound: the Warning beep page (SV_ALARM);
//! - WPColorPalette: colours to drag onto a folder (its background, or
//!   with Shift its text) or onto any window (PP_BACKGROUNDCOLOR /
//!   PP_FOREGROUNDCOLOR);
//! - WPSchemePalette: colour schemes to drag onto a window: the scheme
//!   becomes the system's colours (WinSetSysColors) for every window;
//! - WPFontPalette: fonts to drag onto a folder (its font, kept with it)
//!   or onto any window (PP_FONTNAMESIZE: it and its children draw in
//!   it).
//!
//! What is set is kept where IBM's PM keeps it, in OS2.INI:
//! `PM_ControlPanel` (DoubleClickSpeed, SwapMouseButton, CursorBlinkRate,
//! KeyRepeatRate, KeyRepeatDelay, Beep) and `PM_Colors` ("R G B" by the
//! colour's name); the shell applies them when it starts (`apply_saved`).

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj::OPEN_SETTINGS;
use crate::pm::*;
use crate::som;
use crate::util::{parse_dec, prf_read, prf_write, z, HINI_USER};
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMGPI.368"]
    fn GpiCreateLogFont(hps: u32, name: *const u8, lcid: i32, fat: *const u8) -> i32;
    #[link_name = "PMGPI.513"]
    fn GpiSetCharSet(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMGPI.510"]
    fn GpiSetCharBox(hps: u32, p: *const [i32; 2]) -> u32;
    #[link_name = "PMGPI.378"]
    fn GpiDeleteSetId(hps: u32, lcid: i32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
    #[link_name = "PMWIN.826"]
    fn WinQuerySysColor(desk: u32, clr: i32, reserved: i32) -> u32;
    #[link_name = "PMWIN.871"]
    fn WinSetSysColors(desk: u32, options: u32, format: u32, first: i32, n: u32, p: *const u32) -> u32;
    #[link_name = "PMWIN.873"]
    fn WinSetSysValue(desk: u32, i: i32, v: i32) -> u32;
}

const SV_SWAPBUTTON: i32 = 0;
const SV_DBLCLKTIME: i32 = 1;
const SV_ALARM: i32 = 6;
const SV_CURSORRATE: i32 = 9;

pub const SYSCLR_BACKGROUND: i32 = -24;
pub const SYSCLR_WINDOW: i32 = -20;

// ---- OS2.INI ----

const CP: &[u8] = b"PM_ControlPanel";

fn read_num(key: &[u8]) -> Option<i32> {
    let v = prf_read(HINI_USER, CP, key)?;
    let s = v.strip_suffix(&[0]).unwrap_or(&v);
    parse_dec(s.trim_ascii()).map(|n| n as i32)
}

fn write_num(key: &[u8], n: i32) {
    let mut s = alloc::format!("{n}").into_bytes();
    s.push(0);
    prf_write(HINI_USER, CP, key, Some(&s));
}

fn sysval(i: i32) -> i32 {
    unsafe { WinQuerySysValue(HWND_DESKTOP, i) }
}

fn set_sysval(i: i32, key: &[u8], v: i32) {
    unsafe { WinSetSysValue(HWND_DESKTOP, i, v) };
    write_num(key, v);
}

/// The key repeat (delay ms, rate cps) as last set (or the defaults).
fn typematic() -> (i32, i32) {
    (read_num(b"KeyRepeatDelay").unwrap_or(500), read_num(b"KeyRepeatRate").unwrap_or(20))
}

/// KBD$'s KBD_SETTYPAMATICRATE.
fn set_typematic(delay: i32, rate: i32) {
    let (mut h, mut act) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS, OPEN_SHARE_DENYNONE | OPEN_ACCESS_READWRITE
    if unsafe { os2::DosOpen(b"\\DEV\\KBD$\0".as_ptr(), &mut h, &mut act, 0, 0, 1, 0x42, core::ptr::null_mut()) } != 0 {
        return;
    }
    let mut p = [delay as u16, rate as u16];
    let mut cb = 4u32;
    unsafe {
        os2::DosDevIOCtl(h, 4, 0x54, p.as_mut_ptr() as *mut u8, 4, &mut cb, core::ptr::null_mut(), 0, core::ptr::null_mut());
        os2::DosClose(h);
    }
}

/// The colours a scheme sets: (PM_Colors name, SYSCLR_*).
const COLORS: [(&[u8], i32); 16] = [
    (b"Background", -24),
    (b"ActiveTitle", -23),
    (b"InactiveTitle", -22),
    (b"ActiveTitleText", -28),
    (b"InactiveTitleText", -29),
    (b"DialogBackground", -34),
    (b"ButtonLight", -41),
    (b"ButtonMiddle", -40),
    (b"ButtonDark", -39),
    (b"Window", -20),
    (b"WindowText", -17),
    (b"HiliteBackground", -32),
    (b"HiliteForeground", -33),
    (b"WindowFrame", -19),
    (b"ActiveBorder", -15),
    (b"InactiveBorder", -14),
];

fn parse_rgb(v: &[u8]) -> Option<u32> {
    let s = v.strip_suffix(&[0]).unwrap_or(v);
    let n: Vec<u32> = s.split(|&c| c == b' ').filter(|p| !p.is_empty()).filter_map(parse_dec).collect();
    (n.len() == 3).then(|| (n[0] & 0xff) << 16 | (n[1] & 0xff) << 8 | (n[2] & 0xff))
}

static mut SCHEMED: bool = false;

/// The desktop's background unless it has its own: the scheme's, or
/// rs2's.
pub fn desktop_background() -> u32 {
    if unsafe { SCHEMED } { unsafe { WinQuerySysColor(HWND_DESKTOP, SYSCLR_BACKGROUND, 0) } } else { crate::view::DESKTOP_BG }
}

/// A system colour (SYSCLR_*) as RGB.
pub fn sys_color(i: i32) -> u32 {
    unsafe { WinQuerySysColor(HWND_DESKTOP, i, 0) }
}

/// A folder's background unless it has its own.
pub fn folder_background() -> u32 {
    unsafe { WinQuerySysColor(HWND_DESKTOP, SYSCLR_WINDOW, 0) }
}

/// Make the colours the system's: (SYSCLR_*, RGB) pairs.
fn set_colors(pairs: &[(i32, u32)]) {
    let flat: Vec<u32> = pairs.iter().flat_map(|&(i, c)| [i as u32, c]).collect();
    // LCOLF_INDRGB
    unsafe { WinSetSysColors(HWND_DESKTOP, 0, 1, 0, flat.len() as u32, flat.as_ptr()) };
    unsafe { SCHEMED = true };
    // The folders without their own background follow.
    let objs: Vec<usize> = crate::view::wins().iter().map(|w| w.obj).collect();
    for o in objs {
        crate::view::background_changed(o);
    }
}

/// When the shell starts: the settings OS2.INI keeps.
pub fn apply_saved() {
    for (key, i) in [(&b"DoubleClickSpeed"[..], SV_DBLCLKTIME), (b"SwapMouseButton", SV_SWAPBUTTON), (b"CursorBlinkRate", SV_CURSORRATE), (b"Beep", SV_ALARM)] {
        if let Some(v) = read_num(key) {
            unsafe { WinSetSysValue(HWND_DESKTOP, i, v) };
        }
    }
    if read_num(b"KeyRepeatRate").is_some() || read_num(b"KeyRepeatDelay").is_some() {
        let (d, r) = typematic();
        set_typematic(d, r);
    }
    let pairs: Vec<(i32, u32)> = COLORS.iter().filter_map(|&(n, i)| Some((i, parse_rgb(&prf_read(HINI_USER, b"PM_Colors", n)?)?))).collect();
    if !pairs.is_empty() {
        set_colors(&pairs);
    }
}

// ---- Settings pages ----

fn check(h: u32, id: u16, on: bool) {
    unsafe { WinSendDlgItemMsg(h, id as u32, BM_SETCHECK, on as u32, 0) };
}

fn checked(h: u32, id: u16) -> bool {
    unsafe { WinSendDlgItemMsg(h, id as u32, BM_QUERYCHECK, 0, 0) != 0 }
}

// WC_SPINBUTTON; SPBM_SETLIMITS, SPBM_SETCURRENTVALUE, SPBM_QUERYVALUE;
// SPBN_CHANGE.
const WC_SPINBUTTON: u16 = 0x20;
const SPBM_QUERYVALUE: u32 = 0x205;
const SPBM_SETLIMITS: u32 = 0x207;
const SPBM_SETCURRENTVALUE: u32 = 0x208;
const SPBN_CHANGE: u32 = 0x20d;

/// A numeric spin button (SPBS_MASTER | SPBS_NUMERICONLY).
fn spin((x, y, cx): (i16, i16, i16), id: u16) -> Item {
    item(WC_SPINBUTTON, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0011, (x, y, cx, 12), id)
}

fn spin_init(h: u32, id: u16, lo: i32, hi: i32, v: i32) {
    unsafe {
        WinSendDlgItemMsg(h, id as u32, SPBM_SETLIMITS, hi as u32, lo as u32);
        WinSendDlgItemMsg(h, id as u32, SPBM_SETCURRENTVALUE, v as u32, 0);
    }
}

fn spin_value(h: u32, id: u16) -> Option<i32> {
    let mut v = 0i32;
    // SPBQ_DONOTUPDATE
    (unsafe { WinSendDlgItemMsg(h, id as u32, SPBM_QUERYVALUE, &mut v as *mut i32 as u32, 3 << 16) } != 0).then_some(v)
}

fn radio(t: &[u8], (x, y, cx): (i16, i16, i16), id: u16, first: bool) -> Item {
    // BS_AUTORADIOBUTTON
    item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (x, y, cx, 10), id)
}

fn add_page(o: usize, nb: u32, proc_: extern "C" fn(u32, u32, u32, u32) -> u32, items: Vec<Item>, tab: &'static [u8]) -> u32 {
    let tpl = template(book::page_frame(), items);
    let page = book::make_page(nb, proc_, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, tab)
}

// Mouse.
const ID_DBLCLK: u16 = 600;
const ID_RIGHT: u16 = 601;
const ID_LEFT: u16 = 602;

extern "C" fn mouse_timing_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            spin_init(h, ID_DBLCLK, 100, 2000, sysval(SV_DBLCLKTIME));
            0
        }
        WM_CONTROL if (a & 0xffff) as u16 == ID_DBLCLK && a >> 16 == SPBN_CHANGE => {
            if let Some(v) = spin_value(h, ID_DBLCLK).filter(|v| (100..=2000).contains(v)) {
                set_sysval(SV_DBLCLKTIME, b"DoubleClickSpeed", v);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddMouseTimingPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, mouse_timing_proc, alloc::vec![
        book::label(b"Double-click time (milliseconds):", (6, 104, 150)),
        spin((160, 102, 50), ID_DBLCLK),
        book::label(b"Two clicks this close together are a double-click.", (6, 84, 220)),
    ], b"~Timing\0")
}

extern "C" fn mouse_setup_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            check(h, if sysval(SV_SWAPBUTTON) != 0 { ID_LEFT } else { ID_RIGHT }, true);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            match (a & 0xffff) as u16 {
                ID_RIGHT => set_sysval(SV_SWAPBUTTON, b"SwapMouseButton", 0),
                ID_LEFT => set_sysval(SV_SWAPBUTTON, b"SwapMouseButton", 1),
                _ => {}
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddMouseTypePage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, mouse_setup_proc, alloc::vec![
        book::label(b"Mouse button setup:", (6, 104, 200)),
        radio(b"~Right-handed (button 1 is the left button)", (10, 90, 220), ID_RIGHT, true),
        radio(b"~Left-handed (button 1 is the right button)", (10, 76, 220), ID_LEFT, false),
    ], b"~Setup\0")
}

extern "C" fn mouse_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPMouse"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wpAddMouseTypePage(o, nb);
    wpAddMouseTimingPage(o, nb);
    1
}

// Keyboard.
const ID_RATE: u16 = 610;
const ID_DELAY: u16 = 611;
const ID_BLINK: u16 = 612;

extern "C" fn kbd_timing_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let (d, r) = typematic();
            spin_init(h, ID_RATE, 2, 30, r);
            spin_init(h, ID_DELAY, 250, 1000, d);
            spin_init(h, ID_BLINK, 0, 1000, sysval(SV_CURSORRATE));
            0
        }
        WM_CONTROL if a >> 16 == SPBN_CHANGE => {
            let id = (a & 0xffff) as u16;
            let Some(v) = spin_value(h, id) else { return 0 };
            match id {
                ID_RATE | ID_DELAY => {
                    let (mut d, mut r) = typematic();
                    if id == ID_RATE && (2..=30).contains(&v) {
                        r = v;
                    } else if id == ID_DELAY && (250..=1000).contains(&v) {
                        d = v;
                    } else {
                        return 0;
                    }
                    write_num(b"KeyRepeatRate", r);
                    write_num(b"KeyRepeatDelay", d);
                    set_typematic(d, r);
                }
                ID_BLINK if (0..=1000).contains(&v) => set_sysval(SV_CURSORRATE, b"CursorBlinkRate", v),
                _ => {}
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddKeyboardTimingPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, kbd_timing_proc, alloc::vec![
        book::label(b"Repeat rate (characters a second):", (6, 104, 150)),
        spin((160, 102, 50), ID_RATE),
        book::label(b"Repeat delay (milliseconds):", (6, 86, 150)),
        spin((160, 84, 50), ID_DELAY),
        book::label(b"Cursor blink rate (milliseconds):", (6, 68, 150)),
        spin((160, 66, 50), ID_BLINK),
    ], b"~Timing\0")
}

extern "C" fn kbd_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPKeyboard"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wpAddKeyboardTimingPage(o, nb);
    1
}

// Sound.
const ID_BEEP: u16 = 620;
const ID_TEST: u16 = 621;

extern "C" fn beep_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            check(h, ID_BEEP, sysval(SV_ALARM) != 0);
            0
        }
        WM_CONTROL if (a & 0xffff) as u16 == ID_BEEP && (a >> 16) == 1 => {
            set_sysval(SV_ALARM, b"Beep", checked(h, ID_BEEP) as i32);
            0
        }
        WM_COMMAND => {
            if a & 0xffff == ID_TEST as u32 {
                // WA_WARNING
                unsafe { WinAlarm(HWND_DESKTOP, 0) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddSoundWarningBeepPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, beep_proc, alloc::vec![
        book::label(b"Warning beep:", (6, 104, 200)),
        // BS_AUTOCHECKBOX
        item(WC_BUTTON, b"~Beep when a warning or error is shown", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (10, 90, 220, 10), ID_BEEP),
        item(WC_BUTTON, b"~Test", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (10, 66, 54, 14), ID_TEST),
    ], b"~Warning beep\0")
}

extern "C" fn sound_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPSound"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wpAddSoundWarningBeepPage(o, nb);
    1
}

// ---- The palettes ----

/// The colour palette's colours.
const PALETTE: [u32; 30] = [
    0x000000, 0x404040, 0x808080, 0xc0c0c0, 0xcccccc, 0xffffff,
    0x800000, 0xff0000, 0xff8080, 0x804000, 0xff8000, 0xffc080,
    0x808000, 0xffff00, 0xffff80, 0x008000, 0x00ff00, 0x80ff80,
    0x008080, 0x2e6e8e, 0x00ffff, 0x000080, 0x0000ff, 0x8080ff,
    0x800080, 0xff00ff, 0xff80ff, 0x4b0082, 0x6a5acd, 0xe6e6fa,
];

/// The schemes: (name, colours in COLORS' order).
const SCHEMES: [(&[u8], [u32; 16]); 6] = [
    (b"Warp 4", [0x2e6e8e, 0x000080, 0x808080, 0xffffff, 0xcccccc, 0xcccccc, 0xffffff, 0xcccccc, 0x808080, 0xffffff, 0x000000, 0x000080, 0xffffff, 0x000000, 0xcccccc, 0xcccccc]),
    (b"Arctic", [0x9cb4c8, 0x4a6e96, 0xa0aab4, 0xffffff, 0xe8eef4, 0xdce6f0, 0xffffff, 0xdce6f0, 0x8296aa, 0xffffff, 0x10202c, 0x4a6e96, 0xffffff, 0x203040, 0xdce6f0, 0xdce6f0]),
    (b"Bright Blue", [0x0050c8, 0x0000ff, 0x7890c0, 0xffffff, 0xe0e8ff, 0xc8d8f8, 0xffffff, 0xc8d8f8, 0x6878a8, 0xffffff, 0x000040, 0x0000ff, 0xffffff, 0x000040, 0xc8d8f8, 0xc8d8f8]),
    (b"Dusk", [0x3c2846, 0x6e3c64, 0x5a5064, 0xffe6c8, 0xc8beb4, 0xd2c8be, 0xf0e8e0, 0xd2c8be, 0x8c7c78, 0xfaf5f0, 0x281e1e, 0x6e3c64, 0xffffff, 0x281e1e, 0xd2c8be, 0xd2c8be]),
    (b"High Contrast", [0x000000, 0x00ffff, 0x404040, 0x000000, 0xffffff, 0x000000, 0xffffff, 0x000000, 0xc0c0c0, 0x000000, 0xffffff, 0x00ffff, 0x000000, 0xffffff, 0x000000, 0x000000]),
    (b"Monochrome", [0x808080, 0x000000, 0xa0a0a0, 0xffffff, 0x000000, 0xffffff, 0xffffff, 0xffffff, 0x000000, 0xffffff, 0x000000, 0x000000, 0xffffff, 0x000000, 0xffffff, 0xffffff]),
];

/// The font palette's fonts (PP_FONTNAMESIZE names).
const FONTS: [&[u8]; 12] = [
    b"8.Helv", b"10.Helv", b"12.Helv", b"10.Helvetica Bold", b"14.Helvetica", b"18.Helvetica Bold",
    b"10.Times New Roman", b"12.Times New Roman", b"14.Times New Roman Bold", b"10.Courier", b"12.Courier", b"9.WarpSans",
];

const PAL_CLASS: &[u8] = b"WPS_Palette\0";
const COLOR_CELL: (i32, i32, i32) = (28, 22, 6);
const SCHEME_CELL: (i32, i32, i32) = (108, 74, 3);
const FONT_CELL: (i32, i32, i32) = (224, 30, 2);

/// What a palette holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Colors,
    Schemes,
    Fonts,
}

struct Pal {
    obj: usize,
    frame: u32,
    client: u32,
    kind: Kind,
    /// The cell being dragged.
    dragging: Option<usize>,
}

static mut PALS: Vec<Pal> = Vec::new();

#[allow(static_mut_refs)]
fn pals() -> &'static mut Vec<Pal> {
    unsafe { &mut PALS }
}

fn hab() -> u32 {
    unsafe { WinQueryAnchorBlock(HWND_DESKTOP) }
}

fn cells(kind: Kind) -> usize {
    match kind {
        Kind::Colors => PALETTE.len(),
        Kind::Schemes => SCHEMES.len(),
        Kind::Fonts => FONTS.len(),
    }
}

/// (cell width, height, columns).
fn cell_size(kind: Kind) -> (i32, i32, i32) {
    match kind {
        Kind::Colors => COLOR_CELL,
        Kind::Schemes => SCHEME_CELL,
        Kind::Fonts => FONT_CELL,
    }
}

fn client_size(kind: Kind) -> (i32, i32) {
    let (w, h, cols) = cell_size(kind);
    let rows = (cells(kind) as i32 + cols - 1) / cols;
    (cols * (w + 6) + 6, rows * (h + 6) + 6)
}

fn cell_rect(kind: Kind, height: i32, i: usize) -> RectL {
    let (w, h, cols) = cell_size(kind);
    let (c, r) = (i as i32 % cols, i as i32 / cols);
    let x = 6 + c * (w + 6);
    let t = height - 6 - r * (h + 6);
    RectL { l: x, b: t - h, r: x + w, t }
}

fn hit(p: usize, x: i32, y: i32) -> Option<usize> {
    let mut all = RectL::default();
    unsafe { WinQueryWindowRect(pals()[p].client, &mut all) };
    let kind = pals()[p].kind;
    (0..cells(kind)).find(|&i| {
        let r = cell_rect(kind, all.t, i);
        x >= r.l && x < r.r && y >= r.b && y < r.t
    })
}

fn fill(hps: u32, l: i32, b: i32, r: i32, t: i32, c: u32) {
    unsafe { WinFillRect(hps, &RectL { l, b, r, t }, c as i32) };
}

fn paint(h: u32) {
    let Some(p) = pals().iter().position(|p| p.client == h) else { return };
    let kind = pals()[p].kind;
    unsafe {
        let mut r = RectL::default();
        let hps = WinBeginPaint(h, 0, &mut r);
        // LCOLF_RGB
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
        let mut all = RectL::default();
        WinQueryWindowRect(h, &mut all);
        fill(hps, all.l, all.b, all.r, all.t, WinQuerySysColor(HWND_DESKTOP, -34, 0));
        for i in 0..cells(kind) {
            let c = cell_rect(kind, all.t, i);
            fill(hps, c.l - 1, c.b - 1, c.r + 1, c.t + 1, 0x000000);
            if kind == Kind::Colors {
                fill(hps, c.l, c.b, c.r, c.t, PALETTE[i]);
                continue;
            }
            if kind == Kind::Fonts {
                fill(hps, c.l, c.b, c.r, c.t, 0xffffff);
                font_sample(hps, i, c);
                continue;
            }
            // A small picture of the scheme: the desktop, a window with
            // its title bar and text, and the name below.
            let (name, s) = SCHEMES[i];
            fill(hps, c.l, c.b, c.r, c.t, s[0]);
            let (wl, wr, wt, wb) = (c.l + 8, c.r - 8, c.t - 6, c.b + 22);
            fill(hps, wl, wb, wr, wt, s[13]);
            fill(hps, wl + 1, wt - 11, wr - 1, wt - 1, s[1]);
            fill(hps, wl + 1, wb + 1, wr - 1, wt - 11, s[9]);
            fill(hps, wl + 4, wb + 4, wl + 28, wb + 10, s[11]);
            fill(hps, wl + 4, wb + 12, wr - 6, wb + 14, s[10]);
            fill(hps, c.l, c.b, c.r, c.b + 16, s[5]);
            let mut tr = RectL { l: c.l + 1, b: c.b + 1, r: c.r - 1, t: c.b + 15 };
            WinDrawText(hps, name.len() as i32, name.as_ptr(), &mut tr, s[10] as i32, s[5] as i32, 0x0100 | 0x0400);
        }
        WinEndPaint(hps);
    }
}

/// The window under the pointer that a drop goes to, and the folder
/// view it is part of.
fn drop_target() -> (u32, Option<usize>) {
    let mut pt = PointL::default();
    unsafe { WinQueryPointerPos(HWND_DESKTOP, &mut pt) };
    let target = unsafe { WinWindowFromPoint(HWND_DESKTOP, &pt, 1) };
    let mut h = target;
    // QW_PARENT
    while h != 0 && h != HWND_DESKTOP {
        if let Some(w) = crate::view::wins().iter().position(|w| w.view != OPEN_SETTINGS && (w.child == h || w.client == h || w.frame == h)) {
            return (target, Some(w));
        }
        h = unsafe { WinQueryWindow(h, 5) };
    }
    (target, None)
}

fn drop_cell(p: usize, i: usize, shift: bool) {
    let (target, view) = drop_target();
    if target == 0 || pals().iter().any(|q| q.client == target || q.frame == target) {
        return;
    }
    if pals()[p].kind == Kind::Fonts {
        let name = FONTS[i];
        match view {
            // A folder: its font (kept with the folder).
            Some(w) => crate::fs::set_folder_font(crate::view::wins()[w].obj, Some(name)),
            // PP_FONTNAMESIZE
            None => {
                let z = z(name);
                unsafe { WinSetPresParam(target, 15, z.len() as u32, z.as_ptr() as *const u32) };
            }
        }
        return;
    }
    if pals()[p].kind == Kind::Schemes {
        let s = &SCHEMES[i].1;
        let pairs: Vec<(i32, u32)> = COLORS.iter().zip(s.iter()).map(|(&(_, id), &c)| (id, c)).collect();
        set_colors(&pairs);
        for (&(name, _), &c) in COLORS.iter().zip(s.iter()) {
            let v = alloc::format!("{} {} {}\0", c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff);
            prf_write(HINI_USER, b"PM_Colors", name, Some(v.as_bytes()));
        }
        return;
    }
    let c = PALETTE[i];
    match view {
        // A folder: its background (kept with the folder).
        Some(w) if !shift => crate::fs::set_folder_background(crate::view::wins()[w].obj, Some(c)),
        // PP_FOREGROUNDCOLOR (Shift), PP_BACKGROUNDCOLOR.
        _ => unsafe {
            WinSetPresParam(target, if shift { 1 } else { 3 }, 4, &c);
            WinInvalidateRect(target, core::ptr::null(), 1);
        },
    }
}

extern "C" fn pal_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    let Some(p) = pals().iter().position(|p| p.client == h) else { return unsafe { WinDefWindowProc(h, msg, a, b) } };
    let pos = |v: u32| ((v & 0xffff) as i16 as i32, (v >> 16) as i16 as i32);
    match msg {
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN => {
            let (x, y) = pos(a);
            if let Some(i) = hit(p, x, y) {
                pals()[p].dragging = Some(i);
                unsafe { WinSetCapture(HWND_DESKTOP, h) };
            }
            unsafe { WinDefWindowProc(h, msg, a, b) }
        }
        WM_MOUSEMOVE if pals()[p].dragging.is_some() => {
            // SPTR_MOVE while a colour is carried.
            unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, 5, 0)) };
            1
        }
        WM_BUTTON1UP => {
            if let Some(i) = pals()[p].dragging.take() {
                unsafe {
                    WinSetCapture(HWND_DESKTOP, 0);
                    WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, SPTR_ARROW, 0));
                }
                // KC_SHIFT in mp2's high word.
                drop_cell(p, i, (b >> 16) & 0x08 != 0);
            }
            1
        }
        WM_CLOSE => {
            let f = pals()[p].frame;
            pals().remove(p);
            unsafe { WinDestroyWindow(f) };
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

fn open_palette(o: usize, kind: Kind) -> u32 {
    if let Some(p) = pals().iter().find(|p| p.obj == o) {
        unsafe { WinSetWindowPos(p.frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE) };
        return p.frame;
    }
    static mut REGISTERED: bool = false;
    unsafe {
        if !REGISTERED {
            WinRegisterClass(hab(), PAL_CLASS.as_ptr(), pal_proc, 0, 0);
            REGISTERED = true;
        }
    }
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_DLGBORDER | FCF_TASKLIST;
    let mut client = 0;
    let t = z(&crate::obj::title(o));
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, PAL_CLASS.as_ptr(), t.as_ptr(), 0, 0, 1, &mut client) };
    if frame == 0 {
        return 0;
    }
    pals().push(Pal { obj: o, frame, client, kind, dragging: None });
    let (mut fr, mut cr) = (RectL::default(), RectL::default());
    unsafe {
        WinSetWindowPos(frame, 0, 0, 0, 200, 200, SWP_SIZE);
        WinQueryWindowRect(frame, &mut fr);
        WinQueryWindowRect(client, &mut cr);
    }
    let (cw, ch) = client_size(kind);
    let (fw, fh) = (cw + (fr.r - fr.l) - (cr.r - cr.l), ch + (fr.t - fr.b) - (cr.t - cr.b));
    let (sw, sh) = (unsafe { WinQuerySysValue(HWND_DESKTOP, 20) }, unsafe { WinQuerySysValue(HWND_DESKTOP, 21) });
    unsafe { WinSetWindowPos(frame, HWND_TOP, (sw - fw) / 2, (sh - fh) / 2, fw, fh, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE) };
    frame
}

fn palette_open(class: &str, kind: Kind, o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd(class), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32;
    }
    open_palette(o, kind)
}

extern "C" fn color_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    palette_open("WPColorPalette", Kind::Colors, o, cnr, view, param)
}

extern "C" fn scheme_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    palette_open("WPSchemePalette", Kind::Schemes, o, cnr, view, param)
}

extern "C" fn font_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    palette_open("WPFontPalette", Kind::Fonts, o, cnr, view, param)
}

methods! {
    __m_wpAddMouseTimingPage => wpAddMouseTimingPage, 2;
    __m_wpAddMouseTypePage => wpAddMouseTypePage, 2;
    __m_mouse_add_pages => mouse_add_pages, 2;
    __m_wpAddKeyboardTimingPage => wpAddKeyboardTimingPage, 2;
    __m_kbd_add_pages => kbd_add_pages, 2;
    __m_wpAddSoundWarningBeepPage => wpAddSoundWarningBeepPage, 2;
    __m_sound_add_pages => sound_add_pages, 2;
    __m_color_open => color_open, 4;
    __m_scheme_open => scheme_open, 4;
    __m_font_open => font_open, 4;
}

pub fn impl_mouse() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![("wpAddMouseTimingPage", __m_wpAddMouseTimingPage as usize), ("wpAddMouseTypePage", __m_wpAddMouseTypePage as usize)],
        overrides: alloc::vec![("wpAddSettingsPages", __m_mouse_add_pages as usize)],
    }
}

pub fn impl_keyboard() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![("wpAddKeyboardTimingPage", __m_wpAddKeyboardTimingPage as usize)],
        overrides: alloc::vec![("wpAddSettingsPages", __m_kbd_add_pages as usize)],
    }
}

pub fn impl_sound() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![("wpAddSoundWarningBeepPage", __m_wpAddSoundWarningBeepPage as usize)],
        overrides: alloc::vec![("wpAddSettingsPages", __m_sound_add_pages as usize)],
    }
}

pub fn impl_color_palette() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_color_open as usize)] }
}

pub fn impl_scheme_palette() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_scheme_open as usize)] }
}

pub fn impl_font_palette() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpOpen", __m_font_open as usize)] }
}

/// A font palette cell: the font's name in that font, centred.
fn font_sample(hps: u32, i: usize, c: RectL) {
    let name = FONTS[i];
    let Some(dot) = name.iter().position(|&b| b == b'.') else { return };
    let pt: i32 = core::str::from_utf8(&name[..dot]).ok().and_then(|t| t.parse().ok()).unwrap_or(10);
    let face = &name[dot + 1..];
    let (face, bold) = match face.strip_suffix(b" Bold") {
        Some(f) => (f, true),
        None => (face, false),
    };
    // Shown at no more than 14 points, to fit the cell.
    let px = (pt.min(14) * 96 + 36) / 72;
    // FATTRS: fsSelection (FATTR_SEL_BOLD), szFacename, usCodePage,
    // lMaxBaselineExt; image faces at that size, the others as outlines
    // (FATTR_FONTUSE_OUTLINE) with an em-square character box.
    let image = matches!(face, b"Helv" | b"Tms Rmn" | b"WarpSans" | b"System Proportional");
    let mut fat = [0u8; 56];
    fat[0..2].copy_from_slice(&56u16.to_le_bytes());
    fat[2..4].copy_from_slice(&(if bold { 0x20u16 } else { 0 }).to_le_bytes());
    fat[8..8 + face.len().min(31)].copy_from_slice(&face[..face.len().min(31)]);
    fat[42..44].copy_from_slice(&850u16.to_le_bytes());
    fat[44..48].copy_from_slice(&(px * 9 / 8).to_le_bytes());
    if !image {
        fat[54..56].copy_from_slice(&4u16.to_le_bytes());
    }
    unsafe {
        GpiCreateLogFont(hps, core::ptr::null(), 1, fat.as_ptr());
        GpiSetCharSet(hps, 1);
        if !image {
            GpiSetCharBox(hps, &[px << 16, px << 16]);
        }
        let mut tr = RectL { l: c.l + 2, b: c.b, r: c.r - 2, t: c.t };
        // DT_CENTER | DT_VCENTER
        WinDrawText(hps, name.len() as i32, name.as_ptr(), &mut tr, 0, 0xffffff, 0x0100 | 0x0400);
        GpiSetCharSet(hps, 0);
        GpiDeleteSetId(hps, 1);
    }
}
