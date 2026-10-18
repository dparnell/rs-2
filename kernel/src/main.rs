//! rs2 kernel.

#![no_std]
#![no_main]
// Much of the API surface is filled in incrementally.
#![allow(dead_code)]

extern crate alloc;

mod api;
mod arch;
mod codepage;
mod irq;
mod log;
mod mm;
mod screen;
mod serial;
mod sync;
mod task;
mod trap;

use bootinfo::BootInfo;

core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "    and rsp, -16",
    "    call {main}",
    "2:  cli",
    "    hlt",
    "    jmp 2b",
    main = sym kernel_main,
);

static mut BOOT_INFO: *const BootInfo = core::ptr::null();

pub fn boot_info() -> &'static BootInfo {
    unsafe { &*BOOT_INFO }
}

extern "C" fn kernel_main(info: &'static BootInfo) -> ! {
    serial::init();
    serial::write_str("rs2 kernel starting\n");
    assert_eq!(info.magic, bootinfo::MAGIC, "bad boot info");
    unsafe { BOOT_INFO = info };
    screen::init(info.framebuffer);
    println!("rs2 - OS/2 Warp compatible kernel, version {}", env!("CARGO_PKG_VERSION"));
    let fb = &info.framebuffer;
    println!("framebuffer {}x{} at {:#x}", fb.width, fb.height, fb.base);
    arch::gdt::init();
    arch::idt::init();
    mm::init(info);
    for m in info.modules() {
        println!("module {} ({} bytes)", m.name(), m.len);
    }
    irq::init();
    arch::enable_interrupts();
    let start = irq::ticks();
    while irq::ticks() < start + 100 {
        arch::halt();
    }
    println!("timer ok: {} ticks", irq::ticks());
    let v = alloc::vec![1u32, 2, 3];
    println!("heap ok: {:?}", v);
    loop {
        arch::halt();
    }
}

/// Print a frame-pointer backtrace.
pub fn backtrace(mut rbp: u64) {
    log::print_unlocked(format_args!("backtrace:"));
    for _ in 0..16 {
        if rbp < 0xffff_8000_0000_0000 || rbp & 7 != 0 {
            break;
        }
        let ret = unsafe { *((rbp + 8) as *const u64) };
        log::print_unlocked(format_args!(" {ret:#x}"));
        rbp = unsafe { *(rbp as *const u64) };
    }
    log::print_unlocked(format_args!("\n"));
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    arch::disable_interrupts();
    log::print_unlocked(format_args!("\nKERNEL PANIC: {info}\n"));
    let rbp: u64;
    unsafe { core::arch::asm!("mov {}, rbp", out(reg) rbp) };
    backtrace(rbp);
    arch::halt_forever();
}
