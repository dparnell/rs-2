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

/// Day of week, 0 = Sunday (Sakamoto's method).
pub fn weekday(y: u16, m: u8, d: u8) -> u8 {
    const T: [u16; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m as usize).clamp(1, 12) - 1] + d as u16) % 7) as u8
}

/// Seconds since 1970-01-01 00:00:00.
pub fn unix_time(t: &DateTime) -> u32 {
    let days_before = |y: u32| -> u32 { 365 * (y - 1970) + (y - 1969) / 4 - (y - 1901) / 100 + (y - 1601) / 400 };
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let y = t.year as u32;
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mut days = days_before(y) + CUM[(t.month as usize).clamp(1, 12) - 1] + t.day as u32 - 1;
    if leap && t.month > 2 {
        days += 1;
    }
    days * 86400 + t.hour as u32 * 3600 + t.minute as u32 * 60 + t.second as u32
}
