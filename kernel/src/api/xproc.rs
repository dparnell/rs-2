//! More process and thread services: signal exceptions sent between
//! processes and their acknowledgement, signal focus, must-complete
//! sections, thread-local memory, DosCreateThread2, the handle limit,
//! DosQueryModFromEIP, DosVerifyPidTid, process dumps.

use alloc::string::String;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use super::signal::{SIG_CTRLBREAK, SIG_CTRLC};
use super::{err, Ctx};
use crate::task::process::{self, info, TLS_DWORDS};
use crate::task::thread::{State, Thread};
use crate::task::Process;

const XCPT_SIGNAL_INTR: u32 = 1;
const XCPT_SIGNAL_KILLPROC: u32 = 3;
const XCPT_SIGNAL_BREAK: u32 = 4;
const ERROR_NESTING_TOO_DEEP: u32 = 173; // ERROR_NESTING_NOT_ALLOWED
const ERROR_BAD_LENGTH: u32 = 24;
const MAX_NESTING: u32 = 0xffff;

/// A thread ended (Process::thread_exited).
pub fn thread_ended(_p: &Arc<Process>, _tid: u32) {}

fn live(p: &Process) -> bool {
    p.exit_info().is_none()
}

// ---- Signals ----

/// DosSendSignalException(pid, exception): XCPT_SIGNAL_INTR or
/// XCPT_SIGNAL_BREAK to another process (its handlers see XCPT_SIGNAL).
pub fn dos_send_signal_exception(c: &mut Ctx) -> u32 {
    let (pid, x) = (c.arg(0), c.arg(1));
    let sig = match x {
        XCPT_SIGNAL_INTR => SIG_CTRLC,
        XCPT_SIGNAL_BREAK => SIG_CTRLBREAK,
        _ => return err::INVALID_FUNCTION,
    };
    let Some(p) = process::by_pid(pid).filter(|p| live(p)) else { return err::INVALID_PROCID };
    super::signal::post(&p, sig);
    0
}

/// DosAcknowledgeSignalException(ulSignalNum): further signals of that
/// kind may be delivered again.
pub fn dos_acknowledge_signal_exception(c: &mut Ctx) -> u32 {
    let sig = c.arg(0);
    let bit = match sig {
        XCPT_SIGNAL_INTR => SIG_CTRLC,
        XCPT_SIGNAL_KILLPROC => super::signal::SIG_KILLPROCESS,
        XCPT_SIGNAL_BREAK => SIG_CTRLBREAK,
        _ => return err::INVALID_SIGNAL_NUMBER,
    };
    c.process.sig_held.fetch_and(!(1 << bit), Ordering::Relaxed);
    super::signal::release(&c.process);
    0
}

/// DosSetSignalExceptionFocus(flag, pulTimes): SIG_SETFOCUS (1) or
/// SIG_UNSETFOCUS (0); pulTimes gets the nesting count.
pub fn dos_set_signal_exception_focus(c: &mut Ctx) -> u32 {
    let (flag, ptimes) = (c.arg(0), c.arg(1));
    let f = &c.process.sig_focus;
    let n = match flag {
        1 => {
            let n = f.load(Ordering::Relaxed);
            if n >= MAX_NESTING {
                return ERROR_NESTING_TOO_DEEP;
            }
            f.store(n + 1, Ordering::Relaxed);
            n + 1
        }
        0 => {
            let n = f.load(Ordering::Relaxed);
            if n == 0 {
                return err::ALREADY_RESET;
            }
            f.store(n - 1, Ordering::Relaxed);
            n - 1
        }
        _ => return err::INVALID_PARAMETER,
    };
    if ptimes != 0 {
        c.write_u32(ptimes, n);
    }
    0
}

/// TIB2 tib2_usMCCount: the must-complete count programs may look at.
fn write_mc(c: &Ctx, n: u32) {
    let tib2 = c.thread.tib() + info::TIB2;
    c.write_u16(tib2 + 12, n as u16);
}

/// DosEnterMustComplete(pulNesting): signals are held until the matching
/// DosExitMustComplete.
pub fn dos_enter_must_complete(c: &mut Ctx) -> u32 {
    let n = c.thread.must_complete.load(Ordering::Relaxed);
    if n >= MAX_NESTING {
        return ERROR_NESTING_TOO_DEEP;
    }
    c.thread.must_complete.store(n + 1, Ordering::Relaxed);
    write_mc(c, n + 1);
    if c.arg(0) != 0 {
        c.write_u32(c.arg(0), n + 1);
    }
    0
}

