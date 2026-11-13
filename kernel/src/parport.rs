//! Parallel ports: the hardware behind LPT1-LPT3, as IBM's PRINT01.SYS
//! drives it (its disassembly studied with print01.sym; the notes are in
//! docs/PRINTING.md "The parallel port driver"), with the IEEE 1284
//! modes of IBM's PAR1284.SYS (known here from its symbol file).
//!
//! Ports are found through ACPI (PNP0400/PNP0401 devices: the I/O range
//! of `_CRS`, a second range at base+400h for the ECP registers, the
//! interrupt), PCI parallel cards (class 07h subclass 01h: BAR 0 the
//! port, BAR 1 the ECP registers) and, for what neither lists, by
//! probing the standard addresses (whose data latch reads back). The
//! BIOS data area that PRINT01 reads (40:08) is not filled under UEFI.
//! 378h is LPT1, 278h LPT2 and 3BCh LPT3; other ports take the free
//! names in the order found.
//!
//! Output (compatibility mode) puts each byte on the data lines and
//! strobes it once the printer reports ready (not busy, selected, paper,
//! no fault); the printer's acknowledge pulse interrupts (control bit
//! 10h, the port's IRQ through the IOAPIC) and wakes the writing thread,
//! which sleeps meanwhile. A port without a usable interrupt is polled a
//! timer tick at a time. As PRINT01, a printer that makes no progress for
//! the port's timeout (PRT_SETIRQTIMEOUT, seconds, 120 by default) ends
//! the write with an error from its status lines (20 no printer
//! answering, 28 out of paper, 29 write fault) through the hard-error
//! pop-up, unless infinite retry is on (PRT_SETINFINITERETRY), which
//! waits as long as it takes.
//!
//! Ports with an ECP chip (an extended control register at base+402h
//! that behaves as one) write through the chip's FIFO by default
//! (PRT_SETDATAXFERMODE 1: the chip handshakes each byte itself) and in
//! ECP mode when asked (PRT_SETCOMMMODE 4: IEEE 1284 negotiation, then
//! the ECP data FIFO; the compatibility FIFO when the printer refuses);
//! EPP ports write through the EPP data register (compatibility mode
//! when nothing answers). The IEEE 1284 device ID is read in byte mode
//! on bidirectional ports, else in nibble mode (PRT_QUERDEVICEID). The
//! protocol steps are in crates/parport (`ieee1284` here), host-tested
//! against a simulated port; this file gives them port I/O, sleeping
//! and the interrupt.
//!
//! Kernel command line: `lptpoll` polls every port (no interrupts);
//! `lptstall=BYTES,MS` makes the ports report "out of paper" for MS ms
//! after every BYTES bytes (testing the timeout: QEMU's port is never
//! busy).

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use ieee1284::{CAP_BYTE, CAP_ECP, CAP_EPP, CTR_IDLE, CTR_INTEN, CTR_SELECT, ECR_NOINTR, ECR_PS2, STS_ACK, STS_BUSY, STS_ERROR, STS_PAPER, STS_SELECT, Waited};

use crate::arch::idt::TrapFrame;
use crate::arch::{inb, outb};
use crate::sync::SpinLock;
use crate::task::WaitQueue;

pub const NPORTS: usize = 3;
const STD_BASES: [u16; NPORTS] = [0x378, 0x278, 0x3bc];

/// PRT_SETCOMMMODE / PRT_QUERYCOMMMODE values.
pub const COMM_COMPAT: u8 = 0;
pub const COMM_NIBBLE: u8 = 1;
pub const COMM_BYTE: u8 = 2;
pub const COMM_EPP: u8 = 3;
pub const COMM_ECP: u8 = 4;
/// PRT_SETDATAXFERMODE / PRT_QUERYDATAXFERMODE values.
pub const XFER_PIO: u8 = 0;
pub const XFER_FIFO: u8 = 1;

// Errors (OS/2 error codes; device errors are 19 + the driver's code).
const INVALID_FUNCTION: u32 = 1;
const INVALID_HANDLE: u32 = 6;
const BAD_COMMAND: u32 = ieee1284::BAD_COMMAND;
const INVALID_PARAMETER: u32 = 87;
const INTERRUPT: u32 = ieee1284::INTERRUPT;

/// PRT_GETPRINTERSTATUS bits.
const PRINTER_TIMEOUT: u8 = 0x01;
const PRINTER_SELECTED: u8 = 0x10;
const PRINTER_NOT_BUSY: u8 = 0x80;

/// The default timeout, seconds (PRINT01's, `/TOU:` on its DEVICE line).
const DEFAULT_TIMEOUT: u16 = 120;

