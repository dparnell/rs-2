//! ACPI power sources: the batteries (PNP0C0A control-method batteries)
//! and AC adapters (ACPI0003) of the namespace, read by running their AML
//! ([`aml`]) on a kernel thread of their own, the "acpi" thread.
//!
//! At start-up the thread loads the DSDT and SSDTs into the interpreter,
//! tells the firmware rs2 uses the APIC (`_PIC (1)`), sets up the
//! embedded controller ([`crate::ec`], from the ECDT or the PNP0C09
//! device's `_CRS`), runs `_REG` for the address spaces it can reach and
//! the devices' `_INI`. Then it polls: every five seconds (at once after a
//! `Notify` of the device) `_PSR` of the adapters and `_STA`/`_BST` of the
//! batteries, `_BIX` (else `_BIF`) when a battery appears or its AML says
//! the information changed (Notify 0x81); every half second the EC's
//! pending events, whose `_Qxx` methods it runs as the GPE handler would.
//! The results are kept in [`status`] for `APM$` (category 0xC function
//! 0x60) and the Power object: nothing on those paths runs AML.
//!
//! `noaml` on the kernel command line leaves the thread out.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use aml::{Interp, Obj};

use crate::sync::SpinLock;

/// One battery as its AML reports it. Capacities are in `unit`s
/// (mWh or mAh); 0xFFFFFFFF is "unknown" as in ACPI.
#[derive(Clone, Debug, Default)]
pub struct Battery {
    pub path: String,
    pub present: bool,
    /// mWh (false) or mAh (true).
    pub mah: bool,
    pub design: u32,
    pub full: u32,
    pub warning: u32,
    pub low: u32,
    pub voltage: u32,
    pub model: String,
    /// `_BST` state: bit 0 discharging, 1 charging, 2 critical.
    pub state: u32,
    pub rate: u32,
    pub remaining: u32,
}

const UNKNOWN: u32 = 0xffff_ffff;

impl Battery {
    /// Charge in percent of the last full charge.
    pub fn percent(&self) -> Option<u32> {
        let full = if self.full != 0 && self.full != UNKNOWN { self.full } else { self.design };
        if !self.present || full == 0 || full == UNKNOWN || self.remaining == UNKNOWN {
            return None;
        }
        Some((self.remaining as u64 * 100 / full as u64).min(100) as u32)
    }

    pub fn charging(&self) -> bool {
        self.state & 2 != 0
    }

    pub fn discharging(&self) -> bool {
        self.state & 1 != 0
    }

    /// Minutes left on battery (discharging with a known rate).
    pub fn minutes_left(&self) -> Option<u32> {
        if !self.present || !self.discharging() || self.rate == 0 || self.rate == UNKNOWN || self.remaining == UNKNOWN {
            return None;
        }
        Some((self.remaining as u64 * 60 / self.rate as u64).min(0x7ffe) as u32)
    }

    pub fn critical(&self) -> bool {
        self.state & 4 != 0 || (self.low != UNKNOWN && self.low != 0 && self.remaining != UNKNOWN && self.remaining <= self.low)
    }

    pub fn low(&self) -> bool {
        self.warning != UNKNOWN && self.warning != 0 && self.remaining != UNKNOWN && self.remaining <= self.warning
    }
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Some: the AC adapters' `_PSR` (any on line); None: no adapter.
    pub ac: Option<bool>,
    pub batteries: Vec<Battery>,
}

static STATUS: SpinLock<Option<Status>> = SpinLock::new(None);

/// The power sources as last read; None before the ACPI thread has read
/// them, or without any.
pub fn status() -> Option<Status> {
    STATUS.lock().clone()
}

pub fn start(cmdline: &str) {
    if cmdline.split_whitespace().any(|w| w == "noaml") {
        crate::println!("acpi: AML interpreter off (noaml)");
        return;
    }
    if crate::acpi::info().is_none_or(|i| i.aml.is_empty()) {
        return;
    }
    crate::task::kthread::spawn("acpi", run);
}

