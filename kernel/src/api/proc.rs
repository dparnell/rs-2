//! Processes and threads: DosExecPgm, DosCWait, DosCreateThread, ...

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::{err, Ctx};
use crate::arch::gdt::{TIB_SEL, USER_CS, USER_DS};
use crate::arch::idt::TrapFrame;
use crate::fs::Handle;
use crate::loader;
use crate::task::thread::State;
use crate::task::{process, sched, Process};
use crate::vm::{self, pag, SegKind};

/// Exec flags.
const EXEC_SYNC: u32 = 0;
const EXEC_ASYNC: u32 = 1;
const EXEC_ASYNCRESULT: u32 = 2;
const EXEC_BACKGROUND: u32 = 4;

/// Read a double-NUL terminated block of strings.
pub(super) fn read_block(c: &Ctx, addr: u32, max: usize) -> Option<Vec<String>> {
    let mut v = Vec::new();
    let mut a = addr;
    loop {
        let s = c.read_str(a, 4096)?;
        if s.is_empty() {
            break;
        }
        a += s.len() as u32 + 1;
        v.push(s);
        if v.len() > max {
            break;
        }
    }
    Some(v)
}

/// Handles a child inherits: every slot not marked no-inherit.
fn inherited_handles(p: &Process) -> Vec<Option<Handle>> {
    let ni = p.noinherit.lock().clone();
    p.handles
        .lock()
        .iter()
        .enumerate()
        .map(|(i, h)| if ni.contains(&(i as u32)) { None } else { h.clone() })
        .collect()
}

/// The parent's environment strings.
pub(super) fn parent_env(c: &Ctx) -> Vec<String> {
    read_block(c, c.process.env, 4096).unwrap_or_default()
}

fn exec(c: &mut Ctx) -> u32 {
    let (fail, cbfail, flags, pargs, penv, pres, pname) = if c.is16 {
        (c.ptr(0), c.arg(1) & 0xffff, c.arg(2), c.ptr(3), c.ptr(4), c.ptr(5), c.ptr(6))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4), c.arg(5), c.arg(6))
    };
    let Some(name) = c.read_str(pname, 260) else { return err::INVALID_ADDRESS };
    if super::tracing() {
        crate::trace!("    exec {name}");
    }
    // Arguments: "program\0arguments\0\0" (either part may be missing).
    let (argv0, args) = if pargs == 0 {
        (name.clone(), String::new())
    } else {
        let a0 = c.read_str(pargs, 4096).unwrap_or_default();
        let rest = c.read_str(pargs + a0.len() as u32 + 1, 4096).unwrap_or_default();
        (a0, rest)
    };
    if super::tracing() {
        crate::trace!("    argv0 {argv0:?} args {args:?}");
    }
    let env = if penv == 0 { parent_env(c) } else { read_block(c, penv, 4096).unwrap_or_default() };
    let mut path = match super::file::resolve(c, &name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // A bare program name is looked for in the current directory, then
    // along the caller's PATH (UNPACK starts "UNPACK2.EXE" this way).
    if !name.contains(['\\', '/', ':']) && crate::fs::stat(&path).is_err() {
        let dirs = super::doscalls::env_value(c, "PATH").unwrap_or_default();
        for d in dirs.split(';').filter(|d| !d.is_empty()) {
            let cand = alloc::format!("{}\\{}", d.trim_end_matches('\\'), name);
            if let Ok(p) = super::file::resolve(c, &cand) {
                if crate::fs::stat(&p).is_ok() {
                    path = p;
                    break;
                }
            }
        }
    }
    let mode = flags & 7;
    if !matches!(mode, EXEC_SYNC | EXEC_ASYNC | EXEC_ASYNCRESULT | EXEC_BACKGROUND) {
        return err::INVALID_FUNCTION;
    }
    // A DOS program runs in a DOS session of its own (in a window when
    // the caller is in one); the call returns at once.
    let full = path.display();
    let up = full.to_ascii_uppercase();
    if up.ends_with(".COM") || up.ends_with(".BAT") || up.ends_with(".EXE") {
        if let Some(data) = crate::fs::read_file(&full).filter(|d| super::sesmgr::is_dos_program(&full, d)) {
            drop(data);
            return match super::sesmgr::start_dos_session(c, &full, args.trim_start(), env) {
                Ok((_, pid, _)) => {
                    write_result(c, pres, if mode == EXEC_SYNC { 0 } else { pid }, 0);
                    0
                }
                Err(e) => e,
            };
        }
    }
    let _ = crate::loader::take_fail();
    let r = process::exec(process::ExecParams {
        path: &path.display(),
        program: &argv0,
        args: &args,
        env,
        parent: Some(c.process.clone()),
        handles: inherited_handles(&c.process),
        detached: mode == EXEC_ASYNC || mode == EXEC_BACKGROUND,
        session: c.process.session.load(core::sync::atomic::Ordering::Relaxed),
    });
    let child = match r {
        Ok(p) => p,
        Err(e) => {
            // The module that failed: a missing DLL or the one lacking an
            // import, else the program itself.
            let culprit = crate::loader::take_fail().unwrap_or(name);
            if fail != 0 && cbfail > 0 {
                let n = (cbfail as usize - 1).min(culprit.len());
                c.write_str(fail, &culprit[..n]);
            }
            return e;
        }
    };
    if mode == EXEC_SYNC {
        child.exit_waiters.wait_until(None, || child.exit_info().is_some());
        let info = child.exit_info().unwrap_or(process::ExitInfo { cause: 0, code: 0 });
        process::reap(child.pid);
        write_result(c, pres, info.cause, info.code);
    } else {
        // Asynchronous: codeTerminate receives the child's PID.
        write_result(c, pres, child.pid, 0);
    }
    0
}

