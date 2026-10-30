//! DosDebug: one process debugging another (IBM's task/dbg*.c).
//!
//! A program started with the trace option (DosStartSession TraceOpt,
//! DosExecPgm EXEC_TRACE) is a debuggee: its thread 1 waits before its
//! first instruction until a debugger connects (DBG_C_Connect) and lets it
//! run. From then on the debuggee's threads stop at events (breakpoints,
//! single steps, exceptions, module loads, thread starts) and post a
//! notification; the whole process stops with them. The debugger collects
//! one notification per DosDebug call (DBG_C_Go, DBG_C_SStep, DBG_C_Stop,
//! DBG_C_Continue) and, while the debuggee is stopped, reads and writes its
//! memory and registers.
//!
//! As in IBM's kernel:
//! * DosDebug clears Cmd before a command runs; a notification puts its
//!   code there and fills the uDB's registers with those of the thread that
//!   posted it (`DbgCall`).
//! * A breakpoint is reported with EIP backed up to the INT 3 (`DbgTrap`),
//!   as a pre-first-chance XCPT_BREAKPOINT exception; a single step as a
//!   pre-first-chance XCPT_SINGLE_STEP at the next instruction.
//! * Other exceptions are reported at first chance with the report and
//!   context records on the debuggee's stack (Buffer, Len), and at last
//!   chance before the default action.
//! * The debugger's verdict for an exception comes with DBG_C_Continue:
//!   XCPT_CONTINUE_SEARCH passes it on to the program's handlers,
//!   XCPT_CONTINUE_EXECUTION resumes at the (possibly rewritten) registers,
//!   XCPT_CONTINUE_STOP keeps the debuggee stopped and resumes it so at the
//!   next DBG_C_Go (`DbgContinue`).
//! * Module loads: DBG_C_Connect has the program's module reported first;
//!   each DLL follows as the debuggee runs on (one per DBG_C_Go), before its
//!   initialisation routine, as do DLLs loaded later (DosLoadModule).

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::exceptions::xcpt;
use super::{err, Ctx};
use crate::arch::idt::{TrapFrame, API_VECTOR};
use crate::loader::{Format, Module};
use crate::sync::SpinLock;
use crate::task::thread::{State, Thread};
use crate::task::{process, sched, Process, WaitQueue};
use crate::vm;

/// DosDebug commands (Toolkit bsedos.h DBG_C_*).
mod cmd {
    pub const NULL: i32 = 0;
    pub const READ_MEM_I: i32 = 1;
    pub const READ_MEM_D: i32 = 2;
    pub const READ_REG: i32 = 3;
    pub const WRITE_MEM_I: i32 = 4;
    pub const WRITE_MEM_D: i32 = 5;
    pub const WRITE_REG: i32 = 6;
    pub const GO: i32 = 7;
    pub const TERM: i32 = 8;
    pub const SSTEP: i32 = 9;
    pub const STOP: i32 = 10;
    pub const FREEZE: i32 = 11;
    pub const RESUME: i32 = 12;
    pub const NUM_TO_ADDR: i32 = 13;
    pub const READ_CO_REGS: i32 = 14;
    pub const WRITE_CO_REGS: i32 = 15;
    pub const THRD_STAT: i32 = 17;
    pub const MAP_RO_ALIAS: i32 = 18;
    pub const MAP_RW_ALIAS: i32 = 19;
    pub const UNMAP_ALIAS: i32 = 20;
    pub const CONNECT: i32 = 21;
    pub const READ_MEM_BUF: i32 = 22;
    pub const WRITE_MEM_BUF: i32 = 23;
    pub const CONTINUE: i32 = 27;
    pub const ADDR_TO_OBJECT: i32 = 28;
    pub const LIN_TO_SEL: i32 = 30;
    pub const SEL_TO_LIN: i32 = 31;
}

/// Notifications (DBG_N_*).
mod note {
    pub const SUCCESS: i32 = 0;
    pub const ERROR: i32 = -1;
    pub const PROC_TERM: i32 = -6;
    pub const EXCEPTION: i32 = -7;
    pub const MODULE_LOAD: i32 = -8;
    pub const THREAD_TERM: i32 = -10;
    pub const ASYNC_STOP: i32 = -11;
    pub const ALIAS_FREE: i32 = -13;
    pub const THREAD_CREATE: i32 = -15;
    pub const MODULE_FREE: i32 = -16;
}

/// Exception chances (DBG_X_*), in Value of DBG_N_Exception.
const PRE_FIRST_CHANCE: u32 = 0;
const FIRST_CHANCE: u32 = 1;
const LAST_CHANCE: u32 = 2;
const STACK_INVALID: u32 = 3;

const CONTINUE_STOP: u32 = 0x0071_6668;
const DBG_L_386: u32 = 1;
const DBG_CO_387: u32 = 1;
const DBG_LEN_387: u32 = 108;
const DBG_O_OBJMTE: u32 = 0x1000_0000;

const TF: u64 = 0x100;

/// uDB field offsets.
mod udb {
    pub const PID: usize = 0x00;
    pub const TID: usize = 0x04;
    pub const CMD: usize = 0x08;
    pub const VALUE: usize = 0x0c;
    pub const ADDR: usize = 0x10;
    pub const BUFFER: usize = 0x14;
    pub const LEN: usize = 0x18;
    pub const INDEX: usize = 0x1c;
    pub const MTE: usize = 0x20;
    pub const EAX: usize = 0x24;
    pub const ECX: usize = 0x28;
    pub const EDX: usize = 0x2c;
    pub const EBX: usize = 0x30;
    pub const ESP: usize = 0x34;
    pub const EBP: usize = 0x38;
    pub const ESI: usize = 0x3c;
    pub const EDI: usize = 0x40;
    pub const EFLAGS: usize = 0x44;
    pub const EIP: usize = 0x48;
    /// Segment blocks: limit, base, access byte, attributes, selector.
    pub const CS: usize = 0x4c;
    pub const DS: usize = 0x58;
    pub const ES: usize = 0x64;
    pub const FS: usize = 0x70;
    pub const GS: usize = 0x7c;
    pub const SS: usize = 0x88;
    pub const SIZE: usize = 0x94;
}

