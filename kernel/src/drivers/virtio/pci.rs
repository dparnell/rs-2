//! The virtio PCI transport.
//!
//! Modern (virtio 1.x) devices describe their register blocks with
//! vendor-specific PCI capabilities: common configuration (features,
//! status, queue setup), notification area, ISR status and the
//! device-specific configuration, each a window into a memory BAR.
//! Legacy (0.9.5) and transitional devices driven the old way have all of
//! it in I/O BAR 0: host features, guest features, queue PFN, queue size,
//! queue select, queue notify, status, ISR, then the device configuration
//! (at 20, as MSI-X stays disabled). A transitional device with modern
//! capabilities is driven as a modern one.
//!
//! INTx is masked through the PCI command register. Queues a driver
//! asks interrupts for ([`Device::queue_irq`]) get an MSI-X table entry
//! each (entry 0 is the configuration-change vector) and an [`Event`]
//! the handler signals; the vector is set through queue_msix_vector
//! (modern) or the legacy MSI-X header fields, which move the legacy
//! device configuration from offset 20 to 24 as soon as MSI-X is on.
//! Devices without MSI-X, and every queue with `nomsi`, are polled.

use core::ptr::{read_volatile, write_volatile};

use super::queue::{Reg, Virtqueue};
use super::{ACKNOWLEDGE, DRIVER, DRIVER_OK, FAILED, FEATURES_OK, F_VERSION_1};
use crate::arch::{inb, inl, inw, outb, outl, outw};
use crate::drivers::pci as bus;
use crate::mm::mmio;
use crate::msi::{Event, MsiX};

// Capability types (cfg_type).
const CAP_COMMON: u8 = 1;
const CAP_NOTIFY: u8 = 2;
const CAP_ISR: u8 = 3;
const CAP_DEVICE: u8 = 4;

// Common configuration (struct virtio_pci_common_cfg).
const DEVICE_FEATURE_SELECT: usize = 0x00;
const DEVICE_FEATURE: usize = 0x04;
const DRIVER_FEATURE_SELECT: usize = 0x08;
const DRIVER_FEATURE: usize = 0x0c;
const MSIX_CONFIG: usize = 0x10;
const NUM_QUEUES: usize = 0x12;
const DEVICE_STATUS: usize = 0x14;
const CONFIG_GENERATION: usize = 0x15;
const QUEUE_SELECT: usize = 0x16;
const QUEUE_SIZE: usize = 0x18;
const QUEUE_MSIX_VECTOR: usize = 0x1a;
const QUEUE_ENABLE: usize = 0x1c;
const QUEUE_NOTIFY_OFF: usize = 0x1e;
const QUEUE_DESC: usize = 0x20;
const QUEUE_DRIVER: usize = 0x28;
const QUEUE_DEVICE: usize = 0x30;

// Legacy I/O registers.
const L_HOST_FEATURES: u16 = 0;
const L_GUEST_FEATURES: u16 = 4;
const L_QUEUE_PFN: u16 = 8;
const L_QUEUE_SIZE: u16 = 12;
const L_QUEUE_SELECT: u16 = 14;
const L_QUEUE_NOTIFY: u16 = 16;
const L_STATUS: u16 = 18;
const L_CONFIG: u16 = 20;
// Legacy MSI-X header (present while MSI-X is enabled).
const L_CONFIG_VECTOR: u16 = 20;
const L_QUEUE_VECTOR: u16 = 22;
const L_CONFIG_MSIX: u16 = 24;
/// No MSI-X vector.
const NO_VECTOR: u16 = 0xffff;

/// Largest queue a driver asks for on a modern device.
const MAX_QUEUE: u16 = 256;

enum Regs {
    Modern { common: *mut u8, notify: *mut u8, notify_mult: u32, device: *mut u8 },
    Legacy { io: u16 },
}

/// A virtio function and its transport.
pub struct Device {
    pub pci: bus::Device,
    regs: Regs,
    /// Features accepted by [`Device::negotiate`].
    pub features: u64,
    /// MSI-X table, once a queue asked for interrupts; next free entry.
    msix: Option<MsiX>,
    next_entry: u16,
}

