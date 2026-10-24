//! The PC's interrupt controllers, timer, keyboard controller and CMOS.

use alloc::collections::VecDeque;

/// One 8259A.
#[derive(Clone, Default)]
pub struct Pic8259 {
    pub irr: u8,
    pub isr: u8,
    pub imr: u8,
    pub base: u8,
    /// Initialisation: next ICW expected (0 = none).
    icw: u8,
    icw4: bool,
    read_isr: bool,
    auto_eoi: bool,
}

impl Pic8259 {
    fn write(&mut self, port: u8, v: u8) {
        if port == 0 {
            if v & 0x10 != 0 {
                // ICW1
                self.icw = 2;
                self.icw4 = v & 1 != 0;
                self.imr = 0;
                self.isr = 0;
                self.irr = 0;
                self.read_isr = false;
            } else if v & 0x08 == 0 {
                // OCW2: EOI
                match v >> 5 {
                    1 | 5 => {
                        // Non-specific: the highest priority in service.
                        if self.isr != 0 {
                            self.isr &= self.isr - 1;
                        }
                    }
                    3 | 7 => self.isr &= !(1 << (v & 7)),
                    _ => {}
                }
            } else if v & 0x02 != 0 {
                // OCW3
                self.read_isr = v & 1 != 0;
            }
        } else {
            match self.icw {
                2 => {
                    self.base = v & 0xf8;
                    self.icw = 3;
                }
                3 => {
                    self.icw = if self.icw4 { 4 } else { 0 };
                }
                4 => {
                    self.auto_eoi = v & 2 != 0;
                    self.icw = 0;
                }
                _ => self.imr = v,
            }
        }
    }

    fn read(&self, port: u8) -> u8 {
        if port == 0 {
            if self.read_isr { self.isr } else { self.irr }
        } else {
            self.imr
        }
    }

    /// The highest-priority request that may interrupt now.
    fn pending(&self) -> Option<u8> {
        let req = self.irr & !self.imr;
        if req == 0 {
            return None;
        }
        let irq = req.trailing_zeros() as u8;
        // Lower numbers have priority; one in service at the same or a
        // higher priority holds it off.
        if self.isr != 0 && self.isr.trailing_zeros() as u8 <= irq {
            return None;
        }
        Some(irq)
    }

    fn ack(&mut self, irq: u8) {
        self.irr &= !(1 << irq);
        if !self.auto_eoi {
            self.isr |= 1 << irq;
        }
    }
}

/// The master/slave pair.
#[derive(Clone)]
pub struct Pic {
    pub m: Pic8259,
    pub s: Pic8259,
}

impl Default for Pic {
    fn default() -> Self {
        let mut p = Pic { m: Pic8259::default(), s: Pic8259::default() };
        p.m.base = 0x08;
        p.s.base = 0x70;
        p
    }
}

impl Pic {
    pub fn raise(&mut self, irq: u8) {
        if irq < 8 {
            self.m.irr |= 1 << irq;
        } else {
            self.s.irr |= 1 << (irq - 8);
            self.m.irr |= 1 << 2;
        }
    }

    #[inline]
    pub fn pending(&self) -> bool {
        if self.m.irr & !self.m.imr == 0 {
            return false;
        }
        match self.m.pending() {
            Some(2) => self.s.pending().is_some(),
            Some(_) => true,
            None => false,
        }
    }

    pub fn ack(&mut self) -> Option<u8> {
        let irq = self.m.pending()?;
        if irq == 2 {
            let s = self.s.pending()?;
            self.s.ack(s);
            self.m.ack(2);
            if self.s.irr != 0 {
                self.m.irr |= 1 << 2;
            }
            return Some(self.s.base + s);
        }
        self.m.ack(irq);
        Some(self.m.base + irq)
    }

    pub fn io_write(&mut self, port: u16, v: u8) {
        match port {
            0x20 | 0x21 => self.m.write((port & 1) as u8, v),
            _ => self.s.write((port & 1) as u8, v),
        }
    }

    pub fn io_read(&self, port: u16) -> u8 {
        match port {
            0x20 | 0x21 => self.m.read((port & 1) as u8),
            _ => self.s.read((port & 1) as u8),
        }
    }
}

/// The PIT's input clock.
pub const PIT_HZ: u64 = 1_193_182;

#[derive(Clone, Copy, Default)]
pub struct PitChannel {
    pub mode: u8,
    /// Reload value (0 = 65536).
    pub reload: u32,
    /// 1 low byte, 2 high byte, 3 low then high.
    access: u8,
    write_hi: bool,
    read_hi: bool,
    latched: Option<u16>,
    status_latch: Option<u8>,
    /// Counting started at this PIT tick.
    pub start: u64,
    /// Waiting for the second byte of the reload value.
    partial: Option<u8>,
    /// The gate (channel 2: port 61h bit 0).
    pub gate: bool,
}

