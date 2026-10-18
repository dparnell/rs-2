//! Processes.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use super::thread::Thread;
use super::{sched, WaitQueue};
use crate::arch::gdt::{TIB_SEL, USER_CS, USER_DS};
use crate::arch::idt::TrapFrame;
use crate::fs::Handle;
use crate::loader::{self, Module};
use crate::sync::SpinLock;
use crate::vm::{self, pag, AddressSpace, SegKind};

/// Offsets in the per-process information area.
pub mod info {
    /// Process information block.
    pub const PIB: u32 = 0;
    /// Thread information blocks: TIB at `TIB0 + n * TIB_STRIDE`, TIB2 at
    /// `+ TIB2`.
    pub const TIB0: u32 = 0x100;
    pub const TIB_STRIDE: u32 = 0x40;
    pub const TIB2: u32 = 0x20;
    pub const MAX_THREADS: u32 = 0x3fc;
    /// Local information segment (16-bit), in its own tile.
    pub const LINFOSEG: u32 = 0x1_0000;
    pub const SIZE: u32 = 0x2_0000;
}

/// Why and how a process ended (for DosCWait / DosWaitChild).
#[derive(Clone, Copy, Debug)]
pub struct ExitInfo {
    /// TC_EXIT 0, TC_HARDERROR 1, TC_TRAP 2, TC_KILLPROCESS 3.
    pub cause: u32,
    pub code: u32,
}

pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub space: SpinLock<AddressSpace>,
    pub exe: Arc<Module>,
    pub threads: SpinLock<Vec<Arc<Thread>>>,
    next_tid: AtomicU32,
    pub handles: SpinLock<Vec<Option<Handle>>>,
    /// Handle slots marked OPEN_FLAGS_NOINHERIT (a per-handle property).
    pub noinherit: SpinLock<alloc::collections::BTreeSet<u32>>,
    /// Linear addresses of the environment and command line.
    pub env: u32,
    pub cmdline: u32,
    pub info: u32,
    /// Current drive (0 = A:) and current directory per drive (no drive
    /// letter, no leading backslash).
    pub cwd: SpinLock<(u8, [String; 26])>,
    pub exit: SpinLock<Option<ExitInfo>>,
    pub exit_waiters: WaitQueue,
    /// Woken when a child of this process ends.
    pub child_waiters: WaitQueue,
    /// Woken when one of this process's threads ends, resumes or the
    /// critical section is released.
    pub thread_waiters: WaitQueue,
    /// BeginLIBPATH, EndLIBPATH, LIBPATHSTRICT.
    pub ext_libpath: SpinLock<(String, String, bool)>,
    /// DLLs with per-process initialisation that have run it here.
    pub inited: SpinLock<Vec<u16>>,
    /// Exit list routines: (order, address, 16-bit).
    pub exitlist: SpinLock<Vec<(u8, u32, bool)>>,
    /// Thread running the exit list (0 = none) and whether it should
    /// proceed to the next routine at its next return to user mode.
    pub exitlist_tid: AtomicU32,
    pub exitlist_next: core::sync::atomic::AtomicBool,
    /// Critical section owner (tid, 0 = none) and nesting count.
    pub critsec: SpinLock<(u32, u32)>,
    /// Started with EXEC_ASYNC: nobody will collect the result.
    pub detached: core::sync::atomic::AtomicBool,
    /// Modules loaded with DosLoadModule (with use counts).
    pub loaded: SpinLock<Vec<(Arc<Module>, u32)>>,
    /// Logical video buffer (VioGetBuf), 0 if none.
    pub lvb: SpinLock<u32>,
    /// Signal waiting for delivery to thread 1.
    pub pending_signal: SpinLock<Option<u32>>,
    /// 16-bit signal handlers: (far handler, action) per signal number.
    pub signals: SpinLock<[(u32, u32); 8]>,
    /// Open directory searches (DosFindFirst handles).
    pub searches: SpinLock<alloc::collections::BTreeMap<u32, crate::api::file::Search>>,
}

static NEXT_PID: AtomicU32 = AtomicU32::new(1);
static PROCESSES: SpinLock<Vec<Arc<Process>>> = SpinLock::new(Vec::new());

pub fn by_pid(pid: u32) -> Option<Arc<Process>> {
    PROCESSES.lock().iter().find(|p| p.pid == pid).cloned()
}

