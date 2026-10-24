//! A DOS virtual machine around the `x86emu` CPU: the PC hardware a DOS
//! program sees (memory, PIC, PIT, keyboard controller, CMOS, VGA,
//! speaker), a BIOS, a DOS 5-compatible kernel, XMS, EMS, a mouse driver,
//! a DPMI 0.9 host and a COMMAND.COM, all in Rust outside the emulated
//! CPU. The world outside comes in through [`Host`]: files, time, input,
//! the screen. See docs/DOS.md.
//!
//! BIOS and DOS services are entered through callbacks: every vector the
//! machine provides points to `FE 38 n n` in ROM, which stops the CPU
//! with the callback number; the machine services it on the guest's
//! registers and memory and resumes. Programs may hook and chain these
//! vectors as on a PC.

#![no_std]

extern crate alloc;

pub mod bios;
pub mod devices;
pub mod dos;
pub mod dpmi;
pub mod ems;
pub mod host;
pub mod hw;
pub mod mouse;
pub mod shell;
pub mod video;
pub mod xms;

use alloc::string::String;
use alloc::vec::Vec;

pub use dos::pack_dt;
pub use host::{Create, DateTime, DirEntry, Event, Host, OpenMode};
use hw::Hw;
use x86emu::{Cpu, Exit, CS, SS};

/// Settings of a DOS session (OS/2's DOS settings).
#[derive(Clone, Debug)]
pub struct Config {
    /// XMS memory in KB (XMS_MEMORY_LIMIT).
    pub xms_kb: u32,
    /// EMS memory in KB (EMS_MEMORY_LIMIT).
    pub ems_kb: u32,
    /// DPMI memory in KB (DPMI_MEMORY_LIMIT).
    pub dpmi_kb: u32,
    /// DOS_UMB: upper memory blocks for DOS.
    pub umb: bool,
    /// DOS_HIGH: DOS in the HMA (frees conventional memory).
    pub high: bool,
    /// An x87 coprocessor.
    pub fpu: bool,
    /// The DOS version reported (major, minor).
    pub version: (u8, u8),
    /// The environment of the first program (NAME=value).
    pub env: Vec<String>,
    /// Print each callback on the host log (debugging).
    pub trace: bool,
    /// The country code (COUNTRY=) and code page.
    pub country: u16,
    pub codepage: u16,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            xms_kb: 3072,
            ems_kb: 2048,
            dpmi_kb: 3072,
            umb: true,
            high: true,
            fpu: true,
            version: (20, 45),
            env: Vec::new(),
            trace: false,
            country: 1,
            codepage: 437,
        }
    }
}

/// How a callback ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cb {
    Done,
    /// The service must wait (no key yet): run it again after waiting.
    Retry,
}

pub struct Machine {
    pub cpu: Cpu,
    pub hw: Hw,
    pub cfg: Config,
    pub dos: dos::Dos,
    pub xms: xms::Xms,
    pub ems: ems::Ems,
    pub mouse: mouse::Mouse,
    pub dpmi: dpmi::Dpmi,
    pub shells: Vec<shell::Shell>,
    /// The first program ended with this exit code.
    pub exited: Option<u8>,
    /// Set by a service that wants the guest to take interrupt n next.
    pub chain_int: Option<u8>,
    /// Waiting for input (a service returned Retry, or HLT).
    pub waiting: bool,
    /// When the screen was last presented (microseconds).
    last_present: u64,
    /// Host time at the start (microseconds) and the PIT tick then.
    t0: u64,
    events: Vec<Event>,
    /// Font data in ROM: segment:offset of the 8x8, 8x14 and 8x16 fonts.
    pub fonts: [(u16, u16); 3],
    /// Consecutive keyboard polls without a key (idle detection).
    pub polls: u32,
    speaker_on: Option<u32>,
    /// The user closed the session.
    pub close_requested: bool,
}

