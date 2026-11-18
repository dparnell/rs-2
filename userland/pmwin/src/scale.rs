//! The UI scale for high-dpi screens: `SET RS2SCALE=150` in CONFIG.SYS
//! (Display sets it) makes PM's controls, frames, menus, system bitmaps,
//! icons, pointers, dialog units and fonts that much bigger while the
//! screen keeps its resolution, as OS/2's "large fonts" (120 dpi)
//! display drivers did: DevQueryCaps reports 96 x scale / 100 as the
//! font resolution, so programs that size their fonts in points get
//! them bigger too.
//!
//! The first PM process reads it into the shared state; it changes at
//! the next start (programs keep the metrics they asked for).

use crate::state::st;

/// The scale in percent: 100 (none) to 300, in steps of 25.
pub fn pct() -> i32 {
    st().scale
}

/// RS2SCALE from this process's environment (the first PM process).
pub fn from_env() -> i32 {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"RS2SCALE\0".as_ptr(), &mut v) } != 0 || v.is_null() {
        return 100;
    }
    let t = unsafe { core::ffi::CStr::from_ptr(v as *const core::ffi::c_char) }.to_bytes().trim_ascii();
    let t = t.strip_suffix(b"%").unwrap_or(t);
    let n: i32 = core::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(100);
    ((n.clamp(100, 300) + 12) / 25) * 25
}

/// `n` pels at 96 dpi, scaled (rounded to the nearest pel).
pub fn px(n: i32) -> i32 {
    let p = pct();
    if p == 100 {
        return n;
    }
    let v = n * p;
    if v >= 0 { (v + 50) / 100 } else { -((-v + 50) / 100) }
}

/// Dialog units to pels: 2 pels each at 96 dpi.
pub fn du(n: i32) -> i32 {
    px(2 * n)
}

/// Pels to dialog units.
pub fn from_du(n: i32) -> i32 {
    n * 100 / (2 * pct())
}

/// The screen's font resolution in dots per inch (CAPS_HORIZONTAL_FONT_RES).
pub fn dpi() -> i32 {
    96 * pct() / 100
}