pub fn all() -> Vec<Arc<Process>> {
    PROCESSES.lock().clone()
}

/// Build the environment block: `VAR=VALUE\0...\0` followed by the command
/// line `program\0arguments\0\0`. Returns the block and the offset of the
/// command line.
fn env_block(env: &[String], path: &str, program: &str, args: &str) -> (Vec<u8>, u32) {
    let mut b = Vec::new();
    for e in env {
        b.extend_from_slice(e.as_bytes());
        b.push(0);
    }
    if env.is_empty() {
        b.push(0);
    }
    b.push(0);
    // OS/2 puts the program's full path between the environment and the
    // command line; CMD.EXE finds the end of the environment by walking
    // back over it from the command line.
    b.extend_from_slice(path.as_bytes());
    b.push(0);
    let cmd = b.len() as u32;
    b.extend_from_slice(program.as_bytes());
    b.push(0);
    b.extend_from_slice(args.as_bytes());
    b.push(0);
    b.push(0);
    (b, cmd)
}

pub struct ExecParams<'a> {
    pub path: &'a str,
    /// Program name as given (argv[0]).
    pub program: &'a str,
    pub args: &'a str,
    pub env: Vec<String>,
    pub parent: Option<Arc<Process>>,
    pub handles: Vec<Option<Handle>>,
    pub detached: bool,
}

pub fn exec(p: ExecParams<'_>) -> Result<Arc<Process>, u32> {
    let data = crate::fs::read_file(p.path).ok_or(loader::err::FILE_NOT_FOUND)?;
    let mut space = AddressSpace::new().ok_or(loader::err::NOT_ENOUGH_MEMORY)?;
    let exe = loader::load_exe(p.path, data, &mut space)?;
    let start = exe.start.ok_or(loader::err::INVALID_MODULETYPE)?;

    // Environment (its own tile so 16-bit code gets a selector for it).
    let (block, cmd_off) = env_block(&p.env, p.path, p.program, p.args);
    let env = space
        .alloc(block.len() as u32, pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None)
        .map_err(|e| e.code())?;
    space.poke_bytes(env, &block);
    // The segment is exactly as long as the block, as on OS/2: CMD.EXE
    // sizes the environment with DosSizeSeg and keeps the command line at
    // the end of the segment when it adds variables.
    space.set_seg_limit(env, block.len() as u32 - 1);

    // Information area: PIB, TIBs, local infoseg.
    let info = space
        .alloc(info::SIZE, pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None)
        .map_err(|e| e.code())?;
    let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let ppid = p.parent.as_ref().map_or(0, |q| q.pid);
    let pib = [pid, ppid, exe.hmte as u32, env + cmd_off, env, 0, exe_type(&exe)];
    write_u32s(&mut space, info + info::PIB, &pib);

    // Stack.
    let (stack_flat, stack_sel, stack_off) = if start.stack.flat != 0 || start.stack.sel != 0 {
        (start.stack.flat, start.stack.sel, start.stack.off)
    } else {
        // No stack object: allocate one (32-bit programs).
        let size = 0x10000;
        let base = space
            .alloc(size, pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, None)
            .map_err(|e| e.code())?;
        (base + size, vm::tile_selector(base + size - 1), 0)
    };

    let mut frame = TrapFrame { rflags: 0x202, ..Default::default() };
    let hmte = exe.hmte as u32;
    if start.entry.is16 {
        let env_sel = vm::tile_selector(env);
        frame.cs = start.entry.sel as u64;
        frame.rip = start.entry.off as u64;
        frame.ss = stack_sel as u64;
        frame.rsp = stack_off as u64;
        frame.ds = start.autodata.map_or(0, |a| exe.objects[a].selector()) as u64;
        frame.es = 0;
        frame.rax = env_sel as u64;
        frame.rbx = cmd_off as u64;
        frame.rcx = (start.autodata_size & 0xffff) as u64;
    } else {
        // [esp] = return address (exit stub), hmod, 0, env, cmdline.
        let sp = (stack_flat - 20) & !3;
        let exit_stub = loader::gate::slot_address(0, 0);
        write_u32s(&mut space, sp, &[exit_stub, hmte, 0, env, env + cmd_off]);
        frame.cs = USER_CS as u64;
        frame.rip = start.entry.flat as u64;
        frame.ss = USER_DS as u64;
        frame.ds = USER_DS as u64;
        frame.es = USER_DS as u64;
        frame.fs = TIB_SEL as u64;
        frame.rsp = sp as u64;
    }

    let mut handles = p.handles;
    if handles.is_empty() {
        handles = alloc::vec![Some(Handle::Con), Some(Handle::Con), Some(Handle::Con)];
    }
    let cwd = match &p.parent {
        Some(q) => q.cwd.lock().clone(),
        None => (2, Default::default()),
    };
    let proc = Arc::new(Process {
        pid,
        ppid,
        space: SpinLock::new(space),
        exe: exe.clone(),
        threads: SpinLock::new(Vec::new()),
        next_tid: AtomicU32::new(1),
        handles: SpinLock::new(handles),
        noinherit: SpinLock::new(alloc::collections::BTreeSet::new()),
        env,
        cmdline: env + cmd_off,
        info,
        cwd: SpinLock::new(cwd),
        exit: SpinLock::new(None),
        exit_waiters: WaitQueue::new(),
        child_waiters: WaitQueue::new(),
        thread_waiters: WaitQueue::new(),
        critsec: SpinLock::new((0, 0)),
        inited: SpinLock::new(Vec::new()),
        ext_libpath: SpinLock::new(match &p.parent {
            Some(q) => q.ext_libpath.lock().clone(),
            None => (String::new(), String::new(), false),
        }),
        exitlist: SpinLock::new(Vec::new()),
        exitlist_tid: AtomicU32::new(0),
        exitlist_next: core::sync::atomic::AtomicBool::new(false),
        detached: core::sync::atomic::AtomicBool::new(p.detached),
        loaded: SpinLock::new(Vec::new()),
        searches: SpinLock::new(alloc::collections::BTreeMap::new()),
        lvb: SpinLock::new(0),
        signals: SpinLock::new([(0, 0); 8]),
        pending_signal: SpinLock::new(None),
    });
    let stack_bottom = stack_flat.saturating_sub(start.stack.flat.min(0x10000).max(0x1000));
    let t = proc.new_thread(frame, stack_bottom, stack_flat)?;
    {
        let mut sp = proc.space.lock();
        write_linfoseg(&proc, &mut sp, &start, &exe, &t, env, cmd_off);
    }
    // DLL initialisation runs on thread 1 before the entry point.
    let calls = crate::api::module::init_order(core::slice::from_ref(&exe), &proc);
    crate::api::module::start(&proc, &t, t.user_frame(), calls, true, None);
    PROCESSES.lock().push(proc.clone());
    sched::start(t);
    Ok(proc)
}

