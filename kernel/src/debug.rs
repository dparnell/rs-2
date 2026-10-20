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
pub fn dump_threads() {
    crate::trace!("---- threads (Ctrl+Alt+F12) ----");
    let mut v = alloc::string::String::new();
    for i in crate::msi::FIRST..=crate::msi::LAST {
        let n = crate::msi::count(i);
        if n != 0 {
            v.push_str(&alloc::format!(" {i:#x}:{n}"));
        }
    }
    crate::trace!("msi counts:{v}; waits that timed out (missed): {}", crate::msi::missed());
    let Some(procs) = crate::task::process::try_all() else {
        crate::trace!("process list busy");
        crate::smp::dump_all();
        return;
    };
    for p in procs {
        let Some(threads) = p.threads.try_lock().map(|g| g.clone()) else {
            crate::trace!("pid {}: thread list busy", p.pid);
            continue;
        };
        for t in threads {
            let state = t.state();
            let f = t.user_frame();
            crate::trace!(
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
            crate::trace!("    kernel:{}", stack_line(sp, top));
        }
    }
    crate::trace!("---- CPUs ----");
    crate::smp::dump_all();
}
