//! Segmentation: memory access checks, segment loads, far control
//! transfers, interrupts and IRET, in real and protected mode.

use crate::attr::*;
use crate::flags::*;
use crate::*;

/// A descriptor as read from the GDT or LDT.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Desc {
    pub raw: u64,
}

impl Desc {
    pub fn base(&self) -> u32 {
        ((self.raw >> 16) & 0xff_ffff) as u32 | (((self.raw >> 56) & 0xff) as u32) << 24
    }
    pub fn limit(&self) -> u32 {
        let l = (self.raw & 0xffff) as u32 | (((self.raw >> 48) & 0xf) as u32) << 16;
        if self.attr() & G != 0 { l << 12 | 0xfff } else { l }
    }
    pub fn attr(&self) -> u32 {
        ((self.raw >> 40) & 0xff) as u32 | (((self.raw >> 52) & 0xf) as u32) << 12
    }
    pub fn dpl(&self) -> u8 {
        ((self.raw >> 45) & 3) as u8
    }
    pub fn present(&self) -> bool {
        self.raw & (1 << 47) != 0
    }
    /// Code or data segment (S bit).
    pub fn is_seg(&self) -> bool {
        self.raw & (1 << 44) != 0
    }
    /// System descriptor type (S = 0).
    pub fn sys_type(&self) -> u8 {
        ((self.raw >> 40) & 0xf) as u8
    }
    pub fn is_code(&self) -> bool {
        self.is_seg() && self.attr() & CODE != 0
    }
    pub fn conforming(&self) -> bool {
        self.is_code() && self.attr() & DC != 0
    }
    pub fn seg(&self, sel: u16) -> Seg {
        Seg { sel, base: self.base(), limit: self.limit(), attr: self.attr() }
    }
    // Gates.
    pub fn gate_sel(&self) -> u16 {
        (self.raw >> 16) as u16
    }
    pub fn gate_off(&self) -> u32 {
        (self.raw & 0xffff) as u32 | if self.sys_type() & 8 != 0 { ((self.raw >> 48) as u32) << 16 } else { 0 }
    }
    pub fn gate_params(&self) -> u32 {
        ((self.raw >> 32) & 0x1f) as u32
    }
    pub fn gate32(&self) -> bool {
        self.sys_type() & 8 != 0
    }
}

impl Cpu {
    /// Linear address of an access of `len` bytes at `s:off`, checked
    /// against the segment's limit and (in protected mode) its type.
    #[inline]
    pub(crate) fn addr(&self, s: usize, off: u32, len: u32, write: bool) -> R<u32> {
        let sg = &self.seg[s];
        let fault = || if s == SS { ss_fault(0) } else { gp(0) };
        if self.cr0 & cr0::PE != 0 {
            let a = sg.attr;
            if a & UNUSABLE != 0 {
                return Err(fault());
            }
            if a & CODE != 0 {
                if write || a & RW == 0 {
                    return Err(gp(0));
                }
            } else if write && a & RW == 0 {
                return Err(fault());
            }
        }
        let end = off.wrapping_add(len - 1);
        if sg.attr & (CODE | DC) == DC {
            // Expand down: valid offsets are above the limit.
            let max = if sg.attr & BIG != 0 { 0xffff_ffff } else { 0xffff };
            if off <= sg.limit || end > max || end < off {
                return Err(fault());
            }
        } else if end > sg.limit || end < off {
            return Err(fault());
        }
        Ok(sg.base.wrapping_add(off))
    }

