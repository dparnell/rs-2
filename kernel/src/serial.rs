//! COM1 debug output.

use crate::arch::{inb, outb};

const COM1: u16 = 0x3f8;

/// A UART answers at COM1. Machines without one (the Surface Pro 4) may
/// read 00h from the port: then every byte waited the full timeout below
/// for "transmitter empty", and printing looked like a hung boot.
static PRESENT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn present() -> bool {
    PRESENT.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn init() {
    // The scratch register keeps what is written to it (16450 and later).
    let found = unsafe {
        outb(COM1 + 7, 0x5a);
        let a = inb(COM1 + 7);
        outb(COM1 + 7, 0xa5);
        a == 0x5a && inb(COM1 + 7) == 0xa5
    };
    PRESENT.store(found, core::sync::atomic::Ordering::Relaxed);
    if !found {
        return;
    }
    unsafe {
        outb(COM1 + 1, 0x00); // no interrupts
        outb(COM1 + 3, 0x80); // DLAB
        outb(COM1, 0x01); // 115200 baud
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03); // 8N1
        outb(COM1 + 2, 0xc7); // FIFO
        outb(COM1 + 4, 0x0b);
    }
}

/// COM1's registers (chardev.rs takes it for line settings and DosWrite
/// to COM1).
pub static COM1_HW: crate::sync::SpinLock<()> = crate::sync::SpinLock::new(());

pub fn write_byte(b: u8) {
    if !present() {
        return;
    }
    // Tried, not waited for: the log must get out even if a CPU stopped
    // holding it (panics).
    let mut g = None;
    for _ in 0..100_000 {
        g = COM1_HW.try_lock();
        if g.is_some() {
            break;
        }
        core::hint::spin_loop();
    }
    let _g = g;
    unsafe {
        let mut spins = 0;
        while inb(COM1 + 5) & 0x20 == 0 && spins < 100_000 {
            spins += 1;
        }
        outb(COM1, b);
    }
}

pub fn write_str(s: &str) {
    crate::klog::write(s.as_bytes());
    for b in s.bytes() {
        if b == b'\n' {
            write_byte(b'\r');
        }
        write_byte(b);
    }
}
