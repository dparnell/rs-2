//! An x86 CPU emulator for rs2's DOS sessions.
//!
//! Emulates an 80486SX-class processor (the 8086-80386 instruction set
//! plus the few 486 additions programs use: BSWAP, XADD, CMPXCHG,
//! INVLPG) in real mode and in protected mode (16- and 32-bit segments,
//! the GDT and LDT, the IDT with interrupt, trap and call gates,
//! privilege levels with the stack switch through the TSS, the
//! exceptions with their error codes). Paging and virtual-8086 mode are
//! not emulated: the DPMI host that drives the CPU owns the linear
//! address space and switches modes by changing the CPU state itself.
//! An x87 FPU is optional ([`Cpu::fpu`]).
//!
//! Memory and I/O are the [`Bus`]'s: linear addresses (= physical) and
//! port numbers. [`Cpu::run`] executes instructions until a budget is
//! used, the CPU halts, or the guest executes the *callback* instruction
//! `FE 38 lo hi`, an invalid encoding through which the BIOS and DOS
//! code of the virtual machine (Rust, outside the CPU) is entered: the
//! callback's number is returned and the instruction pointer is behind
//! it, so the machine services it on the guest's registers and resumes.

#![no_std]

extern crate alloc;

mod alu;
mod exec;
pub mod fpu;
mod pm;

/// Memory and I/O as the CPU sees them.
pub trait Bus {
    fn read8(&mut self, addr: u32) -> u8;
    fn write8(&mut self, addr: u32, v: u8);
    fn read16(&mut self, addr: u32) -> u16 {
        self.read8(addr) as u16 | (self.read8(addr.wrapping_add(1)) as u16) << 8
    }
    fn read32(&mut self, addr: u32) -> u32 {
        self.read16(addr) as u32 | (self.read16(addr.wrapping_add(2)) as u32) << 16
    }
    fn write16(&mut self, addr: u32, v: u16) {
        self.write8(addr, v as u8);
        self.write8(addr.wrapping_add(1), (v >> 8) as u8);
    }
    fn write32(&mut self, addr: u32, v: u32) {
        self.write16(addr, v as u16);
        self.write16(addr.wrapping_add(2), (v >> 16) as u16);
    }
    /// Read `size` (1, 2 or 4) bytes from an I/O port.
    fn io_read(&mut self, port: u16, size: u8) -> u32;
    fn io_write(&mut self, port: u16, size: u8, v: u32);
    /// Whether a hardware interrupt is pending (checked while IF is set).
    fn irq_pending(&mut self) -> bool {
        false
    }
    /// Acknowledge the pending hardware interrupt: its vector.
    fn irq_ack(&mut self) -> Option<u8> {
        None
    }
}

/// Segment register numbers (the instruction encoding's).
pub const ES: usize = 0;
pub const CS: usize = 1;
pub const SS: usize = 2;
pub const DS: usize = 3;
pub const FS: usize = 4;
pub const GS: usize = 5;

/// General register numbers.
pub const EAX: usize = 0;
pub const ECX: usize = 1;
pub const EDX: usize = 2;
pub const EBX: usize = 3;
pub const ESP: usize = 4;
pub const EBP: usize = 5;
pub const ESI: usize = 6;
pub const EDI: usize = 7;

pub mod flags {
    pub const CF: u32 = 0x0001;
    pub const PF: u32 = 0x0004;
    pub const AF: u32 = 0x0010;
    pub const ZF: u32 = 0x0040;
    pub const SF: u32 = 0x0080;
    pub const TF: u32 = 0x0100;
    pub const IF: u32 = 0x0200;
    pub const DF: u32 = 0x0400;
    pub const OF: u32 = 0x0800;
    pub const IOPL: u32 = 0x3000;
    pub const NT: u32 = 0x4000;
    pub const RF: u32 = 0x1_0000;
    pub const VM: u32 = 0x2_0000;
    pub const AC: u32 = 0x4_0000;
    /// The arithmetic flags.
    pub const ARITH: u32 = CF | PF | AF | ZF | SF | OF;
}
use flags::*;

