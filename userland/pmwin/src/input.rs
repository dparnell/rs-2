//! Routing raw input (the PM server's input thread): every key and mouse
//! event goes straight to the queue of the thread that owns the target
//! window at the moment it happens. Nothing waits for the previous event
//! to be processed, so a thread that stops reading its queue only loses
//! its own input.
//!
//! Frames whose thread does not respond are handled here: a click
//! activates them (WM_ACTIVATE and friends are posted for later), their
//! title bar and sizing border move and size them, their min/max buttons
//! hide them.

use gpi::Point;
use pmwin::HWND_DESKTOP;

use crate::frame::{self, FID_MINMAX, FID_SYSMENU, FID_TITLEBAR};
use crate::msg::post_queue;
use crate::state::{st, QMsg};
use crate::win::{self, *};
use crate::{menu, pointer};

/// INPUTEVENT from Rs2ReadInput.
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Input {
    pub kind: u16,
    pub flags: u16,
    pub a: u32,
    pub b: u32,
    pub time: u32,
}

pub const EV_KEY: u16 = 1;
pub const EV_MOUSE: u16 = 2;
pub const EV_REPAINT: u16 = 3;
pub const EV_PROCEXIT: u16 = 4;
pub const EV_BACKGROUND: u16 = 5;
pub const EV_MODE: u16 = 6;
pub const EV_HOSTSIZE: u16 = 7;

fn pack(x: i32, y: i32) -> u32 {
    (x as u32 & 0xffff) | (y as u32) << 16
}

/// Input played back by a journal playback hook (not recorded again).
pub static PLAYBACK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Post an input message to the window's queue, stamped with the
/// pointer position and time of the event. The system queue's messages
/// as IBM's PM has them (mouse moves, buttons, WM_CHAR) are marked for
/// the journal record hook.
pub fn post_input(h: u32, msg: u32, mp1: u32, mp2: u32, time: u32) {
    let s = st();
    let q = s.queue_of(h);
    let p = s.pointer;
    let raw = (WM_MOUSEMOVE..=WM_CHAR).contains(&msg) && !PLAYBACK.load(core::sync::atomic::Ordering::Relaxed);
    let reserved = if raw { crate::hooks::QMSG_INPUT } else { 0 };
    post_queue(q, QMsg { hwnd: h, msg, mp1, mp2, time, x: p.x, y: p.y, reserved });
}

