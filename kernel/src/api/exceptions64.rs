//! Exception handlers for 64-bit code (docs/API64.md, "Exceptions").
//!
//! The same model as 32-bit code (exceptions.rs): a chain of registration
//! records whose head is the 64-bit TIB's `pexchain`; the kernel builds
//! the report and context records on the user stack and calls each
//! handler in user mode, with the return address in the API gate (slot
//! HANDLER_RETURN_ORDINAL), so its result comes back to the kernel. The
//! records are the 64-bit ones:
//!
//! ```c
//! typedef struct _EXCEPTIONREGISTRATIONRECORD {   /* 16 bytes */
//!     struct _EXCEPTIONREGISTRATIONRECORD *prev_structure;
//!     _ERR *ExceptionHandler;
//! } EXCEPTIONREGISTRATIONRECORD;
//! typedef struct _EXCEPTIONREPORTRECORD {          /* 64 bytes */
//!     ULONG ExceptionNum, fHandlerFlags;
//!     struct _EXCEPTIONREPORTRECORD *NestedExceptionReportRecord;
//!     PVOID ExceptionAddress;
//!     ULONG cParameters, reserved;
//!     ULONG_PTR ExceptionInfo[4];
//! } EXCEPTIONREPORTRECORD;
//! typedef struct _CONTEXT {                         /* 704 bytes */
//!     ULONG ContextFlags, reserved;
//!     ULONG64 rax, rbx, rcx, rdx, rsi, rdi, rbp, rsp, r8 ... r15;
//!     ULONG64 rip, rflags;
//!     ULONG cs, ss, ds, es, fs, gs;
//!     ULONG reserved2[4];
//!     BYTE fxsave[512];   /* 16-byte aligned; not filled yet */
//! } CONTEXTRECORD;
//! ```
//!
//! Handlers are `ULONG handler(PEXCEPTIONREPORTRECORD,
//! PEXCEPTIONREGISTRATIONRECORD, PCONTEXTRECORD, PVOID)` (System V). The end
//! of the chain is -1 (all 64 bits). Registration records and reports may
//! be anywhere in the 64-bit address space (above 4 GB too); the records
//! the kernel builds go on the faulting stack.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::exceptions::{default_action, finish_unwind, xcpt, Dispatch, Unwind, HANDLER_RETURN_ORDINAL};
use super::{err, Ctx};
use crate::arch::gdt::{TIB_SEL, USER_CS, USER_CS64, USER_DS};
use crate::arch::idt::TrapFrame;
use crate::loader::gate;
use crate::task::process::{info, info64_base, TIB64_0};
use crate::task::thread::Thread;
use crate::task::Process;
use crate::vm::AddressSpace;

pub const REPORT_SIZE: u32 = 64;
pub const CONTEXT_SIZE: u32 = 704;
const END: u64 = u64::MAX;

/// Context record offsets.
mod ctx {
    pub const FLAGS: u32 = 0;
    /// rax rbx rcx rdx rsi rdi rbp rsp r8 ... r15, 8 bytes each.
    pub const REGS: u32 = 8;
    pub const RIP: u32 = 136;
    pub const RFLAGS: u32 = 144;
    pub const CS: u32 = 152;
    pub const SS: u32 = 156;
    pub const DS: u32 = 160;
    pub const ES: u32 = 164;
    pub const FS: u32 = 168;
    pub const GS: u32 = 172;
}

/// CONTEXT_CONTROL | CONTEXT_INTEGER | CONTEXT_SEGMENTS (no floating
/// point state yet).
const CONTEXT_FULL: u32 = 7;

fn rd64(sp: &mut AddressSpace, a: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    sp.read_bytes64(a, &mut b).then(|| u64::from_le_bytes(b))
}

fn wr64(sp: &mut AddressSpace, a: u64, v: u64) -> bool {
    sp.write_bytes64(a, &v.to_le_bytes())
}

/// The 64-bit TIB's `pexchain` slot of a thread.
fn head_slot(t: &Thread) -> u64 {
    (info64_base() + TIB64_0 + t.tid * info::TIB_STRIDE) as u64
}

/// A record address from the chain: None at its end (-1, or 0).
fn link(v: u64) -> Option<u64> {
    if v == END || v == 0 || v == 0xffff_ffff { None } else { Some(v) }
}

