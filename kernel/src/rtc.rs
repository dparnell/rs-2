//! CMOS real-time clock.

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

fn cmos(reg: u8) -> u8 {
    unsafe {
        outb(0x70, reg);
        inb(0x71)
    }
}

fn read_raw() -> [u8; 7] {
    while cmos(0x0a) & 0x80 != 0 {}
    [cmos(0x00), cmos(0x02), cmos(0x04), cmos(0x07), cmos(0x08), cmos(0x09), cmos(0x32)]
}

pub fn now() -> DateTime {
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

fn cmos_write(reg: u8, v: u8) {
    unsafe {
        outb(0x70, reg);
        outb(0x71, v);
    }
}

/// Set the clock (DosSetDateTime).
pub fn set(t: &DateTime) {
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
