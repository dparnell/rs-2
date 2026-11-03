//! A Sound Blaster 16 (DSP 4.05, CT1745 mixer, OPL3 FM: crate::opl) and
//! the two 8237 DMA controllers it plays from. Programs find it through
//! the BLASTER variable (`A220 I7 D1 H5 P330 T6`); a BLASTER the session
//! sets chooses the base port, IRQ, DMA channels and model (`T1`/`T3`
//! Sound Blaster 1.x/2.0 with DSP 2.01, `T2`/`T4` Sound Blaster Pro with
//! DSP 3.02, `T6` Sound Blaster 16).
//!
//! Playback runs on the machine's clock (crate::audio pulls the samples
//! as output time passes): 8-bit DMA (single-cycle, auto-init, high
//! speed, SB Pro stereo through mixer register 0Eh) and the SB16's 8- and
//! 16-bit commands (Bxh/Cxh: signed or unsigned, mono or stereo) on the
//! 8- and 16-bit channels, the direct DAC (10h), silence blocks (80h);
//! recording delivers silence. At the end of a block the DSP raises its
//! interrupt (acknowledged at base+Eh for 8-bit, base+Fh for 16-bit
//! transfers).

use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// The 8237 channels 0-7 (4-7 on the second controller: 16-bit, in words).
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

/// The two 8237s.
#[derive(Clone)]
pub struct Dma {
    pub ch: [DmaChannel; 8],
    flip: [bool; 2],
}

impl Default for Dma {
    fn default() -> Self {
        let mut d = Dma { ch: [DmaChannel::default(); 8], flip: [false; 2] };
        for c in d.ch.iter_mut() {
            c.masked = true;
        }
        d
    }
}

const PAGE_PORTS: [u16; 8] = [0x87, 0x83, 0x81, 0x82, 0x8f, 0x8b, 0x89, 0x8a];

impl Dma {
    /// Controller (0, 1) and register (0-15) of a port.
    fn reg(port: u16) -> Option<(usize, u16)> {
        match port {
            0x00..=0x0f => Some((0, port)),
            0xc0..=0xdf if port & 1 == 0 => Some((1, (port - 0xc0) / 2)),
            _ => None,
        }
    }

    pub fn io_write(&mut self, port: u16, v: u8) {
        if let Some(c) = PAGE_PORTS.iter().position(|&p| p == port) {
            self.ch[c].page = v;
            return;
        }
        let Some((k, r)) = Dma::reg(port) else { return };
        let base = k * 4;
        match r {
            0..=7 => {
                let c = &mut self.ch[base + (r / 2) as usize];
                let hi = self.flip[k];
                self.flip[k] = !hi;
                let set = |w: u16| if hi { (w & 0xff) | (v as u16) << 8 } else { (w & 0xff00) | v as u16 };
                if r & 1 == 0 {
                    c.base_addr = set(c.base_addr);
                    c.addr = c.base_addr;
                } else {
                    c.base_count = set(c.base_count);
                    c.count = c.base_count;
                }
            }
            0x0a => self.ch[base + (v & 3) as usize].masked = v & 4 != 0,
            0x0b => self.ch[base + (v & 3) as usize].mode = v,
            0x0c => self.flip[k] = false,
            0x0d => {
                for c in &mut self.ch[base..base + 4] {
                    *c = DmaChannel { masked: true, page: c.page, ..Default::default() };
                }
                self.flip[k] = false;
            }
            0x0e => {
                for c in &mut self.ch[base..base + 4] {
                    c.masked = false;
                }
            }
            0x0f => {
                for (i, c) in self.ch[base..base + 4].iter_mut().enumerate() {
                    c.masked = v & (1 << i) != 0;
                }
            }
            _ => {}
        }
    }

    pub fn io_read(&mut self, port: u16) -> u8 {
        if let Some(c) = PAGE_PORTS.iter().position(|&p| p == port) {
            return self.ch[c].page;
        }
        let Some((k, r)) = Dma::reg(port) else { return 0xff };
        match r {
            0..=7 => {
                let c = &self.ch[k * 4 + (r / 2) as usize];
                let v = if r & 1 == 0 { c.addr } else { c.count };
                let hi = self.flip[k];
                self.flip[k] = !hi;
                if hi { (v >> 8) as u8 } else { v as u8 }
            }
            // Status: the terminal counts reached (not kept).
            8 => 0,
            _ => 0xff,
        }
    }
}

