//! TOOLHELP: the task, module and global heap databases, stack traces,
//! MemoryRead/MemoryWrite and the notifications debuggers register for.
//!
//! On Windows every task shares one address space, and TOOLHELP walks
//! KERNEL's own lists. Here each task is a process (docs/WIN16.md,
//! "Tasks and scheduling"): selectors mean nothing outside their process,
//! and the same numbers are other segments in the next one. So:
//!
//! - the **task registry**, a shared object every WIN16 process maps,
//!   lists the tasks: process, instance, stack, start address, segments,
//!   module name and path, and each one's NotifyRegister callback;
//! - each task has its **task database** (TDB, Windows' layout with the
//!   "TD" signature) whose selector is its task handle (GetCurrentTask);
//! - another task is seen through a **view**: a TDB, a module database
//!   (the NE header of its file, the segment table holding handles) and
//!   copies of its segments (read with Rs2Win16ReadTask), all selectors
//!   of this process. Everything TOOLHELP says about that task (its task
//!   handle, hInstance, hModule, SS, its start CS:IP, GlobalEntryModule's
//!   blocks) is in terms of the view, so a debugger reads (and writes its
//!   breakpoints into) the copies, never into its own segments that
//!   happen to have the same numbers.
//!
//! Notifications cross processes as PM queue messages: a task that starts
//! or ends posts NFY_STARTTASK / NFY_EXITTASK (and OutputDebugString's
//! NFY_OUTSTR) to every process with a callback, whose WIN16 calls the
//! callback from its message loop, with GetCurrentTask answering the
//! task the event is about, as it would inside that task on Windows. The
//! other task keeps running meanwhile: a debugger's breakpoints are not
//! hit and it cannot stop, step or read the live registers of the task
//! (docs/WIN16.md, "Delphi 1.0").

use core::sync::atomic::{AtomicU32, Ordering};

use alloc::vec::Vec;

use thunk16::{flat, Pascal, PascalCall};

use crate::{far_str, Global};

const SLOTS: usize = 64;
const SEGS: usize = 48;
const MAGIC: u32 = 0x5431_3657; // "W16T"
/// The PM queue message of a notification: mp1 the NFY_ code and the
/// process of the task it is about, mp2 the data.
pub const NOTIFY_MSG: u32 = 0x0004_0000 | 0x4e46;

const NFY_STARTTASK: u16 = 4;
const NFY_EXITTASK: u16 = 5;
const NFY_OUTSTR: u16 = 11;

/// One task in the registry (its own selectors).
#[repr(C)]
#[derive(Clone, Copy)]
struct Slot {
    pid: u32,
    hinst: u16,
    nsegs: u16,
    hmq: u32,
    /// NotifyRegister's callback (16:16) and flags.
    notify: u32,
    notify_flags: u16,
    /// InterruptRegister's callback.
    interrupt: u32,
    /// A debugger let the task go on from its start (task_started waits
    /// for it).
    go: u32,
    ss: u16,
    sp: u16,
    stack_top: u16,
    stack_min: u16,
    stack_bottom: u16,
    start: u32,
    segs: [u16; SEGS],
    name: [u8; 10],
    path: [u8; 82],
    /// OutputDebugString's last string.
    out: [u8; 130],
}

#[repr(C)]
struct Registry {
    magic: u32,
    lock: AtomicU32,
    slots: [Slot; SLOTS],
}

/// Another task as this process sees it.
struct View {
    pid: u32,
    tdb: u16,
    moddb: u16,
    /// Its segments: (its selector, the copy's).
    segs: Vec<(u16, u16)>,
    name: Vec<u8>,
    path: Vec<u8>,
    exited: bool,
    /// Where it stopped (its copy's CS:IP), for TaskGetCSIP/TaskSetCSIP.
    csip: u32,
    /// Debugged through the kernel (RS2API 423): its INT 3s and single
    /// steps stop it, and the copies' changes (breakpoints) are written
    /// into it.
    attached: bool,
    /// Each copy as last read from the task (what the debugger changed
    /// since is written back).
    base: Vec<Vec<u8>>,
}

struct State {
    reg: *mut Registry,
    /// This task's TDB.
    tdb: u16,
    /// GetCurrentTask while a notification of another task is delivered.
    current: u16,
    exited: bool,
    views: Vec<View>,
    /// NFY_OUTSTR's string, readable by the callback.
    out: *mut u8,
    /// An interrupt callback is running (int3).
    in_int: bool,
    /// The IRET that ended the interrupt callback: CS, IP, FLAGS.
    iret: Option<(u16, u16, u16)>,
    /// A debugger timer runs (GetMessage wakes to look for stops).
    timer: bool,
    last_poll: u32,
    /// Stopped threads to let go when this task next yields (Windows
    /// runs the other task only then): (process, thread, registers).
    pending: Vec<(u32, u32, Option<[u32; 16]>)>,
    /// The thread handling a stop (w16lock.rs): its id, its PM queue, the
    /// task it stands for (GetCurrentTask there) and whether it waits
    /// for messages; the stop record it was given.
    stop_tid: u32,
    stop_hmq: u32,
    stop_task: u16,
    stop_pid: u32,
    stop_waiting: bool,
    stop_rec: [u32; 20],
}

static ST: Global<State> = Global::new(State { reg: core::ptr::null_mut(), tdb: 0, current: 0, exited: false, views: Vec::new(), out: core::ptr::null_mut(), in_int: false, iret: None, timer: false, last_poll: 0, pending: Vec::new(), stop_tid: 0, stop_hmq: 0, stop_task: 0, stop_pid: 0, stop_waiting: false, stop_rec: [0; 20] });

fn pib() -> Option<&'static os2::Pib> {
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    unsafe { pib.as_ref() }
}

fn pid() -> u32 {
    pib().map_or(0, |p| p.pid)
}

/// The registry, mapped (and made by the first task).
fn registry() -> Option<&'static mut Registry> {
    let s = ST.get();
    if s.reg.is_null() {
        let name = b"\\SHAREMEM\\RS2W16\\TASKS.DAT\0";
        let mut p: *mut u8 = core::ptr::null_mut();
        let size = core::mem::size_of::<Registry>() as u32;
        let rw = os2::PAG_READ | os2::PAG_WRITE;
        if unsafe { os2::DosGetNamedSharedMem(&mut p, name.as_ptr(), rw) } != 0
            && unsafe { os2::DosAllocSharedMem(&mut p, name.as_ptr(), size, rw | os2::PAG_COMMIT) } != 0
            && unsafe { os2::DosGetNamedSharedMem(&mut p, name.as_ptr(), rw) } != 0
        {
            return None;
        }
        s.reg = p as *mut Registry;
        let r = unsafe { &mut *s.reg };
        if r.magic != MAGIC {
            r.magic = MAGIC;
        }
    }
    Some(unsafe { &mut *s.reg })
}

fn lock(r: &Registry) {
    while r.lock.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
        unsafe { os2::DosSleep(0) };
    }
}

