//! Signals during blocking calls: DosSendSignalException to a child that
//! is reading a pipe or waiting on a semaphore, with and without a
//! handler; Ctrl+C from the keyboard (`STRESS ctrlc`).

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, start_child, wait_child};

static SIGNALS: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn sig_handler(
    rep: *mut ExceptionReportRecord,
    _reg: *mut ExceptionRegistrationRecord,
    _ctx: *mut ContextRecord,
    _d: *mut u8,
) -> u32 {
    unsafe {
        if (*rep).num == XCPT_SIGNAL {
            SIGNALS.fetch_add(1, Ordering::Relaxed);
            DosAcknowledgeSignalException((*rep).info[0]);
            return XCPT_CONTINUE_EXECUTION;
        }
    }
    XCPT_CONTINUE_SEARCH
}

fn parse(s: Option<&alloc::string::String>) -> u32 {
    s.and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Child: with a signal handler, read pipe handle `args[0]`; the first
/// read is interrupted, the second gets data.
/// Exit code: signals * 1000 + first rc * 10 + (second read ok).
pub fn child_sig(args: &[alloc::string::String]) -> u32 {
    let h = parse(args.first());
    let mut rec = ExceptionRegistrationRecord { prev: null_mut(), handler: sig_handler };
    unsafe {
        DosSetExceptionHandler(&mut rec);
        let mut times = 0;
        DosSetSignalExceptionFocus(1, &mut times);
        let mut buf = [0u8; 8];
        let mut n = 0;
        let rc1 = DosRead(h, buf.as_mut_ptr(), 8, &mut n);
        let rc2 = DosRead(h, buf.as_mut_ptr(), 8, &mut n);
        let ok2 = (rc2 == 0 && n == 1 && buf[0] == b'x') as u32;
        DosUnsetExceptionHandler(&mut rec);
        SIGNALS.load(Ordering::Relaxed) * 1000 + rc1 * 10 + ok2
    }
}

/// Child: with a signal handler, wait on a private event (posted by
/// nobody); the wait is interrupted.
pub fn child_sigsem() -> u32 {
    let mut rec = ExceptionRegistrationRecord { prev: null_mut(), handler: sig_handler };
    unsafe {
        DosSetExceptionHandler(&mut rec);
        let mut ev = 0;
        DosCreateEventSem(null(), &mut ev, 0, 0);
        let rc = DosWaitEventSem(ev, 20_000);
        DosUnsetExceptionHandler(&mut rec);
        SIGNALS.load(Ordering::Relaxed) * 1000 + rc
    }
}

pub fn run() {
    unsafe {
        let (mut r, mut w) = (0, 0);
        DosCreatePipe(&mut r, &mut w, 512);
        let pid = start_child(&alloc::format!("sig {r}"));
        DosSleep(300);
        let rc = DosSendSignalException(pid, XCPT_SIGNAL_INTR);
        DosSleep(200);
        let mut n = 0;
        DosWrite(w, b"x".as_ptr(), 1, &mut n);
        let (wrc, cause, code, _) = wait_child(pid);
        if code != 1951 {
            println!("  child result {code} (signals*1000 + first rc*10 + second read ok)");
        }
        check("signal interrupts a pipe read (ERROR_INTERRUPT), read again works", rc == 0 && wrc == 0 && cause == 0 && code == 1951);
        DosClose(r);
        DosClose(w);

        let pid = start_child("sigsem");
        DosSleep(300);
        let rc = DosSendSignalException(pid, XCPT_SIGNAL_BREAK);
        let (_, cause, code, _) = wait_child(pid);
        check("signal interrupts DosWaitEventSem", rc == 0 && cause == 0 && code == 1000 + ERROR_INTERRUPT);

        // No handler: the default action ends the process.
        let pid = start_child("block event");
        DosSleep(300);
        let rc = DosSendSignalException(pid, XCPT_SIGNAL_INTR);
        let (_, cause, _, _) = wait_child(pid);
        check("unhandled signal ends the child", rc == 0 && cause == 3);

        check("bad signal number fails", DosSendSignalException(pid, 2) != 0);
    }
}

/// `STRESS ctrlc`: wait in a pipe read for Ctrl+C.
pub fn ctrlc() -> u32 {
    let mut rec = ExceptionRegistrationRecord { prev: null_mut(), handler: sig_handler };
    unsafe {
        DosSetExceptionHandler(&mut rec);
        let (mut r, mut w) = (0, 0);
        DosCreatePipe(&mut r, &mut w, 512);
        println!("press Ctrl+C");
        let mut buf = [0u8; 8];
        let mut n = 0;
        let rc = DosRead(r, buf.as_mut_ptr(), 8, &mut n);
        println!("pipe read returned {rc} after {} signal(s)", SIGNALS.load(Ordering::Relaxed));
        let ok = rc == ERROR_INTERRUPT && SIGNALS.load(Ordering::Relaxed) == 1;
        println!("{} Ctrl+C interrupts a pipe read", if ok { "PASS" } else { "FAIL" });
        DosUnsetExceptionHandler(&mut rec);
        (!ok) as u32
    }
}
