//! Exception management for OS/2 programs.
//!
//! A CPU exception (or DosRaiseException) in 32-bit code is dispatched to
//! the thread's chain of exception registration records (`FS:[0]`). The
//! report and context records are built on the user stack; each handler is
//! called in user mode with a return address in the API gate, so its result
//! comes back to the kernel, which then resumes the thread (possibly with a
//! modified context), tries the next handler, or applies the default action.
//!
//! Unwinding (DosUnwindException) calls the handlers between the chain head
//! and a target record with `EH_UNWINDING` the same way.

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::arch::gdt::{TIB_SEL, USER_CS, USER_DS};
use crate::arch::idt::TrapFrame;
use crate::loader::gate;
use crate::task::thread::Thread;
use crate::task::Process;
use crate::vm;

pub mod xcpt {
    pub const ACCESS_VIOLATION: u32 = 0xC000_0005;
    pub const IN_PAGE_ERROR: u32 = 0xC000_0006;
    pub const ILLEGAL_INSTRUCTION: u32 = 0xC000_001C;
    pub const UNWIND: u32 = 0xC000_0026;
    pub const FLOAT_DENORMAL_OPERAND: u32 = 0xC000_0094;
    pub const FLOAT_DIVIDE_BY_ZERO: u32 = 0xC000_0095;
    pub const FLOAT_INEXACT_RESULT: u32 = 0xC000_0096;
    pub const FLOAT_INVALID_OPERATION: u32 = 0xC000_0097;
    pub const FLOAT_OVERFLOW: u32 = 0xC000_0098;
    pub const FLOAT_STACK_CHECK: u32 = 0xC000_0099;
    pub const FLOAT_UNDERFLOW: u32 = 0xC000_009A;
    pub const INTEGER_DIVIDE_BY_ZERO: u32 = 0xC000_009B;
    pub const INTEGER_OVERFLOW: u32 = 0xC000_009C;
    pub const PRIVILEGED_INSTRUCTION: u32 = 0xC000_009D;
    pub const DATATYPE_MISALIGNMENT: u32 = 0xC000_009E;
    pub const BREAKPOINT: u32 = 0xC000_009F;
    pub const SINGLE_STEP: u32 = 0xC000_00A0;
    pub const ARRAY_BOUNDS_EXCEEDED: u32 = 0xC000_0093;
    pub const GUARD_PAGE_VIOLATION: u32 = 0x8000_0001;
    pub const PROCESS_TERMINATE: u32 = 0xC001_0001;
    pub const ASYNC_PROCESS_TERMINATE: u32 = 0xC001_0002;
    pub const SIGNAL: u32 = 0xC001_0003;

    pub const CONTINUE_SEARCH: u32 = 0;
    pub const CONTINUE_EXECUTION: u32 = 0xFFFF_FFFF;
    pub const END_OF_CHAIN: u32 = 0xFFFF_FFFF;

    pub const EH_NONCONTINUABLE: u32 = 0x01;
    pub const EH_UNWINDING: u32 = 0x02;
    pub const EH_EXIT_UNWIND: u32 = 0x04;
    pub const EH_STACK_INVALID: u32 = 0x08;
    pub const EH_NESTED_CALL: u32 = 0x10;
}

/// Gate slot (0, 17): exception handlers return here.
pub const HANDLER_RETURN_ORDINAL: u32 = 17;

const REPORT_SIZE: u32 = 0x24;
const CONTEXT_SIZE: u32 = 0xb0;

