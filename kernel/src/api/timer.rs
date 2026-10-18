//! Timers: DosAsyncTimer/DosStartTimer/DosStopTimer, the 16-bit
//! DosTimerAsync/Start/Stop, and the high-resolution DosTmrQuery*.

use alloc::vec::Vec;

use super::{err, Ctx};
use crate::irq;
use crate::sync::SpinLock;

#[derive(Clone, Copy)]
enum Action {
    /// Post a 32-bit event semaphore.
    PostEvent(u32),
    /// Clear a 16-bit RAM semaphore at a physical address.
    ClearRam(u64),
    /// Clear a 16-bit system semaphore handle.
    ClearSys(u32),
}

struct Timer {
    handle: u32,
    pid: u32,
    deadline: u64,
    /// Period in ticks for repeating timers.
    period: Option<u64>,
    action: Action,
}

static TIMERS: SpinLock<Vec<Timer>> = SpinLock::new(Vec::new());
static NEXT: SpinLock<u32> = SpinLock::new(1);

fn ticks_for(ms: u32) -> u64 {
    // OS/2 rounds up to its timer interval; at least one tick.
    ((ms as u64) * irq::HZ / 1000).max(1)
}

fn add(pid: u32, ms: u32, repeat: bool, action: Action) -> u32 {
    let h = {
        let mut n = NEXT.lock();
        let h = *n;
        *n += 1;
        h
    };
    let t = ticks_for(ms);
    TIMERS.lock().push(Timer { handle: h, pid, deadline: irq::ticks() + t, period: repeat.then_some(t), action });
    h
}

/// Called from the timer interrupt.
pub fn tick() {
    let now = irq::ticks();
    let mut fire = Vec::new();
    {
        let mut ts = TIMERS.lock();
        if ts.is_empty() {
            return;
        }
        ts.retain_mut(|t| {
            if t.deadline > now {
                return true;
            }
            fire.push(t.action);
            match t.period {
                Some(p) => {
                    t.deadline += p;
                    true
                }
                None => false,
            }
        });
    }
    for a in fire {
        match a {
            Action::PostEvent(h) => super::sem::post_event_handle(h),
            Action::ClearRam(phys) => {
                unsafe { (crate::mm::phys::phys_to_virt(phys) as *mut u32).write_volatile(0) };
                super::sem::wake_16bit();
            }
            Action::ClearSys(h) => super::sem::clear_sys16(h),
        }
    }
}

pub fn process_ended(pid: u32) {
    TIMERS.lock().retain(|t| t.pid != pid);
}

fn start32(c: &mut Ctx, repeat: bool) -> u32 {
    let (ms, hsem, ph) = (c.arg(0), c.arg(1), c.arg(2));
    if !super::sem::is_event_handle(c, hsem) {
        return err::INVALID_HANDLE;
    }
    let h = add(c.process.pid, ms, repeat, Action::PostEvent(hsem));
    c.write_u32(ph, h);
    0
}

pub fn dos_async_timer(c: &mut Ctx) -> u32 {
    start32(c, false)
}

pub fn dos_start_timer(c: &mut Ctx) -> u32 {
    start32(c, true)
}

pub fn dos_stop_timer(c: &mut Ctx) -> u32 {
    let h = c.arg(0);
    let pid = c.process.pid;
    let mut ts = TIMERS.lock();
    let n = ts.len();
    ts.retain(|t| !(t.handle == h && t.pid == pid));
    if ts.len() == n { 326 } else { 0 } // ERROR_TS_HANDLE
}

fn start16(c: &mut Ctx, repeat: bool) -> u32 {
    let (ms, hsem, ph) = (c.arg(0), c.arg(1), c.ptr(2));
    let action = if hsem >> 16 == 0x0008 {
        Action::ClearSys(hsem)
    } else {
        let lin = c.far16_to_flat(hsem);
        let phys = {
            let mut sp = c.space();
            if !sp.check(lin, 4, true) {
                return err::INVALID_HANDLE;
            }
            crate::mm::paging::translate(sp.root, lin as u64)
        };
        match phys {
            Some(p) => Action::ClearRam(p),
            None => return err::INVALID_HANDLE,
        }
    };
    let h = add(c.process.pid, ms, repeat, action);
    c.write_u16(ph, h as u16);
    0
}

pub fn dos16_timer_async(c: &mut Ctx) -> u32 {
    start16(c, false)
}

pub fn dos16_timer_start(c: &mut Ctx) -> u32 {
    start16(c, true)
}

/// Frequency of DosTmrQueryTime (the 8254 input clock, as OS/2 reports).
pub const TMR_FREQ: u64 = 1_193_167;

pub fn dos_tmr_query_freq(c: &mut Ctx) -> u32 {
    c.write_u32(c.arg(0), TMR_FREQ as u32);
    0
}

pub fn dos_tmr_query_time(c: &mut Ctx) -> u32 {
    let ms = irq::uptime_ms();
    let v = ms * TMR_FREQ / 1000;
    let p = c.arg(0);
    c.write_u32(p, v as u32);
    c.write_u32(p + 4, (v >> 32) as u32);
    0
}