fn write_result(c: &Ctx, at: u32, a: u32, b: u32) {
    if at == 0 {
        return;
    }
    if c.is16 {
        c.write_u16(at, a as u16);
        c.write_u16(at + 2, b as u16);
    } else {
        c.write_u32(at, a);
        c.write_u32(at + 4, b);
    }
}

pub fn dos_exec_pgm(c: &mut Ctx) -> u32 {
    exec(c)
}

/// DosCWait (16-bit) / DosWaitChild (32-bit):
/// (action, option, presult, ppid, pid)
pub fn dos_wait_child(c: &mut Ctx) -> u32 {
    let (scope, nowait, pres, ppid, pid) = if c.is16 {
        (c.arg(0), c.arg(1), c.ptr(2), c.ptr(3), c.arg(4))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4))
    };
    let _ = scope;
    let me = c.process.pid;
    let children = || -> Vec<Arc<Process>> {
        process::all().into_iter().filter(|p| p.ppid == me && (pid == 0 || p.pid == pid)).collect()
    };
    if children().is_empty() {
        return if pid == 0 { err::WAIT_NO_CHILDREN } else { err::INVALID_PROCID };
    }
    let done = || children().into_iter().find(|p| p.exit_info().is_some() && p.threads.lock().is_empty());
    let found = if nowait != 0 {
        match done() {
            Some(p) => p,
            None => return err::CHILD_NOT_COMPLETE,
        }
    } else {
        let mut got = None;
        c.process.child_waiters.wait_until(None, || {
            got = done();
            got.is_some()
        });
        match got {
            Some(p) => p,
            None => return err::INTERRUPT,
        }
    };
    let info = found.exit_info().unwrap();
    process::reap(found.pid);
    write_result(c, pres, info.cause, info.code);
    if c.is16 { c.write_u16(ppid, found.pid as u16) } else { c.write_u32(ppid, found.pid) };
    0
}

