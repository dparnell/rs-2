//! Expanded memory (LIM EMS 4.0, INT 67h) with a 64 KB page frame at
//! E000h. The device driver header EMMXXXX0 that programs look for sits
//! at the start of INT 67h's segment.

use alloc::string::String;
use alloc::vec::Vec;

use x86emu::{DS, ES};

use crate::hw::{EMS_FRAME, EMS_PAGE};
use crate::Machine;

/// Segment of the INT 67h handler (its device header at offset 0).
pub const EMS_SEG: u16 = 0xf100;

#[derive(Default)]
pub struct Ems {
    /// Handles: the global pages they own (None = free handle).
    pub handles: Vec<Option<Vec<u16>>>,
    pub names: Vec<[u8; 8]>,
    /// Global pages free.
    pub free: Vec<u16>,
    pub total: u16,
    /// Saved mappings (47h/48h) by handle.
    pub saved: Vec<Option<[Option<u16>; 4]>>,
}

impl Machine {
    pub(crate) fn ems_init(&mut self) {
        if self.cfg.ems_kb == 0 {
            return;
        }
        let pages = (self.cfg.ems_kb / 16).min(2048) as u16;
        self.ems.total = pages;
        self.ems.free = (0..pages).rev().collect();
        // Handle 0: the operating system's (no pages).
        self.ems.handles.push(Some(Vec::new()));
        self.ems.names.push([0; 8]);
        self.ems.saved.push(None);
        let a = Machine::lin(EMS_SEG, 0) as usize;
        let mut h = [0u8; 18];
        h[0..4].copy_from_slice(&[0xff; 4]);
        h[4..6].copy_from_slice(&0xc000u16.to_le_bytes());
        h[10..18].copy_from_slice(b"EMMXXXX0");
        self.hw.ram[a..a + 18].copy_from_slice(&h);
        self.hw.ram[a + 0x12..a + 0x17].copy_from_slice(&[0xfe, 0x38, 0x67, 0x00, 0xcf]);
        self.set_vector(0x67, EMS_SEG, 0x12);
    }

    /// The global page of logical page `l` of handle `h`.
    fn ems_page(&self, h: u16, l: u16) -> Option<u16> {
        self.ems.handles.get(h as usize)?.as_ref()?.get(l as usize).copied()
    }

    fn ems_grow(&mut self) {
        let need = self.ems.total as usize * EMS_PAGE;
        if self.hw.ems.len() < need {
            self.hw.ems.resize(need, 0);
        }
    }

    fn ems_map(&mut self, phys: u8, h: u16, l: u16) -> u8 {
        if phys > 3 {
            return 0x8b;
        }
        if self.ems.handles.get(h as usize).is_none_or(|x| x.is_none()) {
            return 0x83;
        }
        if l == 0xffff {
            self.hw.ems_map[phys as usize] = None;
            return 0;
        }
        match self.ems_page(h, l) {
            Some(g) => {
                self.ems_grow();
                self.hw.ems_map[phys as usize] = Some(g);
                self.hw.vga.dirty = true;
                0
            }
            None => 0x8a,
        }
    }

