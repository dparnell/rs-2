//! Exceptions: a handler raising a nested exception, unwinding handlers
//! from inside a handler, a fault inside a handler, many exceptions in a
//! row, an unhandled trap ending a child.

use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

use crate::api::*;
use crate::{check, run_child};

/// Event log: each handler call appends a code.
static LOG: [AtomicU32; 16] = [const { AtomicU32::new(0) }; 16];
static NLOG: AtomicU32 = AtomicU32::new(0);
static OUTER: AtomicU32 = AtomicU32::new(0);
static FAULTS: AtomicU32 = AtomicU32::new(0);
static NESTED_SEEN: AtomicU32 = AtomicU32::new(0);

const X: u32 = 0x2000_0100;
const Y: u32 = 0x2000_0200;
const Z: u32 = 0x2000_0300;

fn log(v: u32) {
    let i = NLOG.fetch_add(1, Ordering::Relaxed) as usize;
    if i < LOG.len() {
        LOG[i].store(v, Ordering::Relaxed);
    }
}

fn logged() -> alloc::vec::Vec<u32> {
    let n = (NLOG.load(Ordering::Relaxed) as usize).min(LOG.len());
    LOG[..n].iter().map(|v| v.load(Ordering::Relaxed)).collect()
}

fn reset() {
    NLOG.store(0, Ordering::Relaxed);
}

fn raise(code: u32) -> u32 {
    let mut r: ExceptionReportRecord = unsafe { core::mem::zeroed() };
    r.num = code;
    unsafe { DosRaiseException(&mut r) }
}

type Handler = ExceptionHandler;

unsafe extern "C" fn outer_a(rep: *mut ExceptionReportRecord, _: *mut ExceptionRegistrationRecord, ctx: *mut ContextRecord, _: *mut u8) -> u32 {
    unsafe {
        if (*rep).flags & EH_UNWINDING != 0 {
            log(0xA0);
            return XCPT_CONTINUE_SEARCH;
        }
        match (*rep).num {
            Y => {
                log(0xA2);
                XCPT_CONTINUE_EXECUTION
            }
            Z => {
                log(0xA3);
                // Unwind everything above this record, then carry on.
                let me = OUTER.load(Ordering::Relaxed) as *mut ExceptionRegistrationRecord;
                DosUnwindException(me, core::ptr::null(), rep);
                log(0xA4);
                XCPT_CONTINUE_EXECUTION
            }
            XCPT_ACCESS_VIOLATION => {
                log(0xA5);
                (*ctx).eip += 2;
                XCPT_CONTINUE_EXECUTION
            }
            _ => XCPT_CONTINUE_SEARCH,
        }
    }
}

unsafe extern "C" fn inner_b(rep: *mut ExceptionReportRecord, _: *mut ExceptionRegistrationRecord, ctx: *mut ContextRecord, _: *mut u8) -> u32 {
    unsafe {
        if (*rep).flags & EH_UNWINDING != 0 {
            log(0xB0);
            return XCPT_CONTINUE_SEARCH;
        }
        match (*rep).num {
            X => {
                log(0xB1);
                // Raise another exception from inside the handler.
                let rc = raise(Y);
                log(if rc == 0 { 0xB3 } else { 0xBE });
                XCPT_CONTINUE_EXECUTION
            }
            Y => {
                log(if (*rep).flags & EH_NESTED_CALL != 0 { 0xB2 } else { 0xBF });
                XCPT_CONTINUE_SEARCH
            }
            0x2000_0400 => {
                // Fault inside the handler: the outer handler skips it.
                log(0xB4);
                core::arch::asm!("xor eax, eax", "mov dword ptr [eax], ecx", out("eax") _, in("ecx") 1);
                log(0xB5);
                XCPT_CONTINUE_EXECUTION
            }
            _ => {
                let _ = ctx;
                XCPT_CONTINUE_SEARCH
            }
        }
    }
}

unsafe extern "C" fn inner_c(rep: *mut ExceptionReportRecord, _: *mut ExceptionRegistrationRecord, _: *mut ContextRecord, _: *mut u8) -> u32 {
    unsafe {
        if (*rep).flags & EH_UNWINDING != 0 {
            log(0xC0);
        }
        XCPT_CONTINUE_SEARCH
    }
}

/// Skip the faulting 2-byte store and count.
unsafe extern "C" fn skipper(rep: *mut ExceptionReportRecord, _: *mut ExceptionRegistrationRecord, ctx: *mut ContextRecord, _: *mut u8) -> u32 {
    unsafe {
        if (*rep).flags & EH_UNWINDING != 0 {
            return XCPT_CONTINUE_SEARCH;
        }
        if (*rep).flags & EH_NESTED_CALL != 0 {
            NESTED_SEEN.fetch_add(1, Ordering::Relaxed);
        }
        if (*rep).num == XCPT_ACCESS_VIOLATION {
            FAULTS.fetch_add(1, Ordering::Relaxed);
            (*ctx).eip += 2;
            return XCPT_CONTINUE_EXECUTION;
        }
        if (*rep).num == X {
            return XCPT_CONTINUE_EXECUTION;
        }
    }
    XCPT_CONTINUE_SEARCH
}

fn rec(h: Handler) -> ExceptionRegistrationRecord {
    ExceptionRegistrationRecord { prev: null_mut(), handler: h }
}

fn head() -> u32 {
    let h: u32;
    unsafe { core::arch::asm!("mov {0}, fs:[0]", out(reg) h) };
    h
}