/// Context record field offsets.
mod ctx {
    pub const FLAGS: u32 = 0x00;
    // ContextFlags, ctx_env[7], ctx_stack[8] (packed 10-byte FPREGs).
    pub const GS: u32 = 0x70;
    pub const FS: u32 = 0x74;
    pub const ES: u32 = 0x78;
    pub const DS: u32 = 0x7c;
    pub const EDI: u32 = 0x80;
    pub const ESI: u32 = 0x84;
    pub const EAX: u32 = 0x88;
    pub const EBX: u32 = 0x8c;
    pub const ECX: u32 = 0x90;
    pub const EDX: u32 = 0x94;
    pub const EBP: u32 = 0x98;
    pub const EIP: u32 = 0x9c;
    pub const CS: u32 = 0xa0;
    pub const EFLAGS: u32 = 0xa4;
    pub const ESP: u32 = 0xa8;
    pub const SS: u32 = 0xac;
}

/// What the kernel is doing for a thread's exception.
#[derive(Clone, Copy, Debug)]
pub struct Dispatch {
    pub report: u32,
    pub context: u32,
    /// Registration record whose handler is running.
    pub reg: u32,
    pub unwind: Option<Unwind>,
    /// Exception code (for the default action).
    pub code: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Unwind {
    /// Stop when reaching this record (END_OF_CHAIN = unwind everything).
    pub target: u32,
    /// Where to continue after the unwind; None = return to the caller of
    /// DosUnwindException as if it returned.
    pub target_ip: u32,
    /// Saved caller state for the final transfer.
    pub resume: TrapFrame,
    /// Termination unwind: the thread ends when it is done.
    pub exit: bool,
}

fn rd(sp: &mut vm::AddressSpace, a: u32) -> Option<u32> {
    let mut b = [0u8; 4];
    sp.read_bytes(a, &mut b).then(|| u32::from_le_bytes(b))
}

fn wr(sp: &mut vm::AddressSpace, a: u32, v: u32) -> bool {
    sp.poke_bytes(a, &v.to_le_bytes())
}

/// Map a CPU trap to an OS/2 exception code and its parameters.
fn translate(f: &TrapFrame) -> (u32, Vec<u32>) {
    use xcpt::*;
    match f.vector {
        0 => (INTEGER_DIVIDE_BY_ZERO, Vec::new()),
        1 => (SINGLE_STEP, Vec::new()),
        3 => (BREAKPOINT, Vec::new()),
        4 => (INTEGER_OVERFLOW, Vec::new()),
        5 => (ARRAY_BOUNDS_EXCEEDED, Vec::new()),
        6 => (ILLEGAL_INSTRUCTION, Vec::new()),
        14 => {
            let addr = crate::arch::read_cr2() as u32;
            let rw = if f.error & 2 != 0 { 2 } else { 1 };
            (ACCESS_VIOLATION, alloc::vec![rw, addr])
        }
        // General protection: privileged instruction or segment violation.
        13 | 11 | 12 | 10 => (ACCESS_VIOLATION, alloc::vec![0, 0xffff_ffff]),
        16 | 19 => (FLOAT_INVALID_OPERATION, Vec::new()),
        17 => (DATATYPE_MISALIGNMENT, Vec::new()),
        _ => (ACCESS_VIOLATION, alloc::vec![0, 0]),
    }
}

/// Fill a context record from a trap frame.
pub(crate) fn save_context(sp: &mut vm::AddressSpace, at: u32, f: &TrapFrame) {
    let mut b = alloc::vec![0u8; CONTEXT_SIZE as usize];
    let put = |b: &mut Vec<u8>, o: u32, v: u32| b[o as usize..o as usize + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut b, ctx::FLAGS, 0x7); // CONTEXT_FULL
    put(&mut b, ctx::GS, f.gs as u32);
    put(&mut b, ctx::FS, f.fs as u32);
    put(&mut b, ctx::ES, f.es as u32);
    put(&mut b, ctx::DS, f.ds as u32);
    put(&mut b, ctx::EDI, f.rdi as u32);
    put(&mut b, ctx::ESI, f.rsi as u32);
    put(&mut b, ctx::EAX, f.rax as u32);
    put(&mut b, ctx::EBX, f.rbx as u32);
    put(&mut b, ctx::ECX, f.rcx as u32);
    put(&mut b, ctx::EDX, f.rdx as u32);
    put(&mut b, ctx::EBP, f.rbp as u32);
    put(&mut b, ctx::EIP, f.rip as u32);
    put(&mut b, ctx::CS, f.cs as u32);
    put(&mut b, ctx::EFLAGS, f.rflags as u32);
    put(&mut b, ctx::ESP, f.rsp as u32);
    put(&mut b, ctx::SS, f.ss as u32);
    sp.poke_bytes(at, &b);
}

/// Load a (possibly handler-modified) context record into a trap frame.
fn load_context(sp: &mut vm::AddressSpace, at: u32, f: &mut TrapFrame) {
    let g = |sp: &mut vm::AddressSpace, o: u32| rd(sp, at + o).unwrap_or(0) as u64;
    f.gs = g(sp, ctx::GS) & 0xffff;
    f.fs = g(sp, ctx::FS) & 0xffff;
    f.es = g(sp, ctx::ES) & 0xffff;
    f.ds = g(sp, ctx::DS) & 0xffff;
    f.rdi = g(sp, ctx::EDI);
    f.rsi = g(sp, ctx::ESI);
    f.rax = g(sp, ctx::EAX);
    f.rbx = g(sp, ctx::EBX);
    f.rcx = g(sp, ctx::ECX);
    f.rdx = g(sp, ctx::EDX);
    f.rbp = g(sp, ctx::EBP);
    f.rip = g(sp, ctx::EIP);
    f.cs = g(sp, ctx::CS) & 0xffff;
    // User code may not set IOPL, VM or other system flags.
    let fl = g(sp, ctx::EFLAGS);
    f.rflags = (f.rflags & !0xcd5) | (fl & 0xcd5) | 0x202;
    f.rsp = g(sp, ctx::ESP);
    f.ss = g(sp, ctx::SS) & 0xffff;
}

/// Is the thread running 32-bit flat code?
fn flat32(sp: &vm::AddressSpace, f: &TrapFrame) -> bool {
    vm::desc_big(sp.descriptor(f.cs as u16)) && vm::desc_big(sp.descriptor(f.ss as u16))
}

/// Entry point for CPU exceptions raised by user code.
pub fn deliver(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) {
    let (code, info) = translate(f);
    if !raise(f, p, t, code, 0, &info, f.rip as u32) {
        default_action(f, p, t, code);
    }
}

/// Start dispatching an exception. Returns false if there is no handler to
/// call (the caller applies the default action).
pub fn raise(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>, code: u32, flags: u32, info: &[u32], address: u32) -> bool {
    let tib = t.tib();
    let mut sp = p.space.lock();
    if !flat32(&sp, f) {
        // Exceptions in 16-bit code are not dispatched to 32-bit handlers
        // here; 16-bit programs use DosSetVec.
        return false;
    }
    let Some(head) = rd(&mut sp, tib) else { return false };
    if head == xcpt::END_OF_CHAIN || head == 0 {
        return false;
    }
    let nested = !t.xcpt.lock().is_empty();
    // Build the records below the current stack pointer.
    let esp = (f.rsp as u32).wrapping_sub(256) & !15;
    let context = esp.wrapping_sub(CONTEXT_SIZE);
    let report = context.wrapping_sub(REPORT_SIZE) & !15;
    let mut r = alloc::vec![0u8; REPORT_SIZE as usize];
    let put = |b: &mut Vec<u8>, o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut r, 0, code);
    put(&mut r, 4, flags | if nested { xcpt::EH_NESTED_CALL } else { 0 });
    put(&mut r, 8, 0);
    put(&mut r, 12, address);
    put(&mut r, 16, info.len().min(4) as u32);
    for (i, v) in info.iter().take(4).enumerate() {
        put(&mut r, 20 + i * 4, *v);
    }
    if !sp.poke_bytes(report, &r) {
        return false;
    }
    save_context(&mut sp, context, f);
    let d = Dispatch { report, context, reg: head, unwind: None, code };
    call_handler(&mut sp, f, &d, report.wrapping_sub(16));
    drop(sp);
    t.xcpt.lock().push(d);
    true
}

