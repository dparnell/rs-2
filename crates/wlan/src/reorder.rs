//! Receive reordering for 802.11n block-ack sessions (IEEE 802.11-2020
//! 10.25.6, Linux `mwifiex/11n_rxreorder.c`).
//!
//! Once a block-ack session runs for a traffic class (TID), the access
//! point sends frames in aggregates and retries only what was lost, so
//! they can arrive out of order or twice. Full-MAC firmware such as
//! Marvell's leaves putting them back in order to the host: each frame
//! carries its sequence number (12 bits), and frames are handed on only
//! in sequence order, within a window of `size` numbers that starts at
//! the oldest one still awaited.
//!
//! A gap that stays open (a frame the AP gave up on) is skipped after
//! [`FLUSH_MS`]; a Block Ack Request moves the window on at once.

use alloc::vec::Vec;

/// Traffic identifiers (user priorities 0-7).
pub const TIDS: usize = 8;
/// How long a gap in the window may hold frames back (Linux
/// `MIN_FLUSH_TIMER_MS`).
pub const FLUSH_MS: u64 = 50;

const SEQ_MOD: u16 = 4096;
const HALF: u16 = 2048;

fn diff(a: u16, b: u16) -> u16 {
    a.wrapping_sub(b) & (SEQ_MOD - 1)
}

struct Window<T> {
    /// The oldest sequence number still awaited.
    start: u16,
    /// Slot of `start` in `buf`.
    head: usize,
    buf: Vec<Option<T>>,
    /// Since when frames are held back behind a gap.
    held_since: Option<u64>,
}

impl<T> Window<T> {
    fn size(&self) -> u16 {
        self.buf.len() as u16
    }

    /// Hand on what is stored (in order) and move the window to `to`.
    fn flush_to(&mut self, to: u16, out: &mut Vec<T>) {
        let n = diff(to, self.start).min(self.size());
        for _ in 0..n {
            if let Some(f) = self.buf[self.head].take() {
                out.push(f);
            }
            self.head = (self.head + 1) % self.buf.len();
        }
        self.start = to;
    }

    /// Hand on the frames in order from the window's start.
    fn release(&mut self, out: &mut Vec<T>) {
        while let Some(f) = self.buf[self.head].take() {
            out.push(f);
            self.head = (self.head + 1) % self.buf.len();
            self.start = (self.start + 1) % SEQ_MOD;
        }
    }

    fn holding(&self) -> bool {
        self.buf.iter().any(|f| f.is_some())
    }

    fn note_held(&mut self, now: u64) {
        if self.holding() {
            self.held_since.get_or_insert(now);
        } else {
            self.held_since = None;
        }
    }
}

/// The reorder windows of one peer (the access point), by TID.
pub struct Reorder<T> {
    tids: [Option<Window<T>>; TIDS],
}

impl<T> Default for Reorder<T> {
    fn default() -> Self {
        Reorder { tids: core::array::from_fn(|_| None) }
    }
}

