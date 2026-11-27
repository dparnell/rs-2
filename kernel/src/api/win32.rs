//! Kernel support for rs2's Win32 layer (docs/WIN32.md): Windows PE
//! programs run as OS/2 processes whose KERNEL32 is rs2's W32KERN and
//! whose NTDLL is W32NTDLL.
//!
//! **Exceptions** are dispatched as Windows NT does it: a CPU exception in
//! a Win32 process is not walked through the handler chain by the kernel
//! (Win32's handlers take other records and return other values than
//! OS/2's) but handed to a dispatcher in user mode, W32NTDLL's, which the
//! kernel calls with an EXCEPTION_RECORD and a CONTEXT in Windows' layout
//! on the thread's stack, as NT calls KiUserExceptionDispatcher. The
//! dispatcher walks the frames at FS:[0], and resumes with
//! `Rs2W32Continue` or gives up with `Rs2W32Unhandled`. The stack grows
//! through its guard page without asking the handlers, as on Windows.
//!
//! | Ordinal | Call |
//! |---------|------|
//! | 430 | `Rs2W32Init(ULONG pfnDispatcher)`: the dispatcher, called as `cdecl (PEXCEPTION_RECORD, PCONTEXT)` |
//! | 431 | `Rs2W32Continue(PCONTEXT)`: resume the thread in that context (NtContinue) |
//! | 432 | `Rs2W32Unhandled(PEXCEPTION_RECORD, PCONTEXT)`: nobody handled it: the default action (the process ends, POPUPLOG) |
//! | 433 | `Rs2W32Modules(PULONG pBuf, ULONG cMax)`: the PE modules the process uses, dependencies first, as (image base, module handle) pairs, at most `cMax`; returns how many there are |
//! | 434 | `Rs2W32AllocAt(ULONG addr, ULONG cb, ULONG fl)`: private memory at `addr` (64 KB aligned; PAG_ flags), as VirtualAlloc with an address reserves it |

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::exceptions::{default_action, xcpt};
use super::Ctx;
use crate::arch::gdt::{TIB_SEL, USER_CS, USER_DS};
use crate::arch::idt::TrapFrame;
use crate::loader::{Format, Module};
use crate::task::process::{teb, Process};
use crate::task::thread::Thread;
use crate::vm;

/// NT status codes the kernel raises.
pub mod status {
    pub const ACCESS_VIOLATION: u32 = 0xC000_0005;
    pub const DATATYPE_MISALIGNMENT: u32 = 0x8000_0002;
    pub const BREAKPOINT: u32 = 0x8000_0003;
    pub const SINGLE_STEP: u32 = 0x8000_0004;
    pub const ARRAY_BOUNDS_EXCEEDED: u32 = 0xC000_008C;
    pub const FLOAT_DENORMAL_OPERAND: u32 = 0xC000_008D;
    pub const FLOAT_DIVIDE_BY_ZERO: u32 = 0xC000_008E;
    pub const FLOAT_INEXACT_RESULT: u32 = 0xC000_008F;
    pub const FLOAT_INVALID_OPERATION: u32 = 0xC000_0090;
    pub const FLOAT_OVERFLOW: u32 = 0xC000_0091;
    pub const FLOAT_STACK_CHECK: u32 = 0xC000_0092;
    pub const FLOAT_UNDERFLOW: u32 = 0xC000_0093;
    pub const INTEGER_DIVIDE_BY_ZERO: u32 = 0xC000_0094;
    pub const INTEGER_OVERFLOW: u32 = 0xC000_0095;
    pub const PRIVILEGED_INSTRUCTION: u32 = 0xC000_0096;
    pub const ILLEGAL_INSTRUCTION: u32 = 0xC000_001D;
    pub const STACK_OVERFLOW: u32 = 0xC000_00FD;
}

const RECORD_SIZE: u32 = 0x50;
const CONTEXT_SIZE: u32 = 0x2cc;

/// CONTEXT (i386) field offsets and flags.
mod ctx {
    pub const FLAGS: u32 = 0x00;
    pub const GS: u32 = 0x8c;
    pub const FS: u32 = 0x90;
    pub const ES: u32 = 0x94;
    pub const DS: u32 = 0x98;
    pub const EDI: u32 = 0x9c;
    pub const ESI: u32 = 0xa0;
    pub const EBX: u32 = 0xa4;
    pub const EDX: u32 = 0xa8;
    pub const ECX: u32 = 0xac;
    pub const EAX: u32 = 0xb0;
    pub const EBP: u32 = 0xb4;
    pub const EIP: u32 = 0xb8;
    pub const CS: u32 = 0xbc;
    pub const EFLAGS: u32 = 0xc0;
    pub const ESP: u32 = 0xc4;
    pub const SS: u32 = 0xc8;