    pub(crate) fn rd8<B: Bus>(&self, b: &mut B, s: usize, off: u32) -> R<u8> {
        Ok(b.read8(self.addr(s, off, 1, false)?))
    }
    pub(crate) fn rd16<B: Bus>(&self, b: &mut B, s: usize, off: u32) -> R<u16> {
        Ok(b.read16(self.addr(s, off, 2, false)?))
    }
    pub(crate) fn rd32<B: Bus>(&self, b: &mut B, s: usize, off: u32) -> R<u32> {
        Ok(b.read32(self.addr(s, off, 4, false)?))
    }
    pub(crate) fn wr8<B: Bus>(&self, b: &mut B, s: usize, off: u32, v: u8) -> R<()> {
        b.write8(self.addr(s, off, 1, true)?, v);
        Ok(())
    }
    pub(crate) fn wr16<B: Bus>(&self, b: &mut B, s: usize, off: u32, v: u16) -> R<()> {
        b.write16(self.addr(s, off, 2, true)?, v);
        Ok(())
    }
    pub(crate) fn wr32<B: Bus>(&self, b: &mut B, s: usize, off: u32, v: u32) -> R<()> {
        b.write32(self.addr(s, off, 4, true)?, v);
        Ok(())
    }
    pub(crate) fn rd_sz<B: Bus>(&self, b: &mut B, s: usize, off: u32, sz: u8) -> R<u32> {
        match sz {
            1 => self.rd8(b, s, off).map(|v| v as u32),
            2 => self.rd16(b, s, off).map(|v| v as u32),
            _ => self.rd32(b, s, off),
        }
    }
    pub(crate) fn wr_sz<B: Bus>(&self, b: &mut B, s: usize, off: u32, sz: u8, v: u32) -> R<()> {
        match sz {
            1 => self.wr8(b, s, off, v as u8),
            2 => self.wr16(b, s, off, v as u16),
            _ => self.wr32(b, s, off, v),
        }
    }

    // ---- The stack ----

    pub(crate) fn push_sz<B: Bus>(&mut self, b: &mut B, v: u32, sz: u8) -> R<()> {
        let big = self.seg[SS].big();
        let sp = if big { self.regs[ESP] } else { self.regs[ESP] & 0xffff };
        let nsp = sp.wrapping_sub(sz as u32) & if big { 0xffff_ffff } else { 0xffff };
        self.wr_sz(b, SS, nsp, sz, v)?;
        self.set_sp(nsp);
        Ok(())
    }

    pub(crate) fn pop_sz<B: Bus>(&mut self, b: &mut B, sz: u8) -> R<u32> {
        let sp = self.stack_ptr();
        let v = self.rd_sz(b, SS, sp, sz)?;
        self.set_sp(sp.wrapping_add(sz as u32));
        Ok(v)
    }

    /// Read the stack at `depth` bytes above the top, without popping.
    pub(crate) fn peek_sz<B: Bus>(&self, b: &mut B, depth: u32, sz: u8) -> R<u32> {
        let m = if self.seg[SS].big() { 0xffff_ffff } else { 0xffff };
        self.rd_sz(b, SS, self.stack_ptr().wrapping_add(depth) & m, sz)
    }

    pub(crate) fn set_sp(&mut self, v: u32) {
        if self.seg[SS].big() {
            self.regs[ESP] = v;
        } else {
            self.regs[ESP] = (self.regs[ESP] & 0xffff_0000) | (v & 0xffff);
        }
    }

    pub(crate) fn add_sp(&mut self, n: u32) {
        let sp = self.stack_ptr().wrapping_add(n);
        self.set_sp(sp);
    }

    /// Push on another stack (`ss`, `*esp`), for privilege changes.
    fn push_other<B: Bus>(&self, b: &mut B, ss: &Seg, esp: &mut u32, v: u32, sz: u8) -> R<()> {
        let m = if ss.big() { 0xffff_ffff } else { 0xffff };
        let n = esp.wrapping_sub(sz as u32) & m;
        let end = n.wrapping_add(sz as u32 - 1);
        let ok = if ss.attr & DC != 0 {
            n > ss.limit && end <= m && end >= n
        } else {
            end <= ss.limit && end >= n
        };
        if !ok {
            return Err(ss_fault(0));
        }
        let lin = ss.base.wrapping_add(n);
        match sz {
            2 => b.write16(lin, v as u16),
            _ => b.write32(lin, v),
        }
        *esp = n;
        Ok(())
    }

    // ---- Descriptors ----

