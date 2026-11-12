//! CMOS real-time clock and the system clock.
//!
//! The system clock is the CMOS clock read once, then the uptime counter
//! (milliseconds) added to it, so that the time is monotonic and has
//! milliseconds (the CMOS clock has only seconds). `BASE` is the clock's
//! time (milliseconds since 1970) at uptime 0. Until the first second edge
//! of the CMOS clock is seen (the timer interrupt watches for it in the
//! first second after boot: tick()), `BASE` is the start of the second read
//! at boot. After that, resync() (four times a second) compares with the
//! CMOS clock and moves the clock forward when it fell behind (lost timer
//! ticks) or holds it back when it ran ahead; `LAST` keeps the times handed
//! out from going backwards (only DosSetDateTime may move the clock back).

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use crate::arch::{inb, outb};

#[derive(Clone, Copy, Debug, Default)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// 0 = Sunday.
    pub weekday: u8,
}

pub(crate) fn cmos(reg: u8) -> u8 {
    unsafe {
        outb(0x70, reg);
        inb(0x71)
    }
}

fn read_raw() -> [u8; 7] {
    while cmos(0x0a) & 0x80 != 0 {}
    [cmos(0x00), cmos(0x02), cmos(0x04), cmos(0x07), cmos(0x08), cmos(0x09), cmos(0x32)]
}

/// Serialises the CMOS index/data port pair: the timer interrupt reads the
/// clock too (tick()), so it only tries the lock.
static CMOS_LOCK: AtomicBool = AtomicBool::new(false);

fn lock_cmos() {
    while CMOS_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

fn unlock_cmos() {
    CMOS_LOCK.store(false, Ordering::Release);
}

/// Read the CMOS clock (to the second).
fn read_cmos() -> DateTime {
    lock_cmos();
    read_locked()
}

/// Read the CMOS clock if nobody else is reading it (the timer interrupt
/// may have interrupted a reader on this processor).
fn try_read_cmos() -> Option<DateTime> {
    CMOS_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).ok()?;
    Some(read_locked())
}

fn read_locked() -> DateTime {
    // Read until two consecutive reads agree (avoid update tearing).
    let mut a = read_raw();
    loop {
        let b = read_raw();
        if a == b {
            break;
        }
        a = b;
    }
    let status_b = cmos(0x0b);
    unlock_cmos();
    let bcd = status_b & 0x04 == 0;
    let conv = |v: u8| if bcd { (v & 0x0f) + (v >> 4) * 10 } else { v };
    let mut hour = a[2];
    let pm = hour & 0x80 != 0;
    hour = conv(hour & 0x7f);
    if status_b & 0x02 == 0 && pm {
        hour = (hour % 12) + 12;
    }
    let century = if a[6] != 0 { conv(a[6]) as u16 } else { 20 };
    let year = century * 100 + conv(a[5]) as u16;
    let month = conv(a[4]);
    let day = conv(a[3]);
    DateTime {
        year,
        month,
        day,
        hour,
        minute: conv(a[1]),
        second: conv(a[0]),
        weekday: weekday(year, month, day),
    }
}

static BASE: AtomicU64 = AtomicU64::new(0);
static LAST: AtomicU64 = AtomicU64::new(0);
/// 0 = not read yet, 1 = read, waiting for a second edge, 2 = in phase.
static STATE: AtomicU8 = AtomicU8::new(0);
/// The CMOS second register read at boot (tick() waits for it to change).
static BOOT_SECOND: AtomicU8 = AtomicU8::new(0xff);
static STARTING: AtomicBool = AtomicBool::new(false);

