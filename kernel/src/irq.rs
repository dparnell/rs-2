//! Legacy 8259 PIC and 8254 PIT.
//!
//! Enough interrupt routing to bring the system up; the IOAPIC/LAPIC path
//! replaces it once ACPI is parsed.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::arch::idt::TrapFrame;
use crate::arch::{inb, outb};
use crate::trap::IRQ_BASE;

/// Timer frequency.
pub const HZ: u64 = 1000;

static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Milliseconds since boot.
pub fn uptime_ms() -> u64 {
    ticks() * 1000 / HZ
}

type Handler = fn(&mut TrapFrame);
static mut HANDLERS: [Option<Handler>; 16] = [None; 16];

pub fn register(irq: u8, h: Handler) {
    unsafe { (*core::ptr::addr_of_mut!(HANDLERS))[irq as usize] = Some(h) };
    unmask(irq);
}

pub fn unmask(irq: u8) {
    if crate::apic::active() {
        crate::apic::route_irq(irq);
        return;
    }
    unsafe {
        if irq < 8 {
            outb(0x21, inb(0x21) & !(1 << irq));
        } else {
            outb(0xa1, inb(0xa1) & !(1 << (irq - 8)));
            outb(0x21, inb(0x21) & !(1 << 2));
        }
    }
}

fn eoi(irq: u8) {
    if crate::apic::active() {
        crate::apic::eoi();
        return;
    }
    unsafe {
        if irq >= 8 {
            outb(0xa0, 0x20);
        }
        outb(0x20, 0x20);
    }
}

pub fn handle(irq: u8, f: &mut TrapFrame) {
    // Spurious IRQ 7 / 15 have no in-service bit.
    if (irq == 7 || irq == 15) && !crate::apic::active() {
        unsafe {
            let port = if irq == 7 { 0x20 } else { 0xa0 };
            outb(port, 0x0b);
            if inb(port) & 0x80 == 0 {
                if irq == 15 {
                    outb(0x20, 0x20);
                }
                return;
            }
        }
    }
    eoi(irq);
    if irq == 0 {
        TICKS.fetch_add(1, Ordering::Relaxed);
        crate::task::timer_tick(f);
        return;
    }
    if let Some(h) = unsafe { (*core::ptr::addr_of!(HANDLERS))[irq as usize] } {
        h(f);
    }
}

pub fn init() {
    unsafe {
        // ICW1..4: cascade, vectors IRQ_BASE.., 8086 mode.
        outb(0x20, 0x11);
        outb(0xa0, 0x11);
        outb(0x21, IRQ_BASE as u8);
        outb(0xa1, IRQ_BASE as u8 + 8);
        outb(0x21, 4);
        outb(0xa1, 2);
        outb(0x21, 1);
        outb(0xa1, 1);
        outb(0x21, 0xff);
        outb(0xa1, 0xff);

        // PIT channel 0, rate generator.
        let div = (1_193_182 / HZ) as u16;
        outb(0x43, 0x34);
        outb(0x40, div as u8);
        outb(0x40, (div >> 8) as u8);
    }
    unmask(0);
}