pub fn dos_exit_must_complete(c: &mut Ctx) -> u32 {
    let n = c.thread.must_complete.load(Ordering::Relaxed);
    if n == 0 {
        return err::ALREADY_RESET;
    }
    c.thread.must_complete.store(n - 1, Ordering::Relaxed);
    write_mc(c, n - 1);
    if c.arg(0) != 0 {
        c.write_u32(c.arg(0), n - 1);
    }
    if n == 1 {
        super::signal::release(&c.process);
    }
    0
}

/// 16-bit DosSendSignal(idProcess, usSigNumber): Ctrl+C (1) or Ctrl+Break
/// (4) to a process.
pub fn dos16_send_signal(c: &mut Ctx) -> u32 {
    let (pid, sig) = (c.arg(0), c.arg(1));
    if sig != SIG_CTRLC && sig != SIG_CTRLBREAK {
        return err::INVALID_SIGNAL_NUMBER;
    }
    let Some(p) = process::by_pid(pid).filter(|p| live(p)) else { return err::INVALID_PROCID };
    super::signal::post(&p, sig);
    0
}

/// 16-bit DosFlagProcess(pid, fScope, usFlagNum, usFlagArg): process
/// flags A, B, C are signals 5, 6, 7.
pub fn dos16_flag_process(c: &mut Ctx) -> u32 {
    let (pid, scope, flag) = (c.arg(0), c.arg(1), c.arg(2));
    if flag > 2 {
        return err::INVALID_FLAG_NUMBER;
    }
    let Some(p) = process::by_pid(pid).filter(|p| live(p)) else { return err::INVALID_PROCID };
    let mut targets = alloc::vec![p];
    if scope == 0 {
        // FLGP_SUBTREE: the descendants too.
        let mut i = 0;
        while i < targets.len() {
            let parent = targets[i].pid;
            for q in process::all() {
                if q.ppid == parent && live(&q) && !targets.iter().any(|t| t.pid == q.pid) {
                    targets.push(q);
                }
            }
            i += 1;
        }
    }
    for t in &targets {
        // Only processes with a handler for the flag get it.
        if t.signals.lock()[5 + flag as usize].1 == 2 {
            super::signal::post(t, 5 + flag);
        }
    }
    0
}

// ---- Thread-local memory ----

/// Save and restore the running thread's thread-local memory around a
/// context switch (scheduler, interrupts off).
pub fn switch_tls(prev: &Thread, next: &Thread) {
    if let Some(p) = prev.process() {
        let ph = p.tls_phys.load(Ordering::Relaxed);
        if ph != 0 && prev.state() != State::Dead {
            let src = crate::mm::phys::phys_to_virt(ph) as *const u32;
            unsafe { core::ptr::copy_nonoverlapping(src, (*prev.tls.get()).as_mut_ptr(), TLS_DWORDS) };
        }
    }
    if let Some(p) = next.process() {
        let ph = p.tls_phys.load(Ordering::Relaxed);
        if ph != 0 {
            let dst = crate::mm::phys::phys_to_virt(ph) as *mut u32;
            unsafe { core::ptr::copy_nonoverlapping((*next.tls.get()).as_ptr(), dst, TLS_DWORDS) };
        }
    }
}

/// DosAllocThreadLocalMemory(cb, ppul): cb dwords (1..8) at an address
/// that is the same in every thread but holds each thread's own values.
pub fn dos_alloc_thread_local_memory(c: &mut Ctx) -> u32 {
    let (n, pp) = (c.arg(0), c.arg(1));
    if n == 0 || n > 8 {
        return err::INVALID_PARAMETER;
    }
    let page = c.process.info + info::TLS;
    if c.process.tls_phys.load(Ordering::Relaxed) == 0 {
        let phys = {
            let mut sp = c.space();
            if !sp.poke_bytes(page, &[0u8; TLS_DWORDS * 4]) {
                return err::NOT_ENOUGH_MEMORY;
            }
            crate::mm::paging::translate(sp.root, page as u64)
        };
        let Some(phys) = phys else { return err::NOT_ENOUGH_MEMORY };
        c.process.tls_phys.store(phys, Ordering::Relaxed);
    }
    let used = c.process.tls_used.load(Ordering::Relaxed);
    let mask = (1u64 << n) - 1;
    let Some(first) = (0..=TLS_DWORDS as u32 - n).find(|&i| (used as u64 >> i) & mask == 0) else {
        return err::NOT_ENOUGH_MEMORY;
    };
    c.process.tls_used.store(used | (mask << first) as u32, Ordering::Relaxed);
    // Allocation sizes, so DosFreeThreadLocalMemory knows how much to free:
    // kept in the dword bits above (one run per allocation start).
    TLS_RUNS.lock().push((c.process.pid, first, n));
    let addr = page + first * 4;
    // Fresh memory reads as zero in every thread.
    for t in c.process.threads.lock().iter() {
        let tls = unsafe { &mut *t.tls.get() };
        for d in &mut tls[first as usize..(first + n) as usize] {
            *d = 0;
        }
    }
    c.write(addr, &alloc::vec![0u8; n as usize * 4]);
    c.write_u32(pp, addr);
    0
}

