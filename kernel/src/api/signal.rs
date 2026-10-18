//! Signals: Ctrl+C / Ctrl+Break and DosKillProcess-style termination
//! requests delivered to programs.
//!
//! 32-bit programs receive XCPT_SIGNAL through their exception handlers;
//! 16-bit programs get the handler registered with DosSetSigHandler (or the
//! signal is ignored / ends the process per its action).

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::arch::idt::TrapFrame;
use crate::task::thread::{State, Thread};
use crate::task::{process, sched, Process};
use crate::vm;

/// 16-bit signal numbers.
pub const SIG_CTRLC: u32 = 1;
pub const SIG_BROKENPIPE: u32 = 2;
pub const SIG_KILLPROCESS: u32 = 3;
pub const SIG_CTRLBREAK: u32 = 4;

/// XCPT_SIGNAL parameters.
const XCPT_SIGNAL_INTR: u32 = 1;
const XCPT_SIGNAL_KILLPROC: u32 = 3;
const XCPT_SIGNAL_BREAK: u32 = 4;

/// Actions for DosSetSigHandler.
const SIGA_KILL: u32 = 0;
const SIGA_IGNORE: u32 = 1;
const SIGA_ACCEPT: u32 = 2;
const SIGA_ERROR: u32 = 3;

/// The process that receives keyboard signals: the most recently started
/// live process in the (single) session.
pub fn foreground() -> Option<Arc<Process>> {
    let all: Vec<Arc<Process>> = process::all();
    all.into_iter()
        .filter(|p| p.exit_info().is_none() && !p.detached.load(Ordering::Relaxed))
        .max_by_key(|p| p.pid)
}

/// Thread that receives signals (thread 1, or the first live thread).
fn target_thread(p: &Process) -> Option<Arc<Thread>> {
    let ts = p.threads.lock();
    ts.iter().find(|t| t.tid == 1 && t.state() != State::Dead).or_else(|| ts.iter().find(|t| t.state() != State::Dead)).cloned()
}

/// Post a signal to a process (from interrupt or kernel context).
pub fn post(p: &Arc<Process>, sig: u32) {
    *p.pending_signal.lock() = Some(sig);
    if let Some(t) = target_thread(p) {
        t.interrupt();
        sched::wake(&t);
    }
}

/// Keyboard hook: Ctrl+C or Ctrl+Break pressed.
pub fn keyboard(sig: u32) {
    if let Some(p) = foreground() {
        post(&p, sig);
    }
}

/// Deliver a pending signal on the way back to user mode. Returns true if
/// the frame was redirected (or the process told to exit).
pub fn deliver(t: &Arc<Thread>, p: &Arc<Process>, f: &mut TrapFrame) -> bool {
    if target_thread(p).map(|x| x.id) != Some(t.id) {
        return false;
    }
    let Some(sig) = p.pending_signal.lock().take() else { return false };
    t.clear_interrupt();
    if !p.is_16bit() {
        let info = match sig {
            SIG_CTRLBREAK => XCPT_SIGNAL_BREAK,
            SIG_KILLPROCESS => XCPT_SIGNAL_KILLPROC,
            _ => XCPT_SIGNAL_INTR,
        };
        let code = super::exceptions::xcpt::SIGNAL;
        if !super::exceptions::raise(f, p, t, code, 0, &[info], f.rip as u32) {
            super::exceptions::default_action(f, p, t, code);
        }
        return true;
    }
    let (handler, action) = p.signals.lock()[sig.min(7) as usize];
    match action {
        SIGA_IGNORE => false,
        SIGA_ACCEPT | SIGA_ERROR if handler != 0 => {
            // Far _Pascal handler(USHORT usSigArg, USHORT usSigNum) that
            // returns (RETF 4) to the interrupted instruction.
            let mut sp = p.space.lock();
            let ss_base = vm::desc_base(sp.descriptor(f.ss as u16));
            let spv = (f.rsp as u32 & 0xffff).wrapping_sub(8) & 0xffff;
            let mut b = Vec::new();
            b.extend_from_slice(&(f.rip as u16).to_le_bytes());
            b.extend_from_slice(&(f.cs as u16).to_le_bytes());
            b.extend_from_slice(&(sig as u16).to_le_bytes()); // usSigNum
            b.extend_from_slice(&0u16.to_le_bytes()); // usSigArg
            sp.poke_bytes(ss_base + spv, &b);
            f.rsp = (f.rsp & !0xffff) | spv as u64;
            f.cs = (handler >> 16) as u64;
            f.rip = (handler & 0xffff) as u64;
            true
        }
        _ => {
            let _ = SIGA_KILL;
            p.exit(3, 0);
            true
        }
    }
}
