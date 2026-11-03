//! An FM synthesizer: the Yamaha YMF262 (OPL3) of a Sound Blaster 16,
//! which also behaves as the YM3812 (OPL2) of an AdLib and the older
//! Sound Blasters while its OPL3 mode bit (105h) is off.
//!
//! Ports: 388h-38Bh (AdLib, and the OPL3's second register bank at
//! 38Ah/38Bh), the Sound Blaster's base+0-3 and base+8/9.
//!
//! The model follows the chip's structure: 18 two-operator channels in two
//! banks (or six four-operator channels), each operator a phase generator
//! (F-number, block, multiplier, vibrato), one of eight waveforms, and an
//! envelope generator (attack, decay, sustain, release with key scaling)
//! whose attenuation is added to the total level, key scale level and
//! tremolo in the chip's 0.1875 dB steps; feedback on the first operator;
//! rhythm mode (bass drum, snare, tom, cymbal, hi-hat with the chip's
//! noise and phase mixing); the two timers and the status register that
//! programs detect the chip by. It renders at the machine's output rate
//! (crate::audio::RATE) rather than the chip's 49716 Hz.

use crate::audio::RATE;

/// The chip's own sample rate.
const CHIP_RATE: u64 = 49716;

const MULT2: [u32; 16] = [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];
const KSL_ROM: [i32; 16] = [0, 32, 40, 45, 48, 51, 53, 55, 56, 58, 59, 60, 61, 62, 63, 64];
const KSL_SHIFT: [u32; 4] = [8, 1, 2, 0];
/// Attenuation at which an operator is silent (0.1875 dB steps).
const MAX_ATT: u32 = 511;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Eg {
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Default)]
struct Op {
    am: bool,
    vib: bool,
    egt: bool,
    ksr: bool,
    mult: u8,
    ksl: u8,
    tl: u8,
    ar: u8,
    dr: u8,
    sl: u8,
    rr: u8,
    ws: u8,
    /// Phase: the top 10 bits index the waveform.
    phase: u32,
    /// Envelope attenuation in 1/65536 steps.
    env: u32,
    eg: Eg,
    /// Bit 0 keyed by B0h, bit 1 by the rhythm register.
    key: u8,
    out: i32,
    prev: i32,
}

#[derive(Clone, Copy, Default)]
struct Chan {
    fnum: u16,
    block: u8,
    fb: u8,
    cnt: bool,
    /// C0h's output bits (left, right, and the second DAC's two).
    pan: u8,
    op: [Op; 2],
}

#[derive(Clone, Copy, Default)]
struct Timer {
    reg: u8,
    /// Started at (microseconds).
    start: Option<u64>,
    masked: bool,
}

pub struct Opl {
    ch: [Chan; 18],
    /// The register address latched for each bank.
    pub addr: [u8; 2],
    /// OPL3 mode (105h bit 0).
    new: bool,
    /// Waveform select enable (OPL2, 01h bit 5).
    wse: bool,
    /// Four-operator pairs (104h).
    conn4: u8,
    nts: bool,
    /// BDh: tremolo depth, vibrato depth, rhythm mode and its keys.
    dam: bool,
    dvb: bool,
    rhythm: bool,
    noise: u32,
    /// Chip samples elapsed (for the LFOs), and the fraction of one.
    ticks: u64,
    tick_frac: u64,
    timers: [Timer; 2],
    /// Timer flags (status bits 6 and 5).
    flags: u8,
    /// The chip's notion of time: the machine's clock, advanced a little
    /// by each status read so that delay loops reading the status port
    /// see time pass within one slice of instructions.
    pub clock: u64,
    /// Any operator sounds.
    active: bool,
    gain: [u32; 512],
    qsin: [u32; 256],
    dec_inc: [u32; 64],
    att_k: [u64; 64],
}

impl Default for Opl {
    fn default() -> Self {
        Opl::new()
    }
}

/// sin(x) for x in [0, pi/2] (a Taylor series is exact enough here).
fn sin(x: f64) -> f64 {
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    for n in 1..12 {
        term *= -x2 / ((2 * n) as f64 * (2 * n + 1) as f64);
        sum += term;
    }
    sum
}

fn sqrt(v: f64) -> f64 {
    let mut x = v;
    for _ in 0..40 {
        x = 0.5 * (x + v / x);
    }
    x
}

