//! rs2 kernel.

#![no_std]
#![no_main]
// Much of the API surface is filled in incrementally.
#![allow(dead_code)]

extern crate alloc;

mod acpi;
mod aml;
mod api;
mod apic;
mod arch;
mod audio;
mod bench;
mod chardev;
mod codepage;
mod config;
mod conkbd;
mod console;
mod debug;
mod display;
mod drivers;
mod fs;
mod infoseg;
mod irq;
mod kbd;
mod loader;
mod pipe;
mod power;
mod pty;
mod log;
mod mm;
mod monitor;
mod msi;
mod mouse;
mod net;
mod nls;
mod nls_data;
mod npipe;
mod rtc;
mod screen;
mod serial;
mod session;
mod smp;
mod sync;
mod task;
mod trap;
mod vdm;
mod vm;
mod viowin;
mod vmware;

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
    smp::init_bsp();
    arch::idt::init();
    arch::init_fpu();
    mm::init(info);
    acpi::init(info.rsdp);
    rtc::start();
    irq::init();
    task::sched::init();
    fs::init(info);
    vmware::init();
    if info.cmdline().split_whitespace().any(|w| w == "nocache") {
        fs::cache::DISABLED.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    drivers::init();
    fs::cdaudio::simulate();
    loader::init();
    infoseg::init();
    api::perf::init();
    kbd::init();
    mouse::init();
    chardev::init();
    arch::enable_interrupts();
    if !apic::init() {
        println!("apic: not available, using the 8259 PICs");
    }

    let cmdline = info.cmdline();
    println!("command line: {cmdline}");
    if cmdline.split_whitespace().any(|w| w == "apitrace") {
        api::TRACE.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    // apitrace=NAME: only the programs whose name starts with NAME.
    if let Some(n) = cmdline.split_whitespace().find_map(|w| w.strip_prefix("apitrace=")) {
        api::TRACE_NAME.set(n.to_ascii_uppercase());
    }
    if cmdline.split_whitespace().any(|w| w == "dbgtrace") {
        api::debug::TRACE.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    smp::start_aps();
    msi::start();
    drivers::hotplug::start();
    fs::cache::start();
    bench::start(cmdline);
    boot::start(cmdline);
    // This boot context is now CPU 0's idle thread.
    task::sched::idle_loop();
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
        /// CHKDSK runs at the head of the queue.
        checks: usize,
        /// What starts in the background once they are done.
        background: Vec<(String, String)>,
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
        if let Some(n) = bootinfo::video_colors(cmdline) {
            crate::display::set_colors256(n == 256);
        }
        let cfg = crate::config::load().unwrap_or_default();
        crate::config::apply_memory(&cfg);
        crate::config::apply_nls(&cfg);
        crate::config::apply_sched(&cfg);
        crate::config::apply_cache(&cfg);
        crate::config::apply_video(&cfg);
        if let Some(lp) = &cfg.libpath {
            crate::fs::set_libpath(lp);
        }
        start_network(cmdline, cfg.network.as_deref());
        let mut env = default_env();
        for e in &cfg.env {
            let key = e.split('=').next().unwrap_or("");
            env.retain(|x| !x.split('=').next().unwrap_or("").eq_ignore_ascii_case(key));
            env.push(e.clone());
        }
        let init = cmdline.split_whitespace().find_map(|w| w.strip_prefix("init="));
        let args = cmdline.split_once(" -- ").map_or("", |(_, a)| a);
        // "textmode" (the boot loader adds it when T is pressed) boots to
        // the command prompt (OS2_SHELL) instead of PROTSHELL's desktop.
        let textmode = cmdline.split_whitespace().any(|w| w.eq_ignore_ascii_case("textmode"));
        let os2_shell = env.iter().find_map(|e| e.strip_prefix("OS2_SHELL=")).map(|s| s.to_string());
        let shell = match (init, &cfg.protshell) {
            (Some(i), _) => (i.to_string(), args.to_string()),
            (None, Some(p)) if !textmode => p.clone(),
            (None, _) => (os2_shell.unwrap_or_else(|| "CMD.EXE".to_string()), String::new()),
        };
        if textmode {
            crate::println!("boot: text mode (textmode): shell {}", shell.0);
        }
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
        let mut background: Vec<(String, String)> = cfg.run.iter().map(|(p, a)| (find_program(p), a.clone())).collect();
        // The PM shell's SYS_DLLS LoadOneTime list: REXXINIT starts the
        // REXX API server thread. SYSDLLS keeps such modules resident.
        let sys_dlls: Vec<&str> = ["REXXINIT"].into_iter().filter(|m| crate::fs::find_module(m).is_ok()).collect();
        let loader = find_program("SYSDLLS.EXE");
        if !sys_dlls.is_empty() && crate::fs::read_file(&loader).is_some() {
            background.push((loader, sys_dlls.join(" ")));
        }
        // Before anything else writes to them: what is on them may be half
        // done. The drives the IFS lines do not list only get a warning.
        let listed = crate::config::autocheck(&cfg);
        let mut checks = Vec::new();
        for d in (0..26).filter(|&d| crate::fs::drive(d).is_some_and(|f| f.needs_check())) {
            let letter = (b'A' + d) as char;
            if listed.contains(&d) {
                checks.push((find_program("CHKDSK.EXE"), alloc::format!("{letter}: /F /AUTOCHECK")));
            } else {
                crate::println!("boot: drive {letter}: was not shut down cleanly; run CHKDSK {letter}: /F");
            }
        }
        let mut queue: VecDeque<(String, String)> = checks.iter().cloned().collect();
        queue.extend(cfg.call.iter().map(|(p, a)| (find_program(p), a.clone())));
        queue.push_back((shell_path.clone(), shell.1.clone()));
        let restart = init.is_none();
        *STATE.lock() = Some(Boot { env, queue, shell: (shell_path, shell.1), current: 0, restart, checks: checks.len(), background });
        next();
    }

    /// Networking as `net=` on the command line, else NETWORK= in
    /// CONFIG.SYS, else DHCP says.
    fn start_network(cmdline: &str, config_sys: Option<&str>) {
        let spec = cmdline.split_whitespace().find_map(|w| w.strip_prefix("net=")).or(config_sys).unwrap_or("dhcp");
        let cfg = crate::net::Config::parse(spec).unwrap_or_else(|| {
            crate::println!("net: cannot parse network configuration {spec:?}; using DHCP");
            crate::net::Config::Dhcp
        });
        crate::net::start(cfg);
    }

    fn spawn(path: &str, args: &str, env: &[String], detached: bool) -> Option<u32> {
        let r = crate::task::process::exec(crate::task::process::ExecParams {
            path,
            program: path,
            args,
            env: env.to_vec(),
            parent: None,
            handles: Vec::new(),
            detached,
            session: 1,
            traced: false,
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
            let (item, env, last, background) = {
                let mut g = STATE.lock();
                let Some(b) = g.as_mut() else { return };
                let last = b.queue.len() <= 1;
                let background = if b.checks == 0 { core::mem::take(&mut b.background) } else { Vec::new() };
                b.checks = b.checks.saturating_sub(1);
                if b.queue.is_empty() && !b.restart {
                    (None, b.env.clone(), last, background)
                } else {
                    let item = b.queue.pop_front().unwrap_or_else(|| b.shell.clone());
                    (Some(item), b.env.clone(), last, background)
                }
            };
            for (p, a) in &background {
                spawn(p, a, &env, true);
            }
            let Some(item) = item else { return };
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

    /// The boot environment (CONFIG.SYS SET statements).
    pub fn env() -> Vec<String> {
        STATE.lock().as_ref().map(|b| b.env.clone()).unwrap_or_default()
    }

    /// A top-level process ended.
    pub fn process_ended(pid: u32) {
        let ours = STATE.lock().as_ref().is_some_and(|b| b.current == pid);
        if ours {
            // The machine may be switched off now (test runs are).
            crate::fs::cache::sync_all();
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
    smp::halt_others();
    log::print_unlocked(format_args!("\nKERNEL PANIC: {info}\n"));
    let rbp: u64;
    unsafe { core::arch::asm!("mov {}, rbp", out(reg) rbp) };
    backtrace(rbp);
    arch::halt_forever();
}