static TLS_RUNS: crate::sync::SpinLock<alloc::vec::Vec<(u32, u32, u32)>> = crate::sync::SpinLock::new(alloc::vec::Vec::new());

pub fn dos_free_thread_local_memory(c: &mut Ctx) -> u32 {
    let p = c.arg(0);
    let page = c.process.info + info::TLS;
    if p < page || p >= page + TLS_DWORDS as u32 * 4 || p % 4 != 0 {
        return err::INVALID_PARAMETER;
    }
    let first = (p - page) / 4;
    let pid = c.process.pid;
    let run = {
        let mut r = TLS_RUNS.lock();
        let i = r.iter().position(|e| e.0 == pid && e.1 == first);
        i.map(|i| r.remove(i))
    };
    let Some((_, first, n)) = run else { return err::INVALID_PARAMETER };
    let mask = (((1u64 << n) - 1) << first) as u32;
    c.process.tls_used.fetch_and(!mask, Ordering::Relaxed);
    0
}

/// A process ended: forget its thread-local allocations.
pub fn process_ended(pid: u32) {
    TLS_RUNS.lock().retain(|e| e.0 != pid);
}

// ---- Threads ----

/// DosCreateThread2(PTHREADCREATE): cbSize, pTid, pfnStart, lParam, lFlag,
/// pStack (0 = the system allocates cbStack bytes), cbStack.
pub fn dos_create_thread2(c: &mut Ctx) -> u32 {
    let tc = c.arg(0);
    let Some(cb) = c.read_u32(tc) else { return err::INVALID_ADDRESS };
    if cb < 28 {
        return err::INVALID_PARAMETER;
    }
    let f: alloc::vec::Vec<u32> = (0..7).map(|i| c.read_u32(tc + i * 4).unwrap_or(0)).collect();
    let (ptid, pfn, param, flag, stack, cbstack) = (f[1], f[2], f[3], f[4], f[5], f[6]);
    if pfn == 0 {
        return err::INVALID_PARAMETER;
    }
    let user = if stack != 0 {
        if cbstack < 4096 {
            return err::INVALID_PARAMETER;
        }
        Some((stack, cbstack))
    } else {
        None
    };
    super::proc::create_thread32(c, ptid, pfn, param, flag, cbstack, user)
}

/// DosVerifyPidTid(pid, tid)
pub fn dos_verify_pid_tid(c: &mut Ctx) -> u32 {
    let (pid, tid) = (c.arg(0), c.arg(1));
    let Some(p) = process::by_pid(pid).filter(|p| live(p)) else { return err::INVALID_PROCID };
    let ok = p.threads.lock().iter().any(|t| t.tid == tid && t.state() != State::Dead);
    if ok { 0 } else { err::INVALID_THREADID }
}

// ---- File handles ----

/// DosSetMaxFH(cFH): at least 20 and not below the handles in use.
pub fn dos_set_max_fh(c: &mut Ctx) -> u32 {
    let n = c.arg(0);
    let used = c.process.handles.lock().len() as u32;
    if n < 20 || n < used || n > 32768 {
        return err::INVALID_PARAMETER;
    }
    c.process.max_fh.store(n, Ordering::Relaxed);
    0
}

/// DosSetRelMaxFH(pcbReqCount, pcbCurMaxFH): change the limit by a signed
/// amount (0 = query); never below what is in use.
pub fn dos_set_rel_max_fh(c: &mut Ctx) -> u32 {
    let (preq, pmax) = (c.arg(0), c.arg(1));
    let delta = c.read_u32(preq).unwrap_or(0) as i32;
    let cur = c.process.max_fh.load(Ordering::Relaxed) as i64;
    let used = c.process.handles.lock().len() as i64;
    let want = (cur + delta as i64).clamp(used.max(20), 32768);
    c.process.max_fh.store(want as u32, Ordering::Relaxed);
    c.write_u32(pmax, want as u32);
    0
}

// ---- Modules ----

/// The module and object containing linear address `a`, for the calling
/// process: (handle, object index, offset, module name).
pub fn module_at(p: &Process, a: u32) -> Option<(u16, u32, u32, String)> {
    let mut mods = crate::loader::all_modules();
    mods.insert(0, p.exe.clone());
    for m in mods {
        for (i, o) in m.objects.iter().enumerate() {
            if o.size != 0 && a >= o.base && a - o.base < o.size {
                return Some((m.hmte, i as u32, a - o.base, m.name.clone()));
            }
        }
    }
    None
}

