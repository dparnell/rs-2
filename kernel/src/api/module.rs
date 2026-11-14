//! Module APIs and DLL initialisation.
//!
//! DLL initialisation routines run in user mode on the thread that caused
//! the load: at process start before the program's entry point, or inside
//! DosLoadModule before it returns. Each routine returns through gate slot
//! (0, 18); a zero result fails the load.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::{err, Ctx};
use crate::arch::gdt::{TIB_SEL, USER_CS, USER_DS};
use crate::arch::idt::TrapFrame;
use crate::loader::{self, gate, Module};
use crate::task::thread::Thread;
use crate::task::Process;
use crate::vm;

/// Gate slot (0, 18): DLL init/term routines return here.
pub const INIT_RETURN_ORDINAL: u32 = 18;
/// Gate slot (0, 2046): a thread whose next init routine another thread
/// is running (or has run) enters the kernel here and waits for it.
pub const INIT_WAIT_ORDINAL: u32 = 2046;

// ---- One initialisation at a time ----
//
// A DLL's global initialisation runs once, its per-process one once per
// process, although several processes (or threads of one) may load it at
// once: the routines run in user mode, so the loader lock cannot cover
// them. The thread about to run one claims it here; another that comes to
// the same routine waits in the kernel (INIT_WAIT_ORDINAL) until it has
// returned, then goes on without it, or runs it itself if it failed or
// its thread ended. IBM's loader serialises this with its semaphore.

struct Claim {
    hmte: u16,
    /// 0 for a global initialisation, else the process.
    pid: u32,
    /// The thread running it (Thread::id).
    thread: u32,
}

static CLAIMS: crate::sync::SpinLock<Vec<Claim>> = crate::sync::SpinLock::new(Vec::new());
static CLAIM_DONE: crate::task::WaitQueue = crate::task::WaitQueue::new();

enum Turn {
    /// This thread runs it.
    Run,
    /// Done already: skip it.
    Done,
    /// Another thread runs it now.
    Busy,
}

fn scope(m: &Module, p: &Process) -> u32 {
    if m.per_process_init { p.pid } else { 0 }
}

fn done(m: &Module, p: &Process) -> bool {
    if m.per_process_init { p.inited.lock().contains(&m.hmte) } else { m.global_init_done.load(Ordering::Acquire) }
}

fn claim(m: &Module, p: &Process, t: &Thread) -> Turn {
    let mut c = CLAIMS.lock();
    if done(m, p) {
        return Turn::Done;
    }
    let pid = scope(m, p);
    match c.iter().find(|x| x.hmte == m.hmte && x.pid == pid) {
        Some(x) if x.thread == t.id => Turn::Run,
        Some(_) => Turn::Busy,
        None => {
            c.push(Claim { hmte: m.hmte, pid, thread: t.id });
            Turn::Run
        }
    }
}

fn unclaim(m: &Module, p: &Process) {
    let pid = scope(m, p);
    CLAIMS.lock().retain(|x| !(x.hmte == m.hmte && x.pid == pid));
    CLAIM_DONE.wake_all();
}

/// A thread ended: what it was initialising is free for others.
pub fn thread_ended(thread: u32) {
    let mut c = CLAIMS.lock();
    let n = c.len();
    c.retain(|x| x.thread != thread);
    if c.len() != n {
        drop(c);
        CLAIM_DONE.wake_all();
    }
}

/// Gate slot INIT_WAIT_ORDINAL: wait until the next init routine is done
/// or ours to run.
pub fn init_wait(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) {
    loop {
        let Some((call, top)) = t.init_run.lock().as_ref().and_then(|r| r.calls.front().cloned().map(|c| (c, r.stack_top))) else {
            return;
        };
        match claim(&call.module, p, t) {
            Turn::Run => {
                setup_call(p, t, f, &call, top);
                return;
            }
            Turn::Done => {
                // As if it had run and succeeded.
                f.rax = 1;
                returned(f, p, t);
                return;
            }
            Turn::Busy => {
                let m = call.module.clone();
                let ok = CLAIM_DONE.wait_until(None, || {
                    let pid = scope(&m, p);
                    !CLAIMS.lock().iter().any(|x| x.hmte == m.hmte && x.pid == pid)
                });
                if !ok && t.exit_requested() {
                    return;
                }
            }
        }
    }
}
const ERROR_INIT_ROUTINE_FAILED: u32 = 295;