    /// Read the descriptor for `sel` (GDT or LDT); #GP(sel) when it is
    /// outside the table.
    pub(crate) fn read_desc<B: Bus>(&self, b: &mut B, sel: u16) -> R<Desc> {
        let (base, limit) = if sel & 4 != 0 {
            if self.ldtr.attr & UNUSABLE != 0 || self.ldtr.sel & !3 == 0 {
                return Err(gp(sel as u32 & !3));
            }
            (self.ldtr.base, self.ldtr.limit)
        } else {
            (self.gdtr.base, self.gdtr.limit)
        };
        let off = (sel & !7) as u32;
        if off + 7 > limit {
            return Err(gp(sel as u32 & !3));
        }
        let a = base.wrapping_add(off);
        Ok(Desc { raw: b.read32(a) as u64 | (b.read32(a.wrapping_add(4)) as u64) << 32 })
    }

    /// Set the accessed bit of the descriptor for `sel`.
    fn touch<B: Bus>(&self, b: &mut B, sel: u16, d: &Desc) {
        if d.raw & (1 << 40) == 0 {
            let base = if sel & 4 != 0 { self.ldtr.base } else { self.gdtr.base };
            let a = base.wrapping_add((sel & !7) as u32 + 5);
            let v = b.read8(a);
            b.write8(a, v | 1);
        }
    }

    /// Load a data or stack segment register (MOV, POP, LDS...).
    pub(crate) fn load_sreg<B: Bus>(&mut self, b: &mut B, s: usize, sel: u16) -> R<()> {
        if !self.pm() || self.flags & VM != 0 {
            self.seg[s].sel = sel;
            self.seg[s].base = (sel as u32) << 4;
            if self.flags & VM != 0 {
                self.seg[s].limit = 0xffff;
                self.seg[s].attr = if s == CS { CODE16 } else { DATA16 } | (3 << 5);
            }
            return Ok(());
        }
        let rpl = (sel & 3) as u8;
        if sel & !3 == 0 {
            if s == SS {
                return Err(gp(0));
            }
            self.seg[s] = Seg { sel, base: 0, limit: 0, attr: UNUSABLE };
            return Ok(());
        }
        let d = self.read_desc(b, sel)?;
        let e = sel as u32 & !3;
        if s == SS {
            if !d.is_seg() || d.is_code() || d.attr() & RW == 0 || rpl != self.cpl || d.dpl() != self.cpl {
                return Err(gp(e));
            }
            if !d.present() {
                return Err(ss_fault(e));
            }
        } else {
            if !d.is_seg() || (d.is_code() && d.attr() & RW == 0) {
                return Err(gp(e));
            }
            if (!d.is_code() || !d.conforming()) && d.dpl() < self.cpl.max(rpl) {
                return Err(gp(e));
            }
            if !d.present() {
                return Err(np(e));
            }
        }
        self.touch(b, sel, &d);
        self.seg[s] = d.seg(sel);
        Ok(())
    }

    /// Load CS for a transfer to a code segment at the current or
    /// (conforming) any privilege; the checks of JMP/CALL far.
    fn check_code<B: Bus>(&self, b: &mut B, sel: u16, d: &Desc) -> R<()> {
        let e = sel as u32 & !3;
        let rpl = (sel & 3) as u8;
        if !d.is_code() {
            return Err(gp(e));
        }
        if d.conforming() {
            if d.dpl() > self.cpl {
                return Err(gp(e));
            }
        } else if rpl > self.cpl || d.dpl() != self.cpl {
            return Err(gp(e));
        }
        if !d.present() {
            return Err(np(e));
        }
        let _ = b;
        Ok(())
    }

    fn set_cs(&mut self, sel: u16, d: &Desc, cpl: u8) {
        let mut s = d.seg((sel & !3) | cpl as u16);
        s.attr |= ACCESSED;
        self.seg[CS] = s;
        self.cpl = cpl;
    }