pub fn dos_kill_process(c: &mut Ctx) -> u32 {
    let (scope, pid) = (c.arg(0), c.arg(1));
    let Some(p) = process::by_pid(pid) else { return err::INVALID_PROCID };
    if p.exit_info().is_some() {
        return err::INVALID_PROCID;
    }
    // The PM window list (the PM server) ends PM programs that are not
    // its children.
    let pm = crate::display::is_pm(c.process.pid) && crate::display::is_pm(p.pid);
    if p.pid != c.process.pid && p.ppid != c.process.pid && !pm {
        return err::NOT_DESCENDANT;
    }
    // DKP_PROCESSTREE (1): the descendants too, deepest first.
    if scope & 1 != 0 {
        let mut tree = alloc::vec![p.pid];
        let mut i = 0;
        while i < tree.len() {
            let parent = tree[i];
            for q in process::all() {
                if q.ppid == parent && q.exit_info().is_none() && !tree.contains(&q.pid) {
                    tree.push(q.pid);
                }
            }
            i += 1;
        }
        for &d in tree.iter().skip(1).rev() {
            if let Some(q) = process::by_pid(d) {
                q.exit(3, 0);
            }
        }
    }
    p.exit(3, 0); // TC_KILLPROCESS
    0
}

pub fn dos16_get_ppid(c: &mut Ctx) -> u32 {
    let Some(p) = process::by_pid(c.arg(0)) else { return err::INVALID_PROCID };
    c.write_u16(c.ptr(1), p.ppid as u16);
    0
}

// ---- Threads ----

/// Slot (0, 1) of the gate: 32-bit threads return here, which ends the
/// thread with the return value as its result.
pub const THREAD_EXIT_ORDINAL: u32 = 1;

pub fn dos_create_thread(c: &mut Ctx) -> u32 {
    // DosCreateThread(ptid, pfn, param, flag, cbStack)
    let (ptid, pfn, param, flag, cb) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    create_thread32(c, ptid, pfn, param, flag, cb, None)
}

/// A 32-bit thread running pfn(param): on a stack the system allocates
/// (freed when the thread ends) or, from DosCreateThread2, on the caller's
/// stack `user` (base, size).
pub(super) fn create_thread32(c: &Ctx, ptid: u32, pfn: u32, param: u32, flag: u32, cb: u32, user: Option<(u32, u32)>) -> u32 {
    let (base, top, owned) = match user {
        Some((b, n)) => (b, b + n, 0),
        None => {
            let size = cb.max(8192).next_multiple_of(vm::PAGE);
            let r = c.space().alloc(size, pag::READ | pag::WRITE, SegKind::Data, None);
            let base = match r {
                Ok(b) => b,
                Err(e) => return e.code(),
            };
            let mut sp = c.space();
            if flag & 2 != 0 {
                // STACK_COMMITTED
                let _ = sp.set_mem(base, size, pag::COMMIT | pag::READ | pag::WRITE);
            } else {
                // Top page committed, a guard page below it grows the stack.
                let _ = sp.set_mem(base + size - vm::PAGE, vm::PAGE, pag::COMMIT | pag::READ | pag::WRITE);
                if size >= 2 * vm::PAGE {
                    let _ = sp.set_mem(base + size - 2 * vm::PAGE, vm::PAGE, pag::COMMIT | pag::GUARD | pag::READ | pag::WRITE);
                }
            }
            (base, base + size, base)
        }
    };
    // Return address and parameter, with the parameter 16-byte aligned as
    // compilers expect at a function entry (SSE spills).
    let sp = ((top - 4) & !15) - 4;
    let exit_stub = loader::gate::slot_address(0, THREAD_EXIT_ORDINAL);
    {
        let mut b = Vec::new();
        b.extend_from_slice(&exit_stub.to_le_bytes());
        b.extend_from_slice(&param.to_le_bytes());
        c.space().poke_bytes(sp, &b);
    }
    let frame = TrapFrame {
        rflags: 0x202,
        cs: USER_CS as u64,
        rip: pfn as u64,
        ss: USER_DS as u64,
        rsp: sp as u64,
        ds: USER_DS as u64,
        es: USER_DS as u64,
        fs: TIB_SEL as u64,
        ..Default::default()
    };
    let r = start_thread(c, frame, base, top, ptid, flag & 1 != 0, false, owned);
    if r != 0 && owned != 0 {
        let _ = c.space().free(owned);
    }
    r
}

