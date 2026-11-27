//! Times and clocks. OS/2 keeps local time and the offset to UTC
//! (DosGetDateTime's timezone, minutes west of UTC, -1 if unknown: then
//! local time is taken as UTC). FILETIMEs are 100 ns units since 1601.

use crate::{err, fail, FALSE, TRUE};

/// SYSTEMTIME.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub weekday: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub ms: u16,
}

/// Days since 1601-01-01 of a date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    // Days since 1970-01-01, then from 1601.
    era * 146097 + doe - 719468 + 134774
}

fn civil_from_days(days1601: i64) -> (i64, i64, i64) {
    let z = days1601 - 134774 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn st_to_ft(st: &SystemTime) -> Option<u64> {
    if st.month == 0 || st.month > 12 || st.day == 0 || st.day > 31 || st.hour > 23 || st.minute > 59 || st.second > 59 || st.ms > 999 || st.year < 1601 {
        return None;
    }
    let days = days_from_civil(st.year as i64, st.month as i64, st.day as i64);
    let secs = days * 86400 + st.hour as i64 * 3600 + st.minute as i64 * 60 + st.second as i64;
    Some(secs as u64 * 10_000_000 + st.ms as u64 * 10_000)
}

pub fn ft_to_st(ft: u64) -> SystemTime {
    let ms_total = ft / 10_000;
    let secs = (ms_total / 1000) as i64;
    let days = secs / 86400;
    let rem = secs % 86400;
    let (y, m, d) = civil_from_days(days);
    SystemTime {
        year: y as u16,
        month: m as u16,
        // 1601-01-01 was a Monday.
        weekday: ((days + 1) % 7) as u16,
        day: d as u16,
        hour: (rem / 3600) as u16,
        minute: (rem % 3600 / 60) as u16,
        second: (rem % 60) as u16,
        ms: (ms_total % 1000) as u16,
    }
}

/// Minutes to add to local time for UTC (OS/2's timezone), 0 if unknown.
pub fn bias() -> i64 {
    let mut dt: os2::DateTime = unsafe { core::mem::zeroed() };
    unsafe { os2::DosGetDateTime(&mut dt) };
    if dt.timezone == -1 { 0 } else { dt.timezone as i64 }
}

fn local_now() -> SystemTime {
    let mut dt: os2::DateTime = unsafe { core::mem::zeroed() };
    unsafe { os2::DosGetDateTime(&mut dt) };
    SystemTime {
        year: dt.year,
        month: dt.month as u16,
        weekday: dt.weekday as u16,
        day: dt.day as u16,
        hour: dt.hours as u16,
        minute: dt.minutes as u16,
        second: dt.seconds as u16,
        ms: dt.hundredths as u16 * 10,
    }
}

pub fn now_ft() -> u64 {
    let l = st_to_ft(&local_now()).unwrap_or(0);
    (l as i64 + bias() * 60 * 10_000_000) as u64
}

pub unsafe fn put_ft(p: *mut u32, ft: u64) {
    if !p.is_null() {
        unsafe {
            *p = ft as u32;
            *p.add(1) = (ft >> 32) as u32;
        }
    }
}

pub unsafe fn get_ft(p: *const u32) -> u64 {
    unsafe { *p as u64 | (*p.add(1) as u64) << 32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetSystemTimeAsFileTime(ft: *mut u32) {
    unsafe { put_ft(ft, now_ft()) };
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetSystemTime(st: *mut SystemTime) {
    unsafe { *st = ft_to_st(now_ft()) };
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetLocalTime(st: *mut SystemTime) {
    unsafe { *st = local_now() };
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SetLocalTime(_st: *const SystemTime) -> i32 {
    fail(err::ACCESS_DENIED, FALSE)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn SystemTimeToFileTime(st: *const SystemTime, ft: *mut u32) -> i32 {
    match st_to_ft(unsafe { &*st }) {
        Some(v) => {
            unsafe { put_ft(ft, v) };
            TRUE
        }
        None => fail(err::INVALID_PARAMETER, FALSE),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FileTimeToSystemTime(ft: *const u32, st: *mut SystemTime) -> i32 {
    let v = unsafe { get_ft(ft) };
    if v >> 63 != 0 {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    unsafe { *st = ft_to_st(v) };
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FileTimeToLocalFileTime(ft: *const u32, local: *mut u32) -> i32 {
    let v = unsafe { get_ft(ft) } as i64 - bias() * 600_000_000;
    unsafe { put_ft(local, v as u64) };
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn LocalFileTimeToFileTime(local: *const u32, ft: *mut u32) -> i32 {
    let v = unsafe { get_ft(local) } as i64 + bias() * 600_000_000;
    unsafe { put_ft(ft, v as u64) };
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn CompareFileTime(a: *const u32, b: *const u32) -> i32 {
    let (a, b) = unsafe { (get_ft(a), get_ft(b)) };
    (a > b) as i32 - (a < b) as i32
}

/// A FILETIME (UTC) from OS/2's FDATE and FTIME (local).
pub fn ft_from_os2(date: u16, time: u16) -> u64 {
    if date == 0 {
        return 0;
    }
    let st = SystemTime {
        year: 1980 + (date >> 9),
        month: (date >> 5) & 15,
        day: date & 31,
        hour: time >> 11,
        minute: (time >> 5) & 63,
        second: (time & 31) * 2,
        ..Default::default()
    };
    st_to_ft(&st).map_or(0, |l| (l as i64 + bias() * 600_000_000) as u64)
}

/// OS/2's FDATE and FTIME (local) of a FILETIME (UTC).
pub fn os2_from_ft(ft: u64) -> (u16, u16) {
    let st = ft_to_st((ft as i64 - bias() * 600_000_000) as u64);
    let y = st.year.clamp(1980, 2107) - 1980;
    (y << 9 | st.month << 5 | st.day, st.hour << 11 | st.minute << 5 | st.second / 2)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FileTimeToDosDateTime(ft: *const u32, date: *mut u16, time: *mut u16) -> i32 {
    let st = ft_to_st(unsafe { get_ft(ft) });
    let y = st.year.clamp(1980, 2107) - 1980;
    unsafe {
        *date = y << 9 | st.month << 5 | st.day;
        *time = st.hour << 11 | st.minute << 5 | st.second / 2;
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn DosDateTimeToFileTime(date: u16, time: u16, ft: *mut u32) -> i32 {
    let st = SystemTime {
        year: 1980 + (date >> 9),
        month: (date >> 5) & 15,
        day: date & 31,
        hour: time >> 11,
        minute: (time >> 5) & 63,
        second: (time & 31) * 2,
        ..Default::default()
    };
    match st_to_ft(&st) {
        Some(v) => {
            unsafe { put_ft(ft, v) };
            TRUE
        }
        None => fail(err::INVALID_PARAMETER, FALSE),
    }
}

/// TIME_ZONE_INFORMATION: Bias, then names and dates nobody here knows.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GetTimeZoneInformation(tz: *mut u8) -> u32 {
    unsafe {
        core::ptr::write_bytes(tz, 0, 172);
        (tz as *mut i32).write(bias() as i32);
    }
    0 // TIME_ZONE_ID_UNKNOWN
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetTickCount() -> u32 {
    let mut ms = 0u32;
    unsafe { os2::DosQuerySysInfo(14, 14, &mut ms, 4) };
    ms
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn QueryPerformanceCounter(c: *mut u64) -> i32 {
    let mut t = 0u64;
    unsafe {
        os2::DosTmrQueryTime(&mut t);
        c.write_unaligned(t);
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn QueryPerformanceFrequency(f: *mut u64) -> i32 {
    let mut hz = 0u32;
    unsafe {
        os2::DosTmrQueryFreq(&mut hz);
        f.write_unaligned(hz as u64);
    }
    TRUE
}