/// A mouse event: the pointer moved to `p` and/or the buttons changed.
pub fn mouse(p: Point, buttons: u8, shift: u8, time: u32) {
    // Mouse messages carry KC_SHIFT, KC_CTRL and KC_ALT in mp2's high word.
    let kc = (if shift & 0x3 != 0 { 0x08 } else { 0 }) | (if shift & 0x4 != 0 { 0x10 } else { 0 }) | (if shift & 0x8 != 0 { 0x20 } else { 0 });
    let mp2 = (kc as u32) << 16;
    let s = st();
    // SV_SWAPBUTTON: the right button is button 1 (left-handed).
    let buttons = if s.sysvals.get(&0).is_some_and(|&v| v != 0) { (buttons & !3) | (buttons & 1) << 1 | (buttons >> 1 & 1) } else { buttons };
    pointer::move_to(s, p);
    let prev = core::mem::replace(&mut s.buttons, buttons);
    // VK_BUTTON1..3.
    for i in 0..3 {
        set_key(s, 1 + i, buttons >> i & 1 != 0);
    }
    note_shift(s, shift as u32);
    let down = buttons & !prev;
    let up = prev & !buttons;
    // Moving or sizing a frame.
    if s.drag.is_some() {
        frame::track(p, buttons);
        return;
    }
    // A window of a thread that does not respond keeps no capture.
    if s.capture != 0 && s.is_hung(s.capture) {
        s.capture = 0;
    }
    let target = if s.capture != 0 { s.capture } else { s.desk.window_at(p) };
    if target == 0 {
        return;
    }
    // A window the pointer comes to starts with the arrow (its
    // WM_MOUSEMOVE processing may set another shape).
    if st().ptr_window != target {
        st().ptr_window = target;
        if st().capture == 0 {
            pointer::set_system(pmwin::sysptr::SPTR_ARROW);
        }
    }
    // Disabled windows (the owner of a modal dialog) get no input; a
    // click brings up the window that disabled it.
    if s.capture == 0 && !input_enabled(target) {
        if down != 0 {
            let top = win::top_level(target);
            let modal = st().desk.get(HWND_DESKTOP).and_then(|w| {
                w.children.iter().rev().copied().find(|&c| st().desk.get(c).is_some_and(|x| x.owner == top && x.visible_style()))
            });
            if let Some(d) = modal {
                win::activate(d);
            }
        }
        return;
    }
    if down & 1 != 0 && st().is_hung(target) {
        let (lt, lp, lb) = st().last_click;
        let dbl = lb == 1 && time.wrapping_sub(lt) < dbl_time() && (lp.x - p.x).abs() < 4 && (lp.y - p.y).abs() < 4;
        if hung_click(target, p, dbl) {
            st().last_click = if dbl { (0, Point::new(-100, -100), 0) } else { (time, p, 1) };
            return;
        }
    }
    let o = st().desk.origin(target);
    let mp1 = pack(p.x - o.x, p.y - o.y);
    if down & 1 != 0 && target != HWND_DESKTOP && st().capture == 0 && !menu::is_menu(target) {
        win::activate(target);
    }
    if down == 0 && up == 0 {
        post_input(target, WM_MOUSEMOVE, mp1, mp2, time);
        ext_move(p, mp2, time);
    }
    // Button 1 left, 2 right, 3 middle: WM_BUTTONnDOWN = 0x71 + 3(n-1),
    // a second press nearby within SV_DBLCLKTIME is WM_BUTTONnDBLCLK.
    for (bit, n) in [(1u8, 0u32), (2, 1), (4, 2)] {
        if down & bit != 0 {
            let (lt, lp, lb) = st().last_click;
            let dbl = lb == bit && time.wrapping_sub(lt) < dbl_time() && (lp.x - p.x).abs() < 4 && (lp.y - p.y).abs() < 4;
            st().last_click = if dbl { (0, Point::new(-100, -100), 0) } else { (time, p, bit) };
            let msg = if dbl { WM_BUTTON1DOWN + 2 } else { WM_BUTTON1DOWN } + 3 * n;
            post_input(target, msg, mp1, mp2, time);
            ext_down(target, msg, p, time);
            translate(target, msg, mp1, mp2, time);
        }
        if up & bit != 0 {
            let msg = WM_BUTTON1UP + 3 * n;
            post_input(target, msg, mp1, mp2, time);
            ext_up(target, msg, p, mp1, mp2, time);
            translate(target, msg, mp1, mp2, time);
        }
    }
}

// ---- Extended mouse messages ----
//
// As IBM's PMWIN makes them (ProcessSQmsg, TranslateMouseMsg): after a
// button goes up where it went down (within the double-click time, inside
// an icon-sized box, on the same window) WM_BUTTONnCLICK; when the
// pointer leaves the double-click box with the button down
// WM_BUTTONnMOTIONSTART, and WM_BUTTONnMOTIONEND when it goes up. The
// mouse mapping then turns these into WM_SINGLESELECT, WM_OPEN,
// WM_CONTEXTMENU, WM_BEGINDRAG and so on. OS/2's Mahjongg selects tiles
// on WM_BUTTON1CLICK.

const WM_BUTTON1MOTIONSTART: u32 = 0x0411;
const WM_BUTTON1CLICK: u32 = 0x0413;
const WM_MOUSEENTER: u32 = 0x041e;
const WM_MOUSELEAVE: u32 = 0x041f;

/// IBM's default mouse mapping: (message, required KC_ flags in mp2's high
/// word or -1 for any, what it becomes).
const MOUSE_MAP: [(u32, i32, u32); 9] = [
    (0x0414, -1, 0x0420),   // button 2 motion start: WM_BEGINDRAG
    (0x0415, -1, 0x0421),   // button 2 motion end: WM_ENDDRAG
    (0x0413, -1, 0x0422),   // button 1 click: WM_SINGLESELECT
    (0x0073, -1, 0x0423),   // button 1 double click: WM_OPEN
    (0x0416, 0, 0x0424),    // button 2 click: WM_CONTEXTMENU
    (0x0074, 0x20, 0x0429), // Alt+button 2 down: WM_PICKUP
    (0x0071, 0x20, 0x0426), // Alt+button 1 down: WM_TEXTEDIT
    (0x0411, -1, 0x0427),   // button 1 motion start: WM_BEGINSELECT
    (0x0412, -1, 0x0428),   // button 1 motion end: WM_ENDSELECT
];

struct Ext {
    /// The press that may become a click: its message, window, deadline
    /// and box.
    click: Option<(u32, u32, u32, gpi::Rect)>,
    /// The press that may become a motion: its message, window, box, and
    /// whether MOTIONSTART went out.
    motion: Option<(u32, u32, gpi::Rect, bool)>,
    /// The window the pointer was last over (WM_MOUSEENTER/LEAVE).
    over: u32,
}