/// A port found.
#[derive(Clone, Debug)]
pub struct Info {
    pub base: u16,
    /// The extended control register (0: none, not an ECP port).
    pub ecr: u16,
    /// ISA interrupt level (ACPI or the standard one), or for a PCI card
    /// the line through ACPI's routing.
    pub irq: Option<u8>,
    pub source: &'static str,
    pub pci: Option<crate::drivers::pci::Device>,
    pub caps: u8,
}

impl Info {
    fn regs(&self) -> ieee1284::Port {
        ieee1284::Port { base: self.base, ecr: self.ecr }
    }
}

struct Port {
    info: Option<Info>,
    /// Characters per line, lines per inch (PRT_SETFRAMECTL).
    frame: [u8; 2],
    infinite_retry: bool,
    timeout_s: u16,
    /// Active code page and font (PRT_ACTIVATEFONT).
    font: [u16; 2],
    title: Vec<u8>,
    comm: u8,
    xfer: u8,
    /// The last write ended on the timeout (PRINTER_TIMEOUT).
    timed_out: bool,
    /// The printer did not negotiate ECP mode, or no EPP peripheral
    /// answered: output in compatibility mode until the mode is set again
    /// or the printer is initialised.
    ecp_refused: bool,
    epp_failed: bool,
}

const fn port() -> Port {
    Port {
        info: None,
        frame: [80, 6],
        infinite_retry: false,
        timeout_s: DEFAULT_TIMEOUT,
        font: [0, 0],
        title: Vec::new(),
        comm: COMM_COMPAT,
        xfer: XFER_PIO,
        timed_out: false,
        ecp_refused: false,
        epp_failed: false,
    }
}

static PORTS: [SpinLock<Port>; NPORTS] = [const { SpinLock::new(port()) }; NPORTS];
static FOUND: AtomicBool = AtomicBool::new(false);
static PROBE: crate::sync::SpinLock<()> = crate::sync::SpinLock::new(());
/// Interrupt-driven: the handler is installed and ACK interrupts enabled.
static IRQ_ON: [AtomicBool; NPORTS] = [const { AtomicBool::new(false) }; NPORTS];
/// Interrupts taken, by port (the handler signals every interrupt-driven
/// port: the waiters check their status lines).
static SIGNALS: [AtomicU64; NPORTS] = [const { AtomicU64::new(0) }; NPORTS];
static WAIT: [WaitQueue; NPORTS] = [const { WaitQueue::new() }; NPORTS];
/// 0 not tried, 1 interrupt-driven, 2 polled.
static IRQ_STATE: [AtomicU8; NPORTS] = [const { AtomicU8::new(0) }; NPORTS];
/// One writer per port at a time; others sleep.
static OWNER: [AtomicBool; NPORTS] = [const { AtomicBool::new(false) }; NPORTS];
static OWNER_WAIT: WaitQueue = WaitQueue::new();
/// Bytes written by the hardware path, by port, and by interrupt.
static SENT: [AtomicU64; NPORTS] = [const { AtomicU64::new(0) }; NPORTS];
static ACKED: [AtomicU64; NPORTS] = [const { AtomicU64::new(0) }; NPORTS];

fn status(base: u16) -> u8 {
    if STALL_EVERY.load(Ordering::Relaxed) != 0 && crate::irq::ticks() < STALL_UNTIL.load(Ordering::Relaxed) {
        // A stalled printer (test): busy, out of paper, selected.
        return STS_PAPER | STS_SELECT | STS_ERROR | STS_ACK;
    }
    unsafe { inb(base + 1) }
}

// Testing the timeout without a printer that runs out of paper:
// `lptstall=BYTES,MS` on the kernel command line makes every port report
// "out of paper" for MS milliseconds after each BYTES bytes.
static STALL_EVERY: AtomicU64 = AtomicU64::new(0);
static STALL_MS: AtomicU64 = AtomicU64::new(0);
static STALL_UNTIL: AtomicU64 = AtomicU64::new(0);
static STALL_NEXT: AtomicU64 = AtomicU64::new(0);

/// Before byte number `sent` of a port: start a test stall when due.
fn stall_check(sent: u64) {
    let every = STALL_EVERY.load(Ordering::Relaxed);
    if every != 0 && sent >= STALL_NEXT.load(Ordering::Relaxed) {
        STALL_NEXT.store(sent + every, Ordering::Relaxed);
        STALL_UNTIL.store(crate::irq::ticks() + STALL_MS.load(Ordering::Relaxed) * crate::irq::HZ / 1000, Ordering::Relaxed);
        crate::println!("parport: test stall at byte {sent}");
    }
}