    pub const I386: u32 = 0x0001_0000;
    pub const CONTROL: u32 = 0x01;
    pub const INTEGER: u32 = 0x02;
    pub const SEGMENTS: u32 = 0x04;
}

fn is_w32(p: &Process) -> bool {
    p.w32.load(Ordering::Relaxed) != 0
}

/// Rs2W32Init(pfnDispatcher)
pub fn init(c: &mut Ctx) -> u32 {
    if !is_w32(&c.process) {
        return super::err::NOT_SUPPORTED;
    }
    c.process.w32_dispatch.store(c.arg(0), Ordering::Relaxed);
    0
}

/// The NT code and parameters of a CPU exception.
fn translate(f: &TrapFrame) -> (u32, Vec<u32>) {
    use status::*;
    match f.vector {
        0 => (INTEGER_DIVIDE_BY_ZERO, Vec::new()),
        1 => (SINGLE_STEP, Vec::new()),
        3 => (BREAKPOINT, alloc::vec![0]),
        4 => (INTEGER_OVERFLOW, Vec::new()),
        5 => (ARRAY_BOUNDS_EXCEEDED, Vec::new()),
        6 => (ILLEGAL_INSTRUCTION, Vec::new()),
        14 => {
            let addr = crate::arch::read_cr2() as u32;
            // 0 read, 1 write, 8 execute (instruction fetch).
            let rw = if f.error & 0x10 != 0 { 8 } else if f.error & 2 != 0 { 1 } else { 0 };
            (ACCESS_VIOLATION, alloc::vec![rw, addr])
        }
        16 | 19 => (FLOAT_INVALID_OPERATION, Vec::new()),
        17 => (DATATYPE_MISALIGNMENT, Vec::new()),
        _ => (ACCESS_VIOLATION, alloc::vec![0, 0xffff_ffff]),
    }
}

/// The OS/2 code for an NT code (the default action's report).
fn os2_code(code: u32) -> u32 {
    use status::*;
    match code {
        INTEGER_DIVIDE_BY_ZERO => xcpt::INTEGER_DIVIDE_BY_ZERO,
        INTEGER_OVERFLOW => xcpt::INTEGER_OVERFLOW,
        ILLEGAL_INSTRUCTION => xcpt::ILLEGAL_INSTRUCTION,
        PRIVILEGED_INSTRUCTION => xcpt::PRIVILEGED_INSTRUCTION,
        BREAKPOINT => xcpt::BREAKPOINT,
        SINGLE_STEP => xcpt::SINGLE_STEP,
        ARRAY_BOUNDS_EXCEEDED => xcpt::ARRAY_BOUNDS_EXCEEDED,
        DATATYPE_MISALIGNMENT => xcpt::DATATYPE_MISALIGNMENT,
        FLOAT_DENORMAL_OPERAND..=FLOAT_UNDERFLOW => xcpt::FLOAT_INVALID_OPERATION,
        ACCESS_VIOLATION | STACK_OVERFLOW => xcpt::ACCESS_VIOLATION,
        c => c,
    }
}

fn put(b: &mut [u8], o: u32, v: u32) {
    b[o as usize..o as usize + 4].copy_from_slice(&v.to_le_bytes());
}

fn get(b: &[u8], o: u32) -> u32 {
    u32::from_le_bytes([b[o as usize], b[o as usize + 1], b[o as usize + 2], b[o as usize + 3]])
}