#[derive(Clone)]
pub struct InitCall {
    pub module: Arc<Module>,
    /// 0 = initialise, 1 = terminate.
    pub flag: u32,
}

/// A sequence of init/term calls in progress on a thread.
pub struct InitRun {
    pub calls: VecDeque<InitCall>,
    /// State to continue in when all calls are done.
    pub resume: TrapFrame,
    /// At process start a failure ends the process; in DosLoadModule it
    /// makes the API fail.
    pub at_start: bool,
    /// Address that receives the module handle (DosLoadModule) and whether
    /// it is a 16-bit (USHORT) location.
    pub result_handle: Option<(u32, bool, u16)>,
    /// Termination routines at process end: when they are done the exit
    /// list machinery finishes the exit.
    pub then_exit: bool,
    /// Linear address below which the routines' stack frames go.
    pub stack_top: u32,
    /// The run this one started inside (a DLL's init routine that loads
    /// another DLL): it goes on when this one is done.
    pub outer: Option<alloc::boxed::Box<InitRun>>,
}

/// Modules (dependencies first) whose init routine must run for `space`.
pub fn init_order(roots: &[Arc<Module>], p: &Process) -> Vec<InitCall> {
    let mut out = Vec::new();
    let mut seen: Vec<u16> = Vec::new();
    fn visit(m: &Arc<Module>, p: &Process, seen: &mut Vec<u16>, out: &mut Vec<InitCall>) {
        if seen.contains(&m.hmte) {
            return;
        }
        seen.push(m.hmte);
        for imp in &m.deps() {
            visit(imp, p, seen, out);
        }
        if !m.is_dll || m.init.is_none() {
            return;
        }
        let already = if m.per_process_init {
            p.inited.lock().contains(&m.hmte)
        } else {
            m.global_init_done.load(Ordering::Relaxed)
        };
        if !already {
            out.push(InitCall { module: m.clone(), flag: 0 });
        }
    }
    for r in roots {
        visit(r, p, &mut seen, &mut out);
    }
    out
}

/// DLL termination calls due when `p` ends: modules with per-process
/// termination whose initialisation ran, dependents before dependencies.
pub fn term_order(p: &Process) -> Vec<InitCall> {
    in_use(p).into_iter().rev().filter(|m| needs_term(m, p)).map(|module| InitCall { module, flag: 1 }).collect()
}

/// The modules a process uses (its program, what it loaded, their imports),
/// imports first.
fn in_use(p: &Process) -> Vec<Arc<Module>> {
    let mut roots = alloc::vec![p.exe.clone()];
    roots.extend(p.loaded.lock().iter().map(|(m, _)| m.clone()));
    let mut seen: Vec<u16> = Vec::new();
    let mut order: Vec<Arc<Module>> = Vec::new();
    fn visit(m: &Arc<Module>, seen: &mut Vec<u16>, order: &mut Vec<Arc<Module>>) {
        if seen.contains(&m.hmte) {
            return;
        }
        seen.push(m.hmte);
        for imp in &m.deps() {
            visit(imp, seen, order);
        }
        order.push(m.clone());
    }
    for r in &roots {
        visit(r, &mut seen, &mut order);
    }
    order
}

fn needs_term(m: &Module, p: &Process) -> bool {
    m.is_dll
        && m.per_process_term
        && m.init.is_some_and(|s| !s.entry.is16)
        && if m.per_process_init { p.inited.lock().contains(&m.hmte) } else { m.global_init_done.load(Ordering::Relaxed) }
}