/// Is the trapping code 64-bit?
pub fn is64(f: &TrapFrame) -> bool {
    f.cs as u16 == USER_CS64
}

/// A CPU trap as an OS/2 exception code and its parameters.
fn translate(f: &TrapFrame) -> (u32, Vec<u64>) {
    use xcpt::*;
    match f.vector {
        0 => (INTEGER_DIVIDE_BY_ZERO, Vec::new()),
        1 => (SINGLE_STEP, Vec::new()),
        3 => (BREAKPOINT, Vec::new()),
        4 => (INTEGER_OVERFLOW, Vec::new()),
        6 => (ILLEGAL_INSTRUCTION, Vec::new()),
        14 => (ACCESS_VIOLATION, alloc::vec![if f.error & 2 != 0 { 2 } else { 1 }, crate::arch::read_cr2()]),
        13 | 11 | 12 | 10 => (ACCESS_VIOLATION, alloc::vec![0, END]),
        16 | 19 => (FLOAT_INVALID_OPERATION, Vec::new()),
        17 => (DATATYPE_MISALIGNMENT, Vec::new()),
        _ => (ACCESS_VIOLATION, alloc::vec![0, 0]),
    }
}

fn regs(f: &TrapFrame) -> [u64; 16] {
    [f.rax, f.rbx, f.rcx, f.rdx, f.rsi, f.rdi, f.rbp, f.rsp, f.r8, f.r9, f.r10, f.r11, f.r12, f.r13, f.r14, f.r15]
}

/// Fill a context record from a trap frame.
pub fn save_context(sp: &mut AddressSpace, at: u64, f: &TrapFrame) {
    let mut b = alloc::vec![0u8; CONTEXT_SIZE as usize];
    let put32 = |b: &mut Vec<u8>, o: u32, v: u32| b[o as usize..o as usize + 4].copy_from_slice(&v.to_le_bytes());
    let put64 = |b: &mut Vec<u8>, o: u32, v: u64| b[o as usize..o as usize + 8].copy_from_slice(&v.to_le_bytes());
    put32(&mut b, ctx::FLAGS, CONTEXT_FULL);
    for (i, r) in regs(f).iter().enumerate() {
        put64(&mut b, ctx::REGS + 8 * i as u32, *r);
    }
    put64(&mut b, ctx::RIP, f.rip);
    put64(&mut b, ctx::RFLAGS, f.rflags);
    for (o, v) in [(ctx::CS, f.cs), (ctx::SS, f.ss), (ctx::DS, f.ds), (ctx::ES, f.es), (ctx::FS, f.fs), (ctx::GS, f.gs)] {
        put32(&mut b, o, v as u32);
    }
    sp.write_bytes64(at, &b);
}

/// Load a (possibly handler-modified) context record into a trap frame.
fn load_context(sp: &mut AddressSpace, at: u64, f: &mut TrapFrame) {
    let mut b = alloc::vec![0u8; ctx::GS as usize + 4];
    if !sp.read_bytes64(at, &mut b) {
        return;
    }
    let g64 = |o: u32| u64::from_le_bytes(b[o as usize..o as usize + 8].try_into().unwrap());
    let g32 = |o: u32| u32::from_le_bytes(b[o as usize..o as usize + 4].try_into().unwrap()) as u64;
    let r: Vec<u64> = (0..16).map(|i| g64(ctx::REGS + 8 * i)).collect();
    (f.rax, f.rbx, f.rcx, f.rdx, f.rsi, f.rdi, f.rbp, f.rsp) = (r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]);
    (f.r8, f.r9, f.r10, f.r11, f.r12, f.r13, f.r14, f.r15) = (r[8], r[9], r[10], r[11], r[12], r[13], r[14], r[15]);
    f.rip = g64(ctx::RIP);
    // User code may not set IOPL, VM or other system flags.
    f.rflags = (f.rflags & !0xcd5) | (g64(ctx::RFLAGS) & 0xcd5) | 0x202;
    // Long mode, or the 32-bit code of a thunk (before_user_return checks
    // the rest).
    let cs = g32(ctx::CS) & 0xffff;
    f.cs = if cs == USER_CS as u64 { cs } else { USER_CS64 as u64 };
    for (o, s) in [(ctx::SS, &mut f.ss), (ctx::DS, &mut f.ds), (ctx::ES, &mut f.es), (ctx::FS, &mut f.fs), (ctx::GS, &mut f.gs)] {
        *s = g32(o) & 0xffff;
    }
}