struct Udb([u8; udb::SIZE]);

impl Udb {
    fn get(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    fn set(&mut self, o: usize, v: u32) {
        self.0[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn sel(&self, block: usize) -> u16 {
        u16::from_le_bytes([self.0[block + 10], self.0[block + 11]])
    }
    fn error(&mut self, code: u32) {
        self.set(udb::CMD, note::ERROR as u32);
        self.set(udb::VALUE, code);
    }
}

/// A notification waiting for the debugger. `mask` says which uDB fields
/// it sets (IBM's DbgNotify flags): 1 Value, 2 Addr, 4 Buffer, 8 Len,
/// 0x10 Index.
#[derive(Clone, Copy)]
struct Event {
    tid: u32,
    note: i32,
    mask: u8,
    value: u32,
    addr: u32,
    buffer: u32,
    len: u32,
    index: u32,
}

impl Event {
    fn new(tid: u32, note: i32) -> Event {
        Event { tid, note, mask: 0, value: 0, addr: 0, buffer: 0, len: 0, index: 0 }
    }
    fn exception(tid: u32, chance: u32, addr: u32, buffer: u32, len: u32) -> Event {
        Event { mask: 0xf, value: chance, addr, buffer, len, ..Event::new(tid, note::EXCEPTION) }
    }
    fn module(tid: u32, note: i32, hmte: u16) -> Event {
        Event { mask: 3, value: hmte as u32, ..Event::new(tid, note) }
    }
}

struct Debuggee {
    pid: u32,
    /// The process that started it traced.
    tracer: u32,
    /// The connected debugger, 0 until DBG_C_Connect.
    debugger: u32,
    /// When it was started (ticks): it waits ten minutes for a debugger.
    since: u64,
    /// The debugger let it run (DBG_C_Go, DBG_C_SStep).
    running: bool,
    /// Single stepping this thread: only it runs.
    step: u32,
    /// Notifications not yet collected.
    events: VecDeque<Event>,
    /// Threads stopped at an exception and the debugger's verdict.
    verdicts: Vec<(u32, u32)>,
    /// DBG_C_Freeze'd threads.
    frozen: Vec<u32>,
    /// Threads held in the kernel by the debugger.
    parked: Vec<u32>,
    /// Threads the debugger knows (thread 1, and those it was told of).
    announced: Vec<u32>,
    /// Modules reported with DBG_N_ModuleLoad.
    modules: Vec<u16>,
    /// The thread of the last notification (Tid 0 means it).
    last_tid: u32,
    /// DBG_C_Term: everything runs to its end.
    term: bool,
    /// The process has ended (DBG_N_ProcTerm posted).
    ended: bool,
}

impl Debuggee {
    fn released(&self, tid: u32) -> bool {
        self.term
            || (self.debugger != 0
                && self.running
                && (self.step == 0 || self.step == tid)
                && !self.frozen.contains(&tid)
                && !self.events.iter().any(|e| e.tid == tid))
    }
}

static DEBUGGEES: SpinLock<Vec<Debuggee>> = SpinLock::new(Vec::new());
/// Both sides wait here: debuggee threads for the debugger to let them
/// run, the debugger for a notification.
static WAIT: WaitQueue = WaitQueue::new();

/// A program was started with the trace option (before its thread 1 runs).
pub fn traced(p: &Arc<Process>) {
    DEBUGGEES.lock().push(Debuggee {
        pid: p.pid,
        tracer: p.ppid,
        debugger: 0,
        since: crate::irq::ticks(),
        running: false,
        step: 0,
        events: VecDeque::new(),
        verdicts: Vec::new(),
        frozen: Vec::new(),
        parked: Vec::new(),
        announced: alloc::vec![1],
        modules: Vec::new(),
        last_tid: 1,
        term: false,
        ended: false,
    });
}

fn with<R>(pid: u32, f: impl FnOnce(&mut Debuggee) -> R) -> Option<R> {
    DEBUGGEES.lock().iter_mut().find(|d| d.pid == pid).map(f)
}

/// The modules a process uses: its program first, then the DLLs in the
/// order their initialisation runs (kernel-implemented ones have no file
/// and are left out).
fn modules_of(p: &Process) -> Vec<Arc<Module>> {
    fn visit(m: &Arc<Module>, seen: &mut Vec<u16>, out: &mut Vec<Arc<Module>>) {
        if seen.contains(&m.hmte) {
            return;
        }
        seen.push(m.hmte);
        for imp in &m.imports {
            visit(imp, seen, out);
        }
        if m.format != Format::Virtual {
            out.push(m.clone());
        }
    }
    let mut out = alloc::vec![p.exe.clone()];
    let mut seen = alloc::vec![p.exe.hmte];
    for imp in &p.exe.imports {
        visit(imp, &mut seen, &mut out);
    }
    let loaded: Vec<Arc<Module>> = p.loaded.lock().iter().map(|(m, _)| m.clone()).collect();
    for m in &loaded {
        visit(m, &mut seen, &mut out);
    }
    out
}

fn code_linear(p: &Process, f: &TrapFrame) -> u32 {
    let space = p.space.lock();
    space.linear(f.cs as u16, f.rip as u32).unwrap_or(f.rip as u32)
}

/// With several CPUs, make the process's threads that run user code right
/// now enter the kernel (they are held on their way back).
fn quiesce(p: &Process) {
    if crate::smp::online_count() < 2 {
        return;
    }
    let running = || p.threads.lock().iter().any(|t| t.state() == State::Running && t.on_cpu.load(Ordering::Acquire));
    if !running() {
        return;
    }
    let me = crate::smp::this().bit();
    for cpu in crate::smp::online() {
        if cpu.bit() != me {
            crate::smp::kick(cpu);
        }
    }
    for _ in 0..200 {
        if !running() {
            return;
        }
        sched::sleep_ms(1);
    }
}

// ---- The debuggee's side ----

/// Wait (a second at most) until the debugger lets this thread run. Ends a
/// debuggee whose debugger is gone or never came.
fn park(t: &Arc<Thread>, p: &Arc<Process>) {
    let (pid, tid) = (p.pid, t.tid);
    let _ = with(pid, |d| {
        if !d.parked.contains(&tid) {
            d.parked.push(tid);
        }
    });
    let deadline = crate::irq::ticks() + crate::irq::HZ;
    WAIT.wait_uninterruptible_until(deadline, || {
        t.exit_requested() || with(pid, |d| d.released(tid)).unwrap_or(true)
    });
    let _ = with(pid, |d| d.parked.retain(|&x| x != tid));
    let (debugger, tracer, since) = with(pid, |d| (d.debugger, d.tracer, d.since)).unwrap_or((0, 0, 0));
    let gone = if debugger != 0 {
        process::by_pid(debugger).is_none_or(|q| q.exit_info().is_some())
    } else {
        // IBM's DbgHandler waits ten minutes for DBG_C_Connect.
        process::by_pid(tracer).is_none_or(|q| q.exit_info().is_some())
            || crate::irq::ticks().saturating_sub(since) > 600 * crate::irq::HZ
    };
    if gone && with(pid, |d| d.debugger == debugger && !d.term).unwrap_or(false) {
        DEBUGGEES.lock().retain(|d| d.pid != pid);
        p.debugged.store(false, Ordering::Relaxed);
        p.exit(3, 0);
    }
}

/// Post a notification and stop until the debugger lets the thread go on.
/// Returns the debugger's verdict for an exception.
fn stop(t: &Arc<Thread>, p: &Arc<Process>, f: &mut TrapFrame, ev: Event) -> u32 {
    let (pid, tid) = (p.pid, t.tid);
    let posted = with(pid, |d| {
        if d.debugger == 0 || d.term {
            return false;
        }
        if d.step == tid {
            f.rflags &= !TF;
            d.step = 0;
        }
        d.running = false;
        d.verdicts.retain(|v| v.0 != tid);
        d.events.push_back(ev);
        true
    });
    if posted != Some(true) {
        return xcpt::CONTINUE_SEARCH;
    }
    WAIT.wake_all();
    loop {
        if t.exit_requested() || with(pid, |d| d.released(tid)).unwrap_or(true) {
            break;
        }
        park(t, p);
    }
    with(pid, |d| {
        let v = d.verdicts.iter().find(|v| v.0 == tid).map_or(xcpt::CONTINUE_EXECUTION, |v| v.1);
        d.verdicts.retain(|v| v.0 != tid);
        v
    })
    .unwrap_or(xcpt::CONTINUE_SEARCH)
}

/// Called on every return to user mode of a debuggee's thread (and before
/// a new thread's first instruction): reports what the debugger has not
/// been told yet and holds the thread while the debuggee is stopped.
pub fn hold(t: &Arc<Thread>, p: &Arc<Process>, f: &mut TrapFrame) {
    if !p.debugged.load(Ordering::Relaxed) {
        return;
    }
    let tid = t.tid;
    loop {
        if t.exit_requested() {
            return;
        }
        let mods = modules_of(p);
        let ev = with(p.pid, |d| {
            if d.debugger == 0 || d.term {
                return None;
            }
            if !d.announced.contains(&tid) {
                d.announced.push(tid);
                return Some(Event::new(tid, note::THREAD_CREATE));
            }
            if let Some(m) = mods.iter().find(|m| !d.modules.contains(&m.hmte)) {
                d.modules.push(m.hmte);
                return Some(Event::module(tid, note::MODULE_LOAD, m.hmte));
            }
            if let Some(i) = d.modules.iter().position(|h| !mods.iter().any(|m| m.hmte == *h)) {
                let h = d.modules.remove(i);
                return Some(Event::module(tid, note::MODULE_FREE, h));
            }
            // A single step over the API gate's INT: the call is the
            // instruction (the CPU would trap only after the next one).
            if d.step == tid && f.vector == API_VECTOR as u64 && f.rflags & TF != 0 {
                return Some(Event::exception(tid, PRE_FIRST_CHANCE, 0, xcpt::SINGLE_STEP, 0));
            }
            None
        });
        let Some(ev) = ev else {
            p.debugged.store(false, Ordering::Relaxed);
            return;
        };
        match ev {
            Some(mut e) => {
                if e.note == note::EXCEPTION {
                    e.addr = code_linear(p, f);
                }
                stop(t, p, f, e);
            }
            None => {
                if with(p.pid, |d| d.released(tid)).unwrap_or(true) {
                    return;
                }
                park(t, p);
            }
        }
    }
}

/// The first return to user mode of a new thread (sched.rs
/// thread_trampoline): in a debuggee it goes through the checks of
/// every other return, so it can be held before its first instruction.
#[unsafe(no_mangle)]
pub extern "C" fn first_user_return(f: &mut TrapFrame) {
    let debugged = sched::current().process().is_some_and(|p| p.debugged.load(Ordering::Relaxed));
    if !debugged {
        return;
    }
    crate::smp::bkl_acquire();
    if crate::task::before_user_return(f) {
        crate::task::sched::exit_current();
    }
    crate::smp::bkl_release();
}

fn connected(p: &Process) -> bool {
    p.debugged.load(Ordering::Relaxed) && with(p.pid, |d| d.debugger != 0 && !d.term).unwrap_or(false)
}

/// A breakpoint (INT 3) or single step trap in a debuggee. True: handled
/// (the thread goes on as the debugger left it); false: deliver it as an
/// exception to the program.
pub fn trap(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) -> bool {
    if (f.vector != 1 && f.vector != 3) || !connected(p) {
        return false;
    }
    let lin = code_linear(p, f);
    let (code, addr) = if f.vector == 3 {
        // Back to the INT 3 (CC, or CD 03).
        let back = {
            let mut sp = p.space.lock();
            let mut b = [0u8; 2];
            if sp.read_bytes(lin.wrapping_sub(1), &mut b[1..]) && b[1] == 0xcc {
                1
            } else if sp.read_bytes(lin.wrapping_sub(2), &mut b) && b == [0xcd, 0x03] {
                2
            } else {
                0
            }
        };
        f.rip = f.rip.wrapping_sub(back);
        (xcpt::BREAKPOINT, lin.wrapping_sub(back as u32))
    } else {
        f.rflags &= !TF;
        (xcpt::SINGLE_STEP, lin)
    };
    stop(t, p, f, Event::exception(t.tid, PRE_FIRST_CHANCE, addr, code, 0)) != xcpt::CONTINUE_SEARCH
}

/// First (`last` false) or last chance notification of an exception. True:
/// the debugger handled it (the thread resumes at its registers).
pub fn exception(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>, code: u32, info: &[u32], last: bool) -> bool {
    if !connected(p) {
        return false;
    }
    let lin = code_linear(p, f);
    let ev = match super::exceptions::records(p, t, f, code, info, lin) {
        Some((report, context)) => {
            Event::exception(t.tid, if last { LAST_CHANCE } else { FIRST_CHANCE }, lin, report, context)
        }
        None => Event::exception(t.tid, STACK_INVALID, lin, code, 0),
    };
    stop(t, p, f, ev) != xcpt::CONTINUE_SEARCH
}

/// A thread of the process ended (others go on).
pub fn thread_ended(p: &Process, tid: u32) {
    if !p.debugged.load(Ordering::Relaxed) {
        return;
    }
    let posted = with(p.pid, |d| {
        d.announced.retain(|&x| x != tid);
        d.frozen.retain(|&x| x != tid);
        if d.debugger == 0 || d.term {
            return false;
        }
        d.events.push_back(Event { mask: 3, ..Event::new(tid, note::THREAD_TERM) });
        true
    });
    if posted == Some(true) {
        WAIT.wake_all();
    }
}

/// A process ended: a debuggee reports DBG_N_ProcTerm; a debugger's
/// debuggees end with it (IBM's DbgKillDebuggees).
pub fn process_ended(p: &Process) {
    let code = p.exit_info().map_or(0, |e| e.code);
    let orphans: Vec<u32> = {
        let mut g = DEBUGGEES.lock();
        if let Some(d) = g.iter_mut().find(|d| d.pid == p.pid) {
            d.ended = true;
            if d.debugger != 0 {
                d.events.push_back(Event { mask: 0x13, value: code, ..Event::new(d.last_tid, note::PROC_TERM) });
            }
        }
        g.retain(|d| !(d.pid == p.pid && d.debugger == 0));
        let mine: Vec<u32> = g.iter().filter(|d| d.debugger == p.pid || (d.debugger == 0 && d.tracer == p.pid)).map(|d| d.pid).collect();
        g.retain(|d| !mine.contains(&d.pid));
        mine
    };
    ALIASES.lock().retain(|a| a.debugger != p.pid);
    WAIT.wake_all();
    for pid in orphans {
        if let Some(q) = process::by_pid(pid) {
            q.debugged.store(false, Ordering::Relaxed);
            q.exit(3, 0);
        }
    }
}

// ---- The debugger's side ----

fn thread_of(p: &Process, tid: u32) -> Option<Arc<Thread>> {
    p.threads.lock().iter().find(|t| t.tid == tid && t.state() != State::Dead).cloned()
}

/// Selector block: limit, base, access byte, attributes (byte 6 of the
/// descriptor without the limit bits), selector.
fn put_seg(u: &mut Udb, block: usize, space: &vm::AddressSpace, sel: u16, base: Option<u32>) {
    let (lim, b, acc, atr) = if sel == 0 {
        (0, 0, 0, 0)
    } else {
        let d = space.descriptor(sel);
        let base = base.unwrap_or(vm::desc_base(d));
        (vm::desc_limit(d), base, (d >> 40) as u8, ((d >> 48) as u8) & 0xf0)
    };
    u.set(block, lim);
    u.set(block + 4, b);
    u.0[block + 8] = acc;
    u.0[block + 9] = atr;
    u.0[block + 10..block + 12].copy_from_slice(&sel.to_le_bytes());
}

fn read_regs(p: &Process, t: &Thread, u: &mut Udb) {
    let f = *t.user_frame();
    u.set(udb::TID, t.tid);
    u.set(udb::MTE, p.exe.hmte as u32);
    for (o, v) in [
        (udb::EAX, f.rax),
        (udb::ECX, f.rcx),
        (udb::EDX, f.rdx),
        (udb::EBX, f.rbx),
        (udb::ESP, f.rsp),
        (udb::EBP, f.rbp),
        (udb::ESI, f.rsi),
        (udb::EDI, f.rdi),
        (udb::EFLAGS, f.rflags),
        (udb::EIP, f.rip),
    ] {
        u.set(o, v as u32);
    }
    let space = p.space.lock();
    put_seg(u, udb::CS, &space, f.cs as u16, None);
    put_seg(u, udb::DS, &space, f.ds as u16, None);
    put_seg(u, udb::ES, &space, f.es as u16, None);
    // FS of 32-bit code is the thread's TIB (one GDT selector, its base
    // switched with the thread).
    let fs_base = (f.fs as u16 == crate::arch::gdt::TIB_SEL).then(|| t.tib());
    put_seg(u, udb::FS, &space, f.fs as u16, fs_base);
    put_seg(u, udb::GS, &space, f.gs as u16, None);
    put_seg(u, udb::SS, &space, f.ss as u16, None);
}

/// A selector the thread may run with: CS a present DPL 3 code segment
/// (not 64-bit), SS a writable DPL 3 data segment, the others null or
/// loadable data/readable code.
fn selectors_ok(space: &vm::AddressSpace, u: &Udb) -> bool {
    let d = |s: u16| space.descriptor(s);
    let present = |d: u64| d & (1 << 47) != 0 && d & (1 << 44) != 0 && (d >> 45) & 3 == 3;
    let code = |d: u64| d & (1 << 43) != 0;
    let cs = u.sel(udb::CS);
    let ss = u.sel(udb::SS);
    let cs_ok = cs & 3 == 3 && present(d(cs)) && code(d(cs)) && d(cs) & (1 << 53) == 0;
    let ss_ok = ss & 3 == 3 && present(d(ss)) && !code(d(ss)) && d(ss) & (1 << 41) != 0;
    let data_ok = [udb::DS, udb::ES, udb::FS, udb::GS].iter().all(|&b| {
        let s = u.sel(b);
        s < 4 || (present(d(s)) && (!code(d(s)) || d(s) & (1 << 41) != 0))
    });
    cs_ok && ss_ok && data_ok
}

fn write_regs(p: &Process, t: &Thread, u: &Udb) -> bool {
    if !selectors_ok(&p.space.lock(), u) {
        return false;
    }
    let f = t.user_frame();
    f.rax = u.get(udb::EAX) as u64;
    f.rcx = u.get(udb::ECX) as u64;
    f.rdx = u.get(udb::EDX) as u64;
    f.rbx = u.get(udb::EBX) as u64;
    f.rsp = u.get(udb::ESP) as u64;
    f.rbp = u.get(udb::EBP) as u64;
    f.rsi = u.get(udb::ESI) as u64;
    f.rdi = u.get(udb::EDI) as u64;
    f.rip = u.get(udb::EIP) as u64;
    // The arithmetic flags, TF and DF; interrupts stay enabled.
    f.rflags = (f.rflags & !0xdd5) | (u.get(udb::EFLAGS) as u64 & 0xdd5) | 0x202;
    f.cs = u.sel(udb::CS) as u64;
    f.ds = u.sel(udb::DS) as u64;
    f.es = u.sel(udb::ES) as u64;
    f.fs = u.sel(udb::FS) as u64;
    f.gs = u.sel(udb::GS) as u64;
    f.ss = u.sel(udb::SS) as u64;
    true
}

/// FXSAVE image to the 108-byte FSAVE format (32-bit protected mode).
fn fsave_from_fxsave(fx: &[u8; 512]) -> [u8; 108] {
    let mut o = [0u8; 108];
    let w = |o: &mut [u8; 108], at: usize, v: u32| o[at..at + 4].copy_from_slice(&v.to_le_bytes());
    let fcw = u16::from_le_bytes([fx[0], fx[1]]);
    let fsw = u16::from_le_bytes([fx[2], fx[3]]);
    let top = (fsw >> 11) & 7;
    let mut ftw = 0u16;
    for phys in 0..8u16 {
        let tag = if fx[4] & (1 << phys) == 0 {
            3
        } else {
            let st = ((phys + 8 - top) & 7) as usize;
            let r = &fx[32 + st * 16..32 + st * 16 + 10];
            let exp = u16::from_le_bytes([r[8], r[9]]) & 0x7fff;
            let mant = u64::from_le_bytes(r[..8].try_into().unwrap());
            match (exp, mant) {
                (0x7fff, _) => 2,
                (0, 0) => 1,
                (0, _) => 2,
                _ if mant >> 63 == 0 => 2,
                _ => 0,
            }
        };
        ftw |= tag << (phys * 2);
    }
    w(&mut o, 0, 0xffff_0000 | fcw as u32);
    w(&mut o, 4, 0xffff_0000 | fsw as u32);
    w(&mut o, 8, 0xffff_0000 | ftw as u32);
    w(&mut o, 12, u32::from_le_bytes(fx[8..12].try_into().unwrap()));
    let fop = u16::from_le_bytes([fx[6], fx[7]]) as u32 & 0x7ff;
    w(&mut o, 16, u16::from_le_bytes([fx[12], fx[13]]) as u32 | fop << 16);
    w(&mut o, 20, u32::from_le_bytes(fx[16..20].try_into().unwrap()));
    w(&mut o, 24, 0xffff_0000 | u16::from_le_bytes([fx[20], fx[21]]) as u32);
    for i in 0..8 {
        o[28 + i * 10..38 + i * 10].copy_from_slice(&fx[32 + i * 16..42 + i * 16]);
    }
    o
}

fn fxsave_from_fsave(fx: &mut [u8; 512], s: &[u8]) {
    let r = |at: usize| u32::from_le_bytes(s[at..at + 4].try_into().unwrap());
    fx[0..2].copy_from_slice(&(r(0) as u16).to_le_bytes());
    fx[2..4].copy_from_slice(&(r(4) as u16).to_le_bytes());
    let ftw = r(8);
    fx[4] = (0..8).fold(0u8, |a, i| if (ftw >> (i * 2)) & 3 != 3 { a | 1 << i } else { a });
    fx[6..8].copy_from_slice(&((r(16) >> 16) as u16 & 0x7ff).to_le_bytes());
    fx[8..12].copy_from_slice(&r(12).to_le_bytes());
    fx[12..14].copy_from_slice(&(r(16) as u16).to_le_bytes());
    fx[16..20].copy_from_slice(&r(20).to_le_bytes());
    fx[20..22].copy_from_slice(&(r(24) as u16).to_le_bytes());
    for i in 0..8 {
        fx[32 + i * 16..42 + i * 16].copy_from_slice(&s[28 + i * 10..38 + i * 10]);
        fx[42 + i * 16..48 + i * 16].fill(0);
    }
}

/// Read the debuggee's memory (any committed page, whatever its
/// protection).
fn peek(p: &Process, addr: u32, out: &mut [u8]) -> bool {
    p.space.lock().read_bytes(addr, out)
}

fn poke(p: &Process, addr: u32, data: &[u8]) -> bool {
    p.space.lock().debug_write(addr, data)
}

/// Hand the notification to the debugger: Cmd and the fields it sets, then
/// the registers of the thread that posted it.
fn deliver(p: Option<&Process>, u: &mut Udb, pid: u32, e: Event) {
    u.set(udb::CMD, e.note as u32);
    u.set(udb::PID, pid);
    u.set(udb::TID, e.tid);
    for (bit, o, v) in [
        (1, udb::VALUE, e.value),
        (2, udb::ADDR, e.addr),
        (4, udb::BUFFER, e.buffer),
        (8, udb::LEN, e.len),
        (0x10, udb::INDEX, e.index),
    ] {
        if e.mask & bit != 0 {
            u.set(o, v);
        }
    }
    if let Some(p) = p {
        if let Some(t) = thread_of(p, e.tid) {
            read_regs(p, &t, u);
        }
    }
}

/// The next notification, if one is waiting.
fn next_event(pid: u32) -> Option<Event> {
    with(pid, |d| {
        let e = d.events.pop_front()?;
        d.last_tid = e.tid;
        Some(e)
    })
    .flatten()
}

/// Let the debuggee run (all threads, or only `step`) until it posts a
/// notification, and return that.
fn run(_c: &Ctx, p: &Arc<Process>, u: &mut Udb, step: u32) {
    let pid = p.pid;
    if let Some(e) = next_event(pid) {
        deliver(Some(p), u, pid, e);
        return;
    }
    if with(pid, |d| d.ended).unwrap_or(true) {
        u.error(err::INVALID_PROCID);
        return;
    }
    if step != 0 {
        let Some(t) = thread_of(p, step) else {
            u.error(err::INVALID_THREADID);
            return;
        };
        t.user_frame().rflags |= TF;
    }
    let _ = with(pid, |d| {
        d.running = true;
        d.step = step;
    });
    WAIT.wake_all();
    let ready = || with(pid, |d| !d.events.is_empty() || !d.running).unwrap_or(true);
    let ok = WAIT.wait_until(None, ready);
    if !ok {
        // Interrupted (a signal for the debugger, Ctrl+Break): the
        // debuggee stops.
        let _ = with(pid, |d| d.running = false);
    }
    quiesce(p);
    match next_event(pid) {
        Some(e) => deliver(Some(p), u, pid, e),
        None => {
            let tid = with(pid, |d| d.last_tid).unwrap_or(1);
            deliver(Some(p), u, pid, Event::new(tid, note::ASYNC_STOP));
        }
    }
}

/// The module and object containing a linear address.
fn object_at(p: &Process, addr: u32) -> Option<(Arc<Module>, usize)> {
    modules_of(p).into_iter().find_map(|m| {
        let i = m.objects.iter().position(|o| addr >= o.base && addr - o.base < o.size.max(1))?;
        Some((m, i))
    })
}

/// DosDebug(puDB)
pub fn dos_debug(c: &mut Ctx) -> u32 {
    let at = c.arg(0);
    let mut u = Udb([0; udb::SIZE]);
    if !c.read(at, &mut u.0) {
        return err::INVALID_ADDRESS;
    }
    let cmd = u.get(udb::CMD) as i32;
    u.set(udb::CMD, note::SUCCESS as u32);
    command(c, &mut u, cmd);
    if !c.write(at, &u.0) {
        return err::INVALID_ADDRESS;
    }
    0
}

fn command(c: &mut Ctx, u: &mut Udb, cmd: i32) {
    let me = c.process.pid;
    let pid = u.get(udb::PID);
    if cmd == cmd::NULL {
        return;
    }
    if cmd == cmd::CONNECT {
        if u.get(udb::VALUE) != DBG_L_386 || u.get(udb::TID) != 0 {
            u.error(err::INVALID_DATA);
            return;
        }
        let Some(p) = process::by_pid(pid) else {
            u.error(err::INVALID_PROCID);
            return;
        };
        let exe = p.exe.hmte;
        // A debugger reconnecting drops what it had of an ended session.
        DEBUGGEES.lock().retain(|d| !(d.debugger == me && d.ended));
        let ok = with(pid, |d| {
            if d.debugger != 0 || d.ended {
                return false;
            }
            d.debugger = me;
            d.modules.push(exe);
            d.events.push_back(Event::module(1, note::MODULE_LOAD, exe));
            true
        });
        if ok != Some(true) {
            u.error(err::INVALID_PROCID);
            return;
        }
        if let Some(t) = thread_of(&p, 1) {
            read_regs(&p, &t, u);
        }
        return;
    }
    // Every other command needs a debuggee of the caller's.
    if with(pid, |d| d.debugger == me) != Some(true) {
        u.error(err::INVALID_PROCID);
        return;
    }
    let p = process::by_pid(pid).filter(|p| p.debugged.load(Ordering::Relaxed));
    let tid_arg = u.get(udb::TID);
    let tid = if tid_arg == 0 { with(pid, |d| d.last_tid).unwrap_or(1) } else { tid_arg };
    // Commands that work on a live process.
    let live = |u: &mut Udb| -> Option<Arc<Process>> {
        if p.is_none() {
            u.error(err::INVALID_PROCID);
        }
        p.clone()
    };
    match cmd {
        cmd::GO => {
            let Some(p) = live(u) else {
                // The process ended: what it posted last is still there.
                if let Some(e) = next_event(pid) {
                    deliver(None, u, pid, e);
                }
                return;
            };
            run(c, &p, u, 0);
        }
        cmd::SSTEP => {
            let Some(p) = live(u) else { return };
            run(c, &p, u, tid);
        }
        cmd::STOP => {
            if let Some(e) = next_event(pid) {
                deliver(p.as_deref(), u, pid, e);
                return;
            }
            let was = with(pid, |d| core::mem::replace(&mut d.running, false)).unwrap_or(false);
            if let Some(p) = &p {
                quiesce(p);
            }
            WAIT.wake_all();
            let last = with(pid, |d| d.last_tid).unwrap_or(1);
            let e = Event::new(last, if was { note::ASYNC_STOP } else { note::SUCCESS });
            deliver(p.as_deref(), u, pid, e);
        }
        cmd::CONTINUE => {
            let v = u.get(udb::VALUE);
            if ![xcpt::CONTINUE_SEARCH, xcpt::CONTINUE_EXECUTION, CONTINUE_STOP].contains(&v) {
                u.error(err::INVALID_DATA);
                return;
            }
            let verdict = if v == CONTINUE_STOP { xcpt::CONTINUE_EXECUTION } else { v };
            let _ = with(pid, |d| {
                d.verdicts.retain(|x| x.0 != tid);
                d.verdicts.push((tid, verdict));
            });
            if v != CONTINUE_STOP {
                match &p {
                    Some(p) => run(c, p, u, 0),
                    None => {
                        if let Some(e) = next_event(pid) {
                            deliver(None, u, pid, e);
                        }
                    }
                }
            }
        }
        cmd::TERM => {
            let _ = with(pid, |d| {
                d.term = true;
                d.running = true;
            });
            WAIT.wake_all();
            if let Some(p) = &p {
                p.exit(3, 0);
                for _ in 0..500 {
                    if p.threads.lock().is_empty() {
                        break;
                    }
                    sched::sleep_ms(10);
                }
            }
            DEBUGGEES.lock().retain(|d| d.pid != pid);
        }
        cmd::READ_REG | cmd::WRITE_REG | cmd::THRD_STAT | cmd::READ_CO_REGS | cmd::WRITE_CO_REGS | cmd::FREEZE | cmd::RESUME => {
            let Some(p) = live(u) else { return };
            thread_command(c, &p, u, cmd, tid, tid_arg);
        }
        cmd::READ_MEM_I | cmd::READ_MEM_D => {
            let Some(p) = live(u) else { return };
            let mut b = [0u8; 2];
            if peek(&p, u.get(udb::ADDR), &mut b) {
                u.set(udb::VALUE, u16::from_le_bytes(b) as u32);
            } else {
                u.error(err::INVALID_ADDRESS);
            }
        }
        cmd::WRITE_MEM_I | cmd::WRITE_MEM_D => {
            let Some(p) = live(u) else { return };
            let v = u.get(udb::VALUE) as u16;
            if !poke(&p, u.get(udb::ADDR), &v.to_le_bytes()) {
                u.error(err::INVALID_ADDRESS);
            }
        }
        cmd::READ_MEM_BUF | cmd::WRITE_MEM_BUF => {
            let Some(p) = live(u) else { return };
            let (addr, buf, len) = (u.get(udb::ADDR), u.get(udb::BUFFER), u.get(udb::LEN));
            let mut done = 0u32;
            let mut chunk = alloc::vec![0u8; len.min(0x1_0000) as usize];
            while done < len {
                let n = (len - done).min(chunk.len() as u32) as usize;
                let ok = if cmd == cmd::READ_MEM_BUF {
                    peek(&p, addr + done, &mut chunk[..n]) && c.write(buf + done, &chunk[..n])
                } else {
                    c.read(buf + done, &mut chunk[..n]) && poke(&p, addr + done, &chunk[..n])
                };
                if !ok {
                    u.error(err::INVALID_ADDRESS);
                    return;
                }
                done += n as u32;
            }
        }
        cmd::NUM_TO_ADDR => {
            let n = u.get(udb::VALUE) as usize;
            let m = crate::loader::by_handle(u.get(udb::MTE) as u16);
            match m.as_ref().and_then(|m| m.objects.get(n.wrapping_sub(1))) {
                Some(o) => u.set(udb::ADDR, o.base),
                None => u.error(err::INVALID_PARAMETER),
            }
        }
        cmd::ADDR_TO_OBJECT => {
            let Some(p) = live(u) else { return };
            match object_at(&p, u.get(udb::ADDR)) {
                Some((m, i)) => {
                    let o = &m.objects[i];
                    u.set(udb::BUFFER, o.base);
                    u.set(udb::LEN, o.size);
                    u.set(udb::VALUE, o.prot | DBG_O_OBJMTE);
                    u.set(udb::MTE, m.hmte as u32);
                    u.set(udb::INDEX, i as u32 + 1);
                }
                None => u.error(err::INVALID_ADDRESS),
            }
        }
        cmd::SEL_TO_LIN => {
            let Some(p) = live(u) else { return };
            let (sel, off) = (u.get(udb::VALUE) as u16, u.get(udb::INDEX));
            let space = p.space.lock();
            let d = space.descriptor(sel);
            if sel < 4 || d & (1 << 47) == 0 {
                drop(space);
                u.error(err::INVALID_ADDRESS);
            } else {
                u.set(udb::ADDR, vm::desc_base(d).wrapping_add(off));
            }
        }
        cmd::LIN_TO_SEL => {
            let a = u.get(udb::ADDR);
            if a >= vm::TILED_TOP {
                u.error(err::INVALID_ADDRESS);
            } else {
                u.set(udb::VALUE, vm::tile_selector(a) as u32);
                u.set(udb::INDEX, a & 0xffff);
            }
        }
        cmd::MAP_RO_ALIAS | cmd::MAP_RW_ALIAS => {
            let Some(p) = live(u) else { return };
            map_alias(c, &p, u, cmd == cmd::MAP_RW_ALIAS);
        }
        cmd::UNMAP_ALIAS => unmap_alias(c, u),
        _ => u.error(err::INVALID_FUNCTION),
    }
}

fn thread_command(c: &Ctx, p: &Arc<Process>, u: &mut Udb, cmd: i32, tid: u32, tid_arg: u32) {
    let _ = c;
    if matches!(cmd, cmd::FREEZE | cmd::RESUME) && tid_arg == 0 {
        // Every thread.
        let all: Vec<u32> = p.threads.lock().iter().map(|t| t.tid).collect();
        let _ = with(p.pid, |d| {
            d.frozen.clear();
            if cmd == cmd::FREEZE {
                d.frozen = all;
            }
        });
        WAIT.wake_all();
        return;
    }
    let Some(t) = thread_of(p, tid) else {
        u.error(err::INVALID_THREADID);
        return;
    };
    match cmd {
        cmd::READ_REG => read_regs(p, &t, u),
        cmd::WRITE_REG => {
            if !write_regs(p, &t, u) {
                u.error(err::INVALID_ACCESS);
            }
        }
        cmd::THRD_STAT => {
            let frozen = with(p.pid, |d| (d.frozen.contains(&tid), d.parked.contains(&tid))).unwrap_or((false, false));
            let cs = p.critsec.lock().0;
            let state = if t.suspend.load(Ordering::Relaxed) > 0 {
                1
            } else if cs != 0 && cs != tid {
                3
            } else if t.state() == State::Blocked && !frozen.1 {
                2
            } else {
                0
            };
            let mut b = [frozen.0 as u8, state, 0, 0];
            b[2..4].copy_from_slice(&(t.priority() as u16).to_le_bytes());
            let n = u.get(udb::LEN).min(4) as usize;
            if !c.write(u.get(udb::BUFFER), &b[..n]) {
                u.error(err::INVALID_ADDRESS);
            }
        }
        cmd::READ_CO_REGS | cmd::WRITE_CO_REGS => {
            if u.get(udb::VALUE) != DBG_CO_387 || u.get(udb::LEN) < DBG_LEN_387 || u.get(udb::INDEX) != 0 {
                u.error(err::INVALID_PARAMETER);
                return;
            }
            // The thread is switched out: its saved FPU state is current.
            let fx = unsafe { &mut (*t.fpu.get()).0 };
            if cmd == cmd::READ_CO_REGS {
                let s = fsave_from_fxsave(fx);
                if !c.write(u.get(udb::BUFFER), &s) {
                    u.error(err::INVALID_ADDRESS);
                }
            } else {
                let mut s = [0u8; 108];
                if !c.read(u.get(udb::BUFFER), &mut s) {
                    u.error(err::INVALID_ADDRESS);
                    return;
                }
                fxsave_from_fsave(fx, &s);
            }
        }
        cmd::FREEZE => {
            let _ = with(p.pid, |d| {
                if !d.frozen.contains(&tid) {
                    d.frozen.push(tid);
                }
            });
        }
        cmd::RESUME => {
            let _ = with(p.pid, |d| d.frozen.retain(|&x| x != tid));
            WAIT.wake_all();
        }
        _ => {}
    }
}

// ---- Aliases (DBG_C_MapROAlias, DBG_C_MapRWAlias) ----

/// An alias a debugger has of a debuggee's memory: the debuggee's address
/// space (its page table root) and range, the debugger and the alias.
struct Alias {
    root: u64,
    start: u32,
    end: u32,
    pid: u32,
    debugger: u32,
    base: u32,
}

static ALIASES: SpinLock<Vec<Alias>> = SpinLock::new(Vec::new());

fn map_alias(c: &Ctx, p: &Process, u: &mut Udb, write: bool) {
    let (addr, len) = (u.get(udb::ADDR), u.get(udb::LEN));
    match vm::alias(&p.space, &c.process.space, addr, len, write) {
        Ok(a) => {
            let first = addr & !(vm::PAGE - 1);
            ALIASES.lock().push(Alias {
                root: p.space.lock().root,
                start: first,
                end: addr + len,
                pid: p.pid,
                debugger: c.process.pid,
                base: a & !(vm::PAGE - 1),
            });
            u.set(udb::BUFFER, a);
        }
        Err(e) => u.error(e),
    }
}

fn unmap_alias(c: &Ctx, u: &mut Udb) {
    let a = u.get(udb::BUFFER) & !(vm::PAGE - 1);
    let me = c.process.pid;
    let known = {
        let mut g = ALIASES.lock();
        let n = g.len();
        g.retain(|x| !(x.debugger == me && x.base == a));
        g.len() != n
    };
    if !known || c.space().free(a).is_err() {
        u.error(err::INVALID_ADDRESS);
    }
}

/// Memory of an address space is being freed (vm::free): the aliases of it
/// go now, before its frames can be reused, and their debuggers hear of it
/// (DBG_N_AliasFree, Value = the alias).
pub fn memory_freed(root: u64, start: u32, end: u32) {
    let gone: Vec<Alias> = {
        let mut g = ALIASES.lock();
        if g.is_empty() {
            return;
        }
        let (hit, keep): (Vec<Alias>, Vec<Alias>) = core::mem::take(&mut *g).into_iter().partition(|a| a.root == root && a.start < end && start < a.end);
        *g = keep;
        hit
    };
    for a in gone {
        if let Some(q) = process::by_pid(a.debugger) {
            let _ = q.space.lock().free(a.base);
        }
        let posted = with(a.pid, |d| {
            if d.debugger != a.debugger {
                return false;
            }
            d.events.push_back(Event { mask: 1, value: a.base, ..Event::new(d.last_tid, note::ALIAS_FREE) });
            true
        });
        if posted == Some(true) {
            WAIT.wake_all();
        }
    }
}