fn unlock(r: &Registry) {
    r.lock.store(0, Ordering::Release);
}

/// The live tasks (a copy).
fn tasks() -> Vec<Slot> {
    let Some(r) = registry() else { return Vec::new() };
    lock(r);
    let v = r.slots.iter().filter(|s| s.pid != 0).copied().collect();
    unlock(r);
    v
}

fn slot(pid: u32) -> Option<Slot> {
    tasks().into_iter().find(|s| s.pid == pid)
}

fn with_slot<T>(pid: u32, f: impl FnOnce(&mut Slot) -> T) -> Option<T> {
    let r = registry()?;
    lock(r);
    let out = r.slots.iter_mut().find(|s| s.pid == pid).map(f);
    unlock(r);
    out
}

fn put16(p: *mut u8, off: usize, v: u16) {
    unsafe { (p.add(off) as *mut u16).write_unaligned(v) };
}

fn put32(p: *mut u8, off: usize, v: u32) {
    unsafe { (p.add(off) as *mut u32).write_unaligned(v) };
}

fn get16(p: *const u8, off: usize) -> u16 {
    unsafe { (p.add(off) as *const u16).read_unaligned() }
}

fn put_str(p: *mut u8, off: usize, max: usize, s: &[u8]) {
    let n = s.len().min(max - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(off), n);
        *p.add(off + n) = 0;
    }
}

/// A tiled block of this process (a selector of its own): (selector,
/// address).
fn new_block(size: u32) -> Option<(u16, *mut u8)> {
    let mut p: *mut u8 = core::ptr::null_mut();
    let fl = os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | os2::OBJ_TILE;
    if unsafe { os2::DosAllocMem(&mut p, size.clamp(0x1000, 0x1_0000), fl) } != 0 {
        return None;
    }
    Some(((thunk16::far(p) >> 16) as u16, p))
}

/// Fill a TDB (Windows' layout: the fields programs read).
#[allow(clippy::too_many_arguments)]
fn fill_tdb(p: *mut u8, tdb: u16, ss_sp: u32, hinst: u16, hmodule: u16, hmq: u32, psp: u16, cmdshow: u16, name: &[u8]) {
    put32(p, 0x02, ss_sp);
    put16(p, 0x0c, tdb);
    put16(p, 0x1a, 0x030a);
    put16(p, 0x1c, hinst);
    put16(p, 0x1e, hmodule);
    put16(p, 0x20, (hmq >> 16) as u16 | 7);
    put16(p, 0x60, psp);
    put16(p, 0xa8, cmdshow);
    for (i, c) in name.iter().take(8).enumerate() {
        unsafe { *p.add(0xf2 + i) = *c };
    }
    put16(p, 0xfa, 0x4454); // "TD"
}

fn exe_hmod() -> u32 {
    pib().map_or(0, |p| p.hmte)
}

/// InitTask: this task's TDB and its registry entry; NFY_STARTTASK to
/// the debuggers (`start`: the program's CS:IP).
pub fn task_started(hinst: u16, psp: u16, cmdshow: u16, stack: (u16, u16, u16, u16, u16), start: u32) {
    let Some((tdb, p)) = new_block(0x1000) else { return };
    let s = ST.get();
    s.tdb = tdb;
    let path = crate::module::path_of(exe_hmod()).unwrap_or_default();
    let name = crate::module::base_name(&path);
    let t = crate::kernel::TASK.get();
    fill_tdb(p, tdb, (stack.0 as u32) << 16 | stack.1 as u32, hinst, hinst, t.hmq, psp, cmdshow, &name);
    let Some(r) = registry() else { return };
    let mut slot = Slot {
        pid: pid(),
        hinst,
        nsegs: 0,
        hmq: t.hmq,
        notify: 0,
        notify_flags: 0,
        interrupt: 0,
        go: 0,
        ss: stack.0,
        sp: stack.1,
        stack_top: stack.2,
        stack_min: stack.3,
        stack_bottom: stack.4,
        start,
        segs: [0; SEGS],
        name: [0; 10],
        path: [0; 82],
        out: [0; 130],
    };
    let hmod = exe_hmod();
    let n = (unsafe { crate::pm::Rs2Win16Selector(6, hmod, 0) } as usize).min(SEGS);
    for i in 0..n {
        slot.segs[i] = unsafe { crate::pm::Rs2Win16Selector(6, hmod, i as u32 + 1) } as u16;
    }
    slot.nsegs = n as u16;
    put_str(slot.name.as_mut_ptr(), 0, 10, &name);
    put_str(slot.path.as_mut_ptr(), 0, 82, &path);
    lock(r);
    // A slot of a process that is gone is reused.
    if let Some(i) = r.slots.iter().position(|x| x.pid == slot.pid).or_else(|| r.slots.iter().position(|x| x.pid == 0)) {
        r.slots[i] = slot;
    }
    unlock(r);
    notify_all(NFY_STARTTASK, start);
    // A debugger (another task with notification and interrupt callbacks)
    // may stop the task at its start: it says when it may go on.
    let me = pid();
    let debugged = || tasks().iter().any(|t| t.pid != me && t.notify != 0 && t.interrupt != 0);
    if debugged() {
        for _ in 0..1500 {
            if self::slot(me).is_none_or(|s| s.go != 0) || !debugged() {
                break;
            }
            unsafe { os2::DosSleep(20) };
        }
    }
}

/// The task's PM message queue exists (notifications are posted to it).
pub fn queue_created(hmq: u32) {
    with_slot(pid(), |s| s.hmq = hmq);
}

/// The task ends (its exit code): NFY_EXITTASK, and it leaves the
/// registry.
pub fn task_exiting(code: u16) {
    let s = ST.get();
    if s.exited || s.tdb == 0 {
        return;
    }
    s.exited = true;
    notify_all(NFY_EXITTASK, code as u32 & 0xff);
    let me = pid();
    if let Some(r) = registry() {
        lock(r);
        for x in r.slots.iter_mut().filter(|x| x.pid == me) {
            x.pid = 0;
        }
        unlock(r);
    }
}

/// OutputDebugString: NFY_OUTSTR (the string travels in the registry).
pub fn output_debug_string(s: &[u8]) {
    if ST.get().tdb == 0 {
        return;
    }
    with_slot(pid(), |x| put_str(x.out.as_mut_ptr(), 0, x.out.len(), s));
    notify_all(NFY_OUTSTR, 0);
}

fn notify_all(id: u16, data: u32) {
    let me = pid();
    for t in tasks() {
        if t.notify != 0 && t.hmq != 0 {
            let mp1 = id as u32 | me << 16;
            if t.pid == me {
                // This task's own callback (a program watching itself).
                deliver(mp1, data);
            } else {
                unsafe { crate::pm::WinPostQueueMsg(t.hmq, NOTIFY_MSG, mp1, data) };
            }
        }
    }
}

