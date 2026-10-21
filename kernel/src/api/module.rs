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
        for imp in &m.imports {
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
    let mut roots = alloc::vec![p.exe.clone()];
    roots.extend(p.loaded.lock().iter().map(|(m, _)| m.clone()));
    let mut seen: Vec<u16> = Vec::new();
    let mut order: Vec<Arc<Module>> = Vec::new();
    fn visit(m: &Arc<Module>, seen: &mut Vec<u16>, order: &mut Vec<Arc<Module>>) {
        if seen.contains(&m.hmte) {
            return;
        }
        seen.push(m.hmte);
        for imp in &m.imports {
            visit(imp, seen, order);
        }
        order.push(m.clone());
    }
    for r in &roots {
        visit(r, &mut seen, &mut order);
    }
    order.into_iter().rev().filter(|m| needs_term(m, p)).map(|module| InitCall { module, flag: 1 }).collect()
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

/// Point `f` at the init routine of `call`, returning to the gate.
fn setup_call(p: &Process, f: &mut TrapFrame, call: &InitCall, top: u32) {
    let m = &call.module;
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
    let run = InitRun { calls: calls.into(), resume: *f, at_start, result_handle, stack_top, then_exit: false };
    setup_call(p, f, run.calls.front().unwrap(), stack_top);
    *t.init_run.lock() = Some(run);
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
            let run = guard.take().unwrap();
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
            p.inited.lock().push(call.module.hmte);
        } else {
            call.module.global_init_done.store(true, Ordering::Relaxed);
        }
    }
    if let Some(next) = run.calls.front().cloned() {
        let top = run.stack_top;
        drop(guard);
        setup_call(p, f, &next, top);
        return;
    }
    let run = guard.take().unwrap();
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
    let r = loader::load_dll(&name, || c.space());
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
    let t = if pname != 0 && ord == 0 {
        let Some(name) = c.read_str(pname, 256) else { return err::INVALID_ADDRESS };
        if super::tracing() {
            crate::trace!("    proc {name} in {}", m.name);
        }
        resolve_named(&m, &name)
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
        Err(e) => e,
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
        Ok(Format::Mz) => 0x0020,      // FAPPTYP_DOS
        Ok(Format::Le { .. }) => 0x0040, // FAPPTYP_VIRTDRV
        Err(_) => 0x0010,              // FAPPTYP_BOUND / unknown
    }
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
    let t = app_type(&data);
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