/// Segment attributes: the descriptor's access byte in bits 0-7, its
/// flags nibble (AVL, L, D/B, G) in bits 12-15, as VMX keeps them.
pub mod attr {
    pub const ACCESSED: u32 = 0x01;
    /// Data: writable. Code: readable.
    pub const RW: u32 = 0x02;
    /// Data: expand down. Code: conforming.
    pub const DC: u32 = 0x04;
    pub const CODE: u32 = 0x08;
    /// Code or data (not a system descriptor).
    pub const S: u32 = 0x10;
    pub const DPL: u32 = 0x60;
    pub const P: u32 = 0x80;
    /// 32-bit segment (D/B).
    pub const BIG: u32 = 0x4000;
    pub const G: u32 = 0x8000;
    /// A null selector was loaded: any use faults.
    pub const UNUSABLE: u32 = 0x1_0000;
    /// A present, accessed, writable data segment (real mode).
    pub const DATA16: u32 = P | S | RW | ACCESSED;
    /// A present, accessed, readable code segment (real mode).
    pub const CODE16: u32 = P | S | CODE | RW | ACCESSED;
}

/// A segment register with its descriptor cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seg {
    pub sel: u16,
    pub base: u32,
    /// Byte granular limit.
    pub limit: u32,
    pub attr: u32,
}

impl Seg {
    /// A real-mode data segment.
    pub const fn real(sel: u16) -> Seg {
        Seg { sel, base: (sel as u32) << 4, limit: 0xffff, attr: attr::DATA16 }
    }
    pub const fn real_code(sel: u16) -> Seg {
        Seg { sel, base: (sel as u32) << 4, limit: 0xffff, attr: attr::CODE16 }
    }
    pub fn big(&self) -> bool {
        self.attr & attr::BIG != 0
    }
    pub fn dpl(&self) -> u8 {
        ((self.attr & attr::DPL) >> 5) as u8
    }
    pub fn is_code(&self) -> bool {
        self.attr & attr::CODE != 0
    }
}

/// GDTR or IDTR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dtr {
    pub base: u32,
    pub limit: u32,
}

/// Why [`Cpu::run`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The instruction budget is used up.
    Budget,
    /// HLT with no interrupt pending (or interrupts disabled).
    Halt,
    /// The callback instruction `FE 38 n`.
    Callback(u16),
    /// A fault while delivering a double fault (the CPU stops).
    Shutdown,
}

/// An exception raised by an instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub vec: u8,
    pub code: Option<u32>,
}

pub(crate) type R<T> = Result<T, Fault>;

pub(crate) const fn gp(code: u32) -> Fault {
    Fault { vec: 13, code: Some(code) }
}
pub(crate) const fn ss_fault(code: u32) -> Fault {
    Fault { vec: 12, code: Some(code) }
}
pub(crate) const fn np(code: u32) -> Fault {
    Fault { vec: 11, code: Some(code) }
}
pub(crate) const fn ts(code: u32) -> Fault {
    Fault { vec: 10, code: Some(code) }
}
pub(crate) const UD: Fault = Fault { vec: 6, code: None };
pub(crate) const DE: Fault = Fault { vec: 0, code: None };
pub(crate) const NM: Fault = Fault { vec: 7, code: None };

pub struct Cpu {
    pub regs: [u32; 8],
    pub eip: u32,
    pub flags: u32,
    pub seg: [Seg; 6],
    pub cr0: u32,
    pub cr2: u32,
    pub cr3: u32,
    pub dr: [u32; 8],
    pub gdtr: Dtr,
    pub idtr: Dtr,
    pub ldtr: Seg,
    pub tr: Seg,
    /// Current privilege level (protected mode).
    pub cpl: u8,
    pub halted: bool,
    /// The x87 coprocessor, if the machine has one.
    pub fpu: Option<alloc::boxed::Box<fpu::Fpu>>,
    /// Instructions executed.
    pub icount: u64,
    /// Interrupts are held off for one instruction (after MOV SS, STI).
    inhibit: bool,
    exit: Option<Exit>,
    start_eip: u32,
    start_esp: u32,
}

/// CR0 bits.
pub mod cr0 {
    pub const PE: u32 = 0x01;
    pub const MP: u32 = 0x02;
    pub const EM: u32 = 0x04;
    pub const TS: u32 = 0x08;
    pub const ET: u32 = 0x10;
    pub const NE: u32 = 0x20;
    pub const PG: u32 = 0x8000_0000;
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    /// A CPU after reset, in real mode at F000:FFF0.
    pub fn new() -> Cpu {
        let mut seg = [Seg::real(0); 6];
        seg[CS] = Seg { sel: 0xf000, base: 0xffff0000, limit: 0xffff, attr: attr::CODE16 };
        Cpu {
            regs: [0; 8],
            eip: 0xfff0,
            flags: 0x0002,
            seg,
            cr0: cr0::ET,
            cr2: 0,
            cr3: 0,
            dr: [0; 8],
            gdtr: Dtr::default(),
            idtr: Dtr { base: 0, limit: 0x3ff },
            ldtr: Seg::default(),
            tr: Seg::default(),
            cpl: 0,
            halted: false,
            fpu: None,
            icount: 0,
            inhibit: false,
            exit: None,
            start_eip: 0,
            start_esp: 0,
        }
    }

