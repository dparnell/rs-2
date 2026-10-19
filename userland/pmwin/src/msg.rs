//! Message queues, sending and posting, the message loop and timers.
//!
//! Every thread with a message queue has a shared event semaphore; posting
//! to a queue (or invalidating one of its windows) wakes it when the PM
//! lock is released. A message sent to a window of another thread goes
//! into that thread's queue of sent messages and the sender waits for the
//! reply, answering messages sent to itself meanwhile. Window procedures
//! of applications run without the PM lock. The PM server never waits for
//! an application: what it sends is processed later, like a posted
//! message, and its result is dropped.

#![allow(non_snake_case)]

use core::sync::atomic::{AtomicU32, Ordering};

use pmwin::HWND_DESKTOP;

use crate::ctl;
use crate::gpiapi::PointL;
use crate::state::{self, now, pid, st, QMsg, Queue, Sent, WndProc};
use crate::win::*;
use crate::{accel, lock};

unsafe extern "C" {
    #[link_name = "DOSCALLS.324"]
    fn DosCreateEventSem(name: *const u8, phev: *mut u32, attr: u32, posted: u32) -> u32;
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> u32;
    #[link_name = "DOSCALLS.326"]
    fn DosCloseEventSem(hev: u32) -> u32;
    #[link_name = "DOSCALLS.327"]
    fn DosResetEventSem(hev: u32, pul: *mut u32) -> u32;
    #[link_name = "DOSCALLS.328"]
    fn DosPostEventSem(hev: u32) -> u32;
    #[link_name = "DOSCALLS.329"]
    fn DosWaitEventSem(hev: u32, ms: u32) -> u32;
}

const DC_SEM_SHARED: u32 = 1;

// ---- Waking queues ----

/// Shared event semaphores of other processes this process has opened.
static OPENED: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];

/// Make sure this process may post `sem` (created by process `owner`).
fn open_sem(sem: u32, owner: u32) -> bool {
    if owner == pid() || OPENED.iter().any(|s| s.load(Ordering::Relaxed) == sem) {
        return true;
    }
    let mut h = sem;
    if unsafe { DosOpenEventSem(core::ptr::null(), &mut h) } != 0 {
        return false;
    }
    match OPENED.iter().find(|s| s.load(Ordering::Relaxed) == 0) {
        Some(slot) => slot.store(sem, Ordering::Relaxed),
        // Table full: forget one (it stays open).
        None => OPENED[sem as usize % OPENED.len()].store(sem, Ordering::Relaxed),
    }
    true
}

/// Post a queue's semaphore.
pub fn wake_queue(q: &Queue) {
    if open_sem(q.sem, q.pid) {
        unsafe { DosPostEventSem(q.sem) };
    }
}

/// Wake the threads of the queues that got messages or something to
/// paint (called as the PM lock is released).
pub fn wake_pending() {
    let Some(s) = state::get() else { return };
    if s.wake.is_empty() && !s.desk.wake_pending() {
        return;
    }
    let mut set = core::mem::take(&mut s.wake);
    let t = now();
    for q in s.desk.take_wake() {
        if let Some(x) = s.queues.get_mut(&q) {
            x.msgs.arrived(t);
        }
        set.insert(q);
    }
    let mut server = false;
    for q in set {
        let Some(x) = s.queues.get(&q) else { continue };
        server |= x.hung;
        wake_queue(x);
    }
    // The server paints for queues that do not respond.
    if server {
        let p = s.pointer;
        if let Some(x) = s.queues.get_mut(&s.server_queue) {
            if !x.msgs.iter().any(|m| m.msg == crate::server::WM_RS2_GHOSTS) {
                let m = QMsg { hwnd: HWND_DESKTOP, msg: crate::server::WM_RS2_GHOSTS, mp1: 0, mp2: 0, time: t, x: p.x, y: p.y, reserved: 0 };
                x.msgs.post(m);
            }
            wake_queue(x);
        }
    }
}