/// Read the CMOS clock to start the system clock (at boot).
pub fn start() {
    if STATE.load(Ordering::Acquire) != 0 || STARTING.swap(true, Ordering::AcqRel) {
        // Another CPU may be reading the clock: wait for it.
        while STATE.load(Ordering::Acquire) == 0 {
            core::hint::spin_loop();
        }
        return;
    }
    let t = read_cmos();
    lock_cmos();
    BOOT_SECOND.store(cmos(0x00), Ordering::Relaxed);
    unlock_cmos();
    let up = mono_ms();
    BASE.store((unix_time(&t) as u64 * 1000).saturating_sub(up), Ordering::Release);
    STATE.store(1, Ordering::Release);
}

/// Called by the timer interrupt every millisecond: until the clock is in
/// phase with the CMOS clock, look for its second changing.
pub fn tick() {
    if STATE.load(Ordering::Acquire) != 1 {
        return;
    }
    if CMOS_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        return;
    }
    let busy = cmos(0x0a) & 0x80 != 0;
    let s = cmos(0x00);
    unlock_cmos();
    if busy || s == BOOT_SECOND.load(Ordering::Relaxed) {
        return;
    }
    // A new second has just begun: the second read at boot plus one.
    let up = mono_ms();
    let base = BASE.load(Ordering::Relaxed);
    let now = (base + up) / 1000 * 1000 + 1000;
    BASE.store(now.saturating_sub(up).max(base), Ordering::Release);
    STATE.store(2, Ordering::Release);
}

/// Compare the clock with the CMOS clock (called four times a second).
pub fn resync() {
    if STATE.load(Ordering::Acquire) != 2 {
        return;
    }
    let Some(t) = try_read_cmos() else { return };
    let cm = unix_time(&t) as u64 * 1000;
    let up = mono_ms();
    let base = BASE.load(Ordering::Relaxed);
    let now = base + up;
    if now < cm {
        // Behind (timer ticks lost): catch up.
        BASE.store(cm - up, Ordering::Release);
    } else if now >= cm + 1250 {
        // Ahead by more than the polling explains: hold back gently.
        BASE.store(base - ((now - cm - 1000) / 4).max(1), Ordering::Release);
    }
}

/// The time in milliseconds since 1970-01-01 00:00:00 (local time).
pub fn now_ms() -> u64 {
    start();
    let t = BASE.load(Ordering::Acquire) + mono_ms();
    let last = LAST.fetch_max(t, Ordering::AcqRel);
    t.max(last)
}

/// The current date and time.
pub fn now() -> DateTime {
    from_unix((now_ms() / 1000) as u32)
}

/// The current date and time with hundredths of a second.
pub fn now_hundredths() -> (DateTime, u8) {
    let ms = now_ms();
    (from_unix((ms / 1000) as u32), ((ms % 1000) / 10) as u8)
}

/// The date and time of a count of seconds since 1970.
pub fn from_unix(secs: u32) -> DateTime {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as u16;
    DateTime {
        year: y,
        month: m,
        day: d,
        hour: (rem / 3600) as u8,
        minute: (rem / 60 % 60) as u8,
        second: (rem % 60) as u8,
        weekday: weekday(y, m, d),
    }
}

fn cmos_write(reg: u8, v: u8) {
    unsafe {
        outb(0x70, reg);
        outb(0x71, v);
    }
}

/// Write the CMOS clock.
fn write_cmos(t: &DateTime) {
    lock_cmos();
    let status_b = cmos(0x0b);
    let bcd = status_b & 0x04 == 0;
    let enc = |v: u8| if bcd { (v / 10) << 4 | (v % 10) } else { v };
    let mut hour = t.hour;
    if status_b & 0x02 == 0 {
        // 12-hour mode: 1-12 with bit 7 for PM.
        let pm = hour >= 12;
        hour %= 12;
        if hour == 0 {
            hour = 12;
        }
        hour = enc(hour) | if pm { 0x80 } else { 0 };
    } else {
        hour = enc(hour);
    }
    // Hold updates while writing.
    cmos_write(0x0b, status_b | 0x80);
    cmos_write(0x00, enc(t.second));
    cmos_write(0x02, enc(t.minute));
    cmos_write(0x04, hour);
    cmos_write(0x07, enc(t.day));
    cmos_write(0x08, enc(t.month));
    cmos_write(0x09, enc((t.year % 100) as u8));
    cmos_write(0x32, enc((t.year / 100) as u8));
    cmos_write(0x0b, status_b & !0x80);
    unlock_cmos();
}

