//! Where the time of a workload goes (`CACHE -STATS:RESET`, then
//! `CACHE -STATS:SHOW` writes a report to the log): every volume's block
//! requests, counted where the file systems reach their devices
//! ([`crate::api::harderr`]), time in the API calls and in page faults.
//! Nothing is counted until the first reset.
//!
//! Two bitmaps of sector numbers tell what a cache could save: a sector
//! read again, or written again, would not have reached the disk. They
//! are by LBA within a volume, so volumes share them: an estimate.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicU8, Ordering};

use crate::api::names;

static ON: AtomicBool = AtomicBool::new(false);
static START: AtomicU64 = AtomicU64::new(0);

const MAX_ORDINAL: usize = 1024;
/// Sectors the bitmaps cover (8 GB of 512-byte sectors).
const MAP_SECTORS: usize = 1 << 24;

#[derive(Default)]
struct Op {
    n: AtomicU64,
    sectors: AtomicU64,
    cycles: AtomicU64,
    again: AtomicU64,
}

impl Op {
    const fn new() -> Op {
        Op { n: AtomicU64::new(0), sectors: AtomicU64::new(0), cycles: AtomicU64::new(0), again: AtomicU64::new(0) }
    }
    fn clear(&self) {
        for c in [&self.n, &self.sectors, &self.cycles, &self.again] {
            c.store(0, Ordering::Relaxed);
        }
    }
}

static READ: Op = Op::new();
static WRITE: Op = Op::new();
static FLUSH: Op = Op::new();
/// What got past the block cache.
static DEV_READ: Op = Op::new();
static DEV_WRITE: Op = Op::new();
static FAULT: Op = Op::new();
static API: [Op; MAX_ORDINAL] = [const { Op::new() }; MAX_ORDINAL];
static OTHER_API: Op = Op::new();

/// Bit 0: seen by a read or a write; bit 1: written.
static MAP: AtomicPtr<AtomicU8> = AtomicPtr::new(core::ptr::null_mut());

fn map() -> Option<&'static [AtomicU8]> {
    let p = MAP.load(Ordering::Acquire);
    (!p.is_null()).then(|| unsafe { core::slice::from_raw_parts(p, MAP_SECTORS) })
}

/// The start of a timed operation, if counting.
#[inline]
pub fn begin() -> Option<u64> {
    ON.load(Ordering::Relaxed).then(crate::arch::rdtsc)
}

fn add(op: &Op, t0: u64, sectors: u64) {
    op.n.fetch_add(1, Ordering::Relaxed);
    op.sectors.fetch_add(sectors, Ordering::Relaxed);
    op.cycles.fetch_add(crate::arch::rdtsc().wrapping_sub(t0), Ordering::Relaxed);
}

fn mark(op: &Op, lba: u64, sectors: u64, bits: u8, test: u8) {
    let Some(m) = map() else { return };
    let mut again = 0;
    for s in lba..lba + sectors {
        if let Some(b) = m.get(s as usize) {
            if b.fetch_or(bits, Ordering::Relaxed) & test != 0 {
                again += 1;
            }
        }
    }
    op.again.fetch_add(again, Ordering::Relaxed);
}

pub fn read(t0: Option<u64>, lba: u64, sectors: u64) {
    if let Some(t) = t0 {
        add(&READ, t, sectors);
        mark(&READ, lba, sectors, 1, 1);
    }
}

pub fn write(t0: Option<u64>, lba: u64, sectors: u64) {
    if let Some(t) = t0 {
        add(&WRITE, t, sectors);
        mark(&WRITE, lba, sectors, 3, 2);
    }
}

pub fn dev_read(t0: Option<u64>, sectors: u64) {
    if let Some(t) = t0 {
        add(&DEV_READ, t, sectors);
    }
}

pub fn dev_write(t0: Option<u64>, sectors: u64) {
    if let Some(t) = t0 {
        add(&DEV_WRITE, t, sectors);
    }
}

pub fn flush(t0: Option<u64>) {
    if let Some(t) = t0 {
        add(&FLUSH, t, 0);
    }
}

pub fn fault(t0: Option<u64>) {
    if let Some(t) = t0 {
        add(&FAULT, t, 0);
    }
}

pub fn api(t0: Option<u64>, module: u8, ordinal: u16) {
    if let Some(t) = t0 {
        let op = if module == 0 { API.get(ordinal as usize).unwrap_or(&OTHER_API) } else { &OTHER_API };
        add(op, t, 0);
    }
}

/// Start counting from zero.
pub fn reset() {
    ON.store(false, Ordering::Relaxed);
    for op in [&READ, &WRITE, &DEV_READ, &DEV_WRITE, &FLUSH, &FAULT, &OTHER_API].into_iter().chain(API.iter()) {
        op.clear();
    }
    match map() {
        Some(m) => m.iter().for_each(|b| b.store(0, Ordering::Relaxed)),
        None => {
            let v: Vec<AtomicU8> = (0..MAP_SECTORS).map(|_| AtomicU8::new(0)).collect();
            MAP.store(v.leak().as_mut_ptr(), Ordering::Release);
        }
    }
    START.store(crate::arch::rdtsc(), Ordering::Relaxed);
    ON.store(true, Ordering::Relaxed);
}

/// Report what was counted since the last reset.
pub fn show() {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    let per_ms = crate::rtc::tsc_per_ms().max(1);
    let ms = |c: u64| c / per_ms;
    let wall = ms(crate::arch::rdtsc().wrapping_sub(START.load(Ordering::Relaxed))).max(1);
    let get = |c: &AtomicU64| c.load(Ordering::Relaxed);
    let share = |c: u64| ms(c) * 1000 / wall;
    crate::println!("iostat: {} ms", wall);
    for (name, op) in [("read", &READ), ("write", &WRITE)] {
        let (n, s, c, a) = (get(&op.n), get(&op.sectors), get(&op.cycles), get(&op.again));
        crate::println!(
            "iostat: {}: {} requests, {} sectors ({} again), {} ms ({}.{}%), {} us each",
            name, n, s, a, ms(c), share(c) / 10, share(c) % 10, c * 1000 / per_ms / n.max(1)
        );
    }
    for (name, op) in [("device read", &DEV_READ), ("device write", &DEV_WRITE)] {
        let (n, s, c) = (get(&op.n), get(&op.sectors), get(&op.cycles));
        crate::println!("iostat: {}: {} requests, {} sectors, {} ms ({}.{}%)", name, n, s, ms(c), share(c) / 10, share(c) % 10);
    }
    let (n, c) = (get(&FLUSH.n), get(&FLUSH.cycles));
    crate::println!("iostat: flush: {} requests, {} ms ({}.{}%)", n, ms(c), share(c) / 10, share(c) % 10);
    let (n, c) = (get(&FAULT.n), get(&FAULT.cycles));
    crate::println!("iostat: exceptions: {} from user code, {} ms ({}.{}%)", n, ms(c), share(c) / 10, share(c) % 10);
    let mut top: Vec<(u64, u64, usize)> = API.iter().enumerate().map(|(i, o)| (get(&o.cycles), get(&o.n), i)).filter(|e| e.1 != 0).collect();
    top.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    for (c, n, i) in top.into_iter().take(24) {
        crate::println!("iostat: {:>8} ms {:>8} calls {}", ms(c), n, names::display(0, i as u16));
    }
    crate::println!("iostat: {:>8} ms {:>8} calls other modules", ms(get(&OTHER_API.cycles)), get(&OTHER_API.n));
}