impl Opl {
    pub fn new() -> Opl {
        let mut o = Opl {
            ch: [Chan::default(); 18],
            addr: [0; 2],
            new: false,
            wse: false,
            conn4: 0,
            nts: false,
            dam: false,
            dvb: false,
            rhythm: false,
            noise: 1,
            ticks: 0,
            tick_frac: 0,
            timers: [Timer::default(); 2],
            flags: 0,
            clock: 0,
            active: false,
            gain: [0; 512],
            qsin: [0; 256],
            dec_inc: [0; 64],
            att_k: [0; 64],
        };
        for c in o.ch.iter_mut() {
            c.pan = 0x30;
            for op in c.op.iter_mut() {
                op.env = MAX_ATT << 16;
            }
        }
        // 2^(-i/32) for the attenuation steps (0.1875 dB = 1/32 octave).
        let mut step = 2.0f64;
        for _ in 0..5 {
            step = sqrt(step);
        }
        let step = 1.0 / step;
        let mut f = 65535.0f64;
        let mut frac = [0u32; 32];
        for v in frac.iter_mut() {
            *v = f as u32;
            f *= step;
        }
        for (i, g) in o.gain.iter_mut().enumerate() {
            *g = frac[i % 32] >> (i / 32);
        }
        for (i, s) in o.qsin.iter_mut().enumerate() {
            *s = (sin((i as f64 + 0.5) * core::f64::consts::PI / 512.0) * 65535.0) as u32;
        }
        // Envelope rates: rate 4 (register value 1) decays from 0 to
        // 96 dB in 39.28 s and attacks in 2.826 s; four more halve that,
        // the steps between divide it by 1, 1.25, 1.5, 1.75.
        for r in 4..64u64 {
            let sub = 4 + (r & 3);
            let sh = (r >> 2) - 1;
            o.dec_inc[r as usize] = ((((MAX_ATT as u64) << 16) * sub << sh) * 1000 / (4 * 39280 * RATE as u64)) as u32;
            // Attack is exponential: k per sample takes ln(512) / k
            // samples (in 1/2^24).
            o.att_k[r as usize] = if r >= 60 { u64::MAX } else { (((624u64 << 24) / 100) * sub << sh) * 1000 / (4 * 2826 * RATE as u64) };
        }
        o
    }

    /// Whether anything sounds (rendering can be skipped otherwise).
    pub fn active(&self) -> bool {
        self.active
    }

    /// The status register.
    pub fn status(&mut self, us_now: u64, opl3: bool) -> u8 {
        self.clock = self.clock.max(us_now) + 1;
        let now = self.clock;
        for (i, t) in self.timers.iter_mut().enumerate() {
            let Some(start) = t.start else { continue };
            let period = (256 - t.reg as u64) * if i == 0 { 80 } else { 320 };
            if now >= start + period {
                if !t.masked {
                    self.flags |= 0x40 >> i;
                }
                let n = (now - start) / period;
                t.start = Some(start + n * period);
            }
        }
        let irq = if self.flags != 0 { 0x80 } else { 0 };
        irq | self.flags | if opl3 { 0 } else { 0x06 }
    }