unsafe impl Send for Device {}

fn cfg8(d: &bus::Device, off: u8) -> u8 {
    (d.read(off & !3) >> ((off & 3) * 8)) as u8
}

fn cfg32(d: &bus::Device, off: u8) -> u32 {
    d.read(off)
}

/// The virtio capabilities of `d`: (type, BAR, offset, length, extra).
fn capabilities(d: &bus::Device) -> alloc::vec::Vec<(u8, u8, u32, u32, u32)> {
    let mut v = alloc::vec::Vec::new();
    if d.read(0x04) >> 16 & 0x10 == 0 {
        return v;
    }
    let mut off = cfg8(d, 0x34) & !3;
    let mut guard = 0;
    while off >= 0x40 && guard < 48 {
        guard += 1;
        if cfg8(d, off) == 0x09 {
            let typ = cfg8(d, off + 3);
            let bar = cfg8(d, off + 4);
            let extra = if typ == CAP_NOTIFY { cfg32(d, off + 16) } else { 0 };
            v.push((typ, bar, cfg32(d, off + 8), cfg32(d, off + 12), extra));
        }
        off = cfg8(d, off + 1) & !3;
    }
    v
}

/// Map the window a capability describes, if it lies in a memory BAR.
fn map_cap(d: &bus::Device, bar: u8, offset: u32, len: u32) -> Option<*mut u8> {
    if bar > 5 || d.read(0x10 + bar * 4) & 1 != 0 {
        return None;
    }
    let base = d.bar(bar);
    if base == 0 || len == 0 {
        return None;
    }
    Some(mmio::map(base + offset as u64, len as u64))
}

impl Device {
    /// Find the transport of a virtio function and enable it (memory and
    /// I/O decoding, bus mastering, INTx masked).
    pub fn new(d: &bus::Device) -> Option<Device> {
        d.enable();
        d.write(0x04, (d.read(0x04) & 0xffff) | 1 << 10);
        let caps = capabilities(d);
        let find = |t: u8| caps.iter().find(|c| c.0 == t && c.1 <= 5 && d.read(0x10 + c.1 * 4) & 1 == 0).copied();
        if let (Some(c), Some(n), Some(_)) = (find(CAP_COMMON), find(CAP_NOTIFY), find(CAP_ISR)) {
            let common = map_cap(d, c.1, c.2, c.3)?;
            let notify = map_cap(d, n.1, n.2, n.3)?;
            let device = find(CAP_DEVICE).and_then(|x| map_cap(d, x.1, x.2, x.3)).unwrap_or(core::ptr::null_mut());
            let regs = Regs::Modern { common, notify, notify_mult: n.4, device };
            return Some(Device { pci: *d, regs, features: 0, msix: None, next_entry: 1 });
        }
        // Legacy interface: transitional IDs with an I/O BAR 0.
        let bar0 = d.read(0x10);
        if (0x1000..=0x103f).contains(&d.device) && bar0 & 1 != 0 && bar0 & !3 != 0 {
            return Some(Device { pci: *d, regs: Regs::Legacy { io: (bar0 & 0xfffc) as u16 }, features: 0, msix: None, next_entry: 1 });
        }
        None
    }

    pub fn name(&self) -> alloc::string::String {
        alloc::format!("{:02x}:{:02x}.{}", self.pci.bus, self.pci.dev, self.pci.func)
    }

    pub fn is_legacy(&self) -> bool {
        matches!(self.regs, Regs::Legacy { .. })
    }

    fn common(&self) -> *mut u8 {
        match self.regs {
            Regs::Modern { common, .. } => common,
            Regs::Legacy { .. } => core::ptr::null_mut(),
        }
    }

    fn c8(&self, off: usize) -> u8 {
        unsafe { read_volatile(self.common().add(off)) }
    }

    fn c16(&self, off: usize) -> u16 {
        unsafe { read_volatile(self.common().add(off) as *const u16) }
    }