/// The port I/O the protocol code (crates/parport) runs on: the port's
/// registers (the status read through the test stall), waits that sleep
/// until the port's interrupt or a timer tick.
struct Hw {
    /// The LPT index (interrupt signals), or NPORTS while probing.
    n: usize,
    base: u16,
    /// Bytes sent before this write (the test stalls count them).
    sent: u64,
}

impl Hw {
    fn new(n: usize, base: u16) -> Hw {
        Hw { n, base, sent: if n < NPORTS { SENT[n].load(Ordering::Relaxed) } else { 0 } }
    }
}

impl ieee1284::Io for Hw {
    fn inb(&mut self, port: u16) -> u8 {
        if port == self.base + 1 { status(self.base) } else { unsafe { inb(port) } }
    }

    fn outb(&mut self, port: u16, v: u8) {
        unsafe { outb(port, v) }
    }

    fn deadline(&mut self, ms: u64) -> u64 {
        crate::irq::ticks() + (ms * crate::irq::HZ / 1000).max(1)
    }

    fn expired(&mut self, d: u64) -> bool {
        crate::irq::ticks() >= d
    }

    fn mark(&mut self) -> u64 {
        if self.n < NPORTS { SIGNALS[self.n].load(Ordering::Acquire) } else { 0 }
    }

    /// Sleep until the port's interrupt (re-checking every 50 ms) or,
    /// polled, a timer tick; yield where sleeping is not allowed.
    fn pause(&mut self, d: u64, mark: u64) -> bool {
        if !may_sleep() {
            crate::task::sched::yield_now();
            return true;
        }
        if crate::task::sched::current().exit_requested() {
            return false;
        }
        let n = self.n;
        let now = crate::irq::ticks();
        if n >= NPORTS {
            crate::task::sched::sleep_ms(1);
            return true;
        }
        let irq = IRQ_ON[n].load(Ordering::Acquire);
        let until = if irq { d.min(now + 50 * crate::irq::HZ / 1000) } else { now + 1 };
        WAIT[n].wait_uninterruptible_until(until, || SIGNALS[n].load(Ordering::Acquire) != mark);
        true
    }

    fn before_byte(&mut self, i: usize) {
        stall_check(self.sent + i as u64);
    }
}

fn caps_text(caps: u8) -> String {
    let mut s = String::from("SPP, nibble");
    for (b, t) in [(CAP_BYTE, ", byte"), (CAP_EPP, ", EPP"), (CAP_ECP, ", ECP")] {
        if caps & b != 0 {
            s.push_str(t);
        }
    }
    s
}

/// The ports ACPI lists: (base, ECR, IRQ).
fn acpi_ports() -> Vec<(u16, u16, Option<u8>)> {
    let mut v = Vec::new();
    for a in crate::acpi::devices() {
        if !matches!(a.hid.as_str(), "PNP0400" | "PNP0401") || a.absent {
            continue;
        }
        let res = a.crs.as_deref().map(crate::resmgr::crs_resources).unwrap_or_default();
        let ios: Vec<u16> = res.iter().filter_map(|r| if let crate::resmgr::Res::Io { base, .. } = r { Some(*base) } else { None }).collect();
        let irq = res.iter().find_map(|r| if let crate::resmgr::Res::Irq { level, .. } = r { Some(*level as u8) } else { None });
        let Some(&base) = ios.first() else { continue };
        let ecr = ios.iter().find(|&&b| b == base + 0x400).map_or(0, |b| b + 2);
        v.push((base, ecr, irq));
    }
    v
}

/// A PCI parallel port: class 07h subclass 01h with a programming
/// interface of a register-compatible port (00h SPP, 01h bidirectional,
/// 02h ECP; 03h, an IEEE 1284 controller, and FEh, a 1284 target, have
/// other registers). BAR 0 is the port, BAR 1 (when an I/O BAR) the ECP
/// registers: the ECR at its third byte, as on the NetMos/MosChip
/// 9805/9865 and Oxford OX16PCI952/OXPCIe952 parallel functions.
/// (base, ECR) or why not.
pub fn pci_port(class: u8, subclass: u8, progif: u8, bar0: u32, bar1: u32) -> Result<(u16, u16), &'static str> {
    if (class, subclass) != (7, 1) {
        return Err("not a parallel port");
    }
    if progif > 2 {
        return Err("not register-compatible (IEEE 1284 controller or target)");
    }
    if bar0 & 1 == 0 {
        return Err("memory-mapped registers");
    }
    let base = (bar0 & 0xfffc) as u16;
    if base == 0 {
        return Err("BAR 0 not assigned");
    }
    let ecr = if bar1 & 1 != 0 && bar1 & 0xfffc != 0 { (bar1 & 0xfffc) as u16 + 2 } else { 0 };
    Ok((base, ecr))
}

