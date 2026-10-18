//! Message queues, sending and posting, the message loop, timers and the
//! input from the kernel (keys, mouse, repaint requests).

#![allow(non_snake_case)]

use gpi::Point;
use pmwin::HWND_DESKTOP;

use crate::gpiapi::PointL;
use crate::ctl;
use crate::pointer;
use crate::state::{self, now, st, QMsg, Queue, WndProc};
use crate::{accel, frame, lock, menu};
use crate::win::{self, *};

/// INPUTEVENT from Rs2ReadInput.
#[repr(C)]
#[derive(Default)]
struct Input {
    kind: u16,
    flags: u16,
    a: u32,
    b: u32,
    time: u32,
}

unsafe extern "C" {
    #[link_name = "RS2API.2"]
    fn Rs2ReadInput(ev: *mut Input, ms: u32) -> u32;
}

const EV_KEY: u16 = 1;
const EV_MOUSE: u16 = 2;
const EV_REPAINT: u16 = 3;

/// Call a window procedure. A window of another thread gets the message
/// through its queue while the caller waits for the reply.
pub fn send(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let s = st();
    let Some((p, q)) = s.desk.get(h).map(|w| (w.proc_, w.queue)).filter(|&(p, _)| p != 0) else { return 0 };
    let me = state::tid();
    if let Some(queue) = s.queues.get_mut(&q).filter(|x| x.tid != me) {
        let id = s.next_sent;
        s.next_sent += 1;
        queue.sent.push_back(state::Sent { id, hwnd: h, msg, mp1, mp2 });
        return wait_reply(q, id);
    }
    let f: WndProc = unsafe { core::mem::transmute(p as usize) };
    f(h, msg, mp1, mp2)
}

/// Wait for the reply to a message sent to queue `q`, answering messages
/// sent to the caller meanwhile.
fn wait_reply(q: u32, id: u32) -> u32 {
    loop {
        let Some(queue) = st().queues.get_mut(&q) else { return 0 };
        if let Some(i) = queue.replies.iter().position(|r| r.0 == id) {
            return queue.replies.swap_remove(i).1;
        }
        if !queue.sent.iter().any(|m| m.id == id) && !st().queues.contains_key(&q) {
            return 0;
        }
        answer_sent();
        lock::sleep(1);
    }
}

/// Process the messages other threads sent to the caller's queue.
fn answer_sent() {
    loop {
        let s = st();
        let q = s.my_queue();
        let Some(m) = s.queues.get_mut(&q).and_then(|x| x.sent.pop_front()) else { return };
        let r = send(m.hwnd, m.msg, m.mp1, m.mp2);
        // The reply goes to the queue the message was sent to (the
        // sender polls there).
        if let Some(x) = st().queues.get_mut(&q) {
            x.replies.push((m.id, r));
        }
    }
}

