//! The ACPI embedded controller (ACPI 6, chapter 12): the keyboard-
//! controller-like interface through which the AML of laptops reaches the
//! battery, the AC adapter, lid, fans and hot keys. Two I/O ports, from
//! the ECDT or the PNP0C09 device's `_CRS`: data (usually 0x62) and
//! command/status (0x66). A byte is read with command 0x80 and its
//! address, written with 0x81, address and value; 0x84 asks which event
//! (`_Qxx` method) the controller has pending when status bit SCI_EVT is
//! set.
//!
//! Only the ACPI thread ([`crate::acpipm`]) uses it, so transactions need
//! no lock. Events are polled (SCI_EVT) rather than taken from the GPE.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::sync::Once;

const OBF: u8 = 0x01;
const IBF: u8 = 0x02;
const SCI_EVT: u8 = 0x20;

const RD_EC: u8 = 0x80;
const WR_EC: u8 = 0x81;
const QR_EC: u8 = 0x84;

/// How long one step of a transaction may take.
const TIMEOUT_MS: u64 = 50;

pub struct Ec {
    pub data: u16,
    pub command: u16,
    pub gpe: u32,
}

static EC: Once<Ec> = Once::new();
static TIMEOUTS: AtomicU32 = AtomicU32::new(0);

pub fn init(data: u16, command: u16, gpe: u32) {
    crate::println!("ec: data {data:#x}, command {command:#x}, GPE {gpe:#x}");
    EC.set(Ec { data, command, gpe });
}

pub fn present() -> bool {
    EC.get().is_some()
}

pub fn get() -> Option<&'static Ec> {
    EC.get()
}

/// Transactions that timed out (shown in the log).
pub fn timeouts() -> u32 {
    TIMEOUTS.load(Ordering::Relaxed)
}

impl Ec {
    fn status(&self) -> u8 {
        unsafe { crate::arch::inb(self.command) }
    }

    fn wait(&self, ready: impl Fn(u8) -> bool) -> bool {
        let deadline = crate::rtc::mono_ms() + TIMEOUT_MS;
        loop {
            if ready(self.status()) {
                return true;
            }
            if crate::rtc::mono_ms() > deadline {
                TIMEOUTS.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            core::hint::spin_loop();
        }
    }

    fn send_cmd(&self, c: u8) -> bool {
        self.wait(|s| s & IBF == 0) && {
            unsafe { crate::arch::outb(self.command, c) };
            true
        }
    }

    fn send_data(&self, v: u8) -> bool {
        self.wait(|s| s & IBF == 0) && {
            unsafe { crate::arch::outb(self.data, v) };
            true
        }
    }

    fn recv(&self) -> Option<u8> {
        self.wait(|s| s & OBF != 0).then(|| unsafe { crate::arch::inb(self.data) })
    }

    /// Drop a stale byte the controller left in its output buffer.
    fn flush(&self) {
        for _ in 0..16 {
            if self.status() & OBF == 0 {
                break;
            }
            unsafe { crate::arch::inb(self.data) };
        }
    }

    pub fn read(&self, addr: u8) -> Option<u8> {
        self.flush();
        if !self.send_cmd(RD_EC) || !self.send_data(addr) {
            return None;
        }
        self.recv()
    }

    pub fn write(&self, addr: u8, v: u8) -> bool {
        self.flush();
        self.send_cmd(WR_EC) && self.send_data(addr) && self.send_data(v) && self.wait(|s| s & IBF == 0)
    }

    /// An event is waiting for a query.
    pub fn event_pending(&self) -> bool {
        self.status() & SCI_EVT != 0
    }

    /// The pending event's number (0: none).
    pub fn query(&self) -> Option<u8> {
        self.flush();
        if !self.send_cmd(QR_EC) {
            return None;
        }
        self.recv()
    }
}