pub fn run() {
    unsafe {
        let mut a = rec(outer_a);
        let mut b = rec(inner_b);
        let mut c = rec(inner_c);
        DosSetExceptionHandler(&mut a);
        OUTER.store(&mut a as *mut _ as u32, Ordering::Relaxed);
        DosSetExceptionHandler(&mut b);

        reset();
        let rc = raise(X);
        let l = logged();
        if l != [0xB1, 0xB2, 0xA2, 0xB3] {
            println!("  log {:x?}", l);
        }
        check("nested exception from a handler", rc == 0 && l == [0xB1, 0xB2, 0xA2, 0xB3]);

        reset();
        let rc = raise(0x2000_0400);
        let l = logged();
        if l != [0xB4, 0xA5, 0xB5] {
            println!("  log {:x?}", l);
        }
        check("fault inside a handler handled by the outer one", rc == 0 && l == [0xB4, 0xA5, 0xB5]);

        DosSetExceptionHandler(&mut c);
        reset();
        let rc = raise(Z);
        let l = logged();
        if l != [0xA3, 0xC0, 0xB0, 0xA4] {
            println!("  log {:x?}", l);
        }
        check("unwind from inside a handler", rc == 0 && l == [0xA3, 0xC0, 0xB0, 0xA4]);
        check("chain head is the target after the unwind", head() == &mut a as *mut _ as u32);
        DosUnsetExceptionHandler(&mut a);

        // Exceptions after an unwind are not nested any more.
        let mut s = rec(skipper);
        DosSetExceptionHandler(&mut s);
        NESTED_SEEN.store(0, Ordering::Relaxed);
        FAULTS.store(0, Ordering::Relaxed);
        for _ in 0..2000 {
            core::arch::asm!("xor eax, eax", "mov dword ptr [eax], ecx", out("eax") _, in("ecx") 1);
        }
        check("2000 access violations handled", FAULTS.load(Ordering::Relaxed) == 2000);
        check("no stale nested state", NESTED_SEEN.load(Ordering::Relaxed) == 0);
        for _ in 0..500 {
            raise(X);
        }
        check("500 raised exceptions handled", NESTED_SEEN.load(Ordering::Relaxed) == 0);
        DosUnsetExceptionHandler(&mut s);

        // Termination: a killed thread's handlers see
        // XCPT_ASYNC_PROCESS_TERMINATE, one that exits itself sees
        // XCPT_PROCESS_TERMINATE; a handler that faults meanwhile does not
        // keep the thread alive.
        TERM.store(0, Ordering::Relaxed);
        let t = crate::thread(term_victim, 0);
        DosSleep(50);
        crate::api::DosKillThread(t);
        let rc = crate::join(t);
        check("killed thread's handler sees XCPT_ASYNC_PROCESS_TERMINATE", rc == 0 && TERM.load(Ordering::Relaxed) == 0xC001_0002);
        TERM.store(0, Ordering::Relaxed);
        let t = crate::thread(term_self, 0);
        crate::join(t);
        check("exiting thread's handler sees XCPT_PROCESS_TERMINATE", TERM.load(Ordering::Relaxed) == 0xC001_0001);
        TERM.store(0, Ordering::Relaxed);
        let t = crate::thread(term_faulty, 0);
        DosSleep(50);
        crate::api::DosKillThread(t);
        check("faulting termination handler ends the thread", crate::join(t) == 0 && TERM.load(Ordering::Relaxed) == 1);

        // Unhandled trap in a child: TC_TRAP.
        let (cause, _) = run_child("crash");
        check("unhandled access violation ends the child with TC_TRAP", cause == 2);
        let (cause, code) = run_child("raise");
        check("unhandled raised exception ends the child", cause == 2 || (cause == 0 && code != 0));
    }
}

static TERM: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn term_handler(rep: *mut ExceptionReportRecord, _: *mut ExceptionRegistrationRecord, _: *mut ContextRecord, _: *mut u8) -> u32 {
    unsafe {
        if (*rep).flags & EH_UNWINDING != 0 && ((*rep).num == 0xC001_0001 || (*rep).num == 0xC001_0002) {
            TERM.store((*rep).num, Ordering::Relaxed);
        }
    }
    XCPT_CONTINUE_SEARCH
}

unsafe extern "C" fn faulty_handler(rep: *mut ExceptionReportRecord, _: *mut ExceptionRegistrationRecord, _: *mut ContextRecord, _: *mut u8) -> u32 {
    unsafe {
        if (*rep).flags & EH_UNWINDING != 0 {
            TERM.fetch_add(1, Ordering::Relaxed);
            core::arch::asm!("xor eax, eax", "mov dword ptr [eax], ecx", out("eax") _, in("ecx") 1);
        }
    }
    XCPT_CONTINUE_SEARCH
}

unsafe extern "C" fn term_victim(_: u32) {
    let mut r = rec(term_handler);
    unsafe {
        DosSetExceptionHandler(&mut r);
        DosSleep(100_000);
        DosUnsetExceptionHandler(&mut r);
    }
}

unsafe extern "C" fn term_self(_: u32) {
    let mut r = rec(term_handler);
    unsafe {
        DosSetExceptionHandler(&mut r);
        DosExit(EXIT_THREAD, 0);
    }
}

unsafe extern "C" fn term_faulty(_: u32) {
    let mut r = rec(faulty_handler);
    unsafe {
        DosSetExceptionHandler(&mut r);
        DosSleep(100_000);
    }
}

/// Child: access violation without a handler.
pub fn child_crash() -> u32 {
    unsafe { core::arch::asm!("xor eax, eax", "mov dword ptr [eax], ecx", out("eax") _, in("ecx") 1) };
    0
}

pub fn child_raise() -> u32 {
    raise(0xE000_0001);
    0
}
