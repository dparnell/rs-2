//! Clocks and sleeping. CLOCK_MONOTONIC is the high-resolution timer
//! (DosTmrQueryTime); CLOCK_REALTIME is DosGetDateTime's local time taken
//! as UTC when OS/2 knows no time zone, with the monotonic clock's
//! sub-second part so that it advances smoothly.

use core::ffi::c_int;

use crate::errno::fail;
use crate::sys::*;

static mut FREQ: u32 = 0;

/// Nanoseconds of the monotonic clock.
pub fn monotonic_ns() -> u64 {
    unsafe {
        if FREQ == 0 {
            let mut f = 0;
            DosTmrQueryFreq(&mut f);
            FREQ = if f == 0 { 1_193_182 } else { f };
        }
        let mut t = 0u64;
        DosTmrQueryTime(&mut t);
        let f = FREQ as u64;
        (t / f) * 1_000_000_000 + (t % f) * 1_000_000_000 / f
    }
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// (seconds, nanoseconds) since 1970 UTC.
pub fn realtime() -> (i64, u32) {
    let mut dt = DateTime::default();
    unsafe { DosGetDateTime(&mut dt) };
    let days = days_from_civil(dt.year as i64, dt.month as u32, dt.day as u32);
    let mut secs = days * 86400 + dt.hours as i64 * 3600 + dt.minutes as i64 * 60 + dt.seconds as i64;
    if dt.timezone != -1 {
        // Minutes west of UTC.
        secs += dt.timezone as i64 * 60;
    }
    (secs, dt.hundredths as u32 * 10_000_000)
}

fn now(clk: c_int) -> Option<(i64, u32)> {
    match clk {
        libc::CLOCK_REALTIME | libc::CLOCK_REALTIME_COARSE => Some(realtime()),
        libc::CLOCK_MONOTONIC | libc::CLOCK_MONOTONIC_RAW | libc::CLOCK_MONOTONIC_COARSE | libc::CLOCK_BOOTTIME
        | libc::CLOCK_PROCESS_CPUTIME_ID | libc::CLOCK_THREAD_CPUTIME_ID => {
            let ns = monotonic_ns();
            Some(((ns / 1_000_000_000) as i64, (ns % 1_000_000_000) as u32))
        }
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_gettime(clk: c_int, ts: *mut libc::timespec) -> c_int {
    let Some((s, ns)) = now(clk) else { return fail(libc::EINVAL) };
    unsafe {
        (*ts).tv_sec = s as _;
        (*ts).tv_nsec = ns as _;
    }
    0
}

/// glibc's 64-bit time_t variant (struct __timespec64: i64 seconds, i32
/// nanoseconds, padding).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __clock_gettime64(clk: c_int, ts: *mut [u32; 4]) -> c_int {
    let Some((s, ns)) = now(clk) else { return fail(libc::EINVAL) };
    unsafe { *ts = [s as u32, (s >> 32) as u32, ns, 0] };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_getres(_clk: c_int, ts: *mut libc::timespec) -> c_int {
    if !ts.is_null() {
        unsafe {
            (*ts).tv_sec = 0;
            (*ts).tv_nsec = 1000;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gettimeofday(tv: *mut libc::timeval, _tz: *mut core::ffi::c_void) -> c_int {
    let (s, ns) = realtime();
    if !tv.is_null() {
        unsafe {
            (*tv).tv_sec = s as _;
            (*tv).tv_usec = (ns / 1000) as _;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn time(t: *mut libc::time_t) -> libc::time_t {
    let s = realtime().0 as libc::time_t;
    if !t.is_null() {
        unsafe { *t = s };
    }
    s
}

fn sleep_ns(ns: u64) {
    let ms = ns.div_ceil(1_000_000);
    let end = monotonic_ns() + ns;
    unsafe { DosSleep(ms.min(u32::MAX as u64) as u32) };
    // DosSleep may wake a little early on a coarse timer.
    while monotonic_ns() < end {
        unsafe { DosSleep(1) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nanosleep(req: *const libc::timespec, rem: *mut libc::timespec) -> c_int {
    let (s, ns) = unsafe { ((*req).tv_sec as i64, (*req).tv_nsec as i64) };
    if s < 0 || !(0..1_000_000_000).contains(&ns) {
        return fail(libc::EINVAL);
    }
    sleep_ns(s as u64 * 1_000_000_000 + ns as u64);
    if !rem.is_null() {
        unsafe {
            (*rem).tv_sec = 0;
            (*rem).tv_nsec = 0;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_nanosleep(clk: c_int, flags: c_int, req: *const libc::timespec, rem: *mut libc::timespec) -> c_int {
    let (s, ns) = unsafe { ((*req).tv_sec as i64, (*req).tv_nsec as i64) };
    let mut total = s * 1_000_000_000 + ns;
    if flags & libc::TIMER_ABSTIME != 0 {
        let Some((cs, cns)) = now(clk) else { return libc::EINVAL };
        total -= cs * 1_000_000_000 + cns as i64;
    }
    if total > 0 {
        sleep_ns(total as u64);
    }
    if !rem.is_null() {
        unsafe {
            (*rem).tv_sec = 0;
            (*rem).tv_nsec = 0;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn usleep(us: u32) -> c_int {
    sleep_ns(us as u64 * 1000);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sleep(s: u32) -> u32 {
    sleep_ns(s as u64 * 1_000_000_000);
    0
}
