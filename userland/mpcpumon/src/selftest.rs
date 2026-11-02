//! `MPCPUMON /SELFTEST`: the load arithmetic, the counters (they are
//! there for every processor and the busy time grows while this thread
//! works) and the graph's geometry. PASS/FAIL lines on the serial port.

use alloc::vec::Vec;

use pmkit::RectL;

use crate::perf::{self, Perf, HISTORY};
use crate::{bar_span, hit, trace, F_BAR, F_LINE};

struct T {
    fails: u32,
}

impl T {
    fn check(&mut self, ok: bool, what: &str) {
        let mut s = Vec::new();
        s.extend_from_slice(if ok { b"PASS " } else { b"FAIL " });
        s.extend_from_slice(what.as_bytes());
        trace(&s);
        if !ok {
            self.fails += 1;
        }
    }
}

pub fn run() -> u32 {
    let mut t = T { fails: 0 };
    // Arithmetic.
    t.check(perf::load(0, 50, 1000, 1100) == 50, "load: half busy is 50%");
    t.check(perf::load(0, 500, 0, 100) == 100, "load: clipped at 100%");
    t.check(perf::load(10, 5, 0, 100) == 0, "load: a counter going back counts as idle");
    t.check(perf::load(0, 0, 7, 7) == 0, "load: no time passed");
    t.check(perf::timer_value((-300i64) as u64, 1000) == 700, "a running timer counter adds the time now");
    t.check(perf::timer_value(1234, 99999) == 1234, "a stopped timer counter is its value");
    t.check(perf::average(&[100, 0, 50], &[true, true, false]) == 50, "the average counts processors online only");
    t.check(perf::average(&[10], &[false]) == 0, "no processor online averages 0");
    // Geometry.
    let g = RectL::new(40, 30, 440, 230);
    let (l0, r0) = bar_span(&g, 2, 0);
    let (l1, _) = bar_span(&g, 2, 1);
    let (la, ra) = bar_span(&g, 2, 2);
    t.check(l0 > g.l && r0 < l1 && ra <= g.r && la > l1, "bars stay apart and inside the graph");
    let hist = alloc::vec![alloc::vec![100u8; HISTORY], alloc::vec![0u8; HISTORY], alloc::vec![50u8; HISTORY]];
    t.check(hit(&g, &hist, F_BAR, (l0 + r0) / 2, g.b + 20) == Some(0), "a click on a bar finds its processor");
    t.check(hit(&g, &hist, F_LINE, 200, g.t - 1) == Some(0), "a click on a line finds its processor");
    t.check(hit(&g, &hist, F_LINE, 200, g.b + 100) .is_none(), "a click away from the lines finds none");
    // The counters.
    match Perf::open() {
        None => t.check(false, "the performance counters open"),
        Some(mut p) => {
            let mut n = [0u32; 1];
            unsafe { os2::DosQuerySysInfo(26, 26, n.as_mut_ptr(), 4) };
            t.check(p.count() == n[0] as usize, &alloc::format!("a counter block per processor ({} of {})", p.count(), n[0]));
            t.check(p.online.iter().all(|&o| o), "every processor is online");
            let t0 = perf::now();
            let b0: Vec<u64> = (0..p.count()).map(|i| p.busy(i, t0)).collect();
            // Work for a while (about 200 ms of timer).
            let mut x = 1u32;
            let freq = {
                let mut f = 0u32;
                unsafe { os2::DosTmrQueryFreq(&mut f) };
                f.max(1) as u64
            };
            while perf::now().wrapping_sub(t0) < freq / 5 {
                x = core::hint::black_box(x.wrapping_mul(1664525).wrapping_add(1013904223));
            }
            let t1 = perf::now();
            let b1: Vec<u64> = (0..p.count()).map(|i| p.busy(i, t1)).collect();
            let grew: u64 = b0.iter().zip(b1.iter()).map(|(a, b)| b.saturating_sub(*a)).sum();
            t.check(grew > 0, "the busy time grows while a thread works");
            let loads = p.sample();
            let max = *loads[..p.count()].iter().max().unwrap_or(&0);
            t.check(loads.iter().all(|&l| l <= 100), "loads are percentages");
            t.check(max >= 50, &alloc::format!("the working processor is busy ({max}%)"));
            t.check(p.history.len() == p.count() + 1 && p.history.iter().all(|h| h.len() == HISTORY), "a history per processor and the average");
            t.check(p.current(p.count()) == perf::average(&loads[..p.count()], &p.online), "the newest average is the processors' average");
            Perf::close();
        }
    }
    trace(alloc::format!("mpcpumon selftest done: {} failure(s)", t.fails).as_bytes());
    t.fails
}