/// Find the ports (once, at boot after ACPI and PCI).
pub fn init() {
    // One probe; a caller on another CPU waits for it to finish.
    let _g = PROBE.lock();
    if FOUND.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(v) = crate::boot_info().cmdline().split_whitespace().find_map(|w| w.strip_prefix("lptstall=")) {
        let mut it = v.split(',').map(|x| x.parse::<u64>().unwrap_or(0));
        let (every, ms) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
        STALL_EVERY.store(every, Ordering::Relaxed);
        STALL_NEXT.store(every, Ordering::Relaxed);
        STALL_MS.store(ms, Ordering::Relaxed);
    }
    let probe = |base: u16| ieee1284::latch_ok(&mut Hw::new(NPORTS, base), &ieee1284::Port { base, ecr: 0 });
    let mut found: Vec<Info> = Vec::new();
    for (base, ecr, irq) in acpi_ports() {
        if probe(base) {
            found.push(Info { base, ecr, irq, source: "ACPI", pci: None, caps: 0 });
        }
    }
    for d in crate::drivers::pci::enumerate() {
        if d.class != 7 || d.subclass != 1 {
            continue;
        }
        let (bar0, bar1) = (d.read(0x10), d.read(0x14));
        let (base, ecr) = match pci_port(d.class, d.subclass, d.progif, bar0, bar1) {
            Ok(x) => x,
            Err(why) => {
                crate::println!("PCI parallel port {:04x}:{:04x} at {:02x}:{:02x}.{}: {why}, not used", d.vendor, d.device, d.bus, d.dev, d.func);
                continue;
            }
        };
        if found.iter().any(|f| f.base == base) {
            continue;
        }
        d.enable();
        if probe(base) {
            found.push(Info { base, ecr, irq: None, source: "PCI", pci: Some(d), caps: 0 });
        }
    }
    for (i, &base) in STD_BASES.iter().enumerate() {
        if !found.iter().any(|f| f.base == base) && probe(base) {
            // PRINT01: IRQ 5 for 278h, 7 for the others.
            found.push(Info { base, ecr: 0, irq: Some(if i == 1 { 5 } else { 7 }), source: "probe", pci: None, caps: 0 });
        }
    }
    // ECP registers where ACPI or the card did not say: at base+402h
    // (not on a PCI card, whose base+400h may be anything).
    for f in found.iter_mut() {
        let mut hw = Hw::new(NPORTS, f.base);
        if f.ecr == 0 && f.pci.is_none() && ieee1284::ecr_ok(&mut hw, f.base, f.base + 0x402) {
            f.ecr = f.base + 0x402;
        } else if f.ecr != 0 && !ieee1284::ecr_ok(&mut hw, f.base, f.ecr) {
            f.ecr = 0;
        }
        f.caps = ieee1284::capabilities(&mut hw, &f.regs());
    }
    // Names: the standard addresses keep theirs, others take free ones.
    let mut slots: [Option<Info>; NPORTS] = [None, None, None];
    let mut rest = Vec::new();
    for f in found {
        match STD_BASES.iter().position(|&b| b == f.base) {
            Some(i) if slots[i].is_none() => slots[i] = Some(f),
            _ => rest.push(f),
        }
    }
    for f in rest {
        match slots.iter().position(|s| s.is_none()) {
            Some(i) => slots[i] = Some(f),
            None => crate::println!("parallel port at {:#x} ({}): no LPT name left", f.base, f.source),
        }
    }
    for (i, s) in slots.into_iter().enumerate() {
        let Some(f) = s else { continue };
        crate::println!(
            "LPT{}: parallel port at {:#x} ({}), {}{}, IRQ {}, status {:#04x}",
            i + 1,
            f.base,
            f.source,
            caps_text(f.caps),
            if f.ecr != 0 { alloc::format!(", ECR at {:#x}", f.ecr) } else { String::new() },
            f.irq.map_or(String::from("none"), |q| alloc::format!("{q}")),
            status(f.base)
        );
        let mut p = PORTS[i].lock();
        // An ECP chip writes through its FIFO unless told otherwise.
        p.xfer = if f.caps & CAP_ECP != 0 { XFER_FIFO } else { XFER_PIO };
        p.info = Some(f);
    }
}