fn exe_type(exe: &Module) -> u32 {
    // PIB type: 0 full screen, 2 VIO window, 3 PM, 4 detached.
    match exe.app_type {
        3 => 3,
        2 => 2,
        _ => 0,
    }
}

fn write_u32s(space: &mut AddressSpace, at: u32, v: &[u32]) {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    space.poke_bytes(at, &b);
}

fn write_linfoseg(
    p: &Process,
    space: &mut AddressSpace,
    start: &loader::Start,
    exe: &Module,
    t: &Thread,
    env: u32,
    cmd_off: u32,
) {
    // LINFOSEG: pidCurrent, pidParent, prtyCurrent, tidCurrent, sgCurrent,
    // rfProcStatus, dummy1, fForeground, typeProcess, dummy2,
    // selEnvironment, offCmdLine, cbDataSegment, cbStack, cbHeap, hmod,
    // selDS.
    let mut b = Vec::new();
    for w in [p.pid as u16, p.ppid as u16, 0x200, t.tid as u16, 1] {
        b.extend_from_slice(&w.to_le_bytes());
    }
    b.extend_from_slice(&[0, 0, 1, exe_type(exe) as u8, 0]);
    // Align: selEnvironment at offset 16.
    while b.len() < 16 {
        b.push(0);
    }
    let words = [
        vm::tile_selector(env),
        cmd_off as u16,
        start.autodata_size as u16,
        0,
        0,
        exe.hmte,
        start.autodata.map_or(0, |a| exe.objects[a].selector()),
    ];
    for w in words {
        b.extend_from_slice(&w.to_le_bytes());
    }
    space.poke_bytes(p.info + info::LINFOSEG, &b);
}

