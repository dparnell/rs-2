//! The System Setup objects IBM keeps in WPCONFIG.DLL and WINCFG.DLL
//! that the other modules do not provide: WPCountry (country.rs),
//! WPPower (power.rs), WPSpecialNeeds (spneeds.rs) and WPWinConfig, the
//! WIN-OS/2 Setup object (wincfg.rs). This module gives wps.rs their
//! implementations, their metaclasses' titles and icons (rs2's own, from
//! pmwin::setupicons) and the entries that put them in System Setup.

use alloc::vec::Vec;

use crate::methods;
use crate::pm::*;
use crate::som;
use crate::wps::Impl;

pub use pmwin::setupicons as si;

/// The classes this module and its siblings implement.
pub fn has(name: &str) -> bool {
    matches!(
        name,
        "WPCountry" | "M_WPCountry" | "WPPower" | "M_WPPower" | "WPSpecialNeeds" | "M_WPSpecialNeeds" | "WPWinConfig" | "M_WPWinConfig"
    )
}

pub fn implementation(name: &str) -> Impl {
    match name {
        "WPCountry" => crate::country::impl_country(),
        "WPPower" => crate::power::impl_power(),
        "M_WPPower" => crate::power::impl_m_power(),
        "WPSpecialNeeds" => crate::spneeds::impl_special_needs(),
        "WPWinConfig" => crate::wincfg::impl_win_config(),
        _ => impl_meta(),
    }
}

// ---- The metaclasses: titles and icons ----

/// The class titles (rs2's wording).
fn title_of(class: &[u8]) -> Option<&'static [u8]> {
    Some(match class {
        b"WPCountry" => b"Country\0",
        b"WPPower" => b"Power\0",
        b"WPSpecialNeeds" => b"Special Needs\0",
        b"WPWinConfig" => b"WIN-OS/2 Setup\0",
        _ => return None,
    })
}

fn icon_id(class: &[u8]) -> u32 {
    match class {
        b"WPCountry" => si::COUNTRY,
        b"WPPower" => si::POWER,
        b"WPSpecialNeeds" => si::SPECIAL_NEEDS,
        b"WPWinConfig" => si::WINOS2,
        _ => 0,
    }
}

/// The class and its ancestors, nearest first.
fn ancestors(cls: usize) -> Vec<usize> {
    let mut v = Vec::new();
    let mut c = cls;
    while c != 0 && v.len() < 32 {
        v.push(c);
        c = som::sc_call(som::sc::SOM_GET_PARENT, &[c]);
    }
    v
}

extern "C" fn cls_title(cls: usize) -> *const u8 {
    for c in ancestors(cls) {
        if let Some(t) = title_of(som::class_name(c)) {
            return t.as_ptr();
        }
    }
    som::sc_call(som::sc::SOM_GET_NAME, &[cls]) as *const u8
}

static mut ICONS: [u32; si::LAST as usize + 1] = [0; si::LAST as usize + 1];

/// One of the System Setup icons (made once, kept for the process).
pub fn icon(id: u32) -> u32 {
    if id == 0 || id > si::LAST {
        return 0;
    }
    let c = unsafe { &mut *(&raw mut ICONS) };
    if c[id as usize] == 0 {
        c[id as usize] = si::icon(id).map_or(0, |(big, mini)| make_icon(&big, &mini));
    }
    c[id as usize]
}

extern "C" fn cls_icon(cls: usize) -> u32 {
    for c in ancestors(cls) {
        let id = icon_id(som::class_name(c));
        if id != 0 {
            return icon(id);
        }
    }
    0
}

methods! {
    __m_ss_cls_title => cls_title, 1;
    __m_ss_cls_icon => cls_icon, 1;
}

/// The metaclasses' overrides (M_WPCountry, M_WPPower, ...).
pub fn meta_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpclsQueryTitle", __m_ss_cls_title as usize), ("wpclsQueryIcon", __m_ss_cls_icon as usize)]
}

fn impl_meta() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: meta_overrides() }
}

