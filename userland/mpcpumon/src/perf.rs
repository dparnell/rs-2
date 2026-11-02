//! The processors' load, read as OS/2's Pulse reads it: DosConfigurePerf
//! turns the kernel's performance blocks on, DosAliasPerfCtrs walks the
//! registered blocks (flags 1|2|8: the next block's text block, whose
//! handle names the block and whose name says "CPU") and maps each
//! processor's counter block (flags 1|0x10). The counter at 0x18 is the
//! time the processor was not idle (DosTmrQueryTime units); while the
//! processor is busy it holds the accumulated time minus the start of the
//! interval (negative), so the current timer value is added to it. The
//! load over a sample is the not-idle time it grew by over the time that
//! passed. See kernel/src/api/perf.rs.

use alloc::vec::Vec;

/// Samples kept per processor (the graph's width in samples).
pub const HISTORY: usize = 100;

const TM_NOT_IDLE: usize = 0x18;
const ERROR_ALREADY: u32 = 0;

pub struct Perf {
    /// Each processor's counter block.
    blocks: Vec<*const u8>,
    /// The last sample: the time and each processor's not-idle time.
    last_time: u64,
    last_busy: Vec<u64>,
    /// Processors online (DosGetProcessorStatus).
    pub online: Vec<bool>,
    /// Load history in percent, by processor, the newest last; the
    /// average of the processors online last of all.
    pub history: Vec<Vec<u8>>,
}

/// The timer now (DosTmrQueryTime).
pub fn now() -> u64 {
    let mut t = 0u64;
    unsafe { os2::DosTmrQueryTime(&mut t) };
    t
}

/// A timer counter's value at time `now`: a negative counter is running
/// (accumulated time minus its start).
pub fn timer_value(raw: u64, now: u64) -> u64 {
    if (raw as i64) < 0 { raw.wrapping_add(now) } else { raw }
}

/// The load in percent over an interval: the busy time it grew by over
/// the time that passed, 0..100.
pub fn load(busy0: u64, busy1: u64, t0: u64, t1: u64) -> u8 {
    let dt = t1.wrapping_sub(t0);
    if dt == 0 || busy1 < busy0 {
        return 0;
    }
    let db = busy1 - busy0;
    ((db.saturating_mul(100) + dt / 2) / dt).min(100) as u8
}

/// The average of the loads of the processors online.
pub fn average(loads: &[u8], online: &[bool]) -> u8 {
    let (mut sum, mut n) = (0u32, 0u32);
    for (i, &l) in loads.iter().enumerate() {
        if online.get(i).copied().unwrap_or(true) {
            sum += l as u32;
            n += 1;
        }
    }
    if n == 0 { 0 } else { (sum / n) as u8 }
}

fn read_u64(p: *const u8) -> u64 {
    unsafe { core::ptr::read_volatile(p as *const u64) }
}

impl Perf {
    /// Configure the counters and map every processor's block. None when
    /// the system has none.
    pub fn open() -> Option<Perf> {
        let dummy = [0u8; 16];
        unsafe {
            // As Pulse: turn them off first (whatever an earlier run left),
            // then on.
            os2::DosConfigurePerf(0, 4, 0, 0, dummy.as_ptr(), 0);
            if os2::DosConfigurePerf(0x28, 0, 0, 0, dummy.as_ptr(), 0) != ERROR_ALREADY {
                return None;
            }
        }
        let mut blocks = Vec::new();
        let mut handle = 0u32;
        loop {
            let (mut p, mut cb) = (core::ptr::null_mut::<u8>(), 0u32);
            if unsafe { os2::DosAliasPerfCtrs(0x0b, handle, &mut p, &mut cb) } != 0 || p.is_null() {
                break;
            }
            let text = unsafe { core::slice::from_raw_parts(p, (cb as usize).max(0x30)) };
            let id = u16::from_le_bytes([text[6], text[7]]) as u32;
            if id == 0 {
                break;
            }
            handle = id << 16;
            let flags = u32::from_le_bytes([text[8], text[9], text[10], text[11]]);
            let name_off = u32::from_le_bytes([text[0x10], text[0x11], text[0x12], text[0x13]]) as usize;
            let is_cpu = text.get(name_off..name_off + 3) == Some(b"CPU");
            if flags & 0x50 != 0x50 || !is_cpu {
                continue;
            }
            let (mut c, mut cbc) = (core::ptr::null_mut::<u8>(), 0u32);
            if unsafe { os2::DosAliasPerfCtrs(0x11, handle, &mut c, &mut cbc) } == 0 && !c.is_null() {
                blocks.push(c as *const u8);
            }
            if blocks.len() >= 64 {
                break;
            }
        }
        if blocks.is_empty() {
            Perf::close();
            return None;
        }
        let n = blocks.len();
        let mut p = Perf { blocks, last_time: 0, last_busy: alloc::vec![0; n], online: alloc::vec![true; n], history: alloc::vec![alloc::vec![0u8; HISTORY]; n + 1] };
        p.refresh_status();
        p.last_time = now();
        for i in 0..n {
            p.last_busy[i] = p.busy(i, p.last_time);
        }
        Some(p)
    }

    /// Turn the counters off again.
    pub fn close() {
        let dummy = [0u8; 16];
        unsafe { os2::DosConfigurePerf(0, 4, 0, 0, dummy.as_ptr(), 0) };
    }

    pub fn count(&self) -> usize {
        self.blocks.len()
    }

    /// Processor `i`'s not-idle time at `t`.
    pub fn busy(&self, i: usize, t: u64) -> u64 {
        timer_value(read_u64(unsafe { self.blocks[i].add(TM_NOT_IDLE) }), t)
    }

    /// Read whether each processor is online.
    pub fn refresh_status(&mut self) {
        for i in 0..self.blocks.len() {
            let mut s = 1u32;
            unsafe { os2::DosGetProcessorStatus(i as u32 + 1, &mut s) };
            self.online[i] = s != 0;
        }
    }

    /// Take a sample: each processor's load since the last one (0 for a
    /// processor offline) and their average go into the history.
    pub fn sample(&mut self) -> Vec<u8> {
        let t = now();
        let mut loads = Vec::with_capacity(self.blocks.len());
        for i in 0..self.blocks.len() {
            let b = self.busy(i, t);
            let l = if self.online[i] { load(self.last_busy[i], b, self.last_time, t) } else { 0 };
            self.last_busy[i] = b;
            loads.push(l);
        }
        self.last_time = t;
        let avg = average(&loads, &self.online);
        for (i, &l) in loads.iter().chain(core::iter::once(&avg)).enumerate() {
            let h = &mut self.history[i];
            h.remove(0);
            h.push(l);
        }
        loads.push(avg);
        loads
    }

    /// The newest load of processor `i` (the average at `count()`).
    pub fn current(&self, i: usize) -> u8 {
        *self.history[i].last().unwrap_or(&0)
    }
}