/// The port behind LPTn (0-based).
pub fn info(n: usize) -> Option<Info> {
    if n >= NPORTS {
        return None;
    }
    init();
    PORTS[n].lock().info.clone()
}

pub fn present(n: usize) -> bool {
    info(n).is_some()
}

// ---- Interrupts ----

fn irq_handler(_f: &mut TrapFrame) {
    for p in 0..NPORTS {
        if IRQ_ON[p].load(Ordering::Acquire) {
            ACKED[p].fetch_add(1, Ordering::Relaxed);
            SIGNALS[p].fetch_add(1, Ordering::AcqRel);
            WAIT[p].wake_all();
        }
    }
}

fn pci_handler(ctx: usize) {
    if ctx < NPORTS && IRQ_ON[ctx].load(Ordering::Acquire) {
        ACKED[ctx].fetch_add(1, Ordering::Relaxed);
        SIGNALS[ctx].fetch_add(1, Ordering::AcqRel);
        WAIT[ctx].wake_all();
    }
}

/// Install the interrupt of port `n` on first use (the IOAPIC is up by
/// then); polled when there is none or its line is taken.
fn setup_irq(n: usize, f: &Info) {
    if IRQ_STATE[n].load(Ordering::Acquire) != 0 {
        return;
    }
    // Writers on other CPUs may get here at once: one sets the interrupt
    // up, the others see it done.
    static SETUP: SpinLock<()> = SpinLock::new(());
    let _g = SETUP.lock();
    if IRQ_STATE[n].load(Ordering::Acquire) != 0 {
        return;
    }
    let on = if crate::boot_info().cmdline().split_whitespace().any(|w| w.eq_ignore_ascii_case("lptpoll")) {
        false
    } else if let Some(d) = &f.pci {
        crate::msi::setup_intx(d, pci_handler, n).is_some()
    } else if let Some(q) = f.irq.filter(|&q| q < 16 && q != 0 && q != 2) {
        // Other LPT ports may share the level; nothing else may.
        let shared = (0..NPORTS).any(|i| i != n && IRQ_STATE[i].load(Ordering::Acquire) == 1 && info(i).and_then(|x| x.irq) == Some(q));
        if shared || !crate::irq::registered(q) {
            crate::irq::register(q, irq_handler);
            true
        } else {
            false
        }
    } else {
        false
    };
    IRQ_STATE[n].store(if on { 1 } else { 2 }, Ordering::Release);
    IRQ_ON[n].store(on, Ordering::Release);
    match (on, f.irq) {
        (true, Some(q)) => crate::println!("LPT{}: interrupt-driven output on IRQ {q}", n + 1),
        (true, None) => crate::println!("LPT{}: interrupt-driven output on the card's PCI interrupt", n + 1),
        _ => crate::println!("LPT{}: polled output (no interrupt)", n + 1),
    }
}

/// Whether the current context may sleep.
fn may_sleep() -> bool {
    crate::smp::this().irq_depth.load(Ordering::Relaxed) == 0
        && crate::sync::held_spin_locks() == 0
        && crate::task::sched::current_opt().is_some_and(|t| !t.is_idle())
}

// ---- Output ----

struct Owner(usize);

impl Owner {
    fn take(n: usize) -> Owner {
        while OWNER[n].swap(true, Ordering::AcqRel) {
            if may_sleep() {
                OWNER_WAIT.wait_uninterruptible_until(crate::irq::ticks() + 100, || !OWNER[n].load(Ordering::Acquire));
            } else {
                crate::task::sched::yield_now();
            }
        }
        Owner(n)
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        OWNER[self.0].store(false, Ordering::Release);
        OWNER_WAIT.wake_all();
    }
}

/// How the next bytes go out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
    Spp,
    Fifo,
    Ecp,
    Epp,
}

/// The transfer for the port's modes: ECP mode (unless the printer
/// refused it: then the FIFO), EPP (unless nothing answered), the ECP
/// chip's compatibility FIFO, or a byte at a time.
fn path(p: &Port, caps: u8) -> Path {
    match p.comm {
        COMM_ECP if caps & CAP_ECP != 0 && !p.ecp_refused => Path::Ecp,
        COMM_EPP if caps & CAP_EPP != 0 && !p.epp_failed => Path::Epp,
        _ if p.xfer == XFER_FIFO && caps & CAP_ECP != 0 => Path::Fifo,
        _ => Path::Spp,
    }
}