/// Build the report and context records below `stack` (past the red
/// zone); returns their addresses.
fn build_records(sp: &mut AddressSpace, f: &TrapFrame, stack: u64, code: u32, flags: u32, info: &[u64], address: u64) -> Option<(u64, u64)> {
    let base = stack.wrapping_sub(256) & !15;
    let context = base.wrapping_sub(CONTEXT_SIZE as u64) & !15;
    let report = context.wrapping_sub(REPORT_SIZE as u64) & !15;
    let mut r = alloc::vec![0u8; REPORT_SIZE as usize];
    r[0..4].copy_from_slice(&code.to_le_bytes());
    r[4..8].copy_from_slice(&flags.to_le_bytes());
    r[16..24].copy_from_slice(&address.to_le_bytes());
    r[24..28].copy_from_slice(&(info.len().min(4) as u32).to_le_bytes());
    for (i, v) in info.iter().take(4).enumerate() {
        r[32 + i * 8..40 + i * 8].copy_from_slice(&v.to_le_bytes());
    }
    if !sp.write_bytes64(report, &r) {
        return None;
    }
    save_context(sp, context, f);
    Some((report, context))
}

/// Set up the frame to call the handler of `d.reg`:
/// handler(report, registration, context, 0), returning into the gate.
fn call_handler(sp: &mut AddressSpace, f: &mut TrapFrame, d: &Dispatch) {
    let handler = rd64(sp, d.reg + 8).unwrap_or(0);
    let ret = gate::slot_address(0, HANDLER_RETURN_ORDINAL) as u64;
    let rsp = (d.stack & !15) - 8;
    wr64(sp, rsp, ret);
    *f = TrapFrame {
        rip: handler,
        rsp,
        rdi: d.report,
        rsi: d.reg,
        rdx: d.context,
        rcx: 0,
        cs: USER_CS64 as u64,
        ss: USER_DS as u64,
        ds: USER_DS as u64,
        es: USER_DS as u64,
        fs: TIB_SEL as u64,
        gs: 0,
        rflags: 0x202,
        ..*f
    };
}

/// A CPU exception in 64-bit code.
pub fn deliver(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) {
    let (code, info) = translate(f);
    if super::tracing() {
        crate::trace!("    64-bit exception {code:x} {info:x?} at {:x} vector {} error {:x} rsp {:x}", f.rip, f.vector, f.error, f.rsp);
    }
    if !raise(f, p, t, code, 0, &info, f.rip) {
        default_action(f, p, t, code);
    }
}

/// Start dispatching an exception in 64-bit code: false if the thread has
/// no handler (the caller applies the default action).
pub fn raise(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>, code: u32, flags: u32, info: &[u64], address: u64) -> bool {
    let mut sp = p.space.lock();
    let Some(head) = rd64(&mut sp, head_slot(t)).and_then(link) else { return false };
    let stack = f.rsp;
    // Dispatches whose records are above the stack pointer now were left
    // by a handler that jumped out instead of returning.
    t.xcpt.lock().retain(|d| d.stack > stack);
    let nested = !t.xcpt.lock().is_empty();
    let flags = flags | if nested { xcpt::EH_NESTED_CALL } else { 0 };
    let Some((report, context)) = build_records(&mut sp, f, stack, code, flags, info, address) else { return false };
    let d = Dispatch { report, context, reg: head, unwind: None, code, stack: report.wrapping_sub(16), is64: true };
    call_handler(&mut sp, f, &d);
    drop(sp);
    t.xcpt.lock().push(d);
    true
}

