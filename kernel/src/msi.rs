//! Message-signalled interrupts (PCI MSI and MSI-X).
//!
//! PCI INTx pins cannot be routed without the ACPI namespace (_PRT), so
//! devices interrupt through messages: a memory write of `data` to
//! `0xFEE0_0000 | destination APIC id << 12` that the LAPIC of that CPU
//! takes as an edge-triggered, fixed-delivery interrupt on vector `data`.
//! MSI (capability 05h) has one address/data pair for 1-32 vectors, MSI-X
//! (capability 11h) a table in a memory BAR with one entry per vector,
//! each with its own mask bit.
//!
//! Vectors [`FIRST`]..=[`LAST`] (except the API gate) are handed out to
//! drivers with a handler and a context word; the trap dispatcher calls
//! [`dispatch`], which acknowledges the LAPIC and runs the handler. A
//! handler runs in interrupt context on whatever CPU the message names:
//! no big kernel lock, only spin locks (try_lock where a thread may hold
//! the lock), short work, then it wakes the waiting thread ([`Event`]) or
//! posts to a kernel thread.
//!
//! Drivers program their devices while the PCI bus is probed, with
//! interrupts still off and the boot processor as destination, but the
//! messages stay off (MSI disabled, MSI-X function-masked: a message
//! before the LAPIC is set up could not be acknowledged); [`start`] runs
//! once the LAPICs and the other processors are up, spreads the vectors
//! over the online CPUs, turns the messages on and lets [`Event::wait`]
//! block instead of polling. Until then, and for devices without MSI or MSI-X, or with
//! `nomsi` on the kernel command line, the drivers poll as before.

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::drivers::pci;
use crate::sync::SpinLock;
use crate::task::WaitQueue;

/// Device vectors: above the ISA IRQs and exceptions, below the IPIs.
pub const FIRST: u8 = 0x30;
pub const LAST: u8 = 0xef;

pub const CAP_MSI: u8 = 0x05;
pub const CAP_MSIX: u8 = 0x11;

/// Handler (fn(usize) as usize, 0 = free) and its context, by vector.
static HANDLERS: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];
static CONTEXT: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];
/// Interrupts taken, by vector.
static COUNT: [AtomicU64; 256] = [const { AtomicU64::new(0) }; 256];
/// Where each vector's message is programmed (to retarget it).
static TARGETS: SpinLock<Vec<Target>> = SpinLock::new(Vec::new());
/// MSI-X functions (device, capability) to unmask in [`start`].
static FUNCTIONS: SpinLock<Vec<(pci::Device, u8)>> = SpinLock::new(Vec::new());
static NEXT: SpinLock<u8> = SpinLock::new(FIRST);
static LIVE: AtomicBool = AtomicBool::new(false);
/// Lost-interrupt safety net: completions found by a timed re-check.
static MISSED: AtomicU64 = AtomicU64::new(0);
/// Waits on armed events that slept, and that had to poll instead.
static BLOCKED: AtomicU64 = AtomicU64::new(0);
static POLLED: AtomicU64 = AtomicU64::new(0);
/// Destination of new vectors until [`start`] spreads them.
static BOOT_APIC: AtomicU32 = AtomicU32::new(u32::MAX);

#[derive(Clone, Copy)]
enum Slot {
    /// MSI capability at `cap` (one vector).
    Msi { dev: pci::Device, cap: u8 },
    /// MSI-X table entry (virtual address of its first dword).
    MsiX { entry: usize },
}

struct Target {
    vector: u8,
    slot: Slot,
    apic: u32,
}

unsafe impl Send for Target {}

/// Whether message-signalled interrupts may be used (not `nomsi`).
pub fn enabled() -> bool {
    !crate::boot_info().cmdline().split_whitespace().any(|w| w.eq_ignore_ascii_case("nomsi"))
}

/// Whether interrupts are delivered: waiting threads may block.
pub fn live() -> bool {
    LIVE.load(Ordering::Acquire)
}

fn boot_apic() -> u32 {
    let a = BOOT_APIC.load(Ordering::Relaxed);
    if a != u32::MAX {
        return a;
    }
    // The initial APIC id (CPUID 1), also before the LAPIC is mapped.
    let id = crate::arch::cpuid(1, 0).1 >> 24;
    BOOT_APIC.store(id, Ordering::Relaxed);
    id
}