/// A CPU exception in a Win32 process: to W32NTDLL's dispatcher. False if
/// there is none (the process has not set it up yet, or the thread is not
/// in flat 32-bit code).
pub fn deliver(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) -> bool {
    let dispatcher = p.w32_dispatch.load(Ordering::Relaxed);
    if dispatcher == 0 || f.cs as u16 != USER_CS || f.ss as u16 != USER_DS {
        return false;
    }
    let (code, info) = translate(f);
    let mut cf = *f;
    let mut address = f.rip as u32;
    if f.vector == 3 {
        // The breakpoint's address, where Windows' context points too.
        address = address.wrapping_sub(1);
        cf.rip = address as u64;
    }
    if super::tracing() {
        crate::trace!("    win32 exception {code:x} {info:x?} at {address:x} esp {:x}", f.rsp);
    }
    let esp = (f.rsp as u32).wrapping_sub(512) & !15;
    let context = esp.wrapping_sub(CONTEXT_SIZE) & !15;
    let record = context.wrapping_sub(RECORD_SIZE) & !15;
    let mut r = alloc::vec![0u8; RECORD_SIZE as usize];
    put(&mut r, 0, code);
    put(&mut r, 4, 0);
    put(&mut r, 8, 0);
    put(&mut r, 12, address);
    put(&mut r, 16, info.len() as u32);
    for (i, v) in info.iter().take(15).enumerate() {
        put(&mut r, 20 + i as u32 * 4, *v);
    }
    let cx = save_context(&cf);
    let sp = record.wrapping_sub(12);
    let mut args = alloc::vec![0u8; 12];
    put(&mut args, 4, record);
    put(&mut args, 8, context);
    {
        let mut s = p.space.lock();
        if !(s.poke_bytes(record, &r) && s.poke_bytes(context, &cx) && s.poke_bytes(sp, &args)) {
            // No stack left for the records: the default action.
            return false;
        }
    }
    let _ = t;
    f.rip = dispatcher as u64;
    f.rsp = sp as u64;
    f.cs = USER_CS as u64;
    f.ss = USER_DS as u64;
    f.ds = USER_DS as u64;
    f.es = USER_DS as u64;
    f.fs = TIB_SEL as u64;
    f.rflags = 0x202;
    true
}

fn save_context(f: &TrapFrame) -> Vec<u8> {
    let mut b = alloc::vec![0u8; CONTEXT_SIZE as usize];
    put(&mut b, ctx::FLAGS, ctx::I386 | ctx::CONTROL | ctx::INTEGER | ctx::SEGMENTS);
    put(&mut b, ctx::GS, f.gs as u32);
    put(&mut b, ctx::FS, f.fs as u32);
    put(&mut b, ctx::ES, f.es as u32);
    put(&mut b, ctx::DS, f.ds as u32);
    put(&mut b, ctx::EDI, f.rdi as u32);
    put(&mut b, ctx::ESI, f.rsi as u32);
    put(&mut b, ctx::EBX, f.rbx as u32);
    put(&mut b, ctx::EDX, f.rdx as u32);
    put(&mut b, ctx::ECX, f.rcx as u32);
    put(&mut b, ctx::EAX, f.rax as u32);
    put(&mut b, ctx::EBP, f.rbp as u32);
    put(&mut b, ctx::EIP, f.rip as u32);
    put(&mut b, ctx::CS, f.cs as u32);
    put(&mut b, ctx::EFLAGS, f.rflags as u32);
    put(&mut b, ctx::ESP, f.rsp as u32);
    put(&mut b, ctx::SS, f.ss as u32);
    b
}

/// A user-mode selector, or `dflt` for one that is not (a context must
/// not make the kernel's IRET fault).
fn user_sel(v: u32, dflt: u16) -> u64 {
    if v & 3 == 3 && v <= 0xffff { v as u64 } else { dflt as u64 }
}

/// Load a CONTEXT into a trap frame, the parts its flags name.
fn load_context(b: &[u8], f: &mut TrapFrame) {
    let flags = get(b, ctx::FLAGS);
    if flags & ctx::SEGMENTS != 0 {
        f.gs = user_sel(get(b, ctx::GS), 0);
        f.fs = user_sel(get(b, ctx::FS), TIB_SEL);
        f.es = user_sel(get(b, ctx::ES), USER_DS);
        f.ds = user_sel(get(b, ctx::DS), USER_DS);
    }
    if flags & ctx::INTEGER != 0 {
        f.rdi = get(b, ctx::EDI) as u64;
        f.rsi = get(b, ctx::ESI) as u64;
        f.rbx = get(b, ctx::EBX) as u64;
        f.rdx = get(b, ctx::EDX) as u64;
        f.rcx = get(b, ctx::ECX) as u64;
        f.rax = get(b, ctx::EAX) as u64;
    }
    if flags & ctx::CONTROL != 0 {
        f.rbp = get(b, ctx::EBP) as u64;
        f.rip = get(b, ctx::EIP) as u64;
        f.cs = user_sel(get(b, ctx::CS), USER_CS);
        // User code may not set IOPL, VM or other system flags.
        let fl = get(b, ctx::EFLAGS) as u64;
        f.rflags = (f.rflags & !0xcd5) | (fl & 0xcd5) | 0x202;
        f.rsp = get(b, ctx::ESP) as u64;
        f.ss = user_sel(get(b, ctx::SS), USER_DS);
    }
}

