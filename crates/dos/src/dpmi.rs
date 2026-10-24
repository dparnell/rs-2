//! The DPMI 0.9 host.
//!
//! A program finds it through INT 2Fh AX=1687h and enters protected mode
//! by calling the entry point it returns. The host then gives the CPU a
//! GDT, an IDT, a TSS and an LDT of its own (in a block of extended
//! memory, the *host area*) and runs the client at ring 3 with LDT
//! selectors. Every IDT gate leads to a ring-0 stub `FE 38 n; IRETD`:
//! the callback is this module, which serves INT 31h, passes software
//! interrupts to the client's protected-mode handlers or reflects them
//! to real mode, delivers exceptions to the client's exception handlers,
//! and maps hardware interrupts (taken through vectors 50h-5Fh while in
//! protected mode) to the client's handlers or to real mode.
//!
//! Reflection to real mode saves the protected-mode CPU state (segment
//! caches, privilege, the stack) and runs the real-mode handler from
//! the IVT with a return address that is another callback, which puts
//! the saved state back: the emulated CPU switches modes by changing its
//! state, as a host using V86 mode would by switching tasks.

use alloc::vec;
use alloc::vec::Vec;

use x86emu::flags::{CF, IF, TF};
use x86emu::{attr, Dtr, Seg, CS, DS, EAX, EBX, ECX, EDI, EDX, ES, ESI, ESP, FS, GS, SS};

use crate::xms::{Block, Owner};
use crate::{bios, Cb, Host, Machine};

pub const CB_FIRST: u16 = 0x200;
pub const CB_LAST: u16 = 0x50f;

/// Callbacks.
const CB_VEC: u16 = 0x200;
const CB_ENTER: u16 = 0x300;
const CB_RMRET: u16 = 0x301;
const CB_SIMRET: u16 = 0x302;
const CB_EXCRET: u16 = 0x303;
const CB_RMCBRET: u16 = 0x304;
const CB_RAW_TO_PM: u16 = 0x305;
const CB_RAW_TO_RM: u16 = 0x306;
const CB_DEFEXC: u16 = 0x380;
const CB_DEFINT: u16 = 0x400;
const CB_RMCB: u16 = 0x500;
const RMCBS: usize = 16;

/// Real-mode stubs in the ROM segment.
const RM_ENTER: u16 = bios::DPMI_CODE;
const RM_RMRET: u16 = bios::DPMI_CODE + 0x08;
const RM_SIMRET: u16 = bios::DPMI_CODE + 0x10;
const RM_RAW: u16 = bios::DPMI_CODE + 0x18;
const RM_SAVE: u16 = bios::DPMI_CODE + 0x20;
const RM_RMCB: u16 = bios::DPMI_CODE + 0x40;

/// The host area: offsets.
const AREA_SIZE: u32 = 0x20000;
const H_GDT: u32 = 0x0000;
const H_IDT: u32 = 0x0100;
const H_TSS: u32 = 0x0900;
const H_VEC: u32 = 0x1000;
const H_DEFINT: u32 = 0x1800;
const H_EXCRET: u32 = 0x2000;
const H_RMCBRET: u32 = 0x2008;
const H_RAW: u32 = 0x2010;
const H_SAVE: u32 = 0x2018;
const H_DEFEXC: u32 = 0x2100;
const H_STACK0: u32 = 0x7000;
const H_LOCKED: u32 = 0x8000;
const H_LOCKED_TOP: u32 = 0xc000;
const H_LDT: u32 = 0x10000;
const LDT_ENTRIES: usize = 8192;

/// GDT selectors.
const SEL_CODE0: u16 = 0x08;
const SEL_DATA0: u16 = 0x10;
const SEL_CODE3: u16 = 0x1b;
const SEL_TSS: u16 = 0x20;
const SEL_LDT: u16 = 0x28;
const SEL_FLAT3: u16 = 0x33;
const SEL_CODE3_16: u16 = 0x3b;
const SEL_STACK3_16: u16 = 0x43;

/// Vectors of hardware interrupts while in protected mode.
const HW_BASE: u8 = 0x50;

/// The CPU state saved while a reflected interrupt runs in real mode.
#[derive(Clone)]
struct PmState {
    regs: [u32; 8],
    seg: [Seg; 6],
    eip: u32,
    esp: u32,
    flags: u32,
    cpl: u8,
    /// TSS ESP0 to restore.
    esp0: u32,
    kind: Reflect,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reflect {
    /// A reflected interrupt: the result flags go into the ring-0 frame.
    Interrupt { ring0: bool },
    /// INT 31h 0300h-0302h: registers go back into the structure.
    Simulate { regs: u32 },
}

#[derive(Clone, Copy, Default)]
struct Rmcb {
    used: bool,
    proc_sel: u16,
    proc_off: u32,
    regs_sel: u16,
    regs_off: u32,
}

pub struct Client {
    pub psp: u16,
    pub bits32: bool,
    /// Linear address of the host area.
    area: u32,
    ldt_used: Vec<bool>,
    pm_vectors: Vec<Option<(u16, u32)>>,
    exc_handlers: Vec<Option<(u16, u32)>>,
    saved: Vec<PmState>,
    /// The real-mode stack for reflections: segment and current top.
    rm_stack: u16,
    rm_sp: u16,
    /// DOS memory blocks: first selector -> (segment, selectors).
    dos_blocks: Vec<(u16, u16, u16)>,
    rmcbs: [Rmcb; RMCBS],
    /// The locked stack's current top (exception handlers, callbacks).
    locked_top: u32,
    /// The PSP's environment segment (restored at the end).
    env_seg: u16,
}

#[derive(Default)]
pub struct Dpmi {
    pub client: Option<Client>,
}

impl Client {
    fn alloc_sel(&mut self, n: usize) -> Option<u16> {
        // The first 16 are kept for 000Dh (allocate specific).
        let mut run = 0;
        for i in 16..LDT_ENTRIES {
            if self.ldt_used[i] {
                run = 0;
                continue;
            }
            run += 1;
            if run == n {
                let first = i + 1 - n;
                for j in first..=i {
                    self.ldt_used[j] = true;
                }
                return Some(((first as u16) << 3) | 7);
            }
        }
        None
    }

    fn sel_ok(&self, sel: u16) -> bool {
        sel & 4 != 0 && ((sel >> 3) as usize) < LDT_ENTRIES && self.ldt_used[(sel >> 3) as usize]
    }

    fn desc_addr(&self, sel: u16) -> u32 {
        self.area + H_LDT + (sel as u32 & !7)
    }
}

fn desc(base: u32, limit: u32, access: u8, flags: u8) -> u64 {
    let (lim, g) = if limit > 0xfffff { (limit >> 12, 0x8) } else { (limit, 0) };
    (lim as u64 & 0xffff)
        | (base as u64 & 0xff_ffff) << 16
        | (access as u64) << 40
        | ((lim as u64 >> 16) & 0xf) << 48
        | ((flags | g) as u64 & 0xf) << 52
        | ((base as u64 >> 24) & 0xff) << 56
}

/// A gate in the IDT.
fn gate(sel: u16, off: u32, dpl: u8) -> u64 {
    (off as u64 & 0xffff) | (sel as u64) << 16 | ((0x8e | dpl << 5) as u64) << 40 | ((off as u64 >> 16) & 0xffff) << 48
}

impl Machine {
    fn dpmi_rw64(&mut self, a: u32) -> u64 {
        self.hw.rd32(a) as u64 | (self.hw.rd32(a + 4) as u64) << 32
    }
    fn dpmi_ww64(&mut self, a: u32, v: u64) {
        self.hw.wr32(a, v as u32);
        self.hw.wr32(a + 4, (v >> 32) as u32);
    }