    pub(crate) fn jmp_far<B: Bus>(&mut self, b: &mut B, sel: u16, off: u32) -> R<()> {
        if !self.pm() || self.flags & VM != 0 {
            self.seg[CS].sel = sel;
            self.seg[CS].base = (sel as u32) << 4;
            if self.flags & VM == 0 {
                self.seg[CS].attr = (self.seg[CS].attr & !BIG) | CODE | S | P;
            }
            self.eip = off;
            return Ok(());
        }
        if sel & !3 == 0 {
            return Err(gp(0));
        }
        let d = self.read_desc(b, sel)?;
        let e = sel as u32 & !3;
        if d.is_seg() {
            self.check_code(b, sel, &d)?;
            if off > d.limit() {
                return Err(gp(0));
            }
            self.touch(b, sel, &d);
            let cpl = self.cpl;
            self.set_cs(sel, &d, cpl);
            self.eip = off;
            return Ok(());
        }
        match d.sys_type() {
            4 | 0xc => {
                // Call gate: same privilege only for JMP.
                let rpl = (sel & 3) as u8;
                if d.dpl() < self.cpl || d.dpl() < rpl {
                    return Err(gp(e));
                }
                if !d.present() {
                    return Err(np(e));
                }
                let tsel = d.gate_sel();
                if tsel & !3 == 0 {
                    return Err(gp(0));
                }
                let td = self.read_desc(b, tsel)?;
                let te = tsel as u32 & !3;
                if !td.is_code() || (td.conforming() && td.dpl() > self.cpl) || (!td.conforming() && td.dpl() != self.cpl) {
                    return Err(gp(te));
                }
                if !td.present() {
                    return Err(np(te));
                }
                let toff = d.gate_off();
                if toff > td.limit() {
                    return Err(gp(0));
                }
                self.touch(b, tsel, &td);
                let cpl = self.cpl;
                self.set_cs(tsel, &td, cpl);
                self.eip = toff;
                Ok(())
            }
            // Task switches are not emulated.
            _ => Err(gp(e)),
        }
    }

    pub(crate) fn call_far<B: Bus>(&mut self, b: &mut B, sel: u16, off: u32, sz: u8) -> R<()> {
        if !self.pm() || self.flags & VM != 0 {
            let cs = self.seg[CS].sel as u32;
            let ip = self.eip;
            self.push_sz(b, cs, sz)?;
            self.push_sz(b, ip, sz)?;
            return self.jmp_far(b, sel, off);
        }
        if sel & !3 == 0 {
            return Err(gp(0));
        }
        let d = self.read_desc(b, sel)?;
        let e = sel as u32 & !3;
        if d.is_seg() {
            self.check_code(b, sel, &d)?;
            if off > d.limit() {
                return Err(gp(0));
            }
            let cs = self.seg[CS].sel as u32;
            let ip = self.eip;
            self.push_sz(b, cs, sz)?;
            self.push_sz(b, ip, sz)?;
            self.touch(b, sel, &d);
            let cpl = self.cpl;
            self.set_cs(sel, &d, cpl);
            self.eip = off;
            return Ok(());
        }
        match d.sys_type() {
            4 | 0xc => {
                let rpl = (sel & 3) as u8;
                if d.dpl() < self.cpl || d.dpl() < rpl {
                    return Err(gp(e));
                }
                if !d.present() {
                    return Err(np(e));
                }
                let tsel = d.gate_sel();
                if tsel & !3 == 0 {
                    return Err(gp(0));
                }
                let td = self.read_desc(b, tsel)?;
                let te = tsel as u32 & !3;
                if !td.is_code() || td.dpl() > self.cpl {
                    return Err(gp(te));
                }
                if !td.present() {
                    return Err(np(te));
                }
                let toff = d.gate_off();
                let gsz = if d.gate32() { 4 } else { 2 };
                if toff > td.limit() {
                    return Err(gp(0));
                }
                if !td.conforming() && td.dpl() < self.cpl {
                    // To an inner privilege level: the TSS's stack.
                    let ndpl = td.dpl();
                    let (nss_sel, mut nesp) = self.tss_stack(b, ndpl)?;
                    let nss = self.check_tss_ss(b, nss_sel, ndpl)?;
                    let old_ss = self.seg[SS].sel as u32;
                    let old_esp = self.regs[ESP];
                    let old_sp = self.stack_ptr();
                    let n = d.gate_params();
                    let mut params = [0u32; 32];
                    for i in 0..n {
                        params[i as usize] = self.peek_sz(b, i * gsz as u32, gsz)?;
                    }
                    self.push_other(b, &nss, &mut nesp, old_ss, gsz)?;
                    self.push_other(b, &nss, &mut nesp, if gsz == 4 { old_esp } else { old_sp }, gsz)?;
                    for i in (0..n).rev() {
                        self.push_other(b, &nss, &mut nesp, params[i as usize], gsz)?;
                    }
                    let cs = self.seg[CS].sel as u32;
                    let ip = self.eip;
                    self.push_other(b, &nss, &mut nesp, cs, gsz)?;
                    self.push_other(b, &nss, &mut nesp, ip, gsz)?;
                    self.seg[SS] = nss;
                    self.regs[ESP] = if nss.big() { nesp } else { (self.regs[ESP] & 0xffff_0000) | (nesp & 0xffff) };
                    self.touch(b, tsel, &td);
                    self.set_cs(tsel, &td, ndpl);
                    self.eip = toff;
                } else {
                    let cs = self.seg[CS].sel as u32;
                    let ip = self.eip;
                    self.push_sz(b, cs, gsz)?;
                    self.push_sz(b, ip, gsz)?;
                    self.touch(b, tsel, &td);
                    let cpl = self.cpl;
                    self.set_cs(tsel, &td, cpl);
                    self.eip = toff;
                }
                Ok(())
            }
            _ => Err(gp(e)),
        }
    }