impl PitChannel {
    pub fn period(&self) -> u64 {
        if self.reload == 0 { 65536 } else { self.reload as u64 }
    }

    /// The counter at PIT tick `now`.
    fn count(&self, now: u64) -> u16 {
        let p = self.period();
        let e = now.saturating_sub(self.start);
        match self.mode {
            0 | 1 => (p.saturating_sub(e)) as u16,
            3 => {
                // Square wave: counts down by two, twice per period.
                let half = p.div_ceil(2).max(1);
                let r = e % p;
                let r = if r < half { r } else { r - half };
                (p - (r * 2) % p) as u16
            }
            _ => (p - e % p) as u16,
        }
    }

    /// The output pin at PIT tick `now`.
    pub fn out(&self, now: u64) -> bool {
        let p = self.period();
        let e = now.saturating_sub(self.start);
        match self.mode {
            0 => e >= p,
            2 => e % p != p - 1,
            3 => e % p < p.div_ceil(2),
            _ => true,
        }
    }
}

#[derive(Clone, Default)]
pub struct Pit {
    pub ch: [PitChannel; 3],
    /// Channel 0 interrupts delivered up to this PIT tick.
    pub irq_done: u64,
    /// The programmed rate changed: the machine re-evaluates channel 0.
    pub changed: bool,
}

impl Pit {
    pub fn new() -> Pit {
        let mut p = Pit::default();
        for c in p.ch.iter_mut() {
            c.mode = 3;
            c.access = 3;
        }
        p.ch[0].gate = true;
        p.ch[1].gate = true;
        p.ch[1].reload = 18;
        p
    }

    pub fn io_write(&mut self, port: u16, v: u8, now: u64) {
        if port == 0x43 {
            let c = (v >> 6) as usize;
            if c == 3 {
                // Read-back.
                for i in 0..3 {
                    if v & (2 << i) != 0 {
                        if v & 0x20 == 0 {
                            self.ch[i].latched = Some(self.ch[i].count(now));
                        }
                        if v & 0x10 == 0 {
                            let ch = &self.ch[i];
                            let st = (ch.out(now) as u8) << 7 | ch.access << 4 | ch.mode << 1;
                            self.ch[i].status_latch = Some(st);
                        }
                    }
                }
                return;
            }
            let acc = (v >> 4) & 3;
            if acc == 0 {
                self.ch[c].latched = Some(self.ch[c].count(now));
                return;
            }
            let ch = &mut self.ch[c];
            ch.access = acc;
            ch.mode = (v >> 1) & 7;
            if ch.mode > 5 {
                ch.mode -= 4;
            }
            ch.write_hi = false;
            ch.read_hi = false;
            ch.partial = None;
            return;
        }
        let c = (port - 0x40) as usize;
        let ch = &mut self.ch[c];
        let done = match ch.access {
            1 => {
                ch.reload = v as u32;
                true
            }
            2 => {
                ch.reload = (v as u32) << 8;
                true
            }
            _ => match ch.partial.take() {
                None => {
                    ch.partial = Some(v);
                    false
                }
                Some(lo) => {
                    ch.reload = lo as u32 | (v as u32) << 8;
                    true
                }
            },
        };
        if done {
            ch.start = now;
            if c == 0 {
                self.irq_done = now;
                self.changed = true;
            }
        }
    }

    pub fn io_read(&mut self, port: u16, now: u64) -> u8 {
        let c = (port - 0x40) as usize;
        let ch = &mut self.ch[c];
        if let Some(s) = ch.status_latch.take() {
            return s;
        }
        let v = match ch.latched {
            Some(l) => l,
            None => ch.count(now),
        };
        let r = match ch.access {
            1 => v as u8,
            2 => (v >> 8) as u8,
            _ => {
                let hi = ch.read_hi;
                ch.read_hi = !hi;
                if hi {
                    ch.latched = None;
                    (v >> 8) as u8
                } else {
                    v as u8
                }
            }
        };
        if ch.access != 3 {
            ch.latched = None;
        }
        r
    }

    /// Channel 2's frequency while it drives the speaker.
    pub fn speaker_hz(&self) -> u32 {
        (PIT_HZ / self.ch[2].period()) as u32
    }
}

