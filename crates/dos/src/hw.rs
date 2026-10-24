//! The machine's memory and I/O space as the CPU sees them.

use alloc::vec;
use alloc::vec::Vec;

use crate::devices::{Cmos, Kbc, Pic, Pit, PIT_HZ};
use crate::host::DateTime;
use crate::video::Vga;

/// RAM below the end of the HMA.
pub const LOW_RAM: usize = 0x110000;
/// Extended memory (XMS, DPMI) starts here.
pub const EXT_BASE: u32 = 0x110000;

pub const EMS_PAGE: usize = 0x4000;
/// Segment of the EMS page frame.
pub const EMS_FRAME: u32 = 0xe000;

pub struct Hw {
    pub ram: Vec<u8>,
    /// Memory above 1 MB + 64 KB, grown as XMS and DPMI hand it out.
    pub ext: Vec<u8>,
    pub a20: bool,
    pub vga: Vga,
    pub pic: Pic,
    pub pit: Pit,
    pub kbc: Kbc,
    pub cmos: Cmos,
    /// Port 61h.
    pub port61: u8,
    /// EMS pages, and the logical page each frame window shows.
    pub ems: Vec<u8>,
    pub ems_map: [Option<u16>; 4],
    /// Upper memory C8000h-DFFFFh is RAM (DOS=UMB).
    pub umb: bool,
    /// The current time in PIT ticks (set by the machine between slices).
    pub pit_now: u64,
    /// Microseconds, for the VGA retrace and port 61h's refresh bit.
    pub us_now: u64,
    pub now: DateTime,
    /// Ports written that nothing emulates (each logged once).
    pub unknown_ports: Vec<u16>,
    /// The speaker state changed.
    pub speaker_changed: bool,
    /// Ports 3F8h etc.: reads give "no UART".
    pub com_present: bool,
}

impl Hw {
    pub fn new() -> Hw {
        Hw {
            ram: vec![0; LOW_RAM],
            ext: Vec::new(),
            a20: false,
            vga: Vga::new(),
            pic: Pic::default(),
            pit: Pit::new(),
            kbc: Kbc::new(),
            cmos: Cmos::default(),
            port61: 0,
            ems: Vec::new(),
            ems_map: [None; 4],
            umb: false,
            pit_now: 0,
            us_now: 0,
            now: DateTime::default(),
            unknown_ports: Vec::new(),
            speaker_changed: false,
            com_present: false,
        }
    }

    /// Make extended memory reach `end` (a linear address).
    pub fn grow_ext(&mut self, end: u32) {
        let need = end.saturating_sub(EXT_BASE) as usize;
        if self.ext.len() < need {
            self.ext.resize(need, 0);
        }
    }

    #[inline]
    fn ems_slot(&self, a: u32) -> Option<usize> {
        let w = ((a - (EMS_FRAME << 4)) / EMS_PAGE as u32) as usize;
        self.ems_map[w].map(|p| p as usize * EMS_PAGE + (a as usize & (EMS_PAGE - 1)))
    }

    #[inline]
    pub fn rd(&mut self, a: u32) -> u8 {
        let a = self.wrap(a);
        if a < 0xa0000 {
            return self.ram[a as usize];
        }
        if a < 0xc0000 {
            if a < 0xb0000 && self.vga.planar() {
                return self.vga.read(a - 0xa0000);
            }
            return self.ram[a as usize];
        }
        if (a as usize) < LOW_RAM {
            if (0xe0000..0xf0000).contains(&a) {
                if let Some(i) = self.ems_slot(a) {
                    return self.ems.get(i).copied().unwrap_or(0xff);
                }
            }
            return self.ram[a as usize];
        }
        self.ext.get((a - EXT_BASE) as usize).copied().unwrap_or(0xff)
    }

