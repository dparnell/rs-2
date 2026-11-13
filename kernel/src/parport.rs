//! Parallel ports: the hardware behind LPT1-LPT3, as IBM's PRINT01.SYS
//! drives it (its disassembly studied with print01.sym; the notes are in
//! docs/PRINTING.md "The parallel port driver").
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
//! that behaves as one) can be switched to FIFO transfers
//! (PRT_SETDATAXFERMODE 1: the chip handshakes each byte itself) and to
//! ECP mode (IEEE 1284 negotiation, then the FIFO); EPP ports write
//! through the EPP data register. The IEEE 1284 device ID is read in
//! nibble mode (PRT_QUERDEVICEID).
//!
//! Kernel command line: `lptpoll` polls every port (no interrupts);
//! `lptstall=BYTES,MS` makes the ports report "out of paper" for MS ms
//! after every BYTES bytes (testing the timeout: QEMU's port is never
//! busy).

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use crate::arch::idt::TrapFrame;
use crate::arch::{inb, outb};
use crate::sync::SpinLock;
use crate::task::WaitQueue;

pub const NPORTS: usize = 3;
const STD_BASES: [u16; NPORTS] = [0x378, 0x278, 0x3bc];

// Control register (base+2). STROBE, AUTOFD and SELECTIN are inverted
// on the cable: a 1 drives the line low.
const CTR_STROBE: u8 = 0x01;
const CTR_AUTOFD: u8 = 0x02;
const CTR_INIT: u8 = 0x04;
const CTR_SELECT: u8 = 0x08;
const CTR_INTEN: u8 = 0x10;
const CTR_DIR: u8 = 0x20;
/// Idle: not resetting, printer selected.
const CTR_IDLE: u8 = CTR_INIT | CTR_SELECT;

// Status register (base+1), as read: BUSY reads 0 while busy, ACK and
// ERROR are active low.
const STS_EPP_TIMEOUT: u8 = 0x01;
const STS_ERROR: u8 = 0x08;
const STS_SELECT: u8 = 0x10;
const STS_PAPER: u8 = 0x20;
const STS_ACK: u8 = 0x40;
const STS_BUSY: u8 = 0x80;

// Extended control register modes (bits 7-5) and bits.
const ECR_SPP: u8 = 0x00;
const ECR_PS2: u8 = 0x20;
const ECR_FIFO: u8 = 0x40;
const ECR_ECP: u8 = 0x60;
const ECR_EPP: u8 = 0x80;
/// nErrIntrEn and serviceIntr set: no ECP interrupts.
const ECR_NOINTR: u8 = 0x14;
const ECR_EMPTY: u8 = 0x01;
const ECR_FULL: u8 = 0x02;

/// Capabilities beyond compatibility and nibble mode (every port).
pub const CAP_BYTE: u8 = 0x01;
pub const CAP_EPP: u8 = 0x02;
pub const CAP_ECP: u8 = 0x04;

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
/// Device error 1 (unknown unit): PRINT01's for a printer that answers
/// with "not busy" but is not ready.
const BAD_UNIT: u32 = 20;
const BAD_COMMAND: u32 = 22;
const OUT_OF_PAPER: u32 = 28;
const WRITE_FAULT: u32 = 29;
const INVALID_PARAMETER: u32 = 87;
const INTERRUPT: u32 = 95;

/// PRT_GETPRINTERSTATUS bits.
const PRINTER_TIMEOUT: u8 = 0x01;
const PRINTER_IO_ERROR: u8 = 0x08;
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
    }
}

static PORTS: [SpinLock<Port>; NPORTS] = [const { SpinLock::new(port()) }; NPORTS];
static FOUND: AtomicBool = AtomicBool::new(false);
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

/// PRINT01's view of the status lines: ACK and ERROR made active high.
fn ibm_status(raw: u8) -> u8 {
    (raw & 0xf8) ^ (STS_ACK | STS_ERROR)
}