/// This process's view of another task (made when first needed).
fn view(p: u32) -> Option<&'static mut View> {
    let s = ST.get();
    if let Some(i) = s.views.iter().position(|v| v.pid == p) {
        return s.views.get_mut(i);
    }
    let t = slot(p)?;
    // Its segments: copies read from it.
    let mut segs = Vec::new();
    for &sel in &t.segs[..t.nsegs as usize] {
        let Some((copy, cp)) = new_block(0x1_0000) else { continue };
        unsafe { crate::pm::Rs2Win16ReadTask(p, sel as u32, 0, cp, 0x1_0000) };
        segs.push((sel, copy));
    }
    let name = unsafe { crate::cstr(t.name.as_ptr()) }.to_vec();
    let path = unsafe { crate::cstr(t.path.as_ptr()) }.to_vec();
    let moddb = module_database(&path, &segs).unwrap_or(0);
    let (tdb, tp) = new_block(0x1000)?;
    let map = |sel: u16| segs.iter().find(|x| x.0 | 7 == sel | 7).map_or(0, |x| x.1);
    fill_tdb(tp, tdb, (map(t.ss) as u32) << 16 | t.sp as u32, map(t.hinst), moddb, t.hmq, 0, 1, &name);
    let csip = (map((t.start >> 16) as u16) as u32) << 16 | t.start & 0xffff;
    let base = segs.iter().map(|&(_, c)| unsafe { core::slice::from_raw_parts(flat((c as u32) << 16), 0x1_0000) }.to_vec()).collect();
    // A debugger (this task has an interrupt callback) debugs it.
    let attached = with_slot(pid(), |s| s.interrupt).unwrap_or(0) != 0 && unsafe { crate::pm::Rs2Win16Debug(0, p, 0, 0, 0) } != 0;
    if attached && !s.timer {
        // GetMessage wakes now and then to look for stops.
        s.timer = unsafe { crate::pm::WinStartTimer(crate::kernel::TASK.get().hab, 0, 0x7d15, 50) } != 0;
    }
    if crate::tracing() {
        crate::trace(format_args!("W16 view of task {p}: {} segments, attached {attached}", segs.len()));
    }
    s.views.push(View { pid: p, tdb, moddb, segs, name, path, exited: false, csip, attached, base });
    s.views.last_mut()
}

/// Read the task's segments into the copies again (it stopped).
fn refresh(v: &mut View) {
    for (i, &(real, copy)) in v.segs.iter().enumerate() {
        let cp = flat((copy as u32) << 16);
        unsafe { crate::pm::Rs2Win16ReadTask(v.pid, real as u32, 0, cp, 0x1_0000) };
        if let Some(b) = v.base.get_mut(i) {
            b.copy_from_slice(unsafe { core::slice::from_raw_parts(cp, 0x1_0000) });
        }
    }
}

/// Write what the debugger changed in the copies (its breakpoints, a
/// variable) into the task.
fn sync(v: &mut View) {
    if !v.attached {
        return;
    }
    for (i, &(real, copy)) in v.segs.iter().enumerate() {
        let cp = flat((copy as u32) << 16);
        let now = unsafe { core::slice::from_raw_parts(cp, 0x1_0000) };
        let Some(base) = v.base.get_mut(i) else { continue };
        let mut o = 0;
        while o < now.len() {
            if now[o] == base[o] {
                o += 1;
                continue;
            }
            let start = o;
            while o < now.len() && now[o] != base[o] && o - start < 256 {
                o += 1;
            }
            let n = o - start;
            if unsafe { crate::pm::Rs2Win16Debug(2, v.pid, (real as u32) << 16 | start as u32, now.as_ptr().add(start) as u32, n as u32) } != 0 {
                base[start..o].copy_from_slice(&now[start..o]);
            }
            if crate::tracing() {
                crate::trace(format_args!("W16 debug: {n} bytes to {real:04x}:{start:04x} of task {}", v.pid));
            }
        }
    }
}

/// GetMessage/PeekMessage: a debugger's breakpoints go into its tasks,
/// and a task that stopped (INT 3, a single step) is given to the
/// interrupt callback. `idle`: this task yields (Yield, or no message
/// for GetMessage/PeekMessage), and the tasks it let go run, as Windows
/// would switch to them only then.
pub fn poll(idle: bool) {
    let s = ST.get();
    if s.in_int || !s.views.iter().any(|v| v.attached && !v.exited) {
        return;
    }
    // Tasks the debugger let go run now, with its breakpoints.
    if idle && !s.pending.is_empty() && !on_stop_thread() {
        for v in s.views.iter_mut().filter(|v| v.attached && !v.exited) {
            sync(v);
        }
        for (p, tid, regs) in core::mem::take(&mut ST.get().pending) {
            let rp = regs.as_ref().map_or(0, |r| r.as_ptr() as u32);
            unsafe { crate::pm::Rs2Win16Debug(4, p, tid, rp, 0) };
        }
    }
    let s = ST.get();
    let now = unsafe { crate::pm::WinGetCurrentTime(crate::kernel::TASK.get().hab) };
    if now.wrapping_sub(s.last_poll) < 20 {
        return;
    }
    s.last_poll = now;
    for v in s.views.iter_mut().filter(|v| v.attached && !v.exited) {
        sync(v);
    }
    if ST.get().stop_tid != 0 {
        return;
    }
    let mut rec = [0u32; 20];
    if unsafe { crate::pm::Rs2Win16Debug(3, 0, rec.as_mut_ptr() as u32, 0, 0) } == 0 {
        return;
    }
    stopped(&rec);
}

/// A debugged task stopped: the callback with TOOLHELP's frame, then the
/// task goes on from where the callback's IRET says.
fn stopped(rec: &[u32; 20]) {
    let s = ST.get();
    if s.stop_tid != 0 {
        return;
    }
    s.stop_rec = *rec;
    s.stop_pid = rec[0];
    s.stop_task = view(rec[0]).map_or(0, |v| v.tdb);
    s.stop_waiting = false;
    s.stop_tid = u32::MAX;
    crate::w16lock::other_started();
    let mut t = 0u32;
    if unsafe { os2::DosCreateThread(&mut t, stop_thread, 0, 0, 0x2_0000) } != 0 {
        crate::w16lock::other_ended();
        ST.get().stop_tid = 0;
        unsafe { crate::pm::Rs2Win16Debug(4, rec[0], rec[1], 0, 0) };
    }
}

/// The thread a stop is handled on: the stopped task, as far as the
/// interrupt callback can tell (GetCurrentTask, its own message queue for
/// PostAppMessage to that task), running when the debugger's thread
/// waits for messages.
unsafe extern "C" fn stop_thread(_: u32) {
    crate::w16lock::acquire();
    ST.get().stop_tid = this_tid();
    let hab = unsafe { crate::pm::WinInitialize(0) };
    let hmq = unsafe { crate::pm::WinCreateMsgQueue(hab, 0) };
    ST.get().stop_hmq = hmq;
    let rec = ST.get().stop_rec;
    run_stop(&rec);
    unsafe {
        crate::pm::WinDestroyMsgQueue(hmq);
        crate::pm::WinTerminate(hab);
    }
    let s = ST.get();
    s.stop_tid = 0;
    s.stop_hmq = 0;
    s.stop_task = 0;
    crate::w16lock::other_ended();
    crate::w16lock::release();
}