/// Start the termination calls at process end.
pub fn start_term(p: &Process, t: &Thread, f: &mut TrapFrame, calls: Vec<InitCall>) -> bool {
    if !start(p, t, f, calls, false, None) {
        return false;
    }
    if let Some(r) = t.init_run.lock().as_mut() {
        r.then_exit = true;
    }
    true
}

/// Point `f` at the init routine of `call`, returning to the gate (or at
/// INIT_WAIT_ORDINAL when the initialisation is another thread's or done).
fn setup_call(p: &Process, t: &Thread, f: &mut TrapFrame, call: &InitCall, top: u32) {
    let m = &call.module;
    if call.flag == 0 && !matches!(claim(m, p, t), Turn::Run) {
        // The thread decides in the kernel, on its own (at exec this runs
        // on the parent's thread).
        let esp = top.wrapping_sub(0x40) & !0xf;
        f.ss = USER_DS as u64;
        f.rsp = esp as u64;
        f.cs = USER_CS as u64;
        f.rip = gate::slot_address(0, INIT_WAIT_ORDINAL) as u64;
        f.ds = USER_DS as u64;
        f.es = USER_DS as u64;
        f.fs = TIB_SEL as u64;
        f.rflags = 0x202;
        return;
    }
    let start = m.init.unwrap();
    let ret = gate::slot_address(0, INIT_RETURN_ORDINAL);
    let mut sp = p.space.lock();
    if start.entry.is16 {
        // Far call on a 16-bit view of the thread's stack: DI = module
        // handle, SI = heap size, DS = the DLL's automatic data segment.
        let base = top.wrapping_sub(0x40) & !0xf;
        let ss = vm::tile_selector(base);
        let spoff = base & 0xffff;
        let mut b = Vec::new();
        b.extend_from_slice(&(ret as u16).to_le_bytes());
        b.extend_from_slice(&vm::tile_selector(ret).to_le_bytes());
        sp.poke_bytes(base.wrapping_sub(4), &b);
        f.ss = ss as u64;
        f.rsp = (spoff.wrapping_sub(4) & 0xffff) as u64;
        f.cs = start.entry.sel as u64;
        f.rip = start.entry.off as u64;
        f.ds = start.autodata.map_or(0, |a| m.objects[a].selector()) as u64;
        f.es = 0;
        f.fs = 0;
        f.rdi = m.hmte as u64;
        f.rsi = 0;
        f.rcx = 0;
        f.rax = 0;
        // A Windows DLL's LibEntry: DI the instance (DGROUP), CX the local
        // heap's size, ES:SI the command line (none).
        if let Some((heap, _)) = start.windows {
            f.rdi = f.ds;
            f.rcx = heap as u64;
        }
    } else {
        // _DLL_InitTerm(hmod, flag)
        let esp = top.wrapping_sub(0x40) & !0xf;
        let mut b = Vec::new();
        for v in [ret, m.hmte as u32, call.flag] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        sp.poke_bytes(esp, &b);
        f.ss = USER_DS as u64;
        f.rsp = esp as u64;
        f.cs = USER_CS as u64;
        f.rip = start.entry.flat as u64;
        f.ds = USER_DS as u64;
        f.es = USER_DS as u64;
        f.fs = TIB_SEL as u64;
    }
    f.rflags = 0x202;
}