/// Allocate a vector for `handler(ctx)`.
pub fn alloc(handler: fn(usize), ctx: usize) -> Option<u8> {
    if !enabled() {
        return None;
    }
    let mut next = NEXT.lock();
    while *next <= LAST {
        let v = *next;
        *next += 1;
        if v == crate::arch::idt::API_VECTOR {
            continue;
        }
        CONTEXT[v as usize].store(ctx, Ordering::Relaxed);
        HANDLERS[v as usize].store(handler as usize, Ordering::Release);
        return Some(v);
    }
    crate::println!("msi: out of interrupt vectors");
    None
}

/// The message for `vector` to the LAPIC `apic`: (address, data).
fn message(vector: u8, apic: u32) -> (u64, u32) {
    (0xfee0_0000 | (apic as u64 & 0xff) << 12, vector as u32)
}

/// Interrupt on a device vector (from the trap dispatcher).
pub fn dispatch(vector: u8) {
    if crate::apic::active() {
        crate::apic::eoi();
    }
    let h = HANDLERS[vector as usize].load(Ordering::Acquire);
    if h == 0 {
        return;
    }
    COUNT[vector as usize].fetch_add(1, Ordering::Relaxed);
    crate::api::perf::interrupt(crate::smp::this().index());
    let f: fn(usize) = unsafe { core::mem::transmute(h) };
    f(CONTEXT[vector as usize].load(Ordering::Relaxed));
}

/// Interrupts taken on `vector` so far.
pub fn count(vector: u8) -> u64 {
    COUNT[vector as usize].load(Ordering::Relaxed)
}

/// Device interrupts taken in all.
pub fn total() -> u64 {
    COUNT.iter().map(|c| c.load(Ordering::Relaxed)).sum()
}

/// "0x30 (CPU 0): 12, ...": interrupts taken per allocated vector.
pub fn summary() -> alloc::string::String {
    let ts = TARGETS.lock();
    let mut s = alloc::string::String::new();
    for t in ts.iter() {
        if !s.is_empty() {
            s.push_str(", ");
        }
        s.push_str(&alloc::format!("{:#x} (APIC {}): {}", t.vector, t.apic, count(t.vector)));
    }
    s
}

/// Completions that only a timed re-check found (lost interrupts).
pub fn missed() -> u64 {
    MISSED.load(Ordering::Relaxed)
}

/// Waits on interrupt-driven events so far: (slept, polled because the
/// caller could not sleep).
pub fn waits() -> (u64, u64) {
    (BLOCKED.load(Ordering::Relaxed), POLLED.load(Ordering::Relaxed))
}

fn count_wait(blocked: bool) {
    if live() {
        if blocked { &BLOCKED } else { &POLLED }.fetch_add(1, Ordering::Relaxed);
    }
}

// ---- MSI ----

/// Enable single-vector MSI on `dev` for `vector` (INTx is masked).
pub fn enable_msi(dev: &pci::Device, vector: u8) -> bool {
    let Some(cap) = dev.find_cap(CAP_MSI) else { return false };
    let apic = boot_apic();
    let ctl = dev.read16(cap + 2);
    // Disabled while programming; one message (MME 0).
    dev.write16(cap + 2, ctl & !(1 | 7 << 4));
    write_msi(dev, cap, vector, apic);
    if ctl & 1 << 8 != 0 {
        // Per-vector masking: unmask vector 0.
        let mask = if ctl & 1 << 7 != 0 { cap + 0x10 } else { cap + 0x0c };
        dev.write(mask, dev.read(mask) & !1);
    }
    dev.set_intx_disabled(true);
    if live() {
        dev.write16(cap + 2, (ctl & !(7 << 4)) | 1);
    }
    TARGETS.lock().push(Target { vector, slot: Slot::Msi { dev: *dev, cap }, apic });
    true
}

fn write_msi(dev: &pci::Device, cap: u8, vector: u8, apic: u32) {
    let (addr, data) = message(vector, apic);
    let ctl = dev.read16(cap + 2);
    dev.write(cap + 4, addr as u32);
    if ctl & 1 << 7 != 0 {
        dev.write(cap + 8, (addr >> 32) as u32);
        dev.write16(cap + 0x0c, data as u16);
    } else {
        dev.write16(cap + 8, data as u16);
    }
}