fn this_tid() -> u32 {
    let mut tib: *mut os2::Tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    unsafe { tib.as_ref().and_then(|t| t.ptib2.as_ref()).map_or(0, |t2| t2.tid) }
}

/// On the stop's thread?
fn on_stop_thread() -> bool {
    let s = ST.get().stop_tid;
    s != 0 && s == this_tid()
}

/// GetMessage on the stop's thread waits (the debugger's thread runs).
pub fn stop_thread_waits(w: bool) {
    if on_stop_thread() {
        ST.get().stop_waiting = w;
    }
}

/// PostAppMessage to a task being stopped: its thread's queue.
pub fn task_queue(task: u16) -> Option<u32> {
    let s = ST.get();
    (s.stop_tid != 0 && s.stop_hmq != 0 && task != 0 && task | 7 == s.stop_task | 7).then_some(s.stop_hmq)
}

/// The PM queue of the calling thread (the stop's thread has its own).
pub fn thread_queue() -> Option<u32> {
    on_stop_thread().then(|| ST.get().stop_hmq).filter(|&q| q != 0)
}

fn run_stop(rec: &[u32; 20]) {
    let (p, tid, vector) = (rec[0], rec[1], rec[2] as u16);
    let mut regs = [0u32; 16];
    regs.copy_from_slice(&rec[4..20]);
    let Some(v) = view(p) else {
        unsafe { crate::pm::Rs2Win16Debug(4, p, tid, 0, 0) };
        return;
    };
    refresh(v);
    let (cs, ip, fl) = (regs[10] as u16, regs[8] as u16, regs[9] as u16);
    let copy_cs = translate(v, cs);
    v.csip = (copy_cs as u32) << 16 | ip as u32;
    let tdb = v.tdb;
    if crate::tracing() {
        crate::trace(format_args!("W16 debug: task {p} INT {vector} at {cs:04x}:{ip:04x} (copy {copy_cs:04x})"));
    }
    let cb = with_slot(pid(), |s| s.interrupt).unwrap_or(0);
    let mut resume: Option<[u32; 16]> = None;
    if cb != 0 && copy_cs != 0 {
        let s = ST.get();
        let _ = tdb;
        s.in_int = true;
        s.iret = None;
        PascalCall::new().w(fl).w(copy_cs).w(ip).w(0).w(vector).w(regs[0] as u16).call(cb);
        let s = ST.get();
        s.in_int = false;
        if let Some((tcs, tip, tfl)) = s.iret.take() {
            let real = view(p).map_or(0, |v| v.segs.iter().find(|x| x.1 | 7 == tcs | 7).map_or(0, |x| x.0));
            if real != 0 {
                let mut r = regs;
                r[10] = real as u32;
                r[8] = tip as u32;
                r[9] = tfl as u32;
                resume = Some(r);
            }
        }
    }
    // The task goes on once the debugger yields (Windows switches to it
    // only then), with the breakpoints the debugger set meanwhile.
    if crate::tracing() {
        crate::trace(format_args!("W16 debug: task {p} goes on at {:?} when the debugger yields", resume.map(|r| (r[10], r[8], r[9]))));
    }
    ST.get().pending.push((p, tid, resume));
}

/// A module database as Windows keeps it: the NE header and its tables
/// from the file, the segment table's entries ten bytes long with the
/// segments' handles (here the copies' selectors) at +8.
fn module_database(path: &[u8], segs: &[(u16, u16)]) -> Option<u16> {
    let file = crate::module::read_file(path, 0x4_0000)?;
    let ne = get16(file.as_ptr(), 0x3c) as usize;
    if file.len() < ne + 0x40 || &file[ne..ne + 2] != b"NE" {
        return None;
    }
    let h = &file[ne..];
    let g = |o: usize| get16(h.as_ptr(), o) as usize;
    let (nseg, segtab) = (g(0x1c), g(0x22));
    // The resident tables end with the entry table (offset 4, length 6);
    // the non-resident names are elsewhere in the file.
    let end = (g(4) + g(6)).max(g(0x2a)).min(h.len()).min(0xf000);
    let (sel, p) = new_block(0x1_0000)?;
    let grow = 2 * nseg;
    unsafe {
        core::ptr::copy_nonoverlapping(h.as_ptr(), p, segtab.min(end));
        for i in 0..nseg {
            let (src, dst) = (segtab + 8 * i, segtab + 10 * i);
            if src + 8 > end {
                break;
            }
            core::ptr::copy_nonoverlapping(h.as_ptr().add(src), p.add(dst), 8);
            put16(p, dst + 8, segs.get(i).map_or(0, |s| s.1));
        }
        let after = segtab + 8 * nseg;
        if after < end {
            core::ptr::copy_nonoverlapping(h.as_ptr().add(after), p.add(after + grow), end - after);
        }
    }
    // The tables after the segment table moved.
    for o in [0x04, 0x24, 0x26, 0x28, 0x2a] {
        let v = g(o);
        if v > segtab {
            put16(p, o, (v + grow) as u16);
        }
    }
    // The usage count.
    put16(p, 2, 1);
    Some(sel)
}

/// This process's selector for another task's `sel` (its copy).
fn translate(v: &View, sel: u16) -> u16 {
    v.segs.iter().find(|x| x.0 | 7 == sel | 7).map_or(0, |x| x.1)
}

/// A notification from another task (GetMessage met NOTIFY_MSG): the
/// callback, with GetCurrentTask answering that task.
pub fn deliver(mp1: u32, data: u32) {
    let me = pid();
    let Some(cb) = with_slot(me, |s| s.notify) else { return };
    if cb == 0 {
        return;
    }
    let (id, from) = (mp1 as u16, mp1 >> 16);
    let (task, data) = if from == me {
        (ST.get().tdb, data)
    } else {
        // The out string before the view (it may change meanwhile).
        let out = if id == NFY_OUTSTR { slot(from).map(|s| s.out) } else { None };
        let Some(v) = view(from) else { return };
        if id == NFY_EXITTASK {
            if v.exited {
                return;
            }
            v.exited = true;
        }
        let data = match id {
            // The start address in the copy of its code.
            NFY_STARTTASK => (translate(v, (data >> 16) as u16) as u32) << 16 | data & 0xffff,
            NFY_OUTSTR => {
                let s = ST.get();
                if s.out.is_null() {
                    s.out = new_block(0x1000).map_or(core::ptr::null_mut(), |b| b.1);
                }
                if s.out.is_null() {
                    return;
                }
                put_str(s.out, 0, 130, unsafe { crate::cstr(out.unwrap_or([0; 130]).as_ptr()) });
                thunk16::far(s.out)
            }
            _ => data,
        };
        (v.tdb, data)
    };
    let s = ST.get();
    let saved = s.current;
    s.current = task;
    if crate::tracing() {
        crate::trace(format_args!("W16 notify {id} {data:#x} (task {task:#x}) -> {cb:#x}"));
    }
    PascalCall::new().w(id).l(data).call(cb);
    if id == NFY_STARTTASK && from != me {
        // The debugger's breakpoint on the start goes into the task (it
        // stops there itself), or, not debugged, is played here.
        match view(from) {
            Some(v) if v.attached => sync(v),
            _ => entry_breakpoint(data),
        }
        with_slot(from, |s| s.go = 1);
    }
    ST.get().current = saved;
}

