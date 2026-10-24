//! Instruction decoding and execution.

use crate::alu::{mask, sext};
use crate::flags::*;
use crate::*;

/// Prefix state of the instruction being decoded.
#[derive(Clone, Copy)]
pub(crate) struct D {
    pub op32: bool,
    pub ad32: bool,
    pub seg: Option<usize>,
    /// 0, 0xF2 (REPNE) or 0xF3 (REP/REPE).
    pub rep: u8,
}

impl D {
    #[inline]
    fn osz(&self) -> u8 {
        if self.op32 { 4 } else { 2 }
    }
}

/// A ModRM operand.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Rm {
    Reg(u8),
    Mem(usize, u32),
}

impl Cpu {
    // ---- Fetching ----

    #[inline(always)]
    fn f8<B: Bus>(&mut self, b: &mut B) -> R<u8> {
        let cs = &self.seg[CS];
        if self.eip > cs.limit {
            return Err(gp(0));
        }
        let v = b.read8(cs.base.wrapping_add(self.eip));
        self.eip = self.eip.wrapping_add(1);
        Ok(v)
    }

    #[inline(always)]
    fn f16<B: Bus>(&mut self, b: &mut B) -> R<u16> {
        let cs = &self.seg[CS];
        if self.eip.wrapping_add(1) > cs.limit || self.eip == u32::MAX {
            return Err(gp(0));
        }
        let v = b.read16(cs.base.wrapping_add(self.eip));
        self.eip = self.eip.wrapping_add(2);
        Ok(v)
    }

    #[inline(always)]
    fn f32<B: Bus>(&mut self, b: &mut B) -> R<u32> {
        let cs = &self.seg[CS];
        if self.eip.wrapping_add(3) > cs.limit || self.eip > u32::MAX - 3 {
            return Err(gp(0));
        }
        let v = b.read32(cs.base.wrapping_add(self.eip));
        self.eip = self.eip.wrapping_add(4);
        Ok(v)
    }

    /// An immediate of size `sz`.
    #[inline(always)]
    fn imm<B: Bus>(&mut self, b: &mut B, sz: u8) -> R<u32> {
        match sz {
            1 => self.f8(b).map(|v| v as u32),
            2 => self.f16(b).map(|v| v as u32),
            _ => self.f32(b),
        }
    }

    fn modrm<B: Bus>(&mut self, b: &mut B, d: &D) -> R<(u8, Rm)> {
        let m = self.f8(b)?;
        let md = m >> 6;
        let reg = (m >> 3) & 7;
        let rm = m & 7;
        if md == 3 {
            return Ok((reg, Rm::Reg(rm)));
        }
        let (off, def) = if !d.ad32 {
            let r = |i: usize| self.regs[i];
            let (base, def) = match rm {
                0 => (r(EBX).wrapping_add(r(ESI)), DS),
                1 => (r(EBX).wrapping_add(r(EDI)), DS),
                2 => (r(EBP).wrapping_add(r(ESI)), SS),
                3 => (r(EBP).wrapping_add(r(EDI)), SS),
                4 => (r(ESI), DS),
                5 => (r(EDI), DS),
                6 => {
                    if md == 0 {
                        (self.f16(b)? as u32, DS)
                    } else {
                        (r(EBP), SS)
                    }
                }
                _ => (r(EBX), DS),
            };
            let disp = match md {
                1 => self.f8(b)? as i8 as i32 as u32,
                2 => self.f16(b)? as u32,
                _ => 0,
            };
            (base.wrapping_add(disp) & 0xffff, def)
        } else {
            let mut def = DS;
            let base = if rm == 4 {
                let sib = self.f8(b)?;
                let scale = sib >> 6;
                let idx = ((sib >> 3) & 7) as usize;
                let bs = (sib & 7) as usize;
                let base = if bs == 5 && md == 0 {
                    self.f32(b)?
                } else {
                    if bs == ESP || bs == EBP {
                        def = SS;
                    }
                    self.regs[bs]
                };
                let index = if idx == 4 { 0 } else { self.regs[idx] << scale };
                base.wrapping_add(index)
            } else if rm == 5 && md == 0 {
                self.f32(b)?
            } else {
                if rm as usize == EBP {
                    def = SS;
                }
                self.regs[rm as usize]
            };
            let disp = match md {
                1 => self.f8(b)? as i8 as i32 as u32,
                2 => self.f32(b)?,
                _ => 0,
            };
            (base.wrapping_add(disp), def)
        };
        Ok((reg, Rm::Mem(d.seg.unwrap_or(def), off)))
    }

    // ---- Operands ----

    #[inline(always)]
    pub(crate) fn getr(&self, r: u8, sz: u8) -> u32 {
        match sz {
            1 => {
                if r < 4 {
                    self.regs[r as usize] & 0xff
                } else {
                    (self.regs[r as usize - 4] >> 8) & 0xff
                }
            }
            2 => self.regs[r as usize] & 0xffff,
            _ => self.regs[r as usize],
        }
    }

    #[inline(always)]
    pub(crate) fn setr(&mut self, r: u8, sz: u8, v: u32) {
        match sz {
            1 => {
                if r < 4 {
                    let x = &mut self.regs[r as usize];
                    *x = (*x & !0xff) | (v & 0xff);
                } else {
                    let x = &mut self.regs[r as usize - 4];
                    *x = (*x & !0xff00) | (v & 0xff) << 8;
                }
            }
            2 => {
                let x = &mut self.regs[r as usize];
                *x = (*x & !0xffff) | (v & 0xffff);
            }
            _ => self.regs[r as usize] = v,
        }
    }

    #[inline(always)]
    fn get<B: Bus>(&mut self, b: &mut B, rm: Rm, sz: u8) -> R<u32> {
        match rm {
            Rm::Reg(r) => Ok(self.getr(r, sz)),
            Rm::Mem(s, o) => self.rd_sz(b, s, o, sz),
        }
    }

    #[inline(always)]
    fn set<B: Bus>(&mut self, b: &mut B, rm: Rm, sz: u8, v: u32) -> R<()> {
        match rm {
            Rm::Reg(r) => {
                self.setr(r, sz, v);
                Ok(())
            }
            Rm::Mem(s, o) => self.wr_sz(b, s, o, sz, v),
        }
    }

    /// Check a read-modify-write memory operand is writable before the
    /// read (so a fault leaves nothing half done).
    #[inline(always)]
    fn check_w(&self, rm: Rm, sz: u8) -> R<()> {
        if let Rm::Mem(s, o) = rm {
            self.addr(s, o, sz as u32, true)?;
        }
        Ok(())
    }

