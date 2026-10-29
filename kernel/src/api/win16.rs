//! Kernel support for rs2's Win16 layer (docs/WIN16.md): Windows programs
//! run as OS/2 processes whose KERNEL, USER and GDI are WIN16.DLL.
//!
//! Windows programs execute software interrupts: INT 21h (DOS, also
//! through KERNEL's DOS3Call), INT 2Fh, the floating point emulator's INT
//! 34h-3Dh, and register-based KERNEL entries (InitTask) that WIN16.DLL
//! implements as an INT of their own. On OS/2 an INT in an application is
//! a protection fault; for a process that registered an interrupt handler
//! with `Rs2Win16SetIntHandler` the kernel reflects it there instead, as
//! Windows' DPMI host reflects them into KERNEL.
//!
//! Protection faults in the program's 16-bit code go to the handler too
//! (the interrupt number is 100h + the exception number), so it can
//! emulate what Windows tolerates (an invalid selector loaded into ES).
//!
//! The handler is a `_pascal far` 16-bit procedure taking a far pointer
//! to the interrupted registers (INTREGS, below). It is called on the
//! interrupted stack, which then holds from SP up: the far return address
//! (the gate slot `RETURN_ORDINAL`), the pointer, and INTREGS. It may
//! change any register in INTREGS; when it returns (RETF 4) the kernel
//! loads them and resumes after the INT instruction (at IP, for a
//! fault). A fault the handler leaves alone (the number still 100h +
//! the exception) is delivered as the exception it was.
//!
//! INTREGS (USHORTs): AX, BX, CX, DX, SI, DI, BP, DS, ES, FLAGS, IP, CS,
//! the interrupt number, SP, the fault's error code.
//!
//! | Ordinal | Call |
//! |---------|------|
//! | 420 | `Rs2Win16SetIntHandler(ULONG pfnHandler16)` (32-bit `_System`; 0 removes it) |
//! | 421 | `Rs2Win16Selector(ULONG op, ULONG sel, ULONG value)`: KERNEL's selector functions (below) |
//! | 422 | `Rs2Win16ReadTask(ULONG pid, ULONG sel, ULONG off, PVOID buf, ULONG cb)`: read another Windows task's memory (TOOLHELP: debuggers' copies of the debuggee's segments); the bytes read |
//! | 423 | `Rs2Win16Debug(ULONG op, ULONG pid, ULONG a, ULONG b, ULONG c)`: debug another Windows task (below) |

use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use super::Ctx;
use crate::arch::idt::TrapFrame;
use crate::loader::gate;
use crate::task::process::Process;
use crate::task::thread::Thread;
use crate::vm;

/// Gate slot (module 0) the handler returns to; no OS/2 DOSCALLS export
/// has this ordinal.
pub const RETURN_ORDINAL: u32 = 21;

const REGS: usize = 15;
const AX: usize = 0;
const BX: usize = 1;
const CX: usize = 2;
const DX: usize = 3;
const SI: usize = 4;
const DI: usize = 5;
const BP: usize = 6;
const DS: usize = 7;
const ES: usize = 8;
const FLAGS: usize = 9;
const IP: usize = 10;
const CS: usize = 11;
const INTNO: usize = 12;
const SP: usize = 13;
const ERROR: usize = 14;
/// The interrupt number of a reflected fault: 100h + the exception.
const FAULT: u16 = 0x100;

/// RS2API 420: Rs2Win16SetIntHandler(pfnHandler16).
pub fn set_int_handler(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    c.process.win16_int.store(h, Ordering::Relaxed);
    0
}

