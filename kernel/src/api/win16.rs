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

/// RS2API 421: Rs2Win16Selector(op, sel, value), for KERNEL's selector
/// functions (AllocSelector, AllocCStoDSAlias, SetSelectorBase...) and
/// DPMI's. Selectors made here are aliases (vm alias tiles: an LDT entry
/// not describing its own tile). `op`: 0 a new selector like `sel` (a
/// copy of its descriptor; `value` 1 makes it data, 2 16-bit code, 0
/// keeps the type), 1 the base of `sel`, 2 set the base of an alias, 3
/// set its limit, 4 free an alias, 5 the limit of `sel`, 6 the selector
/// of segment `value` (1-based) of the module whose handle is `sel`
/// (0: the number of segments), 7 make `sel` itself data (`value` 1) or
/// 16-bit code (2), its base and limit kept. Returns the
/// selector or value, 0 (or 0xFFFFFFFF for 1 and 5) on error; 2-4 return
/// 1 on success.
pub fn selector(c: &mut Ctx) -> u32 {
    let (op, sel, value) = (c.arg(0), c.arg(1) as u16, c.arg(2));
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

/// A protection fault in user code: if it is in 16-bit code of a process
/// with a Win16 interrupt handler (an INT instruction, or another fault
/// outside the handler's own code), enter the handler.
pub fn reflect(f: &mut TrapFrame, p: &Arc<Process>) -> bool {
    let handler = p.win16_int.load(Ordering::Relaxed);
    if handler == 0 || f.vector != 13 {
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
    let int = f.error & 3 == 2 && insn == [0xcd, (f.error >> 3) as u8];
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