/// The interpreter's access to the machine.
#[derive(Default)]
struct KHost {
    /// Uncached mappings of the physical pages SystemMemory regions use.
    pages: BTreeMap<u64, u64>,
    messages: u32,
}

impl KHost {
    fn mem(&mut self, addr: u64) -> *mut u8 {
        let page = addr & !0xfff;
        let va = *self.pages.entry(page).or_insert_with(|| crate::mm::mmio::map(page, 4096) as u64);
        (va + (addr & 0xfff)) as *mut u8
    }
}

fn pci_parts(addr: u64) -> (u8, u8, u8, u16) {
    ((addr >> 24) as u8, (addr >> 19) as u8 & 0x1f, (addr >> 16) as u8 & 7, addr as u16)
}

impl aml::Host for KHost {
    fn read(&mut self, space: u8, addr: u64, bits: u8) -> Option<u64> {
        use crate::arch::{inb, inl, inw};
        Some(match space {
            aml::space::MEMORY => unsafe {
                let p = self.mem(addr);
                match bits {
                    8 => p.read_volatile() as u64,
                    16 => (p as *mut u16).read_volatile() as u64,
                    32 => (p as *mut u32).read_volatile() as u64,
                    _ => (p as *mut u64).read_volatile(),
                }
            },
            aml::space::IO => unsafe {
                let port = addr as u16;
                match bits {
                    8 => inb(port) as u64,
                    16 => inw(port) as u64,
                    32 => inl(port) as u64,
                    _ => inl(port) as u64 | (inl(port + 4) as u64) << 32,
                }
            },
            aml::space::PCI => {
                let (bus, dev, func, off) = pci_parts(addr);
                if off >= 256 {
                    return Some(u64::MAX >> (64 - bits as u32));
                }
                let mut v = 0u64;
                for i in 0..(bits as u16 / 8) {
                    let o = off + i;
                    let d = crate::drivers::pci::read32(bus, dev, func, (o & !3) as u8);
                    v |= ((d >> ((o & 3) * 8)) as u64 & 0xff) << (8 * i);
                }
                v
            }
            aml::space::EC => {
                let ec = crate::ec::get()?;
                let mut v = 0u64;
                for i in 0..(bits as u64 / 8) {
                    v |= (ec.read((addr + i) as u8)? as u64) << (8 * i);
                }
                v
            }
            _ => return None,
        })
    }

    fn write(&mut self, space: u8, addr: u64, bits: u8, value: u64) -> bool {
        use crate::arch::{outb, outl, outw};
        match space {
            aml::space::MEMORY => unsafe {
                let p = self.mem(addr);
                match bits {
                    8 => p.write_volatile(value as u8),
                    16 => (p as *mut u16).write_volatile(value as u16),
                    32 => (p as *mut u32).write_volatile(value as u32),
                    _ => (p as *mut u64).write_volatile(value),
                }
            },
            aml::space::IO => unsafe {
                let port = addr as u16;
                match bits {
                    8 => outb(port, value as u8),
                    16 => outw(port, value as u16),
                    32 => outl(port, value as u32),
                    _ => {
                        outl(port, value as u32);
                        outl(port + 4, (value >> 32) as u32);
                    }
                }
            },
            aml::space::PCI => {
                let (bus, dev, func, off) = pci_parts(addr);
                if off >= 256 {
                    return true;
                }
                for i in 0..(bits as u16 / 8) {
                    let o = off + i;
                    let a = (o & !3) as u8;
                    let sh = (o & 3) * 8;
                    let d = crate::drivers::pci::read32(bus, dev, func, a);
                    let d = (d & !(0xff << sh)) | (((value >> (8 * i)) as u32 & 0xff) << sh);
                    crate::drivers::pci::write32(bus, dev, func, a, d);
                }
            }
            aml::space::EC => {
                let Some(ec) = crate::ec::get() else { return false };
                for i in 0..(bits as u64 / 8) {
                    if !ec.write((addr + i) as u8, (value >> (8 * i)) as u8) {
                        return false;
                    }
                }
            }
            _ => return false,
        }
        true
    }