static mut EXT: Ext = Ext { click: None, motion: None, over: 0 };

fn ext() -> &'static mut Ext {
    unsafe { &mut *core::ptr::addr_of_mut!(EXT) }
}

fn around(p: Point, cx: i32, cy: i32) -> gpi::Rect {
    gpi::Rect::new(p.x - cx / 2, p.y - cy / 2, p.x + cx / 2 + 1, p.y + cy / 2 + 1)
}

/// The mouse mapping: post what a mouse message stands for.
fn translate(h: u32, msg: u32, mp1: u32, mp2: u32, time: u32) {
    for &(trigger, kc, out) in &MOUSE_MAP {
        if trigger == msg && (kc < 0 || kc as u32 == mp2 >> 16) {
            post_input(h, out, mp1, mp2, time);
        }
    }
}

fn ext_post(h: u32, msg: u32, mp1: u32, mp2: u32, time: u32) {
    if st().desk.exists(h) {
        post_input(h, msg, mp1, mp2, time);
        translate(h, msg, mp1, mp2, time);
    }
}

fn ext_down(h: u32, msg: u32, p: Point, time: u32) {
    let e = ext();
    let dbl = (msg - WM_BUTTON1DOWN) % 3 == 2;
    if dbl {
        // A double click ends the click (no WM_BUTTONnCLICK for its up).
        e.click = None;
        return;
    }
    let (cxi, cyi) = (st_sysval(SV_CXICON), st_sysval(SV_CYICON));
    e.click = Some((msg, h, time.wrapping_add(500), around(p, cxi, cyi)));
    if e.motion.is_none() {
        e.motion = Some((msg, h, around(p, 4, 4), false));
    }
}

fn ext_move(p: Point, mp2: u32, time: u32) {
    let e = ext();
    let target = if st().capture != 0 { st().capture } else { st().desk.window_at(p) };
    if target != e.over {
        if e.over != 0 && st().desk.exists(e.over) {
            post_input(e.over, WM_MOUSELEAVE, e.over, target, time);
        }
        post_input(target, WM_MOUSEENTER, target, e.over, time);
        e.over = target;
    }
    if let Some((msg, h, r, started)) = e.motion {
        if !started && !r.contains(p) {
            e.motion = Some((msg, h, r, true));
            if st().desk.exists(h) {
                // Where the motion started, in the window's coordinates.
                let o = st().desk.origin(h);
                let c = Point::new((r.left + r.right - 1) / 2, (r.bottom + r.top - 1) / 2);
                let n = (msg - WM_BUTTON1DOWN) / 3;
                ext_post(h, WM_BUTTON1MOTIONSTART + 3 * n, pack(c.x - o.x, c.y - o.y), mp2, time);
            }
        }
    }
}

fn ext_up(h: u32, msg: u32, p: Point, mp1: u32, mp2: u32, time: u32) {
    let e = ext();
    let n = (msg - WM_BUTTON1UP) / 3;
    if let Some((m, mh, _, started)) = e.motion {
        if msg == m + 1 {
            e.motion = None;
            if started {
                ext_post(mh, WM_BUTTON1MOTIONSTART + 1 + 3 * n, mp1, mp2, time);
            }
        }
    }
    if let Some((m, ch, deadline, r)) = e.click {
        if msg == m + 1 {
            e.click = None;
            if ch == h && (time.wrapping_sub(deadline) as i32) <= 0 && r.contains(p) {
                ext_post(h, WM_BUTTON1CLICK + 3 * n, mp1, mp2, time);
            }
        }
    }
}

const SV_CXICON: i32 = 38;
const SV_CYICON: i32 = 39;

fn st_sysval(i: i32) -> i32 {
    crate::misc::WinQuerySysValue(HWND_DESKTOP, i)
}