    pub(crate) fn dpmi_rom_init(&mut self) {
        let rom = (bios::ROM as usize) << 4;
        let put = |m: &mut Machine, off: u16, code: &[u8]| {
            let a = rom + off as usize;
            m.hw.ram[a..a + code.len()].copy_from_slice(code);
        };
        let cb = |n: u16| [0xfe, 0x38, n as u8, (n >> 8) as u8];
        put(self, RM_ENTER, &cb(CB_ENTER));
        put(self, RM_RMRET, &cb(CB_RMRET));
        put(self, RM_SIMRET, &cb(CB_SIMRET));
        put(self, RM_RAW, &cb(CB_RAW_TO_PM));
        put(self, RM_SAVE, &[0xcb]);
        for k in 0..RMCBS as u16 {
            put(self, RM_RMCB + k * 8, &cb(CB_RMCB + k));
        }
    }

    /// INT 2Fh AX=1687h: the host is there.
    pub(crate) fn dpmi_detect<H: Host>(&mut self, host: &mut H) {
        let _ = host;
        if self.cfg.dpmi_kb == 0 {
            return;
        }
        self.cpu.set_ax(0);
        self.cpu.set_bx(1);
        self.cpu.set_cl(4);
        self.cpu.set_dx(0x005a);
        self.cpu.set_si(0);
        self.cpu.seg[ES] = Seg::real(bios::ROM);
        self.cpu.set_di(RM_ENTER);
    }

    fn client(&mut self) -> &mut Client {
        self.dpmi.client.as_mut().unwrap()
    }

    pub(crate) fn dpmi_callback<H: Host>(&mut self, host: &mut H, n: u16) -> Cb {
        if n == CB_ENTER {
            self.dpmi_enter(host);
            return Cb::Done;
        }
        if self.dpmi.client.is_none() {
            host.log(&alloc::format!("dpmi: callback {n:#x} without a client"));
            return Cb::Done;
        }
        match n {
            CB_VEC..=0x2ff => self.dpmi_vector(host, (n - CB_VEC) as u8),
            CB_RMRET | CB_SIMRET => self.dpmi_back_to_pm(),
            CB_EXCRET => self.dpmi_exception_return(),
            CB_RMCBRET => self.dpmi_rmcb_return(),
            CB_RAW_TO_PM => self.dpmi_raw_to_pm(),
            CB_RAW_TO_RM => self.dpmi_raw_to_rm(),
            0x380..=0x39f => {
                let v = (n - CB_DEFEXC) as u8;
                self.dpmi_unhandled(host, v);
            }
            0x400..=0x4ff => self.dpmi_reflect((n - CB_DEFINT) as u8, false),
            0x500..=0x50f => self.dpmi_rmcb_enter((n - CB_RMCB) as usize),
            _ => {}
        }
        Cb::Done
    }

    // ---- Entering protected mode ----

    fn dpmi_enter<H: Host>(&mut self, host: &mut H) {
        let bits32 = self.cpu.ax() & 1 != 0;
        // The far return address of the call to the entry point.
        let ret_ip = self.cpu.pop(&mut self.hw, 2).unwrap_or(0) as u16;
        let ret_cs = self.cpu.pop(&mut self.hw, 2).unwrap_or(0) as u16;
        let psp = self.dos.psp;
        if self.dpmi.client.is_some() {
            // One client per session (as a nested one would need).
            self.cpu.seg[CS] = Seg::real_code(ret_cs);
            self.cpu.eip = ret_ip as u32;
            self.cpu.set_ax(0x8011);
            self.cpu.flags |= CF;
            return;
        }
        // The A20 line is on as long as the client runs (the host area is
        // above 1 MB).
        self.hw.a20 = true;
        let limit = self.ext_limit();
        let Some(area) = self.xms.ext.find(AREA_SIZE, limit) else {
            self.cpu.seg[CS] = Seg::real_code(ret_cs);
            self.cpu.eip = ret_ip as u32;
            self.cpu.set_ax(0x8013);
            self.cpu.flags |= CF;
            return;
        };
        self.hw.grow_ext(area + AREA_SIZE);
        self.xms.ext.blocks.push(Block { start: area, size: AREA_SIZE, owner: Owner::Dpmi(psp), locks: 1 });
        for i in 0..AREA_SIZE {
            self.hw.wr(area + i, 0);
        }
        let rm_stack = self.alloc_mem(host, 0x100, psp).unwrap_or(0);
        self.dpmi_rom_init();
        // GDT.
        let g = area + H_GDT;
        let entries: [u64; 9] = [
            0,
            desc(area, AREA_SIZE - 1, 0x9b, 0x4),
            desc(0, 0xffff_ffff, 0x93, 0x4),
            desc(area, AREA_SIZE - 1, 0xfb, 0x4),
            desc(area + H_TSS, 0x67, 0x89, 0),
            desc(area + H_LDT, (LDT_ENTRIES * 8 - 1) as u32, 0x82, 0),
            desc(0, 0xffff_ffff, 0xf3, 0x4),
            desc(area, 0xffff, 0xfb, 0),
            desc(area + H_LOCKED, H_LOCKED_TOP - H_LOCKED - 1, 0xf3, 0),
        ];
        for (i, e) in entries.iter().enumerate() {
            self.dpmi_ww64(g + i as u32 * 8, *e);
        }
        // IDT: software interrupts from ring 3 for 20h-FFh (but the
        // hardware vectors); exceptions and 00h-1Fh as software
        // interrupts come through #GP.
        for v in 0..256u32 {
            let dpl = if v >= 0x20 && !(HW_BASE as u32..HW_BASE as u32 + 16).contains(&v) { 3 } else { 0 };
            self.dpmi_ww64(area + H_IDT + v * 8, gate(SEL_CODE0, H_VEC + v * 8, dpl));
            let s = area + H_VEC + v * 8;
            let n = CB_VEC + v as u16;
            self.hw.write_bytes(s, &[0xfe, 0x38, n as u8, (n >> 8) as u8, 0xcf]);
            let d = area + H_DEFINT + v * 8;
            let n = CB_DEFINT + v as u16;
            self.hw.write_bytes(d, &[0xfe, 0x38, n as u8, (n >> 8) as u8, 0xcf]);
        }
        for v in 0..32u32 {
            let n = CB_DEFEXC + v as u16;
            self.hw.write_bytes(area + H_DEFEXC + v * 8, &[0xfe, 0x38, n as u8, (n >> 8) as u8, 0xcb]);
        }
        let stub = |m: &mut Machine, off: u32, n: u16| m.hw.write_bytes(area + off, &[0xfe, 0x38, n as u8, (n >> 8) as u8]);
        stub(self, H_EXCRET, CB_EXCRET);
        stub(self, H_RMCBRET, CB_RMCBRET);
        stub(self, H_RAW, CB_RAW_TO_RM);
        self.hw.wr(area + H_SAVE, 0xcb);
        // TSS: the ring-0 stack.
        self.hw.wr32(area + H_TSS + 4, area + H_STACK0);
        self.hw.wr32(area + H_TSS + 8, SEL_DATA0 as u32);
        let mut c = Client {
            psp,
            bits32,
            area,
            ldt_used: vec![false; LDT_ENTRIES],
            pm_vectors: vec![None; 256],
            exc_handlers: vec![None; 32],
            saved: Vec::new(),
            rm_stack,
            rm_sp: 0xffe,
            dos_blocks: Vec::new(),
            rmcbs: [Rmcb::default(); RMCBS],
            locked_top: H_LOCKED_TOP,
            env_seg: 0,
        };
        // The client's first selectors: CS, DS, SS, ES (PSP), environment.
        let (rds, rss) = (self.cpu.seg[DS].sel, self.cpu.seg[SS].sel);
        let mk = |m: &mut Machine, c: &mut Client, base: u32, limit: u32, access: u8, flags: u8| -> u16 {
            let s = c.alloc_sel(1).unwrap();
            m.dpmi_ww64(c.desc_addr(s), desc(base, limit, access, flags));
            s
        };
        let cs = mk(self, &mut c, (ret_cs as u32) << 4, 0xffff, 0xfb, 0);
        let ds = mk(self, &mut c, (rds as u32) << 4, 0xffff, 0xf3, 0);
        let ss = mk(self, &mut c, (rss as u32) << 4, 0xffff, 0xf3, if bits32 { 0x4 } else { 0 });
        let es = mk(self, &mut c, (psp as u32) << 4, 0xff, 0xf3, 0);
        let env = self.hw.rd16(Machine::lin(psp, 0x2c));
        c.env_seg = env;
        if env != 0 {
            let size = (self.mcb(env - 1).2 as u32 * 16).max(16);
            let es_env = mk(self, &mut c, (env as u32) << 4, size - 1, 0xf3, 0);
            self.hw.wr16(Machine::lin(psp, 0x2c), es_env);
        }
        self.dpmi.client = Some(c);
        // Into protected mode, ring 3.
        let flags = self.cpu.flags & !CF;
        self.cpu.cr0 |= 1;
        self.cpu.gdtr = Dtr { base: g, limit: 9 * 8 - 1 };
        self.cpu.idtr = Dtr { base: area + H_IDT, limit: 256 * 8 - 1 };
        self.cpu.ldtr = Seg { sel: SEL_LDT, base: area + H_LDT, limit: (LDT_ENTRIES * 8 - 1) as u32, attr: 0x82 };
        self.cpu.tr = Seg { sel: SEL_TSS, base: area + H_TSS, limit: 0x67, attr: 0x8b };
        self.hw.wr(g + SEL_TSS as u32 + 5, 0x8b);
        self.cpu.cpl = 3;
        let sp = self.cpu.sp();
        let _ = self.cpu.load_seg(&mut self.hw, DS, ds);
        let _ = self.cpu.load_seg(&mut self.hw, ES, es);
        let _ = self.cpu.load_seg(&mut self.hw, SS, ss);
        self.cpu.regs[ESP] = sp as u32;
        let _ = self.cpu.load_seg(&mut self.hw, FS, 0);
        let _ = self.cpu.load_seg(&mut self.hw, GS, 0);
        let _ = self.cpu.jump_far(&mut self.hw, cs, ret_ip as u32);
        // IOPL 3: the client reaches the virtual devices' ports.
        self.cpu.flags = (flags & !(CF | 0x3000)) | 0x3000;
        self.hw.irq_remap = Some(HW_BASE);
        host.log(&alloc::format!("dpmi: {} client entered protected mode (cs={cs:#x})", if bits32 { "32-bit" } else { "16-bit" }));
    }