/// RS2API 422: Rs2Win16ReadTask(pid, sel, off, buf, cb): up to `cb`
/// bytes (64 KB at most) at `sel:off` in process `pid` into `buf`. Both
/// processes must be Windows programs (an interrupt handler registered);
/// pages that are not there read as zeros. The bytes read (0 if `sel` is
/// not a selector there).
pub fn read_task(c: &mut Ctx) -> u32 {
    let (pid, sel, off, buf, cb) = (c.arg(0), c.arg(1) as u16, c.arg(2), c.arg(3), c.arg(4).min(0x1_0000));
    if c.process.win16_int.load(Ordering::Relaxed) == 0 {
        return 0;
    }
    let Some(p) = crate::task::process::by_pid(pid) else { return 0 };
    if p.win16_int.load(Ordering::Relaxed) == 0 || sel & 4 == 0 || sel >> 3 == 0 {
        return 0;
    }
    // Read into a copy first: the two address spaces are never locked at
    // once.
    let data = {
        let mut sp = p.space.lock();
        let d = sp.descriptor(sel);
        if d == 0 {
            return 0;
        }
        let mut limit = (d & 0xffff) as u32 | ((d >> 32) & 0xf_0000) as u32;
        if d & (1 << 55) != 0 {
            limit = limit << 12 | 0xfff;
        }
        if off > limit {
            return 0;
        }
        let n = cb.min(limit - off + 1);
        let Some(lin) = sp.linear(sel, off) else { return 0 };
        let mut v = alloc::vec![0u8; n as usize];
        // Page by page: one that is not there stays zero.
        let mut i = 0u32;
        while i < n {
            let step = (0x1000 - (lin.wrapping_add(i) & 0xfff)).min(n - i);
            let _ = sp.read_bytes(lin.wrapping_add(i), &mut v[i as usize..(i + step) as usize]);
            i += step;
        }
        v
    };
    if !c.write(buf, &data) {
        return 0;
    }
    data.len() as u32
}

/// RS2API 421: Rs2Win16Selector(op, sel, value), for KERNEL's selector
/// functions (AllocSelector, AllocCStoDSAlias, SetSelectorBase...) and
/// DPMI's. Selectors made here are aliases (vm alias tiles: an LDT entry
/// not describing its own tile). `op`: 0 a new selector like `sel` (a
/// copy of its descriptor; `value` 1 makes it data, 2 16-bit code, 0
/// keeps the type), 1 the base of `sel`, 2 set the base of an alias, 3
/// set its limit, 4 free an alias, 5 the limit of `sel`, 6 the selector
/// of segment `value` (1-based) of the module whose handle is `sel`
/// (0: the number of segments), 9 `value` new consecutive selectors (8
/// apart, data, base 0, limit 0: DPMI's allocation, for huge blocks): the
/// first, 8 the handle of the module one of whose
/// segments `sel` is, 7 make `sel` itself data (`value` 1) or
/// 16-bit code (2), its base and limit kept. Returns the
/// selector or value, 0 (or 0xFFFFFFFF for 1 and 5) on error; 2-4 return
/// 1 on success.
pub fn selector(c: &mut Ctx) -> u32 {
    let (op, sel, value) = (c.arg(0), c.arg(1) as u16, c.arg(2));
    if op == 9 {
        let n = value.clamp(1, 256);
        let mut sp = c.process.space.lock();
        let Some(first) = sp.find_private(n * 0x1_0000) else { return 0 };
        for i in 0..n {
            let Ok(sel) = sp.alias_at(first + i * 0x1_0000, 0, 0, vm::SegKind::Data) else { return 0 };
            sp.ldt[(sel >> 3) as usize] |= 1 << 41;
        }
        return vm::tile_selector(first) as u32;
    }
    if op == 8 {
        // This process's program or a DLL: another program's segments may
        // have the same selectors in its own LDT.
        let s = sel | 7;
        let exe = c.process.exe.clone();
        return crate::loader::all_modules()
            .iter()
            .find(|m| (m.is_dll || Arc::ptr_eq(m, &exe)) && m.objects.iter().any(|o| o.selector() == s))
            .map_or(0, |m| m.hmte as u32);
    }
    if op == 6 {
        let Some(m) = crate::loader::by_handle(sel) else { return 0 };
        return match value {
            0 => m.objects.len() as u32,
            n => m.objects.get(n as usize - 1).map_or(0, |o| o.selector() as u32),
        };
    }
    let mut sp = c.process.space.lock();
    let ldt = |sel: u16| sel & 4 != 0 && (sel >> 3) != 0;
    let d = if ldt(sel) { sp.descriptor(sel) } else { 0 };
    let base = ((d >> 16) & 0xff_ffff) as u32 | ((d >> 56) as u32) << 24;
    let mut limit = (d & 0xffff) as u32 | ((d >> 32) & 0xf_0000) as u32;
    if d & (1 << 55) != 0 {
        limit = limit << 12 | 0xfff;
    }
    // An alias tile: reserved, no descriptors of its own (SegKind::None).
    let alias = |sp: &vm::AddressSpace| {
        let tile = vm::tiled_flat(sel, 0);
        ldt(sel) && sp.region(tile).is_some_and(|r| r.seg == vm::SegKind::None && r.base == tile)
    };
    match op {
        0 => {
            let kind = match value {
                1 => vm::SegKind::Data,
                2 => vm::SegKind::Code16,
                _ if d & (1 << 43) != 0 => vm::SegKind::Code16,
                _ => vm::SegKind::Data,
            };
            if d == 0 && sel != 0 {
                return 0;
            }
            let Ok(new) = sp.alias(base, if d == 0 { 0 } else { limit }, kind) else { return 0 };
            // alias() makes data read-only: Windows' are writable.
            if kind == vm::SegKind::Data {
                let i = (new >> 3) as usize;
                sp.ldt[i] |= 1 << 41;
            }
            new as u32
        }
        1 if d != 0 => base,
        5 if d != 0 => limit,
        2 | 3 if alias(&sp) => {
            let i = (sel >> 3) as usize;
            let (b, l) = if op == 2 { (value, limit) } else { (base, value) };
            let old = sp.ldt[i];
            let g = l > 0xfffff;
            let l = if g { l >> 12 } else { l };
            let mut n = old & !(0xffff | 0xff_ffff << 16 | 0xf << 48 | 0xff << 56 | 1 << 55);
            n |= (l & 0xffff) as u64 | (b as u64 & 0xff_ffff) << 16 | ((l as u64 >> 16) & 0xf) << 48 | (b as u64 >> 24) << 56;
            if g {
                n |= 1 << 55;
            }
            sp.ldt[i] = n;
            1
        }
        4 if alias(&sp) => {
            sp.ldt[(sel >> 3) as usize] = 0;
            let _ = sp.free(vm::tiled_flat(sel, 0));
            1
        }
        7 if d != 0 && (value == 1 || value == 2) => {
            use crate::arch::gdt::access::*;
            let a = PRESENT | DPL3 | CODE_DATA | ACCESSED | if value == 2 { CODE | READABLE } else { WRITABLE };
            let i = (sel >> 3) as usize;
            sp.ldt[i] = sp.ldt[i] & !(0xff << 40 | 1 << 54) | (a as u64) << 40;
            1
        }
        1 | 5 => u32::MAX,
        _ => 0,
    }
}

