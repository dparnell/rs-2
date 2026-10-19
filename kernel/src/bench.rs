//! Built-in measurements (`diskbench` on the kernel command line): CPU
//! time of an idle system and of disk I/O, disk throughput and latency.
//!
//! Every CPU's timer interrupt counts whether it interrupted the idle
//! thread or something else ([`account`]); the boot processor also
//! sums the TSC cycles its periodic tick work takes. The benchmark runs
//! on a kernel thread once the system has settled: an idle phase, then
//! for every disk a sequential read and random 4 KB reads (read only).

use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::smp::MAX_CPUS;

static BUSY: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static TICKS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static TICK_CYCLES: AtomicU64 = AtomicU64::new(0);

/// Timer interrupt on this CPU: account the tick to busy or idle.
pub fn account() {
    let c = crate::smp::this();
    let i = c.index();
    TICKS[i].fetch_add(1, Ordering::Relaxed);
    // Interrupts are off: this CPU's current thread is stable.
    if c.current().as_ref().is_some_and(|t| !t.is_idle()) {
        BUSY[i].fetch_add(1, Ordering::Relaxed);
    }
}

/// Cycles the boot processor spent in its periodic tick work.
pub fn add_tick_cycles(c: u64) {
    TICK_CYCLES.fetch_add(c, Ordering::Relaxed);
}

/// (busy ticks summed over the CPUs, ticks of the boot CPU, tick cycles).
fn sample() -> (u64, u64, u64) {
    let busy = BUSY.iter().map(|b| b.load(Ordering::Relaxed)).sum();
    (busy, TICKS[0].load(Ordering::Relaxed), TICK_CYCLES.load(Ordering::Relaxed))
}

/// Start the benchmark thread if the command line asks for it.
pub fn start(cmdline: &str) {
    if !cmdline.split_whitespace().any(|w| w.eq_ignore_ascii_case("diskbench")) {
        return;
    }
    crate::task::kthread::spawn("diskbench", run);
}

fn sleep_ms(ms: u64) {
    crate::task::sched::sleep_ms(ms);
}

fn run() {
    // Let the shell come up.
    sleep_ms(8000);
    let (b0, t0, c0) = sample();
    let tsc0 = crate::arch::rdtsc();
    sleep_ms(4000);
    let (b1, t1, c1) = sample();
    let tsc1 = crate::arch::rdtsc();
    let ticks = (t1 - t0).max(1);
    let tsc_per_us = ((tsc1 - tsc0) / (ticks * 1000 / crate::irq::HZ).max(1) / 1000).max(1);
    crate::println!(
        "bench: idle: busy {}/{} ticks ({}.{}% of one CPU), tick work {} cycles/tick ({} ns)",
        b1 - b0,
        ticks,
        (b1 - b0) * 100 / ticks,
        (b1 - b0) * 1000 / ticks % 10,
        (c1 - c0) / ticks,
        (c1 - c0) / ticks * 1000 / tsc_per_us
    );
    let disks = crate::drivers::DISKS.lock().clone();
    for (n, d) in disks.iter().enumerate() {
        let ss = d.sector_size();
        if d.sector_count() == 0 || ss == 0 {
            continue;
        }
        // Sequential: 16 MB (or the disk) four times, in 64 KB requests.
        let chunk = 64 * 1024 / ss;
        let total = (16 * 1024 * 1024 / ss as u64).min(d.sector_count() / chunk as u64 * chunk as u64);
        let mut buf = vec![0u8; chunk * ss];
        let (i0, m0) = (crate::msi::total(), crate::msi::missed());
        let (b0, t0, _) = sample();
        let s0 = crate::arch::rdtsc();
        let mut lba = 0;
        let mut ok = true;
        // Four passes, for enough timer ticks to account.
        while ok && lba < 4 * total {
            if d.read(lba % total, &mut buf).is_err() {
                ok = false;
            }
            lba += chunk as u64;
        }
        let us = ((crate::arch::rdtsc() - s0) / tsc_per_us).max(1);
        let (b1, t1, _) = sample();
        let bytes = lba * ss as u64;
        crate::println!(
            "bench: disk {n}: sequential {} KB in {} ms = {} KB/s, CPU busy {}%{}",
            bytes / 1024,
            us / 1000,
            bytes * 1_000_000 / 1024 / us,
            (b1 - b0) * 100 / (t1 - t0).max(1),
            if ok { "" } else { " (read error)" }
        );
        // Random: 4 KB reads spread over the disk.
        let per = (4096 / ss).max(1);
        let mut small = vec![0u8; per * ss];
        let span = d.sector_count().saturating_sub(per as u64).max(1);
        let mut seed = 0x1234_5678u64;
        let count = 500u64;
        let (b0, t0, _) = sample();
        let c0 = crate::arch::rdtsc();
        for _ in 0..count {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let at = (seed >> 33) % span;
            if d.read(at, &mut small).is_err() {
                break;
            }
        }
        let us = (crate::arch::rdtsc() - c0) / tsc_per_us;
        let (b1, t1, _) = sample();
        crate::println!(
            "bench: disk {n}: random 4 KB reads: {} us each, CPU busy {}%; {} device interrupts, {} missed",
            us / count,
            (b1 - b0) * 100 / (t1 - t0).max(1),
            crate::msi::total() - i0,
            crate::msi::missed() - m0
        );
    }
    let per: alloc::vec::Vec<(u64, u64)> = (0..crate::smp::online_count() as usize)
        .map(|i| (BUSY[i].load(Ordering::Relaxed), TICKS[i].load(Ordering::Relaxed)))
        .collect();
    crate::println!("bench: per CPU busy/ticks since boot {per:?}");
    crate::println!("bench: done");
    // Interrupts per device vector, now and after a while (input typed
    // meanwhile, network traffic).
    for _ in 0..2 {
        crate::println!("bench: interrupts by vector: {}", crate::msi::summary());
        sleep_ms(30_000);
    }
}