fn msi_mask(dev: &pci::Device, cap: u8, masked: bool) {
    let ctl = dev.read16(cap + 2);
    if ctl & 1 << 8 == 0 {
        return;
    }
    let off = if ctl & 1 << 7 != 0 { cap + 0x10 } else { cap + 0x0c };
    let m = dev.read(off);
    dev.write(off, if masked { m | 1 } else { m & !1 });
}

// ---- MSI-X ----

/// The MSI-X capability of a device and its mapped vector table.
pub struct MsiX {
    dev: pci::Device,
    cap: u8,
    table: *mut u32,
    size: u16,
}

unsafe impl Send for MsiX {}
unsafe impl Sync for MsiX {}

impl MsiX {
    /// Find and map the MSI-X table of `dev`. All entries start masked.
    pub fn find(dev: &pci::Device) -> Option<MsiX> {
        if !enabled() {
            return None;
        }
        let cap = dev.find_cap(CAP_MSIX)?;
        let ctl = dev.read16(cap + 2);
        let size = (ctl & 0x7ff) + 1;
        let t = dev.read(cap + 4);
        let bir = (t & 7) as u8;
        if bir > 5 || dev.read(0x10 + bir * 4) & 1 != 0 {
            return None;
        }
        let base = dev.bar(bir);
        if base == 0 {
            return None;
        }
        let table = crate::mm::mmio::map(base + (t & !7) as u64, size as u64 * 16) as *mut u32;
        let m = MsiX { dev: *dev, cap, table, size };
        for i in 0..size {
            m.mask(i, true);
        }
        Some(m)
    }

    /// Table entries (vectors the device can use).
    pub fn size(&self) -> u16 {
        self.size
    }

    fn entry(&self, i: u16) -> *mut u32 {
        unsafe { self.table.add(i as usize * 4) }
    }

    /// Point entry `i` at `vector` and unmask it.
    pub fn set(&self, i: u16, vector: u8) {
        if i >= self.size {
            return;
        }
        let apic = boot_apic();
        write_entry(self.entry(i), vector, apic);
        TARGETS.lock().push(Target { vector, slot: Slot::MsiX { entry: self.entry(i) as usize }, apic });
    }

    pub fn mask(&self, i: u16, masked: bool) {
        if i < self.size {
            entry_mask(self.entry(i), masked);
        }
    }

    /// Turn MSI-X on (and INTx off); the messages flow once [`start`]
    /// lifts the function mask.
    pub fn enable(&self) {
        let ctl = self.dev.read16(self.cap + 2);
        self.dev.set_intx_disabled(true);
        if live() {
            self.dev.write16(self.cap + 2, (ctl | 1 << 15) & !(1 << 14));
        } else {
            self.dev.write16(self.cap + 2, ctl | 1 << 15 | 1 << 14);
            FUNCTIONS.lock().push((self.dev, self.cap));
        }
    }
}

fn entry_mask(e: *mut u32, masked: bool) {
    unsafe {
        let c = read_volatile(e.add(3));
        write_volatile(e.add(3), if masked { c | 1 } else { c & !1 });
    }
}

/// Program an MSI-X entry (masked while it changes) and unmask it.
fn write_entry(e: *mut u32, vector: u8, apic: u32) {
    let (addr, data) = message(vector, apic);
    entry_mask(e, true);
    unsafe {
        write_volatile(e, addr as u32);
        write_volatile(e.add(1), (addr >> 32) as u32);
        write_volatile(e.add(2), data);
    }
    entry_mask(e, false);
}

/// One vector for `dev`: MSI-X entry 0 if `msix` and the device has it,
/// else MSI. Returns the vector and which kind it is.
pub fn setup(dev: &pci::Device, msix: bool, handler: fn(usize), ctx: usize) -> Option<(u8, &'static str)> {
    if !enabled() {
        return None;
    }
    if msix {
        if let Some(m) = MsiX::find(dev) {
            let v = alloc(handler, ctx)?;
            m.set(0, v);
            m.enable();
            return Some((v, "MSI-X"));
        }
    }
    dev.find_cap(CAP_MSI)?;
    let v = alloc(handler, ctx)?;
    enable_msi(dev, v).then_some((v, "MSI"))
}