/// A 64-bit handler returned through the gate with its result in RAX.
pub fn handler_returned(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>, mut d: Dispatch) {
    let result = f.rax as u32;
    let mut sp = p.space.lock();
    let prev = rd64(&mut sp, d.reg).unwrap_or(END);
    if let Some(u) = d.unwind {
        // Unwinding: the record is off the chain, whatever the result.
        wr64(&mut sp, head_slot(t), prev);
        match link(prev) {
            Some(r) if r != u.target => {
                d.reg = r;
                call_handler(&mut sp, f, &d);
                drop(sp);
                t.xcpt.lock().push(d);
            }
            _ => finish_unwind(f, &u, t),
        }
        return;
    }
    if result == xcpt::CONTINUE_EXECUTION {
        let flags = rd64(&mut sp, d.report).unwrap_or(0) >> 32;
        if flags as u32 & xcpt::EH_NONCONTINUABLE == 0 {
            load_context(&mut sp, d.context, f);
            return;
        }
    }
    match link(prev) {
        Some(r) => {
            d.reg = r;
            call_handler(&mut sp, f, &d);
            drop(sp);
            t.xcpt.lock().push(d);
        }
        None => {
            // Unhandled: the faulting state for the report.
            load_context(&mut sp, d.context, f);
            drop(sp);
            default_action(f, p, t, d.code);
        }
    }
}

/// The thread must end: its 64-bit handlers see XCPT_(ASYNC_)PROCESS_TERMINATE
/// while the whole chain unwinds. True while handler code is to run.
pub fn exit_unwind(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) -> bool {
    use core::sync::atomic::Ordering;
    let mut sp = p.space.lock();
    let Some(head) = rd64(&mut sp, head_slot(t)).and_then(link) else { return false };
    let code = if t.sync_exit.load(Ordering::Relaxed) { xcpt::PROCESS_TERMINATE } else { xcpt::ASYNC_PROCESS_TERMINATE };
    let Some((report, context)) =
        build_records(&mut sp, f, f.rsp.wrapping_sub(256), code, xcpt::EH_UNWINDING | xcpt::EH_EXIT_UNWIND, &[], f.rip)
    else {
        return false;
    };
    let u = Unwind { target: END, target_ip: 0, resume: *f, exit: true };
    let d = Dispatch { report, context, reg: head, unwind: Some(u), code, stack: report.wrapping_sub(16), is64: true };
    call_handler(&mut sp, f, &d);
    drop(sp);
    t.xcpt.lock().push(d);
    t.exit_unwind.store(1, Ordering::Relaxed);
    true
}

/// DosSetExceptionHandler(pERegRec)
pub fn dos_set_exception_handler(c: &mut Ctx) -> u32 {
    let rec = c.uptr(0);
    let slot = head_slot(&c.thread);
    let mut sp = c.space();
    let Some(prev) = rd64(&mut sp, slot) else { return err::INVALID_ADDRESS };
    if rec == 0 || !wr64(&mut sp, rec, if prev == 0 { END } else { prev }) {
        return err::INVALID_ADDRESS;
    }
    wr64(&mut sp, slot, rec);
    0
}

/// DosUnsetExceptionHandler(pERegRec): takes it off the chain wherever it is.
pub fn dos_unset_exception_handler(c: &mut Ctx) -> u32 {
    let rec = c.uptr(0);
    let slot = head_slot(&c.thread);
    let mut sp = c.space();
    let Some(next) = rd64(&mut sp, rec) else { return err::INVALID_ADDRESS };
    let mut at = slot;
    for _ in 0..4096 {
        let Some(v) = rd64(&mut sp, at) else { break };
        if v == rec {
            wr64(&mut sp, at, next);
            return 0;
        }
        match link(v) {
            Some(r) => at = r,
            None => break,
        }
    }
    err::INVALID_PARAMETER
}

/// DosRaiseException(pReport)
pub fn dos_raise_exception(c: &mut Ctx) -> u32 {
    let p = c.uptr(0);
    let mut raw = [0u8; REPORT_SIZE as usize];
    if !c.uread(p, &mut raw) {
        return err::INVALID_ADDRESS;
    }
    let g32 = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let g64 = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().unwrap());
    let code = g32(0);
    let flags = g32(4) & xcpt::EH_NONCONTINUABLE;
    let info: Vec<u64> = (0..g32(24).min(4) as usize).map(|i| g64(32 + i * 8)).collect();
    // As if DosRaiseException returned 0.
    let mut b = [0u8; 8];
    c.uread(c.f.rsp, &mut b);
    let ret = u64::from_le_bytes(b);
    c.f.rip = ret;
    c.f.rsp += 8;
    c.f.rax = 0;
    let (proc, thread) = (c.process.clone(), c.thread.clone());
    c.no_return = true;
    if !raise(c.f, &proc, &thread, code, flags, &info, ret) {
        default_action(c.f, &proc, &thread, code);
    }
    0
}