/// INT 3 trapped in another task: the interrupt callback is called with
/// TOOLHELP's frame (from SP up: its return address, AX, the interrupt
/// number, a handle, IP, CS, FLAGS of the task, here the copy's CS).
/// It returns (RETF) to pass it on, or IRETs to resume the task: the
/// IRET to a copy (a data selector) faults, and `iret_trap` makes it the
/// callback's return.
fn int3(cs: u16, ip: u16) {
    let Some(cb) = with_slot(pid(), |s| s.interrupt).filter(|&c| c != 0) else { return };
    if crate::tracing() {
        crate::trace(format_args!("W16 INT 3 at {cs:04x}:{ip:04x} -> {cb:#x}"));
    }
    ST.get().in_int = true;
    PascalCall::new().w(0x0202).w(cs).w(ip).w(0).w(3).w(0).call(cb);
    ST.get().in_int = false;
}

/// The debugger put a breakpoint (INT 3) on the start of a task it
/// heard of: the task stops there, as it would on Windows.
fn entry_breakpoint(start: u32) {
    let (cs, ip) = ((start >> 16) as u16, start as u16);
    if cs == 0 || selector(cs).is_none_or(|(_, l)| ip as u32 > l) {
        return;
    }
    let b = unsafe { *flat(start) };
    if b == 0xcc {
        int3(cs, ip.wrapping_add(1));
    }
}

/// A protection fault in this task (the interrupt handler's INTREGS): an
/// interrupt callback's IRET to another task's copy is its return
/// (resume), back to where `int3` called it. True if it was that.
pub fn iret_trap(r: &mut crate::dos::Regs) -> bool {
    use crate::dos::reg::*;
    if !ST.get().in_int || (ST.get().stop_tid != 0 && !on_stop_thread()) {
        return false;
    }
    let ss = (thunk16::sp16() >> 16) as u16;
    let at = flat((r[CS] as u32) << 16 | r[IP] as u32);
    if at.is_null() || unsafe { *at } != 0xcf {
        return false;
    }
    let st = |o: u16| read16(ss, r[SP].wrapping_add(o)).unwrap_or(0);
    let (tip, tcs, tfl) = (st(0), st(2), st(4));
    if view_block(tcs).is_none() {
        return false;
    }
    ST.get().iret = Some((tcs, tip, tfl));
    if crate::tracing() {
        crate::trace(format_args!("W16 INT 3 handler resumes {tcs:04x}:{tip:04x}"));
    }
    if let Some(v) = ST.get().views.iter_mut().find(|v| v.segs.iter().any(|s| s.1 | 7 == tcs | 7)) {
        v.csip = (tcs as u32) << 16 | tip as u32;
    }
    // Back to int3's call, as the callback's own far return would.
    let back = thunk16::callback_return();
    r[IP] = back as u16;
    r[CS] = (back >> 16) as u16;
    r[SP] = r[SP].wrapping_add(6);
    r[AX] = 1;
    true
}

/// WinExec, LoadModule: wait (a while) for the Windows program just
/// started as process `child` to reach InitTask, and deliver the
/// notifications it sent: its instance handle (None if it does not
/// start). A debugger's view of it is made then.
pub fn wait_started(child: u32) -> Option<u16> {
    let mut found = false;
    for _ in 0..250 {
        if slot(child).is_some() {
            found = true;
            break;
        }
        // A program that ended is not waited for.
        let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
        let mut pid = 0u32;
        if unsafe { os2::DosWaitChild(0, 1, &mut res, &mut pid, child) } == 0 {
            return None;
        }
        unsafe { os2::DosSleep(20) };
    }
    if !found {
        return None;
    }
    // Its NFY_STARTTASK, before the caller goes on (Windows runs the new
    // task's start inside LoadModule).
    if with_slot(pid(), |s| s.notify).unwrap_or(0) != 0 {
        let hab = crate::kernel::TASK.get().hab;
        for _ in 0..50 {
            let mut q = crate::pm::QMsg::default();
            if unsafe { crate::pm::WinPeekMsg(hab, &mut q, 0, NOTIFY_MSG, NOTIFY_MSG, 1) } != 0 {
                deliver(q.mp1, q.mp2);
                break;
            }
            unsafe { os2::DosSleep(20) };
        }
        let t = slot(child)?;
        let v = view(child)?;
        let inst = translate(v, t.hinst);
        // A debugged task runs to the debugger's breakpoint on its start
        // inside LoadModule too: that stop is given before it returns.
        if v.attached {
            for _ in 0..250 {
                let mut rec = [0u32; 20];
                if unsafe { crate::pm::Rs2Win16Debug(3, child, rec.as_mut_ptr() as u32, 0, 0) } != 0 {
                    stopped(&rec);
                    // Its handling runs (on its thread) until the task goes
                    // on or waits for the debugger.
                    for _ in 0..500 {
                        let s = ST.get();
                        if s.stop_tid == 0 || s.stop_waiting {
                            break;
                        }
                        crate::w16lock::waiting(|| unsafe { os2::DosSleep(10) });
                    }
                    break;
                }
                if slot(child).is_none() {
                    break;
                }
                unsafe { os2::DosSleep(20) };
            }
        }
        return Some(inst);
    }
    slot(child).map(|t| t.hinst)
}

/// Make GetCurrentTask answer another task (0: this one): the old value.
pub fn set_current(t: u16) -> u16 {
    // (The stop's thread stays the stopped task.)
    if on_stop_thread() {
        return t;
    }
    core::mem::replace(&mut ST.get().current, t)
}

/// GetCurrentTask.
pub fn current_task() -> u16 {
    let s = ST.get();
    if on_stop_thread() && s.stop_task != 0 {
        s.stop_task
    } else if s.current != 0 {
        s.current
    } else if s.tdb != 0 {
        s.tdb
    } else {
        crate::kernel::TASK.get().hinst
    }
}

/// This task's process.
pub fn own_pid() -> u32 {
    pid()
}

/// This task's TDB.
pub fn own_tdb() -> u16 {
    ST.get().tdb
}

/// The view of the task with this handle.
fn view_of(h: u16) -> Option<&'static mut View> {
    let s = ST.get();
    let i = s.views.iter().position(|v| v.tdb | 7 == h | 7 && !v.exited)?;
    s.views.get_mut(i)
}