/// Output to LPTn (0-based): (error, bytes taken). Without a port the
/// bytes go nowhere (counted, traced with `apitrace`).
pub fn write(n: usize, data: &[u8]) -> (u32, usize) {
    let Some(f) = info(n) else { return (0, data.len()) };
    setup_irq(n, &f);
    let _own = Owner::take(n);
    let regs = f.regs();
    let mut done = 0;
    loop {
        let (path, timeout, infinite) = {
            let p = PORTS[n].lock();
            (path(&p, f.caps), p.timeout_s.max(1) as u64 * 1000, p.infinite_retry)
        };
        let mut hw = Hw::new(n, f.base);
        let rest = &data[done..];
        let (k, r) = match path {
            Path::Ecp => match ieee1284::ecp_write(&mut hw, &regs, rest, timeout) {
                // The printer does not negotiate ECP: the compatibility
                // FIFO from now on.
                (0, Err(BAD_COMMAND)) => {
                    crate::println!("LPT{}: the printer does not negotiate ECP mode; compatibility mode through the FIFO", n + 1);
                    PORTS[n].lock().ecp_refused = true;
                    continue;
                }
                x => x,
            },
            Path::Epp => match ieee1284::epp_write(&mut hw, &regs, rest) {
                // No EPP peripheral answered: compatibility mode.
                (0, Err(_)) if done == 0 => {
                    crate::println!("LPT{}: no EPP peripheral answers; compatibility mode", n + 1);
                    PORTS[n].lock().epp_failed = true;
                    continue;
                }
                x => x,
            },
            Path::Fifo => ieee1284::fifo_write(&mut hw, &regs, rest, timeout),
            Path::Spp => {
                let ctr = CTR_IDLE | if IRQ_ON[n].load(Ordering::Acquire) { CTR_INTEN } else { 0 };
                ieee1284::spp_write(&mut hw, &regs, ctr, rest, timeout, infinite)
            }
        };
        done += k;
        let before = SENT[n].fetch_add(k as u64, Ordering::Relaxed);
        if k > 0 && (before == 0 || before / 4096 != (before + k as u64) / 4096) {
            let (s, a) = counters(n);
            crate::println!("LPT{}: {} bytes sent, {} acknowledge interrupts ({})", n + 1, s, a, if IRQ_ON[n].load(Ordering::Acquire) { "interrupt-driven" } else { "polled" });
        }
        match r {
            Ok(()) => {
                PORTS[n].lock().timed_out = false;
                return (0, done);
            }
            Err(e) if e == INTERRUPT => return (e, done),
            // Infinite retry (the FIFO and EPP paths stop on errors):
            // try again a little later, without a pop-up.
            Err(_) if infinite => {
                if crate::task::sched::current_opt().is_some_and(|t| t.exit_requested()) {
                    return (INTERRUPT, done);
                }
                if may_sleep() {
                    crate::task::sched::sleep_ms(100);
                }
                continue;
            }
            Err(e) => {
                PORTS[n].lock().timed_out = true;
                crate::println!("LPT{}: printer stopped after {} of {} bytes, status {:#04x}: error {}", n + 1, done, data.len(), status(f.base), e);
                let dev = alloc::format!("LPT{}", n + 1);
                match crate::api::harderr::popup_device(e, &dev) {
                    crate::api::harderr::Action::Retry => continue,
                    crate::api::harderr::Action::Fail => return (e, done),
                    crate::api::harderr::Action::End => return (INTERRUPT, done),
                }
            }
        }
    }
}

/// Pulse INIT low (PRINT01's `initialprt`: about 50 us), then wait up to
/// a second for the printer to come back not busy.
fn init_printer(n: usize, f: &Info) {
    let base = f.base;
    unsafe {
        if f.ecr != 0 {
            outb(f.ecr, ECR_PS2 | ECR_NOINTR);
        }
        outb(base + 2, CTR_SELECT);
        for _ in 0..60 {
            let _ = inb(base + 1);
        }
        outb(base + 2, CTR_IDLE | if IRQ_ON[n].load(Ordering::Acquire) { CTR_INTEN } else { 0 });
    }
    let mut hw = Hw::new(n, base);
    let _: Waited = ieee1284::wait_for(&mut hw, 1000, 32, |io| ieee1284::Io::inb(io, base + 1) & STS_BUSY != 0);
}

// ---- Category 5 IOCtls ----

/// The data packet bytes a "set" function reads.
pub fn input_len(f: u32) -> u32 {
    match f {
        0x42 | 0x4e => 2,
        0x44 | 0x52 | 0x53 => 1,
        0x48 | 0x6a => 4,
        _ => 0,
    }
}