/// DosUnwindException(pERegRec target, pTargetIP, pReport): calls the
/// handlers above `target` with EH_UNWINDING, then goes on at pTargetIP
/// (or returns, when it is 0) with the caller's stack.
pub fn dos_unwind_exception(c: &mut Ctx) -> u32 {
    let (target, target_ip, preport) = (c.arg64(0), c.arg64(1), c.uptr(2));
    let target = link(target).unwrap_or(END);
    let t = c.thread.clone();
    let mut resume = *c.f;
    {
        let mut b = [0u8; 8];
        c.uread(c.f.rsp, &mut b);
        resume.rip = u64::from_le_bytes(b);
        resume.rsp = c.f.rsp + 8;
    }
    let proc = c.process.clone();
    let mut sp = proc.space.lock();
    let u = Unwind { target, target_ip, resume, exit: false };
    let head = rd64(&mut sp, head_slot(&t)).and_then(link);
    let Some(head) = head.filter(|&h| h != target) else {
        finish_unwind(c.f, &u, &t);
        c.no_return = true;
        return 0;
    };
    let below = c.f.rsp.wrapping_sub(512) & !15;
    let own = below.wrapping_sub(REPORT_SIZE as u64) & !15;
    let report = if preport != 0 {
        preport
    } else {
        let mut r = alloc::vec![0u8; REPORT_SIZE as usize];
        r[0..4].copy_from_slice(&xcpt::UNWIND.to_le_bytes());
        sp.write_bytes64(own, &r);
        own
    };
    let mut fl = [0u8; 4];
    sp.read_bytes64(report + 4, &mut fl);
    let unwinding = if target == END { xcpt::EH_UNWINDING | xcpt::EH_EXIT_UNWIND } else { xcpt::EH_UNWINDING };
    sp.write_bytes64(report + 4, &(u32::from_le_bytes(fl) | unwinding).to_le_bytes());
    let context = own.wrapping_sub(CONTEXT_SIZE as u64) & !15;
    save_context(&mut sp, context, c.f);
    let d = Dispatch { report, context, reg: head, unwind: Some(u), code: xcpt::UNWIND, stack: context.wrapping_sub(16), is64: true };
    call_handler(&mut sp, c.f, &d);
    drop(sp);
    t.xcpt.lock().push(d);
    c.no_return = true;
    0
}

/// The SYS317x report of an unhandled exception in 64-bit code.
pub fn report(f: &TrapFrame, p: &Process, t: &Thread, sys: &str, what: &str, code: u32) {
    crate::println!(
        "\n{sys}: A program generated an {what} at {:016x}.\n{} - {code:08x}, process {}, thread {} (64-bit).",
        f.rip, p.exe.name, p.pid, t.tid
    );
    crate::println!("TRAP {:04x} ERRCD={:04x} CS:RIP={:04x}:{:016x} RSP={:016x} CR2={:016x}", f.vector, f.error, f.cs, f.rip, f.rsp, crate::arch::read_cr2());
    crate::println!("RAX={:016x} RBX={:016x} RCX={:016x} RDX={:016x}", f.rax, f.rbx, f.rcx, f.rdx);
    crate::println!("RSI={:016x} RDI={:016x} RBP={:016x} R8 ={:016x}", f.rsi, f.rdi, f.rbp, f.r8);
    crate::println!("R9 ={:016x} R10={:016x} R11={:016x} R12={:016x}", f.r9, f.r10, f.r11, f.r12);
    crate::println!("R13={:016x} R14={:016x} R15={:016x}", f.r13, f.r14, f.r15);
    let msg = alloc::format!("\r\n{sys}: A program generated an {what}.\r\n{} at {:#x}\r\n", p.exe.name, f.rip);
    crate::console::write(msg.as_bytes());
}
