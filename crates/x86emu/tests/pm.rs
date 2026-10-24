//! Segment limit and access-right checks on memory accesses, which the
//! QEMU oracle cannot test (its TCG does not check them): expected
//! results from Intel's SDM.

use x86emu::{attr, Bus, Cpu, Exit, Seg, CS, DS, ES, SS};

struct Mem(Vec<u8>);

impl Bus for Mem {
    fn read8(&mut self, a: u32) -> u8 {
        self.0.get(a as usize).copied().unwrap_or(0xff)
    }
    fn write8(&mut self, a: u32, v: u8) {
        if let Some(m) = self.0.get_mut(a as usize) {
            *m = v;
        }
    }
    fn io_read(&mut self, _: u16, _: u8) -> u32 {
        0
    }
    fn io_write(&mut self, _: u16, _: u8, _: u32) {}
}

const CODE: u32 = 0x1000;
const STUBS: u32 = 0x8000;
const IDT: u32 = 0x9000;
const GDT: u32 = 0xa000;

/// A protected-mode CPU at ring 0 with flat 32-bit code and data, an IDT
/// whose gates run `FE 38 vec 00` (so a fault ends `run` with its vector),
/// and `code` at CODE.
fn pm(code: &[u8]) -> (Cpu, Mem) {
    let mut m = Mem(vec![0; 0x40000]);
    m.0[CODE as usize..CODE as usize + code.len()].copy_from_slice(code);
    let gdt: [u64; 3] = [0, 0x00cf9b000000ffff, 0x00cf93000000ffff];
    for (i, d) in gdt.iter().enumerate() {
        m.0[GDT as usize + i * 8..][..8].copy_from_slice(&d.to_le_bytes());
    }
    for v in 0..32u32 {
        let stub = STUBS + v * 4;
        m.0[stub as usize..][..4].copy_from_slice(&[0xfe, 0x38, v as u8, 0]);
        let gate = (stub as u64 & 0xffff) | 0x08 << 16 | 0x8e00u64 << 32 | ((stub as u64) >> 16) << 48;
        m.0[(IDT + v * 8) as usize..][..8].copy_from_slice(&gate.to_le_bytes());
    }
    let mut c = Cpu::new();
    c.cr0 |= 1;
    c.gdtr = x86emu::Dtr { base: GDT, limit: 23 };
    c.idtr = x86emu::Dtr { base: IDT, limit: 32 * 8 - 1 };
    c.seg[CS] = Seg { sel: 0x08, base: 0, limit: 0xffff_ffff, attr: attr::CODE16 | attr::BIG | attr::G };
    for s in [DS, ES, SS, x86emu::FS, x86emu::GS] {
        c.seg[s] = Seg { sel: 0x10, base: 0, limit: 0xffff_ffff, attr: attr::DATA16 | attr::BIG | attr::G };
    }
    c.regs[x86emu::ESP] = 0x30000;
    c.eip = CODE;
    (c, m)
}

/// Run one instruction; the fault it raised (vector, error code) if any.
fn fault(c: &mut Cpu, m: &mut Mem) -> Option<(u16, u32)> {
    let esp = c.regs[x86emu::ESP];
    assert_eq!(c.run(m, 1), Exit::Budget);
    if c.eip < STUBS || c.eip >= STUBS + 128 {
        return None;
    }
    let code = if c.regs[x86emu::ESP] == esp - 16 { m.read32(c.regs[x86emu::ESP]) } else { 0 };
    match c.run(m, 1) {
        Exit::Callback(v) => Some((v, code)),
        e => panic!("{e:?}"),
    }
}

fn small_data(c: &mut Cpu, s: usize, a: u32) {
    c.seg[s] = Seg { sel: 0x10, base: 0x20000, limit: 0xfff, attr: a };
}

#[test]
fn expand_up_limit() {
    let code = [0xa1, 0xfc, 0x0f, 0, 0, 0xa1, 0xfd, 0x0f, 0, 0];
    let (mut c, mut m) = pm(&code);
    small_data(&mut c, DS, attr::DATA16);
    assert_eq!(fault(&mut c, &mut m), None);
    assert_eq!(fault(&mut c, &mut m), Some((13, 0)));
    // The faulting instruction is restarted.
    assert_eq!(m.read32(c.regs[x86emu::ESP] + 4), CODE + 5);
}

#[test]
fn expand_down_limit() {
    // mov al,[1000h]; mov al,[0FFFFh]; mov al,[800h]
    let code = [0xa0, 0, 0x10, 0, 0, 0xa0, 0xff, 0xff, 0, 0, 0xa0, 0, 8, 0, 0];
    let (mut c, mut m) = pm(&code);
    small_data(&mut c, DS, attr::DATA16 | attr::DC);
    assert_eq!(fault(&mut c, &mut m), None);
    assert_eq!(fault(&mut c, &mut m), None);
    assert_eq!(fault(&mut c, &mut m), Some((13, 0)));
}