    /// Write `v` to register `reg` (0-1FFh).
    pub fn write(&mut self, reg: u16, v: u8) {
        let bank = (reg >> 8) as usize & 1;
        let r = reg as u8;
        match r {
            0x01 if bank == 0 => self.wse = v & 0x20 != 0,
            0x02 if bank == 0 => self.timers[0].reg = v,
            0x03 if bank == 0 => self.timers[1].reg = v,
            0x04 if bank == 0 => {
                if v & 0x80 != 0 {
                    self.flags = 0;
                } else {
                    let now = self.clock;
                    for i in 0..2 {
                        let t = &mut self.timers[i];
                        t.masked = v & (0x40 >> i) != 0;
                        if t.masked {
                            self.flags &= !(0x40 >> i);
                        }
                        let run = v & (1 << i) != 0;
                        if run && t.start.is_none() {
                            t.start = Some(now);
                        } else if !run {
                            t.start = None;
                        }
                    }
                }
            }
            0x04 => self.conn4 = v & 0x3f,
            0x05 if bank == 1 => self.new = v & 1 != 0,
            0x08 if bank == 0 => self.nts = v & 0x40 != 0,
            0x20..=0x35 | 0x40..=0x55 | 0x60..=0x75 | 0x80..=0x95 | 0xe0..=0xf5 => {
                let Some((c, o)) = slot(r & 0x1f) else { return };
                let op = &mut self.ch[bank * 9 + c].op[o];
                match r & 0xe0 {
                    0x20 => {
                        op.am = v & 0x80 != 0;
                        op.vib = v & 0x40 != 0;
                        op.egt = v & 0x20 != 0;
                        op.ksr = v & 0x10 != 0;
                        op.mult = v & 15;
                    }
                    0x40 => {
                        op.ksl = v >> 6;
                        op.tl = v & 0x3f;
                    }
                    0x60 => {
                        op.ar = v >> 4;
                        op.dr = v & 15;
                    }
                    0x80 => {
                        op.sl = v >> 4;
                        op.rr = v & 15;
                    }
                    _ => op.ws = v & 7,
                }
            }
            0xa0..=0xa8 => {
                let c = &mut self.ch[bank * 9 + (r & 15) as usize];
                c.fnum = (c.fnum & 0x300) | v as u16;
            }
            0xb0..=0xb8 => {
                let i = bank * 9 + (r & 15) as usize;
                let c = &mut self.ch[i];
                c.fnum = (c.fnum & 0xff) | ((v as u16 & 3) << 8);
                c.block = (v >> 2) & 7;
                let on = v & 0x20 != 0;
                for o in 0..2 {
                    set_key(&mut c.op[o], 1, on);
                }
                if on {
                    self.active = true;
                }
            }
            0xbd if bank == 0 => {
                self.dam = v & 0x80 != 0;
                self.dvb = v & 0x40 != 0;
                let was = self.rhythm;
                self.rhythm = v & 0x20 != 0;
                let keys = if self.rhythm { v } else { 0 };
                if self.rhythm || was {
                    // BD: channel 6; HH 7/op 0, SD 7/op 1, TOM 8/op 0, TC 8/op 1.
                    set_key(&mut self.ch[6].op[0], 2, keys & 0x10 != 0);
                    set_key(&mut self.ch[6].op[1], 2, keys & 0x10 != 0);
                    set_key(&mut self.ch[7].op[0], 2, keys & 0x01 != 0);
                    set_key(&mut self.ch[7].op[1], 2, keys & 0x08 != 0);
                    set_key(&mut self.ch[8].op[0], 2, keys & 0x04 != 0);
                    set_key(&mut self.ch[8].op[1], 2, keys & 0x02 != 0);
                    if keys & 0x1f != 0 {
                        self.active = true;
                    }
                }
            }
            0xc0..=0xc8 => {
                let c = &mut self.ch[bank * 9 + (r & 15) as usize];
                c.pan = v & 0xf0;
                c.fb = (v >> 1) & 7;
                c.cnt = v & 1 != 0;
            }
            _ => {}
        }
    }

    /// Add `out.len()` stereo frames (at crate::audio::RATE) to `out`,
    /// scaled by `vol` (0-256 each side).
    pub fn render(&mut self, out: &mut [[i32; 2]], vol: [i32; 2]) {
        if !self.active {
            return;
        }
        let mut any = false;
        for f in out.iter_mut() {
            self.tick_frac += CHIP_RATE;
            self.ticks += self.tick_frac / RATE as u64;
            self.tick_frac %= RATE as u64;
            let (l, r) = self.sample();
            f[0] += l * vol[0] >> 8;
            f[1] += r * vol[1] >> 8;
        }
        for c in &self.ch {
            if c.op.iter().any(|o| o.eg != Eg::Off) {
                any = true;
            }
        }
        self.active = any;
    }