/// Queue a message for queue `q` (WM_MOUSEMOVE coalesced).
pub fn post_queue(q: u32, m: QMsg) -> bool {
    let s = st();
    let Some(queue) = s.queues.get_mut(&q) else { return false };
    if m.msg == WM_MOUSEMOVE {
        queue.msgs.post_mouse_move(m);
    } else {
        queue.msgs.post(m);
    }
    s.wake.insert(q);
    true
}

pub fn qmsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> QMsg {
    let p = st().pointer;
    QMsg { hwnd: h, msg, mp1, mp2, time: now(), x: p.x, y: p.y, reserved: 0 }
}

/// Queue a message for the window's thread (hwnd 0: the caller's queue).
pub fn post(h: u32, msg: u32, mp1: u32, mp2: u32) -> bool {
    let s = st();
    let q = if h == 0 { s.my_queue() } else { s.queue_of(h) };
    post_queue(q, qmsg(h, msg, mp1, mp2))
}

// ---- Sending ----

/// Window procedures in PMWIN itself (built-in classes, WinDefWindowProc
/// and friends passed as procedures) may run with the lock held.
fn pmwin_code(p: u32) -> bool {
    unsafe extern "C" {
        static __text_start: u8;
        static __text_end: u8;
    }
    let (a, b) = (&raw const __text_start as u32, &raw const __text_end as u32);
    (a..b).contains(&p)
}

/// Call a window procedure on the calling thread (application code runs
/// without the lock).
pub fn call(p: u32, h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let f: WndProc = unsafe { core::mem::transmute(p as usize) };
    if pmwin_code(p) {
        f(h, msg, mp1, mp2)
    } else {
        lock::unlocked(|| f(h, msg, mp1, mp2))
    }
}

/// Send a message: a window of the calling thread gets it at once, one of
/// another thread through its queue while the caller waits for the reply.
pub fn send(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let s = st();
    let Some((p, q)) = s.desk.get(h).map(|w| (w.proc_, w.queue)).filter(|&(p, _)| p != 0) else { return 0 };
    let other = s.queues.get(&q).is_some_and(|x| x.pid != pid() || x.tid != state::tid());
    if !other {
        return call(p, h, msg, mp1, mp2);
    }
    let mine = s.my_queue();
    let id = s.next_sent;
    s.next_sent = s.next_sent.wrapping_add(1).max(1);
    // The PM server never waits for an application.
    let reply = !s.is_server();
    if let Some(queue) = s.queues.get_mut(&q) {
        queue.sent.push_back(Sent { id, hwnd: h, msg, mp1, mp2, from: mine, reply });
        queue.msgs.arrived(now());
    }
    s.wake.insert(q);
    if !reply {
        return 0;
    }
    wait_reply(q, mine, id)
}

/// Notify a window: sent to windows of the calling thread, posted to
/// those of other threads (activation and focus changes never wait for
/// another application).
pub fn notify(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let s = st();
    let q = s.queue_of(h);
    if q != 0 && q != s.my_queue() {
        post(h, msg, mp1, mp2);
        return 0;
    }
    send(h, msg, mp1, mp2)
}

/// Wait for the reply to message `id` sent to queue `to`, answering
/// messages sent to the caller meanwhile.
fn wait_reply(to: u32, mine: u32, id: u32) -> u32 {
    loop {
        answer_sent();
        let s = st();
        if let Some(i) = s.replies.iter().position(|r| r.0 == id) {
            return s.replies.swap_remove(i).1;
        }
        // The receiver is gone (its process ended).
        if !s.queues.contains_key(&to) {
            return 0;
        }
        match s.queues.get(&mine) {
            Some(x) if x.sent.is_empty() => {
                let sem = x.sem;
                let mut n = 0;
                unsafe { DosResetEventSem(sem, &mut n) };
                lock::unlocked(|| unsafe { DosWaitEventSem(sem, 1000) });
            }
            Some(_) => {}
            None => lock::sleep(1),
        }
    }
}