    // ---- Frames ----

    /// The ring-0 frame of the interrupt that entered a vector stub:
    /// (address of its EIP, whether an error code is below it).
    fn ring0_frame(&mut self, has_code: bool) -> u32 {
        let a = self.cpu.seg[SS].base.wrapping_add(self.cpu.regs[ESP]);
        if has_code { a + 4 } else { a }
    }

    fn frame_get(&mut self, f: u32, i: u32) -> u32 {
        self.hw.rd32(f + i * 4)
    }

    fn frame_set(&mut self, f: u32, i: u32, v: u32) {
        self.hw.wr32(f + i * 4, v);
    }

    /// Set CF (and AX on error) in the flags the client gets back from
    /// INT 31h.
    fn dpmi_ret(&mut self, f: u32, r: Result<(), u16>) {
        let mut fl = self.frame_get(f, 2);
        match r {
            Ok(()) => fl &= !CF,
            Err(e) => {
                fl |= CF;
                self.cpu.set_ax(e);
            }
        }
        self.frame_set(f, 2, fl);
    }

    /// The interrupt stub of `v` was entered.
    fn dpmi_vector<H: Host>(&mut self, host: &mut H, v: u8) {
        if (HW_BASE..HW_BASE + 16).contains(&v) {
            let irq = v - HW_BASE;
            let iv = if irq < 8 { 8 + irq } else { 0x70 + irq - 8 };
            self.dpmi_soft(host, iv, true);
            return;
        }
        if v < 0x20 {
            let has_code = matches!(v, 8 | 10..=14 | 17);
            let f = self.ring0_frame(has_code);
            let code = if has_code { self.hw.rd32(f - 4) } else { 0 };
            // A software interrupt to a DPL 0 gate: #GP with the IDT bit.
            if v == 13 && code & 2 != 0 {
                let iv = (code >> 3) as u8;
                self.cpu.regs[ESP] += 4;
                let f = self.ring0_frame(false);
                let (eip, cs) = (self.frame_get(f, 0), self.frame_get(f, 1) as u16);
                let lin = self.dpmi_lin(cs, eip);
                let len = if self.hw.rd(lin) == 0xcd { 2 } else { 1 };
                self.frame_set(f, 0, eip.wrapping_add(len));
                self.dpmi_soft(host, iv, false);
                return;
            }
            // From the client (ring 3): its handler, or the default.
            let from3 = self.hw.rd32(f + 4) & 3 == 3;
            if from3 {
                if let Some(h) = self.client().exc_handlers[v as usize] {
                    self.dpmi_call_exception(v, code, has_code, h);
                    return;
                }
                if v == 1 || v == 3 {
                    // Debug traps without a handler: ignore.
                    if has_code {
                        self.cpu.regs[ESP] += 4;
                    }
                    return;
                }
            }
            self.dpmi_unhandled(host, v);
            return;
        }
        self.dpmi_soft(host, v, false);
    }

    /// Linear address of a protected-mode sel:off.
    fn dpmi_lin(&mut self, sel: u16, off: u32) -> u32 {
        let base = if sel & 4 != 0 {
            let a = self.dpmi.client.as_ref().map_or(0, |c| c.desc_addr(sel));
            let d = self.dpmi_rw64(a);
            ((d >> 16) & 0xff_ffff) as u32 | ((d >> 56) as u32) << 24
        } else {
            let a = self.cpu.gdtr.base + (sel as u32 & !7);
            let d = self.dpmi_rw64(a);
            ((d >> 16) & 0xff_ffff) as u32 | ((d >> 56) as u32) << 24
        };
        base.wrapping_add(off)
    }

    /// A software or hardware interrupt `v` at ring 0 (frame on the host
    /// stack): INT 31h, the client's handler, or real mode.
    fn dpmi_soft<H: Host>(&mut self, host: &mut H, v: u8, hw: bool) {
        let f = self.ring0_frame(false);
        if v == 0x31 && !hw {
            self.int31(host, f);
            return;
        }
        if v == 0x2f && !hw && self.cpu.ax() == 0x1686 {
            self.cpu.set_ax(0);
            return;
        }
        if let Some((sel, off)) = self.client().pm_vectors[v as usize] {
            // To the client's handler at ring 3, with an IRET frame on its
            // stack.
            let (eip, cs, fl, esp, ss) = (self.frame_get(f, 0), self.frame_get(f, 1), self.frame_get(f, 2), self.frame_get(f, 3), self.frame_get(f, 4));
            if cs & 3 == 3 {
                let bits32 = self.client().bits32;
                let ss_base = self.dpmi_lin(ss as u16, 0);
                let big = self.dpmi_seg_big(ss as u16);
                let sz = if bits32 { 4 } else { 2 };
                let mut sp = if big { esp } else { esp & 0xffff };
                for v in [fl, cs, eip] {
                    sp = sp.wrapping_sub(sz);
                    let a = ss_base.wrapping_add(if big { sp } else { sp & 0xffff });
                    if sz == 4 {
                        self.hw.wr32(a, v);
                    } else {
                        self.hw.wr16(a, v as u16);
                    }
                }
                let nesp = if big { sp } else { (esp & 0xffff_0000) | (sp & 0xffff) };
                self.frame_set(f, 0, off);
                self.frame_set(f, 1, sel as u32);
                self.frame_set(f, 2, fl & !(IF | TF));
                self.frame_set(f, 3, nesp);
                return;
            }
        }
        self.dpmi_reflect(v, true);
    }