/// Is `h` a task handle (IsTask)?
pub fn is_task(h: u16) -> bool {
    h != 0 && (h | 7 == own_tdb() | 7 || view_of(h).is_some_and(|v| slot(v.pid).is_some()))
}

/// The process of a task handle.
pub fn task_pid(h: u16) -> Option<u32> {
    if h | 7 == own_tdb() | 7 {
        return Some(pid());
    }
    view_of(h).map(|v| v.pid)
}

/// A structure argument whose dwSize must be at least `n`.
fn sized(p: *mut u8, n: u32) -> bool {
    !p.is_null() && unsafe { (p as *const u32).read_unaligned() } >= n
}

/// The tasks in TaskFirst/TaskNext's order: (task handle, process).
fn task_list() -> Vec<(u16, u32)> {
    let me = pid();
    let mut v = Vec::new();
    for t in tasks() {
        if t.pid == me {
            v.push((own_tdb(), me));
        } else if let Some(w) = view(t.pid) {
            v.push((w.tdb, t.pid));
        }
    }
    v
}

/// TASKENTRY: dwSize, hTask, hTaskParent, hInst, hModule, wSS, wSP,
/// wStackTop, wStackMinimum, wStackBottom, wcEvents, hQueue,
/// szModule[10], wPSPOffset, hNext.
fn fill_task(p: *mut u8, h: u16, pid_: u32, next: u16) -> u32 {
    let Some(t) = slot(pid_) else { return 0 };
    let (inst, module, ss) = if pid_ == pid() {
        (t.hinst, t.hinst, t.ss)
    } else {
        let Some(v) = view(pid_) else { return 0 };
        (translate(v, t.hinst), v.moddb, translate(v, t.ss))
    };
    put16(p, 4, h);
    put16(p, 6, 0);
    put16(p, 8, inst);
    put16(p, 10, module);
    put16(p, 12, ss);
    put16(p, 14, t.sp);
    put16(p, 16, t.stack_top);
    put16(p, 18, t.stack_min);
    put16(p, 20, t.stack_bottom);
    put16(p, 22, 0);
    put16(p, 24, (t.hmq >> 16) as u16 | 7);
    put_str(p, 26, 10, unsafe { crate::cstr(t.name.as_ptr()) });
    put16(p, 36, 0x100);
    put16(p, 38, next);
    1
}

fn task_at(p: *mut u8, i: usize) -> u32 {
    let ts = task_list();
    let Some(&(h, pid_)) = ts.get(i) else { return 0 };
    fill_task(p, h, pid_, ts.get(i + 1).map_or(0, |n| n.0))
}

/// MODULEENTRY: dwSize, szModule[10], hModule, wcUsage, szExePath[256],
/// wNext.
struct ModInfo {
    handle: u16,
    name: Vec<u8>,
    path: Vec<u8>,
}

fn modules() -> Vec<ModInfo> {
    let mut v: Vec<ModInfo> = crate::module::list().into_iter().map(|(h, name, path)| ModInfo { handle: h, name, path }).collect();
    let me = pid();
    for t in tasks() {
        if t.pid != me {
            if let Some(w) = view(t.pid).filter(|w| w.moddb != 0) {
                v.push(ModInfo { handle: w.moddb, name: w.name.clone(), path: w.path.clone() });
            }
        }
    }
    for (name, h, _) in crate::module::SYSTEM {
        let mut path = crate::profile::SYSTEM_DIR.to_vec();
        path.push(b'\\');
        path.extend_from_slice(name);
        path.extend_from_slice(if *name == b"KERNEL" { b".EXE" } else { b".DLL" });
        v.push(ModInfo { handle: *h, name: name.to_vec(), path });
    }
    v
}

fn fill_module(p: *mut u8, ms: &[ModInfo], i: usize) -> u32 {
    let Some(m) = ms.get(i) else { return 0 };
    put_str(p, 4, 10, &m.name);
    put16(p, 14, m.handle);
    put16(p, 16, 1);
    put_str(p, 18, 256, &m.path);
    // wNext: the index of the next entry, plus one (0: the last).
    put16(p, 274, if i + 1 < ms.len() { i as u16 + 2 } else { 0 });
    1
}

/// The view a selector of this process belongs to (a copy, a module
/// database or a TDB of another task's), and what it is: GT_TASK,
/// GT_MODULE, or the segment number.
fn view_block(h: u16) -> Option<(&'static View, u16, u16)> {
    let s = ST.get();
    for v in s.views.iter() {
        if v.tdb | 7 == h | 7 {
            return Some((v, 4, 0));
        }
        if v.moddb | 7 == h | 7 && v.moddb != 0 {
            return Some((v, 6, 0));
        }
        if let Some(i) = v.segs.iter().position(|x| x.1 | 7 == h | 7) {
            return Some((v, 3, i as u16 + 1));
        }
    }
    None
}

/// A selector's base and limit here (None if it is not valid).
fn selector(sel: u16) -> Option<(u32, u32)> {
    let base = unsafe { crate::pm::Rs2Win16Selector(1, sel as u32, 0) };
    let limit = unsafe { crate::pm::Rs2Win16Selector(5, sel as u32, 0) };
    (sel & 4 != 0 && base != u32::MAX && limit != u32::MAX).then_some((base, limit))
}

/// GLOBALENTRY: dwSize, dwAddress, dwBlockSize, hBlock, wcLock,
/// wcPageLock, wFlags, wHeapPresent, hOwner, wType, wData, dwNext,
/// dwNextAlt.
fn fill_global(p: *mut u8, h: u16, next: u32) -> bool {
    let Some((base, limit)) = selector(h | 7) else { return false };
    let tdb = ST.get().tdb;
    let inst = crate::kernel::TASK.get().hinst;
    // A block of another task's view.
    if let Some((v, ty, seg)) = view_block(h) {
        put32(p, 4, base);
        put32(p, 8, limit + 1);
        put16(p, 12, h | 7);
        put16(p, 14, 0);
        put16(p, 16, 0);
        put16(p, 18, 0);
        put16(p, 20, 0);
        put16(p, 22, if ty == 4 { v.tdb } else { v.moddb });
        put16(p, 24, ty);
        put16(p, 26, seg);
        put32(p, 28, next);
        put32(p, 32, 0);
        return true;
    }
    let ty = if h | 7 == tdb | 7 {
        4 // GT_TASK
    } else if h | 7 == inst | 7 {
        1 // GT_DGROUP
    } else if crate::module::find(h).is_some() {
        3 // GT_CODE (a module's segment)
    } else {
        2 // GT_DATA
    };
    put32(p, 4, base);
    put32(p, 8, limit + 1);
    put16(p, 12, h);
    put16(p, 14, crate::kernel::lock_count(h));
    put16(p, 16, 0);
    put16(p, 18, 0);
    put16(p, 20, (ty == 1) as u16);
    put16(p, 22, if ty == 4 { tdb } else { crate::module::exe() });
    put16(p, 24, ty);
    put16(p, 26, 0);
    put32(p, 28, next);
    put32(p, 32, 0);
    true
}