impl<T> Reorder<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// A block-ack session for `tid` starts at sequence number `ssn`
    /// with a window of `size` frames (1-64). Frames held for an earlier
    /// session of that TID are handed on first.
    pub fn start(&mut self, tid: u8, ssn: u16, size: u16, out: &mut Vec<T>) {
        let t = tid as usize % TIDS;
        self.stop(tid, out);
        let size = size.clamp(1, 64) as usize;
        self.tids[t] = Some(Window { start: ssn % SEQ_MOD, head: 0, buf: (0..size).map(|_| None).collect(), held_since: None });
    }

    /// The session for `tid` ends: hand on what it holds.
    pub fn stop(&mut self, tid: u8, out: &mut Vec<T>) {
        if let Some(mut w) = self.tids[tid as usize % TIDS].take() {
            let end = (w.start + w.size()) % SEQ_MOD;
            w.flush_to(end, out);
        }
    }

    /// Every session ends (the link went down).
    pub fn clear(&mut self, out: &mut Vec<T>) {
        for t in 0..TIDS as u8 {
            self.stop(t, out);
        }
    }

    pub fn active(&self, tid: u8) -> bool {
        self.tids[tid as usize % TIDS].is_some()
    }

    /// A received frame with sequence number `seq`: whatever can be
    /// handed on now goes to `out`, in order. Without a session for its
    /// TID it goes straight through. Old frames and duplicates are
    /// dropped.
    pub fn rx(&mut self, tid: u8, seq: u16, frame: T, now: u64, out: &mut Vec<T>) {
        let Some(w) = self.tids[tid as usize % TIDS].as_mut() else {
            out.push(frame);
            return;
        };
        let seq = seq % SEQ_MOD;
        let mut d = diff(seq, w.start);
        if d >= HALF {
            return; // before the window: already handed on or given up
        }
        if d >= w.size() {
            // Beyond the window: move it so that this frame is its last.
            let to = (seq + SEQ_MOD + 1 - w.size()) % SEQ_MOD;
            w.flush_to(to, out);
            d = w.size() - 1;
        }
        let slot = (w.head + d as usize) % w.buf.len();
        if w.buf[slot].is_none() {
            w.buf[slot] = Some(frame);
        }
        w.release(out);
        w.note_held(now);
    }

    /// A Block Ack Request: frames before `seq` will not come again.
    pub fn bar(&mut self, tid: u8, seq: u16, now: u64, out: &mut Vec<T>) {
        let Some(w) = self.tids[tid as usize % TIDS].as_mut() else { return };
        let seq = seq % SEQ_MOD;
        if diff(seq, w.start) >= HALF {
            return;
        }
        w.flush_to(seq, out);
        w.release(out);
        w.note_held(now);
    }

    /// Give up on gaps that held frames back for [`FLUSH_MS`]: hand on
    /// everything stored and move past it.
    pub fn tick(&mut self, now: u64, out: &mut Vec<T>) {
        for w in self.tids.iter_mut().flatten() {
            let Some(t) = w.held_since else { continue };
            if now.saturating_sub(t) < FLUSH_MS {
                continue;
            }
            let last = (0..w.buf.len()).rev().find(|&d| w.buf[(w.head + d) % w.buf.len()].is_some());
            if let Some(d) = last {
                let to = (w.start + d as u16 + 1) % SEQ_MOD;
                w.flush_to(to, out);
            }
            w.release(out);
            w.held_since = None;
            w.note_held(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn rx(r: &mut Reorder<u16>, seq: u16, now: u64) -> Vec<u16> {
        let mut out = Vec::new();
        r.rx(0, seq, seq, now, &mut out);
        out
    }

    #[test]
    fn without_a_session_frames_pass() {
        let mut r = Reorder::new();
        assert_eq!(rx(&mut r, 7, 0), vec![7]);
        assert_eq!(rx(&mut r, 3, 0), vec![3]);
    }

    #[test]
    fn in_order_and_out_of_order() {
        let mut r = Reorder::new();
        let mut out = Vec::new();
        r.start(0, 100, 8, &mut out);
        assert!(out.is_empty());
        assert_eq!(rx(&mut r, 100, 0), vec![100]);
        assert_eq!(rx(&mut r, 102, 0), vec![]);
        assert_eq!(rx(&mut r, 103, 0), vec![]);
        assert_eq!(rx(&mut r, 101, 0), vec![101, 102, 103]);
        // A duplicate and an old frame are dropped.
        assert_eq!(rx(&mut r, 103, 0), vec![]);
        assert_eq!(rx(&mut r, 90, 0), vec![]);
        assert_eq!(rx(&mut r, 104, 0), vec![104]);
    }

    #[test]
    fn a_frame_beyond_the_window_moves_it() {
        let mut r = Reorder::new();
        let mut out = Vec::new();
        r.start(0, 0, 4, &mut out);
        assert_eq!(rx(&mut r, 1, 0), vec![]);
        assert_eq!(rx(&mut r, 2, 0), vec![]);
        // 6 is beyond 0..3: the window becomes 3..6, 0 is given up.
        assert_eq!(rx(&mut r, 6, 0), vec![1, 2]);
        assert_eq!(rx(&mut r, 3, 0), vec![3]);
        assert_eq!(rx(&mut r, 5, 0), vec![]);
        assert_eq!(rx(&mut r, 4, 0), vec![4, 5, 6]);
    }

    #[test]
    fn sequence_numbers_wrap() {
        let mut r = Reorder::new();
        let mut out = Vec::new();
        r.start(0, 4094, 8, &mut out);
        assert_eq!(rx(&mut r, 0, 0), vec![]);
        assert_eq!(rx(&mut r, 4095, 0), vec![]);
        assert_eq!(rx(&mut r, 4094, 0), vec![4094, 4095, 0]);
        assert_eq!(rx(&mut r, 1, 0), vec![1]);
    }

    #[test]
    fn a_gap_is_skipped_after_the_timeout() {
        let mut r = Reorder::new();
        let mut out = Vec::new();
        r.start(0, 10, 16, &mut out);
        assert_eq!(rx(&mut r, 12, 100), vec![]);
        assert_eq!(rx(&mut r, 13, 110), vec![]);
        r.tick(140, &mut out);
        assert!(out.is_empty());
        r.tick(150, &mut out);
        assert_eq!(out, vec![12, 13]);
        // The window is past 13 now: 11 is old.
        assert_eq!(rx(&mut r, 11, 160), vec![]);
        assert_eq!(rx(&mut r, 14, 160), vec![14]);
    }

    #[test]
    fn block_ack_request_moves_the_window() {
        let mut r = Reorder::new();
        let mut out = Vec::new();
        r.start(0, 0, 8, &mut out);
        assert_eq!(rx(&mut r, 2, 0), vec![]);
        assert_eq!(rx(&mut r, 3, 0), vec![]);
        r.bar(0, 2, 0, &mut out);
        assert_eq!(out, vec![2, 3]);
        out.clear();
        r.stop(0, &mut out);
        assert!(!r.active(0));
        assert_eq!(rx(&mut r, 1, 0), vec![1]);
    }

    #[test]
    fn stopping_hands_on_what_is_held() {
        let mut r = Reorder::new();
        let mut out = Vec::new();
        r.start(5, 0, 8, &mut out);
        r.rx(5, 3, 3, 0, &mut out);
        r.rx(5, 1, 1, 0, &mut out);
        assert!(out.is_empty());
        r.clear(&mut out);
        assert_eq!(out, vec![1, 3]);
    }
}
