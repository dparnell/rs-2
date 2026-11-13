//! Pseudo terminal sessions: text sessions that no screen shows.
//!
//! A server (rs2's TELNETD) starts a program in such a session
//! (Rs2PtyStart, api/pty.rs). The session has its own `Screen` and
//! keyboard queue like any other, so full-screen VIO programs, KBD calls
//! and the console handles all work in it, but it never comes to the
//! foreground and is in no window list. The server reads the screen's
//! cells (Rs2VioReadScreen), waits for changes (Rs2PtyWait: every change
//! of a pty session's screen bumps its `changes` counter and wakes the
//! waiters here), types into it (Rs2VioKey) and ends it (Rs2VioControl
//! stop) - what PMVIOWIN does for a windowed session, without PM.

use crate::task::WaitQueue;

static WAIT: WaitQueue = WaitQueue::new();
/// Bumped by every change: waiters test only this under the queue's lock
/// (looking at the screen there would take the screen lock inside the
/// queue's, the reverse of `changed`, which runs under the screen lock).
static GEN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// The screen of a pty session changed, or one ended. Safe with the
/// screen lock held.
pub fn changed() {
    GEN.fetch_add(1, core::sync::atomic::Ordering::Release);
    WAIT.wake_all();
}

/// The change count of all pty sessions together.
pub fn generation() -> u32 {
    GEN.load(core::sync::atomic::Ordering::Acquire)
}

/// Wait until a change after generation `g` (or `deadline`, in ticks).
pub fn wait(deadline: Option<u64>, g: u32) -> bool {
    WAIT.wait_until(deadline, || generation() != g)
}
