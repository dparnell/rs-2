//! Local APIC and IOAPIC.
//!
//! Replaces the 8259 PICs once ACPI has described the interrupt
//! controllers: the LAPIC timer drives the scheduler tick and legacy ISA
//! interrupts are routed through the IOAPIC to the same vectors the PIC used
//! (IRQ_BASE + irq), so the rest of the kernel does not care which is
//! active.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::arch::outb;
use crate::trap::IRQ_BASE;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static LAPIC: AtomicU64 = AtomicU64::new(0);
static IOAPIC: AtomicU64 = AtomicU64::new(0);
/// LAPIC timer count per kernel tick (calibrated on the boot processor).
static TIMER_COUNT: AtomicU64 = AtomicU64::new(0);

pub const SPURIOUS: u8 = 0xff;

fn lapic_reg(off: usize) -> *mut u32 {
    (LAPIC.load(Ordering::Relaxed) as usize + off) as *mut u32
}

fn lapic_read(off: usize) -> u32 {
    unsafe { read_volatile(lapic_reg(off)) }
}

fn lapic_write(off: usize, v: u32) {
    unsafe { write_volatile(lapic_reg(off), v) }
}

fn ioapic_write(reg: u32, v: u32) {
    let base = IOAPIC.load(Ordering::Relaxed) as *mut u32;
    unsafe {
        write_volatile(base, reg);
        write_volatile(base.add(4), v);
    }
}

fn ioapic_read(reg: u32) -> u32 {
    let base = IOAPIC.load(Ordering::Relaxed) as *mut u32;
    unsafe {
        write_volatile(base, reg);
        read_volatile(base.add(4))
    }
}

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn eoi() {
    lapic_write(0xb0, 0);
}

pub fn id() -> u8 {
    (lapic_read(0x20) >> 24) as u8
}

/// Route ISA `irq` to its usual vector, honouring ACPI overrides.
pub fn route_irq(irq: u8) {
    let Some(info) = crate::acpi::info() else { return };
    let mut gsi = irq as u32;
    let mut flags = 0u16;
    if let Some(o) = info.overrides.iter().find(|o| o.irq == irq) {
        gsi = o.gsi;
        flags = o.flags;
    }
    let Some(io) = info.ioapics.iter().find(|i| gsi >= i.gsi_base) else { return };
    let pin = gsi - io.gsi_base;
    let mut lo = (IRQ_BASE as u32) + irq as u32;
    // Polarity: 3 = active low. Trigger: 3 = level.
    if flags & 3 == 3 {
        lo |= 1 << 13;
    }
    if (flags >> 2) & 3 == 3 {
        lo |= 1 << 15;
    }
    let dest = (id() as u32) << 24;
    ioapic_write(0x10 + pin * 2 + 1, dest);
    ioapic_write(0x10 + pin * 2, lo);
}

/// Enable this application processor's LAPIC with the same periodic
/// timer as the boot processor.
pub fn init_ap() {
    lapic_write(0x80, 0);
    lapic_write(0xf0, 0x100 | SPURIOUS as u32);
    lapic_write(0x3e0, 0x3);
    lapic_write(0x320, (1 << 17) | IRQ_BASE as u32);
    lapic_write(0x380, TIMER_COUNT.load(Ordering::Relaxed) as u32);
}

fn icr(dest: u32, low: u32) {
    // Wait for a previous IPI to be accepted (delivery status).
    while lapic_read(0x300) & (1 << 12) != 0 {
        core::hint::spin_loop();
    }
    lapic_write(0x310, dest << 24);
    lapic_write(0x300, low);
}

/// Fixed-delivery IPI.
pub fn send_ipi(dest: u32, vector: u8) {
    icr(dest, vector as u32);
}

/// INIT IPI (assert, level).
pub fn send_init(dest: u32) {
    icr(dest, 0x4500);
}

/// STARTUP IPI: the AP starts in real mode at `page` << 12.
pub fn send_startup(dest: u32, page: u8) {
    icr(dest, 0x4600 | page as u32);
}

pub fn mask_irq(irq: u8) {
    let Some(info) = crate::acpi::info() else { return };
    let gsi = info.overrides.iter().find(|o| o.irq == irq).map_or(irq as u32, |o| o.gsi);
    let Some(io) = info.ioapics.iter().find(|i| gsi >= i.gsi_base) else { return };
    let pin = gsi - io.gsi_base;
    let lo = ioapic_read(0x10 + pin * 2);
    ioapic_write(0x10 + pin * 2, lo | (1 << 16));
}

/// Switch from the PICs to the APICs. The PIT keeps ticking through the
/// PIC until the LAPIC timer is calibrated against it.
pub fn init() -> bool {
    let Some(info) = crate::acpi::info() else { return false };
    if info.lapic == 0 || info.ioapics.is_empty() {
        return false;
    }
    LAPIC.store(crate::mm::mmio::map(info.lapic, 4096) as u64, Ordering::Relaxed);
    IOAPIC.store(crate::mm::mmio::map(info.ioapics[0].address, 4096) as u64, Ordering::Relaxed);
    // Mask every IOAPIC input first.
    let max = (ioapic_read(1) >> 16) & 0xff;
    for pin in 0..=max {
        ioapic_write(0x10 + pin * 2, 1 << 16);
    }
    // Enable the LAPIC.
    lapic_write(0x80, 0);
    lapic_write(0xf0, 0x100 | SPURIOUS as u32);
    // Calibrate the timer: count down during 50 PIT ticks.
    lapic_write(0x3e0, 0x3); // divide by 16
    lapic_write(0x320, 1 << 16); // masked, one shot
    let start = crate::irq::ticks();
    while crate::irq::ticks() == start {
        core::hint::spin_loop();
    }
    lapic_write(0x380, 0xffff_ffff);
    let t0 = crate::irq::ticks();
    while crate::irq::ticks() < t0 + 50 {
        core::hint::spin_loop();
    }
    let elapsed = 0xffff_ffff - lapic_read(0x390);
    let per_tick = elapsed / 50;
    // Retire the PICs.
    crate::arch::disable_interrupts();
    unsafe {
        outb(0x21, 0xff);
        outb(0xa1, 0xff);
    }
    ACTIVE.store(true, Ordering::Relaxed);
    // Periodic LAPIC timer at the kernel tick rate on IRQ 0's vector.
    TIMER_COUNT.store(per_tick.max(1) as u64, Ordering::Relaxed);
    lapic_write(0x320, (1 << 17) | IRQ_BASE as u32);
    lapic_write(0x380, per_tick.max(1));
    for irq in [1u8, 4, 12] {
        route_irq(irq);
    }
    crate::arch::enable_interrupts();
    crate::println!("apic: lapic timer {} counts/tick, ioapic {} inputs", per_tick, max + 1);
    true
}