/// Valid frames (PRINT01's table) and the bytes that select them on an
/// IBM/Epson printer: DC2 (10 cpi) or SI (condensed), ESC 2 (1/6 inch)
/// or ESC 0 (1/8 inch).
const FRAMES: [([u8; 2], [u8; 3]); 4] = [
    ([80, 6], [0x12, 0x1b, b'2']),
    ([80, 8], [0x12, 0x1b, b'0']),
    ([132, 6], [0x0f, 0x1b, b'2']),
    ([132, 8], [0x0f, 0x1b, b'0']),
];

/// A font that can be activated: the printer's own (code page 0, font
/// 0) or the system's prepared code pages with the printer's font.
fn font_ok(cp: u16, font: u16) -> bool {
    (cp == 0 && font == 0) || (font == 0 && crate::nls::is_prepared(cp))
}

/// No code page switching for this printer (PRINT01 without the
/// spooler's font monitor: a device-specific error 3).
const NO_FONT_SWITCH: u32 = 0xff03;

/// Category 5 on the port itself. `input` holds the data packet of the
/// "set" functions (`input_len`), `title` the job title of
/// PRT_SETPRINTJOBTITLE, `out_max` the data packet's size.
pub fn ioctl(n: usize, f: u32, input: &[u8], title: Option<&[u8]>, out_max: u32) -> Result<Vec<u8>, u32> {
    if n >= NPORTS {
        return Err(INVALID_HANDLE);
    }
    let need = input_len(f) as usize;
    if input.len() < need {
        return Err(INVALID_PARAMETER);
    }
    let hw = info(n);
    let word = |v: u16| v.to_le_bytes().to_vec();
    match f {
        // PRT_SETFRAMECTL: one of the four frames; the printer is told.
        0x42 => {
            let fr = [input[0], input[1]];
            let Some(&(_, seq)) = FRAMES.iter().find(|(x, _)| *x == fr) else { return Err(INVALID_PARAMETER) };
            PORTS[n].lock().frame = fr;
            if hw.is_some() {
                let (e, _) = write(n, &seq);
                if e != 0 {
                    return Err(e);
                }
            }
            Ok(Vec::new())
        }
        0x62 => Ok(PORTS[n].lock().frame.to_vec()),
        // PRT_SETINFINITERETRY: 0 or 1.
        0x44 => {
            if input[0] > 1 {
                return Err(INVALID_PARAMETER);
            }
            PORTS[n].lock().infinite_retry = input[0] == 1;
            Ok(Vec::new())
        }
        0x64 => Ok(alloc::vec![PORTS[n].lock().infinite_retry as u8]),
        // PRT_INITPRINTER: INIT pulse; the frame is the default again.
        0x46 => {
            if let Some(info) = &hw {
                setup_irq(n, info);
                let _own = Owner::take(n);
                init_printer(n, info);
            }
            let mut p = PORTS[n].lock();
            p.frame = [80, 6];
            p.ecp_refused = false;
            p.epp_failed = false;
            Ok(Vec::new())
        }
        // PRT_ACTIVATEFONT, PRT_QUERYACTIVEFONT, PRT_VERIFYFONT: CPID
        // (code page, font id).
        0x48 => {
            let cp = u16::from_le_bytes([input[0], input[1]]);
            let font = u16::from_le_bytes([input[2], input[3]]);
            if !font_ok(cp, font) {
                return Err(NO_FONT_SWITCH);
            }
            PORTS[n].lock().font = [cp, font];
            Ok(Vec::new())
        }
        0x69 => {
            let [cp, font] = PORTS[n].lock().font;
            let mut v = word(cp);
            v.extend_from_slice(&font.to_le_bytes());
            Ok(v)
        }
        0x6a => {
            let cp = u16::from_le_bytes([input[0], input[1]]);
            let font = u16::from_le_bytes([input[2], input[3]]);
            if font_ok(cp, font) { Ok(Vec::new()) } else { Err(NO_FONT_SWITCH) }
        }
        // PRT_SETPRINTJOBTITLE: ASCIIZ, at most 127 characters kept.
        0x4d => {
            let t = title.ok_or(INVALID_PARAMETER)?;
            let t: Vec<u8> = t.iter().copied().take_while(|&c| c != 0).take(127).collect();
            if crate::api::tracing() {
                crate::trace!("LPT{}: job title {:?}", n + 1, String::from_utf8_lossy(&t));
            }
            PORTS[n].lock().title = t;
            Ok(Vec::new())
        }
        // PRT_SETIRQTIMEOUT / PRT_QUERYIRQTIMEOUT: seconds.
        0x4e => {
            PORTS[n].lock().timeout_s = u16::from_le_bytes([input[0], input[1]]);
            Ok(Vec::new())
        }
        0x6e => Ok(word(PORTS[n].lock().timeout_s)),
        // PRT_SETCOMMMODE / PRT_QUERYCOMMMODE.
        0x52 => {
            let caps = hw.as_ref().map_or(0, |i| i.caps);
            let ok = match input[0] {
                COMM_COMPAT | COMM_NIBBLE => true,
                COMM_BYTE => caps & CAP_BYTE != 0,
                COMM_EPP => caps & CAP_EPP != 0,
                COMM_ECP => caps & CAP_ECP != 0,
                _ => false,
            };
            if !ok {
                return Err(INVALID_PARAMETER);
            }
            let mut p = PORTS[n].lock();
            p.comm = input[0];
            // The printer may take the mode now.
            p.ecp_refused = false;
            p.epp_failed = false;
            Ok(Vec::new())
        }
        0x72 => Ok(alloc::vec![PORTS[n].lock().comm]),
        // PRT_SETDATAXFERMODE / PRT_QUERYDATAXFERMODE: programmed I/O, or
        // the ECP chip's FIFO.
        0x53 => {
            let caps = hw.as_ref().map_or(0, |i| i.caps);
            match input[0] {
                XFER_PIO => {}
                XFER_FIFO if caps & CAP_ECP != 0 => {}
                _ => return Err(INVALID_PARAMETER),
            }
            PORTS[n].lock().xfer = input[0];
            Ok(Vec::new())
        }
        0x73 => Ok(alloc::vec![PORTS[n].lock().xfer]),
        // PRT_QUERDEVICEID: the IEEE 1284 device ID (byte or nibble mode).
        0x74 => {
            let Some(info) = &hw else { return Err(BAD_COMMAND) };
            setup_irq(n, info);
            let _own = Owner::take(n);
            let mut id = ieee1284::device_id(&mut Hw::new(n, info.base), &info.regs(), info.caps)?;
            if crate::api::tracing() {
                let text = id.get(2..).unwrap_or(&[]);
                crate::trace!("LPT{}: device ID {:?}", n + 1, String::from_utf8_lossy(text));
            }
            id.truncate(out_max as usize);
            Ok(id)
        }
        // PRT_GETPRINTERSTATUS: PRINT01's status (ACK and ERROR made
        // active high, the low bits clear) with the timeout bit after a
        // write that timed out; without a port selected and not busy.
        0x66 => {
            let timed_out = PORTS[n].lock().timed_out as u8 * PRINTER_TIMEOUT;
            let st = match &hw {
                Some(i) => ieee1284::ibm_status(status(i.base)) & 0xfc,
                None => PRINTER_NOT_BUSY | PRINTER_SELECTED,
            };
            Ok(alloc::vec![st | timed_out])
        }
        // PRT_QUERYJOBHANDLE: only spooled handles have a job (api/ioctl.rs).
        0x21 => Err(INVALID_FUNCTION),
        _ => Err(INVALID_FUNCTION),
    }
}