    /// The stack for privilege level `dpl` from the TSS.
    fn tss_stack<B: Bus>(&self, b: &mut B, dpl: u8) -> R<(u16, u32)> {
        let t = &self.tr;
        let ty = (t.attr & 0xf) as u8;
        if t.attr & UNUSABLE != 0 || t.sel & !3 == 0 {
            return Err(ts(0));
        }
        if ty & 8 != 0 {
            let at = 4 + dpl as u32 * 8;
            if at + 5 > t.limit {
                return Err(ts(t.sel as u32 & !3));
            }
            let esp = b.read32(t.base.wrapping_add(at));
            let ss = b.read16(t.base.wrapping_add(at + 4));
            Ok((ss, esp))
        } else {
            let at = 2 + dpl as u32 * 4;
            if at + 3 > t.limit {
                return Err(ts(t.sel as u32 & !3));
            }
            let sp = b.read16(t.base.wrapping_add(at)) as u32;
            let ss = b.read16(t.base.wrapping_add(at + 2));
            Ok((ss, sp))
        }
    }

    fn check_tss_ss<B: Bus>(&self, b: &mut B, sel: u16, dpl: u8) -> R<Seg> {
        let e = sel as u32 & !3;
        if sel & !3 == 0 {
            return Err(ts(0));
        }
        if (sel & 3) as u8 != dpl {
            return Err(ts(e));
        }
        let d = self.read_desc(b, sel).map_err(|_| ts(e))?;
        if !d.is_seg() || d.is_code() || d.attr() & RW == 0 || d.dpl() != dpl {
            return Err(ts(e));
        }
        if !d.present() {
            return Err(ss_fault(e));
        }
        self.touch(b, sel, &d);
        Ok(d.seg(sel))
    }

