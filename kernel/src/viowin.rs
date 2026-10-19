//! Windowed VIO sessions ("OS/2 Window"): text sessions whose screen a
//! PM window shows instead of the framebuffer.
//!
//! Such a session is an ordinary session with its own `Screen`, keyboard
//! queue and mouse queue; its programs run as in a full-screen session.
//! What differs is who looks at the screen: the PM host (`PMVIOWIN.EXE`,
//! started on demand) reads the cells through RS2API and draws them, and
//! feeds the session the keys and mouse events PM routes to its window.
//! The session never waits for the host: the host waits for changes
//! (a generation counter bumped by every screen change of a windowed
//! session, by session list changes and by activation requests).
//!
//! Alt+Home switches such a session between its window and the full
//! screen.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::task::WaitQueue;

/// Bumped by every change the host must see.
static GEN: AtomicU32 = AtomicU32::new(1);
static WAIT: WaitQueue = WaitQueue::new();
/// The host process (0 = none running).
static HOST: AtomicU32 = AtomicU32::new(0);
/// A session whose window the host should activate (0 = none).
static ACTIVATE: AtomicU32 = AtomicU32::new(0);
/// The windowed session whose window has the focus (0 = none).
static FOCUS: AtomicU32 = AtomicU32::new(0);

/// Where the host lives.
pub const HOST_PATH: &str = "C:\\OS2\\PMVIOWIN.EXE";

/// Something the host shows changed. Safe with the screen lock held.
pub fn changed() {
    GEN.fetch_add(1, Ordering::Relaxed);
    WAIT.wake_all();
}

pub fn generation() -> u32 {
    GEN.load(Ordering::Relaxed)
}

/// Wait until the generation differs from `seen` (or `deadline`, in
/// ticks, passes). Returns the generation, or None on timeout or
/// interruption.
pub fn wait(seen: u32, deadline: Option<u64>) -> Option<u32> {
    if WAIT.wait_until(deadline, || GEN.load(Ordering::Relaxed) != seen) { Some(generation()) } else { None }
}

pub fn host() -> u32 {
    HOST.load(Ordering::Relaxed)
}

/// The calling process is the host.
pub fn register_host(pid: u32) {
    let cur = HOST.load(Ordering::Relaxed);
    if cur == 0 || cur == pid || !crate::task::process::all().iter().any(|p| p.pid == cur && p.exit_info().is_none()) {
        HOST.store(pid, Ordering::Relaxed);
    }
}

/// Start the host unless one runs. False if it cannot be started.
pub fn ensure_host(env: Vec<String>) -> bool {
    let cur = host();
    if cur != 0 && crate::task::process::all().iter().any(|p| p.pid == cur && p.exit_info().is_none()) {
        return true;
    }
    if crate::fs::read_file(HOST_PATH).is_none() {
        return false;
    }
    let pm = crate::session::pm_session();
    let r = crate::task::process::exec(crate::task::process::ExecParams {
        path: HOST_PATH,
        program: "PMVIOWIN.EXE",
        args: "",
        env,
        parent: None,
        handles: Vec::new(),
        detached: true,
        session: if pm != 0 { pm } else { 1 },
    });
    match r {
        Ok(p) => {
            HOST.store(p.pid, Ordering::Relaxed);
            true
        }
        Err(e) => {
            crate::println!("viowin: cannot start {HOST_PATH}: error {e}");
            false
        }
    }
}

/// Environment for a host started by the kernel: the boot environment's
/// essentials.
pub fn host_env() -> Vec<String> {
    crate::boot::env()
}

/// Bring the window of windowed session `sid` to the front: PM comes to
/// the foreground and the host activates the window.
pub fn activate(sid: u32) {
    ACTIVATE.store(sid, Ordering::Relaxed);
    if !ensure_host(host_env()) {
        return;
    }
    changed();
    let pm = crate::session::pm_session();
    if pm != 0 {
        crate::session::select(pm);
    }
}

/// The pending activation request (read once).
pub fn take_activate() -> u32 {
    ACTIVATE.swap(0, Ordering::Relaxed)
}

pub fn set_focus(sid: u32, on: bool) {
    if on {
        FOCUS.store(sid, Ordering::Relaxed);
    } else {
        let _ = FOCUS.compare_exchange(sid, 0, Ordering::Relaxed, Ordering::Relaxed);
    }
}

pub fn focus() -> u32 {
    FOCUS.load(Ordering::Relaxed)
}

/// A process ended.
pub fn process_ended(pid: u32) {
    let _ = HOST.compare_exchange(pid, 0, Ordering::Relaxed, Ordering::Relaxed);
}

/// A session ended.
pub fn session_ended(sid: u32) {
    let _ = FOCUS.compare_exchange(sid, 0, Ordering::Relaxed, Ordering::Relaxed);
    let _ = ACTIVATE.compare_exchange(sid, 0, Ordering::Relaxed, Ordering::Relaxed);
    changed();
}
