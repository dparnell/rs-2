//! CPU tests against known results: payloads in tests/asm whose output
//! (results and flags written to port E9h) was recorded on QEMU
//! (`tools/scripts/cputest.sh --oracle`) and must come out the same here.

use x86emu::{Bus, Cpu, Exit, Seg, CS, DS, ES, SS};

struct TestBus {
    mem: Vec<u8>,
    out: Vec<u8>,
    done: bool,
}

impl Bus for TestBus {
    fn read8(&mut self, a: u32) -> u8 {
        self.mem.get(a as usize).copied().unwrap_or(0xff)
    }
    fn write8(&mut self, a: u32, v: u8) {
        if let Some(m) = self.mem.get_mut(a as usize) {
            *m = v;
        }
    }
    fn io_read(&mut self, _port: u16, _size: u8) -> u32 {
        0xffff_ffff
    }
    fn io_write(&mut self, port: u16, size: u8, v: u32) {
        match port {
            0xe9 => self.out.extend_from_slice(&v.to_le_bytes()[..size as usize]),
            0xf4 => self.done = true,
            _ => {}
        }
    }
}

/// Run a payload at 1000:0000 as the boot sector would start it.
fn run(bin: &[u8]) -> Vec<u8> {
    let mut b = TestBus { mem: vec![0; 0x110000], out: Vec::new(), done: false };
    b.mem[0x10000..0x10000 + bin.len()].copy_from_slice(bin);
    let mut cpu = Cpu::new();
    for s in [DS, ES, SS] {
        cpu.seg[s] = Seg::real(0);
    }
    cpu.seg[CS] = Seg::real_code(0x1000);
    cpu.eip = 0;
    cpu.regs[x86emu::ESP] = 0x7c00;
    for _ in 0..10_000 {
        let e = cpu.run(&mut b, 1_000_000);
        if b.done {
            return b.out;
        }
        match e {
            Exit::Budget => {}
            e => panic!("payload stopped: {e:?} at {:04x}:{:04x}", cpu.seg[CS].sel, cpu.eip),
        }
    }
    panic!("payload did not finish");
}

#[test]
fn alu() {
    let got = run(include_bytes!("asm/alu.bin"));
    let want = include_bytes!("asm/alu.expected");
    if got == want {
        return;
    }
    // Name the first differing cases.
    let layout = include_str!("asm/alu.layout");
    let mut lines = layout.lines();
    let vals: Vec<&str> = lines.next().unwrap().split(' ').collect();
    let mut at = 0;
    let mut errors = Vec::new();
    for l in lines {
        let (name, opt) = l.split_once(' ').unwrap();
        let opt: u32 = opt.parse().unwrap();
        if opt == 8 {
            for al in 0..256 {
                for fl in ["0002", "0003", "0012", "0013"] {
                    if got.get(at..at + 4) != want.get(at..at + 4) && errors.len() < 30 {
                        errors.push(format!("{name} al={al:02x} flags={fl}: got {:02x?} want {:02x?}", got.get(at..at + 4), &want[at..at + 4]));
                    }
                    at += 4;
                }
            }
            continue;
        }
        let n = if opt & 2 != 0 { 10 } else { 6 };
        for a in &vals {
            for b in &vals {
                for fl in if opt & 1 != 0 { &["0002", "08d7"][..] } else { &["0002"][..] } {
                    if got.get(at..at + n) != want.get(at..at + n) && errors.len() < 30 {
                        errors.push(format!("{name} a={a} b={b} flags={fl}: got {:02x?} want {:02x?}", got.get(at..at + n), &want[at..at + n]));
                    }
                    at += n;
                }
            }
        }
    }
    panic!("{} bytes vs {} expected; first differences:\n{}", got.len(), want.len(), errors.join("\n"));
}
