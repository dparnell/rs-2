//! rs2 kernel.

#![no_std]
#![no_main]
// Much of the API surface is filled in incrementally.
#![allow(dead_code)]

extern crate alloc;

mod api;
mod arch;
mod codepage;
mod console;
mod fs;
mod infoseg;
mod irq;
mod kbd;
mod loader;
mod log;
mod mm;
mod rtc;
mod screen;
mod serial;
mod sync;
mod task;
mod trap;
mod vm;

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
    irq::init();
    task::sched::init();
    fs::init(info);
    loader::init();
    infoseg::init();
    kbd::init();
    arch::enable_interrupts();

    let cmdline = info.cmdline();
    println!("command line: {cmdline}");
    if cmdline.split_whitespace().any(|w| w == "apitrace") {
        api::TRACE.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    let init = cmdline
        .split_whitespace()
        .find_map(|w| w.strip_prefix("init="))
        .unwrap_or("CMD.EXE");
    // Arguments for init follow "--".
    let args = cmdline.split_once(" -- ").map_or("", |(_, a)| a);
    start_init(init, args);
    // This boot context is now the idle thread.
    loop {
        task::sched::schedule();
        arch::halt();
    }
}

fn start_init(program: &str, args: &str) {
    use alloc::string::ToString;
    let env = alloc::vec![
        "PATH=C:\\;C:\\OS2".to_string(),
        "DPATH=C:\\;C:\\OS2".to_string(),
        "COMSPEC=C:\\CMD.EXE".to_string(),
        "OS2_SHELL=C:\\CMD.EXE".to_string(),
        "PROMPT=$i[$p]".to_string(),
    ];
    let path = alloc::format!("C:\\{}", program.to_ascii_uppercase());
    let r = task::process::exec(task::process::ExecParams {
        path: &path,
        program,
        args,
        env,
        parent: None,
        handles: alloc::vec::Vec::new(),
    });
    match r {
        Ok(p) => println!("started {} as process {}", path, p.pid),
        Err(e) => println!("cannot start {path}: error {e}"),
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