/// Button 1 pressed on a window whose thread does not respond: the
/// server activates its frame and moves, sizes or hides it itself.
/// Returns true if the click was used up here.
fn hung_click(target: u32, p: Point, dbl: bool) -> bool {
    let top = win::top_level(target);
    if top != 0 && top != HWND_DESKTOP {
        win::activate(top);
    }
    let s = st();
    let Some(w) = s.desk.get(target) else { return false };
    let parent_is_frame = s.frames.contains_key(&w.parent);
    let frame_h = if parent_is_frame { w.parent } else { target };
    match w.id {
        FID_TITLEBAR if parent_is_frame => {
            frame::start_drag_at(frame_h, 0, p);
            true
        }
        FID_MINMAX if parent_is_frame => {
            let o = s.desk.origin(target);
            match menu::item_id_at(target, Point::new(p.x - o.x, p.y - o.y)) {
                Some(frame::SC_MINIMIZE) => frame::minimize(frame_h),
                Some(frame::SC_MAXIMIZE) => frame::maximize(frame_h, true),
                Some(frame::SC_RESTORE) => frame::restore(frame_h),
                _ => win::show(frame_h, false),
            }
            true
        }
        _ if frame::is_minimized(target) => {
            if dbl {
                frame::restore(target);
            } else {
                frame::start_drag_at(target, 0, p);
            }
            true
        }
        FID_SYSMENU if parent_is_frame => true,
        _ if s.frames.contains_key(&target) => {
            let o = s.desk.origin(target);
            let e = frame::edges_at(target, Point::new(p.x - o.x, p.y - o.y));
            if e != 0 && frame::fcf(target) & frame::FCF_SIZEBORDER != 0 {
                frame::start_drag_at(target, e, p);
            }
            true
        }
        // A click in the client waits in the queue like any input.
        _ => false,
    }
}

/// Neither the window nor one of its ancestors is disabled.
fn input_enabled(h: u32) -> bool {
    let s = st();
    let mut cur = h;
    while let Some(w) = s.desk.get(cur) {
        if w.style & pmwin::ws::DISABLED != 0 {
            return false;
        }
        cur = w.parent;
    }
    true
}

/// A key: WM_CHAR to the window that has the focus now (or the active
/// window), whether or not its thread keeps up. Ctrl+Esc opens the
/// window list.
pub fn key(ev: &Input) {
    const KC_CHAR: u32 = 0x0001;
    const KC_VIRTUALKEY: u32 = 0x0002;
    const KC_SCANCODE: u32 = 0x0004;
    const KC_SHIFT: u32 = 0x0008;
    const KC_CTRL: u32 = 0x0010;
    const KC_ALT: u32 = 0x0020;
    const KC_KEYUP: u32 = 0x0040;
    let ch = ev.a & 0xff;
    let scan = (ev.a >> 8) & 0xff;
    let up = ev.a >> 24 & 1 != 0;
    let shift = ev.flags as u32;
    if scan == 0x01 && shift & 0x4 != 0 {
        if !up {
            crate::server::window_list();
        }
        return;
    }
    // Ctrl+letter arrives as a control character; PM reports the letter.
    let ch = if shift & 0x4 != 0 && (1..=26).contains(&ch) {
        ch + if shift & 0x3 != 0 { b'A' as u32 - 1 } else { b'a' as u32 - 1 }
    } else {
        ch
    };
    let mut fs = KC_SCANCODE;
    if ch != 0 && ch != 0xe0 {
        fs |= KC_CHAR;
    }
    if shift & 0x3 != 0 {
        fs |= KC_SHIFT;
    }
    if shift & 0x4 != 0 {
        fs |= KC_CTRL;
    }
    if shift & 0x8 != 0 {
        fs |= KC_ALT;
    }
    if up {
        fs |= KC_KEYUP;
    }
    let vkey = virtual_key(scan, ch);
    if vkey != 0 {
        fs |= KC_VIRTUALKEY;
    }
    let s = st();
    note_shift(s, shift);
    if vkey != 0 {
        set_key(s, vkey as usize, !up);
    }
    let target = if s.focus != 0 { s.focus } else { s.active };
    if target != 0 && input_enabled(target) && crate::viowin::key(target, ev) {
        return;
    }
    if target != 0 && input_enabled(target) {
        post_input(target, WM_CHAR, fs | 1 << 16 | scan << 24, (if fs & KC_CHAR != 0 { ch } else { 0 }) | vkey << 16, ev.time);
    }
}

/// A virtual key went down or up: the toggle bit flips on each press.
fn set_key(s: &mut crate::state::State, vk: usize, down: bool) {
    let k = &mut s.keys[vk];
    if down && *k & 0x80 == 0 {
        *k ^= 0x01;
    }
    *k = (*k & 0x01) | if down { 0x80 } else { 0 };
}