    fn dpmi_seg_big(&mut self, sel: u16) -> bool {
        let a = if sel & 4 != 0 { self.dpmi.client.as_ref().map_or(0, |c| c.desc_addr(sel)) } else { self.cpu.gdtr.base + (sel as u32 & !7) };
        self.dpmi_rw64(a) & (1 << 54) != 0
    }

    // ---- Reflection to real mode ----

    /// Run real-mode interrupt `v` and come back here (`ring0`: the
    /// result flags go to the ring-0 frame of a vector stub).
    fn dpmi_reflect(&mut self, v: u8, ring0: bool) {
        self.dpmi_save(Reflect::Interrupt { ring0 });
        let (seg, off) = self.vector(v);
        let fl = self.cpu.flags;
        self.dpmi_to_real();
        let rm = self.client().rm_stack;
        self.cpu.seg[SS] = Seg::real(rm);
        let sp = self.client().rm_sp;
        self.cpu.set16(ESP, sp);
        self.cpu.seg[DS] = Seg::real(rm);
        self.cpu.seg[ES] = Seg::real(rm);
        let _ = self.cpu.push(&mut self.hw, fl & 0xffff & !(IF | TF), 2);
        let _ = self.cpu.push(&mut self.hw, bios::ROM as u32, 2);
        let _ = self.cpu.push(&mut self.hw, RM_RMRET as u32, 2);
        self.cpu.seg[CS] = Seg::real_code(seg);
        self.cpu.eip = off as u32;
        self.cpu.flags &= !(IF | TF);
    }

    fn dpmi_save(&mut self, kind: Reflect) {
        let area = self.client().area;
        let esp0 = self.hw.rd32(area + H_TSS + 4);
        let st = PmState { regs: self.cpu.regs, seg: self.cpu.seg, eip: self.cpu.eip, esp: self.cpu.regs[ESP], flags: self.cpu.flags, cpl: self.cpu.cpl, esp0, kind };
        // Nested entries to ring 0 go below this frame.
        if self.cpu.cpl == 0 {
            let top = self.cpu.regs[ESP].wrapping_sub(16);
            self.hw.wr32(area + H_TSS + 4, top);
        }
        // A nested reflection uses the real-mode stack below this one.
        let c = self.client();
        c.saved.push(st);
        if c.saved.len() > 1 {
            c.rm_sp = c.rm_sp.saturating_sub(0x100);
        }
    }

    /// Switch the CPU to real mode (no segment loads).
    fn dpmi_to_real(&mut self) {
        self.cpu.cr0 &= !1;
        self.cpu.cpl = 0;
        self.cpu.idtr = Dtr { base: 0, limit: 0x3ff };
        self.hw.irq_remap = None;
        for s in [FS, GS] {
            self.cpu.seg[s] = Seg::real(0);
        }
    }

    /// Back from real mode (RMRET or SIMRET): restore the saved state.
    fn dpmi_back_to_pm(&mut self) {
        let Some(st) = self.client().saved.pop() else { return };
        let rflags = self.cpu.flags;
        let regs = self.cpu.regs;
        let rsegs: [u16; 6] = core::array::from_fn(|i| self.cpu.seg[i].sel);
        let rip = self.cpu.eip;
        let area = self.client().area;
        if !self.client().saved.is_empty() {
            let c = self.client();
            c.rm_sp = c.rm_sp.saturating_add(0x100);
        }
        self.cpu.cr0 |= 1;
        self.cpu.seg = st.seg;
        self.cpu.cpl = st.cpl;
        self.cpu.eip = st.eip;
        self.cpu.regs[ESP] = st.esp;
        self.cpu.flags = st.flags;
        self.cpu.idtr = Dtr { base: area + H_IDT, limit: 256 * 8 - 1 };
        self.hw.irq_remap = Some(HW_BASE);
        self.hw.wr32(area + H_TSS + 4, st.esp0);
        match st.kind {
            Reflect::Interrupt { ring0 } => {
                // The arithmetic flags of the handler go back to the caller.
                let m = x86emu::flags::ARITH;
                if ring0 {
                    let f = self.ring0_frame(false);
                    let fl = self.frame_get(f, 2);
                    self.frame_set(f, 2, (fl & !m) | (rflags & m));
                } else {
                    // A default handler stub at ring 3: IRET pops the
                    // caller's flags from its stack; patch them there.
                    let bits32 = self.client().bits32;
                    let ss_base = self.cpu.seg[SS].base;
                    let sp = if self.cpu.seg[SS].big() { self.cpu.regs[ESP] } else { self.cpu.regs[ESP] & 0xffff };
                    let a = ss_base.wrapping_add(sp + if bits32 { 8 } else { 4 });
                    let fl = if bits32 { self.hw.rd32(a) } else { self.hw.rd16(a) as u32 };
                    let nf = (fl & !m) | (rflags & m);
                    if bits32 {
                        self.hw.wr32(a, nf);
                    } else {
                        self.hw.wr16(a, nf as u16);
                    }
                }
            }
            Reflect::Simulate { regs: r } => {
                // The registers into the client's structure; the client's
                // own registers are as they were.
                self.dpmi_store_rm_regs(r, &regs, rflags, rsegs, rip);
                self.cpu.regs = st.regs;
                let f = self.ring0_frame(false);
                self.dpmi_ret(f, Ok(()));
                return;
            }
        }
        // GPRs as the real-mode handler left them.
        let esp = self.cpu.regs[ESP];
        self.cpu.regs = regs;
        self.cpu.regs[ESP] = esp;
    }

    // ---- Exceptions ----

    fn dpmi_call_exception(&mut self, v: u8, code: u32, has_code: bool, (sel, off): (u16, u32)) {
        let f0 = self.cpu.seg[SS].base.wrapping_add(self.cpu.regs[ESP]);
        if has_code {
            self.cpu.regs[ESP] += 4;
        }
        let f = self.ring0_frame(false);
        let (eip, cs, fl, esp, ss) = (self.frame_get(f, 0), self.frame_get(f, 1), self.frame_get(f, 2), self.frame_get(f, 3), self.frame_get(f, 4));
        let _ = f0;
        let bits32 = self.client().bits32;
        let area = self.client().area;
        let top = self.client().locked_top;
        let vals = [if bits32 { H_EXCRET } else { H_EXCRET }, if bits32 { SEL_CODE3 as u32 } else { SEL_CODE3_16 as u32 }, code, eip, cs, fl, esp, ss];
        let sz = if bits32 { 4 } else { 2 };
        let size = sz * vals.len() as u32;
        let base = top - size;
        for (i, v) in vals.iter().enumerate() {
            let a = area + base + i as u32 * sz;
            if sz == 4 {
                self.hw.wr32(a, *v);
            } else {
                self.hw.wr16(a, *v as u16);
            }
        }
        self.client().locked_top = base;
        // IRETD from the stub goes to the handler on the locked stack.
        self.frame_set(f, 0, off);
        self.frame_set(f, 1, sel as u32);
        self.frame_set(f, 2, fl & !(IF | TF));
        if bits32 {
            self.frame_set(f, 3, area + base);
            self.frame_set(f, 4, SEL_FLAT3 as u32);
        } else {
            self.frame_set(f, 3, base - H_LOCKED);
            self.frame_set(f, 4, SEL_STACK3_16 as u32);
        }
        let _ = v;
    }

