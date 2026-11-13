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
/// Handlers by IRQ (a `Handler` as an address, 0 = none): set from thread
/// context (opening a COM port) while other CPUs take interrupts.
static HANDLERS: [core::sync::atomic::AtomicUsize; 16] = [const { core::sync::atomic::AtomicUsize::new(0) }; 16];
/// The read-modify-write of the interrupt masks.
static MASK: crate::sync::SpinLock<()> = crate::sync::SpinLock::new(());

pub fn register(irq: u8, h: Handler) {
    HANDLERS[irq as usize].store(h as usize, Ordering::Release);
    unmask(irq);
}

/// Whether a handler is installed for `irq`.
pub fn registered(irq: u8) -> bool {
    irq < 16 && HANDLERS[irq as usize].load(Ordering::Acquire) != 0
}

pub fn unmask(irq: u8) {
    let _g = MASK.lock();
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
    crate::api::perf::interrupt(crate::smp::this().index());
    if irq == 0 {
        // Every CPU's LAPIC timer arrives here; the boot processor keeps
        // the time and runs the periodic work, the others only account
        // their time slice.
        crate::bench::account();
        if crate::smp::this().index() == 0 {
            // Ticks lost (interrupts that came late: QEMU without KVM
            // drops about a third of them) are made up from the time-stamp
            // counter, a few per interrupt, so that sleeps and timers keep
            // to the clock.
            let behind = crate::rtc::mono_ms().saturating_sub(ticks() * 1000 / HZ + 1);
            if behind > 1000 {
                TICKS.fetch_add(behind - 1000, Ordering::Relaxed);
            }
            let n = 1 + behind.min(4);
            let t0 = crate::arch::rdtsc();
            for _ in 0..n {
                TICKS.fetch_add(1, Ordering::Relaxed);
                crate::rtc::tick();
                crate::task::timer_tick(f);
            }
            crate::bench::add_tick_cycles(crate::arch::rdtsc().wrapping_sub(t0));
        } else {
            crate::task::sched::tick();
        }
        return;
    }
    let h = HANDLERS[irq as usize].load(Ordering::Acquire);
    if h != 0 {
        let h: Handler = unsafe { core::mem::transmute::<usize, Handler>(h) };
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