// ---- System Setup ----

/// `PM_Workplace` keys that say the objects were put in System Setup
/// (once each, so that a desktop of any version gets them, and WIN-OS/2
/// Setup when WIN-OS/2 is installed later).
const INSTALLED_KEY: &[u8] = b"Rs2SystemSetup2";
const WINCFG_KEY: &[u8] = b"Rs2WinOS2Setup";

/// The objects still to make, as IBM's INI.RC places Power and WIN-OS/2
/// Setup (and Warp 3's placed Country) in System Setup:
/// (title;class;location;flags, setup). WIN-OS/2 Setup only where
/// WIN-OS/2 is installed.
pub fn install_entries() -> Vec<(Vec<u8>, Vec<u8>)> {
    use crate::util::{prf_read, prf_write, HINI_USER};
    let mut v: Vec<(&[u8], &[u8])> = Vec::new();
    if prf_read(HINI_USER, b"PM_Workplace", INSTALLED_KEY).is_none() {
        v.push((b"Country;WPCountry;<WP_CONFIG>;PRESERVEOLD", b"NOCOPY=YES;NODELETE=YES;OBJECTID=<WP_CNTRY>"));
        v.push((b"Power;WPPower;<WP_CONFIG>;PRESERVEOLD", b"NOCOPY=YES;NODELETE=YES;OBJECTID=<WP_POWER>"));
        v.push((b"Special Needs;WPSpecialNeeds;<WP_CONFIG>;PRESERVEOLD", b"NOCOPY=YES;NODELETE=YES;OBJECTID=<WP_SPECIALNEEDS>"));
        prf_write(HINI_USER, b"PM_Workplace", INSTALLED_KEY, Some(b"1\0"));
    }
    if crate::wincfg::installed() && prf_read(HINI_USER, b"PM_Workplace", WINCFG_KEY).is_none() {
        v.push((b"WIN-OS/2 Setup;WPWinConfig;<WP_CONFIG>;PRESERVEOLD", b"NOCOPY=YES;NODELETE=YES;OBJECTID=<WP_WINCFG>"));
        prf_write(HINI_USER, b"PM_Workplace", WINCFG_KEY, Some(b"1\0"));
    }
    v.into_iter().map(|(k, s)| (k.to_vec(), s.to_vec())).collect()
}

// ---- Page helpers ----

pub fn group(t: &[u8], (x, y, cx, cy): (i16, i16, i16, i16)) -> Item {
    // SS_GROUPBOX
    item(WC_STATIC, t, WS_VISIBLE | 0x0002, (x, y, cx, cy), 0xffff)
}

pub fn push(t: &[u8], (x, y): (i16, i16), id: u16) -> Item {
    item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP, (x, y, 40, 14), id)
}

pub fn checkbox(t: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    // BS_AUTOCHECKBOX
    item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (x, y, cx, 10), id)
}

pub fn entry((x, y, cx): (i16, i16, i16), id: u16) -> Item {
    // ES_AUTOSCROLL | ES_MARGIN
    item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (x, y, cx, 8), id)
}

pub fn listbox((x, y, cx, cy): (i16, i16, i16, i16), id: u16) -> Item {
    item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (x, y, cx, cy), id)
}

/// A static text that the page changes (a sample, a state).
pub fn text(t: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    item(WC_STATIC, t, WS_VISIBLE | 0x0001, (x, y, cx, 8), id)
}

pub fn enable(h: u32, id: u16, on: bool) {
    unsafe { WinEnableWindow(WinWindowFromID(h, id as u32), on as u32) };
}

/// Undo and Default, with the ids IBM's WPCONFIG pages give them.
pub const ID_UNDO: u16 = 0x66;
pub const ID_DEFAULT: u16 = 0x67;

pub fn undo_default(y: i16) -> [Item; 2] {
    [push(b"~Undo", (4, y), ID_UNDO), push(b"~Default", (48, y), ID_DEFAULT)]
}

pub const SPBN_CHANGE: u32 = 0x20d;