    /// The client's exception handler returned (RETF to the host).
    fn dpmi_exception_return(&mut self) {
        let bits32 = self.client().bits32;
        let sz = if bits32 { 4 } else { 2 };
        let a = self.cpu.seg[SS].base.wrapping_add(if self.cpu.seg[SS].big() { self.cpu.regs[ESP] } else { self.cpu.regs[ESP] & 0xffff });
        let rd = |m: &mut Machine, i: u32| if sz == 4 { m.hw.rd32(a + i * 4) } else { m.hw.rd16(a + i * 2) as u32 };
        let (_code, eip, cs, fl, esp, ss) = (rd(self, 0), rd(self, 1), rd(self, 2), rd(self, 3), rd(self, 4), rd(self, 5));
        let area = self.client().area;
        let c = self.client();
        c.locked_top = (a + 6 * sz - area).min(H_LOCKED_TOP);
        let _ = self.cpu.load_seg(&mut self.hw, SS, ss as u16);
        self.cpu.regs[ESP] = esp;
        let _ = self.cpu.jump_far(&mut self.hw, cs as u16, eip);
        let keep = self.cpu.flags & 0x3000;
        self.cpu.flags = (fl & !0x3000) | keep | 2;
    }

    fn dpmi_unhandled<H: Host>(&mut self, host: &mut H, v: u8) {
        if v < 8 && v != 6 {
            // Divide overflow and friends go to the real-mode handlers.
            self.dpmi_reflect(v, true);
            return;
        }
        let has_code = matches!(v, 8 | 10..=14 | 17);
        let f = self.ring0_frame(has_code);
        let code = if has_code { self.hw.rd32(f - 4) } else { 0 };
        let (eip, cs) = (self.frame_get(f, 0), self.frame_get(f, 1));
        let msg = alloc::format!("\r\nDPMI: exception {v:02X}h (error {code:04X}h) at {cs:04X}:{eip:08X}\r\n");
        host.log(msg.trim());
        self.dpmi_exit_to_real();
        for b in msg.bytes() {
            self.tty_out(b);
        }
        self.dos_exit(host, 0xff, 0);
    }

    /// Leave protected mode for good (the client ends).
    fn dpmi_exit_to_real(&mut self) {
        self.dpmi_to_real();
        let rm = self.dpmi.client.as_ref().map_or(0, |c| c.rm_stack);
        for s in [CS, DS, ES, SS] {
            self.cpu.seg[s] = if s == CS { Seg::real_code(rm) } else { Seg::real(rm) };
        }
        self.cpu.set16(ESP, 0xffe);
    }

    pub(crate) fn dpmi_process_ended(&mut self, psp: u16) {
        let Some(c) = &self.dpmi.client else { return };
        if c.psp != psp {
            return;
        }
        let c = self.dpmi.client.take().unwrap();
        if self.cpu.pm() {
            self.dpmi_exit_to_real_with(&c);
        }
        self.hw.irq_remap = None;
        self.xms.ext.blocks.retain(|b| b.owner != Owner::Dpmi(psp));
        if c.env_seg != 0 {
            self.hw.wr16(Machine::lin(psp, 0x2c), c.env_seg);
        }
        self.cpu.idtr = Dtr { base: 0, limit: 0x3ff };
        self.cpu.cpl = 0;
    }

    fn dpmi_exit_to_real_with(&mut self, c: &Client) {
        self.cpu.cr0 &= !1;
        self.cpu.cpl = 0;
        for s in 0..6 {
            self.cpu.seg[s] = if s == CS { Seg::real_code(c.rm_stack) } else { Seg::real(c.rm_stack) };
        }
    }

    // ---- Real-mode register structures ----

    fn dpmi_load_rm_regs(&mut self, a: u32) -> (u16, u16, u16, u16) {
        let r = |m: &mut Machine, o: u32| m.hw.rd32(a + o);
        self.cpu.regs[EDI] = r(self, 0);
        self.cpu.regs[ESI] = r(self, 4);
        self.cpu.regs[x86emu::EBP] = r(self, 8);
        self.cpu.regs[EBX] = r(self, 16);
        self.cpu.regs[EDX] = r(self, 20);
        self.cpu.regs[ECX] = r(self, 24);
        self.cpu.regs[EAX] = r(self, 28);
        let w = |m: &mut Machine, o: u32| m.hw.rd16(a + o);
        let fl = w(self, 32);
        self.cpu.flags = (self.cpu.flags & !0xffff) | (fl as u32 & 0x0fd5) | 2;
        self.cpu.seg[ES] = Seg::real(w(self, 34));
        self.cpu.seg[DS] = Seg::real(w(self, 36));
        self.cpu.seg[FS] = Seg::real(w(self, 38));
        self.cpu.seg[GS] = Seg::real(w(self, 40));
        (w(self, 42), w(self, 44), w(self, 46), w(self, 48))
    }

    fn dpmi_store_rm_regs(&mut self, a: u32, regs: &[u32; 8], flags: u32, segs: [u16; 6], ip: u32) {
        self.hw.wr32(a, regs[EDI]);
        self.hw.wr32(a + 4, regs[ESI]);
        self.hw.wr32(a + 8, regs[x86emu::EBP]);
        self.hw.wr32(a + 16, regs[EBX]);
        self.hw.wr32(a + 20, regs[EDX]);
        self.hw.wr32(a + 24, regs[ECX]);
        self.hw.wr32(a + 28, regs[EAX]);
        self.hw.wr16(a + 32, flags as u16);
        self.hw.wr16(a + 34, segs[ES]);
        self.hw.wr16(a + 36, segs[DS]);
        self.hw.wr16(a + 38, segs[FS]);
        self.hw.wr16(a + 40, segs[GS]);
        self.hw.wr16(a + 42, ip as u16);
        self.hw.wr16(a + 44, segs[CS]);
        self.hw.wr16(a + 46, regs[ESP] as u16);
        self.hw.wr16(a + 48, segs[SS]);
    }

    // ---- Real-mode callbacks ----

    fn dpmi_rmcb_enter(&mut self, k: usize) {
        let cb = self.client().rmcbs[k];
        if !cb.used {
            return;
        }
        // The real-mode registers into the client's structure.
        let regs_lin = self.dpmi_lin(cb.regs_sel, cb.regs_off);
        let segs: [u16; 6] = core::array::from_fn(|i| self.cpu.seg[i].sel);
        let regs = self.cpu.regs;
        let fl = self.cpu.flags;
        let ip = self.cpu.eip;
        self.dpmi_store_rm_regs(regs_lin, &regs, fl, segs, ip);
        let (rss, rsp) = (self.cpu.seg[SS].sel, self.cpu.sp());
        // Into protected mode: the client's procedure on the locked stack
        // with DS:(E)SI = the real-mode stack, ES:(E)DI = the structure.
        let area = self.client().area;
        let bits32 = self.client().bits32;
        self.cpu.cr0 |= 1;
        self.cpu.cpl = 3;
        self.cpu.idtr = Dtr { base: area + H_IDT, limit: 256 * 8 - 1 };
        self.hw.irq_remap = Some(HW_BASE);
        // A selector for the real-mode stack (kept in slot 15).
        let s = (15u16 << 3) | 7;
        self.client().ldt_used[15] = true;
        let d = self.client().desc_addr(s);
        self.dpmi_ww64(d, desc((rss as u32) << 4, 0xffff, 0xf3, 0));
        let top = self.client().locked_top;
        let sz = if bits32 { 4 } else { 2 };
        let frame = [H_RMCBRET, if bits32 { SEL_CODE3 as u32 } else { SEL_CODE3_16 as u32 }, fl & !(IF | TF)];
        let base = top - sz * 3;
        for (i, v) in frame.iter().enumerate() {
            let a = area + base + i as u32 * sz;
            if sz == 4 {
                self.hw.wr32(a, *v);
            } else {
                self.hw.wr16(a, *v as u16);
            }
        }
        self.client().locked_top = base;
        if bits32 {
            let _ = self.cpu.load_seg(&mut self.hw, SS, SEL_FLAT3);
            self.cpu.regs[ESP] = area + base;
        } else {
            let _ = self.cpu.load_seg(&mut self.hw, SS, SEL_STACK3_16);
            self.cpu.regs[ESP] = base - H_LOCKED;
        }
        let _ = self.cpu.load_seg(&mut self.hw, DS, s);
        self.cpu.regs[ESI] = rsp as u32;
        let _ = self.cpu.load_seg(&mut self.hw, ES, cb.regs_sel);
        self.cpu.regs[EDI] = cb.regs_off;
        let _ = self.cpu.load_seg(&mut self.hw, FS, 0);
        let _ = self.cpu.load_seg(&mut self.hw, GS, 0);
        let _ = self.cpu.jump_far(&mut self.hw, cb.proc_sel, cb.proc_off);
        self.cpu.flags &= !(IF | TF);
    }