    fn mem(rm: Rm) -> R<(usize, u32)> {
        match rm {
            Rm::Mem(s, o) => Ok((s, o)),
            Rm::Reg(_) => Err(UD),
        }
    }

    pub(crate) fn cond(&self, c: u8) -> bool {
        let f = self.flags;
        let r = match c >> 1 {
            0 => f & OF != 0,
            1 => f & CF != 0,
            2 => f & ZF != 0,
            3 => f & (CF | ZF) != 0,
            4 => f & SF != 0,
            5 => f & PF != 0,
            6 => (f & SF != 0) != (f & OF != 0),
            _ => f & ZF != 0 || ((f & SF != 0) != (f & OF != 0)),
        };
        r != (c & 1 != 0)
    }

    /// Near jump to `target` (masked to the operand size).
    fn jmp_near(&mut self, target: u32, sz: u8) -> R<()> {
        let t = target & mask(sz);
        if t > self.seg[CS].limit {
            return Err(gp(0));
        }
        self.eip = t;
        Ok(())
    }

    fn priv0(&self) -> R<()> {
        if self.pm() && self.cpl != 0 {
            return Err(gp(0));
        }
        Ok(())
    }

    fn iopl_ok(&self) -> bool {
        !self.pm() || self.cpl as u32 <= (self.flags & IOPL) >> 12
    }

    fn io_check(&self) -> R<()> {
        if self.iopl_ok() { Ok(()) } else { Err(gp(0)) }
    }

    // ---- The instruction ----

    pub(crate) fn step<B: Bus>(&mut self, b: &mut B) -> R<()> {
        let big = self.seg[CS].big();
        let mut d = D { op32: big, ad32: big, seg: None, rep: 0 };
        let mut n = 0;
        let op = loop {
            let op = self.f8(b)?;
            match op {
                0x26 => d.seg = Some(ES),
                0x2e => d.seg = Some(CS),
                0x36 => d.seg = Some(SS),
                0x3e => d.seg = Some(DS),
                0x64 => d.seg = Some(FS),
                0x65 => d.seg = Some(GS),
                0x66 => d.op32 = !big,
                0x67 => d.ad32 = !big,
                0xf0 => {}
                0xf2 | 0xf3 => d.rep = op,
                _ => break op,
            }
            n += 1;
            if n >= 15 {
                return Err(gp(0));
            }
        };
        self.exec(b, op, &d)
    }