/// Begin running `calls` on the current thread; `f` is redirected to the
/// first one. Returns false if there is nothing to run.
pub fn start(p: &Process, t: &Thread, f: &mut TrapFrame, calls: Vec<InitCall>, at_start: bool, result_handle: Option<(u32, bool, u16)>) -> bool {
    if calls.is_empty() {
        return false;
    }
    // At process start the stack is empty; inside DosLoadModule the
    // routines run below the caller's live frames.
    let stack_top = if at_start {
        t.stack_top.load(Ordering::Relaxed)
    } else {
        let sp = p.space.lock();
        let d = sp.descriptor(f.ss as u16);
        let esp = if vm::desc_big(d) { f.rsp as u32 } else { f.rsp as u32 & 0xffff };
        vm::desc_base(d).wrapping_add(esp).wrapping_sub(0x100)
    };
    let outer = t.init_run.lock().take().map(alloc::boxed::Box::new);
    let run = InitRun { calls: calls.into(), resume: *f, at_start, result_handle, stack_top, then_exit: false, outer };
    let first = run.calls.front().unwrap().clone();
    *t.init_run.lock() = Some(run);
    setup_call(p, t, f, &first, stack_top);
    true
}

/// An init/term routine returned.
pub fn returned(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) {
    let mut guard = t.init_run.lock();
    let Some(run) = guard.as_mut() else { return };
    let call = run.calls.pop_front().unwrap();
    let is16 = call.module.init.is_some_and(|s| s.entry.is16);
    let ok = if is16 { f.rax & 0xffff != 0 } else { f.rax as u32 != 0 };
    if call.flag == 0 {
        if !ok {
            crate::println!("DLL {} failed to initialise", call.module.name);
            unclaim(&call.module, p);
            let mut run = guard.take().unwrap();
            *guard = run.outer.take().map(|b| *b);
            drop(guard);
            if run.at_start {
                p.exit(1, ERROR_INIT_ROUTINE_FAILED);
            } else {
                *f = run.resume;
                f.rax = ERROR_INIT_ROUTINE_FAILED as u64;
            }
            return;
        }
        if call.module.per_process_init {
            let mut i = p.inited.lock();
            if !i.contains(&call.module.hmte) {
                i.push(call.module.hmte);
            }
        } else {
            call.module.global_init_done.store(true, Ordering::Release);
        }
        unclaim(&call.module, p);
    }
    if let Some(next) = run.calls.front().cloned() {
        let top = run.stack_top;
        drop(guard);
        setup_call(p, t, f, &next, top);
        return;
    }
    let mut run = guard.take().unwrap();
    *guard = run.outer.take().map(|b| *b);
    drop(guard);
    *f = run.resume;
    if run.then_exit {
        p.exitlist_next.store(true, Ordering::Relaxed);
        return;
    }
    if let Some((addr, is16, h)) = run.result_handle {
        let mut sp = p.space.lock();
        if is16 {
            sp.poke_bytes(addr, &h.to_le_bytes());
        } else {
            sp.poke_bytes(addr, &(h as u32).to_le_bytes());
        }
        f.rax = 0;
    }
}

// ---- APIs ----

fn load(c: &mut Ctx, fail: u32, cbfail: u32, pname: u32, phmod: u32) -> u32 {
    let Some(name) = c.read_str(pname, 260) else { return err::INVALID_ADDRESS };
    if super::tracing() {
        crate::trace!("    load {name}");
    }
    let _ = loader::take_fail();
    // Until the module is recorded as this process's (loader.rs).
    let loading = loader::lock();
    // What this process uses now: a DLL it let go of comes back with fresh
    // data, as OS/2 reads it in again.
    let used: Vec<u16> = in_use(&c.process).iter().map(|m| m.hmte).collect();
    let r = loader::load_dll_again(&name, &used, || c.space());
    let m = match r {
        Ok(m) => m,
        Err(e) => {
            let name = loader::take_fail().unwrap_or(name);
            if fail != 0 && cbfail > 0 {
                let n = (cbfail as usize - 1).min(name.len());
                c.write_str(fail, &name[..n]);
            }
            return e;
        }
    };
    {
        let mut l = c.process.loaded.lock();
        match l.iter_mut().find(|(x, _)| Arc::ptr_eq(x, &m)) {
            Some(e) => e.1 += 1,
            None => l.push((m.clone(), 1)),
        }
    }
    drop(loading);
    let calls = init_order(core::slice::from_ref(&m), &c.process);
    if calls.is_empty() {
        if c.is16 { c.write_u16(phmod, m.hmte) } else { c.write_u32(phmod, m.hmte as u32) };
        return 0;
    }
    // Run the init routines, then complete the API call: emulate its
    // return now so the resume state is the caller's continuation.
    c.complete_return(0);
    let (p, t) = (c.process.clone(), c.thread.clone());
    start(&p, &t, c.f, calls, false, Some((phmod, c.is16, m.hmte)));
    c.no_return = true;
    0
}