impl Machine {
    pub fn new<H: Host>(host: &mut H, cfg: Config) -> Machine {
        let mut m = Machine {
            cpu: Cpu::new(),
            hw: Hw::new(),
            dos: dos::Dos::new(),
            xms: xms::Xms::default(),
            ems: ems::Ems::default(),
            mouse: mouse::Mouse::default(),
            dpmi: dpmi::Dpmi::default(),
            shells: Vec::new(),
            exited: None,
            chain_int: None,
            waiting: false,
            last_present: 0,
            t0: 0,
            events: Vec::new(),
            fonts: [(0, 0); 3],
            polls: 0,
            speaker_on: None,
            close_requested: false,
            cfg,
        };
        if m.cfg.fpu {
            m.cpu.fpu = Some(Default::default());
        }
        m.hw.umb = m.cfg.umb;
        m.t0 = host.micros();
        m.hw.now = host.now();
        m.bios_init(host);
        m.dos_init(host);
        m.xms_init();
        m.ems_init();
        m.dpmi_rom_init();
        m
    }

    /// Linear address of seg:off.
    #[inline]
    pub fn lin(seg: u16, off: u16) -> u32 {
        ((seg as u32) << 4) + off as u32
    }

    /// Update the clocks from the host.
    fn tick<H: Host>(&mut self, host: &mut H) {
        let us = host.micros().saturating_sub(self.t0);
        self.hw.us_now = us;
        self.hw.pit_now = Hw::pit_ticks(us);
        self.hw.now = host.now();
    }

    /// Deliver what is due: timer interrupts, keyboard bytes.
    fn devices(&mut self) {
        // IRQ 0: one per period that passed (catching up at most a few).
        let p = self.hw.pit.ch[0].period();
        let now = self.hw.pit_now;
        if now >= self.hw.pit.irq_done + p {
            let behind = (now - self.hw.pit.irq_done) / p;
            if behind > 4 {
                self.hw.pit.irq_done = now - p;
            } else {
                self.hw.pit.irq_done += p;
            }
            self.hw.pic.raise(0);
        }
        if self.hw.kbc.refill() {
            self.hw.pic.raise(1);
        }
        self.mouse_irq();
        if self.hw.speaker_changed {
            self.hw.speaker_changed = false;
        }
    }

    /// Microseconds until the next timer interrupt.
    fn next_timer_us(&self) -> u64 {
        let p = self.hw.pit.ch[0].period();
        let due = self.hw.pit.irq_done + p;
        let left = due.saturating_sub(self.hw.pit_now);
        left * 1_000_000 / devices::PIT_HZ
    }

    fn take_events<H: Host>(&mut self, host: &mut H) {
        self.events.clear();
        host.poll(&mut self.events);
        let ev = core::mem::take(&mut self.events);
        for e in &ev {
            match *e {
                Event::Key { scan, ch } => {
                    self.hw.kbc.queue.push_back((scan, ch));
                    self.polls = 0;
                    self.waiting = false;
                }
                Event::Mouse { dx, dy, buttons } => {
                    self.mouse_motion(dx, dy, buttons);
                    self.waiting = false;
                }
                Event::MouseAbs { x, y, buttons } => {
                    self.mouse_abs(x, y, buttons);
                    self.waiting = false;
                }
                Event::Close => self.close_requested = true,
            }
        }
        self.events = ev;
    }

    /// Run until the first program ends; its exit code.
    pub fn run<H: Host>(&mut self, host: &mut H) -> u8 {
        loop {
            if let Some(c) = self.exited {
                host.speaker(None);
                return c;
            }
            if self.close_requested {
                return 0;
            }
            self.step(host);
        }
    }

