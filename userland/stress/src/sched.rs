//! Scheduling: strict priority by class and level, the MAXWAIT starvation
//! boost, and preemption by a higher class when every CPU is busy.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use os2::*;

use crate::api::DosSetPriority;
use crate::{check, join, ms, thread};

const PRTYS_THREAD: u32 = 2;
const PRTYC_NOCHANGE: u32 = 0;
const PRTYC_REGULAR: u32 = 2;
const PRTYC_TIMECRITICAL: u32 = 3;

static STOP: AtomicBool = AtomicBool::new(false);
static COUNTS: [AtomicU32; 17] = [const { AtomicU32::new(0) }; 17];

fn set_priority(class: u32, delta: i32) {
    unsafe { DosSetPriority(PRTYS_THREAD, class, delta, 0) };
}

/// Spin, counting, at regular class level `param >> 8`, slot `param & 0xff`.
unsafe extern "C" fn spinner(param: u32) {
    let (level, slot) = ((param >> 8) as i32, (param & 0xff) as usize);
    // Start from level 0, then raise.
    set_priority(PRTYC_REGULAR, -31);
    set_priority(PRTYC_NOCHANGE, level);
    while !STOP.load(Ordering::Relaxed) {
        COUNTS[slot].fetch_add(1, Ordering::Relaxed);
    }
}

fn cpus() -> u32 {
    let mut v = 1u32;
    unsafe { DosQuerySysInfo(26, 26, &mut v, 4) };
    v.clamp(1, 16)
}

/// Run one spinner per CPU at regular level 20 plus one at level 0 for
/// `window` ms; returns (low count, average high count).
fn contest(window: u32) -> (u32, u32) {
    let n = cpus();
    STOP.store(false, Ordering::Relaxed);
    for c in COUNTS.iter() {
        c.store(0, Ordering::Relaxed);
    }
    let mut tids = alloc::vec::Vec::new();
    for i in 0..n {
        tids.push(thread(spinner, 20 << 8 | (i + 1)));
    }
    tids.push(thread(spinner, 0));
    unsafe { DosSleep(200) };
    let base: alloc::vec::Vec<u32> = COUNTS.iter().map(|c| c.load(Ordering::Relaxed)).collect();
    unsafe { DosSleep(window) };
    let now: alloc::vec::Vec<u32> = COUNTS.iter().map(|c| c.load(Ordering::Relaxed)).collect();
    STOP.store(true, Ordering::Relaxed);
    for t in tids {
        join(t);
    }
    let low = now[0].wrapping_sub(base[0]);
    let high = (1..=n as usize).map(|i| now[i].wrapping_sub(base[i]) as u64).sum::<u64>() / n as u64;
    (low, high as u32)
}

pub fn run() {
    // The test itself must get the CPU while the spinners run.
    set_priority(PRTYC_TIMECRITICAL, 0);
    let n = cpus();
    crate::println!("  {n} processor(s)");

    // Within one class the higher level wins (shorter than MAXWAIT).
    let (low, high) = contest(1000);
    crate::println!("  level 0: {low}, level 20 (average): {high}");
    check("higher level runs, lower level waits", high > 0 && (low as u64) * 20 < high as u64);

    // MAXWAIT (3 s): the starved level-0 thread gets a turn.
    let (low, high) = contest(4500);
    crate::println!("  over 4.5 s - level 0: {low}, level 20 (average): {high}");
    check("starved thread boosted after MAXWAIT", low > 0 && high > 0);

    // A time-critical thread preempts busy regular threads: its sleeps end
    // on time although every CPU is spinning.
    STOP.store(false, Ordering::Relaxed);
    let tids: alloc::vec::Vec<u32> = (0..n).map(|i| thread(spinner, 31 << 8 | (i + 1))).collect();
    unsafe { DosSleep(100) };
    let t0 = ms();
    for _ in 0..50 {
        unsafe { DosSleep(10) };
    }
    let took = ms() - t0;
    STOP.store(true, Ordering::Relaxed);
    for t in tids {
        join(t);
    }
    crate::println!("  50 x DosSleep(10) under load: {took} ms");
    check("time-critical thread preempts regular ones", took < 50 * 20);
    set_priority(PRTYC_REGULAR, -31);
}