    /// Far return (RETF imm): `sz` the operand size.
    pub(crate) fn ret_far<B: Bus>(&mut self, b: &mut B, sz: u8, imm: u32) -> R<()> {
        if !self.pm() || self.flags & VM != 0 {
            let ip = self.pop_sz(b, sz)?;
            let cs = self.pop_sz(b, sz)? as u16;
            self.add_sp(imm);
            return self.jmp_far(b, cs, ip);
        }
        let ip = self.peek_sz(b, 0, sz)?;
        let sel = self.peek_sz(b, sz as u32, sz)? as u16;
        let rpl = (sel & 3) as u8;
        if sel & !3 == 0 {
            return Err(gp(0));
        }
        let e = sel as u32 & !3;
        if rpl < self.cpl {
            return Err(gp(e));
        }
        let d = self.read_desc(b, sel)?;
        if !d.is_code() || (d.conforming() && d.dpl() > rpl) || (!d.conforming() && d.dpl() != rpl) {
            return Err(gp(e));
        }
        if !d.present() {
            return Err(np(e));
        }
        if ip > d.limit() {
            return Err(gp(0));
        }
        if rpl == self.cpl {
            self.add_sp(2 * sz as u32 + imm);
            self.touch(b, sel, &d);
            self.set_cs(sel, &d, rpl);
            self.eip = ip;
            return Ok(());
        }
        // To an outer privilege level: the caller's stack follows.
        let at = 2 * sz as u32 + imm;
        let nesp = self.peek_sz(b, at, sz)?;
        let nss_sel = self.peek_sz(b, at + sz as u32, sz)? as u16;
        let ne = nss_sel as u32 & !3;
        if nss_sel & !3 == 0 {
            return Err(gp(0));
        }
        let sd = self.read_desc(b, nss_sel)?;
        if (nss_sel & 3) as u8 != rpl || !sd.is_seg() || sd.is_code() || sd.attr() & RW == 0 || sd.dpl() != rpl {
            return Err(gp(ne));
        }
        if !sd.present() {
            return Err(ss_fault(ne));
        }
        self.touch(b, sel, &d);
        self.touch(b, nss_sel, &sd);
        self.set_cs(sel, &d, rpl);
        self.eip = ip;
        let nss = sd.seg(nss_sel);
        self.seg[SS] = nss;
        if nss.big() {
            self.regs[ESP] = nesp;
        } else {
            self.regs[ESP] = (self.regs[ESP] & 0xffff_0000) | (nesp & 0xffff);
        }
        self.add_sp(imm);
        self.null_outer_segs();
        Ok(())
    }

    /// After a return to an outer level: data segment registers the new
    /// level may not use become null.
    fn null_outer_segs(&mut self) {
        for s in [ES, DS, FS, GS] {
            let sg = self.seg[s];
            if sg.attr & UNUSABLE != 0 {
                continue;
            }
            let conforming_code = sg.attr & CODE != 0 && sg.attr & DC != 0;
            if !conforming_code && sg.dpl() < self.cpl {
                self.seg[s] = Seg { sel: 0, base: 0, limit: 0, attr: UNUSABLE };
            }
        }
    }