impl Process {
    /// Create a thread (not yet started).
    pub fn new_thread(self: &Arc<Self>, frame: TrapFrame, stack_bottom: u32, stack_top: u32) -> Result<Arc<Thread>, u32> {
        let tid = self.next_tid.fetch_add(1, Ordering::Relaxed);
        if tid > info::MAX_THREADS {
            return Err(164); // ERROR_MAX_THRDS_REACHED
        }
        let t = Thread::new_user(self, tid, frame);
        let tib = self.info + info::TIB0 + tid * info::TIB_STRIDE;
        t.set_tib(tib);
        t.stack_base.store(stack_bottom, Ordering::Relaxed);
        t.stack_top.store(stack_top, Ordering::Relaxed);
        let tib2 = tib + info::TIB2;
        {
            let mut sp = self.space.lock();
            // TIB: exception chain (-1 = end), stack base, stack limit, TIB2,
            // version, ordinal.
            write_u32s(&mut sp, tib, &[0xffff_ffff, stack_bottom, stack_top, tib2, 20, t.id]);
            write_u32s(&mut sp, tib2, &[tid, t.priority(), 20, 0]);
        }
        self.threads.lock().push(t.clone());
        Ok(t)
    }

    pub fn main_thread(&self) -> Option<Arc<Thread>> {
        self.threads.lock().first().cloned()
    }

    pub fn is_16bit(&self) -> bool {
        self.exe.is16
    }

    /// Terminate the whole process: records the result and makes every
    /// thread (including the caller) exit at its next return to user mode.
    pub fn exit(self: &Arc<Self>, cause: u32, code: u32) {
        {
            let mut e = self.exit.lock();
            if e.is_some() {
                return;
            }
            *e = Some(ExitInfo { cause, code });
        }
        // With an exit list, the current thread runs it first (in user
        // mode) and the other threads end now.
        let me = sched::current_opt();
        let have_list = !self.exitlist.lock().is_empty();
        if let (true, Some(me)) = (have_list, me) {
            if me.process().is_some_and(|p| Arc::ptr_eq(&p, self)) {
                for t in self.threads.lock().iter() {
                    if !Arc::ptr_eq(t, &me) {
                        sched::kill(t);
                    }
                }
                self.exitlist_tid.store(me.tid, Ordering::Relaxed);
                self.exitlist_next.store(true, Ordering::Relaxed);
                return;
            }
        }
        self.finish_exit(ExitInfo { cause, code });
    }

    /// Make every thread end.
    pub fn finish_exit(self: &Arc<Self>, _info: ExitInfo) {
        for t in self.threads.lock().iter() {
            sched::kill(t);
        }
    }

    /// Called by `exit_current` as each thread dies; the last one finishes
    /// the process.
    pub fn thread_exited(self: &Arc<Self>) {
        {
            // A dying owner releases the critical section.
            let me = sched::current().tid;
            let mut cs = self.critsec.lock();
            if cs.0 == me {
                *cs = (0, 0);
            }
        }
        self.thread_waiters.wake_all();
        let alive = self
            .threads
            .lock()
            .iter()
            .filter(|t| t.state() != super::thread::State::Dead)
            .count();
        if alive > 0 {
            return;
        }
        {
            let mut e = self.exit.lock();
            if e.is_none() {
                *e = Some(ExitInfo { cause: 0, code: 0 });
            }
        }
        self.threads.lock().clear();
        crate::api::sem::process_ended(self.pid);
        crate::api::timer::process_ended(self.pid);
        crate::api::queue::process_ended(self.pid);
        // Close handles (pipes see their writers disappear).
        let hs = core::mem::take(&mut *self.handles.lock());
        drop(hs);
        self.exit_waiters.wake_all();
        let parent = by_pid(self.ppid);
        match &parent {
            Some(pp) => pp.child_waiters.wake_all(),
            None => {}
        }
        let keep = parent.is_some() && !self.detached.load(Ordering::Relaxed);
        if self.ppid == 0 {
            let code = self.exit_info().map_or(0, |e| e.code);
            crate::println!("[process {} ({}) ended, code {}]", self.pid, self.exe.name, code);
        }
        if !keep {
            reap(self.pid);
        }
        if self.ppid == 0 {
            crate::boot::process_ended(self.pid);
        }
    }

    pub fn exit_info(&self) -> Option<ExitInfo> {
        *self.exit.lock()
    }
}

/// Remove a terminated process from the table (its memory is released once
/// nothing references it any more).
pub fn reap(pid: u32) {
    PROCESSES.lock().retain(|p| p.pid != pid);
}