/// Category 5 on a handle the spooler serves: the printer is the
/// spooler's business; frame, retry, timeout and font settings are kept
/// for the port as the driver keeps them.
pub fn spool_ioctl(n: usize, f: u32, input: &[u8], title: Option<&[u8]>, out_max: u32) -> Result<Vec<u8>, u32> {
    match f {
        // Selected, not busy.
        0x66 => Ok(alloc::vec![PRINTER_NOT_BUSY | PRINTER_SELECTED]),
        // No output to the port: the frame is only remembered, the
        // printer is not reset.
        0x42 => {
            if input.len() < 2 {
                return Err(INVALID_PARAMETER);
            }
            let fr = [input[0], input[1]];
            if !FRAMES.iter().any(|(x, _)| *x == fr) {
                return Err(INVALID_PARAMETER);
            }
            PORTS[n.min(NPORTS - 1)].lock().frame = fr;
            Ok(Vec::new())
        }
        // The spooler drives the printer: no reset, no 1284 traffic.
        0x46 => Ok(Vec::new()),
        0x74 => Err(BAD_COMMAND),
        _ => ioctl(n.min(NPORTS - 1), f, input, title, out_max),
    }
}

/// Counters: (bytes sent, acknowledge interrupts).
pub fn counters(n: usize) -> (u64, u64) {
    if n >= NPORTS {
        return (0, 0);
    }
    (SENT[n].load(Ordering::Relaxed), ACKED[n].load(Ordering::Relaxed))
}