/// Rs2W32Continue(pContext)
pub fn cont(c: &mut Ctx) -> u32 {
    let mut b = alloc::vec![0u8; CONTEXT_SIZE as usize];
    if !c.read(c.arg(0), &mut b) {
        return super::err::INVALID_ADDRESS;
    }
    load_context(&b, c.f);
    c.no_return = true;
    0
}

/// Rs2W32Unhandled(pRecord, pContext)
pub fn unhandled(c: &mut Ctx) -> u32 {
    let (prec, pctx) = (c.arg(0), c.arg(1));
    let code = c.read_u32(prec).unwrap_or(status::ACCESS_VIOLATION);
    let mut b = alloc::vec![0u8; CONTEXT_SIZE as usize];
    if c.read(pctx, &mut b) {
        load_context(&b, c.f);
    }
    if super::tracing() {
        crate::trace!("    win32 exception {code:x} unhandled at {:x}", c.f.rip);
    }
    let (p, t) = (c.process.clone(), c.thread.clone());
    default_action(c.f, &p, &t, os2_code(code));
    c.no_return = true;
    0
}

/// The PE modules a process uses, dependencies first.
pub fn modules(p: &Process) -> Vec<Arc<Module>> {
    fn walk(m: &Arc<Module>, out: &mut Vec<Arc<Module>>, seen: &mut Vec<u16>) {
        if seen.contains(&m.hmte) {
            return;
        }
        seen.push(m.hmte);
        for d in m.deps() {
            walk(&d, out, seen);
        }
        if m.format == Format::Pe {
            out.push(m.clone());
        }
    }
    let mut out = Vec::new();
    let mut seen = Vec::new();
    walk(&p.exe, &mut out, &mut seen);
    let loaded: Vec<Arc<Module>> = p.loaded.lock().iter().map(|(m, _)| m.clone()).collect();
    for m in &loaded {
        walk(m, &mut out, &mut seen);
    }
    out
}

/// Rs2W32Modules(pBuf, cMax)
pub fn query_modules(c: &mut Ctx) -> u32 {
    let (buf, max) = (c.arg(0), c.arg(1));
    let ms = modules(&c.process);
    for (i, m) in ms.iter().take(max as usize).enumerate() {
        let base = m.objects.first().map_or(0, |o| o.base);
        c.write_u32(buf + i as u32 * 8, base);
        c.write_u32(buf + i as u32 * 8 + 4, m.hmte as u32);
    }
    ms.len() as u32
}

/// Rs2W32AllocAt(addr, cb, fl)
pub fn alloc_at(c: &mut Ctx) -> u32 {
    let (addr, size, flags) = (c.arg(0), c.arg(1), c.arg(2));
    if addr % vm::TILE != 0 || addr < vm::ARENA_START || size == 0 {
        return super::err::INVALID_ADDRESS;
    }
    let flags = flags & (vm::pag::READ | vm::pag::WRITE | vm::pag::EXECUTE | vm::pag::GUARD | vm::pag::COMMIT);
    let r = c.space().alloc(size, flags, vm::SegKind::Data, Some(addr));
    match r {
        Ok(_) => 0,
        Err(_) => super::err::INVALID_ADDRESS,
    }
}

/// The stack of a Win32 thread grew down to the page at `page`: its TEB's
/// StackLimit follows (C runtimes' stack probes compare with it).
pub fn stack_grew(p: &Process, t: &Thread, page: u32) {
    if !is_w32(p) {
        return;
    }
    let tib = t.tib();
    let mut s = p.space.lock();
    let mut b = [0u8; 4];
    if s.read_bytes(tib + teb::STACK_LIMIT, &mut b) {
        let limit = u32::from_le_bytes(b);
        let page = page & !(vm::PAGE - 1);
        if page < limit && page + 0x1000_0000 > limit {
            s.poke_bytes(tib + teb::STACK_LIMIT, &page.to_le_bytes());
        }
    }
}