/// Set up the frame to call the handler of `d.reg`.
fn call_handler(sp: &mut vm::AddressSpace, f: &mut TrapFrame, d: &Dispatch, stack: u32) {
    let handler = rd(sp, d.reg + 4).unwrap_or(0);
    let ret = gate::slot_address(0, HANDLER_RETURN_ORDINAL);
    let esp = stack.wrapping_sub(20) & !3;
    // [esp] = return, then (pReport, pRegistration, pContext, pDispatcherContext).
    let args = [ret, d.report, d.reg, d.context, 0];
    let mut b = Vec::new();
    for a in args {
        b.extend_from_slice(&a.to_le_bytes());
    }
    sp.poke_bytes(esp, &b);
    f.rsp = esp as u64;
    f.rip = handler as u64;
    f.cs = USER_CS as u64;
    f.ss = USER_DS as u64;
    f.ds = USER_DS as u64;
    f.es = USER_DS as u64;
    f.fs = TIB_SEL as u64;
    f.rflags = 0x202;
}

/// A handler returned (through the gate) with its result in EAX.
pub fn handler_returned(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) {
    let Some(mut d) = t.xcpt.lock().pop() else { return };
    let result = f.rax as u32;
    let mut sp = p.space.lock();
    if let Some(u) = d.unwind {
        // Unwinding: move to the previous record regardless of the result.
        let prev = rd(&mut sp, d.reg).unwrap_or(xcpt::END_OF_CHAIN);
        wr(&mut sp, t.tib(), prev);
        if prev == u.target || prev == xcpt::END_OF_CHAIN || prev == 0 {
            finish_unwind(&mut sp, f, &u, t);
            return;
        }
        d.reg = prev;
        call_handler(&mut sp, f, &d, d.report.wrapping_sub(16));
        drop(sp);
        t.xcpt.lock().push(d);
        return;
    }
    if result == xcpt::CONTINUE_EXECUTION {
        let flags = rd(&mut sp, d.report + 4).unwrap_or(0);
        if flags & xcpt::EH_NONCONTINUABLE == 0 {
            load_context(&mut sp, d.context, f);
            return;
        }
    }
    // Continue the search.
    let prev = rd(&mut sp, d.reg).unwrap_or(xcpt::END_OF_CHAIN);
    if prev == xcpt::END_OF_CHAIN || prev == 0 {
        // Unhandled: restore the faulting state for the report.
        load_context(&mut sp, d.context, f);
        drop(sp);
        default_action(f, p, t, d.code);
        return;
    }
    d.reg = prev;
    call_handler(&mut sp, f, &d, d.report.wrapping_sub(16));
    drop(sp);
    t.xcpt.lock().push(d);
}