pub fn dos16_create_thread(c: &mut Ctx) -> u32 {
    // DosCreateThread(pfnFun far, pTid, pbStack far): stack pointer is the
    // top of the new thread's stack.
    let (pfn, ptid, stack) = (c.arg(0), c.ptr(1), c.arg(2));
    let ss = (stack >> 16) as u16;
    // The thread function is entered as if far-called: a return address
    // (to the thread exit stub) goes below pbStack, where the caller may
    // have stored the function's arguments.
    let sp_off = (stack & 0xffff).wrapping_sub(4) & 0xffff;
    {
        let ret = crate::loader::gate::slot_address(0, THREAD_EXIT_ORDINAL);
        let far = (vm::tile_selector(ret) as u32) << 16 | (ret & 0xffff);
        let at = c.space().linear(ss, sp_off).unwrap_or(0);
        if !c.write_u32(at, far) {
            return err::INVALID_ADDRESS;
        }
    }
    let frame = TrapFrame {
        rflags: 0x202,
        cs: (pfn >> 16) as u64,
        rip: (pfn & 0xffff) as u64,
        ss: ss as u64,
        rsp: sp_off as u64,
        ds: c.f.ds,
        es: c.f.ds,
        ..Default::default()
    };
    let base = c.space().linear(ss, 0).unwrap_or(0);
    start_thread(c, frame, base, base + sp_off, ptid, false, true, 0)
}

fn start_thread(c: &Ctx, frame: TrapFrame, base: u32, top: u32, ptid: u32, suspended: bool, tid16: bool, owned_stack: u32) -> u32 {
    let t = match c.process.new_thread(frame, base, top) {
        Ok(t) => t,
        Err(e) => return e,
    };
    t.owned_stack.store(owned_stack, Ordering::Relaxed);
    if tid16 { c.write_u16(ptid, t.tid as u16) } else { c.write_u32(ptid, t.tid) };
    if suspended {
        t.suspend.store(1, Ordering::Relaxed);
    }
    sched::start(t);
    0
}

/// Called when a 32-bit thread function returns into the exit stub.
pub fn thread_returned(c: &mut Ctx) -> u32 {
    let code = c.f.rax as u32;
    c.thread.sync_exit.store(true, Ordering::Relaxed);
    let live = c.process.threads.lock().iter().filter(|t| t.state() != State::Dead).count();
    if live > 1 {
        c.thread.request_exit();
    } else {
        c.process.exit(0, code);
    }
    0
}

pub fn dos_wait_thread(c: &mut Ctx) -> u32 {
    // DosWaitThread(ptid, option)
    let (ptid, nowait) = (c.arg(0), c.arg(1) != 0);
    let Some(want) = c.read_u32(ptid) else { return err::INVALID_ADDRESS };
    if want == c.thread.tid {
        return err::INVALID_THREADID;
    }
    let p = c.process.clone();
    let find = |want: u32| -> Result<Option<u32>, u32> {
        let ts = p.threads.lock();
        if want == 0 {
            if let Some(t) = ts.iter().find(|t| t.state() == State::Dead && t.tid != 1) {
                return Ok(Some(t.tid));
            }
            if ts.iter().filter(|t| t.state() != State::Dead).count() <= 1 {
                return Err(err::INVALID_THREADID);
            }
            return Ok(None);
        }
        match ts.iter().find(|t| t.tid == want) {
            Some(t) if t.state() == State::Dead => Ok(Some(want)),
            Some(_) => Ok(None),
            None => Err(err::INVALID_THREADID),
        }
    };
    let mut result = find(want);
    if let Ok(None) = result {
        if nowait {
            return 294; // ERROR_THREAD_NOT_TERMINATED
        }
        p.thread_waiters.wait_until(None, || {
            result = find(want);
            !matches!(result, Ok(None))
        });
    }
    match result {
        Ok(Some(tid)) => {
            p.threads.lock().retain(|t| t.tid != tid);
            c.write_u32(ptid, tid);
            0
        }
        Ok(None) => err::INTERRUPT,
        Err(e) => e,
    }
}