    pub(crate) fn int67(&mut self) {
        if self.cfg.ems_kb == 0 {
            self.cpu.set_ah(0x84);
            return;
        }
        let ah = self.cpu.ah();
        let mut st = 0u8;
        match ah {
            0x40 => {}
            0x41 => self.cpu.set_bx(EMS_FRAME as u16),
            0x42 => {
                let f = self.ems.free.len() as u16;
                self.cpu.set_bx(f);
                self.cpu.set_dx(self.ems.total);
            }
            0x43 | 0x5a => {
                let n = self.cpu.bx();
                if n == 0 && ah == 0x43 {
                    st = 0x89;
                } else if n as usize > self.ems.free.len() {
                    st = if n > self.ems.total { 0x87 } else { 0x88 };
                } else {
                    let pages: Vec<u16> = (0..n).map(|_| self.ems.free.pop().unwrap()).collect();
                    match self.ems.handles.iter().position(|h| h.is_none()) {
                        Some(i) => {
                            self.ems.handles[i] = Some(pages);
                            self.ems.names[i] = [0; 8];
                            self.ems.saved[i] = None;
                            self.cpu.set_dx(i as u16);
                        }
                        None if self.ems.handles.len() < 255 => {
                            self.ems.handles.push(Some(pages));
                            self.ems.names.push([0; 8]);
                            self.ems.saved.push(None);
                            self.cpu.set_dx(self.ems.handles.len() as u16 - 1);
                        }
                        None => {
                            self.ems.free.extend(pages);
                            st = 0x85;
                        }
                    }
                }
            }
            0x44 => {
                let (al, bx, dx) = (self.cpu.al(), self.cpu.bx(), self.cpu.dx());
                st = self.ems_map(al, dx, bx);
            }
            0x45 => {
                let h = self.cpu.dx() as usize;
                match self.ems.handles.get_mut(h).and_then(|x| x.take()) {
                    Some(pages) => {
                        for w in self.hw.ems_map.iter_mut() {
                            if w.is_some_and(|g| pages.contains(&g)) {
                                *w = None;
                            }
                        }
                        self.ems.free.extend(pages);
                        if h == 0 {
                            self.ems.handles[0] = Some(Vec::new());
                        }
                    }
                    None => st = 0x83,
                }
            }
            0x46 => self.cpu.set_al(0x40),
            0x47 => {
                let h = self.cpu.dx() as usize;
                match self.ems.saved.get_mut(h) {
                    Some(s) if s.is_none() => *s = Some(self.hw.ems_map),
                    Some(_) => st = 0x8d,
                    None => st = 0x83,
                }
            }
            0x48 => {
                let h = self.cpu.dx() as usize;
                match self.ems.saved.get_mut(h).and_then(|s| s.take()) {
                    Some(m) => self.hw.ems_map = m,
                    None => st = 0x8e,
                }
            }
            0x4b => {
                let n = self.ems.handles.iter().filter(|h| h.is_some()).count();
                self.cpu.set_bx(n as u16);
            }
            0x4c => match self.ems.handles.get(self.cpu.dx() as usize).and_then(|h| h.as_ref()) {
                Some(p) => {
                    let n = p.len() as u16;
                    self.cpu.set_bx(n);
                }
                None => st = 0x83,
            },
            0x4d => {
                let a = self.seg_lin(ES, self.cpu.di());
                let mut n = 0;
                for (i, h) in self.ems.handles.iter().enumerate() {
                    if let Some(p) = h {
                        self.hw.wr16(a + n * 4, i as u16);
                        self.hw.wr16(a + n * 4 + 2, p.len() as u16);
                        n += 1;
                    }
                }
                self.cpu.set_bx(n as u16);
            }
            0x4e => {
                // Get/set the whole mapping: 16 bytes.
                let al = self.cpu.al();
                if al == 0 || al == 2 {
                    let a = self.seg_lin(ES, self.cpu.di());
                    for (w, m) in self.hw.ems_map.clone().iter().enumerate() {
                        self.hw.wr32(a + w as u32 * 4, m.map_or(0xffff_ffff, |g| g as u32));
                    }
                }
                if al == 1 || al == 2 {
                    let a = self.seg_lin(DS, self.cpu.si());
                    for w in 0..4 {
                        let v = self.hw.rd32(a + w * 4);
                        self.hw.ems_map[w as usize] = if v == 0xffff_ffff || v >= self.ems.total as u32 { None } else { Some(v as u16) };
                    }
                    self.ems_grow();
                }
                if al == 3 {
                    self.cpu.set_al(16);
                }
            }
            0x50 => {
                let al = self.cpu.al();
                let a = self.seg_lin(DS, self.cpu.si());
                let h = self.cpu.dx();
                for i in 0..self.cpu.cx() as u32 {
                    let l = self.hw.rd16(a + i * 4);
                    let p = self.hw.rd16(a + i * 4 + 2);
                    let phys = if al == 1 { (p.wrapping_sub(EMS_FRAME as u16) / 0x400) as u8 } else { p as u8 };
                    st = self.ems_map(phys, h, l);
                    if st != 0 {
                        break;
                    }
                }
            }
            0x51 => {
                let h = self.cpu.dx() as usize;
                let n = self.cpu.bx() as usize;
                match self.ems.handles.get_mut(h).and_then(|x| x.as_mut()) {
                    Some(p) => {
                        while p.len() > n {
                            let g = p.pop().unwrap();
                            self.ems.free.push(g);
                        }
                        while p.len() < n {
                            match self.ems.free.pop() {
                                Some(g) => p.push(g),
                                None => {
                                    st = 0x88;
                                    break;
                                }
                            }
                        }
                        let len = p.len() as u16;
                        self.cpu.set_bx(len);
                    }
                    None => st = 0x83,
                }
            }
            0x52 => self.cpu.set_al(0),
            0x53 => {
                let h = self.cpu.dx() as usize;
                if h >= self.ems.names.len() || self.ems.handles[h].is_none() {
                    st = 0x83;
                } else if self.cpu.al() == 0 {
                    let a = self.seg_lin(ES, self.cpu.di());
                    let n = self.ems.names[h];
                    self.hw.write_bytes(a, &n);
                } else {
                    let a = self.seg_lin(DS, self.cpu.si());
                    let mut n = [0u8; 8];
                    self.hw.read_bytes(a, &mut n);
                    self.ems.names[h] = n;
                }
            }
            0x54 => match self.cpu.al() {
                0 => {
                    let a = self.seg_lin(ES, self.cpu.di());
                    let mut n = 0u32;
                    for i in 0..self.ems.handles.len() {
                        if self.ems.handles[i].is_some() {
                            self.hw.wr16(a + n * 10, i as u16);
                            let nm = self.ems.names[i];
                            self.hw.write_bytes(a + n * 10 + 2, &nm);
                            n += 1;
                        }
                    }
                    self.cpu.set_al(n as u8);
                }
                1 => {
                    let a = self.seg_lin(DS, self.cpu.si());
                    let mut want = [0u8; 8];
                    self.hw.read_bytes(a, &mut want);
                    match self.ems.names.iter().position(|n| *n == want && want != [0; 8]) {
                        Some(i) => self.cpu.set_dx(i as u16),
                        None => st = 0xa0,
                    }
                }
                _ => self.cpu.set_bx(255),
            },
            0x57 => st = self.ems_move(),
            0x58 => {
                if self.cpu.al() == 0 {
                    let a = self.seg_lin(ES, self.cpu.di());
                    for p in 0..4u32 {
                        self.hw.wr16(a + p * 4, EMS_FRAME as u16 + p as u16 * 0x400);
                        self.hw.wr16(a + p * 4 + 2, p as u16);
                    }
                }
                self.cpu.set_cx(4);
            }
            0x59 => {
                if self.cpu.al() == 1 {
                    let f = self.ems.free.len() as u16;
                    self.cpu.set_bx(f);
                    self.cpu.set_dx(self.ems.total);
                } else {
                    st = 0xa4;
                }
            }
            0xde => st = 0x84,
            _ => st = 0x84,
        }
        self.cpu.set_ah(st);
    }