    /// One slice: some instructions and the device work between them.
    pub fn step<H: Host>(&mut self, host: &mut H) {
        self.tick(host);
        self.take_events(host);
        self.devices();
        let spk = self.hw.speaker();
        if spk != self.speaker_on {
            self.speaker_on = spk;
            host.speaker(spk);
        }
        if self.hw.us_now >= self.last_present + 15_000 {
            self.last_present = self.hw.us_now;
            self.present(host);
        }
        if self.waiting || (self.cpu.halted && !self.hw.pic.pending()) {
            // Sleep until the next timer interrupt or input.
            let t = self.next_timer_us().clamp(500, 20_000);
            self.present(host);
            host.wait(t);
            self.waiting = false;
            return;
        }
        for _ in 0..64 {
            let e = self.cpu.run(&mut self.hw, 2000);
            match e {
                Exit::Budget => {}
                Exit::Halt => {
                    if !self.hw.pic.pending() {
                        return;
                    }
                }
                Exit::Callback(n) => {
                    let r = self.callback(host, n);
                    if let Some(v) = self.chain_int.take() {
                        let _ = self.cpu.interrupt(&mut self.hw, v);
                    }
                    if r == Cb::Retry {
                        // Back to the callback instruction, interrupts on.
                        self.cpu.eip = self.cpu.eip.wrapping_sub(4);
                        self.cpu.flags |= x86emu::flags::IF;
                        self.waiting = true;
                        return;
                    }
                    if self.exited.is_some() {
                        return;
                    }
                }
                Exit::Shutdown => {
                    host.log("dos: the CPU shut down (triple fault)");
                    self.exited = Some(255);
                    return;
                }
            }
            // Keep timer interrupts flowing during long computations.
            self.tick(host);
            let p = self.hw.pit.ch[0].period();
            if self.hw.pit_now >= self.hw.pit.irq_done + p {
                return;
            }
        }
    }

    pub fn present<H: Host>(&mut self, host: &mut H) {
        host.present(&self.hw.vga, &self.hw.ram);
        self.hw.vga.dirty = false;
        self.hw.vga.palette_dirty = false;
    }

    /// Service callback `n`.
    fn callback<H: Host>(&mut self, host: &mut H, n: u16) -> Cb {
        if self.cfg.trace {
            let (rcs, rip) = (self.ret_cs(), self.ret_ip());
            let msg = alloc::format!(
                "cb {n:03x} ax={:04x} bx={:04x} cx={:04x} dx={:04x} ds={:04x} es={:04x} from {:04x}:{:04x}",
                self.cpu.ax(),
                self.cpu.bx(),
                self.cpu.cx(),
                self.cpu.dx(),
                self.cpu.seg[x86emu::DS].sel,
                self.cpu.seg[x86emu::ES].sel,
                rcs,
                rip
            );
            host.log(&msg);
        }
        match n {
            0..=0xff => self.interrupt(host, n as u8),
            bios::CB_TERMINATED => {
                self.dos_terminated(host);
                Cb::Done
            }
            xms::CB_XMS => {
                self.xms_call();
                Cb::Done
            }
            shell::CB_SHELL => self.shell_step(host),
            bios::CB_MOUSE_RET => {
                self.mouse_handler_done();
                Cb::Done
            }
            dpmi::CB_FIRST..=dpmi::CB_LAST => self.dpmi_callback(host, n),
            _ => {
                host.log(&alloc::format!("dos: unknown callback {n:#x}"));
                Cb::Done
            }
        }
    }