    /// The callback procedure returned (IRET): back to real mode with the
    /// registers of the structure at ES:(E)DI.
    fn dpmi_rmcb_return(&mut self) {
        let bits32 = self.client().bits32;
        let a = self.cpu.seg[ES].base.wrapping_add(if bits32 { self.cpu.regs[EDI] } else { self.cpu.regs[EDI] & 0xffff });
        let area = self.client().area;
        let sp = self.cpu.seg[SS].base.wrapping_add(if self.cpu.seg[SS].big() { self.cpu.regs[ESP] } else { self.cpu.regs[ESP] & 0xffff });
        self.client().locked_top = (sp - area).min(H_LOCKED_TOP);
        self.dpmi_to_real();
        let (ip, cs, sp, ss) = self.dpmi_load_rm_regs(a);
        self.cpu.seg[SS] = Seg::real(ss);
        self.cpu.set16(ESP, sp);
        self.cpu.seg[CS] = Seg::real_code(cs);
        self.cpu.eip = ip as u32;
    }

    // ---- Raw mode switches ----

    /// Real to protected: AX=DS, CX=ES, DX=SS, (E)BX=(E)SP, SI=CS, (E)DI=(E)IP.
    fn dpmi_raw_to_pm(&mut self) {
        let area = self.client().area;
        let (ds, es, ss, esp, cs, eip) = (self.cpu.ax(), self.cpu.cx(), self.cpu.dx(), self.cpu.regs[EBX], self.cpu.si(), self.cpu.regs[EDI]);
        self.cpu.cr0 |= 1;
        self.cpu.cpl = 3;
        self.cpu.idtr = Dtr { base: area + H_IDT, limit: 256 * 8 - 1 };
        self.hw.irq_remap = Some(HW_BASE);
        let _ = self.cpu.load_seg(&mut self.hw, DS, ds);
        let _ = self.cpu.load_seg(&mut self.hw, ES, es);
        let _ = self.cpu.load_seg(&mut self.hw, SS, ss);
        self.cpu.regs[ESP] = esp;
        let _ = self.cpu.load_seg(&mut self.hw, FS, 0);
        let _ = self.cpu.load_seg(&mut self.hw, GS, 0);
        let _ = self.cpu.jump_far(&mut self.hw, cs, eip);
    }

    fn dpmi_raw_to_rm(&mut self) {
        let (ds, es, ss, esp, cs, eip) = (self.cpu.ax(), self.cpu.cx(), self.cpu.dx(), self.cpu.regs[EBX], self.cpu.si(), self.cpu.regs[EDI]);
        self.dpmi_to_real();
        self.cpu.seg[DS] = Seg::real(ds);
        self.cpu.seg[ES] = Seg::real(es);
        self.cpu.seg[SS] = Seg::real(ss);
        self.cpu.set16(ESP, esp as u16);
        self.cpu.seg[CS] = Seg::real_code(cs);
        self.cpu.eip = eip & 0xffff;
    }

    // ---- INT 31h ----

    fn int31<H: Host>(&mut self, host: &mut H, f: u32) {
        let ax = self.cpu.ax();
        let r = self.int31_fn(host, f, ax);
        if !matches!(ax, 0x0300..=0x0302) || r.is_err() {
            self.dpmi_ret(f, r);
        }
    }

    /// ES:(E)DI as a linear address.
    fn es_edi(&mut self) -> u32 {
        let bits32 = self.client().bits32;
        self.cpu.seg[ES].base.wrapping_add(if bits32 { self.cpu.regs[EDI] } else { self.cpu.regs[EDI] & 0xffff })
    }