    #[inline]
    pub fn wr(&mut self, a: u32, v: u8) {
        let a = self.wrap(a);
        if a < 0xa0000 {
            self.ram[a as usize] = v;
            return;
        }
        if a < 0xc0000 {
            if a < 0xb0000 && self.vga.planar() {
                self.vga.write(a - 0xa0000, v);
                return;
            }
            self.ram[a as usize] = v;
            self.vga.dirty = true;
            return;
        }
        if (a as usize) < LOW_RAM {
            match a {
                0xe0000..=0xeffff => {
                    if let Some(i) = self.ems_slot(a) {
                        if let Some(m) = self.ems.get_mut(i) {
                            *m = v;
                        }
                    } else if self.umb {
                        self.ram[a as usize] = v;
                    }
                }
                0xc8000..=0xdffff if self.umb => self.ram[a as usize] = v,
                0x100000.. => self.ram[a as usize] = v,
                // ROM.
                _ => {}
            }
            return;
        }
        if let Some(m) = self.ext.get_mut((a - EXT_BASE) as usize) {
            *m = v;
        }
    }

    #[inline]
    fn wrap(&self, a: u32) -> u32 {
        if !self.a20 && (0x100000..0x200000).contains(&a) { a & 0xfffff } else { a }
    }

    /// Plain RAM access for the machine's own code (no device mapping).
    pub fn ram_slice(&self, a: u32, len: usize) -> &[u8] {
        &self.ram[a as usize..a as usize + len]
    }

    pub fn read_bytes(&mut self, a: u32, out: &mut [u8]) {
        for (i, b) in out.iter_mut().enumerate() {
            *b = self.rd(a.wrapping_add(i as u32));
        }
    }

    pub fn write_bytes(&mut self, a: u32, data: &[u8]) {
        for (i, b) in data.iter().enumerate() {
            self.wr(a.wrapping_add(i as u32), *b);
        }
    }

    pub fn rd16(&mut self, a: u32) -> u16 {
        self.rd(a) as u16 | (self.rd(a + 1) as u16) << 8
    }
    pub fn wr16(&mut self, a: u32, v: u16) {
        self.wr(a, v as u8);
        self.wr(a + 1, (v >> 8) as u8);
    }
    pub fn rd32(&mut self, a: u32) -> u32 {
        self.rd16(a) as u32 | (self.rd16(a + 2) as u32) << 16
    }
    pub fn wr32(&mut self, a: u32, v: u32) {
        self.wr16(a, v as u16);
        self.wr16(a + 2, (v >> 16) as u16);
    }

    /// A NUL-terminated string at `a` (at most `max` bytes).
    pub fn cstr(&mut self, a: u32, max: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..max {
            let b = self.rd(a + i as u32);
            if b == 0 {
                break;
            }
            v.push(b);
        }
        v
    }

    fn retrace(&self) -> (bool, bool) {
        // 70 Hz frames of 449 lines; vertical retrace in the last 2.
        let frame = 1_000_000 / 70;
        let t = self.us_now % frame;
        let vr = t >= frame - frame * 2 / 449 - 30;
        // Display enable off during horizontal retrace (a fifth of a
        // 31.8 us line), and during vertical retrace.
        let line = t * 449 / frame;
        let in_line = (t * 449) % frame * 32 / frame;
        let de = vr || in_line >= 25 || line >= 400;
        (vr, de)
    }

    pub fn io_in(&mut self, port: u16) -> u8 {
        match port {
            0x20 | 0x21 | 0xa0 | 0xa1 => self.pic.io_read(port),
            0x40..=0x42 => self.pit.io_read(port, self.pit_now),
            0x43 => 0xff,
            0x60 => {
                let v = self.kbc.read_data();
                v
            }
            0x61 => {
                let out2 = self.pit.ch[2].out(self.pit_now);
                let refresh = (self.us_now / 15) & 1 != 0;
                (self.port61 & 0x0f) | (refresh as u8) << 4 | (out2 as u8) << 5
            }
            0x64 => self.kbc.status(),
            0x70 => self.cmos.index,
            0x71 => {
                let now = self.now;
                self.cmos.read(&now)
            }
            0x92 => (self.a20 as u8) << 1,
            0x3b0..=0x3df => {
                let r = self.retrace();
                self.vga.io_read(port, r)
            }
            // Joystick: nothing connected.
            0x201 => 0xff,
            _ => 0xff,
        }
    }