/// The global blocks GlobalFirst/GlobalNext walk: the task's TDB,
/// DGROUP and its GlobalAlloc blocks.
fn globals() -> Vec<u16> {
    let mut v = Vec::new();
    let s = ST.get();
    if s.tdb != 0 {
        v.push(s.tdb);
    }
    v.push(crate::kernel::TASK.get().hinst);
    v.extend(crate::kernel::global_handles());
    v.dedup();
    v
}

fn global_at(p: *mut u8, i: usize) -> u32 {
    let g = globals();
    let Some(&h) = g.get(i) else { return 0 };
    fill_global(p, h, if i + 1 < g.len() { i as u32 + 1 } else { 0 }) as u32
}

/// STACKTRACEENTRY: dwSize, hTask, wSS, wBP, wCS, wIP, hModule, wSegment,
/// wFlags. A frame from BP: the caller's BP at [BP], its return address
/// above (far frames have an odd BP, as Windows' prologues make them).
fn fill_frame(p: *mut u8, ss: u16, bp: u16, cs: u16, ip: u16) {
    put16(p, 4, current_task());
    put16(p, 6, ss);
    put16(p, 8, bp);
    put16(p, 10, cs);
    put16(p, 12, ip);
    let hmod = unsafe { crate::pm::Rs2Win16Selector(8, cs as u32, 0) };
    let module = if hmod != 0 { crate::module::find(cs).map_or(0, |m| m.hinst) } else { 0 };
    put16(p, 14, module);
    let mut seg = 0;
    if hmod != 0 {
        let n = unsafe { crate::pm::Rs2Win16Selector(6, hmod, 0) };
        for i in 1..=n {
            if unsafe { crate::pm::Rs2Win16Selector(6, hmod, i) } as u16 | 7 == cs | 7 {
                seg = i as u16;
            }
        }
    }
    put16(p, 16, seg);
    put16(p, 18, 0);
}

fn read16(ss: u16, off: u16) -> Option<u16> {
    let (base, limit) = selector(ss)?;
    if off as u32 + 1 > limit {
        return None;
    }
    Some(unsafe { ((base as usize + off as usize) as *const u16).read_unaligned() })
}