pub fn dos_load_module(c: &mut Ctx) -> u32 {
    // DosLoadModule(pszFailName, cbFailName, pszModName, phmod)
    let (fail, cb, name, ph) = if c.is16 { (c.ptr(0), c.arg(1), c.ptr(2), c.ptr(3)) } else { (c.arg(0), c.arg(1), c.arg(2), c.arg(3)) };
    load(c, fail, cb, name, ph)
}

pub fn dos_free_module(c: &mut Ctx) -> u32 {
    let h = c.arg(0) as u16;
    let _loading = loader::lock();
    let freed = {
        let mut l = c.process.loaded.lock();
        let Some(i) = l.iter().position(|(m, _)| m.hmte == h) else { return err::INVALID_HANDLE };
        l[i].1 -= 1;
        if l[i].1 > 0 {
            return 0;
        }
        l.remove(i).0
    };
    // The last reference in this process: run its termination routine,
    // unless the module is still in use through the program's imports.
    let still_used = term_order(&c.process).iter().any(|x| Arc::ptr_eq(&x.module, &freed));
    if still_used || !needs_term(&freed, &c.process) {
        return 0;
    }
    if freed.per_process_init {
        c.process.inited.lock().retain(|&x| x != freed.hmte);
    }
    c.complete_return(0);
    let (p, t) = (c.process.clone(), c.thread.clone());
    start(&p, &t, c.f, alloc::vec![InitCall { module: freed, flag: 1 }], false, None);
    c.no_return = true;
    0
}

fn module_by_handle(c: &Ctx, h: u32) -> Option<Arc<Module>> {
    if h == 0 {
        return Some(c.process.exe.clone());
    }
    loader::by_handle(h as u16)
}

/// A procedure by name; "#n" names ordinal n (EPM's macros look up
/// DOSCALLS and PMWIN procedures this way).
fn resolve_named(m: &loader::Module, name: &str) -> Result<loader::Target, u32> {
    match name.strip_prefix('#').and_then(|n| n.parse::<u32>().ok()) {
        Some(o) => m.resolve_ordinal(o, 0),
        None => m.resolve_name(name, 0),
    }
}

pub fn dos_query_proc_addr(c: &mut Ctx) -> u32 {
    // DosQueryProcAddr(hmod, ordinal, pszName, ppfn)
    let (h, ord, pname, ppfn) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    let Some(m) = module_by_handle(c, h) else { return err::INVALID_HANDLE };
    let mut asked = None;
    let t = if pname != 0 && ord == 0 {
        let Some(name) = c.read_str(pname, 256) else { return err::INVALID_ADDRESS };
        if super::tracing() {
            crate::trace!("    proc {name} in {}", m.name);
        }
        let t = resolve_named(&m, &name);
        asked = Some(name);
        t
    } else {
        m.resolve_ordinal(ord, 0)
    };
    match t {
        // 16-bit entry points are returned as 16:16 pointers.
        Ok(t) => {
            let v = if t.is16 && m.format != loader::Format::Virtual { (t.sel as u32) << 16 | t.off as u32 } else { t.flat };
            c.write_u32(ppfn, v);
            0
        }
        Err(e) => {
            // What programs look for and rs2 lacks (they may go without).
            match asked {
                Some(name) => crate::trace!("DosQueryProcAddr: {}.{name} not found", m.name),
                None => crate::trace!("DosQueryProcAddr: {}.{ord} not found", m.name),
            }
            e
        }
    }
}