fn finish_unwind(sp: &mut vm::AddressSpace, f: &mut TrapFrame, u: &Unwind, t: &Arc<Thread>) {
    let _ = sp;
    if u.exit {
        t.exit_unwind.store(2, core::sync::atomic::Ordering::Relaxed);
    }
    // Dispatches of exceptions raised further down the stack than where
    // execution resumes are over (their handlers will never return).
    let resume_sp = u.resume.rsp as u32;
    t.xcpt.lock().retain(|d| d.report > resume_sp);
    *f = u.resume;
    if u.target_ip != 0 {
        f.rip = u.target_ip as u64;
    }
    f.rax = 0;
}

/// DosUnwindException(phandler, pTargetIP, pReport)
pub fn dos_unwind_exception(c: &mut super::Ctx) -> u32 {
    let (target, target_ip, preport) = (c.arg(0), c.arg(1), c.arg(2));
    let t = c.thread.clone();
    let tib = t.tib();
    // The state the caller resumes in: as if the call returned.
    let mut resume = *c.f;
    {
        let mut sp = c.space();
        let ret = rd(&mut sp, c.f.rsp as u32).unwrap_or(0);
        resume.rip = ret as u64;
        resume.rsp = (c.f.rsp as u32).wrapping_add(4) as u64;
    }
    let proc = c.process.clone();
    let mut sp = proc.space.lock();
    let head = rd(&mut sp, tib).unwrap_or(xcpt::END_OF_CHAIN);
    let u = Unwind { target, target_ip, resume, exit: false };
    if head == target || head == xcpt::END_OF_CHAIN || head == 0 {
        finish_unwind(&mut sp, c.f, &u, &t);
        c.no_return = true;
        return 0;
    }
    // Report record for the unwind (caller's or our own).
    let esp = (c.f.rsp as u32).wrapping_sub(512) & !15;
    let report = if preport != 0 {
        preport
    } else {
        let r = esp.wrapping_sub(REPORT_SIZE);
        let mut b = alloc::vec![0u8; REPORT_SIZE as usize];
        b[0..4].copy_from_slice(&xcpt::UNWIND.to_le_bytes());
        sp.poke_bytes(r, &b);
        r
    };
    let flags = rd(&mut sp, report + 4).unwrap_or(0);
    let unwind_flag = if target == xcpt::END_OF_CHAIN { xcpt::EH_UNWINDING | xcpt::EH_EXIT_UNWIND } else { xcpt::EH_UNWINDING };
    wr(&mut sp, report + 4, flags | unwind_flag);
    let context = report.wrapping_sub(CONTEXT_SIZE) & !15;
    save_context(&mut sp, context, c.f);
    let d = Dispatch { report, context, reg: head, unwind: Some(u), code: xcpt::UNWIND };
    call_handler(&mut sp, c.f, &d, context.wrapping_sub(16));
    drop(sp);
    t.xcpt.lock().push(d);
    c.no_return = true;
    0
}

