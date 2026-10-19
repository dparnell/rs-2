//! Message queues: the posted messages of one thread, mouse-move
//! coalescing, and the bookkeeping that tells whether the thread still
//! reads its queue ("not responding" detection).

use alloc::collections::VecDeque;

pub const WM_MOUSEMOVE: u32 = 0x0070;

/// QMSG as applications see it.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct QMsg {
    pub hwnd: u32,
    pub msg: u32,
    pub mp1: u32,
    pub mp2: u32,
    pub time: u32,
    pub x: i32,
    pub y: i32,
    pub reserved: u32,
}

/// The posted messages of a queue and its health.
#[derive(Default)]
pub struct MsgQueue {
    msgs: VecDeque<QMsg>,
    /// Time (ms) since when the queue has had something for its thread
    /// that the thread has not come to get; None while the thread keeps
    /// up (or has nothing waiting).
    pending_since: Option<u32>,
    /// The thread is blocked waiting for messages (it is responsive by
    /// definition).
    waiting: bool,
}

impl MsgQueue {
    pub fn new() -> MsgQueue {
        MsgQueue::default()
    }

    pub fn len(&self) -> usize {
        self.msgs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.msgs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &QMsg> {
        self.msgs.iter()
    }

    /// Something arrived for the thread at `now` (a message, a sent
    /// message or a paint request).
    pub fn arrived(&mut self, now: u32) {
        if self.pending_since.is_none() {
            self.pending_since = Some(now);
        }
    }

    pub fn post(&mut self, m: QMsg) {
        self.arrived(m.time);
        self.msgs.push_back(m);
    }

    /// Post a WM_MOUSEMOVE, replacing one that is still waiting for the
    /// same window at the end of the queue: a thread that falls behind
    /// gets the latest position, not every step.
    pub fn post_mouse_move(&mut self, m: QMsg) {
        if let Some(last) = self.msgs.back_mut() {
            if last.msg == WM_MOUSEMOVE && last.hwnd == m.hwnd {
                *last = m;
                return;
            }
        }
        self.post(m);
    }

    /// The first message for `filter` (0: any; messages for window 0
    /// always match) with a message number in `first..=last` (0, 0: any);
    /// removed when `remove`.
    pub fn take(&mut self, filter: u32, first: u32, last: u32, remove: bool) -> Option<QMsg> {
        let range = |m: u32| (first == 0 && last == 0) || (first <= last && (first..=last).contains(&m)) || (first > last && (m >= first || m <= last));
        let i = self.msgs.iter().position(|m| (filter == 0 || m.hwnd == filter || m.hwnd == 0) && range(m.msg))?;
        if remove { self.msgs.remove(i) } else { Some(self.msgs[i]) }
    }

    /// Remove the messages `keep` rejects (a window was destroyed).
    pub fn retain(&mut self, keep: impl FnMut(&QMsg) -> bool) {
        self.msgs.retain(keep);
    }

    /// The thread came to get messages at `now`; `more` says whether
    /// anything is still waiting for it after this retrieval.
    pub fn retrieved(&mut self, now: u32, more: bool) {
        self.pending_since = more.then_some(now);
    }

    /// The thread blocks waiting for messages (true) or runs again.
    pub fn set_waiting(&mut self, on: bool) {
        self.waiting = on;
        if on {
            self.pending_since = None;
        }
    }

    pub fn waiting(&self) -> bool {
        self.waiting
    }

    /// Not responding: something has been waiting for longer than `limit`
    /// ms without the thread looking at its queue.
    pub fn hung(&self, now: u32, limit: u32) -> bool {
        !self.waiting && self.pending_since.is_some_and(|t| now.wrapping_sub(t) as i32 > limit as i32)
    }

    /// How long input has been waiting (0: nothing waits).
    pub fn waited(&self, now: u32) -> u32 {
        self.pending_since.map_or(0, |t| now.wrapping_sub(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(hwnd: u32, msg: u32, time: u32) -> QMsg {
        QMsg { hwnd, msg, time, ..Default::default() }
    }

    #[test]
    fn mouse_moves_coalesce_at_the_tail() {
        let mut q = MsgQueue::new();
        q.post_mouse_move(QMsg { x: 1, ..m(5, WM_MOUSEMOVE, 0) });
        q.post_mouse_move(QMsg { x: 2, ..m(5, WM_MOUSEMOVE, 1) });
        assert_eq!(q.len(), 1);
        assert_eq!(q.take(0, 0, 0, false).unwrap().x, 2);
        // Another window, or a click in between, keeps the order.
        q.post_mouse_move(m(6, WM_MOUSEMOVE, 2));
        q.post(m(6, 0x71, 3));
        q.post_mouse_move(m(6, WM_MOUSEMOVE, 4));
        assert_eq!(q.len(), 4);
    }

    #[test]
    fn filters() {
        let mut q = MsgQueue::new();
        q.post(m(5, 0x24, 0));
        q.post(m(6, 0x7a, 0));
        q.post(m(0, 0x2a, 0));
        assert_eq!(q.take(6, 0, 0, true).unwrap().msg, 0x7a);
        assert_eq!(q.take(0, 0x2a, 0x2a, true).unwrap().msg, 0x2a);
        assert_eq!(q.take(0, 0x70, 0x7f, false), None);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn hung_after_limit_without_retrieval() {
        let mut q = MsgQueue::new();
        assert!(!q.hung(10_000, 2000));
        q.post(m(5, 0x7a, 1000));
        assert!(!q.hung(2500, 2000));
        assert!(q.hung(3001, 2000));
        // The thread comes back and empties the queue.
        q.retrieved(3100, false);
        assert!(!q.hung(9000, 2000));
        // A thread blocked in WinGetMsg is never hung.
        q.post(m(5, 0x7a, 9000));
        q.set_waiting(true);
        assert!(!q.hung(20_000, 2000));
        q.set_waiting(false);
        q.arrived(20_000);
        assert!(q.hung(22_001, 2000));
        // Retrieving while more is left restarts the clock.
        q.retrieved(22_100, true);
        assert!(!q.hung(23_000, 2000));
        assert!(q.hung(24_200, 2000));
    }

    #[test]
    fn time_wraps() {
        let mut q = MsgQueue::new();
        q.post(m(5, 0x7a, u32::MAX - 100));
        assert!(!q.hung(500, 2000));
        assert!(q.hung(2000, 2000));
    }
}