fn stack_next(p: *mut u8) -> u32 {
    let g = |o| unsafe { (p.add(o) as *const u16).read_unaligned() };
    let (ss, bp, cs) = (g(6), g(8), g(10));
    let bp0 = bp & !1;
    if bp0 == 0 {
        return 0;
    }
    let Some(next_bp) = read16(ss, bp0) else { return 0 };
    let Some(ip) = read16(ss, bp0 + 2) else { return 0 };
    let far = next_bp & 1 != 0 || bp & 1 != 0;
    let ncs = if far { read16(ss, bp0 + 4).unwrap_or(cs) } else { cs };
    if next_bp & !1 <= bp0 && next_bp != 0 {
        return 0;
    }
    fill_frame(p, ss, next_bp, ncs, ip);
    1
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // GlobalHandleToSel(h)
        6250 => {
            let h = a.w();
            if h == 0 { 0 } else { (h | 7) as u32 }
        }
        // GlobalFirst(lpge, flags), GlobalNext(lpge, flags)
        6251 | 6252 => {
            let (p, _fl) = (a.p(), a.w());
            if !sized(p, 36) {
                return Some(0);
            }
            let i = if ord == 6251 { 0 } else { (unsafe { (p.add(28) as *const u32).read_unaligned() }) as usize };
            if ord == 6252 && i == 0 {
                return Some(0);
            }
            global_at(p, i)
        }
        // GlobalInfo(lpgi: dwSize, wcItems, wcItemsFree, wcItemsLRU)
        6253 => {
            let p = a.p();
            if !sized(p, 10) {
                return Some(0);
            }
            put16(p, 4, globals().len() as u16);
            put16(p, 6, 0);
            put16(p, 8, 0);
            1
        }
        // GlobalEntryHandle(lpge, h)
        6254 => {
            let (p, h) = (a.p(), a.w());
            if !sized(p, 36) || h == 0 {
                return Some(0);
            }
            fill_global(p, h, 0) as u32
        }
        // GlobalEntryModule(lpge, hModule, wSeg): a module's segment.
        6255 => {
            let (p, hm, seg) = (a.p(), a.w(), a.w());
            if !sized(p, 36) {
                return Some(0);
            }
            let other = ST.get().views.iter().find(|v| v.moddb | 7 == hm | 7 && v.moddb != 0).map(|v| v.segs.get(seg as usize - 1).map_or(0, |s| s.1));
            let sel = match other {
                Some(s) => s,
                None => crate::module::find(hm).map_or(0, |m| unsafe { crate::pm::Rs2Win16Selector(6, m.hmod, seg as u32) } as u16),
            };
            if sel == 0 || seg == 0 { 0 } else { fill_global(p, sel, 0) as u32 }
        }
        // LocalInfo, LocalFirst, LocalNext: the local heaps are not
        // walked.
        6256 | 6257 => {
            a.p();
            a.w();
            0
        }
        6258 => {
            a.p();
            0
        }
        // ModuleFirst(lpme), ModuleNext(lpme)
        6259 | 6260 => {
            let p = a.p();
            if !sized(p, 276) {
                return Some(0);
            }
            let i = if ord == 6259 { 0 } else { (unsafe { (p.add(274) as *const u16).read_unaligned() }) as usize };
            if ord == 6260 && i == 0 {
                return Some(0);
            }
            fill_module(p, &modules(), i.saturating_sub((ord == 6260) as usize))
        }
        // ModuleFindName(lpme, name), ModuleFindHandle(lpme, h)
        6261 => {
            let (p, n) = (a.p(), a.l());
            let name = far_str(n).unwrap_or(b"").to_ascii_uppercase();
            let ms = modules();
            match ms.iter().position(|m| m.name == name) {
                Some(i) if sized(p, 276) => fill_module(p, &ms, i),
                _ => 0,
            }
        }
        6262 => {
            let (p, h) = (a.p(), a.w());
            let ms = modules();
            match ms.iter().position(|m| m.handle | 7 == h | 7) {
                Some(i) if sized(p, 276) && h != 0 => fill_module(p, &ms, i),
                _ => 0,
            }
        }
        // TaskFirst(lpte), TaskNext(lpte), TaskFindHandle(lpte, hTask)
        6263 => {
            let p = a.p();
            if !sized(p, 40) { 0 } else { task_at(p, 0) }
        }
        6264 => {
            let p = a.p();
            if !sized(p, 40) {
                return Some(0);
            }
            let next = get16(p, 38);
            match task_list().iter().position(|t| t.0 == next) {
                Some(i) if next != 0 => task_at(p, i),
                _ => 0,
            }
        }
        6265 => {
            let (p, h) = (a.p(), a.w());
            if !sized(p, 40) || h == 0 {
                return Some(0);
            }
            match task_list().iter().position(|t| t.0 | 7 == h | 7) {
                Some(i) => task_at(p, i),
                None => 0,
            }
        }
        // StackTraceFirst(lpste, hTask): another task's stack is not
        // reachable; this one's starts at the caller.
        6266 => {
            let (p, h) = (a.p(), a.w());
            if !sized(p, 20) || (h != 0 && h | 7 != current_task() | 7) {
                return Some(0);
            }
            let ss = (thunk16::sp16() >> 16) as u16;
            let frame = flat(thunk16::sp16().wrapping_add(32)) as *const u16;
            // The entry stub saved (fs, bx, di, si, es, ds, bp) below the
            // entry's near call to it, then the caller's return address.
            let (bp, ip, cs) = unsafe { (frame.add(6).read_unaligned(), frame.add(8).read_unaligned(), frame.add(9).read_unaligned()) };
            fill_frame(p, ss, bp, cs, ip);
            1
        }
        // StackTraceCSIPFirst(lpste, wSS, wCS, wIP, wBP)
        6267 => {
            let (p, ss, cs, ip, bp) = (a.p(), a.w(), a.w(), a.w(), a.w());
            if !sized(p, 20) || selector(ss).is_none() {
                return Some(0);
            }
            fill_frame(p, ss, bp, cs, ip);
            1
        }
        // StackTraceNext(lpste)
        6268 => {
            let p = a.p();
            if !sized(p, 20) { 0 } else { stack_next(p) }
        }
        // ClassFirst(lpce), ClassNext(lpce): dwSize, hInst, szClassName[256],
        // wNext.
        6269 | 6270 => {
            let p = a.p();
            if !sized(p, 264) {
                return Some(0);
            }
            let cs = crate::user::class_list();
            let i = if ord == 6269 { 0 } else { (unsafe { (p.add(262) as *const u16).read_unaligned() }) as usize };
            if ord == 6270 && i == 0 {
                return Some(0);
            }
            let i = i.saturating_sub((ord == 6270) as usize);
            let Some((name, inst)) = cs.get(i) else { return Some(0) };
            put16(p, 4, *inst);
            put_str(p, 6, 256, name);
            put16(p, 262, if i + 1 < cs.len() { i as u16 + 2 } else { 0 });
            1
        }
        // SystemHeapInfo(lpshi: dwSize, wUserFreePercent, wGDIFreePercent,
        // hUserSegment, hGDISegment)
        6271 => {
            let p = a.p();
            if !sized(p, 12) {
                return Some(0);
            }
            put16(p, 4, 90);
            put16(p, 6, 90);
            put16(p, 8, crate::module::SYSTEM[1].1);
            put16(p, 10, crate::module::SYSTEM[2].1);
            1
        }
        // MemManInfo(lpmmi): rs2's memory in 4 KB pages.
        6272 => {
            let p = a.p();
            if !sized(p, 42) {
                return Some(0);
            }
            let mut total = 0u32;
            unsafe { os2::DosQuerySysInfo(17, 17, &mut total, 4) };
            let pages = (total / 4096).max(1024);
            for (o, v) in [(4, total / 2), (8, pages / 2), (12, pages / 2), (16, pages * 4), (20, pages / 2), (24, pages / 2), (28, pages), (32, pages * 2), (36, 0)] {
                put32(p, o, v);
            }
            put16(p, 40, 4096);
            1
        }
        // MemoryRead(sel, offset, buf, n), MemoryWrite: this process's
        // selectors (another task's memory is out of reach).
        6278 | 6279 => {
            let (sel, off, buf, n) = (a.w(), a.l(), a.p(), a.l());
            let Some((base, limit)) = selector(sel) else { return Some(0) };
            if off > limit || buf.is_null() {
                return Some(0);
            }
            let n = n.min(limit - off + 1);
            // The selector's own base (aliases are not tiled).
            let p = base as usize + off as usize;
            // Only memory that may be written (a debugger patching an API
            // entry would write into WIN16's code).
            if ord == 6279 {
                let (mut size, mut fl) = (n, 0u32);
                if unsafe { os2::DosQueryMem(p as *mut u8, &mut size, &mut fl) } != 0 || fl & os2::PAG_WRITE == 0 || size < n {
                    return Some(0);
                }
            }
            unsafe {
                if ord == 6278 {
                    core::ptr::copy(p as *const u8, buf, n as usize);
                } else {
                    core::ptr::copy(buf as *const u8, p as *mut u8, n as usize);
                }
            }
            n
        }
        // TaskSetCSIP(hTask, cs, ip), TaskGetCSIP(hTask), TaskSwitch(hTask,
        // csip): tasks are processes, their registers out of reach.
        6281 => {
            let (t, cs, ip) = (a.w(), a.w(), a.w());
            match view_of(t) {
                Some(v) => core::mem::replace(&mut v.csip, (cs as u32) << 16 | ip as u32),
                None => 0,
            }
        }
        6282 => {
            let t = a.w();
            view_of(t).map_or(0, |v| v.csip)
        }
        // TaskSwitch: the task goes on (from where it is: the copy's
        // address is not where its code is).
        6283 => {
            let (t, csip) = (a.w(), a.l());
            let Some(v) = view_of(t) else { return Some(0) };
            v.csip = csip;
            let p = v.pid;
            with_slot(p, |s| s.go = 1).is_some() as u32
        }
        // NotifyRegister(hTask, lpfn, flags), NotifyUnRegister(hTask)
        6273 => {
            let (_t, f, fl) = (a.w(), a.l(), a.w());
            let me = pid();
            if own_tdb() == 0 {
                return Some(0);
            }
            with_slot(me, |s| {
                s.notify = f;
                s.notify_flags = fl;
            })
            .is_some() as u32
        }
        6274 => {
            a.w();
            with_slot(pid(), |s| s.notify = 0);
            1
        }
        // InterruptRegister(hTask, lpfn), InterruptUnRegister(hTask)
        6275 => {
            let (_t, f) = (a.w(), a.l());
            with_slot(pid(), |s| s.interrupt = f);
            1
        }
        6276 => {
            a.w();
            with_slot(pid(), |s| s.interrupt = 0);
            1
        }
        // TerminateApp(hTask, flags): another task is ended (its process);
        // this one ends.
        6277 => {
            let (t, _f) = (a.w(), a.w());
            match task_pid(t).filter(|&p| t != 0 && p != pid()) {
                Some(p) => {
                    unsafe { os2::DosKillProcess(1, p) };
                    // It may not say so itself: NFY_EXITTASK for it here,
                    // and it leaves the registry.
                    deliver(NFY_EXITTASK as u32 | p << 16, 0xff);
                    with_slot(p, |s| s.pid = 0);
                    1
                }
                None => {
                    task_exiting(0xff);
                    unsafe { os2::DosExit(1, 0xff) };
                    0
                }
            }
        }
        _ => return None,
    })
}