    fn sample(&mut self) -> (i32, i32) {
        // LFOs: tremolo a 210-step triangle every 64 chip samples, vibrato
        // 8 steps every 1024.
        let tpos = (self.ticks / 64 % 210) as i32;
        let tri = if tpos < 105 { tpos } else { 210 - tpos };
        let trem = (tri >> if self.dam { 2 } else { 4 }) as u32;
        let vibpos = (self.ticks / 1024 % 8) as u32;
        let bit = (((self.noise >> 14) ^ self.noise) & 1) as u32;
        self.noise = (self.noise >> 1) | (bit << 22);
        let noise = self.noise & 1;
        let (mut l, mut r) = (0i32, 0i32);
        let four = |o: &Opl, c: usize| o.new && c % 9 < 3 && o.conn4 & (1 << (c / 9 * 3 + c % 9)) != 0;
        let mut c = 0;
        while c < 18 {
            if self.rhythm && (6..9).contains(&c) {
                let v = self.rhythm_sample(trem, vibpos, noise);
                let pan = if self.new { self.ch[6].pan } else { 0x30 };
                if pan & 0x50 != 0 {
                    l += v;
                }
                if pan & 0xa0 != 0 {
                    r += v;
                }
                c = 9;
                continue;
            }
            let pan = if self.new { self.ch[c].pan } else { 0x30 };
            let v = if four(self, c) {
                let v = self.four_op(c, trem, vibpos);
                // The pair's second channel is part of this one.
                v
            } else if c % 9 >= 3 && c % 9 < 6 && four(self, c - 3) {
                c += 1;
                continue;
            } else {
                self.two_op(c, trem, vibpos)
            };
            if pan & 0x50 != 0 {
                l += v;
            }
            if pan & 0xa0 != 0 {
                r += v;
            }
            c += 1;
        }
        (l, r)
    }

    /// Advance an operator's phase and envelope; its 10-bit phase.
    fn advance(&mut self, c: usize, o: usize, vibpos: u32) -> u32 {
        let (fnum, block) = (self.ch[c].fnum as i32, self.ch[c].block);
        let dvb = self.dvb;
        let nts = self.nts;
        let op = &mut self.ch[c].op[o];
        let mut f = fnum;
        if op.vib {
            let mut range = (fnum >> 7) & 7;
            if vibpos & 3 == 0 {
                range = 0;
            } else if vibpos & 1 != 0 {
                range >>= 1;
            }
            if !dvb {
                range >>= 1;
            }
            f += if vibpos & 4 != 0 { -range } else { range };
        }
        let inc = ((f.max(0) as u64) << block) * MULT2[op.mult as usize] as u64 * (CHIP_RATE << 11) / RATE as u64;
        let p = op.phase >> 22;
        op.phase = op.phase.wrapping_add(inc as u32);
        // The envelope.
        let ks = ((block as u32) << 1 | ((fnum as u32) >> (if nts { 8 } else { 9 }) & 1)) >> if op.ksr { 0 } else { 2 };
        let rate = |reg: u8| if reg == 0 { 0 } else { ((reg as u32) * 4 + ks).min(63) as usize };
        match op.eg {
            Eg::Off => {}
            Eg::Attack => {
                let k = self.att_k[rate(op.ar)];
                if k == u64::MAX {
                    op.env = 0;
                } else if k > 0 {
                    let d = ((op.env as u64 * k) >> 24) as u32 + 16;
                    op.env = op.env.saturating_sub(d);
                }
                if op.env < 1 << 16 {
                    op.env = 0;
                    op.eg = Eg::Decay;
                }
            }
            Eg::Decay | Eg::Sustain | Eg::Release => {
                let sl = if op.sl == 15 { 31 } else { op.sl as u32 } << 4;
                let reg = match op.eg {
                    Eg::Decay => op.dr,
                    Eg::Sustain if op.egt => 0,
                    _ => op.rr,
                };
                op.env += self.dec_inc[rate(reg)];
                if op.eg == Eg::Decay && op.env >> 16 >= sl {
                    op.env = sl << 16;
                    op.eg = Eg::Sustain;
                }
                if op.env >> 16 >= MAX_ATT {
                    op.env = MAX_ATT << 16;
                    if op.eg != Eg::Sustain || !op.egt {
                        op.eg = Eg::Off;
                    }
                }
            }
        }
        p
    }

