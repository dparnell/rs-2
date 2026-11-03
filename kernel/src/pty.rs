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

/// The screen of a pty session changed, or one ended. Safe with the
/// screen lock held.
pub fn changed() {
    WAIT.wake_all();
}

/// Wait until `cond` holds (or `deadline`, in ticks, passes).
pub fn wait(deadline: Option<u64>, cond: impl Fn() -> bool) -> bool {
    WAIT.wait_until(deadline, cond)
}