/// The card's model, from BLASTER's T.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Model {
    Sb2,
    Pro,
    #[default]
    Sb16,
}

/// The transfer in progress.
#[derive(Clone, Copy, Default)]
pub struct Xfer {
    pub on: bool,
    pub bits16: bool,
    pub stereo: bool,
    pub signed: bool,
    pub auto: bool,
    /// Recording (A/D): the DMA buffer is filled with silence.
    pub input: bool,
    /// A silence block (80h): no DMA.
    pub silence: bool,
    pub paused: bool,
    /// Transfer units (bytes, or words for 16-bit) per block, and left.
    pub block: u32,
    pub left: u32,
}

#[derive(Clone)]
pub struct SoundBlaster {
    pub base: u16,
    pub irq: u8,
    pub dma8: usize,
    pub dma16: usize,
    pub model: Model,
    /// Bytes for the program to read from base+Ah.
    out: VecDeque<u8>,
    /// The command being collected and its parameters so far.
    cmd: Option<u8>,
    args: Vec<u8>,
    reset_written: bool,
    pub speaker: bool,
    /// Frames per second of the transfer (8-bit time constants give
    /// samples, halved for SB Pro stereo).
    pub rate: u32,
    pub tc_rate: bool,
    /// Block length of the 8-bit commands (48h), in bytes.
    pub block8: u32,
    pub x: Xfer,
    /// Interrupts pending: bit 0 8-bit, bit 1 16-bit (mixer 82h).
    pub pending: u8,
    /// A block ended (or F2h/F3h): raise the interrupt line.
    pub irq_edge: bool,
    test_reg: u8,
    /// The CT1745 mixer.
    pub mixer_index: u8,
    pub mixer: [u8; 256],
    /// Direct DAC samples written since the last mix.
    pub dac: VecDeque<u8>,
    /// The frame being played and the fraction of the next.
    pub cur: [i32; 2],
    pub frac: u32,
    /// HW_NOSOUND: everything runs, nothing is heard.
    pub muted: bool,
}

impl Default for SoundBlaster {
    fn default() -> Self {
        let mut s = SoundBlaster {
            base: 0x220,
            irq: 7,
            dma8: 1,
            dma16: 5,
            model: Model::Sb16,
            out: VecDeque::new(),
            cmd: None,
            args: Vec::new(),
            reset_written: false,
            speaker: false,
            rate: 11025,
            tc_rate: true,
            block8: 0x800,
            x: Xfer::default(),
            pending: 0,
            irq_edge: false,
            test_reg: 0,
            mixer_index: 0,
            mixer: [0; 256],
            dac: VecDeque::new(),
            cur: [0; 2],
            frac: 0,
            muted: false,
        };
        s.mixer_reset();
        s
    }
}

impl SoundBlaster {
    /// Settings from a BLASTER value (`A220 I5 D1 H5 T6`).
    pub fn configure(&mut self, blaster: &str) {
        for w in blaster.split_ascii_whitespace() {
            let (k, v) = w.split_at(1);
            let dec = || v.parse::<u32>().ok();
            match k.to_ascii_uppercase().as_str() {
                "A" => {
                    if let Ok(a) = u16::from_str_radix(v, 16) {
                        if (0x210..=0x280).contains(&a) {
                            self.base = a;
                        }
                    }
                }
                "I" => {
                    if let Some(i) = dec().filter(|i| matches!(i, 2 | 3 | 5 | 7 | 9 | 10)) {
                        self.irq = if i == 9 { 2 } else { i as u8 };
                    }
                }
                "D" => {
                    if let Some(d) = dec().filter(|&d| d < 4 && d != 2) {
                        self.dma8 = d as usize;
                    }
                }
                "H" => {
                    if let Some(d) = dec().filter(|&d| (5..8).contains(&d)) {
                        self.dma16 = d as usize;
                    }
                }
                "T" => {
                    self.model = match dec() {
                        Some(1) | Some(3) => Model::Sb2,
                        Some(2) | Some(4) | Some(5) => Model::Pro,
                        _ => Model::Sb16,
                    }
                }
                _ => {}
            }
        }
        self.mixer_reset();
    }

    /// The BLASTER value describing this card.
    pub fn blaster(&self) -> alloc::string::String {
        match self.model {
            Model::Sb16 => alloc::format!("A{:X} I{} D{} H{} P330 T6", self.base, self.irq, self.dma8, self.dma16),
            Model::Pro => alloc::format!("A{:X} I{} D{} T4", self.base, self.irq, self.dma8),
            Model::Sb2 => alloc::format!("A{:X} I{} D{} T3", self.base, self.irq, self.dma8),
        }
    }