/// The data latch reads back what was written.
fn latch_ok(base: u16) -> bool {
    unsafe {
        let ctr = inb(base + 2);
        outb(base + 2, ctr & !CTR_DIR);
        outb(base, 0x55);
        let a = inb(base);
        outb(base, 0xaa);
        let b = inb(base);
        a == 0x55 && b == 0xaa
    }
}

/// An ECP chip's extended control register at `ecr` (the test Linux's
/// parport_pc uses: a register that only mirrors the control register is
/// not one; after a reset the FIFO is empty and mode writes read back).
fn ecr_ok(base: u16, ecr: u16) -> bool {
    unsafe {
        let ctr = inb(base + 2);
        let e = inb(ecr);
        if e & 3 == ctr & 3 {
            outb(base + 2, ctr ^ CTR_AUTOFD);
            let c2 = inb(base + 2);
            let mirrors = inb(ecr) & 2 == c2 & 2;
            outb(base + 2, ctr);
            if mirrors {
                return false;
            }
        }
        if inb(ecr) & 3 != ECR_EMPTY {
            return false;
        }
        outb(ecr, 0x34);
        let ok = inb(ecr) == 0x35;
        outb(ecr, ECR_PS2 | ECR_NOINTR);
        ok
    }
}

/// Clear the EPP timeout bit (chips clear it on a read, by writing 1, or
/// by writing 0); whether it is clear now.
fn clear_epp_timeout(base: u16) -> bool {
    unsafe {
        if inb(base + 1) & STS_EPP_TIMEOUT == 0 {
            return true;
        }
        let _ = inb(base + 1);
        let r = inb(base + 1);
        outb(base + 1, r | STS_EPP_TIMEOUT);
        outb(base + 1, r & !STS_EPP_TIMEOUT);
        inb(base + 1) & STS_EPP_TIMEOUT == 0
    }
}

/// EPP: an address cycle with nobody answering times out (the status
/// timeout bit sets), which an SPP port never reports.
fn epp_ok(base: u16) -> bool {
    if !clear_epp_timeout(base) {
        return false;
    }
    unsafe {
        outb(base + 2, CTR_INIT);
        let _ = inb(base + 3);
        let t = inb(base + 1) & STS_EPP_TIMEOUT != 0;
        clear_epp_timeout(base);
        outb(base + 2, CTR_IDLE);
        t
    }
}

/// Bidirectional (PS/2 byte mode): with the direction bit set the data
/// lines float and no longer read back the latch.
fn byte_ok(base: u16) -> bool {
    unsafe {
        let ctr = inb(base + 2);
        outb(base + 2, ctr | CTR_DIR);
        outb(base, 0x55);
        let a = inb(base);
        outb(base, 0xaa);
        let b = inb(base);
        outb(base + 2, ctr & !CTR_DIR);
        !(a == 0x55 && b == 0xaa)
    }
}