/// The 8042 keyboard controller.
#[derive(Clone, Default)]
pub struct Kbc {
    /// Bytes from the keyboard not yet in the output buffer, with the
    /// character the host's layout gives.
    pub queue: VecDeque<(u8, u8)>,
    /// The output buffer (last byte, kept for re-reads), and whether it
    /// is full.
    pub data: u8,
    pub data_ch: u8,
    pub full: bool,
    /// A command waiting for its data byte (written to port 60h).
    pending: Option<u8>,
    /// Keyboard command waiting for its parameter.
    kbd_pending: Option<u8>,
    pub disabled: bool,
    /// The command byte.
    pub cmd_byte: u8,
    /// The controller asked for A20 to change (output port bit 1).
    pub a20_request: Option<bool>,
    /// A reset was requested (command FEh).
    pub reset: bool,
}

impl Kbc {
    pub fn new() -> Kbc {
        Kbc { cmd_byte: 0x45, ..Default::default() }
    }

    /// Move the next byte into the output buffer; true if IRQ 1 should be
    /// raised.
    pub fn refill(&mut self) -> bool {
        if self.full || self.disabled {
            return false;
        }
        let Some((b, ch)) = self.queue.pop_front() else { return false };
        self.data = b;
        self.data_ch = ch;
        self.full = true;
        self.cmd_byte & 1 != 0
    }

    pub fn read_data(&mut self) -> u8 {
        self.full = false;
        self.data
    }

    pub fn status(&self) -> u8 {
        // System flag, and output buffer full.
        0x14 | self.full as u8
    }

    fn reply(&mut self, b: u8) {
        self.queue.push_front((b, 0));
    }

    pub fn write_cmd(&mut self, v: u8) {
        match v {
            0x20 => {
                let c = self.cmd_byte;
                self.reply(c);
            }
            0x60 | 0xd1 | 0xd2 => self.pending = Some(v),
            0xad => self.disabled = true,
            0xae => self.disabled = false,
            0xaa => self.reply(0x55),
            0xab => self.reply(0x00),
            0xc0 => self.reply(0x00),
            0xd0 => self.reply(0x02),
            0xdd => self.a20_request = Some(false),
            0xdf => self.a20_request = Some(true),
            0xfe => self.reset = true,
            _ => {}
        }
    }

    pub fn write_data(&mut self, v: u8) {
        match self.pending.take() {
            Some(0x60) => self.cmd_byte = v,
            Some(0xd1) => {
                self.a20_request = Some(v & 2 != 0);
                if v & 1 == 0 {
                    self.reset = true;
                }
            }
            Some(0xd2) => self.reply(v),
            _ => {
                // A keyboard command.
                if self.kbd_pending.take().is_some() {
                    self.reply(0xfa);
                    return;
                }
                match v {
                    0xed | 0xf3 => {
                        self.kbd_pending = Some(v);
                        self.reply(0xfa);
                    }
                    0xee => self.reply(0xee),
                    0xf2 => {
                        self.queue.push_front((0x83, 0));
                        self.queue.push_front((0xab, 0));
                        self.reply(0xfa);
                    }
                    0xff => {
                        self.queue.push_front((0xaa, 0));
                        self.reply(0xfa);
                    }
                    _ => self.reply(0xfa),
                }
            }
        }
    }
}

/// CMOS RAM and the real-time clock.
#[derive(Clone)]
pub struct Cmos {
    pub index: u8,
    pub ram: [u8; 128],
}

impl Default for Cmos {
    fn default() -> Self {
        let mut ram = [0u8; 128];
        ram[0x0a] = 0x26;
        ram[0x0b] = 0x02;
        ram[0x0d] = 0x80;
        // No floppies, no hard disk (the BIOS denies direct disk access),
        // 640 KB base memory, VGA, coprocessor flag set by the machine.
        ram[0x14] = 0x00;
        ram[0x15] = 0x80;
        ram[0x16] = 0x02;
        ram[0x32] = 0x19;
        Cmos { index: 0, ram }
    }
}

fn bcd(v: u8) -> u8 {
    (v / 10) << 4 | (v % 10)
}

impl Cmos {
    pub fn read(&mut self, now: &crate::host::DateTime) -> u8 {
        let i = self.index & 0x7f;
        match i {
            0 => bcd(now.second),
            2 => bcd(now.minute),
            4 => bcd(now.hour),
            6 => now.weekday + 1,
            7 => bcd(now.day),
            8 => bcd(now.month),
            9 => bcd((now.year % 100) as u8),
            0x32 => bcd((now.year / 100) as u8),
            0x0c => {
                let v = self.ram[0x0c];
                self.ram[0x0c] = 0;
                v
            }
            _ => self.ram[i as usize],
        }
    }

    pub fn write(&mut self, v: u8) {
        let i = (self.index & 0x7f) as usize;
        if !matches!(i, 0..=9 | 0x0c | 0x0d) {
            self.ram[i] = v;
        }
    }
}