    fn version(&self) -> (u8, u8) {
        match self.model {
            Model::Sb2 => (2, 1),
            Model::Pro => (3, 2),
            Model::Sb16 => (4, 5),
        }
    }

    fn mixer_reset(&mut self) {
        let m = &mut self.mixer;
        *m = [0; 256];
        // SB16 volumes (5 bits in the top of the byte): master, voice and
        // FM at the top (as the session's own output level); the SB Pro
        // registers alike.
        for r in [0x30, 0x31, 0x32, 0x33, 0x34, 0x35] {
            m[r] = 31 << 3;
        }
        m[0x3b] = 0x80;
        for r in [0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47] {
            m[r] = 0;
        }
        m[0x3c] = 0x1f;
        m[0x3d] = 0x15;
        m[0x3e] = 0x0b;
        m[0x44] = 0x80;
        m[0x45] = 0x80;
        m[0x46] = 0x80;
        m[0x47] = 0x80;
        m[0x04] = 0xff;
        m[0x22] = 0xff;
        m[0x26] = 0xff;
        m[0x28] = 0;
        m[0x2e] = 0;
        let irq = match self.irq {
            2 => 1,
            5 => 2,
            7 => 4,
            _ => 8,
        };
        m[0x80] = irq;
        m[0x81] = (1u8 << self.dma8) | (1u8 << self.dma16);
    }

    /// Mixer register writes, keeping the SB Pro and SB16 registers in
    /// step as the CT1745 does.
    fn mixer_write(&mut self, v: u8) {
        let i = self.mixer_index;
        let m = &mut self.mixer;
        match i {
            0x00 => {
                self.mixer_reset();
                return;
            }
            0x80 => {
                self.irq = match v & 0x0f {
                    1 => 2,
                    2 => 5,
                    4 => 7,
                    8 => 10,
                    _ => self.irq,
                };
            }
            0x81 => {
                if let Some(d) = [0usize, 1, 3].into_iter().find(|&d| v & (1 << d) != 0) {
                    self.dma8 = d;
                }
                if let Some(d) = [5usize, 6, 7].into_iter().find(|&d| v & (1 << d) != 0) {
                    self.dma16 = d;
                }
            }
            0x82 => return,
            // SB Pro pairs (left in the high nibble) to SB16 pairs.
            0x04 | 0x22 | 0x26 => {
                let r = match i {
                    0x04 => 0x32,
                    0x22 => 0x30,
                    _ => 0x34,
                };
                m[r] = (v & 0xf0) | 0x08;
                m[r + 1] = (v << 4) | 0x08;
            }
            0x30..=0x35 => {
                let pro = match i & !1 {
                    0x30 => 0x22,
                    0x32 => 0x04,
                    _ => 0x26,
                };
                if i & 1 == 0 {
                    m[pro] = (m[pro] & 0x0f) | (v & 0xf0);
                } else {
                    m[pro] = (m[pro] & 0xf0) | (v >> 4);
                }
            }
            _ => {}
        }
        self.mixer[i as usize] = v;
    }

    fn mixer_read(&self) -> u8 {
        let i = self.mixer_index;
        match i {
            0x80 => match self.irq {
                2 => 1,
                5 => 2,
                7 => 4,
                _ => 8,
            },
            0x81 => (1u8 << self.dma8) | if self.model == Model::Sb16 { 1u8 << self.dma16 } else { 0 },
            0x82 => self.pending | if self.model == Model::Sb16 { 0x20 } else { 0 },
            _ => self.mixer[i as usize],
        }
    }

    /// The gain (0-256) for a pair of SB16 volume registers, with the
    /// master volume: (left, right).
    pub fn gain(&self, reg: usize) -> [i32; 2] {
        // 5-bit volumes in 2 dB steps: 31 is 0 dB.
        let g = |v: u8| -> i32 {
            let steps = 31 - (v >> 3) as i32;
            if steps >= 31 {
                return 0;
            }
            // 2 dB = 10/8 of 1.6 dB... approximated: 256 * 10^(-steps*2/20).
            let mut x = 256 * 1024i64;
            for _ in 0..steps {
                x = x * 813 / 1024;
            }
            (x / 1024) as i32
        };
        let m = &self.mixer;
        [g(m[0x30]) * g(m[reg]) >> 8, g(m[0x31]) * g(m[reg + 1]) >> 8]
    }

