//! Extended memory: the allocator shared by XMS and DPMI, and the XMS
//! 3.0 driver (INT 2Fh AX=4310h entry point).

use alloc::vec::Vec;

use x86emu::DS;

use crate::hw::EXT_BASE;
use crate::Machine;

pub const CB_XMS: u16 = 0x101;

/// Who a block of extended memory belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// XMS handle (by the PSP that allocated it).
    Xms(u16),
    /// DPMI memory block of a client (by PSP).
    Dpmi(u16),
}

#[derive(Clone, Debug)]
pub struct Block {
    pub start: u32,
    pub size: u32,
    pub owner: Owner,
    pub locks: u8,
}

/// Extended memory above 1 MB + 64 KB, handed out in 1 KB units.
#[derive(Default)]
pub struct ExtMem {
    pub blocks: Vec<Block>,
}

impl ExtMem {
    /// Bytes in use by blocks matching `f`.
    pub fn used(&self, f: impl Fn(&Owner) -> bool) -> u32 {
        self.blocks.iter().filter(|b| f(&b.owner)).map(|b| b.size).sum()
    }

    /// The lowest free gap of `size` bytes below `limit` (an address).
    pub fn find(&self, size: u32, limit: u32) -> Option<u32> {
        let mut at = EXT_BASE;
        let mut sorted: Vec<&Block> = self.blocks.iter().collect();
        sorted.sort_by_key(|b| b.start);
        for b in sorted {
            if b.start >= at + size {
                break;
            }
            at = at.max(b.start + b.size);
        }
        if at.checked_add(size)? <= limit { Some(at) } else { None }
    }

    pub fn largest_gap(&self, limit: u32) -> u32 {
        let mut at = EXT_BASE;
        let mut best = 0;
        let mut sorted: Vec<&Block> = self.blocks.iter().collect();
        sorted.sort_by_key(|b| b.start);
        for b in sorted {
            best = best.max(b.start.saturating_sub(at));
            at = at.max(b.start + b.size);
        }
        best.max(limit.saturating_sub(at))
    }
}

#[derive(Default)]
pub struct Xms {
    /// Handles: index + 1 is the handle; the block's start address.
    pub handles: Vec<Option<u32>>,
    pub ext: ExtMem,
    pub hma_used: bool,
    pub a20_local: u32,
}

const XMS_HANDLES: usize = 64;

impl Machine {
    pub(crate) fn xms_init(&mut self) {
        self.xms.hma_used = self.cfg.high;
    }

    /// The top of the extended memory pool (an address).
    pub fn ext_limit(&self) -> u32 {
        EXT_BASE + (self.cfg.xms_kb + self.cfg.dpmi_kb) * 1024
    }

    fn xms_block(&self, h: u16) -> Option<usize> {
        let start = (*self.xms.handles.get((h as usize).wrapping_sub(1))?)?;
        self.xms.ext.blocks.iter().position(|b| b.start == start)
    }

    fn xms_alloc(&mut self, kb: u32) -> Result<u16, u8> {
        let used = self.xms.ext.used(|o| matches!(o, Owner::Xms(_)));
        let bytes = kb.max(1) * 1024;
        if used + bytes > self.cfg.xms_kb * 1024 {
            return Err(0xa0);
        }
        let slot = match self.xms.handles.iter().position(|h| h.is_none()) {
            Some(i) => i,
            None if self.xms.handles.len() < XMS_HANDLES => {
                self.xms.handles.push(None);
                self.xms.handles.len() - 1
            }
            None => return Err(0xa1),
        };
        let start = self.xms.ext.find(bytes, self.ext_limit()).ok_or(0xa0u8)?;
        self.hw.grow_ext(start + bytes);
        let psp = self.dos.psp;
        self.xms.ext.blocks.push(Block { start, size: if kb == 0 { 0 } else { bytes }, owner: Owner::Xms(psp), locks: 0 });
        self.xms.handles[slot] = Some(start);
        Ok(slot as u16 + 1)
    }