    pub fn io_out(&mut self, port: u16, v: u8) {
        match port {
            0x20 | 0x21 | 0xa0 | 0xa1 => self.pic.io_write(port, v),
            0x40..=0x43 => {
                self.pit.io_write(port, v, self.pit_now);
                if port == 0x42 || (port == 0x43 && v >> 6 == 2) {
                    self.speaker_changed = true;
                }
            }
            0x60 => self.kbc.write_data(v),
            0x61 => {
                if (self.port61 ^ v) & 3 != 0 {
                    self.speaker_changed = true;
                }
                self.port61 = v;
                self.pit.ch[2].gate = v & 1 != 0;
            }
            0x64 => self.kbc.write_cmd(v),
            0x70 => self.cmos.index = v,
            0x71 => self.cmos.write(v),
            0x92 => self.a20 = v & 2 != 0,
            0x3b0..=0x3df => self.vga.io_write(port, v),
            // POST codes, DMA page registers, delay port.
            0x80..=0x8f | 0xed => {}
            _ => {
                if !self.unknown_ports.contains(&port) && self.unknown_ports.len() < 64 {
                    self.unknown_ports.push(port);
                }
            }
        }
        if let Some(a) = self.kbc.a20_request.take() {
            self.a20 = a;
        }
    }

    /// The PIT tick count for a host time in microseconds.
    pub fn pit_ticks(us: u64) -> u64 {
        (us as u128 * PIT_HZ as u128 / 1_000_000) as u64
    }

    /// Whether the speaker sounds, and at what frequency.
    pub fn speaker(&self) -> Option<u32> {
        if self.port61 & 3 == 3 && self.pit.ch[2].mode == 3 {
            let hz = self.pit.speaker_hz();
            if (20..20000).contains(&hz) {
                return Some(hz);
            }
        }
        None
    }
}

impl Default for Hw {
    fn default() -> Self {
        Self::new()
    }
}

impl x86emu::Bus for Hw {
    #[inline]
    fn read8(&mut self, a: u32) -> u8 {
        self.rd(a)
    }
    #[inline]
    fn write8(&mut self, a: u32, v: u8) {
        self.wr(a, v)
    }
    #[inline]
    fn read16(&mut self, a: u32) -> u16 {
        if a < 0x9ffff {
            let i = a as usize;
            return u16::from_le_bytes([self.ram[i], self.ram[i + 1]]);
        }
        self.rd(a) as u16 | (self.rd(a.wrapping_add(1)) as u16) << 8
    }
    #[inline]
    fn read32(&mut self, a: u32) -> u32 {
        if a < 0x9fffd {
            let i = a as usize;
            return u32::from_le_bytes([self.ram[i], self.ram[i + 1], self.ram[i + 2], self.ram[i + 3]]);
        }
        self.read16(a) as u32 | (self.read16(a.wrapping_add(2)) as u32) << 16
    }
    #[inline]
    fn write16(&mut self, a: u32, v: u16) {
        if a < 0x9ffff {
            let i = a as usize;
            self.ram[i..i + 2].copy_from_slice(&v.to_le_bytes());
            return;
        }
        self.wr(a, v as u8);
        self.wr(a.wrapping_add(1), (v >> 8) as u8);
    }
    #[inline]
    fn write32(&mut self, a: u32, v: u32) {
        if a < 0x9fffd {
            let i = a as usize;
            self.ram[i..i + 4].copy_from_slice(&v.to_le_bytes());
            return;
        }
        self.write16(a, v as u16);
        self.write16(a.wrapping_add(2), (v >> 16) as u16);
    }
    fn io_read(&mut self, port: u16, size: u8) -> u32 {
        match size {
            1 => self.io_in(port) as u32,
            2 => self.io_in(port) as u32 | (self.io_in(port.wrapping_add(1)) as u32) << 8,
            _ => (0..4).fold(0, |a, i| a | (self.io_in(port.wrapping_add(i)) as u32) << (i * 8)),
        }
    }
    fn io_write(&mut self, port: u16, size: u8, v: u32) {
        // Word writes to the VGA index ports set index and data at once.
        for i in 0..size as u16 {
            self.io_out(port.wrapping_add(i), (v >> (i * 8)) as u8);
        }
    }
    fn irq_pending(&mut self) -> bool {
        self.pic.pending()
    }
    fn irq_ack(&mut self) -> Option<u8> {
        self.pic.ack()
    }
}