    fn exec<B: Bus>(&mut self, b: &mut B, op: u8, d: &D) -> R<()> {
        let osz = d.osz();
        match op {
            // ALU forms.
            0x00..=0x3f if op & 7 < 6 => {
                let aop = op >> 3;
                match op & 7 {
                    0 | 1 => {
                        let sz = if op & 1 == 0 { 1 } else { osz };
                        let (reg, rm) = self.modrm(b, d)?;
                        if aop != 7 {
                            self.check_w(rm, sz)?;
                        }
                        let a = self.get(b, rm, sz)?;
                        let v = self.getr(reg, sz);
                        let r = self.alu(aop, a, v, sz);
                        if aop != 7 {
                            self.set(b, rm, sz, r)?;
                        }
                    }
                    2 | 3 => {
                        let sz = if op & 1 == 0 { 1 } else { osz };
                        let (reg, rm) = self.modrm(b, d)?;
                        let v = self.get(b, rm, sz)?;
                        let a = self.getr(reg, sz);
                        let r = self.alu(aop, a, v, sz);
                        if aop != 7 {
                            self.setr(reg, sz, r);
                        }
                    }
                    4 => {
                        let v = self.f8(b)? as u32;
                        let a = self.getr(0, 1);
                        let r = self.alu(aop, a, v, 1);
                        self.setr(0, 1, r);
                    }
                    _ => {
                        let v = self.imm(b, osz)?;
                        let a = self.getr(0, osz);
                        let r = self.alu(aop, a, v, osz);
                        self.setr(0, osz, r);
                    }
                }
            }
            0x06 | 0x0e | 0x16 | 0x1e => {
                let v = self.seg[(op >> 3) as usize].sel as u32;
                self.push_sz(b, v, osz)?;
            }
            0x07 | 0x17 | 0x1f => {
                let s = (op >> 3) as usize;
                let v = self.peek_sz(b, 0, osz)? as u16;
                self.load_sreg(b, s, v)?;
                self.add_sp(osz as u32);
                if s == SS {
                    self.inhibit = true;
                }
            }
            0x0f => return self.exec0f(b, d),
            0x27 => {
                // DAA
                let al = self.al();
                let cf = self.flags & CF != 0;
                let mut r = al;
                let mut ncf = false;
                if al & 0xf > 9 || self.flags & AF != 0 {
                    r = al.wrapping_add(6);
                    ncf = cf || al > 0xf9;
                    self.flags |= AF;
                } else {
                    self.flags &= !AF;
                }
                if al > 0x99 || cf {
                    r = r.wrapping_add(0x60);
                    ncf = true;
                }
                self.set_al(r);
                self.set_szp(r as u32, 1);
                self.set_flag(CF, ncf);
                self.flags &= !OF;
            }
            0x2f => {
                // DAS
                let al = self.al();
                let cf = self.flags & CF != 0;
                let mut r = al;
                let mut ncf = false;
                if al & 0xf > 9 || self.flags & AF != 0 {
                    r = al.wrapping_sub(6);
                    ncf = cf || al < 6;
                    self.flags |= AF;
                } else {
                    self.flags &= !AF;
                }
                if al > 0x99 || cf {
                    r = r.wrapping_sub(0x60);
                    ncf = true;
                }
                self.set_al(r);
                self.set_szp(r as u32, 1);
                self.set_flag(CF, ncf);
                self.flags &= !OF;
            }
            0x37 => {
                // AAA
                if self.al() & 0xf > 9 || self.flags & AF != 0 {
                    let ax = self.ax().wrapping_add(0x106);
                    self.set_ax(ax);
                    self.flags |= AF | CF;
                } else {
                    self.flags &= !(AF | CF);
                }
                let al = self.al() & 0xf;
                self.set_al(al);
            }
            0x3f => {
                // AAS
                if self.al() & 0xf > 9 || self.flags & AF != 0 {
                    let al = self.al();
                    let ah = self.ah().wrapping_sub(1).wrapping_sub((al < 6) as u8);
                    self.set_al(al.wrapping_sub(6));
                    self.set_ah(ah);
                    self.flags |= AF | CF;
                } else {
                    self.flags &= !(AF | CF);
                }
                let al = self.al() & 0xf;
                self.set_al(al);
            }
            0x40..=0x47 => {
                let r = op & 7;
                let v = self.getr(r, osz);
                let v = self.inc(v, osz);
                self.setr(r, osz, v);
            }
            0x48..=0x4f => {
                let r = op & 7;
                let v = self.getr(r, osz);
                let v = self.dec(v, osz);
                self.setr(r, osz, v);
            }
            0x50..=0x57 => {
                let v = self.getr(op & 7, osz);
                self.push_sz(b, v, osz)?;
            }
            0x58..=0x5f => {
                let v = self.pop_sz(b, osz)?;
                self.setr(op & 7, osz, v);
            }
            0x60 => {
                let sp = self.getr(ESP as u8, osz);
                for r in 0..8u8 {
                    let v = if r == ESP as u8 { sp } else { self.getr(r, osz) };
                    self.push_sz(b, v, osz)?;
                }
            }
            0x61 => {
                let mut vals = [0u32; 8];
                for i in 0..8 {
                    vals[7 - i] = self.peek_sz(b, i as u32 * osz as u32, osz)?;
                }
                self.add_sp(8 * osz as u32);
                for r in 0..8u8 {
                    if r != ESP as u8 {
                        self.setr(r, osz, vals[r as usize]);
                    }
                }
            }
            0x62 => {
                // BOUND
                let (reg, rm) = self.modrm(b, d)?;
                let (s, o) = Self::mem(rm)?;
                let idx = sext(self.getr(reg, osz), osz) as i32;
                let lo = sext(self.rd_sz(b, s, o, osz)?, osz) as i32;
                let hi = sext(self.rd_sz(b, s, o.wrapping_add(osz as u32), osz)?, osz) as i32;
                if idx < lo || idx > hi {
                    return Err(Fault { vec: 5, code: None });
                }
            }
            0x63 => {
                // ARPL
                if !self.pm() || self.flags & VM != 0 {
                    return Err(UD);
                }
                let (reg, rm) = self.modrm(b, d)?;
                self.check_w(rm, 2)?;
                let dst = self.get(b, rm, 2)?;
                let src = self.getr(reg, 2);
                if dst & 3 < src & 3 {
                    self.set(b, rm, 2, (dst & !3) | (src & 3))?;
                    self.flags |= ZF;
                } else {
                    self.flags &= !ZF;
                }
            }
            0x68 => {
                let v = self.imm(b, osz)?;
                self.push_sz(b, v, osz)?;
            }
            0x6a => {
                let v = self.f8(b)? as i8 as i32 as u32;
                self.push_sz(b, v, osz)?;
            }
            0x69 | 0x6b => {
                let (reg, rm) = self.modrm(b, d)?;
                let a = self.get(b, rm, osz)?;
                let v = if op == 0x69 { self.imm(b, osz)? } else { self.f8(b)? as i8 as i32 as u32 };
                let r = self.imul2(a, v, osz);
                self.setr(reg, osz, r);
            }
            0x6c..=0x6f | 0xa4..=0xa7 | 0xaa..=0xaf => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                self.string(b, op, d, sz)?;
            }
            0x70..=0x7f => {
                let rel = self.f8(b)? as i8 as i32 as u32;
                if self.cond(op & 0xf) {
                    self.jmp_near(self.eip.wrapping_add(rel), osz)?;
                }
            }
            0x80..=0x83 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (aop, rm) = self.modrm(b, d)?;
                let v = match op {
                    0x81 => self.imm(b, osz)?,
                    0x83 => sext(self.f8(b)? as u32, 1) & mask(sz),
                    _ => self.f8(b)? as u32,
                };
                if aop != 7 {
                    self.check_w(rm, sz)?;
                }
                let a = self.get(b, rm, sz)?;
                let r = self.alu(aop, a, v, sz);
                if aop != 7 {
                    self.set(b, rm, sz, r)?;
                }
            }
            0x84 | 0x85 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (reg, rm) = self.modrm(b, d)?;
                let a = self.get(b, rm, sz)?;
                let v = self.getr(reg, sz);
                self.logic(a & v, sz);
            }
            0x86 | 0x87 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (reg, rm) = self.modrm(b, d)?;
                self.check_w(rm, sz)?;
                let a = self.get(b, rm, sz)?;
                let v = self.getr(reg, sz);
                self.set(b, rm, sz, v)?;
                self.setr(reg, sz, a);
            }
            0x88 | 0x89 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (reg, rm) = self.modrm(b, d)?;
                let v = self.getr(reg, sz);
                self.set(b, rm, sz, v)?;
            }
            0x8a | 0x8b => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (reg, rm) = self.modrm(b, d)?;
                let v = self.get(b, rm, sz)?;
                self.setr(reg, sz, v);
            }
            0x8c => {
                let (reg, rm) = self.modrm(b, d)?;
                if reg > 5 {
                    return Err(UD);
                }
                let v = self.seg[reg as usize].sel as u32;
                match rm {
                    Rm::Reg(_) => self.set(b, rm, osz, v)?,
                    Rm::Mem(..) => self.set(b, rm, 2, v)?,
                }
            }
            0x8d => {
                let (reg, rm) = self.modrm(b, d)?;
                let (_, o) = Self::mem(rm)?;
                self.setr(reg, osz, o);
            }
            0x8e => {
                let (reg, rm) = self.modrm(b, d)?;
                if reg == CS as u8 || reg > 5 {
                    return Err(UD);
                }
                let v = self.get(b, rm, 2)? as u16;
                self.load_sreg(b, reg as usize, v)?;
                if reg as usize == SS {
                    self.inhibit = true;
                }
            }
            0x8f => {
                let v = self.pop_sz(b, osz)?;
                let (_, rm) = self.modrm(b, d)?;
                self.set(b, rm, osz, v)?;
            }
            0x90 => {}
            0x91..=0x97 => {
                let r = op & 7;
                let a = self.getr(0, osz);
                let v = self.getr(r, osz);
                self.setr(0, osz, v);
                self.setr(r, osz, a);
            }
            0x98 => {
                if d.op32 {
                    self.regs[EAX] = self.regs[EAX] as u16 as i16 as i32 as u32;
                } else {
                    let v = self.al() as i8 as i16 as u16;
                    self.set_ax(v);
                }
            }
            0x99 => {
                if d.op32 {
                    self.regs[EDX] = if self.regs[EAX] & 0x8000_0000 != 0 { 0xffff_ffff } else { 0 };
                } else {
                    let v = if self.ax() & 0x8000 != 0 { 0xffff } else { 0 };
                    self.set_dx(v);
                }
            }
            0x9a => {
                let off = self.imm(b, osz)?;
                let sel = self.f16(b)?;
                self.call_far(b, sel, off, osz)?;
            }
            0x9b => {
                if self.fpu.is_some() && self.cr0 & (cr0::TS | cr0::MP) == (cr0::TS | cr0::MP) {
                    return Err(NM);
                }
            }
            0x9c => {
                let f = self.flags & !(VM | RF);
                self.push_sz(b, f, osz)?;
            }
            0x9d => {
                let f = self.peek_sz(b, 0, osz)?;
                self.add_sp(osz as u32);
                if self.pm() {
                    let cpl = self.cpl;
                    self.set_flags_pm(f, osz, cpl);
                } else {
                    self.set_flags_real(f, osz);
                }
            }
            0x9e => {
                let m = SF | ZF | AF | PF | CF;
                self.flags = (self.flags & !m) | (self.ah() as u32 & m) | 2;
            }
            0x9f => {
                let v = (self.flags & 0xff) as u8;
                self.set_ah(v);
            }
            0xa0..=0xa3 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let off = self.imm(b, if d.ad32 { 4 } else { 2 })?;
                let s = d.seg.unwrap_or(DS);
                if op < 0xa2 {
                    let v = self.rd_sz(b, s, off, sz)?;
                    self.setr(0, sz, v);
                } else {
                    let v = self.getr(0, sz);
                    self.wr_sz(b, s, off, sz, v)?;
                }
            }
            0xa8 => {
                let v = self.f8(b)? as u32;
                let a = self.getr(0, 1);
                self.logic(a & v, 1);
            }
            0xa9 => {
                let v = self.imm(b, osz)?;
                let a = self.getr(0, osz);
                self.logic(a & v, osz);
            }
            0xb0..=0xb7 => {
                let v = self.f8(b)? as u32;
                self.setr(op & 7, 1, v);
            }
            0xb8..=0xbf => {
                let v = self.imm(b, osz)?;
                self.setr(op & 7, osz, v);
            }
            0xc0 | 0xc1 | 0xd0..=0xd3 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (sop, rm) = self.modrm(b, d)?;
                let count = match op {
                    0xc0 | 0xc1 => self.f8(b)? as u32,
                    0xd0 | 0xd1 => 1,
                    _ => self.cl() as u32,
                } & 31;
                self.check_w(rm, sz)?;
                let a = self.get(b, rm, sz)?;
                let r = self.shift(sop, a, count, sz);
                self.set(b, rm, sz, r)?;
            }
            0xc2 | 0xc3 => {
                let imm = if op == 0xc2 { self.f16(b)? as u32 } else { 0 };
                let ip = self.peek_sz(b, 0, osz)?;
                self.jmp_near(ip, osz)?;
                self.add_sp(osz as u32 + imm);
            }
            0xc4 | 0xc5 => {
                let (reg, rm) = self.modrm(b, d)?;
                let (s, o) = Self::mem(rm)?;
                let off = self.rd_sz(b, s, o, osz)?;
                let sel = self.rd16(b, s, o.wrapping_add(osz as u32))?;
                self.load_sreg(b, if op == 0xc4 { ES } else { DS }, sel)?;
                self.setr(reg, osz, off);
            }
            0xc6 | 0xc7 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (_, rm) = self.modrm(b, d)?;
                let v = self.imm(b, sz)?;
                self.set(b, rm, sz, v)?;
            }
            0xc8 => {
                let alloc = self.f16(b)? as u32;
                let level = (self.f8(b)? & 31) as u32;
                let big = self.seg[SS].big();
                let bp = self.regs[EBP];
                self.push_sz(b, bp, osz)?;
                let frame = self.stack_ptr();
                if level > 0 {
                    let mut p = if big { self.regs[EBP] } else { self.regs[EBP] & 0xffff };
                    for _ in 1..level {
                        p = p.wrapping_sub(osz as u32) & if big { 0xffff_ffff } else { 0xffff };
                        let v = self.rd_sz(b, SS, p, osz)?;
                        self.push_sz(b, v, osz)?;
                    }
                    self.push_sz(b, frame, osz)?;
                }
                self.setr(EBP as u8, osz, frame);
                let sp = self.stack_ptr().wrapping_sub(alloc);
                // The new stack top must be inside the segment.
                self.addr(SS, sp & if big { 0xffff_ffff } else { 0xffff }, 1, true)?;
                self.set_sp(sp);
            }
            0xc9 => {
                let bp = if self.seg[SS].big() { self.regs[EBP] } else { self.regs[EBP] & 0xffff };
                let v = self.rd_sz(b, SS, bp, osz)?;
                self.set_sp(bp.wrapping_add(osz as u32));
                self.setr(EBP as u8, osz, v);
            }
            0xca | 0xcb => {
                let imm = if op == 0xca { self.f16(b)? as u32 } else { 0 };
                self.ret_far(b, osz, imm)?;
            }
            0xcc => self.int(b, 3, true, None, false)?,
            0xcd => {
                let v = self.f8(b)?;
                self.int(b, v, true, None, false)?;
            }
            0xce => {
                if self.flags & OF != 0 {
                    self.int(b, 4, true, None, false)?;
                }
            }
            0xcf => self.iret(b, osz)?,
            0xd4 => {
                let base = self.f8(b)?;
                if base == 0 {
                    return Err(DE);
                }
                let al = self.al();
                self.set_ah(al / base);
                self.set_al(al % base);
                self.set_szp(self.al() as u32, 1);
                self.flags &= !(CF | OF | AF);
            }
            0xd5 => {
                let base = self.f8(b)?;
                let al = self.al().wrapping_add(self.ah().wrapping_mul(base));
                self.set_ax(al as u16);
                self.set_szp(al as u32, 1);
                self.flags &= !(CF | OF | AF);
            }
            0xd6 => {
                let v = if self.flags & CF != 0 { 0xff } else { 0 };
                self.set_al(v);
            }
            0xd7 => {
                let s = d.seg.unwrap_or(DS);
                let off = if d.ad32 { self.regs[EBX].wrapping_add(self.al() as u32) } else { (self.regs[EBX].wrapping_add(self.al() as u32)) & 0xffff };
                let v = self.rd8(b, s, off)?;
                self.set_al(v);
            }
            0xd8..=0xdf => return self.esc(b, op, d),
            0xe0..=0xe3 => {
                let rel = self.f8(b)? as i8 as i32 as u32;
                let cm = if d.ad32 { 0xffff_ffff } else { 0xffff };
                let mut c = self.regs[ECX] & cm;
                let take = if op == 0xe3 {
                    c == 0
                } else {
                    c = c.wrapping_sub(1) & cm;
                    self.regs[ECX] = (self.regs[ECX] & !cm) | c;
                    let z = self.flags & ZF != 0;
                    c != 0 && match op {
                        0xe0 => !z,
                        0xe1 => z,
                        _ => true,
                    }
                };
                if take {
                    self.jmp_near(self.eip.wrapping_add(rel), osz)?;
                }
            }
            0xe4 | 0xe5 | 0xec | 0xed => {
                let port = if op < 0xe8 { self.f8(b)? as u16 } else { self.dx() };
                self.io_check()?;
                let sz = if op & 1 == 0 { 1 } else { osz };
                let v = b.io_read(port, sz);
                self.setr(0, sz, v);
            }
            0xe6 | 0xe7 | 0xee | 0xef => {
                let port = if op < 0xe8 { self.f8(b)? as u16 } else { self.dx() };
                self.io_check()?;
                let sz = if op & 1 == 0 { 1 } else { osz };
                let v = self.getr(0, sz);
                b.io_write(port, sz, v);
            }
            0xe8 => {
                let rel = self.imm(b, osz)?;
                let rel = sext(rel, osz);
                let ip = self.eip;
                let target = ip.wrapping_add(rel) & mask(osz);
                if target > self.seg[CS].limit {
                    return Err(gp(0));
                }
                self.push_sz(b, ip, osz)?;
                self.eip = target;
            }
            0xe9 => {
                let rel = sext(self.imm(b, osz)?, osz);
                self.jmp_near(self.eip.wrapping_add(rel), osz)?;
            }
            0xea => {
                let off = self.imm(b, osz)?;
                let sel = self.f16(b)?;
                self.jmp_far(b, sel, off)?;
            }
            0xeb => {
                let rel = self.f8(b)? as i8 as i32 as u32;
                self.jmp_near(self.eip.wrapping_add(rel), osz)?;
            }
            0xf1 => self.int(b, 1, false, None, false)?,
            0xf4 => {
                self.priv0()?;
                self.halted = true;
            }
            0xf5 => self.flags ^= CF,
            0xf6 | 0xf7 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (gop, rm) = self.modrm(b, d)?;
                self.group3(b, gop, rm, sz)?;
            }
            0xf8 => self.flags &= !CF,
            0xf9 => self.flags |= CF,
            0xfa => {
                if !self.iopl_ok() {
                    return Err(gp(0));
                }
                self.flags &= !IF;
            }
            0xfb => {
                if !self.iopl_ok() {
                    return Err(gp(0));
                }
                if self.flags & IF == 0 {
                    self.inhibit = true;
                }
                self.flags |= IF;
            }
            0xfc => self.flags &= !DF,
            0xfd => self.flags |= DF,
            0xfe => {
                let save = self.eip;
                let m = self.f8(b)?;
                if m == 0x38 {
                    let n = self.f16(b)?;
                    self.exit = Some(Exit::Callback(n));
                    return Ok(());
                }
                self.eip = save;
                let (gop, rm) = self.modrm(b, d)?;
                if gop > 1 {
                    return Err(UD);
                }
                self.check_w(rm, 1)?;
                let a = self.get(b, rm, 1)?;
                let r = if gop == 0 { self.inc(a, 1) } else { self.dec(a, 1) };
                self.set(b, rm, 1, r)?;
            }
            0xff => {
                let (gop, rm) = self.modrm(b, d)?;
                match gop {
                    0 | 1 => {
                        self.check_w(rm, osz)?;
                        let a = self.get(b, rm, osz)?;
                        let r = if gop == 0 { self.inc(a, osz) } else { self.dec(a, osz) };
                        self.set(b, rm, osz, r)?;
                    }
                    2 => {
                        let t = self.get(b, rm, osz)?;
                        if t > self.seg[CS].limit {
                            return Err(gp(0));
                        }
                        let ip = self.eip;
                        self.push_sz(b, ip, osz)?;
                        self.eip = t;
                    }
                    3 | 5 => {
                        let (s, o) = Self::mem(rm)?;
                        let off = self.rd_sz(b, s, o, osz)?;
                        let sel = self.rd16(b, s, o.wrapping_add(osz as u32))?;
                        if gop == 3 {
                            self.call_far(b, sel, off, osz)?;
                        } else {
                            self.jmp_far(b, sel, off)?;
                        }
                    }
                    4 => {
                        let t = self.get(b, rm, osz)?;
                        self.jmp_near(t, osz)?;
                    }
                    6 => {
                        let v = self.get(b, rm, osz)?;
                        self.push_sz(b, v, osz)?;
                    }
                    _ => return Err(UD),
                }
            }
            _ => return Err(UD),
        }
        Ok(())
    }

    fn imul2(&mut self, a: u32, v: u32, sz: u8) -> u32 {
        let p = sext(a, sz) as i32 as i64 * sext(v, sz) as i32 as i64;
        let r = p as u32 & mask(sz);
        let fits = sext(r, sz) as i32 as i64 == p;
        self.set_cf_of(!fits, !fits);
        self.set_szp(r, sz);
        r
    }

    fn group3<B: Bus>(&mut self, b: &mut B, gop: u8, rm: Rm, sz: u8) -> R<()> {
        match gop {
            0 | 1 => {
                let a = self.get(b, rm, sz)?;
                let v = self.imm(b, sz)?;
                self.logic(a & v, sz);
            }
            2 => {
                self.check_w(rm, sz)?;
                let a = self.get(b, rm, sz)?;
                self.set(b, rm, sz, !a & mask(sz))?;
            }
            3 => {
                self.check_w(rm, sz)?;
                let a = self.get(b, rm, sz)?;
                let r = self.neg(a, sz);
                self.set(b, rm, sz, r)?;
            }
            4 => {
                // MUL
                let v = self.get(b, rm, sz)? as u64;
                match sz {
                    1 => {
                        let r = self.al() as u64 * v;
                        self.set_ax(r as u16);
                        self.set_cf_of(r > 0xff, r > 0xff);
                        self.set_szp(r as u32 & 0xff, 1);
                    }
                    2 => {
                        let r = self.ax() as u64 * v;
                        self.set_ax(r as u16);
                        self.set_dx((r >> 16) as u16);
                        self.set_cf_of(r > 0xffff, r > 0xffff);
                        self.set_szp(r as u32 & 0xffff, 2);
                    }
                    _ => {
                        let r = self.regs[EAX] as u64 * v;
                        self.regs[EAX] = r as u32;
                        self.regs[EDX] = (r >> 32) as u32;
                        self.set_cf_of(r >> 32 != 0, r >> 32 != 0);
                        self.set_szp(r as u32, 4);
                    }
                }
            }
            5 => {
                // IMUL
                let v = sext(self.get(b, rm, sz)?, sz) as i32 as i64;
                match sz {
                    1 => {
                        let r = self.al() as i8 as i64 * v;
                        self.set_ax(r as u16);
                        let ov = r != r as i8 as i64;
                        self.set_cf_of(ov, ov);
                        self.set_szp(r as u32 & 0xff, 1);
                    }
                    2 => {
                        let r = self.ax() as i16 as i64 * v;
                        self.set_ax(r as u16);
                        self.set_dx((r >> 16) as u16);
                        let ov = r != r as i16 as i64;
                        self.set_cf_of(ov, ov);
                        self.set_szp(r as u32 & 0xffff, 2);
                    }
                    _ => {
                        let r = self.regs[EAX] as i32 as i64 * v;
                        self.regs[EAX] = r as u32;
                        self.regs[EDX] = (r >> 32) as u32;
                        let ov = r != r as i32 as i64;
                        self.set_cf_of(ov, ov);
                        self.set_szp(r as u32, 4);
                    }
                }
            }
            6 => {
                // DIV
                let v = self.get(b, rm, sz)? as u64;
                if v == 0 {
                    return Err(DE);
                }
                match sz {
                    1 => {
                        let n = self.ax() as u64;
                        let q = n / v;
                        if q > 0xff {
                            return Err(DE);
                        }
                        self.set_al(q as u8);
                        self.set_ah((n % v) as u8);
                    }
                    2 => {
                        let n = (self.dx() as u64) << 16 | self.ax() as u64;
                        let q = n / v;
                        if q > 0xffff {
                            return Err(DE);
                        }
                        self.set_ax(q as u16);
                        self.set_dx((n % v) as u16);
                    }
                    _ => {
                        let n = (self.regs[EDX] as u64) << 32 | self.regs[EAX] as u64;
                        let q = n / v;
                        if q > 0xffff_ffff {
                            return Err(DE);
                        }
                        self.regs[EAX] = q as u32;
                        self.regs[EDX] = (n % v) as u32;
                    }
                }
            }
            _ => {
                // IDIV
                let v = sext(self.get(b, rm, sz)?, sz) as i32 as i64;
                if v == 0 {
                    return Err(DE);
                }
                match sz {
                    1 => {
                        let n = self.ax() as i16 as i64;
                        let q = n / v;
                        if q != q as i8 as i64 {
                            return Err(DE);
                        }
                        self.set_al(q as u8);
                        self.set_ah((n % v) as u8);
                    }
                    2 => {
                        let n = ((self.dx() as u32) << 16 | self.ax() as u32) as i32 as i64;
                        let q = n / v;
                        if q != q as i16 as i64 {
                            return Err(DE);
                        }
                        self.set_ax(q as u16);
                        self.set_dx((n % v) as u16);
                    }
                    _ => {
                        let n = ((self.regs[EDX] as u64) << 32 | self.regs[EAX] as u64) as i64 as i128;
                        let v = v as i128;
                        let q = n / v;
                        if q != q as i32 as i128 {
                            return Err(DE);
                        }
                        self.regs[EAX] = q as u32;
                        self.regs[EDX] = (n % v) as u32;
                    }
                }
            }
        }
        Ok(())
    }

    /// String instructions with their REP prefixes.
    fn string<B: Bus>(&mut self, b: &mut B, op: u8, d: &D, sz: u8) -> R<()> {
        let am = if d.ad32 { 0xffff_ffff } else { 0xffff };
        let src = d.seg.unwrap_or(DS);
        let step = if self.flags & DF != 0 { (sz as u32).wrapping_neg() } else { sz as u32 };
        let is_io = op <= 0x6f;
        if is_io {
            self.io_check()?;
        }
        loop {
            if d.rep != 0 && self.regs[ECX] & am == 0 {
                break;
            }
            let si = self.regs[ESI] & am;
            let di = self.regs[EDI] & am;
            let mut adv_si = false;
            let mut adv_di = false;
            match op {
                0x6c | 0x6d => {
                    self.addr(ES, di, sz as u32, true)?;
                    let v = b.io_read(self.dx(), sz);
                    self.wr_sz(b, ES, di, sz, v)?;
                    adv_di = true;
                }
                0x6e | 0x6f => {
                    let v = self.rd_sz(b, src, si, sz)?;
                    b.io_write(self.dx(), sz, v);
                    adv_si = true;
                }
                0xa4 | 0xa5 => {
                    let v = self.rd_sz(b, src, si, sz)?;
                    self.wr_sz(b, ES, di, sz, v)?;
                    adv_si = true;
                    adv_di = true;
                }
                0xa6 | 0xa7 => {
                    let a = self.rd_sz(b, src, si, sz)?;
                    let v = self.rd_sz(b, ES, di, sz)?;
                    self.sub(a, v, 0, sz);
                    adv_si = true;
                    adv_di = true;
                }
                0xaa | 0xab => {
                    let v = self.getr(0, sz);
                    self.wr_sz(b, ES, di, sz, v)?;
                    adv_di = true;
                }
                0xac | 0xad => {
                    let v = self.rd_sz(b, src, si, sz)?;
                    self.setr(0, sz, v);
                    adv_si = true;
                }
                _ => {
                    let a = self.getr(0, sz);
                    let v = self.rd_sz(b, ES, di, sz)?;
                    self.sub(a, v, 0, sz);
                    adv_di = true;
                }
            }
            if adv_si {
                self.regs[ESI] = (self.regs[ESI] & !am) | (si.wrapping_add(step) & am);
            }
            if adv_di {
                self.regs[EDI] = (self.regs[EDI] & !am) | (di.wrapping_add(step) & am);
            }
            if d.rep == 0 {
                break;
            }
            let c = (self.regs[ECX] & am).wrapping_sub(1) & am;
            self.regs[ECX] = (self.regs[ECX] & !am) | c;
            if matches!(op, 0xa6 | 0xa7 | 0xae | 0xaf) {
                let z = self.flags & ZF != 0;
                if (d.rep == 0xf3 && !z) || (d.rep == 0xf2 && z) {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Bit offset addressing of BT/BTS/BTR/BTC with a register offset.
    fn bit_operand(rm: Rm, off: u32, sz: u8) -> (Rm, u32) {
        let bits = sz as u32 * 8;
        match rm {
            Rm::Reg(_) => (rm, off & (bits - 1)),
            Rm::Mem(s, o) => {
                let off = sext(off, sz) as i32;
                let words = off.div_euclid(bits as i32);
                let bit = off.rem_euclid(bits as i32) as u32;
                (Rm::Mem(s, o.wrapping_add((words * sz as i32) as u32)), bit)
            }
        }
    }

    fn bit_op<B: Bus>(&mut self, b: &mut B, kind: u8, rm: Rm, bit: u32, sz: u8) -> R<()> {
        if kind != 0 {
            self.check_w(rm, sz)?;
        }
        let v = self.get(b, rm, sz)?;
        let m = 1u32 << bit;
        self.set_flag(CF, v & m != 0);
        let r = match kind {
            0 => return Ok(()),
            1 => v | m,
            2 => v & !m,
            _ => v ^ m,
        };
        self.set(b, rm, sz, r)
    }

    fn exec0f<B: Bus>(&mut self, b: &mut B, d: &D) -> R<()> {
        let osz = d.osz();
        let op = self.f8(b)?;
        match op {
            0x00 => {
                if !self.pm() || self.flags & VM != 0 {
                    return Err(UD);
                }
                let (gop, rm) = self.modrm(b, d)?;
                match gop {
                    0 | 1 => {
                        let v = if gop == 0 { self.ldtr.sel } else { self.tr.sel } as u32;
                        match rm {
                            Rm::Reg(_) => self.set(b, rm, osz, v)?,
                            _ => self.set(b, rm, 2, v)?,
                        }
                    }
                    2 | 3 => {
                        self.priv0()?;
                        let sel = self.get(b, rm, 2)? as u16;
                        if gop == 2 {
                            self.load_ldt(b, sel)?;
                        } else {
                            self.load_tr(b, sel)?;
                        }
                    }
                    4 | 5 => {
                        let sel = self.get(b, rm, 2)? as u16;
                        let ok = self.verify(b, sel, gop == 5);
                        self.set_flag(ZF, ok);
                    }
                    _ => return Err(UD),
                }
            }
            0x01 => {
                let (gop, rm) = self.modrm(b, d)?;
                match gop {
                    0 | 1 => {
                        let (s, o) = Self::mem(rm)?;
                        let t = if gop == 0 { self.gdtr } else { self.idtr };
                        self.wr16(b, s, o, t.limit as u16)?;
                        let base = if d.op32 { t.base } else { t.base & 0x00ff_ffff };
                        self.wr32(b, s, o.wrapping_add(2), base)?;
                    }
                    2 | 3 => {
                        self.priv0()?;
                        let (s, o) = Self::mem(rm)?;
                        let limit = self.rd16(b, s, o)? as u32;
                        let mut base = self.rd32(b, s, o.wrapping_add(2))?;
                        if !d.op32 {
                            base &= 0x00ff_ffff;
                        }
                        let t = Dtr { base, limit };
                        if gop == 2 {
                            self.gdtr = t;
                        } else {
                            self.idtr = t;
                        }
                    }
                    4 => {
                        let v = self.cr0 & 0xffff;
                        match rm {
                            Rm::Reg(_) => self.set(b, rm, osz, if osz == 4 { self.cr0 } else { v })?,
                            _ => self.set(b, rm, 2, v)?,
                        }
                    }
                    6 => {
                        self.priv0()?;
                        let v = self.get(b, rm, 2)?;
                        // LMSW can set PE but not clear it.
                        let nv = (self.cr0 & !0xe) | (v & 0xf) | (self.cr0 & 1);
                        self.set_cr0(nv);
                    }
                    7 => {
                        self.priv0()?;
                        Self::mem(rm)?;
                    }
                    _ => return Err(UD),
                }
            }
            0x02 | 0x03 => {
                if !self.pm() || self.flags & VM != 0 {
                    return Err(UD);
                }
                let (reg, rm) = self.modrm(b, d)?;
                let sel = self.get(b, rm, 2)? as u16;
                match self.lar_lsl(b, sel, op == 0x03) {
                    Some(v) => {
                        let v = if op == 0x02 && osz == 2 { v & 0xff00 } else { v };
                        self.setr(reg, osz, v);
                        self.flags |= ZF;
                    }
                    None => self.flags &= !ZF,
                }
            }
            0x06 => {
                self.priv0()?;
                self.cr0 &= !cr0::TS;
            }
            0x08 | 0x09 => self.priv0()?,
            0x20..=0x23 => {
                self.priv0()?;
                let m = self.f8(b)?;
                let n = (m >> 3) & 7;
                let r = (m & 7) as usize;
                match op {
                    0x20 => {
                        self.regs[r] = match n {
                            0 => self.cr0,
                            2 => self.cr2,
                            3 => self.cr3,
                            _ => return Err(UD),
                        }
                    }
                    0x22 => match n {
                        0 => {
                            let v = self.regs[r];
                            self.set_cr0(v);
                        }
                        2 => self.cr2 = self.regs[r],
                        3 => self.cr3 = self.regs[r],
                        _ => return Err(UD),
                    },
                    0x21 => self.regs[r] = self.dr[n as usize],
                    _ => self.dr[n as usize] = self.regs[r],
                }
            }
            0x24 | 0x26 => {
                // Test registers: read 0, writes ignored.
                self.priv0()?;
                let m = self.f8(b)?;
                if op == 0x24 {
                    self.regs[(m & 7) as usize] = 0;
                }
            }
            0x0b | 0xff => return Err(UD),
            0x80..=0x8f => {
                let rel = sext(self.imm(b, osz)?, osz);
                if self.cond(op & 0xf) {
                    self.jmp_near(self.eip.wrapping_add(rel), osz)?;
                }
            }
            0x90..=0x9f => {
                let (_, rm) = self.modrm(b, d)?;
                let v = self.cond(op & 0xf) as u32;
                self.set(b, rm, 1, v)?;
            }
            0xa0 | 0xa8 => {
                let v = self.seg[if op == 0xa0 { FS } else { GS }].sel as u32;
                self.push_sz(b, v, osz)?;
            }
            0xa1 | 0xa9 => {
                let v = self.peek_sz(b, 0, osz)? as u16;
                self.load_sreg(b, if op == 0xa1 { FS } else { GS }, v)?;
                self.add_sp(osz as u32);
            }
            0xa3 | 0xab | 0xb3 | 0xbb => {
                let (reg, rm) = self.modrm(b, d)?;
                let off = self.getr(reg, osz);
                let (rm, bit) = Self::bit_operand(rm, off, osz);
                let kind = match op {
                    0xa3 => 0,
                    0xab => 1,
                    0xb3 => 2,
                    _ => 3,
                };
                self.bit_op(b, kind, rm, bit, osz)?;
            }
            0xba => {
                let (gop, rm) = self.modrm(b, d)?;
                if gop < 4 {
                    return Err(UD);
                }
                let bit = self.f8(b)? as u32 & (osz as u32 * 8 - 1);
                self.bit_op(b, gop - 4, rm, bit, osz)?;
            }
            0xa4 | 0xa5 | 0xac | 0xad => {
                let (reg, rm) = self.modrm(b, d)?;
                let count = if op & 1 == 0 { self.f8(b)? as u32 } else { self.cl() as u32 };
                self.check_w(rm, osz)?;
                let a = self.get(b, rm, osz)?;
                let v = self.getr(reg, osz);
                let r = self.shiftd(op < 0xa8, a, v, count, osz);
                self.set(b, rm, osz, r)?;
            }
            0xaf => {
                let (reg, rm) = self.modrm(b, d)?;
                let v = self.get(b, rm, osz)?;
                let a = self.getr(reg, osz);
                let r = self.imul2(a, v, osz);
                self.setr(reg, osz, r);
            }
            0xb0 | 0xb1 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (reg, rm) = self.modrm(b, d)?;
                self.check_w(rm, sz)?;
                let dst = self.get(b, rm, sz)?;
                let acc = self.getr(0, sz);
                self.sub(acc, dst, 0, sz);
                if acc == dst {
                    let v = self.getr(reg, sz);
                    self.set(b, rm, sz, v)?;
                } else {
                    self.set(b, rm, sz, dst)?;
                    self.setr(0, sz, dst);
                }
            }
            0xb2 | 0xb4 | 0xb5 => {
                let (reg, rm) = self.modrm(b, d)?;
                let (s, o) = Self::mem(rm)?;
                let off = self.rd_sz(b, s, o, osz)?;
                let sel = self.rd16(b, s, o.wrapping_add(osz as u32))?;
                let sr = match op {
                    0xb2 => SS,
                    0xb4 => FS,
                    _ => GS,
                };
                self.load_sreg(b, sr, sel)?;
                self.setr(reg, osz, off);
            }
            0xb6 | 0xb7 | 0xbe | 0xbf => {
                let (reg, rm) = self.modrm(b, d)?;
                let ssz = if op & 1 == 0 { 1 } else { 2 };
                let v = self.get(b, rm, ssz)?;
                let v = if op >= 0xbe { sext(v, ssz) } else { v };
                self.setr(reg, osz, v);
            }
            0xbc | 0xbd => {
                let (reg, rm) = self.modrm(b, d)?;
                let v = self.get(b, rm, osz)?;
                if v == 0 {
                    self.flags |= ZF;
                } else {
                    self.flags &= !ZF;
                    let i = if op == 0xbc { v.trailing_zeros() } else { 31 - v.leading_zeros() };
                    self.setr(reg, osz, i);
                }
            }
            0xc0 | 0xc1 => {
                let sz = if op & 1 == 0 { 1 } else { osz };
                let (reg, rm) = self.modrm(b, d)?;
                self.check_w(rm, sz)?;
                let dst = self.get(b, rm, sz)?;
                let src = self.getr(reg, sz);
                let r = self.add(dst, src, 0, sz);
                self.setr(reg, sz, dst);
                self.set(b, rm, sz, r)?;
            }
            0xc8..=0xcf => {
                let r = (op & 7) as usize;
                self.regs[r] = self.regs[r].swap_bytes();
            }
            _ => return Err(UD),
        }
        Ok(())
    }

    /// MOV CR0 / LMSW: a change of PE switches modes.
    fn set_cr0(&mut self, v: u32) {
        let old = self.cr0;
        self.cr0 = v | cr0::ET;
        if (old ^ self.cr0) & cr0::PE != 0 {
            if self.cr0 & cr0::PE != 0 {
                self.cpl = 0;
            } else {
                self.cpl = 0;
            }
        }
    }

    /// VERR/VERW.
    fn verify<B: Bus>(&self, b: &mut B, sel: u16, write: bool) -> bool {
        if sel & !3 == 0 {
            return false;
        }
        let Ok(d) = self.read_desc(b, sel) else { return false };
        if !d.is_seg() {
            return false;
        }
        let rpl = (sel & 3) as u8;
        if !d.conforming() && d.dpl() < self.cpl.max(rpl) {
            return false;
        }
        if write { !d.is_code() && d.attr() & attr::RW != 0 } else { !d.is_code() || d.attr() & attr::RW != 0 }
    }

    /// LAR (access rights) or LSL (limit) of `sel`, if visible.
    fn lar_lsl<B: Bus>(&self, b: &mut B, sel: u16, lsl: bool) -> Option<u32> {
        if sel & !3 == 0 {
            return None;
        }
        let d = self.read_desc(b, sel).ok()?;
        let rpl = (sel & 3) as u8;
        if d.is_seg() {
            if !d.conforming() && d.dpl() < self.cpl.max(rpl) {
                return None;
            }
        } else {
            let ok = if lsl { matches!(d.sys_type(), 1 | 2 | 3 | 9 | 0xb) } else { matches!(d.sys_type(), 1..=5 | 9 | 0xb | 0xc) };
            if !ok || d.dpl() < self.cpl.max(rpl) {
                return None;
            }
        }
        Some(if lsl { d.limit() } else { (d.raw >> 32) as u32 & 0x00f0_ff00 })
    }

    /// Coprocessor escapes (D8-DF).
    fn esc<B: Bus>(&mut self, b: &mut B, op: u8, d: &D) -> R<()> {
        if self.cr0 & (cr0::EM | cr0::TS) != 0 {
            return Err(NM);
        }
        let (reg, rm) = self.modrm(b, d)?;
        if self.fpu.is_none() {
            // No coprocessor: the instruction does nothing (so FNSTSW
            // leaves its target alone and programs see no FPU).
            return Ok(());
        }
        self.fpu_exec(b, op, reg, rm, d.op32)
    }

    pub(crate) fn fpu_mem(rm: Rm) -> Option<(usize, u32)> {
        match rm {
            Rm::Mem(s, o) => Some((s, o)),
            _ => None,
        }
    }
}