    /// Linear address of an XMS move operand.
    fn xms_addr(&mut self, h: u16, off: u32) -> Option<u32> {
        if h == 0 {
            // Real-mode seg:off.
            return Some(Machine::lin((off >> 16) as u16, off as u16));
        }
        let i = self.xms_block(h)?;
        let b = &self.xms.ext.blocks[i];
        if off > b.size {
            return None;
        }
        Some(b.start + off)
    }

    fn xms_free_kb(&self) -> (u32, u32) {
        let used = self.xms.ext.used(|o| matches!(o, Owner::Xms(_)));
        let free = (self.cfg.xms_kb * 1024).saturating_sub(used);
        let gap = self.xms.ext.largest_gap(self.ext_limit());
        (free.min(gap) / 1024, free / 1024)
    }

    pub(crate) fn xms_call(&mut self) {
        let ah = self.cpu.ah();
        let ok = |m: &mut Machine| {
            m.cpu.set_ax(1);
            m.cpu.set_bl(0);
        };
        let fail = |m: &mut Machine, e: u8| {
            m.cpu.set_ax(0);
            m.cpu.set_bl(e);
        };
        match ah {
            0x00 => {
                self.cpu.set_ax(0x0300);
                self.cpu.set_bx(0x0350);
                self.cpu.set_dx(1);
            }
            0x01 => {
                if self.xms.hma_used {
                    fail(self, 0x91);
                } else {
                    self.xms.hma_used = true;
                    ok(self);
                }
            }
            0x02 => {
                if !self.cfg.high && self.xms.hma_used {
                    self.xms.hma_used = false;
                    ok(self);
                } else {
                    fail(self, 0x93);
                }
            }
            0x03 | 0x05 => {
                self.hw.a20 = true;
                if ah == 5 {
                    self.xms.a20_local += 1;
                }
                ok(self);
            }
            0x04 | 0x06 => {
                if ah == 6 && self.xms.a20_local > 0 {
                    self.xms.a20_local -= 1;
                }
                if self.xms.a20_local == 0 && !self.cfg.high {
                    self.hw.a20 = false;
                }
                ok(self);
            }
            0x07 => {
                let a = self.hw.a20 as u16;
                self.cpu.set_ax(a);
                self.cpu.set_bl(0);
            }
            0x08 => {
                let (largest, total) = self.xms_free_kb();
                self.cpu.set_ax(largest.min(0xffff) as u16);
                self.cpu.set_dx(total.min(0xffff) as u16);
                self.cpu.set_bl(if total == 0 { 0xa0 } else { 0 });
            }
            0x88 => {
                let (largest, total) = self.xms_free_kb();
                self.cpu.regs[x86emu::EAX] = largest;
                self.cpu.regs[x86emu::EDX] = total;
                self.cpu.regs[x86emu::ECX] = self.ext_limit() - 1;
                self.cpu.set_bl(0);
            }
            0x09 | 0x89 => {
                let kb = if ah == 0x89 { self.cpu.regs[x86emu::EDX] } else { self.cpu.dx() as u32 };
                match self.xms_alloc(kb) {
                    Ok(h) => {
                        self.cpu.set_dx(h);
                        ok(self);
                    }
                    Err(e) => fail(self, e),
                }
            }
            0x0a => {
                let h = self.cpu.dx();
                match self.xms_block(h) {
                    Some(i) if self.xms.ext.blocks[i].locks == 0 => {
                        self.xms.ext.blocks.remove(i);
                        self.xms.handles[h as usize - 1] = None;
                        ok(self);
                    }
                    Some(_) => fail(self, 0xab),
                    None => fail(self, 0xa2),
                }
            }
            0x0b => {
                let a = Machine::lin(self.cpu.seg[DS].sel, self.cpu.si());
                let len = self.hw.rd32(a);
                let sh = self.hw.rd16(a + 4);
                let so = self.hw.rd32(a + 6);
                let dh = self.hw.rd16(a + 10);
                let doff = self.hw.rd32(a + 12);
                let (Some(src), Some(dst)) = (self.xms_addr(sh, so), self.xms_addr(dh, doff)) else {
                    return fail(self, 0xa3);
                };
                if len & 1 != 0 {
                    return fail(self, 0xa7);
                }
                let a20 = self.hw.a20;
                self.hw.a20 = true;
                if dst >= EXT_BASE {
                    self.hw.grow_ext(dst + len);
                }
                let mut buf = alloc::vec![0u8; len as usize];
                self.hw.read_bytes(src, &mut buf);
                self.hw.write_bytes(dst, &buf);
                self.hw.a20 = a20;
                ok(self);
            }
            0x0c => {
                let h = self.cpu.dx();
                match self.xms_block(h) {
                    Some(i) => {
                        self.xms.ext.blocks[i].locks = self.xms.ext.blocks[i].locks.saturating_add(1);
                        let s = self.xms.ext.blocks[i].start;
                        self.cpu.set_dx((s >> 16) as u16);
                        self.cpu.set_bx(s as u16);
                        self.cpu.set_ax(1);
                    }
                    None => fail(self, 0xa2),
                }
            }
            0x0d => {
                let h = self.cpu.dx();
                match self.xms_block(h) {
                    Some(i) if self.xms.ext.blocks[i].locks > 0 => {
                        self.xms.ext.blocks[i].locks -= 1;
                        ok(self);
                    }
                    Some(_) => fail(self, 0xaa),
                    None => fail(self, 0xa2),
                }
            }
            0x0e | 0x8e => {
                let h = self.cpu.dx();
                match self.xms_block(h) {
                    Some(i) => {
                        let b = self.xms.ext.blocks[i].clone();
                        let free = self.xms.handles.iter().filter(|h| h.is_none()).count() + XMS_HANDLES - self.xms.handles.len();
                        self.cpu.set_bh(b.locks);
                        if ah == 0x8e {
                            self.cpu.set_cx(free as u16);
                            self.cpu.regs[x86emu::EDX] = b.size / 1024;
                        } else {
                            self.cpu.set_bl(free.min(255) as u8);
                            self.cpu.set_dx((b.size / 1024) as u16);
                        }
                        self.cpu.set_ax(1);
                    }
                    None => fail(self, 0xa2),
                }
            }
            0x0f | 0x8f => {
                let h = self.cpu.dx();
                let kb = if ah == 0x8f { self.cpu.regs[x86emu::EBX] } else { self.cpu.bx() as u32 };
                let Some(i) = self.xms_block(h) else { return fail(self, 0xa2) };
                if self.xms.ext.blocks[i].locks > 0 {
                    return fail(self, 0xab);
                }
                let old = self.xms.ext.blocks.remove(i);
                let bytes = kb * 1024;
                let used = self.xms.ext.used(|o| matches!(o, Owner::Xms(_)));
                let start = if used + bytes > self.cfg.xms_kb * 1024 { None } else { self.xms.ext.find(bytes, self.ext_limit()) };
                let Some(start) = start else {
                    self.xms.ext.blocks.push(old);
                    return fail(self, 0xa0);
                };
                self.hw.grow_ext(start + bytes);
                if start != old.start {
                    let n = old.size.min(bytes);
                    let mut buf = alloc::vec![0u8; n as usize];
                    self.hw.read_bytes(old.start, &mut buf);
                    self.hw.write_bytes(start, &buf);
                }
                self.xms.ext.blocks.push(Block { start, size: bytes, ..old });
                self.xms.handles[h as usize - 1] = Some(start);
                ok(self);
            }
            0x10 => {
                // UMBs belong to DOS (DOS=UMB).
                self.cpu.set_dx(0);
                fail(self, 0xb1);
            }
            0x11 | 0x12 => fail(self, 0xb2),
            _ => fail(self, 0x80),
        }
    }

    pub(crate) fn xms_process_ended(&mut self, psp: u16) {
        // XMS blocks outlive programs on DOS too; free those of the
        // session's first program only when it ends (the session ends).
        let _ = psp;
    }
}
