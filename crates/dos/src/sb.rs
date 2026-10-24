//! A Sound Blaster 2.0 (DSP 2.01 at port 220h, IRQ 7, 8-bit DMA on
//! channel 1) and the 8237 DMA controller it plays from. Programs find it
//! through the BLASTER variable (`A220 I7 D1 T3`).
//!
//! Playback runs on the machine's clock: between instruction slices the
//! samples due at the programmed rate are taken from guest memory through
//! the DMA channel and handed to the host as 8-bit unsigned PCM; at the
//! end of a block the DSP raises its interrupt (auto-init mode goes on
//! with the next block).

use alloc::collections::VecDeque;
use alloc::vec::Vec;

pub const SB_BASE: u16 = 0x220;
pub const SB_IRQ: u8 = 7;
pub const SB_DMA: usize = 1;

/// One 8237 channel.
#[derive(Clone, Copy, Default)]
pub struct DmaChannel {
    pub base_addr: u16,
    pub base_count: u16,
    pub addr: u16,
    pub count: u16,
    pub page: u8,
    pub mode: u8,
    pub masked: bool,
}

impl DmaChannel {
    fn auto_init(&self) -> bool {
        self.mode & 0x10 != 0
    }
}

/// The 8237 (channels 0-3).
#[derive(Clone)]
pub struct Dma {
    pub ch: [DmaChannel; 4],
    flip: bool,
}

impl Default for Dma {
    fn default() -> Self {
        let mut d = Dma { ch: [DmaChannel::default(); 4], flip: false };
        for c in d.ch.iter_mut() {
            c.masked = true;
        }
        d
    }
}

impl Dma {
    pub fn io_write(&mut self, port: u16, v: u8) {
        match port {
            0x00..=0x07 => {
                let c = &mut self.ch[(port / 2) as usize];
                let hi = self.flip;
                self.flip = !self.flip;
                if port & 1 == 0 {
                    c.base_addr = if hi { (c.base_addr & 0xff) | (v as u16) << 8 } else { (c.base_addr & 0xff00) | v as u16 };
                    c.addr = c.base_addr;
                } else {
                    c.base_count = if hi { (c.base_count & 0xff) | (v as u16) << 8 } else { (c.base_count & 0xff00) | v as u16 };
                    c.count = c.base_count;
                }
            }
            0x0a => self.ch[(v & 3) as usize].masked = v & 4 != 0,
            0x0b => self.ch[(v & 3) as usize].mode = v,
            0x0c => self.flip = false,
            0x0d => *self = Dma::default(),
            0x0e => {
                for c in self.ch.iter_mut() {
                    c.masked = false;
                }
            }
            0x0f => {
                for (i, c) in self.ch.iter_mut().enumerate() {
                    c.masked = v & (1 << i) != 0;
                }
            }
            0x87 => self.ch[0].page = v,
            0x83 => self.ch[1].page = v,
            0x81 => self.ch[2].page = v,
            0x82 => self.ch[3].page = v,
            _ => {}
        }
    }

    pub fn io_read(&mut self, port: u16) -> u8 {
        match port {
            0x00..=0x07 => {
                let c = &self.ch[(port / 2) as usize];
                let v = if port & 1 == 0 { c.addr } else { c.count };
                let hi = self.flip;
                self.flip = !self.flip;
                if hi { (v >> 8) as u8 } else { v as u8 }
            }
            0x08 => 0,
            0x87 => self.ch[0].page,
            0x83 => self.ch[1].page,
            0x81 => self.ch[2].page,
            0x82 => self.ch[3].page,
            _ => 0xff,
        }
    }
}

#[derive(Clone, Default)]
pub struct SoundBlaster {
    /// Bytes for the program to read from 22Ah.
    out: VecDeque<u8>,
    /// The command being collected and its parameters so far.
    cmd: Option<u8>,
    args: Vec<u8>,
    reset_written: bool,
    pub speaker: bool,
    /// Samples per second.
    pub rate: u32,
    /// Block length in bytes (the count of the last DMA command, + 1).
    pub block: u32,
    /// Bytes of the current block still to play.
    pub left: u32,
    pub playing: bool,
    pub auto: bool,
    pub paused: bool,
    /// Fractional samples due (in 1/1_000_000 of a sample).
    pub acc: u64,
    /// The interrupt is pending (acknowledged by reading 22Eh).
    pub irq: bool,
    /// Last time samples were taken (microseconds).
    pub last_us: u64,
}