    /// SB Pro stereo output (mixer 0Eh bit 1) for the 8-bit commands.
    fn pro_stereo(&self) -> bool {
        self.model != Model::Sb2 && self.mixer[0x0e] & 2 != 0
    }

    fn start8(&mut self, auto: bool, input: bool, block: u32) {
        let stereo = self.pro_stereo();
        self.x = Xfer { on: true, bits16: false, stereo, signed: false, auto, input, silence: false, paused: false, block: block.max(1), left: block.max(1) };
    }

    fn command(&mut self, cmd: u8, a: &[u8]) -> Option<usize> {
        // Parameters each command needs.
        let need = match cmd {
            0x10 | 0x40 | 0xe0 | 0xe4 | 0x38 => 1,
            0x14 | 0x16 | 0x17 | 0x24 | 0x41 | 0x42 | 0x48 | 0x74..=0x77 | 0x80 => 2,
            0xb0..=0xcf => 3,
            _ => 0,
        };
        if a.len() < need {
            return Some(need);
        }
        let word = |i: usize| (a[i] as u32 | (a[i + 1] as u32) << 8) + 1;
        match cmd {
            0x10 => {
                if self.dac.len() < 8192 {
                    self.dac.push_back(a[0]);
                }
            }
            0x40 => {
                self.rate = 1_000_000 / (256 - a[0] as u32).max(1);
                self.tc_rate = true;
            }
            0x41 | 0x42 => {
                self.rate = ((a[0] as u32) << 8 | a[1] as u32).clamp(4000, 48000);
                self.tc_rate = false;
            }
            0x14 | 0x16 | 0x17 | 0x74..=0x77 => self.start8(false, false, word(0)),
            0x24 => self.start8(false, true, word(0)),
            0x48 => self.block8 = word(0),
            0x1c | 0x1f | 0x7d | 0x7f | 0x90 => self.start8(true, false, self.block8),
            0x91 => self.start8(false, false, self.block8),
            0x2c | 0x98 => self.start8(true, true, self.block8),
            0x99 => self.start8(false, true, self.block8),
            0x80 => {
                self.x = Xfer { on: true, silence: true, block: word(0), left: word(0), ..Default::default() };
            }
            0xb0..=0xcf if self.model == Model::Sb16 => {
                let bits16 = cmd < 0xc0;
                let mode = a[0];
                let n = word(1);
                self.x = Xfer {
                    on: true,
                    bits16,
                    stereo: mode & 0x20 != 0,
                    signed: mode & 0x10 != 0,
                    auto: cmd & 4 != 0,
                    input: cmd & 8 != 0,
                    silence: false,
                    paused: false,
                    block: n,
                    left: n,
                };
            }
            0xd0 | 0xd5 => self.x.paused = true,
            0xd4 | 0xd6 => self.x.paused = false,
            0xd9 | 0xda => self.x.auto = false,
            0xd1 => self.speaker = true,
            0xd3 => self.speaker = false,
            0xd8 => self.out.push_back(if self.speaker { 0xff } else { 0 }),
            0xe1 => {
                let (a, b) = self.version();
                self.out.push_back(a);
                self.out.push_back(b);
            }
            0xe0 => self.out.push_back(!a[0]),
            0xe3 if self.model == Model::Sb16 => {
                self.out.extend(b"COPYRIGHT (C) CREATIVE TECHNOLOGY LTD, 1992.\0".iter().copied());
            }
            0xe4 => self.test_reg = a[0],
            0xe8 => self.out.push_back(self.test_reg),
            0x20 => self.out.push_back(0x80),
            0xf2 => self.pending |= 1,
            0xf3 if self.model == Model::Sb16 => self.pending |= 2,
            0xf8 => self.out.push_back(0),
            _ => {}
        }
        None
    }

    /// Whether the DSP wants its interrupt line raised.
    pub fn take_irq(&mut self) -> bool {
        core::mem::take(&mut self.irq_edge)
    }