/// Queue a message for the window's thread (hwnd 0: the caller's queue).
pub fn post(h: u32, msg: u32, mp1: u32, mp2: u32) -> bool {
    let s = st();
    let q = if h == 0 { s.my_queue() } else { s.desk.get(h).map_or(0, |w| w.queue) };
    let p = s.pointer;
    let Some(queue) = s.queues.get_mut(&q) else { return false };
    queue.msgs.push_back(QMsg { hwnd: h, msg, mp1, mp2, time: now(), x: p.x, y: p.y, reserved: 0 });
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn WinCreateMsgQueue(_hab: u32, _cmsg: i32) -> u32 {
    let s = st();
    let tid = state::tid();
    if s.queues.values().any(|q| q.tid == tid) {
        return 0;
    }
    let h = s.next_hmq;
    s.next_hmq += 1;
    s.queues.insert(h, Queue { msgs: Default::default(), tid, sent: Default::default(), replies: Default::default() });
    h
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyMsgQueue(hmq: u32) -> u32 {
    st().queues.remove(&hmq).is_some() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinPostMsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    post(h, msg, mp1, mp2) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinPostQueueMsg(hmq: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let s = st();
    let p = s.pointer;
    let Some(q) = s.queues.get_mut(&hmq) else { return 0 };
    q.msgs.push_back(QMsg { hwnd: 0, msg, mp1, mp2, time: now(), x: p.x, y: p.y, reserved: 0 });
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSendMsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    send(h, msg, mp1, mp2)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinBroadcastMsg(parent: u32, msg: u32, mp1: u32, mp2: u32, _fl: u32) -> u32 {
    let kids = st().desk.get(parent).map_or(alloc::vec::Vec::new(), |w| w.children.clone());
    for k in kids {
        send(k, msg, mp1, mp2);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDispatchMsg(_hab: u32, pq: *const QMsg) -> u32 {
    if pq.is_null() {
        return 0;
    }
    let m = unsafe { pq.read_unaligned() };
    if m.msg == WM_TIMER && m.hwnd == 0 {
        return 0;
    }
    let r = send(m.hwnd, m.msg, m.mp1, m.mp2);
    // A window that ignored WM_PAINT must not get it forever.
    if m.msg == WM_PAINT {
        if let Some(w) = st().desk.get_mut(m.hwnd) {
            w.update = pmwin::Region::new();
        }
    }
    r
}

/// Default processing.
#[unsafe(no_mangle)]
pub extern "C" fn WinDefWindowProc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CLOSE => {
            let q = st().desk.get(h).map_or(0, |w| w.queue);
            let p = st().pointer;
            if let Some(queue) = st().queues.get_mut(&q) {
                queue.msgs.push_back(QMsg { hwnd: 0, msg: WM_QUIT, mp1: 0, mp2: 0, time: now(), x: p.x, y: p.y, reserved: 0 });
            }
            0
        }
        WM_PAINT => {
            let hps = WinBeginPaint(h, 0, core::ptr::null_mut());
            WinEndPaint(hps);
            0
        }
        // Keys a window does not use go up to its parent (a frame
        // control's or a dialog item's frame).
        WM_CHAR => {
            let parent = ctl::parent(h);
            if parent != HWND_DESKTOP && parent != pmwin::HWND_OBJECT && parent != 0 {
                send(parent, msg, mp1, mp2)
            } else {
                0
            }
        }
        // The default for WM_MENUSELECT lets the command be posted.
        ctl::WM_MENUSELECT => 1,
        _ => 0,
    }
}

// ---- Input ----

fn pack(x: i32, y: i32) -> u32 {
    (x as u32 & 0xffff) | (y as u32) << 16
}

/// Read and route kernel input; waits up to `ms` for the first event.
fn pump(ms: u32) {
    let mut wait = ms;
    loop {
        let ev = match unsafe { (*core::ptr::addr_of_mut!(PENDING)).take() } {
            Some(ev) => ev,
            None => {
                let mut ev = Input::default();
                if unsafe { Rs2ReadInput(&mut ev, wait) } != 0 {
                    return;
                }
                ev
            }
        };
        wait = 0;
        match ev.kind {
            EV_MOUSE => mouse(Point::new(ev.a as i32, ev.b as i32), ev.flags as u8),
            EV_KEY => key(&ev),
            EV_REPAINT => {
                let s = st();
                let all = s.desk.get(HWND_DESKTOP).map_or(gpi::Rect::default(), |w| w.rect());
                s.ptr_saved = None;
                s.desk.invalidate_screen(all);
                pointer::draw(s);
            }
            _ => {}
        }
    }
}

fn mouse(p: Point, buttons: u8) {
    let s = st();
    pointer::move_to(s, p);
    let prev = core::mem::replace(&mut s.buttons, buttons);
    // Moving or sizing a frame.
    if s.drag.is_some() {
        frame::track(p, buttons);
        return;
    }
    let target = if s.capture != 0 { s.capture } else { s.desk.window_at(p) };
    if target == 0 {
        return;
    }
    let down = buttons & !prev;
    let up = prev & !buttons;
    // Disabled windows (the owner of a modal dialog) get no input; a
    // click brings up the window that disabled it.
    if s.capture == 0 && !input_enabled(target) {
        if down != 0 {
            let top = win::top_level(target);
            if let Some(&d) = st().desk.get(HWND_DESKTOP).and_then(|w| w.children.iter().rev().find(|&&c| st().desk.get(c).is_some_and(|x| x.owner == top && x.visible_style()))) {
                win::activate(d);
            }
        }
        return;
    }
    let o = st().desk.origin(target);
    let mp1 = pack(p.x - o.x, p.y - o.y);
    if down & 1 != 0 && target != HWND_DESKTOP && st().capture == 0 && !menu::is_menu(target) {
        win::activate(target);
    }
    if down == 0 && up == 0 {
        post(target, WM_MOUSEMOVE, mp1, 0);
    }
    let t = now();
    // Button 1 left, 2 right, 3 middle: WM_BUTTONnDOWN = 0x71 + 3(n-1),
    // a second press nearby within half a second is WM_BUTTONnDBLCLK.
    for (bit, n) in [(1u8, 0u32), (2, 1), (4, 2)] {
        if down & bit != 0 {
            let (lt, lp, lb) = st().last_click;
            let dbl = lb == bit && t.wrapping_sub(lt) < 500 && (lp.x - p.x).abs() < 4 && (lp.y - p.y).abs() < 4;
            st().last_click = if dbl { (0, Point::new(-100, -100), 0) } else { (t, p, bit) };
            post(target, if dbl { WM_BUTTON1DOWN + 2 } else { WM_BUTTON1DOWN } + 3 * n, mp1, 0);
        }
        if up & bit != 0 {
            post(target, WM_BUTTON1UP + 3 * n, mp1, 0);
        }
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

/// WM_CHAR: mp1 = fs | repeat << 16 | scan << 24, mp2 = char | vkey << 16.
fn key(ev: &Input) {
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
    let target = if s.focus != 0 { s.focus } else { s.active };
    if target != 0 && input_enabled(target) {
        post(target, WM_CHAR, fs | 1 << 16 | scan << 24, (if fs & KC_CHAR != 0 { ch } else { 0 }) | vkey << 16);
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

// ---- Timers ----

#[unsafe(no_mangle)]
pub extern "C" fn WinStartTimer(_hab: u32, h: u32, id: u32, ms: u32) -> u32 {
    let s = st();
    s.timers.retain(|t| !(t.hwnd == h && t.id == id));
    let id = if id == 0 { 0x7fff - s.timers.len() as u32 } else { id };
    s.timers.push(state::Timer { hwnd: h, id, interval: ms.max(1), next: now().wrapping_add(ms.max(1)) });
    id
}

#[unsafe(no_mangle)]
pub extern "C" fn WinStopTimer(_hab: u32, h: u32, id: u32) -> u32 {
    let s = st();
    let n = s.timers.len();
    s.timers.retain(|t| !(t.hwnd == h && t.id == id));
    (s.timers.len() != n) as u32
}

/// Post WM_TIMER for timers that are due; returns ms until the next one.
fn run_timers() -> u32 {
    let s = st();
    let t = now();
    let mut next = 100u32;
    let mut due = alloc::vec::Vec::new();
    for tm in s.timers.iter_mut() {
        let left = tm.next.wrapping_sub(t) as i32;
        if left <= 0 {
            due.push((tm.hwnd, tm.id));
            tm.next = t.wrapping_add(tm.interval);
            next = next.min(tm.interval);
        } else {
            next = next.min(left as u32);
        }
    }
    for (h, id) in due {
        // One WM_TIMER per timer in the queue at a time.
        let q = st().my_queue();
        let pending = st().queues.get(&q).is_some_and(|x| x.msgs.iter().any(|m| m.msg == WM_TIMER && m.hwnd == h && m.mp1 == id));
        if !pending {
            post(h, WM_TIMER, id, 0);
        }
    }
    next
}

// ---- The message loop ----

/// Next message for the calling thread; `remove` takes it off the queue.
fn next_message(filter: u32, remove: bool, wait: bool) -> Option<QMsg> {
    loop {
        answer_sent();
        let input = st().input_queue() == st().my_queue();
        if input {
            pump(0);
        }
        let wait_ms = run_timers();
        if input {
            win::paint_desktop();
        }
        let s = st();
        let q = s.my_queue();
        let queue = s.queues.get_mut(&q)?;
        let at = queue.msgs.iter().position(|m| filter == 0 || m.hwnd == filter || m.hwnd == 0);
        if let Some(i) = at {
            let mut m = if remove { queue.msgs.remove(i)? } else { queue.msgs[i] };
            if remove && m.msg == WM_CHAR {
                accel::translate(&mut m);
            }
            return Some(m);
        }
        if let Some(h) = s.desk.paint_pending(q) {
            let p = s.pointer;
            return Some(QMsg { hwnd: h, msg: WM_PAINT, mp1: 0, mp2: 0, time: now(), x: p.x, y: p.y, reserved: 0 });
        }
        if !wait {
            return None;
        }
        if input {
            lock::unlocked(|| wait_input(wait_ms.min(50)));
        } else {
            lock::sleep(wait_ms.clamp(1, 20));
        }
    }
}

/// Wait for kernel input; the event is kept for the next pump.
fn wait_input(ms: u32) {
    let mut ev = Input::default();
    if unsafe { Rs2ReadInput(&mut ev, ms) } == 0 {
        unsafe { PENDING = Some(ev) };
    }
}

static mut PENDING: Option<Input> = None;

/// WinGetMsg(hab, pqmsg, hwndFilter, msgFirst, msgLast): FALSE for WM_QUIT.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetMsg(_hab: u32, pq: *mut QMsg, filter: u32, _first: u32, _last: u32) -> u32 {
    let m = next_message(filter, true, true).unwrap_or(QMsg { msg: WM_QUIT, ..Default::default() });
    if !pq.is_null() {
        unsafe { pq.write_unaligned(m) };
    }
    (m.msg != WM_QUIT) as u32
}

/// WinPeekMsg(hab, pqmsg, hwndFilter, msgFirst, msgLast, fl): PM_REMOVE = 1.
#[unsafe(no_mangle)]
pub extern "C" fn WinPeekMsg(_hab: u32, pq: *mut QMsg, filter: u32, _first: u32, _last: u32, fl: u32) -> u32 {
    match next_message(filter, fl & 1 != 0, false) {
        Some(m) => {
            if !pq.is_null() {
                unsafe { pq.write_unaligned(m) };
            }
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryPointerPos(_desk: u32, p: *mut PointL) -> u32 {
    if p.is_null() {
        return 0;
    }
    let ptr = st().pointer;
    unsafe { p.write_unaligned(PointL { x: ptr.x, y: ptr.y }) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryMsgPos(_hab: u32, p: *mut PointL) -> u32 {
    WinQueryPointerPos(0, p)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryMsgTime(_hab: u32) -> u32 {
    now()
}

#[unsafe(no_mangle)]
pub extern "C" fn WinGetCurrentTime(_hab: u32) -> u32 {
    now()
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetCapture(_desk: u32, h: u32) -> u32 {
    st().capture = h;
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryCapture(_desk: u32) -> u32 {
    st().capture
}