    fn c32(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.common().add(off) as *const u32) }
    }

    fn w8(&self, off: usize, v: u8) {
        unsafe { write_volatile(self.common().add(off), v) }
    }

    fn w16(&self, off: usize, v: u16) {
        unsafe { write_volatile(self.common().add(off) as *mut u16, v) }
    }

    fn w32(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.common().add(off) as *mut u32, v) }
    }

    /// 64-bit fields as two 32-bit writes, low half first.
    fn w64(&self, off: usize, v: u64) {
        self.w32(off, v as u32);
        self.w32(off + 4, (v >> 32) as u32);
    }

    /// The device status register (also used to pace polling loops).
    pub fn status_reg(&self) -> Reg {
        match self.regs {
            Regs::Modern { common, .. } => Reg::Mmio(unsafe { common.add(DEVICE_STATUS) }),
            Regs::Legacy { io } => Reg::Port(io + L_STATUS),
        }
    }

    pub fn status(&self) -> u8 {
        self.status_reg().read8()
    }

    fn set_status(&self, v: u8) {
        match self.regs {
            Regs::Modern { .. } => self.w8(DEVICE_STATUS, v),
            Regs::Legacy { io } => unsafe { outb(io + L_STATUS, v) },
        }
    }

    fn add_status(&self, bits: u8) {
        self.set_status(self.status() | bits);
    }

    /// Reset the device; a modern device has finished when status reads 0.
    pub fn reset(&self) {
        self.set_status(0);
        super::wait(1000, || {}, || self.status() == 0);
    }

    fn device_features(&self) -> u64 {
        match self.regs {
            Regs::Modern { .. } => {
                self.w32(DEVICE_FEATURE_SELECT, 0);
                let lo = self.c32(DEVICE_FEATURE) as u64;
                self.w32(DEVICE_FEATURE_SELECT, 1);
                lo | (self.c32(DEVICE_FEATURE) as u64) << 32
            }
            Regs::Legacy { io } => unsafe { inl(io + L_HOST_FEATURES) as u64 },
        }
    }

    fn set_driver_features(&self, f: u64) {
        match self.regs {
            Regs::Modern { .. } => {
                self.w32(DRIVER_FEATURE_SELECT, 0);
                self.w32(DRIVER_FEATURE, f as u32);
                self.w32(DRIVER_FEATURE_SELECT, 1);
                self.w32(DRIVER_FEATURE, (f >> 32) as u32);
            }
            Regs::Legacy { io } => unsafe { outl(io + L_GUEST_FEATURES, f as u32) },
        }
    }

    /// Reset, acknowledge and negotiate: accept the device's features that
    /// are in `supported` (VERSION_1 is added for modern devices). Returns
    /// the device's full feature set, or None if it refused ours.
    pub fn negotiate(&mut self, supported: u64) -> Option<u64> {
        self.reset();
        self.add_status(ACKNOWLEDGE);
        self.add_status(DRIVER);
        let offered = self.device_features();
        let legacy = self.is_legacy();
        let want = if legacy { offered & supported & 0xffff_ffff } else { offered & (supported | F_VERSION_1) };
        if !legacy && want & F_VERSION_1 == 0 {
            crate::println!("virtio: {}: device does not offer VIRTIO_F_VERSION_1", self.name());
            self.fail();
            return None;
        }
        self.set_driver_features(want);
        if !legacy {
            self.add_status(FEATURES_OK);
            if self.status() & FEATURES_OK == 0 {
                crate::println!("virtio: {}: features {want:#x} not accepted", self.name());
                self.fail();
                return None;
            }
        }
        self.features = want;
        Some(offered)
    }

    pub fn has(&self, feature: u64) -> bool {
        self.features & feature != 0
    }

    /// The device may run: all queues are set up.
    pub fn ready(&self) {
        self.add_status(DRIVER_OK);
    }

    /// Give up on the device.
    pub fn fail(&self) {
        self.add_status(FAILED);
    }

    pub fn num_queues(&self) -> u16 {
        match self.regs {
            Regs::Modern { .. } => self.c16(NUM_QUEUES),
            Regs::Legacy { .. } => u16::MAX,
        }
    }

    /// Legacy device configuration offset (moves while MSI-X is on).
    fn l_config(&self) -> u16 {
        if self.msix.is_some() { L_CONFIG_MSIX } else { L_CONFIG }
    }

    /// The MSI-X table, enabled on first use (None without MSI-X).
    fn msix(&mut self) -> Option<()> {
        if self.msix.is_none() {
            let m = MsiX::find(&self.pci)?;
            if m.size() < 2 {
                return None;
            }
            m.enable();
            self.msix = Some(m);
            // Entry 0: configuration changes, unassigned until asked for.
            self.set_config_vector(NO_VECTOR);
        }
        Some(())
    }

    fn set_config_vector(&self, entry: u16) -> u16 {
        match self.regs {
            Regs::Modern { .. } => {
                self.w16(MSIX_CONFIG, entry);
                self.c16(MSIX_CONFIG)
            }
            Regs::Legacy { io } => unsafe {
                outw(io + L_CONFIG_VECTOR, entry);
                inw(io + L_CONFIG_VECTOR)
            },
        }
    }

    /// Run `hook` in interrupt context when the device configuration
    /// changes (link state, ...). False without MSI-X.
    pub fn config_irq(&mut self, hook: fn()) -> bool {
        if self.msix().is_none() {
            return false;
        }
        let ev = Event::leak();
        ev.set_hook(hook);
        let Some(v) = ev.vector() else { return false };
        self.msix.as_ref().unwrap().set(0, v);
        self.set_config_vector(0) == 0
    }

    /// Point the selected queue at an MSI-X entry: the event its handler
    /// signals, or None (the queue is then polled).
    fn queue_vector(&mut self) -> Option<&'static Event> {
        let entry = self.next_entry;
        self.msix()?;
        if entry >= self.msix.as_ref()?.size() {
            return None;
        }
        let ev = Event::leak();
        let v = ev.vector()?;
        self.msix.as_ref()?.set(entry, v);
        let got = match self.regs {
            Regs::Modern { .. } => {
                self.w16(QUEUE_MSIX_VECTOR, entry);
                self.c16(QUEUE_MSIX_VECTOR)
            }
            Regs::Legacy { io } => unsafe {
                outw(io + L_QUEUE_VECTOR, entry);
                inw(io + L_QUEUE_VECTOR)
            },
        };
        if got != entry {
            // The device could not allocate it: poll this queue.
            self.msix.as_ref()?.mask(entry, true);
            return None;
        }
        self.next_entry += 1;
        crate::println!("virtio: {}: queue {} on MSI-X vector {v:#x}", self.name(), self.queue_selected());
        Some(ev)
    }

    fn queue_selected(&self) -> u16 {
        match self.regs {
            Regs::Modern { .. } => self.c16(QUEUE_SELECT),
            Regs::Legacy { io } => unsafe { inw(io + L_QUEUE_SELECT) },
        }
    }

    /// Set up virtqueue `index` with up to `want` entries (legacy devices
    /// dictate the size), polled. None if the device has no such queue.
    pub fn queue(&mut self, index: u16, want: u16) -> Option<Virtqueue> {
        self.queue_with(index, want, false)
    }

    /// [`Device::queue`] with used-buffer interrupts through MSI-X when the
    /// device has it (and `nomsi` is not given).
    pub fn queue_irq(&mut self, index: u16, want: u16) -> Option<Virtqueue> {
        self.queue_with(index, want, true)
    }

    fn queue_with(&mut self, index: u16, want: u16, irq: bool) -> Option<Virtqueue> {
        let pace = self.status_reg();
        match self.regs {
            Regs::Modern { notify, notify_mult, .. } => {
                if index >= self.num_queues() {
                    return None;
                }
                self.w16(QUEUE_SELECT, index);
                let max = self.c16(QUEUE_SIZE);
                if max == 0 {
                    return None;
                }
                let mut n = 1u16;
                while n * 2 <= max.min(want.clamp(1, MAX_QUEUE)) {
                    n *= 2;
                }
                let q = Virtqueue::new(index, n, Reg::Mmio(core::ptr::null_mut()), pace)?;
                self.w16(QUEUE_SIZE, n);
                self.w64(QUEUE_DESC, q.desc_addr());
                self.w64(QUEUE_DRIVER, q.avail_addr());
                self.w64(QUEUE_DEVICE, q.used_addr());
                let off = self.c16(QUEUE_NOTIFY_OFF) as usize * notify_mult as usize;
                let ev = if irq { self.queue_vector() } else { None };
                self.w16(QUEUE_ENABLE, 1);
                Some(q.with_notify(Reg::Mmio(unsafe { notify.add(off) })).with_event(ev))
            }
            Regs::Legacy { io } => unsafe {
                outw(io + L_QUEUE_SELECT, index);
                let n = inw(io + L_QUEUE_SIZE);
                if n == 0 || !n.is_power_of_two() {
                    return None;
                }
                let q = Virtqueue::new(index, n, Reg::Port(io + L_QUEUE_NOTIFY), pace)?;
                if q.desc_addr() >> 12 > u32::MAX as u64 {
                    return None;
                }
                outl(io + L_QUEUE_PFN, (q.desc_addr() >> 12) as u32);
                let ev = if irq { self.queue_vector() } else { None };
                Some(q.with_event(ev))
            },
        }
    }

    /// Read `N` bytes of device configuration at `off`, consistently
    /// (retrying while the configuration generation changes).
    fn cfg_bytes<const N: usize>(&self, off: usize) -> [u8; N] {
        let mut b = [0u8; N];
        match self.regs {
            Regs::Modern { device, .. } => {
                if device.is_null() {
                    return b;
                }
                loop {
                    let generation = self.c8(CONFIG_GENERATION);
                    // Natural-width accesses where possible.
                    let mut i = 0;
                    while i < N {
                        let p = unsafe { device.add(off + i) };
                        if N - i >= 4 && (off + i) % 4 == 0 {
                            let v = unsafe { read_volatile(p as *const u32) };
                            b[i..i + 4].copy_from_slice(&v.to_le_bytes());
                            i += 4;
                        } else if N - i >= 2 && (off + i) % 2 == 0 {
                            let v = unsafe { read_volatile(p as *const u16) };
                            b[i..i + 2].copy_from_slice(&v.to_le_bytes());
                            i += 2;
                        } else {
                            b[i] = unsafe { read_volatile(p) };
                            i += 1;
                        }
                    }
                    if self.c8(CONFIG_GENERATION) == generation {
                        return b;
                    }
                }
            }
            Regs::Legacy { io } => {
                for (i, x) in b.iter_mut().enumerate() {
                    *x = unsafe { inb(io + self.l_config() + (off + i) as u16) };
                }
                b
            }
        }
    }

    pub fn cfg8(&self, off: usize) -> u8 {
        self.cfg_bytes::<1>(off)[0]
    }

    pub fn cfg16(&self, off: usize) -> u16 {
        u16::from_le_bytes(self.cfg_bytes::<2>(off))
    }

    pub fn cfg32(&self, off: usize) -> u32 {
        u32::from_le_bytes(self.cfg_bytes::<4>(off))
    }

    pub fn cfg64(&self, off: usize) -> u64 {
        u64::from_le_bytes(self.cfg_bytes::<8>(off))
    }

    /// Read `out.len()` bytes of device configuration (byte accesses).
    pub fn cfg_read(&self, off: usize, out: &mut [u8]) {
        for (i, x) in out.iter_mut().enumerate() {
            *x = self.cfg8(off + i);
        }
    }

    pub fn cfg_write8(&self, off: usize, v: u8) {
        match self.regs {
            Regs::Modern { device, .. } => {
                if !device.is_null() {
                    unsafe { write_volatile(device.add(off), v) }
                }
            }
            Regs::Legacy { io } => unsafe { outb(io + self.l_config() + off as u16, v) },
        }
    }
}