/// A fault in user code: if it is in 16-bit code of a process with a
/// Win16 interrupt handler (an INT instruction, or another fault outside
/// the handler's own code), enter the handler. Protection faults, and the
/// divide errors, invalid instructions, stack faults and page faults
/// nothing resolved (a write to a code segment's pages), which Windows
/// reports as its programs' faults.
pub fn reflect(f: &mut TrapFrame, p: &Arc<Process>) -> bool {
    let handler = p.win16_int.load(Ordering::Relaxed);
    if handler == 0 || !matches!(f.vector, 0 | 6 | 12 | 13 | 14) {
        return false;
    }
    let mut sp = p.space.lock();
    let cs = f.cs as u16;
    let ss = f.ss as u16;
    if vm::desc_big(sp.descriptor(cs)) || vm::desc_big(sp.descriptor(ss)) {
        return false;
    }
    let ip = f.rip as u16;
    let Some(lin) = sp.linear(cs, ip as u32) else { return false };
    let mut insn = [0u8; 2];
    if !sp.read_bytes(lin, &mut insn) {
        return false;
    }
    // The error code of an INT through a gate the program may not use:
    // IDT bit and the vector.
    let int = f.vector == 13 && f.error & 3 == 2 && insn == [0xcd, (f.error >> 3) as u8];
    if !int && cs == (handler >> 16) as u16 {
        return false;
    }
    let (number, resume) = if int { ((f.error >> 3) as u16, ip.wrapping_add(2)) } else { (FAULT | f.vector as u16, ip) };
    let regs: [u16; REGS] = [
        f.rax as u16,
        f.rbx as u16,
        f.rcx as u16,
        f.rdx as u16,
        f.rsi as u16,
        f.rdi as u16,
        f.rbp as u16,
        f.ds as u16,
        f.es as u16,
        f.rflags as u16,
        resume,
        cs,
        number,
        f.rsp as u16,
        f.error as u16,
    ];
    let sp16 = (f.rsp as u16).wrapping_sub((REGS * 2 + 8) as u16);
    let regs_off = sp16.wrapping_add(8);
    let ret = gate::slot_address(0, RETURN_ORDINAL);
    let mut b = alloc::vec::Vec::with_capacity(REGS * 2 + 8);
    for w in [ret as u16, vm::tile_selector(ret), regs_off, ss] {
        b.extend_from_slice(&w.to_le_bytes());
    }
    for w in regs {
        b.extend_from_slice(&w.to_le_bytes());
    }
    let base = vm::desc_base(sp.descriptor(ss));
    if !sp.poke_bytes(base.wrapping_add(sp16 as u32), &b) {
        return false;
    }
    f.rsp = sp16 as u64;
    f.cs = (handler >> 16) as u64;
    f.rip = (handler & 0xffff) as u64;
    true
}

