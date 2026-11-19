//! The UI scale for high-dpi screens: `SET RS2SCALE=150` in CONFIG.SYS
//! (Display sets it) makes PM's controls, frames, menus, system bitmaps,
//! icons, pointers, dialog units and fonts that much bigger while the
//! screen keeps its resolution, as OS/2's "large fonts" (120 dpi)
//! display drivers did: DevQueryCaps reports 96 x scale / 100 as the
//! font resolution, so programs that size their fonts in points get
//! them bigger too.
//!
//! Without RS2SCALE (or with `SET RS2SCALE=AUTO`) the resolution PM
//! starts at chooses it (`auto`). The first PM process reads it into the
//! shared state; it changes at the next start (programs keep the metrics
//! they asked for).

use crate::state::st;

/// The scale in percent: 100 (none) to 300, in steps of 25.
pub fn pct() -> i32 {
    st().scale
}

/// RS2SCALE from this process's environment (the first PM process), for
/// a screen of `w` x `h`: without it (or with `AUTO`) the scale the
/// resolution suggests.
pub fn from_env(w: i32, h: i32) -> i32 {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"RS2SCALE\0".as_ptr(), &mut v) } != 0 || v.is_null() {
        return auto(w, h);
    }
    let t = unsafe { core::ffi::CStr::from_ptr(v as *const core::ffi::c_char) }.to_bytes().trim_ascii();
    let t = t.strip_suffix(b"%").unwrap_or(t);
    match core::str::from_utf8(t).ok().and_then(|s| s.trim().parse::<i32>().ok()) {
        Some(n) => ((n.clamp(100, 300) + 12) / 25) * 25,
        None => auto(w, h),
    }
}

/// The scale for a screen of `w` x `h` when none is set. rs2 does not
/// know the screen's size in inches, so the resolution decides, as a
/// laptop's or monitor's pels per inch grow with it: 1080 or 1200 lines
/// 100%, 1440 to 1600 125%, 1800 to 2000 (a Surface's 1824) 150%, 4K's
/// 2160 200%, 5K and more 250%. The smaller side counts (a screen on
/// its side), and at least 1024 x 768 scaled pels stay.
pub fn auto(w: i32, h: i32) -> i32 {
    let short = w.min(h);
    let p = match short {
        ..1400 => 100,
        ..1800 => 125,
        ..2100 => 150,
        ..2800 => 200,
        _ => 250,
    };
    let fit = (w.max(h) * 100 / 1024).min(short * 100 / 768);
    (p.min(fit) / 25 * 25).max(100)
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
