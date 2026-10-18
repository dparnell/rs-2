//! PCI configuration space (legacy 0xCF8/0xCFC mechanism) and device
//! enumeration.

use alloc::vec::Vec;

use crate::arch::{inl, outl};
use crate::sync::SpinLock;

#[derive(Clone, Copy, Debug)]
pub struct Device {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub progif: u8,
    pub irq: u8,
}

static LOCK: SpinLock<()> = SpinLock::new(());

fn addr(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    0x8000_0000 | (bus as u32) << 16 | (dev as u32) << 11 | (func as u32) << 8 | (off as u32 & 0xfc)
}

pub fn read32(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    let _g = LOCK.lock();
    unsafe {
        outl(0xcf8, addr(bus, dev, func, off));
        inl(0xcfc)
    }
}

pub fn write32(bus: u8, dev: u8, func: u8, off: u8, v: u32) {
    let _g = LOCK.lock();
    unsafe {
        outl(0xcf8, addr(bus, dev, func, off));
        outl(0xcfc, v);
    }
}

impl Device {
    pub fn read(&self, off: u8) -> u32 {
        read32(self.bus, self.dev, self.func, off)
    }

    pub fn write(&self, off: u8, v: u32) {
        write32(self.bus, self.dev, self.func, off, v)
    }

    /// Physical address of memory BAR `n` (handles 64-bit BARs).
    pub fn bar(&self, n: u8) -> u64 {
        let lo = self.read(0x10 + n * 4);
        if lo & 1 != 0 {
            return (lo & !3) as u64; // I/O BAR
        }
        let mut a = (lo & !0xf) as u64;
        if (lo >> 1) & 3 == 2 {
            a |= (self.read(0x14 + n * 4) as u64) << 32;
        }
        a
    }

    /// Enable memory space, I/O space and bus mastering.
    pub fn enable(&self) {
        let cmd = self.read(0x04);
        self.write(0x04, (cmd & 0xffff) | 0x7);
    }
}

pub fn enumerate() -> Vec<Device> {
    let mut v = Vec::new();
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            let id = read32(bus, dev, 0, 0);
            if id & 0xffff == 0xffff {
                continue;
            }
            let multi = read32(bus, dev, 0, 0x0c) >> 16 & 0x80 != 0;
            for func in 0..if multi { 8 } else { 1 } {
                let id = read32(bus, dev, func, 0);
                if id & 0xffff == 0xffff {
                    continue;
                }
                let class = read32(bus, dev, func, 0x08);
                let irq = read32(bus, dev, func, 0x3c) as u8;
                v.push(Device {
                    bus,
                    dev,
                    func,
                    vendor: id as u16,
                    device: (id >> 16) as u16,
                    class: (class >> 24) as u8,
                    subclass: (class >> 16) as u8,
                    progif: (class >> 8) as u8,
                    irq,
                });
            }
        }
    }
    v
}