    fn int31_fn<H: Host>(&mut self, host: &mut H, f: u32, ax: u16) -> Result<(), u16> {
        let bits32 = self.client().bits32;
        match ax {
            0x0000 => {
                let n = (self.cpu.cx() as usize).max(1);
                let s = self.client().alloc_sel(n).ok_or(0x8011u16)?;
                for i in 0..n as u16 {
                    let sel = s + i * 8;
                    let a = self.client().desc_addr(sel);
                    self.dpmi_ww64(a, desc(0, 0, 0xf3, if bits32 { 0x4 } else { 0 }));
                }
                self.cpu.set_ax(s);
            }
            0x0001 => {
                let s = self.cpu.bx();
                if !self.client().sel_ok(s) {
                    return Err(0x8022);
                }
                self.client().ldt_used[(s >> 3) as usize] = false;
                let a = self.client().desc_addr(s);
                self.dpmi_ww64(a, 0);
                for r in [DS, ES, FS, GS] {
                    if self.cpu.seg[r].sel & !3 == s & !3 {
                        let _ = self.cpu.load_seg(&mut self.hw, r, 0);
                    }
                }
            }
            0x0002 => {
                let seg = self.cpu.bx();
                let s = self.client().alloc_sel(1).ok_or(0x8011u16)?;
                let a = self.client().desc_addr(s);
                self.dpmi_ww64(a, desc((seg as u32) << 4, 0xffff, 0xf3, 0));
                self.cpu.set_ax(s);
            }
            0x0003 => self.cpu.set_ax(8),
            0x0004 | 0x0005 => {}
            0x0006 => {
                let s = self.cpu.bx();
                if !self.client().sel_ok(s) {
                    return Err(0x8022);
                }
                let b = self.dpmi_lin(s, 0);
                self.cpu.set_cx((b >> 16) as u16);
                self.cpu.set_dx(b as u16);
            }
            0x0007 => {
                let s = self.cpu.bx();
                if !self.client().sel_ok(s) {
                    return Err(0x8022);
                }
                let base = (self.cpu.cx() as u32) << 16 | self.cpu.dx() as u32;
                let a = self.client().desc_addr(s);
                let mut d = self.dpmi_rw64(a);
                d = (d & !0xff00_00ff_ffff_0000) | (base as u64 & 0xff_ffff) << 16 | ((base as u64 >> 24) & 0xff) << 56;
                self.dpmi_ww64(a, d);
                self.dpmi_reload(s);
            }
            0x0008 => {
                let s = self.cpu.bx();
                if !self.client().sel_ok(s) {
                    return Err(0x8022);
                }
                let limit = (self.cpu.cx() as u32) << 16 | self.cpu.dx() as u32;
                if limit > 0xfffff && limit & 0xfff != 0xfff {
                    return Err(0x8021);
                }
                let a = self.client().desc_addr(s);
                let mut d = self.dpmi_rw64(a);
                let (lim, g) = if limit > 0xfffff { (limit >> 12, true) } else { (limit, false) };
                d = (d & !0x008f_0000_0000_ffff) | (lim as u64 & 0xffff) | ((lim as u64 >> 16) & 0xf) << 48 | if g { 1u64 << 55 } else { 0 };
                self.dpmi_ww64(a, d);
                self.dpmi_reload(s);
            }
            0x0009 => {
                let s = self.cpu.bx();
                if !self.client().sel_ok(s) {
                    return Err(0x8022);
                }
                let (acc, ext) = (self.cpu.cl(), self.cpu.ch());
                if acc & 0x10 == 0 || (acc >> 5) & 3 != 3 {
                    return Err(0x8021);
                }
                let a = self.client().desc_addr(s);
                let mut d = self.dpmi_rw64(a);
                d = (d & !(0xffu64 << 40) & !(0xd0u64 << 48)) | (acc as u64) << 40 | ((ext & 0xd0) as u64) << 48;
                self.dpmi_ww64(a, d);
                self.dpmi_reload(s);
            }
            0x000a => {
                let s = self.cpu.bx();
                if !self.client().sel_ok(s) {
                    return Err(0x8022);
                }
                let a = self.client().desc_addr(s);
                let d = self.dpmi_rw64(a);
                let n = self.client().alloc_sel(1).ok_or(0x8011u16)?;
                let na = self.client().desc_addr(n);
                // A data alias: type 2 (read/write data).
                self.dpmi_ww64(na, (d & !(0xfu64 << 40)) | (0x2u64 << 40));
                self.cpu.set_ax(n);
            }
            0x000b | 0x000c => {
                let s = self.cpu.bx();
                if !self.client().sel_ok(s) {
                    return Err(0x8022);
                }
                let a = self.client().desc_addr(s);
                let p = self.es_edi();
                if ax == 0x000b {
                    let d = self.dpmi_rw64(a);
                    self.dpmi_ww64(p, d);
                } else {
                    let d = self.dpmi_rw64(p);
                    if (d >> 44) & 1 == 0 || (d >> 45) & 3 != 3 {
                        return Err(0x8021);
                    }
                    self.dpmi_ww64(a, d);
                    self.dpmi_reload(s);
                }
            }
            0x000d => {
                let s = self.cpu.bx();
                let i = (s >> 3) as usize;
                if !(1..16).contains(&i) || self.client().ldt_used[i] {
                    return Err(0x8011);
                }
                self.client().ldt_used[i] = true;
                let a = self.client().desc_addr(s | 7);
                self.dpmi_ww64(a, desc(0, 0, 0xf3, 0));
            }
            0x0100 => {
                let paras = self.cpu.bx();
                let psp = self.client().psp;
                match self.alloc_strategy(paras, psp, self.dos.strategy) {
                    Ok(seg) => {
                        let bytes = paras as u32 * 16;
                        let n = bytes.div_ceil(0x10000).max(1) as u16;
                        let s = self.client().alloc_sel(n as usize).ok_or(0x8011u16)?;
                        for k in 0..n {
                            let left = bytes.saturating_sub(k as u32 * 0x10000);
                            let a = self.client().desc_addr(s + k * 8);
                            self.dpmi_ww64(a, desc(((seg as u32) << 4) + k as u32 * 0x10000, left.min(0x10000).max(1) - 1, 0xf3, 0));
                        }
                        self.client().dos_blocks.push((s, seg, n));
                        self.cpu.set_ax(seg);
                        self.cpu.set_dx(s);
                    }
                    Err(largest) => {
                        self.cpu.set_bx(largest);
                        return Err(8);
                    }
                }
            }
            0x0101 | 0x0102 => {
                let s = self.cpu.dx();
                let Some(i) = self.client().dos_blocks.iter().position(|b| b.0 == s) else { return Err(0x8022) };
                let (sel, seg, n) = self.client().dos_blocks[i];
                if ax == 0x0101 {
                    self.free_mem(seg).map_err(|_| 9u16)?;
                    for k in 0..n {
                        let x = sel + k * 8;
                        self.client().ldt_used[(x >> 3) as usize] = false;
                    }
                    self.client().dos_blocks.remove(i);
                } else {
                    let paras = self.cpu.bx();
                    if let Err((e, max)) = self.resize_mem(seg, paras) {
                        self.cpu.set_bx(max);
                        return Err(e);
                    }
                    let bytes = paras as u32 * 16;
                    for k in 0..n {
                        let left = bytes.saturating_sub(k as u32 * 0x10000);
                        let a = self.client().desc_addr(sel + k * 8);
                        self.dpmi_ww64(a, desc(((seg as u32) << 4) + k as u32 * 0x10000, left.min(0x10000).max(1) - 1, 0xf3, 0));
                    }
                }
            }
            0x0200 => {
                let (s, o) = self.vector(self.cpu.bl());
                self.cpu.set_cx(s);
                self.cpu.set_dx(o);
            }
            0x0201 => {
                let (v, s, o) = (self.cpu.bl(), self.cpu.cx(), self.cpu.dx());
                self.set_vector(v, s, o);
            }
            0x0202 | 0x0204 => {
                let v = self.cpu.bl() as usize;
                let c = self.client();
                let h = if ax == 0x0202 {
                    if v >= 32 {
                        return Err(0x8021);
                    }
                    c.exc_handlers[v].unwrap_or((if bits32 { SEL_CODE3 } else { SEL_CODE3_16 }, H_DEFEXC + v as u32 * 8))
                } else {
                    c.pm_vectors[v].unwrap_or((if bits32 { SEL_CODE3 } else { SEL_CODE3_16 }, H_DEFINT + v as u32 * 8))
                };
                self.cpu.set_cx(h.0);
                if bits32 {
                    self.cpu.regs[EDX] = h.1;
                } else {
                    self.cpu.set_dx(h.1 as u16);
                }
            }
            0x0203 | 0x0205 => {
                let v = self.cpu.bl() as usize;
                let sel = self.cpu.cx();
                let off = if bits32 { self.cpu.regs[EDX] } else { self.cpu.dx() as u32 };
                let host_code = sel == SEL_CODE3 || sel == SEL_CODE3_16;
                let c = self.client();
                if ax == 0x0203 {
                    if v >= 32 {
                        return Err(0x8021);
                    }
                    c.exc_handlers[v] = if host_code { None } else { Some((sel, off)) };
                } else {
                    c.pm_vectors[v] = if host_code { None } else { Some((sel, off)) };
                }
            }
            0x0300..=0x0302 => return self.dpmi_simulate(ax),
            0x0303 => {
                let k = self.client().rmcbs.iter().position(|c| !c.used).ok_or(0x8015u16)?;
                let (ps, po) = (self.cpu.seg[DS].sel, if bits32 { self.cpu.regs[ESI] } else { self.cpu.si() as u32 });
                let (rs, ro) = (self.cpu.seg[ES].sel, if bits32 { self.cpu.regs[EDI] } else { self.cpu.di() as u32 });
                self.client().rmcbs[k] = Rmcb { used: true, proc_sel: ps, proc_off: po, regs_sel: rs, regs_off: ro };
                self.cpu.set_cx(bios::ROM);
                self.cpu.set_dx(RM_RMCB + k as u16 * 8);
            }
            0x0304 => {
                let (s, o) = (self.cpu.cx(), self.cpu.dx());
                let k = o.wrapping_sub(RM_RMCB) / 8;
                if s != bios::ROM || k as usize >= RMCBS {
                    return Err(0x8024);
                }
                self.client().rmcbs[k as usize].used = false;
            }
            0x0305 => {
                self.cpu.set_ax(0);
                self.cpu.set_bx(bios::ROM);
                self.cpu.set_cx(RM_SAVE);
                self.cpu.set_si(if bits32 { SEL_CODE3 } else { SEL_CODE3_16 });
                self.cpu.regs[EDI] = H_SAVE;
            }
            0x0306 => {
                self.cpu.set_bx(bios::ROM);
                self.cpu.set_cx(RM_RAW);
                self.cpu.set_si(if bits32 { SEL_CODE3 } else { SEL_CODE3_16 });
                self.cpu.regs[EDI] = H_RAW;
            }
            0x0400 => {
                self.cpu.set_ax(0x005a);
                self.cpu.set_bx(0x0001 | 0x0002);
                self.cpu.set_cl(4);
                self.cpu.set_dx(0x0870);
            }
            0x0500 => {
                let used = self.xms.ext.used(|o| matches!(o, Owner::Dpmi(_)));
                let quota = self.cfg.dpmi_kb * 1024 + AREA_SIZE;
                let free = quota.saturating_sub(used);
                let largest = free.min(self.xms.ext.largest_gap(self.ext_limit()));
                let p = self.es_edi();
                for i in 0..12 {
                    self.hw.wr32(p + i * 4, 0xffff_ffff);
                }
                self.hw.wr32(p, largest);
                self.hw.wr32(p + 4, largest / 4096);
                self.hw.wr32(p + 8, largest / 4096);
                self.hw.wr32(p + 0x14, free / 4096);
                self.hw.wr32(p + 0x18, (self.cfg.dpmi_kb / 4).max(1));
            }
            0x0501 => {
                let size = (self.cpu.bx() as u32) << 16 | self.cpu.cx() as u32;
                let start = self.dpmi_alloc_block(size)?;
                self.cpu.set_bx((start >> 16) as u16);
                self.cpu.set_cx(start as u16);
                self.cpu.set_si((start >> 16) as u16);
                self.cpu.set_di(start as u16);
            }
            0x0502 => {
                let h = (self.cpu.si() as u32) << 16 | self.cpu.di() as u32;
                let psp = self.client().psp;
                let i = self.xms.ext.blocks.iter().position(|b| b.start == h && b.owner == Owner::Dpmi(psp) && b.locks == 0).ok_or(0x8023u16)?;
                self.xms.ext.blocks.remove(i);
            }
            0x0503 => {
                let h = (self.cpu.si() as u32) << 16 | self.cpu.di() as u32;
                let size = (self.cpu.bx() as u32) << 16 | self.cpu.cx() as u32;
                let psp = self.client().psp;
                let i = self.xms.ext.blocks.iter().position(|b| b.start == h && b.owner == Owner::Dpmi(psp) && b.locks == 0).ok_or(0x8023u16)?;
                let old = self.xms.ext.blocks.remove(i);
                let new_size = size.max(1).next_multiple_of(4096);
                let start = if new_size <= old.size {
                    self.xms.ext.blocks.push(Block { size: new_size, ..old.clone() });
                    old.start
                } else {
                    match self.dpmi_alloc_block(size) {
                        Ok(s) => {
                            let mut buf = alloc::vec![0u8; old.size as usize];
                            self.hw.read_bytes(old.start, &mut buf);
                            self.hw.write_bytes(s, &buf);
                            s
                        }
                        Err(e) => {
                            self.xms.ext.blocks.push(old);
                            return Err(e);
                        }
                    }
                };
                self.cpu.set_bx((start >> 16) as u16);
                self.cpu.set_cx(start as u16);
                self.cpu.set_si((start >> 16) as u16);
                self.cpu.set_di(start as u16);
            }
            0x0600 | 0x0601 | 0x0602 | 0x0603 | 0x0702 | 0x0703 => {}
            0x0604 => {
                self.cpu.set_bx(0);
                self.cpu.set_cx(4096);
            }
            0x0800 => {
                let phys = (self.cpu.bx() as u32) << 16 | self.cpu.cx() as u32;
                let size = (self.cpu.si() as u32) << 16 | self.cpu.di() as u32;
                // Only the first megabyte (video memory) is there to map.
                if phys.saturating_add(size) > 0x110000 {
                    return Err(0x8021);
                }
            }
            0x0801 => {}
            0x0900..=0x0902 => {
                let fl = self.frame_get(f, 2);
                self.cpu.set_al((fl & IF != 0) as u8);
                let nf = match ax {
                    0x0900 => fl & !IF,
                    0x0901 => fl | IF,
                    _ => fl,
                };
                self.frame_set(f, 2, nf);
            }
            0x0a00 => return Err(0x8001),
            0x0b00..=0x0b03 => return Err(0x8016),
            _ => {
                host.log(&alloc::format!("dpmi: INT 31h AX={ax:04X} not supported"));
                return Err(0x8001);
            }
        }
        Ok(())
    }