/// Process the messages other threads sent to the caller's queue.
pub fn answer_sent() {
    loop {
        let s = st();
        let q = s.my_queue();
        let Some(m) = s.queues.get_mut(&q).and_then(|x| x.sent.pop_front()) else { return };
        let r = if st().desk.exists(m.hwnd) { send(m.hwnd, m.msg, m.mp1, m.mp2) } else { 0 };
        if m.reply {
            let s = st();
            s.replies.push((m.id, r));
            if m.from != 0 {
                s.wake.insert(m.from);
            }
        }
    }
}

/// A queue went away: whoever waits for a reply from it gets 0.
pub fn abandon_sent(queue: &mut Queue) {
    let s = st();
    for m in queue.sent.drain(..) {
        if m.reply {
            s.replies.push((m.id, 0));
            if m.from != 0 {
                s.wake.insert(m.from);
            }
        }
    }
}

// ---- Queues ----

#[unsafe(no_mangle)]
pub extern "C" fn WinCreateMsgQueue(_hab: u32, _cmsg: i32) -> u32 {
    let Some(s) = state::get() else { return 0 };
    if s.my_queue() != 0 {
        return 0;
    }
    let mut sem = 0;
    if unsafe { DosCreateEventSem(core::ptr::null(), &mut sem, DC_SEM_SHARED, 0) } != 0 {
        return 0;
    }
    let s = st();
    let h = s.next_hmq;
    s.next_hmq += 1;
    let q = Queue {
        pid: pid(),
        tid: state::tid(),
        sem,
        msgs: Default::default(),
        sent: Default::default(),
        menu_mode: None,
        accel: 0,
        msg_pos: s.pointer,
        msg_time: now(),
        hung: false,
    };
    s.queues.insert(h, q);
    // A PM program: the PM server runs while there are any.
    let me = pid();
    if !s.procs.contains(&me) {
        s.procs.push(me);
    }
    crate::server::ensure_running();
    h
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyMsgQueue(hmq: u32) -> u32 {
    match st().queues.get(&hmq) {
        Some(q) if q.pid == pid() => {}
        _ => return 0,
    }
    // Its windows go first.
    for h in crate::win::top_windows_of(hmq) {
        crate::win::destroy(h);
    }
    let Some(mut q) = st().queues.remove(&hmq) else { return 0 };
    abandon_sent(&mut q);
    st().timers.retain(|t| t.queue != hmq);
    unsafe { DosCloseEventSem(q.sem) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinPostMsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    post(h, msg, mp1, mp2) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinPostQueueMsg(hmq: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    post_queue(hmq, qmsg(0, msg, mp1, mp2)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSendMsg(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    send(h, msg, mp1, mp2)
}

/// WinBroadcastMsg(hwnd, msg, mp1, mp2, rgf): BMSG_SEND (1),
/// BMSG_POSTQUEUE (2: every queue), BMSG_DESCENDANTS (4), BMSG_FRAMEONLY
/// (8).
#[unsafe(no_mangle)]
pub extern "C" fn WinBroadcastMsg(parent: u32, msg: u32, mp1: u32, mp2: u32, fl: u32) -> u32 {
    if fl & 2 != 0 {
        let qs: alloc::vec::Vec<u32> = st().queues.keys().copied().collect();
        for q in qs {
            post_queue(q, qmsg(0, msg, mp1, mp2));
        }
        return 1;
    }
    let mut all = alloc::vec::Vec::new();
    let mut stack = st().desk.get(parent).map_or(alloc::vec::Vec::new(), |w| w.children.clone());
    while let Some(k) = stack.pop() {
        all.push(k);
        if fl & 4 != 0 {
            stack.extend(st().desk.get(k).map_or(alloc::vec::Vec::new(), |w| w.children.clone()));
        }
    }
    for k in all {
        if fl & 8 != 0 && !st().frames.contains_key(&k) {
            continue;
        }
        if fl & 1 != 0 {
            send(k, msg, mp1, mp2);
        } else {
            post(k, msg, mp1, mp2);
        }
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
            w.ghost = pmwin::Region::new();
        }
    }
    r
}

/// Default processing.
#[unsafe(no_mangle)]
pub extern "C" fn WinDefWindowProc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CLOSE => {
            let q = st().queue_of(h);
            post_queue(q, qmsg(0, WM_QUIT, 0, 0));
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

// ---- Timers ----

#[unsafe(no_mangle)]
pub extern "C" fn WinStartTimer(_hab: u32, h: u32, id: u32, ms: u32) -> u32 {
    let s = st();
    let queue = if h == 0 { s.my_queue() } else { s.queue_of(h) };
    if queue == 0 {
        return 0;
    }
    s.timers.retain(|t| !(t.hwnd == h && t.id == id && t.queue == queue));
    let id = if id == 0 { 0x7fff - s.timers.len() as u32 } else { id };
    s.timers.push(state::Timer { hwnd: h, queue, id, interval: ms.max(1), next: now().wrapping_add(ms.max(1)) });
    // The thread recomputes how long it may sleep.
    s.wake.insert(queue);
    id
}

#[unsafe(no_mangle)]
pub extern "C" fn WinStopTimer(_hab: u32, h: u32, id: u32) -> u32 {
    let s = st();
    let n = s.timers.len();
    s.timers.retain(|t| !(t.hwnd == h && t.id == id));
    (s.timers.len() != n) as u32
}

/// Post WM_TIMER for the queue's timers that are due; returns ms until
/// the next one (u32::MAX: none).
fn run_timers(q: u32) -> u32 {
    let s = st();
    let t = now();
    let mut next = u32::MAX;
    let mut due = alloc::vec::Vec::new();
    for tm in s.timers.iter_mut().filter(|tm| tm.queue == q) {
        let left = tm.next.wrapping_sub(t) as i32;
        if left <= 0 {
            due.push((tm.hwnd, tm.id));
            tm.next = t.wrapping_add(tm.interval);
            next = next.min(tm.interval);
        } else {
            next = next.min(left as u32);
        }
    }
    let p = s.pointer;
    for (h, id) in due {
        let Some(x) = st().queues.get_mut(&q) else { break };
        // One WM_TIMER per timer in the queue at a time.
        if x.msgs.iter().any(|m| m.msg == WM_TIMER && m.hwnd == h && m.mp1 == id) {
            continue;
        }
        // Timers are generated as the thread reads its queue: they do not
        // count as input it fails to take.
        let waited = x.msgs.waited(t);
        x.msgs.post(QMsg { hwnd: h, msg: WM_TIMER, mp1: id, mp2: 0, time: t, x: p.x, y: p.y, reserved: 0 });
        if waited == 0 {
            x.msgs.retrieved(t, false);
        }
    }
    next
}

// ---- The message loop ----

/// Next message for the calling thread; `remove` takes it off the queue.
fn next_message(filter: u32, first: u32, last: u32, remove: bool, wait: bool) -> Option<QMsg> {
    loop {
        answer_sent();
        let q = st().my_queue();
        if q == 0 {
            return None;
        }
        let wait_ms = run_timers(q);
        let s = st();
        let t = now();
        let queue = s.queues.get_mut(&q)?;
        if !queue.sent.is_empty() {
            continue;
        }
        if let Some(mut m) = queue.msgs.take(filter, first, last, remove) {
            if remove {
                queue.msg_pos = gpi::Point::new(m.x, m.y);
                queue.msg_time = m.time;
                let more = !queue.msgs.is_empty();
                queue.msgs.retrieved(t, more);
                if m.msg == WM_CHAR {
                    accel::translate(&mut m);
                }
            }
            return Some(m);
        }
        let paint_ok = (first == 0 && last == 0) || (first..=last).contains(&WM_PAINT);
        if let Some(h) = s.desk.paint_pending(q).filter(|&h| paint_ok && (filter == 0 || filter == h)) {
            if let Some(x) = s.queues.get_mut(&q) {
                x.msgs.retrieved(t, false);
            }
            let p = s.pointer;
            return Some(QMsg { hwnd: h, msg: WM_PAINT, mp1: 0, mp2: 0, time: t, x: p.x, y: p.y, reserved: 0 });
        }
        let queue = s.queues.get_mut(&q)?;
        queue.msgs.retrieved(t, false);
        if !wait {
            return None;
        }
        let sem = queue.sem;
        let mut n = 0;
        unsafe { DosResetEventSem(sem, &mut n) };
        queue.msgs.set_waiting(true);
        lock::unlocked(|| unsafe { DosWaitEventSem(sem, wait_ms) });
        if let Some(x) = st().queues.get_mut(&q) {
            x.msgs.set_waiting(false);
        }
    }
}

/// WinGetMsg(hab, pqmsg, hwndFilter, msgFirst, msgLast): FALSE for WM_QUIT.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetMsg(_hab: u32, pq: *mut QMsg, filter: u32, first: u32, last: u32) -> u32 {
    let m = next_message(filter, first, last, true, true).unwrap_or(QMsg { msg: WM_QUIT, ..Default::default() });
    if !pq.is_null() {
        unsafe { pq.write_unaligned(m) };
    }
    (m.msg != WM_QUIT) as u32
}

/// WinPeekMsg(hab, pqmsg, hwndFilter, msgFirst, msgLast, fl): PM_REMOVE = 1.
#[unsafe(no_mangle)]
pub extern "C" fn WinPeekMsg(_hab: u32, pq: *mut QMsg, filter: u32, first: u32, last: u32, fl: u32) -> u32 {
    match next_message(filter, first, last, fl & 1 != 0, false) {
        Some(m) => {
            if !pq.is_null() {
                unsafe { pq.write_unaligned(m) };
            }
            1
        }
        None => 0,
    }
}

/// WinQueryQueueStatus(hwndDesktop): QS_* bits of what waits (the high
/// word, what was added since the last call, is the same here).
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryQueueStatus(_desk: u32) -> u32 {
    const QS_KEY: u32 = 0x0001;
    const QS_MOUSE: u32 = 0x0006;
    const QS_TIMER: u32 = 0x0008;
    const QS_PAINT: u32 = 0x0010;
    const QS_POSTMSG: u32 = 0x0020;
    const QS_SENDMSG: u32 = 0x0040;
    let s = st();
    let q = s.my_queue();
    let Some(queue) = s.queues.get(&q) else { return 0 };
    let mut fs = 0;
    for m in queue.msgs.iter() {
        fs |= match m.msg {
            WM_CHAR => QS_KEY,
            0x70..=0x7f => QS_MOUSE,
            WM_TIMER => QS_TIMER,
            _ => QS_POSTMSG,
        };
    }
    if !queue.sent.is_empty() {
        fs |= QS_SENDMSG;
    }
    if s.desk.paint_pending(q).is_some() {
        fs |= QS_PAINT;
    }
    fs | fs << 16
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

/// Pointer position when the current message was posted.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryMsgPos(_hab: u32, p: *mut PointL) -> u32 {
    let s = st();
    let pos = s.queues.get(&s.my_queue()).map_or(s.pointer, |q| q.msg_pos);
    if p.is_null() {
        return 0;
    }
    unsafe { p.write_unaligned(PointL { x: pos.x, y: pos.y }) };
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryMsgTime(_hab: u32) -> u32 {
    let s = st();
    s.queues.get(&s.my_queue()).map_or(now(), |q| q.msg_time)
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
