//! Special Needs: WPCONFIG's WPSpecialNeeds object (`<WP_SPECIALNEEDS>`)
//! and the Keyboard object's Special Needs page
//! (wpAddKeyboardSpecialNeedsPage), rs2's own after IBM's SpecialDlgProc
//! (studied with Ghidra): Special Needs on or off, a time-out after which
//! it turns itself off when no key is pressed, the acceptance delay (how
//! long a key must be held to count), and the key repeat rate and delay
//! while it is on.
//!
//! IBM's hands these to the keyboard driver (IMP32SetAIMProfile); rs2's
//! window manager does the acceptance delay and the time-out on the keys
//! it gets (PMWIN's access.rs, rs2-private system values 4000-4002), and
//! the repeat goes to KBD$ (KBD_SETTYPAMATICRATE) as the Keyboard
//! object's does. The settings are kept in OS2.INI `PM_ControlPanel`
//! (SpecialNeeds, SpecialNeedsTimeOut in minutes, SpecialNeedsAccept in
//! milliseconds, SpecialNeedsRepeatRate in characters a second,
//! SpecialNeedsRepeatDelay in milliseconds) and applied when the shell
//! starts.

#![allow(static_mut_refs)]

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio, spin, spin_init, spin_value};
use crate::som;
use crate::syssetup::{group, text, undo_default, ID_DEFAULT, ID_UNDO, SPBN_CHANGE};
use crate::util::{parse_dec, prf_read, prf_write, HINI_USER};
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMWIN.873"]
    fn WinSetSysValue(desk: u32, i: i32, v: i32) -> u32;
}

const SV_RS2_SPNEEDS: i32 = 4000;
const SV_RS2_SPNEEDS_ACCEPT: i32 = 4001;
const SV_RS2_SPNEEDS_TIMEOUT: i32 = 4002;

const CP: &[u8] = b"PM_ControlPanel";

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub on: bool,
    /// Minutes without a key press before it turns off (0: never).
    pub timeout_min: u32,
    pub accept_ms: u32,
    pub rate_cps: u32,
    pub delay_ms: u32,
}

/// IBM's defaults: off, a 10-minute time-out, keys accepted after a
/// quarter of a second, slow repeat.
pub const DEFAULTS: Settings = Settings { on: false, timeout_min: 10, accept_ms: 250, rate_cps: 10, delay_ms: 1000 };

fn read(key: &[u8]) -> Option<u32> {
    let v = prf_read(HINI_USER, CP, key)?;
    parse_dec(v.strip_suffix(&[0]).unwrap_or(&v).trim_ascii())
}

fn write(key: &[u8], v: u32) {
    let s = alloc::format!("{v}\0");
    prf_write(HINI_USER, CP, key, Some(s.as_bytes()));
}

pub fn load() -> Settings {
    let d = DEFAULTS;
    Settings {
        on: read(b"SpecialNeeds").map_or(d.on, |v| v != 0),
        timeout_min: read(b"SpecialNeedsTimeOut").filter(|v| *v <= 30).unwrap_or(d.timeout_min),
        accept_ms: read(b"SpecialNeedsAccept").filter(|v| *v <= 5000).unwrap_or(d.accept_ms),
        rate_cps: read(b"SpecialNeedsRepeatRate").filter(|v| (1..=30).contains(v)).unwrap_or(d.rate_cps),
        delay_ms: read(b"SpecialNeedsRepeatDelay").filter(|v| (250..=2000).contains(v)).unwrap_or(d.delay_ms),
    }
}

/// Keep the settings and give them to PMWIN and the keyboard.
pub fn store(s: &Settings) {
    write(b"SpecialNeeds", s.on as u32);
    write(b"SpecialNeedsTimeOut", s.timeout_min);
    write(b"SpecialNeedsAccept", s.accept_ms);
    write(b"SpecialNeedsRepeatRate", s.rate_cps);
    write(b"SpecialNeedsRepeatDelay", s.delay_ms);
    apply_settings(s);
}

fn apply_settings(s: &Settings) {
    unsafe {
        WinSetSysValue(HWND_DESKTOP, SV_RS2_SPNEEDS_ACCEPT, s.accept_ms as i32);
        WinSetSysValue(HWND_DESKTOP, SV_RS2_SPNEEDS_TIMEOUT, (s.timeout_min * 60) as i32);
        WinSetSysValue(HWND_DESKTOP, SV_RS2_SPNEEDS, s.on as i32);
    }
    if s.on {
        crate::setup::set_typematic(s.delay_ms as i32, s.rate_cps as i32);
    } else {
        let (d, r) = crate::setup::typematic();
        crate::setup::set_typematic(d, r);
    }
}