/// Mask or unmask the message of `vector` (MSI only with per-vector
/// masking).
pub fn mask(vector: u8, masked: bool) {
    let ts = TARGETS.lock();
    for t in ts.iter().filter(|t| t.vector == vector) {
        match t.slot {
            Slot::Msi { dev, cap } => msi_mask(&dev, cap, masked),
            Slot::MsiX { entry } => entry_mask(entry as *mut u32, masked),
        }
    }
}

/// The LAPICs and the other processors are running: spread the vectors
/// over the online CPUs and let waiting threads block.
pub fn start() {
    if !crate::apic::active() {
        // No LAPIC: messages have nowhere to go; keep polling.
        return;
    }
    let cpus: Vec<u32> = crate::smp::online().map(|c| c.apic_id.load(Ordering::Relaxed)).collect();
    let mut ts = TARGETS.lock();
    for (n, t) in ts.iter_mut().enumerate() {
        let apic = cpus.get(n % cpus.len().max(1)).copied().unwrap_or(t.apic);
        t.apic = apic;
        match t.slot {
            Slot::Msi { dev, cap } => write_msi(&dev, cap, t.vector, apic),
            Slot::MsiX { entry } => write_entry(entry as *mut u32, t.vector, apic),
        }
    }
    // Messages on: MSI enable, MSI-X function mask off.
    for t in ts.iter() {
        if let Slot::Msi { dev, cap } = t.slot {
            let ctl = dev.read16(cap + 2);
            dev.write16(cap + 2, (ctl & !(7 << 4)) | 1);
        }
    }
    for (dev, cap) in FUNCTIONS.lock().drain(..) {
        let ctl = dev.read16(cap + 2);
        dev.write16(cap + 2, (ctl | 1 << 15) & !(1 << 14));
    }
    let n = ts.len();
    drop(ts);
    LIVE.store(true, Ordering::Release);
    if n > 0 {
        crate::println!("msi: {n} vector(s) over {} CPU(s)", cpus.len());
    }
}

/// The CPU (LAPIC id) `vector` is delivered to.
pub fn destination(vector: u8) -> Option<u32> {
    TARGETS.lock().iter().find(|t| t.vector == vector).map(|t| t.apic)
}

// ---- Waiting for interrupts ----

/// How often a waiting thread re-checks its condition by itself, in
/// ticks: the safety net against a lost interrupt.
const RECHECK_TICKS: u64 = 20;

/// Polls (each with a device register read) before a wait goes to sleep.
const SPIN_POLLS: u32 = 16;

/// Something an interrupt handler signals and threads wait for: a
/// counter of signals, a wait queue and an optional hook that runs in the
/// handler (for consumers that are not waiting threads).
pub struct Event {
    signals: AtomicU64,
    wait: WaitQueue,
    hook: AtomicUsize,
    /// A vector is assigned: waits may block.
    armed: AtomicBool,
}

impl Event {
    pub const fn new() -> Event {
        Event { signals: AtomicU64::new(0), wait: WaitQueue::new(), hook: AtomicUsize::new(0), armed: AtomicBool::new(false) }
    }