pub fn dos16_get_proc_addr(c: &mut Ctx) -> u32 {
    // DosGetProcAddr(hmod, pszProcName, ppfnProcAddr): a name, or
    // "#ordinal", or a 0:ordinal far pointer.
    let (h, pname_raw, ppfn) = (c.arg(0), c.arg(1), c.ptr(2));
    let Some(m) = module_by_handle(c, h) else { return err::INVALID_HANDLE };
    let t = if pname_raw >> 16 == 0 {
        m.resolve_ordinal(pname_raw & 0xffff, 0)
    } else {
        let Some(name) = c.read_str(c.far16_to_flat(pname_raw), 256) else { return err::INVALID_ADDRESS };
        if super::tracing() {
            crate::trace!("    proc {name} in {}", m.name);
        }
        match name.strip_prefix('#').and_then(|n| n.parse::<u32>().ok()) {
            Some(o) => m.resolve_ordinal(o, 0),
            None => m.resolve_name(&name, 0),
        }
    };
    match t {
        Ok(t) => {
            c.write_u32(ppfn, (t.sel as u32) << 16 | t.off as u32);
            0
        }
        Err(e) => e,
    }
}

pub fn dos_query_proc_type(c: &mut Ctx) -> u32 {
    // DosQueryProcType(hmod, ordinal, pszName, pulProcType)
    let (h, ord, pname, ptype) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3));
    let Some(m) = module_by_handle(c, h) else { return err::INVALID_HANDLE };
    let t = if pname != 0 && ord == 0 {
        let Some(name) = c.read_str(pname, 256) else { return err::INVALID_ADDRESS };
        resolve_named(&m, &name)
    } else {
        m.resolve_ordinal(ord, 0)
    };
    match t {
        Ok(t) => {
            c.write_u32(ptype, if t.is16 { 0 } else { 1 }); // PT_16BIT / PT_32BIT
            0
        }
        Err(e) => e,
    }
}

/// FAPPTYP_* flags for an executable file.
pub fn app_type(data: &[u8]) -> u32 {
    use os2_exe::Format;
    match os2_exe::detect(data) {
        Ok(Format::Lx { header }) => match os2_exe::lx::Module::parse(data, header) {
            Ok(m) => {
                let f = m.header.module_flags;
                let mut t = (f >> 8) & 3;
                if m.header.is_library() {
                    t |= 0x0008; // FAPPTYP_DLL
                }
                t | 0x4000 // FAPPTYP_32BIT
            }
            Err(_) => 0x0010,
        },
        Ok(Format::Ne { header }) => match os2_exe::ne::Module::parse(data, header) {
            Ok(m) => {
                let mut t = ((m.header.flags as u32) >> 8) & 3;
                if m.header.flags & os2_exe::ne::flags::LIBRARY != 0 {
                    t |= 0x0008;
                }
                if m.header.target_os == 2 {
                    t |= 0x0200; // FAPPTYP_WINDOWSREAL
                }
                t
            }
            Err(_) => 0x0010,
        },
        // A Win32 program, as IBM's LDRQAppType reports it (0x5010 for
        // a DLL, IMAGE_FILE_DLL in the characteristics).
        Ok(Format::Mz) if pe_header(data).is_some() => {
            let h = pe_header(data).unwrap_or(0);
            if data.get(h + 0x17).is_some_and(|b| b & 0x20 != 0) { 0x5010 } else { 0x5000 }
        }
        Ok(Format::Mz) => 0x0020,      // FAPPTYP_DOS
        // DosQueryAppType fails for these (le_or_unknown); 0 here.
        Ok(Format::Le { .. }) => 0,
        Err(_) => 0x0010,              // FAPPTYP_BOUND / unknown
    }
}

/// The offset of a PE header behind the MZ header, if there is one.
fn pe_header(data: &[u8]) -> Option<usize> {
    let lfanew = u32::from_le_bytes(data.get(0x3c..0x40)?.try_into().ok()?) as usize;
    (lfanew != 0 && data.get(lfanew..lfanew + 4) == Some(b"PE\0\0")).then_some(lfanew)
}