#[test]
fn stack_limit_is_ss_fault() {
    // mov al, ss:[800h] with an expand-down SS
    let code = [0x36, 0xa0, 0, 8, 0, 0];
    let (mut c, mut m) = pm(&code);
    // The fault is delivered on another stack: give the IDT a flat SS by
    // switching only for the access... SS is used for delivery too, so
    // keep ESP valid in the expand-down segment.
    small_data(&mut c, SS, attr::DATA16 | attr::DC);
    c.regs[x86emu::ESP] = 0x2000;
    let r = fault(&mut c, &mut m);
    assert_eq!(r.map(|r| r.0), Some(12));
}

#[test]
fn code_segment_rights() {
    // mov cs:[0],al ; mov eax,cs:[0]
    let code = [0x2e, 0xa2, 0, 0, 0, 0, 0x2e, 0xa1, 0, 0, 0, 0];
    let (mut c, mut m) = pm(&code);
    assert_eq!(fault(&mut c, &mut m), Some((13, 0)));
    c.eip = CODE + 6;
    assert_eq!(fault(&mut c, &mut m), None);
    // Execute-only: reading through CS faults.
    c.seg[CS].attr &= !attr::RW;
    c.eip = CODE + 6;
    assert_eq!(fault(&mut c, &mut m), Some((13, 0)));
}

#[test]
fn read_only_and_null_data() {
    // mov [0],al ; mov al,[0]
    let code = [0xa2, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0];
    let (mut c, mut m) = pm(&code);
    c.seg[DS].attr &= !attr::RW;
    assert_eq!(fault(&mut c, &mut m), Some((13, 0)));
    c.eip = CODE + 5;
    assert_eq!(fault(&mut c, &mut m), None);
    c.seg[DS] = Seg { sel: 0, base: 0, limit: 0, attr: attr::UNUSABLE };
    c.eip = CODE + 5;
    assert_eq!(fault(&mut c, &mut m), Some((13, 0)));
}

#[test]
fn real_mode_wrap_faults() {
    // 16-bit: mov ax,[0FFFFh] faults (#GP, int 13 via the IVT), mov al,
    // [0FFFFh] does not.
    let mut m = Mem(vec![0; 0x20000]);
    m.0[0x1000..0x1006].copy_from_slice(&[0xa1, 0xff, 0xff, 0xa0, 0xff, 0xff]);
    m.0[13 * 4..13 * 4 + 4].copy_from_slice(&[0x00, 0x30, 0x00, 0x00]);
    m.0[0x3000..0x3004].copy_from_slice(&[0xfe, 0x38, 13, 0]);
    let mut c = Cpu::new();
    c.seg[CS] = Seg::real_code(0);
    for s in [DS, ES, SS] {
        c.seg[s] = Seg::real(0);
    }
    c.regs[x86emu::ESP] = 0x8000;
    c.eip = 0x1003;
    assert_eq!(c.run(&mut m, 1), Exit::Budget);
    assert_eq!(c.eip, 0x1006);
    c.eip = 0x1000;
    assert_eq!(c.run(&mut m, 2), Exit::Callback(13));
}

#[test]
fn rep_movs_and_scas() {
    // 16-bit: rep movsb ; repne scasb
    let mut m = Mem(vec![0; 0x20000]);
    m.0[0x1000..0x1004].copy_from_slice(&[0xf3, 0xa4, 0xf2, 0xae]);
    m.0[0x2000..0x2006].copy_from_slice(b"hello\0");
    let mut c = Cpu::new();
    c.seg[CS] = Seg::real_code(0);
    for s in [DS, ES, SS] {
        c.seg[s] = Seg::real(0);
    }
    c.eip = 0x1000;
    c.set_si(0x2000);
    c.set_di(0x3000);
    c.set_cx(6);
    c.run(&mut m, 1);
    assert_eq!(&m.0[0x3000..0x3006], b"hello\0");
    assert_eq!((c.si(), c.di(), c.cx()), (0x2006, 0x3006, 0));
    c.set_di(0x3000);
    c.set_cx(100);
    c.set_al(0);
    c.run(&mut m, 1);
    assert_eq!((c.di(), c.cx()), (0x3006, 94));
    assert!(c.flag(x86emu::flags::ZF));
}

#[test]
fn fpu_basics() {
    // fld1; fldpi; fmulp; fistp word [2000h]; fnstsw ax
    let code = [0xd9, 0xe8, 0xd9, 0xeb, 0xde, 0xc9, 0xdf, 0x1e, 0x00, 0x20, 0xdf, 0xe0];
    let mut m = Mem(vec![0; 0x20000]);
    m.0[0x1000..0x1000 + code.len()].copy_from_slice(&code);
    let mut c = Cpu::new();
    c.fpu = Some(Default::default());
    c.seg[CS] = Seg::real_code(0);
    for s in [DS, ES, SS] {
        c.seg[s] = Seg::real(0);
    }
    c.eip = 0x1000;
    c.run(&mut m, 5);
    assert_eq!(m.read16(0x2000), 3);
    assert_eq!(c.ax() & 0x3800, 0);
    // Without a coprocessor FNSTSW leaves its target alone.
    let mut c = Cpu::new();
    c.seg[CS] = Seg::real_code(0);
    for s in [DS, ES, SS] {
        c.seg[s] = Seg::real(0);
    }
    c.eip = 0x100a;
    c.set_ax(0x5a5a);
    c.run(&mut m, 1);
    assert_eq!(c.ax(), 0x5a5a);
}