/// The handler returned to the gate slot: SS:SP is at INTREGS.
pub fn returned(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) {
    let mut sp = p.space.lock();
    let ss = f.ss as u16;
    let base = vm::desc_base(sp.descriptor(ss));
    let mut b = [0u8; REGS * 2];
    if !sp.read_bytes(base.wrapping_add(f.rsp as u32 & 0xffff), &mut b) {
        return;
    }
    let r = |i: usize| u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]) as u64;
    f.rax = r(AX);
    f.rbx = r(BX);
    f.rcx = r(CX);
    f.rdx = r(DX);
    f.rsi = r(SI);
    f.rdi = r(DI);
    f.rbp = r(BP);
    f.ds = r(DS);
    f.es = r(ES);
    // Only the arithmetic and direction flags.
    f.rflags = (f.rflags & !0xcd5) | (r(FLAGS) & 0xcd5) | 0x202;
    f.rip = r(IP);
    f.cs = r(CS);
    f.rsp = r(SP);
    drop(sp);
    let n = r(INTNO) as u16;
    if n & FAULT != 0 {
        // Not handled: the fault as it was.
        f.vector = (n & 0xff) as u64;
        f.error = r(ERROR);
        crate::api::exceptions::deliver(f, p, t);
    }
}

// ---- Debugging another Windows task (RS2API 423) ----
//
// A Windows debugger (TOOLHELP's interrupt callbacks, userland/win16
// toolhelp.rs) attaches to a task: from then on the task's INT 3s
// (breakpoints) and single steps (INT 1, the trap flag) in 16-bit code
// stop the thread that took them, here in its trap. The debugger takes
// the stop's registers, writes the task's memory (its breakpoints) and
// resumes the thread with registers of its choice (another CS:IP, the
// trap flag for a single step).
//
// Stop record (ULONGs): pid, tid, the exception (1 or 3), its error code,
// then EAX EBX ECX EDX ESI EDI EBP ESP EIP EFLAGS CS DS ES FS GS SS.

const DREGS: usize = 16;

struct Stop {
    pid: u32,
    tid: u32,
    vector: u32,
    error: u32,
    regs: [u32; DREGS],
    reported: bool,
    resumed: bool,
}

static STOPS: crate::sync::SpinLock<alloc::vec::Vec<Stop>> = crate::sync::SpinLock::new(alloc::vec::Vec::new());
static STOP_WAIT: crate::task::WaitQueue = crate::task::WaitQueue::new();

fn frame_regs(f: &TrapFrame) -> [u32; DREGS] {
    [
        f.rax as u32, f.rbx as u32, f.rcx as u32, f.rdx as u32, f.rsi as u32, f.rdi as u32, f.rbp as u32, f.rsp as u32,
        f.rip as u32, f.rflags as u32, f.cs as u32, f.ds as u32, f.es as u32, f.fs as u32, f.gs as u32, f.ss as u32,
    ]
}

/// A debug exception of an attached task: the thread waits for its
/// debugger (true: handled, the thread goes on, or ends if told to).
pub fn debug_trap(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) -> bool {
    let dbg = p.win16_debugger.load(Ordering::Relaxed);
    if dbg == 0 || (f.vector != 1 && f.vector != 3) {
        return false;
    }
    if vm::desc_big(p.space.lock().descriptor(f.cs as u16)) {
        return false;
    }
    if crate::task::process::by_pid(dbg).is_none() {
        p.win16_debugger.store(0, Ordering::Relaxed);
        return false;
    }
    let (pid, tid) = (p.pid, t.tid);
    STOPS.lock().push(Stop { pid, tid, vector: f.vector as u32, error: f.error as u32, regs: frame_regs(f), reported: false, resumed: false });
    let resumed = |s: &Stop| s.pid == pid && s.tid == tid && s.resumed;
    loop {
        // A second at a time: the debugger may be gone.
        let deadline = crate::irq::ticks() + crate::irq::HZ;
        let done = STOP_WAIT.wait_until(Some(deadline), || STOPS.lock().iter().any(resumed));
        if done || t.exit_requested() {
            break;
        }
        if crate::task::process::by_pid(dbg).is_none() || p.win16_debugger.load(Ordering::Relaxed) != dbg {
            break;
        }
    }
    let mut stops = STOPS.lock();
    let Some(i) = stops.iter().position(|s| s.pid == pid && s.tid == tid) else { return true };
    let s = stops.remove(i);
    drop(stops);
    if s.resumed {
        let r = s.regs;
        f.rax = r[0] as u64;
        f.rbx = r[1] as u64;
        f.rcx = r[2] as u64;
        f.rdx = r[3] as u64;
        f.rsi = r[4] as u64;
        f.rdi = r[5] as u64;
        f.rbp = r[6] as u64;
        f.rsp = r[7] as u64;
        f.rip = r[8] as u64;
        // The arithmetic flags, the direction flag and the trap flag.
        f.rflags = (f.rflags & !0xdd5) | (r[9] as u64 & 0xdd5) | 0x202;
        f.cs = r[10] as u64;
        f.ds = r[11] as u64;
        f.es = r[12] as u64;
    }
    true
}