    /// A leaked event, for handler contexts.
    pub fn leak() -> &'static Event {
        alloc::boxed::Box::leak(alloc::boxed::Box::new(Event::new()))
    }

    /// Handler for [`alloc`]: the context is an `&'static Event`.
    pub fn handler(ctx: usize) {
        let e = unsafe { &*(ctx as *const Event) };
        e.signal();
    }

    /// Allocate a vector that signals this event.
    pub fn vector(&'static self) -> Option<u8> {
        let v = alloc(Event::handler, self as *const Event as usize)?;
        self.armed.store(true, Ordering::Release);
        Some(v)
    }

    /// Mark as interrupt-driven (the caller set up a vector signalling it).
    pub fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    pub fn armed(&self) -> bool {
        self.armed.load(Ordering::Acquire)
    }

    /// Run `f` in interrupt context on every signal.
    pub fn set_hook(&self, f: fn()) {
        self.hook.store(f as usize, Ordering::Release);
    }

    /// From the interrupt handler: count, run the hook, wake the waiters.
    pub fn signal(&self) {
        self.signals.fetch_add(1, Ordering::AcqRel);
        let h = self.hook.load(Ordering::Acquire);
        if h != 0 {
            let f: fn() = unsafe { core::mem::transmute(h) };
            f();
        }
        self.wait.wake_all();
    }

    pub fn signals(&self) -> u64 {
        self.signals.load(Ordering::Acquire)
    }

    /// Whether a wait right now would block (interrupts are delivered and
    /// the caller is a thread that may sleep). Interrupts being disabled
    /// does not prevent it: API calls run that way (the gate is an
    /// interrupt gate) and the switch restores the flag; interrupt
    /// handlers and spin lock holders may not sleep.
    pub fn can_block(&self) -> bool {
        self.armed()
            && live()
            && crate::smp::this().irq_depth.load(Ordering::Relaxed) == 0
            && crate::sync::held_spin_locks() == 0
            && crate::task::sched::current_opt().is_some_and(|t| !t.is_idle())
    }

    /// Wait up to `ms` milliseconds for `cond`. Interrupt-driven, the
    /// thread sleeps until a signal (re-checking every RECHECK_TICKS in
    /// case one is lost); otherwise `cond` is polled, yielding the CPU,
    /// with `pace` (an uncached device register read) between polls,
    /// which also estimates time while the timer is not running yet.
    /// `cond` runs with the wait queue locked: it may only read the
    /// device, never take a lock an interrupt handler holds.
    pub fn wait(&self, ms: u64, mut pace: impl FnMut(), mut cond: impl FnMut() -> bool) -> bool {
        if !self.can_block() {
            if self.armed() {
                count_wait(false);
            }
            return poll(ms, pace, cond);
        }
        // A fast device answers within a few register reads: catching
        // that saves the sleep and the wake-up (an interrupt, often an IPI).
        for _ in 0..SPIN_POLLS {
            if cond() {
                return true;
            }
            pace();
        }
        count_wait(true);
        let deadline = crate::irq::ticks() + (ms * crate::irq::HZ / 1000).max(1);
        loop {
            if cond() {
                return true;
            }
            let now = crate::irq::ticks();
            if now >= deadline {
                return cond();
            }
            let until = deadline.min(now + RECHECK_TICKS);
            let seen = self.signals();
            // `cond` may consume what it found (a FIFO read): a true result
            // seen while going to sleep must not be checked again.
            let mut met = false;
            let woke = self.wait.wait_uninterruptible_until(until, || {
                met = met || (self.signals() == seen && cond());
                met || self.signals() != seen
            });
            if met {
                return true;
            }
            if !woke && cond() {
                MISSED.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
    }

    /// Wait until the signal count differs from `seen` (or `ms` pass).
    /// For drivers whose completion check needs locks: snapshot
    /// [`Event::signals`], check, then wait for the next signal.
    pub fn wait_signal(&self, seen: u64, ms: u64) -> bool {
        if !self.can_block() {
            if self.armed() {
                count_wait(false);
            }
            crate::task::sched::yield_now();
            return true;
        }
        count_wait(true);
        let deadline = crate::irq::ticks() + (ms * crate::irq::HZ / 1000).max(1);
        self.wait.wait_uninterruptible_until(deadline, || self.signals() != seen)
    }
}

/// Poll `cond` for up to `ms` milliseconds, yielding the CPU after the
/// first hundred polls. Before the timer runs (during boot) time is
/// estimated from the polls, each of which includes `pace`.
pub fn poll(ms: u64, mut pace: impl FnMut(), mut cond: impl FnMut() -> bool) -> bool {
    let t0 = crate::irq::ticks();
    let limit = ms * crate::irq::HZ / 1000;
    let mut polls = 0u64;
    loop {
        if cond() {
            return true;
        }
        pace();
        polls += 1;
        let now = crate::irq::ticks();
        let expired = if now != t0 { now - t0 > limit } else { polls > ms * 1000 };
        if expired {
            return cond();
        }
        if polls > 100 {
            crate::task::sched::yield_now();
        }
        core::hint::spin_loop();
    }
}