pub fn dos_kill_thread(c: &mut Ctx) -> u32 {
    let tid = c.arg(0);
    if tid == c.thread.tid {
        return err::INVALID_THREADID;
    }
    // Only a live thread of this process (one that ended and waits to be
    // collected cannot be killed).
    let t = c.process.threads.lock().iter().find(|t| t.tid == tid && t.state() != State::Dead && !t.exit_requested()).cloned();
    match t {
        Some(t) => {
            sched::kill(&t);
            0
        }
        None => err::INVALID_THREADID,
    }
}

pub fn dos_suspend_thread(c: &mut Ctx) -> u32 {
    let tid = c.arg(0);
    let t = c.process.threads.lock().iter().find(|t| t.tid == tid).cloned();
    match t {
        Some(t) => {
            t.suspend.fetch_add(1, Ordering::Relaxed);
            0
        }
        None => err::INVALID_THREADID,
    }
}

pub fn dos_resume_thread(c: &mut Ctx) -> u32 {
    let tid = c.arg(0);
    let t = c.process.threads.lock().iter().find(|t| t.tid == tid).cloned();
    match t {
        Some(t) => {
            if t.suspend.load(Ordering::Relaxed) == 0 {
                return 90; // ERROR_NOT_FROZEN
            }
            t.suspend.fetch_sub(1, Ordering::Relaxed);
            c.process.thread_waiters.wake_all();
            0
        }
        None => err::INVALID_THREADID,
    }
}

/// DosEnterCritSec: when it returns no other thread of the process runs
/// user code until DosExitCritSec. Other threads are held on their way
/// back to user mode (`hold_if_needed`); with several CPUs the ones
/// running right now are interrupted and waited for.
pub fn dos_enter_crit_sec(c: &mut Ctx) -> u32 {
    let me = c.thread.tid;
    loop {
        {
            let mut cs = c.process.critsec.lock();
            if cs.0 == me || cs.0 == 0 {
                cs.0 = me;
                cs.1 += 1;
                if cs.1 > 1 {
                    return 0;
                }
                break;
            }
        }
        // Another thread is in its critical section (it got there while
        // this one was running on another CPU): wait for it.
        let p = c.process.clone();
        p.thread_waiters.wait_until(None, || p.critsec.lock().0 == 0);
        if c.thread.exit_requested() || c.thread.interrupted() {
            return err::INTERRUPT;
        }
    }
    stop_other_threads(c);
    0
}

/// Wait until no other thread of the process is running user code on
/// another CPU (they stop at their next return to user mode).
fn stop_other_threads(c: &Ctx) {
    if crate::smp::online_count() < 2 {
        return;
    }
    let running = || {
        c.process.threads.lock().iter().any(|t| {
            t.tid != c.thread.tid && t.state() == State::Running && t.on_cpu.load(Ordering::Acquire)
        })
    };
    if !running() {
        return;
    }
    // Make every other CPU pass through the kernel, then wait without the
    // kernel lock so they can reach the point where they are held.
    let me = crate::smp::this().bit();
    for cpu in crate::smp::online() {
        if cpu.bit() != me {
            crate::smp::kick(cpu);
        }
    }
    for _ in 0..2000 {
        if !running() {
            return;
        }
        sched::sleep_ms(1);
    }
}

