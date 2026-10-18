//! rs2 kernel.

#![no_std]
#![no_main]
// Much of the API surface is filled in incrementally.
#![allow(dead_code)]

extern crate alloc;

mod acpi;
mod api;
mod apic;
mod arch;
mod codepage;
mod config;
mod console;
mod drivers;
mod fs;
mod infoseg;
mod irq;
mod kbd;
mod loader;
mod pipe;
mod power;
mod log;
mod mm;
mod npipe;
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
    arch::init_fpu();
    mm::init(info);
    acpi::init(info.rsdp);
    irq::init();
    task::sched::init();
    fs::init(info);
    drivers::init();
    loader::init();
    infoseg::init();
    kbd::init();
    arch::enable_interrupts();
    if !apic::init() {
        println!("apic: not available, using the 8259 PICs");
    }

    let cmdline = info.cmdline();
    println!("command line: {cmdline}");
    if cmdline.split_whitespace().any(|w| w == "apitrace") {
        api::TRACE.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    boot::start(cmdline);
    // This boot context is now the idle thread.
    loop {
        task::sched::schedule();
        arch::halt();
    }
}

/// Start-up sequence: CONFIG.SYS, CALL= programs, RUN= programs, then the
/// protected-mode shell (restarted if it exits).
pub mod boot {
    use alloc::collections::VecDeque;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    use crate::sync::SpinLock;

    struct Boot {
        env: Vec<String>,
        /// Programs still to run synchronously, then the shell.
        queue: VecDeque<(String, String)>,
        shell: (String, String),
        current: u32,
        /// Restart the shell when it exits (not for an init= override).
        restart: bool,
    }

    static STATE: SpinLock<Option<Boot>> = SpinLock::new(None);

    fn default_env() -> Vec<String> {
        [
            "PATH=C:\\OS2;C:\\OS2\\SYSTEM;C:\\RS2;C:\\",
            "DPATH=C:\\OS2;C:\\OS2\\SYSTEM;C:\\RS2;C:\\",
            "PROMPT=$i[$p]",
            "HELP=C:\\OS2\\HELP",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn find_program(p: &str) -> String {
        if p.contains(':') {
            return p.to_string();
        }
        if p.contains('\\') {
            return alloc::format!("C:\\{}", p.trim_start_matches('\\'));
        }
        for dir in ["C:\\", "C:\\OS2\\", "C:\\RS2\\"] {
            let full = alloc::format!("{dir}{p}");
            if crate::fs::read_file(&full).is_some() {
                return full;
            }
        }
        alloc::format!("C:\\{p}")
    }

    pub fn start(cmdline: &str) {
        let cfg = crate::config::load().unwrap_or_default();
        if let Some(lp) = &cfg.libpath {
            crate::fs::set_libpath(lp);
        }
        let mut env = default_env();
        for e in &cfg.env {
            let key = e.split('=').next().unwrap_or("");
            env.retain(|x| !x.split('=').next().unwrap_or("").eq_ignore_ascii_case(key));
            env.push(e.clone());
        }
        let init = cmdline.split_whitespace().find_map(|w| w.strip_prefix("init="));
        let args = cmdline.split_once(" -- ").map_or("", |(_, a)| a);
        let shell = match (init, &cfg.protshell) {
            (Some(i), _) => (i.to_string(), args.to_string()),
            (None, Some(p)) => p.clone(),
            (None, None) => ("CMD.EXE".to_string(), String::new()),
        };
        let shell_path = find_program(&shell.0);
        if !env.iter().any(|e| e.starts_with("COMSPEC=")) {
            env.push(alloc::format!("COMSPEC={shell_path}"));
        }
        if !env.iter().any(|e| e.starts_with("OS2_SHELL=")) {
            env.push(alloc::format!("OS2_SHELL={shell_path}"));
        }
        if !cfg.drivers.is_empty() {
            crate::println!("config.sys: {} driver statements handled by native drivers", cfg.drivers.len());
        }
        for (p, a) in &cfg.run {
            spawn(&find_program(p), a, &env, true);
        }
        // The PM shell's SYS_DLLS LoadOneTime list: REXXINIT starts the
        // REXX API server thread. SYSDLLS keeps such modules resident.
        let sys_dlls: Vec<&str> = ["REXXINIT"].into_iter().filter(|m| crate::fs::find_module(m).is_some()).collect();
        let loader = find_program("SYSDLLS.EXE");
        if !sys_dlls.is_empty() && crate::fs::read_file(&loader).is_some() {
            spawn(&loader, &sys_dlls.join(" "), &env, true);
        }
        let mut queue: VecDeque<(String, String)> = cfg.call.iter().map(|(p, a)| (find_program(p), a.clone())).collect();
        queue.push_back((shell_path.clone(), shell.1.clone()));
        let restart = init.is_none();
        *STATE.lock() = Some(Boot { env, queue, shell: (shell_path, shell.1), current: 0, restart });
        next();
    }

    fn spawn(path: &str, args: &str, env: &[String], detached: bool) -> Option<u32> {
        let program = path.rsplit('\\').next().unwrap_or(path).to_string();
        let r = crate::task::process::exec(crate::task::process::ExecParams {
            path,
            program: &program,
            args,
            env: env.to_vec(),
            parent: None,
            handles: Vec::new(),
            detached,
        });
        match r {
            Ok(p) => Some(p.pid),
            Err(e) => {
                crate::println!("cannot start {path}: error {e}");
                None
            }
        }
    }

    /// Start the next boot program (or restart the shell).
    fn next() {
        loop {
            let (item, env, last) = {
                let mut g = STATE.lock();
                let Some(b) = g.as_mut() else { return };
                let last = b.queue.len() <= 1;
                if b.queue.is_empty() && !b.restart {
                    return;
                }
                let item = b.queue.pop_front().unwrap_or_else(|| b.shell.clone());
                (item, b.env.clone(), last)
            };
            match spawn(&item.0, &item.1, &env, false) {
                Some(pid) => {
                    if let Some(b) = STATE.lock().as_mut() {
                        b.current = pid;
                    }
                    return;
                }
                // A shell that cannot start is not retried forever.
                None if last => return,
                None => {}
            }
        }
    }

    /// A top-level process ended.
    pub fn process_ended(pid: u32) {
        let ours = STATE.lock().as_ref().is_some_and(|b| b.current == pid);
        if ours {
            next();
        }
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