/// DosRaiseException(pReport)
pub fn dos_raise_exception(c: &mut super::Ctx) -> u32 {
    let p = c.arg(0);
    let mut raw = [0u8; REPORT_SIZE as usize];
    if !c.read(p, &mut raw) {
        return super::err::INVALID_ADDRESS;
    }
    let g = |o: usize| u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]);
    let code = g(0);
    let flags = g(4) & xcpt::EH_NONCONTINUABLE;
    let n = g(16).min(4) as usize;
    let info: Vec<u32> = (0..n).map(|i| g(20 + i * 4)).collect();
    // The exception appears to come from the instruction after the call:
    // build the context as if DosRaiseException returned 0.
    let ret = c.read_u32(c.f.rsp as u32).unwrap_or(0);
    c.f.rip = ret as u64;
    c.f.rsp = (c.f.rsp as u32).wrapping_add(4) as u64;
    c.f.rax = 0;
    let (proc, thread) = (c.process.clone(), c.thread.clone());
    c.no_return = true;
    if !raise(c.f, &proc, &thread, code, flags, &info, ret) {
        default_action(c.f, &proc, &thread, code);
    }
    0
}

/// The thread must end: call its 32-bit exception handlers with
/// XCPT_ASYNC_PROCESS_TERMINATE (killed, or another thread ended the
/// process) or XCPT_PROCESS_TERMINATE (it ended itself), unwinding the
/// whole chain, before it dies. Returns true while handler code is to run
/// (the frame points at it), false when the thread can die now.
pub fn exit_unwind(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) -> bool {
    use core::sync::atomic::Ordering;
    match t.exit_unwind.load(Ordering::Relaxed) {
        1 => return true,
        2 => return false,
        _ => {}
    }
    t.exit_unwind.store(2, Ordering::Relaxed);
    let tib = t.tib();
    if tib == 0 || p.exitlist_tid.load(Ordering::Relaxed) == t.tid {
        return false;
    }
    let mut sp = p.space.lock();
    if !flat32(&sp, f) {
        return false;
    }
    let Some(head) = rd(&mut sp, tib) else { return false };
    if head == xcpt::END_OF_CHAIN || head == 0 {
        return false;
    }
    let code = if t.sync_exit.load(Ordering::Relaxed) { xcpt::PROCESS_TERMINATE } else { xcpt::ASYNC_PROCESS_TERMINATE };
    let esp = (f.rsp as u32).wrapping_sub(512) & !15;
    let report = esp.wrapping_sub(REPORT_SIZE) & !15;
    let mut r = alloc::vec![0u8; REPORT_SIZE as usize];
    r[0..4].copy_from_slice(&code.to_le_bytes());
    r[4..8].copy_from_slice(&(xcpt::EH_UNWINDING | xcpt::EH_EXIT_UNWIND).to_le_bytes());
    if !sp.poke_bytes(report, &r) {
        return false;
    }
    let context = report.wrapping_sub(CONTEXT_SIZE) & !15;
    save_context(&mut sp, context, f);
    let u = Unwind { target: xcpt::END_OF_CHAIN, target_ip: 0, resume: *f, exit: true };
    let d = Dispatch { report, context, reg: head, unwind: Some(u), code };
    call_handler(&mut sp, f, &d, context.wrapping_sub(16));
    drop(sp);
    t.xcpt.lock().push(d);
    t.exit_unwind.store(1, Ordering::Relaxed);
    true
}