    /// An operator's output (13-bit signed) at 10-bit phase `p`.
    fn output(&self, c: usize, o: usize, p: u32, trem: u32) -> i32 {
        let ch = &self.ch[c];
        let op = &ch.op[o];
        if op.eg == Eg::Off {
            return 0;
        }
        let ksl = ((KSL_ROM[(ch.fnum >> 6) as usize] << 2) - ((8 - ch.block as i32) << 5)).max(0) as u32 >> KSL_SHIFT[op.ksl as usize];
        let mut att = (op.env >> 16) + ((op.tl as u32) << 2) + ksl + if op.am { trem } else { 0 };
        let ws = if self.new { op.ws } else if self.wse { op.ws & 3 } else { 0 };
        let p = p & 1023;
        let hs = |i: u32| {
            let i = i & 511;
            self.qsin[if i < 256 { i } else { 511 - i } as usize]
        };
        let (neg, mag) = match ws {
            0 => (p & 512 != 0, hs(p)),
            1 => (false, if p & 512 != 0 { 0 } else { hs(p) }),
            2 => (false, hs(p)),
            3 => (false, if p & 256 != 0 { 0 } else { hs(p & 255) }),
            4 => {
                if p & 512 != 0 {
                    (false, 0)
                } else {
                    let q = (p << 1) & 1023;
                    (q & 512 != 0, hs(q))
                }
            }
            5 => (false, if p & 512 != 0 { 0 } else { hs(p << 1) }),
            6 => (p & 512 != 0, 65535),
            _ => {
                let neg = p & 512 != 0;
                att += if neg { (!p) & 511 } else { p & 511 };
                (neg, 65535)
            }
        };
        if att >= MAX_ATT || mag == 0 {
            return 0;
        }
        let v = ((mag * self.gain[att as usize]) >> 20) as i32;
        if neg { -v } else { v }
    }

    /// The first operator of channel `c` with its feedback.
    fn first(&mut self, c: usize, trem: u32, vibpos: u32) -> i32 {
        let p = self.advance(c, 0, vibpos);
        let fb = self.ch[c].fb;
        let op = &self.ch[c].op[0];
        let m = if fb > 0 { (op.prev + op.out) >> (9 - fb) } else { 0 };
        let v = self.output(c, 0, p.wrapping_add(m as u32), trem);
        let op = &mut self.ch[c].op[0];
        op.prev = op.out;
        op.out = v;
        v
    }

    fn modulated(&mut self, c: usize, o: usize, m: i32, trem: u32, vibpos: u32) -> i32 {
        let p = self.advance(c, o, vibpos);
        self.output(c, o, p.wrapping_add(m as u32), trem)
    }

    fn two_op(&mut self, c: usize, trem: u32, vibpos: u32) -> i32 {
        if self.ch[c].op.iter().all(|o| o.eg == Eg::Off) {
            return 0;
        }
        let a = self.first(c, trem, vibpos);
        if self.ch[c].cnt {
            a + self.modulated(c, 1, 0, trem, vibpos)
        } else {
            self.modulated(c, 1, a, trem, vibpos)
        }
    }

    fn four_op(&mut self, c: usize, trem: u32, vibpos: u32) -> i32 {
        let d = c + 3;
        if self.ch[c].op.iter().chain(self.ch[d].op.iter()).all(|o| o.eg == Eg::Off) {
            return 0;
        }
        let s1 = self.first(c, trem, vibpos);
        match (self.ch[c].cnt, self.ch[d].cnt) {
            (false, false) => {
                let s2 = self.modulated(c, 1, s1, trem, vibpos);
                let s3 = self.modulated(d, 0, s2, trem, vibpos);
                self.modulated(d, 1, s3, trem, vibpos)
            }
            (true, false) => {
                let s2 = self.modulated(c, 1, 0, trem, vibpos);
                let s3 = self.modulated(d, 0, s2, trem, vibpos);
                s1 + self.modulated(d, 1, s3, trem, vibpos)
            }
            (false, true) => {
                let s2 = self.modulated(c, 1, s1, trem, vibpos);
                let s3 = self.modulated(d, 0, 0, trem, vibpos);
                s2 + self.modulated(d, 1, s3, trem, vibpos)
            }
            (true, true) => {
                let s2 = self.modulated(c, 1, 0, trem, vibpos);
                let s3 = self.modulated(d, 0, s2, trem, vibpos);
                s1 + s3 + self.modulated(d, 1, 0, trem, vibpos)
            }
        }
    }