/// DosQueryModFromEIP(phMod, pObjNum, cbBuf, pBuf, pOffset, Address)
pub fn dos_query_mod_from_eip(c: &mut Ctx) -> u32 {
    let (phmod, pobj, cb, pbuf, poff, addr) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4), c.arg(5));
    let Some((h, obj, off, name)) = module_at(&c.process, addr) else { return err::INVALID_HANDLE };
    if name.len() + 1 > cb as usize {
        return ERROR_BAD_LENGTH;
    }
    c.write_u32(phmod, h as u32);
    c.write_u32(pobj, obj);
    c.write_u32(poff, off);
    c.write_str(pbuf, &name);
    0
}

/// 16-bit DosQueryModFromCS(sel, pQMResult {hmod, segnum, name[...]}).
pub fn dos16_query_mod_from_cs(c: &mut Ctx) -> u32 {
    let (sel, out) = (c.arg(0) as u16, c.ptr(1));
    let base = crate::vm::desc_base(c.space().descriptor(sel));
    let Some((h, obj, _, name)) = module_at(&c.process, base) else { return err::INVALID_HANDLE };
    c.write_u16(out, h);
    c.write_u16(out + 2, obj as u16 + 1);
    c.write_str(out + 4, &name);
    0
}

// ---- Dumps and pop-ups ----

static DUMPS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// DosDumpProcess(flag, drive, pid): DDP_DISABLEPROCDUMP (0),
/// DDP_ENABLEPROCDUMP (1) or DDP_PERFORMPROCDUMP (2). A dump is a text
/// file \PDUMP.nnn on the drive (rs2's own format: modules, threads and
/// their registers), not IBM's binary format.
pub fn dos_dump_process(c: &mut Ctx) -> u32 {
    let (flag, drive, pid) = (c.arg(0), c.arg(1) as u8, c.arg(2));
    match flag {
        0 | 1 => {
            DUMPS.store(flag, Ordering::Relaxed);
            0
        }
        2 => {
            let target = if pid == 0 { Some(c.process.clone()) } else { process::by_pid(pid) };
            let Some(p) = target.filter(|p| live(p)) else { return err::INVALID_PROCID };
            let d = match drive {
                b'a'..=b'z' => drive - b'a',
                b'A'..=b'Z' => drive - b'A',
                0 => 2,
                _ => return err::INVALID_DRIVE,
            };
            write_dump(&p, d)
        }
        _ => err::INVALID_PARAMETER,
    }
}

fn write_dump(p: &Arc<Process>, drive: u8) -> u32 {
    use core::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "rs2 process dump: process {} ({}), parent {}", p.pid, p.exe.path, p.ppid);
    let _ = writeln!(s, "modules:");
    for m in core::iter::once(p.exe.clone()).chain(p.loaded.lock().iter().map(|(m, _)| m.clone())) {
        let _ = writeln!(s, "  {:04x} {}", m.hmte, m.path);
        for (i, o) in m.objects.iter().enumerate() {
            let _ = writeln!(s, "    object {} {:08x} size {:08x} flags {:x}", i + 1, o.base, o.size, o.prot);
        }
    }
    let _ = writeln!(s, "threads:");
    for t in p.threads.lock().iter().filter(|t| t.state() != State::Dead) {
        let f = t.user_frame();
        let _ = writeln!(
            s,
            "  thread {} state {:?} CS:EIP={:04x}:{:08x} SS:ESP={:04x}:{:08x} EAX={:08x} EBX={:08x} ECX={:08x} EDX={:08x} ESI={:08x} EDI={:08x} EBP={:08x}",
            t.tid, t.state(), f.cs, f.rip as u32, f.ss, f.rsp as u32, f.rax as u32, f.rbx as u32, f.rcx as u32,
            f.rdx as u32, f.rsi as u32, f.rdi as u32, f.rbp as u32
        );
    }
    let Some(fs) = crate::fs::drive(drive) else { return err::INVALID_DRIVE };
    let name = (0..1000).map(|n| alloc::format!("PDUMP.{n:03}")).find(|n| fs.stat(n).is_err());
    let Some(name) = name else { return err::CANNOT_MAKE };
    match fs.create(&name, 0) {
        Ok(f) => match f.write(0, s.as_bytes()) {
            Ok(_) => {
                crate::println!("process {} dumped to {}:\\{}", p.pid, (b'A' + drive) as char, name);
                0
            }
            Err(e) => e,
        },
        Err(e) => e,
    }
}

/// DosSuppressPopUps(flag, drive): hard error and trap pop-ups are only
/// logged on rs2.
pub fn dos_suppress_popups(c: &mut Ctx) -> u32 {
    if c.arg(0) > 3 {
        return err::INVALID_PARAMETER;
    }
    0
}