    /// Deliver interrupt `vec`: `soft` for INT n/INT3/INTO (gate DPL
    /// checked), `ext` for hardware interrupts and exceptions not caused
    /// by the program (the EXT bit of error codes).
    pub(crate) fn int<B: Bus>(&mut self, b: &mut B, vec: u8, soft: bool, code: Option<u32>, ext: bool) -> R<()> {
        self.halted = false;
        if !self.pm() {
            let at = self.idtr.base.wrapping_add(vec as u32 * 4);
            let off = b.read16(at) as u32;
            let sel = b.read16(at.wrapping_add(2));
            let f = self.flags as u16 as u32;
            let cs = self.seg[CS].sel as u32;
            let ip = self.eip;
            self.push_sz(b, f, 2)?;
            self.push_sz(b, cs, 2)?;
            self.push_sz(b, ip, 2)?;
            self.flags &= !(IF | TF | AC | RF);
            self.seg[CS].sel = sel;
            self.seg[CS].base = (sel as u32) << 4;
            self.eip = off;
            return Ok(());
        }
        let xb = if ext { 1 } else { 0 };
        let ge = vec as u32 * 8 + 2 + xb;
        if vec as u32 * 8 + 7 > self.idtr.limit {
            return Err(gp(ge));
        }
        let at = self.idtr.base.wrapping_add(vec as u32 * 8);
        let g = Desc { raw: b.read32(at) as u64 | (b.read32(at.wrapping_add(4)) as u64) << 32 };
        let ty = g.sys_type();
        if g.is_seg() || !matches!(ty, 6 | 7 | 0xe | 0xf) {
            // Task gates are not emulated either.
            return Err(gp(ge));
        }
        if soft && g.dpl() < self.cpl {
            return Err(gp(ge));
        }
        if !g.present() {
            return Err(np(ge));
        }
        let tsel = g.gate_sel();
        if tsel & !3 == 0 {
            return Err(gp(xb));
        }
        let td = self.read_desc(b, tsel).map_err(|f| Fault { code: f.code.map(|c| c | xb), ..f })?;
        let te = (tsel as u32 & !3) | xb;
        if !td.is_code() || td.dpl() > self.cpl {
            return Err(gp(te));
        }
        if !td.present() {
            return Err(np(te));
        }
        let gsz = if g.gate32() { 4 } else { 2 };
        let toff = g.gate_off();
        if toff > td.limit() {
            return Err(gp(xb));
        }
        let fl = self.flags;
        let cs = self.seg[CS].sel as u32;
        let ip = self.eip;
        let ncpl;
        if !td.conforming() && td.dpl() < self.cpl {
            let ndpl = td.dpl();
            let (nss_sel, mut nesp) = self.tss_stack(b, ndpl)?;
            let nss = self.check_tss_ss(b, nss_sel, ndpl)?;
            let old_ss = self.seg[SS].sel as u32;
            let old_esp = self.regs[ESP];
            self.push_other(b, &nss, &mut nesp, old_ss, gsz)?;
            self.push_other(b, &nss, &mut nesp, old_esp, gsz)?;
            self.push_other(b, &nss, &mut nesp, fl, gsz)?;
            self.push_other(b, &nss, &mut nesp, cs, gsz)?;
            self.push_other(b, &nss, &mut nesp, ip, gsz)?;
            if let Some(c) = code {
                self.push_other(b, &nss, &mut nesp, c, gsz)?;
            }
            self.seg[SS] = nss;
            self.regs[ESP] = if nss.big() { nesp } else { (self.regs[ESP] & 0xffff_0000) | (nesp & 0xffff) };
            ncpl = ndpl;
        } else {
            self.push_sz(b, fl, gsz)?;
            self.push_sz(b, cs, gsz)?;
            self.push_sz(b, ip, gsz)?;
            if let Some(c) = code {
                self.push_sz(b, c, gsz)?;
            }
            ncpl = self.cpl;
        }
        self.touch(b, tsel, &td);
        self.set_cs(tsel, &td, ncpl);
        self.eip = toff;
        self.flags &= !(TF | NT | RF | VM);
        if ty & 1 == 0 {
            self.flags &= !IF;
        }
        Ok(())
    }