    /// A software or hardware interrupt the machine serves.
    fn interrupt<H: Host>(&mut self, host: &mut H, v: u8) -> Cb {
        match v {
            0x00 => self.fatal_exception(host, "Divide overflow"),
            0x06 => self.fatal_exception(host, "Invalid instruction"),
            0x08 => {
                self.bios_timer();
                Cb::Done
            }
            0x09 => {
                self.bios_keyboard_irq();
                Cb::Done
            }
            0x0d if self.hw.pic.m.isr & 0x20 == 0 => self.fatal_exception(host, "General protection fault"),
            0x0c if self.hw.pic.m.isr & 0x10 == 0 => self.fatal_exception(host, "Stack fault"),
            0x0a..=0x0f => {
                self.eoi(v - 8);
                Cb::Done
            }
            0x10 => {
                self.int10(host);
                Cb::Done
            }
            0x11 => {
                let e = self.hw.rd16(0x410);
                self.cpu.set_ax(e);
                Cb::Done
            }
            0x12 => {
                let m = self.hw.rd16(0x413);
                self.cpu.set_ax(m);
                Cb::Done
            }
            0x13 => {
                self.int13();
                Cb::Done
            }
            0x14 => {
                // Serial: time-out.
                self.cpu.set_ah(0x80);
                Cb::Done
            }
            0x15 => self.int15(host),
            0x16 => self.int16(),
            0x17 => {
                self.cpu.set_ah(0x90);
                Cb::Done
            }
            0x19 => {
                self.exited = Some(0);
                Cb::Done
            }
            0x1a => {
                self.int1a();
                Cb::Done
            }
            0x20..=0x2f => self.dos_interrupt(host, v),
            0x33 => {
                self.int33(host);
                Cb::Done
            }
            0x67 => {
                self.int67();
                Cb::Done
            }
            0x74 => {
                self.mouse_service();
                Cb::Done
            }
            0x70..=0x77 => {
                self.eoi(v - 0x70 + 8);
                Cb::Done
            }
            _ => Cb::Done,
        }
    }

    /// End-of-interrupt for `irq`, as the BIOS's default handlers send.
    pub fn eoi(&mut self, irq: u8) {
        if irq >= 8 {
            self.hw.pic.io_write(0xa0, 0x20);
        }
        self.hw.pic.io_write(0x20, 0x20);
    }

    /// A CPU exception nobody handles: end the program with a message.
    fn fatal_exception<H: Host>(&mut self, host: &mut H, what: &str) -> Cb {
        let msg = alloc::format!("\r\n{what} at {:04x}:{:04x}\r\n", self.ret_cs(), self.ret_ip());
        host.log(msg.trim());
        for b in msg.bytes() {
            self.tty_out(b);
        }
        self.dos_exit(host, 0xff, 0);
        Cb::Done
    }

    // ---- The interrupt frame of a callback ----

    /// Return address of the interrupt that entered the callback.
    pub fn ret_ip(&mut self) -> u16 {
        let a = self.cpu.lin(SS, self.cpu.stack_ptr());
        self.hw.rd16(a)
    }
    pub fn ret_cs(&mut self) -> u16 {
        let a = self.cpu.lin(SS, self.cpu.stack_ptr().wrapping_add(2));
        self.hw.rd16(a)
    }

    /// Set or clear a flag in the flags the callback's IRET restores.
    pub fn set_ret_flag(&mut self, f: u32, on: bool) {
        let a = self.cpu.lin(SS, self.cpu.stack_ptr().wrapping_add(4));
        let mut v = self.hw.rd16(a);
        if on {
            v |= f as u16;
        } else {
            v &= !(f as u16);
        }
        self.hw.wr16(a, v);
        self.cpu.set_flag(f, on);
    }

    /// Carry clear (success) or set with AX = error.
    pub fn ret_cf(&mut self, r: Result<(), u16>) {
        match r {
            Ok(()) => self.set_ret_flag(x86emu::flags::CF, false),
            Err(e) => {
                self.cpu.set_ax(e);
                self.set_ret_flag(x86emu::flags::CF, true);
            }
        }
    }

    /// The flags the callback's IRET restores.
    pub fn ret_flags(&mut self) -> u16 {
        let a = self.cpu.lin(SS, self.cpu.stack_ptr().wrapping_add(4));
        self.hw.rd16(a)
    }

    /// Start a program (path, command tail) as the session's first.
    pub fn start<H: Host>(&mut self, host: &mut H, path: &str, args: &str) -> Result<(), u16> {
        self.dos_start(host, path, args)
    }

    /// CS of the code running now (for diagnostics).
    pub fn where_now(&self) -> (u16, u32) {
        (self.cpu.seg[CS].sel, self.cpu.eip)
    }
}