/// When the shell starts.
pub fn apply() {
    let s = load();
    if s.on {
        apply_settings(&s);
    }
}

// ---- The page ----

const ID_ON: u16 = 0x326;
const ID_OFF: u16 = 0x327;
const ID_TIMEOUT: u16 = 0x321;
const ID_ACCEPT: u16 = 0x322;
const ID_RATE: u16 = 0x323;
const ID_DELAY: u16 = 0x324;
const ID_NOTE: u16 = 0x325;

static mut UNDO: Settings = DEFAULTS;

fn fill(h: u32) {
    let s = load();
    check(h, if s.on { ID_ON } else { ID_OFF }, true);
    spin_init(h, ID_TIMEOUT, 0, 30, s.timeout_min as i32);
    spin_init(h, ID_ACCEPT, 0, 5000, s.accept_ms as i32);
    spin_init(h, ID_RATE, 1, 30, s.rate_cps as i32);
    spin_init(h, ID_DELAY, 250, 2000, s.delay_ms as i32);
    for id in [ID_TIMEOUT, ID_ACCEPT, ID_RATE, ID_DELAY] {
        crate::syssetup::enable(h, id, s.on);
    }
}

extern "C" fn page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe {
                WinSetWindowULong(h, QWL_USER, b);
                UNDO = load();
            }
            fill(h);
            0
        }
        WM_CONTROL => {
            let mut s = load();
            let id = (a & 0xffff) as u16;
            match (id, a >> 16) {
                (ID_ON | ID_OFF, 1) => s.on = checked(h, ID_ON),
                (ID_TIMEOUT | ID_ACCEPT | ID_RATE | ID_DELAY, SPBN_CHANGE) => {
                    let Some(v) = spin_value(h, id) else { return 0 };
                    let v = v.max(0) as u32;
                    match id {
                        ID_TIMEOUT if v <= 30 => s.timeout_min = v,
                        ID_ACCEPT if v <= 5000 => s.accept_ms = v,
                        ID_RATE if (1..=30).contains(&v) => s.rate_cps = v,
                        ID_DELAY if (250..=2000).contains(&v) => s.delay_ms = v,
                        _ => return 0,
                    }
                }
                _ => return 0,
            }
            if s != load() {
                store(&s);
            }
            for id in [ID_TIMEOUT, ID_ACCEPT, ID_RATE, ID_DELAY] {
                crate::syssetup::enable(h, id, s.on);
            }
            0
        }
        WM_COMMAND => {
            let s = match (a & 0xffff) as u16 {
                ID_UNDO => unsafe { UNDO },
                ID_DEFAULT => DEFAULTS,
                _ => return 0,
            };
            store(&s);
            fill(h);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// The Special Needs page (also the Keyboard object's).
pub extern "C" fn wpAddKeyboardSpecialNeedsPage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        group(b"Special needs activation", (4, 92, 228, 26)),
        radio(b"~On", (10, 98, 60), ID_ON, true),
        radio(b"O~ff", (80, 98, 60), ID_OFF, false),
        book::label(b"Turn off after (minutes, 0 never):", (6, 78, 150)),
        spin((160, 76, 40), ID_TIMEOUT),
        book::label(b"Acceptance delay (milliseconds):", (6, 64, 150)),
        spin((160, 62, 50), ID_ACCEPT),
        book::label(b"Repeat rate (characters a second):", (6, 50, 150)),
        spin((160, 48, 40), ID_RATE),
        book::label(b"Repeat delay (milliseconds):", (6, 36, 150)),
        spin((160, 34, 50), ID_DELAY),
        text(b"A key counts only when held for the acceptance delay.", (6, 22, 226), ID_NOTE),
    ];
    items.extend(undo_default(2));
    add_page(o, nb, page_proc, items, b"~Special Needs\0")
}

extern "C" fn spn_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPSpecialNeeds"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wpAddKeyboardSpecialNeedsPage(o, nb);
    1
}

methods! {
    __m_spn_add_pages => spn_add_pages, 2;
    __m_wpAddKeyboardSpecialNeedsPage => wpAddKeyboardSpecialNeedsPage, 2;
}

/// For WPKeyboard's method table.
pub fn keyboard_method() -> (&'static str, usize) {
    ("wpAddKeyboardSpecialNeedsPage", __m_wpAddKeyboardSpecialNeedsPage as usize)
}

pub fn impl_special_needs() -> Impl {
    Impl { data: 0, methods: alloc::vec::Vec::new(), overrides: alloc::vec![("wpAddSettingsPages", __m_spn_add_pages as usize), crate::pages::no_window_page_override()] }
}
