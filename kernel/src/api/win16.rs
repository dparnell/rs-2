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
//! The handler is a `_pascal far` 16-bit procedure taking a far pointer
//! to the interrupted registers (INTREGS, below). It is called on the
//! interrupted stack, which then holds from SP up: the far return address
//! (the gate slot `RETURN_ORDINAL`), the pointer, and INTREGS. It may
//! change any register in INTREGS; when it returns (RETF 4) the kernel
//! loads them and resumes after the INT instruction.
//!
//! INTREGS (USHORTs): AX, BX, CX, DX, SI, DI, BP, DS, ES, FLAGS, IP, CS,
//! the interrupt number, SP.
//!
//! | Ordinal | Call |
//! |---------|------|
//! | 420 | `Rs2Win16SetIntHandler(ULONG pfnHandler16)` (32-bit `_System`; 0 removes it) |

use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use super::Ctx;
use crate::arch::idt::TrapFrame;
use crate::loader::gate;
use crate::task::process::Process;
use crate::vm;

/// Gate slot (module 0) the handler returns to; no OS/2 DOSCALLS export
/// has this ordinal.
pub const RETURN_ORDINAL: u32 = 21;

const REGS: usize = 14;
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

/// RS2API 420: Rs2Win16SetIntHandler(pfnHandler16).
pub fn set_int_handler(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    c.process.win16_int.store(h, Ordering::Relaxed);
    0
}

/// A protection fault in user code: if it is an INT instruction in 16-bit
/// code of a process with a Win16 interrupt handler, enter the handler.
pub fn reflect(f: &mut TrapFrame, p: &Arc<Process>) -> bool {
    let handler = p.win16_int.load(Ordering::Relaxed);
    // The error code of a fault on a gate the program may not use: IDT
    // bit and the vector.
    if handler == 0 || f.vector != 13 || f.error & 3 != 2 {
        return false;
    }
    let vector = (f.error >> 3) as u8;
    let mut sp = p.space.lock();
    let cs = f.cs as u16;
    let ss = f.ss as u16;
    if vm::desc_big(sp.descriptor(cs)) || vm::desc_big(sp.descriptor(ss)) {
        return false;
    }
    let ip = f.rip as u16;
    let Some(lin) = sp.linear(cs, ip as u32) else { return false };
    let mut insn = [0u8; 2];
    if !sp.read_bytes(lin, &mut insn) || insn != [0xcd, vector] {
        return false;
    }
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
        ip.wrapping_add(2),
        cs,
        vector as u16,
        f.rsp as u16,
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
pub fn returned(f: &mut TrapFrame, p: &Arc<Process>) {
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
    let _ = INTNO;
}