/// The shift keys' states from a KBD shift state (shift, Ctrl, Alt down;
/// Scroll, Num and Caps Lock on).
fn note_shift(s: &mut crate::state::State, shift: u32) {
    set_key(s, 0x09, shift & 0x3 != 0); // VK_SHIFT
    set_key(s, 0x0a, shift & 0x4 != 0); // VK_CTRL
    set_key(s, 0x0b, shift & 0x8 != 0); // VK_ALT
    for (vk, bit) in [(0x1e, 0x10), (0x1d, 0x20), (0x0e, 0x40)] {
        // VK_SCRLLOCK, VK_NUMLOCK, VK_CAPSLOCK.
        let k = &mut s.keys[vk];
        *k = (*k & 0x80) | (shift & bit != 0) as u8;
    }
}

/// VK_* for the keys that have one (set-1 scan codes).
fn virtual_key(scan: u32, ch: u32) -> u32 {
    match (scan, ch) {
        (0x0e, _) => 0x05, // VK_BACKSPACE
        (0x0f, _) if ch == 0 => 0x07, // VK_BACKTAB
        (0x0f, _) => 0x06, // VK_TAB
        (0x1c, _) => 0x08, // VK_NEWLINE
        (0x01, _) => 0x0f, // VK_ESC
        (0x39, _) => 0x10, // VK_SPACE
        (0x49, 0 | 0xe0) => 0x11, // VK_PAGEUP
        (0x51, 0 | 0xe0) => 0x12, // VK_PAGEDOWN
        (0x4f, 0 | 0xe0) => 0x13, // VK_END
        (0x47, 0 | 0xe0) => 0x14, // VK_HOME
        (0x4b, 0 | 0xe0) => 0x15, // VK_LEFT
        (0x48, 0 | 0xe0) => 0x16, // VK_UP
        (0x4d, 0 | 0xe0) => 0x17, // VK_RIGHT
        (0x50, 0 | 0xe0) => 0x18, // VK_DOWN
        (0x52, 0 | 0xe0) => 0x1a, // VK_INSERT
        (0x53, 0 | 0xe0) => 0x1b, // VK_DELETE
        (0x3b..=0x44, _) => 0x20 + scan - 0x3b, // VK_F1..VK_F10
        (0x57, _) => 0x2a, // VK_F11
        (0x58, _) => 0x2b, // VK_F12
        _ => 0,
    }
}

/// Handle one raw event (with the PM lock held).
pub fn event(ev: &Input) {
    match ev.kind {
        EV_MOUSE => mouse(Point::new(ev.a as i32, ev.b as i32), ev.flags as u8, (ev.flags >> 8) as u8, ev.time),
        EV_KEY => key(ev),
        EV_BACKGROUND => crate::state::to_shadow(),
        EV_REPAINT => {
            crate::state::to_screen();
            let s = st();
            let all = s.desk.get(HWND_DESKTOP).map_or(gpi::Rect::default(), |w| w.rect());
            s.ptr_saved = None;
            s.desk.invalidate_screen(all);
            for w in s.desk.all_mut() {
                w.ghost = pmwin::Region::new();
            }
            pointer::draw(s);
        }
        EV_PROCEXIT => crate::server::process_gone(ev.a),
        EV_MODE => crate::display::mode_changed(),
        EV_HOSTSIZE => crate::display::host_size(),
        _ => {}
    }
}

/// WinGetKeyState(hwndDesktop, vkey): 0x8000 (sign-extended, as IBM's
/// PMWIN returns it) while the virtual key is down, | 1 when it is
/// toggled. rs2 keeps one state, the input thread's, where PM has a
/// synchronous one per message.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetKeyState(_desk: u32, vk: i32) -> i32 {
    let Some(&k) = st().keys.get(vk as usize) else { return 0 };
    (if k & 0x80 != 0 { 0xffff_8000u32 as i32 } else { 0 }) | (k & 1) as i32
}

/// WinSetKeyboardStateTable(hwndDesktop, pKeyStateTable, fSet): the 256
/// virtual key states (0x80 down, 0x01 toggled) set from the table or
/// copied into it.
#[unsafe(no_mangle)]
pub extern "C" fn WinSetKeyboardStateTable(_desk: u32, table: *mut u8, set: u32) -> u32 {
    if table.is_null() {
        return 0;
    }
    let keys = &mut st().keys;
    unsafe {
        if set != 0 {
            core::ptr::copy_nonoverlapping(table, keys.as_mut_ptr(), 256);
        } else {
            core::ptr::copy_nonoverlapping(keys.as_ptr(), table, 256);
        }
    }
    1
}

/// SV_DBLCLKTIME in milliseconds.
fn dbl_time() -> u32 {
    crate::misc::WinQuerySysValue(1, 1) as u32
}
