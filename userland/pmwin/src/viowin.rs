//! Windows that show windowed VIO sessions (PMVIOWIN.EXE's clients).
//!
//! The host registers each client window with its session
//! (`Rs2PmVioWindow`, private ordinal 9002). While the window has the
//! focus, the input thread hands keys straight to the session in the
//! kernel (RS2API `Rs2VioKey`) with everything the keyboard reported
//! (scan code, extended-key flag, the full shift state, key releases),
//! so the session sees them exactly as on its full screen, in the order
//! they were typed, even while the host is busy. PM keeps Alt+Space and
//! Alt+F4 (the frame's system menu and Close), Shift+PgUp and Shift+PgDn
//! (the window's scroll back) and, while the host marks text, every key
//! (the window gets WM_CHAR as usual).

use alloc::vec::Vec;

use crate::input::Input;
use crate::state::st;

unsafe extern "C" {
    #[link_name = "RS2API.14"]
    fn Rs2VioKey(sid: u32, ev: *const Input) -> u32;
}

/// Rs2PmVioWindow flags: keys go to the session (not as WM_CHAR).
pub const VW_DIRECT: u32 = 0x01;
/// The session is on the full screen (its frame is hidden): the window
/// list shows the session instead of the frame.
pub const VW_FULLSCREEN: u32 = 0x02;

/// Rs2PmVioWindow(hwnd, sid, fl): `hwnd` shows session `sid` (0: no
/// longer), with VW_* flags.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2PmVioWindow(hwnd: u32, sid: u32, fl: u32) -> u32 {
    let s = st();
    let mine = crate::state::pid();
    s.vio.retain(|v| v.0 != hwnd && s.desk.exists(v.0));
    if sid != 0 && s.desk.exists(hwnd) {
        s.vio.push((hwnd, sid, fl, mine));
    }
    1
}

/// The session window `h` shows, with its flags.
pub fn session_of(h: u32) -> Option<(u32, u32)> {
    st().vio.iter().find(|v| v.0 == h).map(|v| (v.1, v.2))
}

/// Frames of sessions that are on the full screen now.
pub fn fullscreen_frames() -> Vec<u32> {
    let s = st();
    s.vio.iter().filter(|v| v.2 & VW_FULLSCREEN != 0).filter_map(|v| s.desk.get(v.0).map(|w| w.parent)).collect()
}

/// A process ended: its registrations go.
pub fn process_gone(pid: u32) {
    st().vio.retain(|v| v.3 != pid);
}

/// A key for window `target` (input thread, lock held): true if it went
/// to the session the window shows.
pub fn key(target: u32, ev: &Input) -> bool {
    let Some((sid, fl)) = session_of(target) else { return false };
    // In menu mode (the system menu is open) keys belong to the menu.
    let s = st();
    let menu = s.queues.get(&s.queue_of(target)).is_some_and(|q| q.menu_mode.is_some());
    if fl & VW_DIRECT == 0 || menu {
        return false;
    }
    let scan = (ev.a >> 8) & 0xff;
    let alt = ev.flags & 0x08 != 0;
    // The frame's own keys: Alt+Space (system menu), Alt+F4 (close).
    if alt && (scan == 0x39 || scan == 0x3e) {
        return false;
    }
    // Shift+PgUp, Shift+PgDn: the scroll back (not with Ctrl or Alt).
    if ev.flags & 0x03 != 0 && ev.flags & 0x0c == 0 && (scan == 0x49 || scan == 0x51) {
        return false;
    }
    unsafe { Rs2VioKey(sid, ev) };
    true
}