    pub fn io_write(&mut self, port: u16, v: u8) {
        match port - self.base {
            4 => self.mixer_index = v,
            5 => self.mixer_write(v),
            6 => {
                if v & 1 != 0 {
                    self.reset_written = true;
                } else if self.reset_written {
                    self.reset_written = false;
                    self.out.clear();
                    self.cmd = None;
                    self.args.clear();
                    self.x = Xfer::default();
                    self.speaker = false;
                    self.pending = 0;
                    self.out.push_back(0xaa);
                }
            }
            0xc => match self.cmd {
                None => {
                    self.args.clear();
                    if self.command(v, &[]).is_some() {
                        self.cmd = Some(v);
                    } else {
                        self.irq_edge |= self.pending != 0 && matches!(v, 0xf2 | 0xf3);
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
        match port - self.base {
            5 => self.mixer_read(),
            0xa => self.out.pop_front().unwrap_or(0xff),
            // Write buffer status: never busy.
            0xc => 0x7f,
            0xe => {
                self.pending &= !1;
                if self.out.is_empty() { 0x7f } else { 0xff }
            }
            0xf => {
                self.pending &= !2;
                0xff
            }
            _ => 0xff,
        }
    }
}

impl crate::hw::Hw {
    /// Take one transfer unit (a byte, or a word on channels 4-7) from DMA
    /// channel `c`, or put one when `put` is given (recording); None when
    /// the channel is masked. The second value: the count ran out.
    pub fn dma_unit(&mut self, c: usize, put: Option<u16>) -> Option<(u16, bool)> {
        let ch = self.dma.ch[c];
        if ch.masked {
            return None;
        }
        let v = if c < 4 {
            let a = (ch.page as u32) << 16 | ch.addr as u32;
            match put {
                Some(v) => {
                    self.wr(a, v as u8);
                    v
                }
                None => self.rd(a) as u16,
            }
        } else {
            let a = ((ch.page as u32) & 0xfe) << 16 | (ch.addr as u32) << 1;
            match put {
                Some(v) => {
                    self.wr16(a, v);
                    v
                }
                None => self.rd16(a),
            }
        };
        let ch = &mut self.dma.ch[c];
        ch.addr = ch.addr.wrapping_add(1);
        let (cnt, wrapped) = ch.count.overflowing_sub(1);
        ch.count = cnt;
        if wrapped {
            if ch.auto_init() {
                ch.addr = ch.base_addr;
                ch.count = ch.base_count;
            } else {
                ch.masked = true;
            }
        }
        Some((v, wrapped))
    }
}

impl crate::Machine {
    /// The Sound Blaster's next frame (left, right, 16-bit), taking its
    /// samples from DMA; raises the DSP's interrupt at the end of a
    /// block.
    pub(crate) fn sb_frame(&mut self) -> [i32; 2] {
        let x = self.hw.sb.x;
        if !x.on || x.paused {
            return [0, 0];
        }
        let chans = if x.stereo { 2 } else { 1 };
        let mut v = [0i32; 2];
        for k in 0..chans {
            let s = if x.silence {
                0
            } else {
                let c = if x.bits16 { self.hw.sb.dma16 } else { self.hw.sb.dma8 };
                let put = x.input.then_some(if x.bits16 { 0 } else { 0x80 });
                match self.hw.dma_unit(c, put) {
                    None => 0,
                    Some((u, _)) => match (x.bits16, x.signed) {
                        (true, true) => u as i16 as i32,
                        (true, false) => u as i32 - 0x8000,
                        (false, true) => ((u as u8 as i8) as i32) << 8,
                        (false, false) => (u as i32 - 0x80) << 8,
                    },
                }
            };
            v[k] = if x.input { 0 } else { s };
            let sb = &mut self.hw.sb;
            sb.x.left = sb.x.left.saturating_sub(1);
            if sb.x.left == 0 {
                sb.pending |= if x.bits16 { 2 } else { 1 };
                sb.irq_edge = true;
                if sb.x.auto {
                    sb.x.left = sb.x.block;
                } else {
                    sb.x.on = false;
                    break;
                }
            }
        }
        if chans == 1 {
            v[1] = v[0];
        }
        // DSPs before 4.xx output only with the speaker on.
        if self.hw.sb.model != Model::Sb16 && !self.hw.sb.speaker {
            return [0, 0];
        }
        v
    }

    /// Frames per second of the transfer.
    pub(crate) fn sb_rate(&self) -> u32 {
        let sb = &self.hw.sb;
        if sb.tc_rate && sb.x.stereo && !sb.x.bits16 { sb.rate / 2 } else { sb.rate }.max(1)
    }

    /// Raise the DSP's interrupt if a block ended.
    pub(crate) fn sb_irq(&mut self) {
        if self.hw.sb.take_irq() {
            let irq = self.hw.sb.irq;
            self.hw.pic.raise(if irq == 2 { 9 } else { irq });
        }
    }
}
