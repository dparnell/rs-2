//! Memory control blocks: 48h-4Ah, 58h, and the UMB link.

use super::{err, FIRST_MCB};
use crate::{Host, Machine};

impl Machine {
    pub fn mcb_write(&mut self, seg: u16, kind: u8, owner: u16, size: u16) {
        let a = Machine::lin(seg, 0);
        self.hw.wr(a, kind);
        self.hw.wr16(a + 1, owner);
        self.hw.wr16(a + 3, size);
    }

    pub fn mcb_name(&mut self, seg: u16, name: &[u8]) {
        let a = Machine::lin(seg, 8);
        for i in 0..8 {
            self.hw.wr(a + i, name.get(i as usize).copied().unwrap_or(0));
        }
    }

    /// (type, owner, size) of the MCB at `seg`.
    pub fn mcb(&mut self, seg: u16) -> (u8, u16, u16) {
        let a = Machine::lin(seg, 0);
        (self.hw.rd(a), self.hw.rd16(a + 1), self.hw.rd16(a + 3))
    }

    /// Link or unlink the upper memory chain (5803h).
    pub fn set_umb_link(&mut self, on: bool) {
        if self.dos.umb_start == 0xffff {
            return;
        }
        // The last conventional block before the hole MCB at 9FFFh.
        let mut seg = FIRST_MCB;
        loop {
            let (k, _, size) = self.mcb(seg);
            let next = seg + size + 1;
            if next == 0x9fff || k == b'Z' && next >= 0x9fff {
                self.hw.wr(Machine::lin(seg, 0), if on { b'M' } else { b'Z' });
                break;
            }
            if k != b'M' || next >= 0xa000 {
                break;
            }
            seg = next;
        }
        self.dos.umb_link = on;
    }

    /// Merge free blocks following `seg` into it.
    fn coalesce(&mut self, seg: u16) {
        loop {
            let (k, owner, size) = self.mcb(seg);
            if k == b'Z' || owner != 0 {
                return;
            }
            let next = seg + size + 1;
            let (nk, nowner, nsize) = self.mcb(next);
            if nowner != 0 || (nk != b'M' && nk != b'Z') {
                return;
            }
            self.mcb_write(seg, nk, 0, size + nsize + 1);
        }
    }

    /// The blocks of the chain from `start` (stopping at 'Z').
    fn chain(&mut self, start: u16) -> alloc::vec::Vec<(u16, u8, u16, u16)> {
        let mut v = alloc::vec::Vec::new();
        let mut seg = start;
        for _ in 0..4096 {
            let (k, owner, size) = self.mcb(seg);
            if k != b'M' && k != b'Z' {
                break;
            }
            v.push((seg, k, owner, size));
            if k == b'Z' {
                break;
            }
            seg = seg.wrapping_add(size).wrapping_add(1);
        }
        v
    }

    /// Allocate `paras` for `owner`; the segment (past the MCB). On
    /// failure, the largest free block in the error.
    pub fn alloc_mem<H: Host>(&mut self, host: &mut H, paras: u16, owner: u16) -> Result<u16, u16> {
        let _ = host;
        self.alloc_strategy(paras, owner, self.dos.strategy).map_err(|_| err::NO_MEMORY)
    }

    pub(crate) fn alloc_strategy(&mut self, paras: u16, owner: u16, strategy: u16) -> Result<u16, u16> {
        // Coalesce free blocks first.
        let blocks = self.chain(FIRST_MCB);
        for &(seg, ..) in &blocks {
            self.coalesce(seg);
        }
        let high = strategy & 0xc0;
        let fit = strategy & 3;
        let umb = self.dos.umb_start;
        let all = self.chain(FIRST_MCB);
        let upper = |s: u16| s >= umb && umb != 0xffff;
        let pick = |filter: &dyn Fn(u16) -> bool| -> Option<(u16, u16)> {
            let cands = all.iter().filter(|b| b.2 == 0 && b.3 >= paras && filter(b.0));
            match fit {
                1 => cands.min_by_key(|b| b.3).map(|b| (b.0, b.3)),
                2 => cands.last().map(|b| (b.0, b.3)),
                _ => cands.into_iter().next().map(|b| (b.0, b.3)),
            }
        };
        let found = if high != 0 && self.dos.umb_link {
            pick(&|s| upper(s)).or_else(|| if high == 0x80 { None } else { pick(&|s| !upper(s)) })
        } else {
            pick(&|s| !upper(s) || self.dos.umb_link)
        };
        let Some((seg, size)) = found else {
            let largest = all.iter().filter(|b| b.2 == 0).map(|b| b.3).max().unwrap_or(0);
            return Err(largest);
        };
        let (k, _, _) = self.mcb(seg);
        if fit == 2 && size > paras {
            // Last fit: the top of the block.
            let rest = size - paras - 1;
            self.mcb_write(seg, b'M', 0, rest);
            let nseg = seg + rest + 1;
            self.mcb_write(nseg, k, owner, paras);
            self.mcb_name(nseg, b"");
            return Ok(nseg + 1);
        }
        if size > paras {
            self.mcb_write(seg, b'M', owner, paras);
            self.mcb_write(seg + paras + 1, k, 0, size - paras - 1);
        } else {
            self.mcb_write(seg, k, owner, size);
        }
        self.mcb_name(seg, b"");
        Ok(seg + 1)
    }