/// Whether IBM's loader rejects the file's header with
/// ERROR_INVALID_EXE_SIGNATURE before it knows its type: an LE executable
/// (a DOS extender's, such as DOS/4GW's, or a Windows VxD) is neither an
/// OS/2 nor a DOS program to it (_ldrOpenNewExe), so DosQueryAppType and
/// DosExecPgm fail with 191.
pub fn le_or_unknown(data: &[u8]) -> bool {
    matches!(os2_exe::detect(data), Ok(os2_exe::Format::Le { .. }))
}

pub fn dos_query_app_type(c: &mut Ctx) -> u32 {
    let (pname, ptype) = if c.is16 { (c.ptr(0), c.ptr(1)) } else { (c.arg(0), c.arg(1)) };
    let Some(name) = c.read_str(pname, 260) else { return err::INVALID_ADDRESS };
    let mut path = match super::file::resolve(c, &name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !path.file_name().contains('.') {
        if let Some(last) = path.parts.last_mut() {
            last.push_str(".EXE");
        }
    }
    let Some(data) = crate::fs::read_file(&path.display()) else { return err::FILE_NOT_FOUND };
    // .COM files are DOS programs.
    let t = if path.file_name().to_ascii_uppercase().ends_with(".COM") && !data.starts_with(b"MZ") {
        0x0020
    } else if le_or_unknown(&data) {
        return crate::loader::err::INVALID_EXE_SIGNATURE;
    } else {
        app_type(&data)
    };
    if c.is16 { c.write_u16(ptype, t as u16) } else { c.write_u32(ptype, t) };
    0
}

// ---- Extended LIBPATH ----

const BEGIN_LIBPATH: u32 = 1;
const END_LIBPATH: u32 = 2;
const LIBPATHSTRICT: u32 = 3;

pub fn dos_set_ext_libpath(c: &mut Ctx) -> u32 {
    // (pszExtLIBPATH, flags)
    let (p, flags) = (if c.is16 { c.ptr(0) } else { c.arg(0) }, c.arg(1));
    let Some(v) = c.read_str(p, 1024) else { return err::INVALID_ADDRESS };
    let mut x = c.process.ext_libpath.lock();
    match flags {
        BEGIN_LIBPATH => x.0 = expand(&v, &x.0, &x.1),
        END_LIBPATH => x.1 = expand(&v, &x.0, &x.1),
        LIBPATHSTRICT => x.2 = v.eq_ignore_ascii_case("T"),
        _ => return err::INVALID_PARAMETER,
    }
    0
}

/// Replace %BeginLIBPATH% and %EndLIBPATH% with their current values
/// (either may appear in either setting).
fn expand(v: &str, begin: &str, end: &str) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    let up = v.to_ascii_uppercase();
    let mut i = 0;
    while i < v.len() {
        if up[i..].starts_with("%BEGINLIBPATH%") {
            out.push_str(begin);
            i += 14;
        } else if up[i..].starts_with("%ENDLIBPATH%") {
            out.push_str(end);
            i += 12;
        } else {
            out.push(v.as_bytes()[i] as char);
            i += 1;
        }
    }
    out
}

pub fn dos_query_ext_libpath(c: &mut Ctx) -> u32 {
    let (p, flags) = (if c.is16 { c.ptr(0) } else { c.arg(0) }, c.arg(1));
    let v = {
        let x = c.process.ext_libpath.lock();
        match flags {
            BEGIN_LIBPATH => x.0.clone(),
            END_LIBPATH => x.1.clone(),
            LIBPATHSTRICT => alloc::string::String::from(if x.2 { "T" } else { "" }),
            _ => return err::INVALID_PARAMETER,
        }
    };
    if c.write_str(p, &v) { 0 } else { err::INVALID_ADDRESS }
}