pub fn dos_exit_crit_sec(c: &mut Ctx) -> u32 {
    let mut cs = c.process.critsec.lock();
    if cs.0 != c.thread.tid {
        return 309;
    }
    cs.1 -= 1;
    if cs.1 == 0 {
        cs.0 = 0;
        drop(cs);
        c.process.thread_waiters.wake_all();
    }
    0
}

// ---- Priorities ----

fn apply_priority(t: &Arc<crate::task::thread::Thread>, class: u32, delta: i32) -> u32 {
    let cur = t.priority();
    let (mut cls, mut lvl) = (cur >> 8, (cur & 0xff) as i32);
    if class != 0 {
        if class > 4 {
            return err::INVALID_PCLASS;
        }
        cls = class;
    }
    if !(-31..=31).contains(&delta) {
        return err::INVALID_PDELTA;
    }
    lvl = (lvl + delta).clamp(0, 31);
    t.set_priority(cls << 8 | lvl as u32);
    // A queued thread moves to its new queue; a running one is looked at
    // again on its way back to user mode.
    sched::reprioritize(t);
    if t.state() == crate::task::thread::State::Running {
        crate::smp::this().need_resched.store(true, Ordering::Relaxed);
    }
    0
}

pub fn dos_set_priority(c: &mut Ctx) -> u32 {
    // DosSetPriority(scope, class, delta, id) / DosSetPrty
    let (scope, class, delta, id) = (c.arg(0), c.arg(1), c.arg(2) as i16 as i32, c.arg(3));
    match scope {
        2 => {
            // PRTYS_THREAD
            let tid = if id == 0 { c.thread.tid } else { id };
            let t = c.process.threads.lock().iter().find(|t| t.tid == tid).cloned();
            match t {
                Some(t) => apply_priority(&t, class, delta),
                None => err::INVALID_THREADID,
            }
        }
        0 | 1 => {
            let p = if id == 0 { Some(c.process.clone()) } else { process::by_pid(id) };
            let Some(p) = p else { return err::INVALID_PROCID };
            for t in p.threads.lock().iter() {
                let r = apply_priority(t, class, delta);
                if r != 0 {
                    return r;
                }
            }
            0
        }
        _ => err::INVALID_SCOPE,
    }
}

pub fn dos16_get_prty(c: &mut Ctx) -> u32 {
    // DosGetPrty(usScope, pusPriority, id)
    let p = c.thread.priority();
    c.write_u16(c.ptr(1), p as u16);
    0
}

/// Park the current thread while it is suspended or another thread owns
/// the process's critical section. Called before returning to user mode.
pub fn hold_if_needed(t: &Arc<crate::task::thread::Thread>, p: &Arc<Process>) {
    let blocked = || {
        if t.exit_requested() || p.exitlist_tid.load(Ordering::Relaxed) == t.tid {
            return false;
        }
        let cs = p.critsec.lock().0;
        t.suspend.load(Ordering::Relaxed) > 0 || (cs != 0 && cs != t.tid)
    };
    while blocked() {
        p.thread_waiters.wait_uninterruptible(|| !blocked());
        if t.exit_requested() {
            break;
        }
    }
}

// ---- Exit lists ----

/// Gate slot (0, 4): an exit list routine returned without calling
/// DosExitList(EXLST_EXIT); treated the same.
pub const EXITLIST_RETURN_ORDINAL: u32 = 4;

pub fn dos_exit_list(c: &mut Ctx) -> u32 {
    let (code, pfn) = (c.arg(0), c.arg(1));
    let order = ((code >> 8) & 0xff) as u8;
    match code & 0xff {
        1 => {
            let mut l = c.process.exitlist.lock();
            if !l.iter().any(|e| e.1 == pfn) {
                l.push((order, pfn, c.is16));
            }
            0
        }
        2 => {
            let mut l = c.process.exitlist.lock();
            let n = l.len();
            l.retain(|e| e.1 != pfn);
            if l.len() == n { err::INVALID_FUNCTION } else { 0 }
        }
        3 => {
            if c.process.exitlist_tid.load(Ordering::Relaxed) != c.thread.tid {
                return err::INVALID_FUNCTION;
            }
            // Run the next routine (or finish) on the way back to user mode.
            c.process.exitlist_next.store(true, Ordering::Relaxed);
            c.no_return = true;
            0
        }
        _ => err::INVALID_DATA,
    }
}