/// No handler took the exception: what OS/2 does by default.
pub fn default_action(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>, code: u32) {
    // A fault while the thread is ending (in a termination handler): just
    // let it die.
    if t.exit_requested() {
        t.exit_unwind.store(2, core::sync::atomic::Ordering::Relaxed);
        return;
    }
    match code {
        xcpt::SIGNAL => {
            p.exit(3, 0);
            return;
        }
        xcpt::PROCESS_TERMINATE | xcpt::ASYNC_PROCESS_TERMINATE | xcpt::UNWIND => return,
        _ => {}
    }
    let (sys, what) = match code {
        xcpt::INTEGER_DIVIDE_BY_ZERO => ("SYS3172", "divide by zero"),
        xcpt::ILLEGAL_INSTRUCTION | xcpt::PRIVILEGED_INSTRUCTION => ("SYS3176", "illegal instruction"),
        xcpt::ACCESS_VIOLATION => ("SYS3175", "access violation"),
        c if (0xC000_0090..=0xC000_009A).contains(&c) => ("SYS3173", "floating point error"),
        _ => ("SYS3170", "unexpected exception"),
    };
    crate::println!(
        "\n{sys}: A program generated an {what} at {:08x}.\n{} - {:08x}, process {}, thread {}.",
        f.rip as u32,
        p.exe.name,
        code,
        p.pid,
        t.tid
    );
    crate::println!(
        "TRAP {:04x} ERRCD={:04x} CS:EIP={:04x}:{:08x} SS:ESP={:04x}:{:08x} CR2={:08x}",
        f.vector,
        f.error,
        f.cs,
        f.rip,
        f.ss,
        f.rsp as u32,
        crate::arch::read_cr2()
    );
    crate::println!(
        "EAX={:08x} EBX={:08x} ECX={:08x} EDX={:08x} ESI={:08x} EDI={:08x} EBP={:08x}",
        f.rax as u32,
        f.rbx as u32,
        f.rcx as u32,
        f.rdx as u32,
        f.rsi as u32,
        f.rdi as u32,
        f.rbp as u32
    );
    crate::println!("DS={:04x} ES={:04x} FS={:04x} GS={:04x}", f.ds, f.es, f.fs, f.gs);
    p.exit(2, 0xffff);
}