    pub(crate) fn iret<B: Bus>(&mut self, b: &mut B, sz: u8) -> R<()> {
        if !self.pm() {
            let ip = self.pop_sz(b, sz)?;
            let cs = self.pop_sz(b, sz)? as u16;
            let f = self.pop_sz(b, sz)?;
            self.seg[CS].sel = cs;
            self.seg[CS].base = (cs as u32) << 4;
            self.eip = ip;
            self.set_flags_real(f, sz);
            return Ok(());
        }
        if self.flags & NT != 0 {
            // Task return: not emulated.
            return Err(ts(self.tr.sel as u32 & !3));
        }
        let ip = self.peek_sz(b, 0, sz)?;
        let sel = self.peek_sz(b, sz as u32, sz)? as u16;
        let f = self.peek_sz(b, 2 * sz as u32, sz)?;
        let rpl = (sel & 3) as u8;
        if sel & !3 == 0 {
            return Err(gp(0));
        }
        let e = sel as u32 & !3;
        if rpl < self.cpl {
            return Err(gp(e));
        }
        let d = self.read_desc(b, sel)?;
        if !d.is_code() || (d.conforming() && d.dpl() > rpl) || (!d.conforming() && d.dpl() != rpl) {
            return Err(gp(e));
        }
        if !d.present() {
            return Err(np(e));
        }
        if ip > d.limit() {
            return Err(gp(0));
        }
        let old_cpl = self.cpl;
        if rpl == self.cpl {
            self.add_sp(3 * sz as u32);
            self.touch(b, sel, &d);
            self.set_cs(sel, &d, rpl);
            self.eip = ip;
            self.set_flags_pm(f, sz, old_cpl);
            return Ok(());
        }
        let nesp = self.peek_sz(b, 3 * sz as u32, sz)?;
        let nss_sel = self.peek_sz(b, 4 * sz as u32, sz)? as u16;
        let ne = nss_sel as u32 & !3;
        if nss_sel & !3 == 0 {
            return Err(gp(0));
        }
        let sd = self.read_desc(b, nss_sel)?;
        if (nss_sel & 3) as u8 != rpl || !sd.is_seg() || sd.is_code() || sd.attr() & RW == 0 || sd.dpl() != rpl {
            return Err(gp(ne));
        }
        if !sd.present() {
            return Err(ss_fault(ne));
        }
        self.touch(b, sel, &d);
        self.touch(b, nss_sel, &sd);
        self.set_cs(sel, &d, rpl);
        self.eip = ip;
        self.set_flags_pm(f, sz, old_cpl);
        let nss = sd.seg(nss_sel);
        self.seg[SS] = nss;
        if nss.big() {
            self.regs[ESP] = nesp;
        } else {
            self.regs[ESP] = (self.regs[ESP] & 0xffff_0000) | (nesp & 0xffff);
        }
        self.null_outer_segs();
        Ok(())
    }

    /// POPF/IRET in real mode.
    pub(crate) fn set_flags_real(&mut self, f: u32, sz: u8) {
        let m = if sz == 2 { 0xffff } else { ARITH | TF | IF | DF | IOPL | NT | AC | 0x0002 | RF };
        let m = m & !(0x8000 | 0x0028) & !(VM);
        self.flags = (self.flags & !m) | (f & m) | 0x0002;
    }

    /// POPF/IRET in protected mode at privilege `cpl`: IOPL only at 0,
    /// IF only when CPL <= IOPL.
    pub(crate) fn set_flags_pm(&mut self, f: u32, sz: u8, cpl: u8) {
        let iopl = ((self.flags & IOPL) >> 12) as u8;
        let mut m = ARITH | TF | DF | NT;
        if sz == 4 {
            m |= AC | RF;
        }
        if cpl == 0 {
            m |= IOPL;
        }
        if cpl <= iopl {
            m |= IF;
        }
        self.flags = (self.flags & !m) | (f & m) | 0x0002;
    }

    /// LLDT.
    pub(crate) fn load_ldt<B: Bus>(&mut self, b: &mut B, sel: u16) -> R<()> {
        if sel & !3 == 0 {
            self.ldtr = Seg { sel, base: 0, limit: 0, attr: UNUSABLE };
            return Ok(());
        }
        if sel & 4 != 0 {
            return Err(gp(sel as u32 & !3));
        }
        let d = self.read_desc(b, sel)?;
        if d.is_seg() || d.sys_type() != 2 {
            return Err(gp(sel as u32 & !3));
        }
        if !d.present() {
            return Err(np(sel as u32 & !3));
        }
        self.ldtr = d.seg(sel);
        Ok(())
    }

    /// LTR: marks the TSS busy.
    pub(crate) fn load_tr<B: Bus>(&mut self, b: &mut B, sel: u16) -> R<()> {
        if sel & !3 == 0 || sel & 4 != 0 {
            return Err(gp(sel as u32 & !3));
        }
        let d = self.read_desc(b, sel)?;
        if d.is_seg() || !matches!(d.sys_type(), 1 | 9) {
            return Err(gp(sel as u32 & !3));
        }
        if !d.present() {
            return Err(np(sel as u32 & !3));
        }
        let a = self.gdtr.base.wrapping_add((sel & !7) as u32 + 5);
        let v = b.read8(a);
        b.write8(a, v | 2);
        let mut s = d.seg(sel);
        s.attr |= 2;
        self.tr = s;
        Ok(())
    }
}