/// RS2API 423: Rs2Win16Debug(op, pid, a, b, c). Both processes must be
/// Windows programs.
/// - 0 attach to task `pid`, 1 detach (its stopped threads go on);
/// - 2 write `c` bytes from `b` at `a` (selector << 16 | offset) in it;
/// - 3 the next stop not yet taken of a task this process debugs (`pid`,
///   or 0 for any) into `a` (the stop record, 80 bytes): 1, 0 if none;
/// - 4 resume thread `a` of `pid` with the registers at `b` (16 ULONGs,
///   the stop record's; 0: as it stopped).
pub fn debug(c: &mut Ctx) -> u32 {
    let (op, pid, a, b, cc) = (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4));
    if c.process.win16_int.load(Ordering::Relaxed) == 0 {
        return 0;
    }
    let me = c.process.pid;
    let target = || crate::task::process::by_pid(pid).filter(|p| p.win16_int.load(Ordering::Relaxed) != 0);
    match op {
        0 => match target() {
            Some(p) if pid != me => {
                p.win16_debugger.store(me, Ordering::Relaxed);
                1
            }
            _ => 0,
        },
        1 => {
            if let Some(p) = target() {
                let _ = p.win16_debugger.compare_exchange(me, 0, Ordering::Relaxed, Ordering::Relaxed);
            }
            for s in STOPS.lock().iter_mut().filter(|s| s.pid == pid) {
                s.resumed = true;
            }
            STOP_WAIT.wake_all();
            1
        }
        2 => {
            let Some(p) = target().filter(|p| p.win16_debugger.load(Ordering::Relaxed) == me) else { return 0 };
            let Some(data) = c.read_vec(b, cc.min(0x1_0000)) else { return 0 };
            let mut sp = p.space.lock();
            let (sel, off) = ((a >> 16) as u16, a & 0xffff);
            if sel & 4 == 0 || sp.descriptor(sel) == 0 {
                return 0;
            }
            let Some(lin) = sp.linear(sel, off) else { return 0 };
            sp.poke_bytes(lin, &data) as u32
        }
        3 => {
            let mut rec = None;
            {
                let mut stops = STOPS.lock();
                let mine = |s: &Stop| !s.reported && !s.resumed && (pid == 0 || s.pid == pid) && crate::task::process::by_pid(s.pid).is_some_and(|p| p.win16_debugger.load(Ordering::Relaxed) == me);
                if let Some(s) = stops.iter_mut().find(|s| mine(s)) {
                    s.reported = true;
                    let mut v = alloc::vec::Vec::with_capacity(80);
                    for w in [s.pid, s.tid, s.vector, s.error].iter().chain(s.regs.iter()) {
                        v.extend_from_slice(&w.to_le_bytes());
                    }
                    rec = Some(v);
                }
            }
            match rec {
                Some(v) => c.write(a, &v) as u32,
                None => 0,
            }
        }
        4 => {
            let regs = if b != 0 { c.read_vec(b, (DREGS * 4) as u32) } else { None };
            let mut ok = 0;
            for s in STOPS.lock().iter_mut().filter(|s| s.pid == pid && s.tid == a && !s.resumed) {
                if let Some(r) = &regs {
                    for i in 0..DREGS {
                        s.regs[i] = u32::from_le_bytes([r[i * 4], r[i * 4 + 1], r[i * 4 + 2], r[i * 4 + 3]]);
                    }
                }
                s.resumed = true;
                ok = 1;
            }
            STOP_WAIT.wake_all();
            ok
        }
        _ => 0,
    }
}