    pub fn free_mem(&mut self, seg: u16) -> Result<(), u16> {
        let m = seg.wrapping_sub(1);
        let (k, _, _) = self.mcb(m);
        if k != b'M' && k != b'Z' {
            return Err(err::INVALID_BLOCK);
        }
        self.hw.wr16(Machine::lin(m, 1), 0);
        Ok(())
    }

    /// Resize the block at `seg`; Err((code, largest possible)).
    pub fn resize_mem(&mut self, seg: u16, paras: u16) -> Result<(), (u16, u16)> {
        let m = seg.wrapping_sub(1);
        let (k, owner, size) = self.mcb(m);
        if k != b'M' && k != b'Z' {
            return Err((err::MCB_DESTROYED, 0));
        }
        if paras <= size {
            if paras < size {
                self.mcb_write(m, b'M', owner, paras);
                self.mcb_write(m + paras + 1, k, 0, size - paras - 1);
                self.coalesce(m + paras + 1);
            }
            return Ok(());
        }
        // Grow into the free blocks after it.
        let mut avail = size;
        let mut last_kind = k;
        let mut next = m + size + 1;
        if k == b'M' {
            loop {
                let (nk, nowner, nsize) = self.mcb(next);
                if nowner != 0 || (nk != b'M' && nk != b'Z') {
                    break;
                }
                avail += nsize + 1;
                last_kind = nk;
                if nk == b'Z' {
                    break;
                }
                next = next + nsize + 1;
            }
        }
        if avail < paras {
            return Err((err::NO_MEMORY, avail));
        }
        if avail > paras {
            self.mcb_write(m, b'M', owner, paras);
            self.mcb_write(m + paras + 1, last_kind, 0, avail - paras - 1);
        } else {
            self.mcb_write(m, last_kind, owner, avail);
        }
        Ok(())
    }

    /// Free every block owned by `psp`.
    pub fn free_owned(&mut self, psp: u16) {
        for (seg, _, owner, _) in self.chain(FIRST_MCB) {
            if owner == psp {
                self.hw.wr16(Machine::lin(seg, 1), 0);
            }
        }
    }

    /// The largest free block (paragraphs).
    pub fn largest_free(&mut self) -> u16 {
        let blocks = self.chain(FIRST_MCB);
        for &(seg, ..) in &blocks {
            self.coalesce(seg);
        }
        let umb = self.dos.umb_start;
        let link = self.dos.umb_link;
        self.chain(FIRST_MCB).iter().filter(|b| b.2 == 0 && (link || b.0 < umb)).map(|b| b.3).max().unwrap_or(0)
    }

    pub(crate) fn mem_fn<H: Host>(&mut self, host: &mut H, ah: u8) {
        match ah {
            0x48 => {
                let n = self.cpu.bx();
                let psp = self.dos.psp;
                match self.alloc_strategy(n, psp, self.dos.strategy) {
                    Ok(seg) => {
                        self.cpu.set_ax(seg);
                        self.ok();
                    }
                    Err(largest) => {
                        self.cpu.set_bx(largest);
                        self.fail(err::NO_MEMORY);
                    }
                }
            }
            0x49 => {
                let es = self.cpu.seg[x86emu::ES].sel;
                match self.free_mem(es) {
                    Ok(()) => self.ok(),
                    Err(e) => self.fail(e),
                }
            }
            0x4a => {
                let (es, bx) = (self.cpu.seg[x86emu::ES].sel, self.cpu.bx());
                match self.resize_mem(es, bx) {
                    Ok(()) => {
                        // A program's block: its PSP's memory top follows.
                        if es == self.dos.psp {
                            self.hw.wr16(Machine::lin(es, 2), es + bx);
                        }
                        self.ok();
                    }
                    Err((e, max)) => {
                        self.cpu.set_bx(max);
                        self.fail(e);
                    }
                }
            }
            0x58 => match self.cpu.al() {
                0 => {
                    let s = self.dos.strategy;
                    self.cpu.set_ax(s);
                    self.ok();
                }
                1 => {
                    self.dos.strategy = self.cpu.bx() & 0xc3;
                    self.ok();
                }
                2 => {
                    let l = self.dos.umb_link as u8;
                    self.cpu.set_al(l);
                    self.ok();
                }
                3 => {
                    if self.dos.umb_start == 0xffff {
                        self.fail(err::INVALID_FUNCTION);
                    } else {
                        let on = self.cpu.bx() & 1 != 0;
                        self.set_umb_link(on);
                        self.ok();
                    }
                }
                _ => self.fail(err::INVALID_FUNCTION),
            },
            _ => {}
        }
        let _ = host;
    }
}
