//! Reset and power off.

use crate::arch::{self, inb, outb, outw};

/// Restart the machine: chipset reset control register, the keyboard
/// controller, then a triple fault.
pub fn reboot() -> ! {
    arch::disable_interrupts();
    crate::println!("Restarting...");
    crate::acpi::reset();
    unsafe {
        outb(0xcf9, 0x02);
        outb(0xcf9, 0x06);
        for _ in 0..100_000 {
            if inb(0x64) & 2 == 0 {
                break;
            }
        }
        outb(0x64, 0xfe);
        // Triple fault: load an empty IDT and raise an exception.
        let null: [u8; 10] = [0; 10];
        core::arch::asm!("lidt [{}]", "int3", in(reg) &null);
    }
    arch::halt_forever()
}

/// Power off (QEMU/Bochs ACPI ports; ACPI proper comes later).
pub fn power_off() -> ! {
    try_power_off();
    crate::println!("It is now safe to turn off your computer.");
    arch::halt_forever()
}

/// Try to power off: ACPI S5, then the QEMU/Bochs ports. Returns if the
/// machine is still running a moment later.
pub fn try_power_off() {
    crate::acpi::power_off();
    unsafe {
        outw(0x604, 0x2000);
        outw(0xb004, 0x2000);
    }
    // The chipset may take a moment.
    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }
}