    /// Channels 6-8 in rhythm mode: the five drums.
    fn rhythm_sample(&mut self, trem: u32, vibpos: u32, noise: u32) -> i32 {
        // Bass drum: channel 6 as a two-operator voice (its carrier alone
        // in AM mode).
        let a = self.first(6, trem, vibpos);
        let bd = if self.ch[6].cnt { self.modulated(6, 1, 0, trem, vibpos) } else { self.modulated(6, 1, a, trem, vibpos) };
        // Hi-hat and snare (channel 7), tom and cymbal (channel 8).
        let p13 = self.advance(7, 0, vibpos);
        let p16 = self.advance(7, 1, vibpos);
        let p14 = self.advance(8, 0, vibpos);
        let p17 = self.advance(8, 1, vibpos);
        let _ = p16;
        let b = |p: u32, n: u32| (p >> n) & 1;
        let rm = (b(p13, 2) ^ b(p13, 7)) | (b(p13, 3) ^ b(p17, 5)) | (b(p17, 3) ^ b(p17, 5));
        let hh_p = rm << 9 | if rm ^ noise != 0 { 0xd0 } else { 0x34 };
        let sd_p = b(p13, 8) << 9 | (b(p13, 8) ^ noise) << 8;
        let tc_p = rm << 9 | 0x80;
        let hh = self.output(7, 0, hh_p, trem);
        let sd = self.output(7, 1, sd_p, trem);
        let tom = self.output(8, 0, p14, trem);
        let tc = self.output(8, 1, tc_p, trem);
        (bd + hh + sd + tom + tc) * 2
    }
}

/// The channel (0-8 in a bank) and operator (0, 1) of an operator
/// register offset (00h-15h).
fn slot(off: u8) -> Option<(usize, usize)> {
    let s = match off {
        0x00..=0x05 => off,
        0x08..=0x0d => off - 2,
        0x10..=0x15 => off - 4,
        _ => return None,
    } as usize;
    Some((s % 3 + 3 * (s / 6), (s / 3) % 2))
}

fn set_key(op: &mut Op, bit: u8, on: bool) {
    let was = op.key != 0;
    if on {
        op.key |= bit;
    } else {
        op.key &= !bit;
    }
    let now = op.key != 0;
    if now && !was {
        op.phase = 0;
        op.eg = Eg::Attack;
    } else if was && !now && op.eg != Eg::Off {
        op.eg = Eg::Release;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine voice at A4: fnum 0x244 block 4 is 440 Hz on the chip.
    fn tone(o: &mut Opl, frames: usize) -> alloc::vec::Vec<[i32; 2]> {
        o.write(0x20, 0x01);
        o.write(0x23, 0x01);
        o.write(0x40, 0x3f);
        o.write(0x43, 0x00);
        o.write(0x60, 0xf0);
        o.write(0x63, 0xf0);
        o.write(0x80, 0x00);
        o.write(0x83, 0x00);
        o.write(0xc0, 0x01);
        o.write(0xa0, 0x44);
        o.write(0xb0, 0x32);
        let mut buf = alloc::vec![[0i32; 2]; frames];
        o.render(&mut buf, [256, 256]);
        buf
    }

    #[test]
    fn sine_at_440() {
        let mut o = Opl::new();
        let buf = tone(&mut o, RATE as usize);
        let ups = buf.windows(2).filter(|w| w[0][0] < 0 && w[1][0] >= 0).count();
        assert!((436..=444).contains(&ups), "{ups} rising zero crossings");
        let peak = buf.iter().map(|f| f[0].abs()).max().unwrap();
        assert!(peak > 3500 && peak <= 4200, "peak {peak}");
    }

    #[test]
    fn release_goes_silent() {
        let mut o = Opl::new();
        tone(&mut o, 100);
        o.write(0x80, 0x0f);
        o.write(0x83, 0x0f);
        o.write(0xb0, 0x12);
        let mut buf = alloc::vec![[0i32; 2]; RATE as usize / 2];
        o.render(&mut buf, [256, 256]);
        assert!(!o.active());
        assert_eq!(buf[buf.len() - 1][0], 0);
    }

    #[test]
    fn timer_detection() {
        // AdLib detection: reset, timer 1 at FFh started, 80 us later
        // both the IRQ and the timer 1 flags.
        let mut o = Opl::new();
        o.write(4, 0x60);
        o.write(4, 0x80);
        assert_eq!(o.status(0, false) & 0xe0, 0);
        o.write(2, 0xff);
        o.write(4, 0x21);
        let mut s = 0;
        for _ in 0..100 {
            s = o.status(0, false);
        }
        assert_eq!(s & 0xe0, 0xc0);
        assert_eq!(s & 0x06, 0x06);
        assert_eq!(o.status(0, true) & 0x06, 0);
    }
}