/// Set the clock (DosSetDateTime, CLOCK$): the CMOS clock and the system
/// clock, which may go backwards here.
pub fn set(t: &DateTime, hundredths: u8) {
    start();
    write_cmos(t);
    let up = mono_ms();
    let ms = unix_time(t) as u64 * 1000 + hundredths.min(99) as u64 * 10;
    STATE.store(2, Ordering::Release);
    BASE.store(ms.saturating_sub(up), Ordering::Release);
    LAST.store(0, Ordering::Release);
}

// The clock counts milliseconds with the processor's time-stamp counter
// once it has been measured (apic::init, against the PIT): timer ticks can
// be lost (late interrupts under emulation), the counter is not. Before
// that, and without a measurement, the uptime counter.
static TSC_PER_MS: AtomicU64 = AtomicU64::new(0);
static TSC0: AtomicU64 = AtomicU64::new(0);
static MONO0: AtomicU64 = AtomicU64::new(0);

/// The time-stamp counter's rate (counts per millisecond), measured.
pub fn set_tsc_rate(per_ms: u64) {
    if per_ms == 0 || TSC_PER_MS.load(Ordering::Acquire) != 0 {
        return;
    }
    MONO0.store(crate::irq::uptime_ms(), Ordering::Relaxed);
    TSC0.store(crate::arch::rdtsc(), Ordering::Relaxed);
    TSC_PER_MS.store(per_ms, Ordering::Release);
}

/// Time-stamp counts per millisecond (0 until measured).
pub fn tsc_per_ms() -> u64 {
    TSC_PER_MS.load(Ordering::Acquire)
}

/// Milliseconds since boot, from the time-stamp counter when measured.
pub fn mono_ms() -> u64 {
    let per = TSC_PER_MS.load(Ordering::Acquire);
    if per == 0 {
        return crate::irq::uptime_ms();
    }
    MONO0.load(Ordering::Relaxed) + crate::arch::rdtsc().saturating_sub(TSC0.load(Ordering::Relaxed)) / per
}

/// Time since boot in units of `freq` per second: `mono_ms` at the
/// time-stamp counter's resolution, or the uptime counter's until measured.
pub fn mono_scaled(freq: u64) -> u64 {
    let per = TSC_PER_MS.load(Ordering::Acquire);
    if per == 0 {
        return crate::irq::uptime_ms() * freq / 1000;
    }
    let d = crate::arch::rdtsc().saturating_sub(TSC0.load(Ordering::Relaxed));
    (MONO0.load(Ordering::Relaxed) + d / per) * freq / 1000 + d % per * freq / (per * 1000)
}

/// Day of week, 0 = Sunday (Sakamoto's method).
pub fn weekday(y: u16, m: u8, d: u8) -> u8 {
    const T: [u16; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m as usize).clamp(1, 12) - 1] + d as u16) % 7) as u8
}

/// Seconds since 1970-01-01 00:00:00.
/// Years outside 1970-2105 (a torn CMOS read) are clamped to them.
pub fn unix_time(t: &DateTime) -> u32 {
    let days_before = |y: u32| -> u32 { 365 * (y - 1970) + (y - 1969) / 4 - (y - 1901) / 100 + (y - 1601) / 400 };
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let y = (t.year as u32).clamp(1970, 2105);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mut days = days_before(y) + CUM[(t.month as usize).clamp(1, 12) - 1] + (t.day as u32).clamp(1, 31) - 1;
    if leap && t.month > 2 {
        days += 1;
    }
    days.saturating_mul(86400).saturating_add(t.hour.min(23) as u32 * 3600 + t.minute.min(59) as u32 * 60 + t.second.min(59) as u32)
}