    /// 57h: move or exchange a region between conventional and expanded
    /// memory.
    fn ems_move(&mut self) -> u8 {
        let a = self.seg_lin(DS, self.cpu.si());
        let len = self.hw.rd32(a);
        let addr = |m: &mut Machine, at: u32| -> Option<(bool, u32)> {
            let kind = m.hw.rd(at);
            let h = m.hw.rd16(at + 1);
            let off = m.hw.rd16(at + 3) as u32;
            let seg = m.hw.rd16(at + 5);
            if kind == 0 {
                Some((false, Machine::lin(seg, off as u16)))
            } else {
                let g = m.ems_page(h, seg)?;
                Some((true, g as u32 * EMS_PAGE as u32 + off))
            }
        };
        let (Some(src), Some(dst)) = (addr(self, a + 4), addr(self, a + 11)) else { return 0x8a };
        self.ems_grow();
        let xchg = self.cpu.al() == 1;
        for i in 0..len {
            let read = |m: &mut Machine, (e, at): (bool, u32)| if e { m.hw.ems.get((at + i) as usize).copied().unwrap_or(0) } else { m.hw.rd(at + i) };
            let write = |m: &mut Machine, (e, at): (bool, u32), v: u8| {
                if e {
                    if let Some(b) = m.hw.ems.get_mut((at + i) as usize) {
                        *b = v;
                    }
                } else {
                    m.hw.wr(at + i, v);
                }
            };
            let s = read(self, src);
            if xchg {
                let d = read(self, dst);
                write(self, src, d);
            }
            write(self, dst, s);
        }
        0
    }

    pub(crate) fn ems_process_ended(&mut self, psp: u16) {
        let _ = psp;
        let _: Option<String> = None;
    }
}