    /// Protected mode (CR0.PE).
    pub fn pm(&self) -> bool {
        self.cr0 & cr0::PE != 0
    }

    /// Load a real-mode segment register.
    pub fn set_seg_real(&mut self, s: usize, sel: u16) {
        self.seg[s].sel = sel;
        self.seg[s].base = (sel as u32) << 4;
        if s == CS {
            self.seg[s] = Seg::real_code(sel);
        }
    }

    /// Load a segment register as the instruction `MOV sreg` would (in
    /// protected mode with the descriptor checks). For the machine.
    pub fn load_seg<B: Bus>(&mut self, bus: &mut B, s: usize, sel: u16) -> Result<(), Fault> {
        self.load_sreg(bus, s, sel)
    }

    /// Jump to `sel:off` as a far JMP would (for the machine).
    pub fn jump_far<B: Bus>(&mut self, bus: &mut B, sel: u16, off: u32) -> Result<(), Fault> {
        self.jmp_far(bus, sel, off)
    }

    pub fn ax(&self) -> u16 {
        self.regs[EAX] as u16
    }
    pub fn bx(&self) -> u16 {
        self.regs[EBX] as u16
    }
    pub fn cx(&self) -> u16 {
        self.regs[ECX] as u16
    }
    pub fn dx(&self) -> u16 {
        self.regs[EDX] as u16
    }
    pub fn si(&self) -> u16 {
        self.regs[ESI] as u16
    }
    pub fn di(&self) -> u16 {
        self.regs[EDI] as u16
    }
    pub fn bp(&self) -> u16 {
        self.regs[EBP] as u16
    }
    pub fn sp(&self) -> u16 {
        self.regs[ESP] as u16
    }
    pub fn al(&self) -> u8 {
        self.regs[EAX] as u8
    }
    pub fn ah(&self) -> u8 {
        (self.regs[EAX] >> 8) as u8
    }
    pub fn bl(&self) -> u8 {
        self.regs[EBX] as u8
    }
    pub fn bh(&self) -> u8 {
        (self.regs[EBX] >> 8) as u8
    }
    pub fn cl(&self) -> u8 {
        self.regs[ECX] as u8
    }
    pub fn ch(&self) -> u8 {
        (self.regs[ECX] >> 8) as u8
    }
    pub fn dl(&self) -> u8 {
        self.regs[EDX] as u8
    }
    pub fn dh(&self) -> u8 {
        (self.regs[EDX] >> 8) as u8
    }
    pub fn set16(&mut self, r: usize, v: u16) {
        self.regs[r] = (self.regs[r] & 0xffff_0000) | v as u32;
    }
    pub fn set_ax(&mut self, v: u16) {
        self.set16(EAX, v)
    }
    pub fn set_bx(&mut self, v: u16) {
        self.set16(EBX, v)
    }
    pub fn set_cx(&mut self, v: u16) {
        self.set16(ECX, v)
    }
    pub fn set_dx(&mut self, v: u16) {
        self.set16(EDX, v)
    }
    pub fn set_si(&mut self, v: u16) {
        self.set16(ESI, v)
    }
    pub fn set_di(&mut self, v: u16) {
        self.set16(EDI, v)
    }
    pub fn set_al(&mut self, v: u8) {
        self.regs[EAX] = (self.regs[EAX] & !0xff) | v as u32;
    }
    pub fn set_ah(&mut self, v: u8) {
        self.regs[EAX] = (self.regs[EAX] & !0xff00) | (v as u32) << 8;
    }
    pub fn set_bl(&mut self, v: u8) {
        self.regs[EBX] = (self.regs[EBX] & !0xff) | v as u32;
    }
    pub fn set_bh(&mut self, v: u8) {
        self.regs[EBX] = (self.regs[EBX] & !0xff00) | (v as u32) << 8;
    }
    pub fn set_cl(&mut self, v: u8) {
        self.regs[ECX] = (self.regs[ECX] & !0xff) | v as u32;
    }
    pub fn set_ch(&mut self, v: u8) {
        self.regs[ECX] = (self.regs[ECX] & !0xff00) | (v as u32) << 8;
    }
    pub fn set_dl(&mut self, v: u8) {
        self.regs[EDX] = (self.regs[EDX] & !0xff) | v as u32;
    }
    pub fn set_dh(&mut self, v: u8) {
        self.regs[EDX] = (self.regs[EDX] & !0xff00) | (v as u32) << 8;
    }
    pub fn set_flag(&mut self, f: u32, on: bool) {
        if on {
            self.flags |= f;
        } else {
            self.flags &= !f;
        }
    }
    pub fn flag(&self, f: u32) -> bool {
        self.flags & f != 0
    }

