//! Debugging aids: Ctrl+Alt+F12 prints every thread to the serial port
//! with its state, where it is in user mode and the kernel return
//! addresses on its stack (symbolise them with llvm-addr2line against the
//! kernel ELF). Locks are only tried, so a dump from a wedged system
//! cannot deadlock; whatever was busy is reported as such.

use crate::task::thread::State;

unsafe extern "C" {
    static __kernel_start: u8;
    static __text_end: u8;
}

fn in_text(v: u64) -> bool {
    let (lo, hi) = unsafe { (&__kernel_start as *const u8 as u64, &__text_end as *const u8 as u64) };
    (lo..hi).contains(&v)
}

/// Return addresses on the current kernel stack above `from`.
fn stack_line(from: u64, top: u64) -> alloc::string::String {
    let mut line = alloc::string::String::new();
    let mut n = 0;
    let mut a = from & !7;
    while a < top && n < 24 {
        let v = unsafe { *(a as *const u64) };
        if in_text(v) {
            line.push_str(&alloc::format!(" {v:x}"));
            n += 1;
        }
        a += 8;
    }
    line
}

/// This CPU's interrupted state and kernel stack (an IPI, or the
/// keyboard interrupt on the CPU that took it).
pub fn dump_cpu(f: &crate::arch::idt::TrapFrame) {
    // One CPU's report at a time (they arrive together).
    static OUT: crate::sync::SpinLock<()> = crate::sync::SpinLock::new(());
    let _g = OUT.lock();
    let me = crate::smp::this();
    let cur = crate::task::sched::current_opt();
    let (id, top) = cur.as_ref().map_or((0, 0), |t| (t.id, t.kstack_top()));
    let here = f as *const _ as u64;
    let stack = if top != 0 && (top - 0x10_0000..top).contains(&here) { stack_line(here, top) } else { alloc::string::String::from(" (not on a thread stack)") };
    crate::trace!("cpu {} thread {} at {:04x}:{:x} rflags {:x}\n    kernel:{}", me.index(), id, f.cs, f.rip, f.rflags, stack);
}

/// Print all threads (from the keyboard interrupt); every CPU, this one
/// included, then reports where it was through an IPI.
/// Ctrl+Alt+F12: the thread report on the serial port.
pub fn dump_threads() {
    thread_report();
}

/// Every thread's state, priority and where it is (kernel return
/// addresses for those not running), on the serial port and as text
/// (Rs2QueryThreadReport, the rescue screen's report).
pub fn thread_report() -> alloc::string::String {
    let mut out = alloc::string::String::new();
    macro_rules! emit {
        ($($a:tt)*) => {{
            let s = alloc::format!($($a)*);
            crate::trace!("{}", s);
            out.push_str(&s);
            out.push('\n');
        }};
    }
    emit!("---- threads (Ctrl+Alt+F12) ----");
    let mut v = alloc::string::String::new();
    for i in crate::msi::FIRST..=crate::msi::LAST {
        let n = crate::msi::count(i);
        if n != 0 {
            v.push_str(&alloc::format!(" {i:#x}:{n}"));
        }
    }
    let (slept, polled) = crate::msi::waits();
    emit!("msi counts:{v}; waits that timed out (missed): {}; waits slept {slept}, polled {polled}", crate::msi::missed());
    let Some(procs) = crate::task::process::try_all() else {
        emit!("process list busy");
        crate::smp::dump_all();
        return out;
    };
    for p in procs {
        let Some(threads) = p.threads.try_lock().map(|g| g.clone()) else {
            emit!("pid {}: thread list busy", p.pid);
            continue;
        };
        for t in threads {
            let state = t.state();
            let f = t.user_frame();
            emit!(
                "pid {} tid {} id {} {:?} prio {:#x} on_cpu {} user {:04x}:{:08x}",
                p.pid,
                t.tid,
                t.id,
                state,
                t.priority(),
                t.on_cpu.load(core::sync::atomic::Ordering::Relaxed),
                f.cs,
                f.rip
            );
            if state == State::Running {
                continue;
            }
            // Return addresses between the saved stack pointer and the
            // user frame (a heuristic: any word that points into .text).
            let sp = unsafe { *t.rsp_slot() };
            let top = t.kstack_top();
            if !(top - 0x10_0000..top).contains(&sp) {
                continue;
            }
            emit!("    kernel:{}", stack_line(sp, top));
        }
    }
    emit!("---- CPUs ----");
    crate::smp::dump_all();
    out
}