impl SoundBlaster {
    fn command(&mut self, cmd: u8, a: &[u8]) -> Option<usize> {
        // Parameters each command needs.
        let need = match cmd {
            0x10 | 0x40 | 0xe0 => 1,
            0x14 | 0x41 | 0x42 | 0x48 | 0x24 | 0x16 | 0x17 | 0x74 | 0x75 | 0x76 | 0x77 => 2,
            _ => 0,
        };
        if a.len() < need {
            return Some(need);
        }
        match cmd {
            0x10 => {}
            0x40 => self.rate = 1_000_000 / (256 - a[0] as u32).max(1),
            0x41 | 0x42 => self.rate = (a[0] as u32) << 8 | a[1] as u32,
            0x14 | 0x16 | 0x17 | 0x74..=0x77 => {
                self.block = (a[0] as u32 | (a[1] as u32) << 8) + 1;
                self.left = self.block;
                self.playing = true;
                self.auto = false;
                self.paused = false;
            }
            0x48 => self.block = (a[0] as u32 | (a[1] as u32) << 8) + 1,
            0x1c | 0x90 => {
                self.left = self.block.max(1);
                self.playing = true;
                self.auto = true;
                self.paused = false;
            }
            0xd0 => self.paused = true,
            0xd4 => self.paused = false,
            0xda => self.auto = false,
            0xd1 => self.speaker = true,
            0xd3 => self.speaker = false,
            0xd8 => self.out.push_back(if self.speaker { 0xff } else { 0 }),
            0xe1 => {
                self.out.push_back(2);
                self.out.push_back(1);
            }
            0xe0 => self.out.push_back(!a[0]),
            0x20 => self.out.push_back(0x80),
            0xf2 => self.irq = true,
            _ => {}
        }
        None
    }

    pub fn io_write(&mut self, port: u16, v: u8) {
        match port - SB_BASE {
            6 => {
                if v & 1 != 0 {
                    self.reset_written = true;
                } else if self.reset_written {
                    self.reset_written = false;
                    let rate = self.rate;
                    *self = SoundBlaster { rate, ..Default::default() };
                    self.out.push_back(0xaa);
                }
            }
            0xc => match self.cmd {
                None => {
                    self.args.clear();
                    if self.command(v, &[]).is_some() {
                        self.cmd = Some(v);
                    }
                }
                Some(c) => {
                    self.args.push(v);
                    let a = core::mem::take(&mut self.args);
                    if self.command(c, &a).is_some() {
                        self.args = a;
                    } else {
                        self.cmd = None;
                    }
                }
            },
            _ => {}
        }
    }

    pub fn io_read(&mut self, port: u16) -> u8 {
        match port - SB_BASE {
            0xa => self.out.pop_front().unwrap_or(0xff),
            // Write buffer status: never busy.
            0xc => 0x7f,
            0xe => {
                self.irq = false;
                if self.out.is_empty() { 0x7f } else { 0xff }
            }
            _ => 0xff,
        }
    }
}

impl crate::hw::Hw {
    /// Take `n` bytes from DMA channel `c`; whether the count ran out.
    pub fn dma_take(&mut self, c: usize, n: usize, out: &mut Vec<u8>) -> bool {
        let mut done = false;
        for _ in 0..n {
            let ch = self.dma.ch[c];
            if ch.masked {
                break;
            }
            let a = (ch.page as u32) << 16 | ch.addr as u32;
            out.push(self.rd(a));
            let ch = &mut self.dma.ch[c];
            ch.addr = ch.addr.wrapping_add(1);
            let (cnt, wrapped) = ch.count.overflowing_sub(1);
            ch.count = cnt;
            if wrapped {
                done = true;
                if ch.auto_init() {
                    ch.addr = ch.base_addr;
                    ch.count = ch.base_count;
                } else {
                    ch.masked = true;
                    break;
                }
            }
        }
        done
    }
}

impl crate::Machine {
    /// Play what is due since the last slice; raise the DSP's interrupt at
    /// the end of a block.
    pub(crate) fn sb_tick<H: crate::Host>(&mut self, host: &mut H) {
        let now = self.hw.us_now;
        let sb = &mut self.hw.sb;
        let dt = now.saturating_sub(sb.last_us).min(100_000);
        sb.last_us = now;
        if !sb.playing || sb.paused || sb.rate == 0 {
            return;
        }
        sb.acc += dt * sb.rate as u64;
        let mut n = (sb.acc / 1_000_000) as u32;
        sb.acc %= 1_000_000;
        let rate = sb.rate;
        let mut pcm = Vec::new();
        let mut irq = false;
        while n > 0 {
            let left = self.hw.sb.left;
            let k = n.min(left);
            self.hw.dma_take(SB_DMA, k as usize, &mut pcm);
            n -= k;
            let sb = &mut self.hw.sb;
            sb.left -= k;
            if sb.left == 0 {
                irq = true;
                if sb.auto {
                    sb.left = sb.block.max(1);
                } else {
                    sb.playing = false;
                    break;
                }
            }
        }
        if !pcm.is_empty() && self.hw.sb.speaker {
            host.pcm(rate, &pcm);
        }
        if irq {
            self.hw.sb.irq = true;
            self.hw.pic.raise(SB_IRQ);
        }
    }
}
