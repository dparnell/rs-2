//! HPFS time stamps: seconds since 1970-01-01 00:00 (local time, as OS/2
//! records them), and conversion to and from DOS packed date/time.

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Civil date (year, month, day) of a day number since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Seconds since 1970 for a calendar date and time.
pub fn from_parts(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> u32 {
    let days = days_from_civil(year as i64, month.clamp(1, 12) as u32, day.clamp(1, 31) as u32);
    let s = days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    s.clamp(0, u32::MAX as i64) as u32
}

/// Seconds since 1970 from a DOS (FDATE, FTIME) pair; 0 stays 0.
pub fn from_dos(date: u16, time: u16) -> u32 {
    if date == 0 {
        return 0;
    }
    from_parts(
        1980 + (date >> 9),
        ((date >> 5) & 0xf) as u8,
        (date & 0x1f) as u8,
        (time >> 11) as u8,
        ((time >> 5) & 0x3f) as u8,
        ((time & 0x1f) * 2) as u8,
    )
}

/// DOS (FDATE, FTIME) for seconds since 1970; times before 1980 give 0.
pub fn to_dos(secs: u32) -> (u16, u16) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (y, m, d) = civil_from_days(days);
    if !(1980..=2107).contains(&y) {
        return (0, 0);
    }
    let date = ((y - 1980) as u16) << 9 | (m as u16) << 5 | d as u16;
    let time = ((rem / 3600) as u16) << 11 | ((rem % 3600 / 60) as u16) << 5 | (rem % 60 / 2) as u16;
    (date, time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        assert_eq!(from_parts(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(from_parts(2000, 3, 1, 0, 0, 0), 951_868_800);
        let t = from_parts(2026, 10, 18, 12, 34, 56);
        let (d, tm) = to_dos(t);
        assert_eq!(from_dos(d, tm), t);
        assert_eq!(to_dos(0), (0, 0));
    }
}