/// Called before returning to user mode: if this thread is running the
/// exit list, redirect it into the next routine. Returns true when the list
/// is done and the thread (and process) must now end.
pub fn exit_list_step(t: &Arc<crate::task::thread::Thread>, p: &Arc<Process>, f: &mut TrapFrame) -> bool {
    if p.exitlist_tid.load(Ordering::Relaxed) != t.tid || !p.exitlist_next.swap(false, Ordering::Relaxed) {
        return false;
    }
    // A thread pulled out of a wait to run the exit list (see
    // Process::exit) must not see its routines' waits interrupted.
    t.clear_interrupt();
    let code = p.exit_info().map_or(0, |e| e.cause);
    loop {
        // Lowest order code first.
        let next = {
            let mut l = p.exitlist.lock();
            let i = l.iter().enumerate().min_by_key(|(_, e)| e.0).map(|(i, _)| i);
            i.map(|i| l.remove(i))
        };
        let Some((_, pfn, is16)) = next else {
            // Then the DLL termination routines.
            if !p.terms_started.swap(true, Ordering::Relaxed) {
                let calls = super::module::term_order(p);
                if super::module::start_term(p, t, f, calls) {
                    return false;
                }
            }
            // Done: now really terminate.
            p.exitlist_tid.store(0, Ordering::Relaxed);
            let info = p.exit_info().unwrap_or(process::ExitInfo { cause: 0, code: 0 });
            p.finish_exit(info);
            return true;
        };
        let stub = loader::gate::slot_address(0, EXITLIST_RETURN_ORDINAL);
        let mut sp = p.space.lock();
        let frame16 = sp.descriptor(f.cs as u16) & (1 << 54) == 0 && f.cs as u16 & 4 != 0;
        if is16 && frame16 {
            // routine(USHORT usTermCode), far _Pascal.
            let ss_base = vm::desc_base(sp.descriptor(f.ss as u16));
            let spv = (f.rsp as u32 & 0xffff).wrapping_sub(6) & 0xffff;
            let mut b = Vec::new();
            b.extend_from_slice(&(stub as u16).to_le_bytes());
            b.extend_from_slice(&vm::tile_selector(stub).to_le_bytes());
            b.extend_from_slice(&(code as u16).to_le_bytes());
            sp.poke_bytes(ss_base + spv, &b);
            f.rsp = (f.rsp & !0xffff) | spv as u64;
            f.cs = (pfn >> 16) as u64;
            f.rip = (pfn & 0xffff) as u64;
            return false;
        }
        if !is16 {
            // routine(ULONG ulTermCode), _System.
            let mut esp = f.rsp as u32;
            if !frame16 {
                esp = esp.wrapping_sub(8);
            } else {
                // Coming from 16-bit code: use the main thread's stack top.
                esp = t.stack_top.load(Ordering::Relaxed).wrapping_sub(64);
            }
            let mut b = Vec::new();
            b.extend_from_slice(&stub.to_le_bytes());
            b.extend_from_slice(&code.to_le_bytes());
            sp.poke_bytes(esp, &b);
            f.rsp = esp as u64;
            f.cs = USER_CS as u64;
            f.ss = USER_DS as u64;
            f.ds = USER_DS as u64;
            f.es = USER_DS as u64;
            f.fs = TIB_SEL as u64;
            f.rip = pfn as u64;
            return false;
        }
        // A 16-bit routine while in 32-bit code: skipped.
    }
}