/// What the port can do (the ECP chip set to PS/2 mode for the tests).
fn capabilities(base: u16, ecr: u16) -> u8 {
    let mut caps = 0;
    if ecr != 0 {
        caps |= CAP_ECP;
        unsafe { outb(ecr, ECR_EPP | ECR_NOINTR) };
        if epp_ok(base) {
            caps |= CAP_EPP;
        }
        unsafe { outb(ecr, ECR_PS2 | ECR_NOINTR) };
    } else if epp_ok(base) {
        caps |= CAP_EPP;
    }
    if byte_ok(base) {
        caps |= CAP_BYTE;
    }
    caps
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

/// Find the ports (once, at boot after ACPI and PCI).
pub fn init() {
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
    let mut found: Vec<Info> = Vec::new();
    for (base, ecr, irq) in acpi_ports() {
        if latch_ok(base) {
            found.push(Info { base, ecr, irq, source: "ACPI", pci: None, caps: 0 });
        }
    }
    for d in crate::drivers::pci::enumerate() {
        if (d.class, d.subclass) != (7, 1) {
            continue;
        }
        let b0 = d.read(0x10);
        if b0 & 1 == 0 {
            continue;
        }
        let base = (b0 & 0xfffc) as u16;
        if found.iter().any(|f| f.base == base) {
            continue;
        }
        let b1 = d.read(0x14);
        let ecr = if b1 & 1 != 0 && b1 & 0xfffc != 0 { (b1 & 0xfffc) as u16 + 2 } else { 0 };
        d.enable();
        if latch_ok(base) {
            found.push(Info { base, ecr, irq: None, source: "PCI", pci: Some(d), caps: 0 });
        }
    }
    for (i, &base) in STD_BASES.iter().enumerate() {
        if !found.iter().any(|f| f.base == base) && latch_ok(base) {
            // PRINT01: IRQ 5 for 278h, 7 for the others.
            found.push(Info { base, ecr: 0, irq: Some(if i == 1 { 5 } else { 7 }), source: "probe", pci: None, caps: 0 });
        }
    }
    // ECP registers where ACPI or the card did not say: at base+402h.
    for f in found.iter_mut() {
        if f.ecr == 0 && f.pci.is_none() && ecr_ok(f.base, f.base + 0x402) {
            f.ecr = f.base + 0x402;
        } else if f.ecr != 0 && !ecr_ok(f.base, f.ecr) {
            f.ecr = 0;
        }
        f.caps = capabilities(f.base, f.ecr);
        unsafe { outb(f.base + 2, CTR_IDLE) };
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
        PORTS[i].lock().info = Some(f);
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
    // Writers on other CPUs may get here at once (DosWrite runs without
    // the legacy lock): one sets the interrupt up, the others see it done.
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

enum Waited {
    Ready,
    Timeout,
    Killed,
}

/// Wait up to `ms` for `cond` (which reads the port): a few quick polls,
/// then sleeping until the port's interrupt (re-checking every 50 ms) or,
/// polled, a timer tick at a time.
fn wait_for(n: usize, ms: u64, mut cond: impl FnMut() -> bool) -> Waited {
    for _ in 0..32 {
        if cond() {
            return Waited::Ready;
        }
    }
    let hz = crate::irq::HZ;
    let deadline = crate::irq::ticks() + (ms * hz / 1000).max(1);
    let irq = IRQ_ON[n].load(Ordering::Acquire);
    loop {
        if cond() {
            return Waited::Ready;
        }
        let now = crate::irq::ticks();
        if now >= deadline {
            return if cond() { Waited::Ready } else { Waited::Timeout };
        }
        if !may_sleep() {
            crate::task::sched::yield_now();
            continue;
        }
        if crate::task::sched::current().exit_requested() {
            return Waited::Killed;
        }
        let until = if irq { deadline.min(now + 50 * hz / 1000) } else { now + 1 };
        let seen = SIGNALS[n].load(Ordering::Acquire);
        WAIT[n].wait_uninterruptible_until(until, || SIGNALS[n].load(Ordering::Acquire) != seen);
    }
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

/// PRINT01's error for a printer that stopped (`prterp`): its status
/// lines say out of paper (28), a fault or nothing answering (29 write
/// fault), or a printer that is not busy but not ready either (20).
fn stop_error(raw: u8) -> u32 {
    let ah = ibm_status(raw);
    let (nb, pe, sel, fault) = (ah & 0x80 != 0, ah & 0x20 != 0, ah & 0x10 != 0, ah & 0x08 != 0);
    if nb && pe && sel {
        BAD_UNIT
    } else if !nb && pe && ((sel && !fault) || (!sel && fault)) {
        OUT_OF_PAPER
    } else if !nb && (sel || fault) || ah == 0 {
        WRITE_FAULT
    } else {
        BAD_UNIT
    }
}

/// The printer takes a byte: not busy, selected, paper, no fault (or
/// not selected while sending DC1, which selects it).
fn ready(raw: u8, byte: u8) -> bool {
    let ah = ibm_status(raw);
    ah == 0x90 || (ah == 0x80 && byte == 0x11)
}

/// Output to LPTn (0-based): (error, bytes taken). Without a port the
/// bytes go nowhere (counted, traced with `apitrace`).
pub fn write(n: usize, data: &[u8]) -> (u32, usize) {
    let Some(f) = info(n) else { return (0, data.len()) };
    setup_irq(n, &f);
    let _own = Owner::take(n);
    let mut done = 0;
    loop {
        let (mode, xfer, timeout, infinite) = {
            let p = PORTS[n].lock();
            (p.comm, p.xfer, p.timeout_s.max(1) as u64 * 1000, p.infinite_retry)
        };
        let (k, r) = match mode {
            COMM_ECP if f.caps & CAP_ECP != 0 => ecp_write(n, &f, &data[done..], timeout, true),
            COMM_EPP if f.caps & CAP_EPP != 0 => epp_write(&f, &data[done..]),
            _ if xfer == XFER_FIFO && f.caps & CAP_ECP != 0 => ecp_write(n, &f, &data[done..], timeout, false),
            _ => spp_write(n, &f, &data[done..], timeout, infinite),
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

/// Compatibility mode, a byte at a time: wait until the printer is
/// ready, data, strobe; the acknowledge pulse interrupts.
fn spp_write(n: usize, f: &Info, data: &[u8], timeout_ms: u64, infinite: bool) -> (usize, Result<(), u32>) {
    let base = f.base;
    let ctr = CTR_IDLE | if IRQ_ON[n].load(Ordering::Acquire) { CTR_INTEN } else { 0 };
    if f.ecr != 0 {
        unsafe { outb(f.ecr, ECR_PS2 | ECR_NOINTR) };
    }
    unsafe { outb(base + 2, ctr) };
    let sent = SENT[n].load(Ordering::Relaxed);
    for (i, &b) in data.iter().enumerate() {
        stall_check(sent + i as u64);
        loop {
            match wait_for(n, timeout_ms, || ready(status(base), b)) {
                Waited::Ready => break,
                Waited::Killed => return (i, Err(INTERRUPT)),
                // Infinite retry: PRINT01 keeps waiting and says nothing.
                Waited::Timeout if infinite => continue,
                Waited::Timeout => return (i, Err(stop_error(status(base)))),
            }
        }
        unsafe {
            outb(base, b);
            // The data settles (at least 0.5 us), a strobe of at least
            // 0.5 us: port reads take about a microsecond.
            let _ = inb(base + 1);
            outb(base + 2, ctr | CTR_STROBE);
            let _ = inb(base + 1);
            let _ = inb(base + 1);
            outb(base + 2, ctr);
        }
    }
    // The last byte taken (or the interrupt), briefly.
    let _ = wait_for(n, 10, || status(base) & STS_BUSY != 0);
    (data.len(), Ok(()))
}

/// EPP: each byte through the EPP data register; the chip handshakes
/// and reports a peripheral that does not answer as a timeout.
fn epp_write(f: &Info, data: &[u8]) -> (usize, Result<(), u32>) {
    let base = f.base;
    if f.ecr != 0 {
        unsafe { outb(f.ecr, ECR_EPP | ECR_NOINTR) };
    }
    unsafe { outb(base + 2, CTR_INIT) };
    clear_epp_timeout(base);
    for (i, &b) in data.iter().enumerate() {
        unsafe { outb(base + 4, b) };
        if status(base) & STS_EPP_TIMEOUT != 0 {
            clear_epp_timeout(base);
            unsafe { outb(base + 2, CTR_IDLE) };
            return (i, Err(WRITE_FAULT));
        }
    }
    unsafe { outb(base + 2, CTR_IDLE) };
    (data.len(), Ok(()))
}

/// Through the ECP chip's FIFO: compatibility mode with the hardware
/// handshake (ECR mode 010), or ECP mode (011) after negotiating it.
fn ecp_write(n: usize, f: &Info, data: &[u8], timeout_ms: u64, ecp: bool) -> (usize, Result<(), u32>) {
    let (base, ecr) = (f.base, f.ecr);
    let fifo = ecr - 2;
    unsafe { outb(ecr, ECR_PS2 | ECR_NOINTR) };
    if ecp {
        if negotiate(n, base, 0x10).is_err() {
            terminate(n, base);
            return (0, Err(WRITE_FAULT));
        }
        // Setup phase: HostAck high (forward data), then the FIFO
        // runs the transfers.
        unsafe { outb(base + 2, CTR_INIT) };
        if !matches!(wait_for(n, 35, || status(base) & STS_PAPER != 0), Waited::Ready) {
            terminate(n, base);
            return (0, Err(WRITE_FAULT));
        }
        unsafe { outb(ecr, ECR_ECP | ECR_NOINTR) };
    } else {
        unsafe {
            outb(base + 2, CTR_IDLE);
            outb(ecr, ECR_FIFO | ECR_NOINTR);
        }
    }
    let mut sent = 0;
    let mut r = Ok(());
    for &b in data {
        match wait_for(n, timeout_ms, || unsafe { inb(ecr) } & ECR_FULL == 0) {
            Waited::Ready => {}
            Waited::Killed => {
                r = Err(INTERRUPT);
                break;
            }
            Waited::Timeout => {
                r = Err(stop_error(status(base)));
                break;
            }
        }
        unsafe { outb(fifo, b) };
        sent += 1;
    }
    if r.is_ok() {
        // Drained, and the printer took the last byte.
        let empty = wait_for(n, timeout_ms, || unsafe { inb(ecr) } & ECR_EMPTY != 0 && status(base) & STS_BUSY != 0);
        if !matches!(empty, Waited::Ready) {
            r = Err(stop_error(status(base)));
        }
    }
    unsafe { outb(ecr, ECR_PS2 | ECR_NOINTR) };
    if ecp {
        terminate(n, base);
    }
    unsafe { outb(base + 2, CTR_IDLE) };
    (sent, r)
}

// ---- IEEE 1284 ----

/// Negotiate extensibility mode `xflag` (events 0-6): the peripheral
/// answers within 35 ms (nAck low, PError, Select and nFault high) and
/// says with Select whether it supports the mode.
fn negotiate(n: usize, base: u16, xflag: u8) -> Result<(), u32> {
    unsafe {
        outb(base + 2, CTR_IDLE);
        outb(base, xflag);
        // nSelectIn high, nAutoFd low.
        outb(base + 2, CTR_INIT | CTR_AUTOFD);
    }
    let answer = |s: u8| s & (STS_ACK | STS_PAPER | STS_SELECT | STS_ERROR) == (STS_PAPER | STS_SELECT | STS_ERROR);
    if !matches!(wait_for(n, 35, || answer(status(base))), Waited::Ready) {
        unsafe { outb(base + 2, CTR_IDLE) };
        return Err(BAD_COMMAND);
    }
    unsafe {
        // Strobe the extensibility byte in, then nStrobe and nAutoFd high.
        outb(base + 2, CTR_INIT | CTR_AUTOFD | CTR_STROBE);
        let _ = inb(base + 1);
        outb(base + 2, CTR_INIT);
    }
    if !matches!(wait_for(n, 35, || status(base) & STS_ACK != 0), Waited::Ready) {
        unsafe { outb(base + 2, CTR_IDLE) };
        return Err(BAD_COMMAND);
    }
    let sel = status(base) & STS_SELECT != 0;
    // Nibble mode (0) is accepted with Select low, the others high.
    if (xflag == 0) == sel {
        terminate(n, base);
        return Err(BAD_COMMAND);
    }
    Ok(())
}

/// Back to compatibility mode (events 22-28).
fn terminate(n: usize, base: u16) {
    unsafe { outb(base + 2, CTR_INIT | CTR_SELECT) };
    let _ = wait_for(n, 35, || status(base) & STS_ACK == 0);
    unsafe { outb(base + 2, CTR_INIT | CTR_SELECT | CTR_AUTOFD) };
    let _ = wait_for(n, 35, || status(base) & STS_ACK != 0);
    unsafe { outb(base + 2, CTR_IDLE) };
}

/// One byte in nibble mode (low nibble first): HostBusy low, wait for
/// PtrClk (nAck) low, the nibble on nFault, Select, PError and Busy.
fn nibble_byte(n: usize, base: u16) -> Option<u8> {
    let mut v = 0u8;
    for half in 0..2 {
        unsafe { outb(base + 2, CTR_INIT | CTR_AUTOFD) };
        if !matches!(wait_for(n, 35, || status(base) & STS_ACK == 0), Waited::Ready) {
            return None;
        }
        let s = status(base);
        let nib = (s >> 3 & 1) | (s >> 4 & 1) << 1 | (s >> 5 & 1) << 2 | ((s & STS_BUSY == 0) as u8) << 3;
        v |= nib << (4 * half);
        unsafe { outb(base + 2, CTR_INIT) };
        if !matches!(wait_for(n, 35, || status(base) & STS_ACK != 0), Waited::Ready) {
            return None;
        }
    }
    Some(v)
}

/// The IEEE 1284 device ID: two bytes of length (big-endian, counting
/// themselves), then `MFG:...;MDL:...;`.
fn device_id(n: usize, f: &Info) -> Result<Vec<u8>, u32> {
    let base = f.base;
    if f.ecr != 0 {
        unsafe { outb(f.ecr, ECR_PS2 | ECR_NOINTR) };
    }
    // Device ID in nibble mode.
    negotiate(n, base, 0x04)?;
    let mut out = Vec::new();
    let r = (|| {
        let hi = nibble_byte(n, base).ok_or(BAD_COMMAND)?;
        let lo = nibble_byte(n, base).ok_or(BAD_COMMAND)?;
        let len = u16::from_be_bytes([hi, lo]).clamp(2, 4096) as usize;
        out.extend_from_slice(&[hi, lo]);
        while out.len() < len {
            // nFault high: no more data.
            if status(base) & STS_ERROR != 0 && out.len() > 2 {
                break;
            }
            out.push(nibble_byte(n, base).ok_or(BAD_COMMAND)?);
        }
        Ok(())
    })();
    terminate(n, base);
    r.map(|_| out)
}

/// Pulse INIT low (PRINT01's `initialprt`: about 50 us), then wait up to
/// a second for the printer to come back not busy.
fn init_printer(n: usize, f: &Info) {
    let base = f.base;
    unsafe {
        outb(base + 2, CTR_SELECT);
        for _ in 0..60 {
            let _ = inb(base + 1);
        }
        outb(base + 2, CTR_IDLE | if IRQ_ON[n].load(Ordering::Acquire) { CTR_INTEN } else { 0 });
    }
    let _ = wait_for(n, 1000, || status(base) & STS_BUSY != 0);
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
            PORTS[n].lock().frame = [80, 6];
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
            PORTS[n].lock().comm = input[0];
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
        // PRT_QUERDEVICEID: the IEEE 1284 device ID (nibble mode).
        0x74 => {
            let Some(info) = &hw else { return Err(BAD_COMMAND) };
            setup_irq(n, info);
            let _own = Owner::take(n);
            let mut id = device_id(n, info)?;
            id.truncate(out_max as usize);
            Ok(id)
        }
        // PRT_GETPRINTERSTATUS: PRINT01's status (ACK and ERROR made
        // active high, the low bits clear) with the timeout bit after a
        // write that timed out; without a port selected and not busy.
        0x66 => {
            let timed_out = PORTS[n].lock().timed_out as u8 * PRINTER_TIMEOUT;
            let st = match &hw {
                Some(i) => ibm_status(status(i.base)) & 0xfc,
                None => PRINTER_NOT_BUSY | PRINTER_SELECTED,
            };
            Ok(alloc::vec![st | timed_out])
        }
        // PRT_QUERYJOBHANDLE: only spooled handles have a job.
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