    /// A segment register holding `s` takes its new descriptor.
    fn dpmi_reload(&mut self, s: u16) {
        for r in [DS, ES, FS, GS, SS] {
            if self.cpu.seg[r].sel & !3 == s & !3 && self.cpu.seg[r].attr & attr::UNUSABLE == 0 {
                let sel = self.cpu.seg[r].sel;
                let _ = self.cpu.load_seg(&mut self.hw, r, sel);
            }
        }
    }

    fn dpmi_alloc_block(&mut self, size: u32) -> Result<u32, u16> {
        let size = size.max(1).next_multiple_of(4096);
        let used = self.xms.ext.used(|o| matches!(o, Owner::Dpmi(_)));
        if used + size > self.cfg.dpmi_kb * 1024 + AREA_SIZE {
            return Err(0x8013);
        }
        let start = self.xms.ext.find(size, self.ext_limit()).ok_or(0x8013u16)?;
        self.hw.grow_ext(start + size);
        let psp = self.client().psp;
        self.xms.ext.blocks.push(Block { start, size, owner: Owner::Dpmi(psp), locks: 0 });
        Ok(start)
    }

    /// 0300h simulate real-mode interrupt, 0301h/0302h call a real-mode
    /// procedure (far return / IRET frame).
    fn dpmi_simulate(&mut self, ax: u16) -> Result<(), u16> {
        let bits32 = self.client().bits32;
        let v = self.cpu.bl();
        let words = self.cpu.cx();
        let regs = self.es_edi();
        // The words to copy from the protected-mode stack (above the
        // ring-0 frame: the client's stack).
        let f = self.ring0_frame(false);
        let (cesp, css) = (self.frame_get(f, 3), self.frame_get(f, 4) as u16);
        let cbase = self.dpmi_lin(css, 0);
        let cbig = self.dpmi_seg_big(css);
        let mut copy = Vec::new();
        for i in 0..words as u32 {
            let off = if cbig { cesp + i * 2 } else { (cesp + i * 2) & 0xffff };
            copy.push(self.hw.rd16(cbase.wrapping_add(off)));
        }
        let _ = bits32;
        self.dpmi_save(Reflect::Simulate { regs });
        self.dpmi_to_real();
        let (ip, cs, sp, ss) = self.dpmi_load_rm_regs(regs);
        let (ss, sp) = if ss == 0 && sp == 0 {
            let c = self.client();
            (c.rm_stack, c.rm_sp)
        } else {
            (ss, sp)
        };
        self.cpu.seg[SS] = Seg::real(ss);
        self.cpu.set16(ESP, sp);
        for w in copy.iter().rev() {
            let _ = self.cpu.push(&mut self.hw, *w as u32, 2);
        }
        let fl = self.cpu.flags;
        let (tcs, tip) = if ax == 0x0300 { self.vector(v) } else { (cs, ip) };
        if ax != 0x0301 {
            let _ = self.cpu.push(&mut self.hw, fl & 0xffff, 2);
        }
        let _ = self.cpu.push(&mut self.hw, bios::ROM as u32, 2);
        let _ = self.cpu.push(&mut self.hw, RM_SIMRET as u32, 2);
        if ax != 0x0301 {
            self.cpu.flags &= !(IF | TF);
        }
        self.cpu.seg[CS] = Seg::real_code(tcs);
        self.cpu.eip = tip as u32;
        Ok(())
    }
}