    fn sleep_ms(&mut self, ms: u64) {
        crate::task::sched::sleep_ms(ms);
    }

    fn stall_us(&mut self, us: u64) {
        let end = crate::rtc::mono_scaled(1_000_000) + us;
        while crate::rtc::mono_scaled(1_000_000) < end {
            core::hint::spin_loop();
        }
    }

    fn timer(&mut self) -> u64 {
        crate::rtc::mono_scaled(10_000_000)
    }

    fn debug(&mut self, s: &str) {
        // Firmware can be chatty: the first hundred lines.
        self.messages += 1;
        if self.messages <= 100 {
            crate::println!("{s}");
        }
    }
}

/// An ACPI path as the ECDT gives it ("\\_SB.PCI0.EC0") in the
/// interpreter's form ("_SB_.PCI0.EC0_").
fn interp_path(s: &str) -> String {
    s.trim_start_matches('\\')
        .split('.')
        .filter(|x| !x.is_empty())
        .map(|seg| {
            let mut v: String = seg.chars().take(4).collect();
            while v.len() < 4 {
                v.push('_');
            }
            v
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The first two I/O ports of a resource template (I/O port and fixed
/// I/O descriptors).
fn io_ports(res: &[u8]) -> Vec<u16> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < res.len() {
        let t = res[i];
        if t & 0x80 == 0 {
            let (kind, len) = ((t >> 3) & 0xf, (t & 7) as usize);
            if kind == 0xf {
                break;
            }
            if kind == 8 && len >= 7 && i + 3 < res.len() {
                v.push(u16::from_le_bytes([res[i + 2], res[i + 3]]));
            } else if kind == 9 && len >= 3 && i + 2 < res.len() {
                v.push(u16::from_le_bytes([res[i + 1], res[i + 2]]));
            }
            i += 1 + len;
        } else {
            if i + 2 >= res.len() {
                break;
            }
            i += 3 + u16::from_le_bytes([res[i + 1], res[i + 2]]) as usize;
        }
    }
    v
}

type Aml = Interp<KHost>;

/// Find and start the embedded controller; returns its device path.
fn setup_ec(it: &mut Aml) -> Option<String> {
    let info = crate::acpi::info()?;
    if let Some(e) = &info.ecdt {
        crate::ec::init(e.data, e.command, e.gpe as u32);
        return Some(interp_path(&e.id));
    }
    for d in it.find_devices("PNP0C09") {
        if it.sta(&d) & 1 == 0 {
            continue;
        }
        let Ok(crs) = it.eval(&format!("{d}._CRS"), vec![]) else { continue };
        let ports = io_ports(&crs.bytes().unwrap_or_default());
        if ports.len() < 2 {
            continue;
        }
        let gpe = it.eval_int(&format!("{d}._GPE"), vec![]).unwrap_or(u64::MAX) as u32;
        crate::ec::init(ports[0], ports[1], gpe);
        return Some(d);
    }
    None
}

fn int_at(v: &[Obj], i: usize) -> u32 {
    v.get(i).and_then(|o| o.int()).map_or(UNKNOWN, |n| n as u32)
}

fn read_info(it: &mut Aml, b: &mut Battery) {
    let bix = format!("{}._BIX", b.path);
    let (v, base, model) = if it.exists(&bix) {
        (it.eval(&bix, vec![]).ok().and_then(|o| o.elements()), 1, 16)
    } else {
        (it.eval(&format!("{}._BIF", b.path), vec![]).ok().and_then(|o| o.elements()), 0, 9)
    };
    let Some(v) = v else { return };
    b.mah = int_at(&v, base) == 1;
    b.design = int_at(&v, base + 1);
    b.full = int_at(&v, base + 2);
    b.voltage = int_at(&v, base + 4);
    b.warning = int_at(&v, base + 5);
    b.low = int_at(&v, base + 6);
    b.model = v.get(model).and_then(|o| o.text()).unwrap_or_default();
}

fn read_state(it: &mut Aml, b: &mut Battery) {
    let Some(v) = it.eval(&format!("{}._BST", b.path), vec![]).ok().and_then(|o| o.elements()) else { return };
    b.state = int_at(&v, 0);
    b.rate = int_at(&v, 1);
    b.remaining = int_at(&v, 2);
    if b.voltage == UNKNOWN || int_at(&v, 3) != UNKNOWN {
        b.voltage = int_at(&v, 3);
    }
}

fn run() {
    let Some(info) = crate::acpi::info() else { return };
    let t0 = crate::rtc::mono_ms();
    let mut it: Box<Aml> = Box::new(Interp::new(KHost::default(), info.dsdt_revision));
    for &(a, n) in &info.aml {
        let block = unsafe { core::slice::from_raw_parts(crate::mm::phys::phys_to_virt(a) as *const u8, n) };
        it.load(block);
    }
    let objects = it.paths().count();
    if it.exists("_PIC") {
        let _ = it.eval("_PIC", vec![Obj::Int(1)]);
    }
    let ec = setup_ec(&mut it);
    for s in [aml::space::MEMORY, aml::space::IO, aml::space::PCI] {
        it.reg(s);
    }
    if ec.is_some() {
        it.reg(aml::space::EC);
    }
    let failed = it.initialize();
    let mut bats: Vec<Battery> =
        it.find_devices("PNP0C0A").into_iter().map(|path| Battery { path, remaining: UNKNOWN, full: UNKNOWN, ..Default::default() }).collect();
    let adapters = it.find_devices("ACPI0003");
    crate::println!(
        "acpi: AML: {objects} objects, {} load errors, {failed} _INI failed, {} battery(ies), {} AC adapter(s), {} ms",
        it.load_errors,
        bats.len(),
        adapters.len(),
        crate::rtc::mono_ms() - t0
    );
    if bats.is_empty() && adapters.is_empty() && ec.is_none() {
        return;
    }
    let mut next_read = 0u64;
    let mut need_info = true;
    let mut first = true;
    loop {
        // The EC's events: run their _Qxx methods.
        if let (Some(e), Some(path)) = (crate::ec::get(), &ec) {
            for _ in 0..8 {
                if !e.event_pending() {
                    break;
                }
                match e.query() {
                    Some(q) if q != 0 => {
                        let m = format!("{path}._Q{q:02X}");
                        if it.exists(&m) {
                            if let Err(err) = it.eval(&m, vec![]) {
                                crate::println!("acpi: {m}: {err:?}");
                            }
                        }
                    }
                    _ => break,
                }
            }
        }
        for (path, v) in it.take_notifications() {
            if bats.iter().any(|b| b.path == path) || adapters.contains(&path) {
                next_read = 0;
                if v == 0x81 {
                    need_info = true;
                }
            }
        }
        let now = crate::rtc::mono_ms();
        if now >= next_read {
            next_read = now + 5000;
            let mut st = Status::default();
            if !adapters.is_empty() {
                let mut on = false;
                for a in &adapters {
                    on |= it.eval_int(&format!("{a}._PSR"), vec![]).unwrap_or(0) != 0;
                }
                st.ac = Some(on);
            }
            for b in bats.iter_mut() {
                let present = it.sta(&b.path) & 0x10 != 0;
                if present && (!b.present || need_info) {
                    b.present = true;
                    read_info(&mut it, b);
                }
                b.present = present;
                if present {
                    read_state(&mut it, b);
                }
            }
            need_info = false;
            st.batteries = bats.clone();
            if first {
                first = false;
                for b in &st.batteries {
                    crate::println!(
                        "acpi: battery {}: {}, {}%{}{}",
                        b.path,
                        if b.present { b.model.as_str() } else { "absent" },
                        b.percent().unwrap_or(0),
                        if b.charging() { ", charging" } else { "" },
                        if b.discharging() { ", discharging" } else { "" }
                    );
                }
                if let Some(on) = st.ac {
                    crate::println!("acpi: AC adapter {}", if on { "on line" } else { "off line" });
                }
            }
            *STATUS.lock() = Some(st);
        }
        crate::task::sched::sleep_ms(if ec.is_some() { 500 } else { 1000 });
    }
}