    /// Linear address of `seg:off` (no checks).
    pub fn lin(&self, s: usize, off: u32) -> u32 {
        self.seg[s].base.wrapping_add(off)
    }

    /// The stack pointer as the stack segment's size uses it.
    pub fn stack_ptr(&self) -> u32 {
        if self.seg[SS].big() { self.regs[ESP] } else { self.regs[ESP] & 0xffff }
    }

    /// Push a word or dword on the guest stack (for the machine).
    pub fn push<B: Bus>(&mut self, bus: &mut B, v: u32, size: u8) -> Result<(), Fault> {
        self.push_sz(bus, v, size)
    }

    /// Pop a word or dword from the guest stack (for the machine).
    pub fn pop<B: Bus>(&mut self, bus: &mut B, size: u8) -> Result<u32, Fault> {
        self.pop_sz(bus, size)
    }

    /// Deliver interrupt `vec` now as `INT vec` would from the current
    /// instruction (the machine reflecting an interrupt into the guest,
    /// e.g. INT 23h). Software interrupt semantics, but no gate DPL check.
    pub fn interrupt<B: Bus>(&mut self, bus: &mut B, vec: u8) -> Result<(), Fault> {
        self.int(bus, vec, false, None, false)
    }

    /// Raise exception `f` as the current instruction's (the machine
    /// faulting on behalf of a callback).
    pub fn raise_fault<B: Bus>(&mut self, bus: &mut B, f: Fault) {
        self.raise(bus, f, false);
    }

    /// Run until `budget` instructions have executed or something stops
    /// the CPU.
    pub fn run<B: Bus>(&mut self, bus: &mut B, budget: u32) -> Exit {
        let mut n = 0;
        while n < budget {
            if self.flags & IF != 0 && !self.inhibit && bus.irq_pending() {
                if let Some(v) = bus.irq_ack() {
                    self.halted = false;
                    self.raise(bus, Fault { vec: v, code: None }, true);
                    if let Some(e) = self.exit.take() {
                        return e;
                    }
                }
            }
            if self.halted {
                return Exit::Halt;
            }
            self.inhibit = false;
            let tf = self.flags & TF != 0;
            self.start_eip = self.eip;
            self.start_esp = self.regs[ESP];
            match self.step(bus) {
                Ok(()) => {
                    if tf && !self.inhibit {
                        self.raise(bus, Fault { vec: 1, code: None }, true);
                    }
                }
                Err(f) => {
                    self.eip = self.start_eip;
                    self.regs[ESP] = self.start_esp;
                    self.raise(bus, f, false);
                }
            }
            n += 1;
            self.icount += 1;
            if let Some(e) = self.exit.take() {
                return e;
            }
        }
        Exit::Budget
    }

    /// Deliver an exception or a hardware interrupt (`ext`: from
    /// outside the program, so not checked against the gate's DPL).
    fn raise<B: Bus>(&mut self, bus: &mut B, f: Fault, ext: bool) {
        let contributory = |v: u8| matches!(v, 0 | 10 | 11 | 12 | 13);
        let mut f = f;
        for _ in 0..3 {
            let saved = (self.regs[ESP], self.seg[SS], self.seg[CS], self.eip, self.cpl, self.flags);
            match self.int(bus, f.vec, false, f.code, ext) {
                Ok(()) => return,
                Err(g) => {
                    (self.regs[ESP], self.seg[SS], self.seg[CS], self.eip, self.cpl, self.flags) = saved;
                    if f.vec == 8 {
                        break;
                    }
                    f = if contributory(f.vec) && contributory(g.vec) { Fault { vec: 8, code: Some(0) } } else { g };
                }
            }
        }
        self.exit = Some(Exit::Shutdown);
        self.halted = true;
    }
}
