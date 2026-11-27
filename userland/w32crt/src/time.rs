//! <time.h>: time_t in seconds since 1970 (32- and 64-bit forms), struct
//! tm, the time zone from Windows' bias, strftime, clock.

use crate::{cstr, k};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Tm {
    pub sec: i32,
    pub min: i32,
    pub hour: i32,
    pub mday: i32,
    pub mon: i32,
    pub year: i32,
    pub wday: i32,
    pub yday: i32,
    pub isdst: i32,
}

#[unsafe(no_mangle)]
pub static mut _timezone: i32 = 0;
#[unsafe(no_mangle)]
pub static mut _daylight: i32 = 0;
#[unsafe(no_mangle)]
pub static mut _dstbias: i32 = -3600;
static mut TZNAME0: [u8; 64] = *b"UTC\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
static mut TZNAME1: [u8; 64] = [0; 64];
#[unsafe(no_mangle)]
pub static mut _tzname: [*mut u8; 2] = [unsafe { (&raw mut TZNAME0) as *mut u8 }, unsafe { (&raw mut TZNAME1) as *mut u8 }];

#[unsafe(no_mangle)]
pub extern "C" fn __p__timezone() -> *mut i32 {
    unsafe { &raw mut _timezone }
}

#[unsafe(no_mangle)]
pub extern "C" fn __p__daylight() -> *mut i32 {
    unsafe { &raw mut _daylight }
}

#[unsafe(no_mangle)]
pub extern "C" fn __p__dstbias() -> *mut i32 {
    unsafe { &raw mut _dstbias }
}

#[unsafe(no_mangle)]
pub extern "C" fn __p__tzname() -> *mut [*mut u8; 2] {
    unsafe { &raw mut _tzname }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _get_timezone(p: *mut i32) -> i32 {
    _tzset();
    unsafe { *p = _timezone };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _get_daylight(p: *mut i32) -> i32 {
    unsafe { *p = 0 };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _get_dstbias(p: *mut i32) -> i32 {
    unsafe { *p = _dstbias };
    0
}

/// Seconds to add to local time for UTC.
fn bias_secs() -> i64 {
    let mut tz = [0u8; 172];
    unsafe { k::GetTimeZoneInformation(tz.as_mut_ptr()) };
    i32::from_le_bytes([tz[0], tz[1], tz[2], tz[3]]) as i64 * 60
}

#[unsafe(no_mangle)]
pub extern "C" fn _tzset() {
    unsafe { _timezone = bias_secs() as i32 };
}

pub fn now() -> i64 {
    let mut ft = 0u64;
    unsafe { k::GetSystemTimeAsFileTime(&mut ft) };
    crate::fs::ft_to_time(ft)
}

unsafe fn put_time<T: TryFrom<i64>>(p: *mut T, t: i64) {
    if !p.is_null() {
        if let Ok(v) = T::try_from(t) {
            unsafe { p.write_unaligned(v) };
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn time(p: *mut i32) -> i32 {
    let t = now();
    unsafe { put_time(p, t) };
    t as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _time32(p: *mut i32) -> i32 {
    unsafe { time(p) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _time64(p: *mut i64) -> i64 {
    let t = now();
    unsafe { put_time(p, t) };
    t
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn to_tm(t: i64) -> Tm {
    let days = t.div_euclid(86400);
    let rem = t.rem_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let yday = days - days_from_civil(y, 1, 1);
    Tm {
        sec: (rem % 60) as i32,
        min: (rem / 60 % 60) as i32,
        hour: (rem / 3600) as i32,
        mday: d as i32,
        mon: m as i32 - 1,
        year: y as i32 - 1900,
        wday: (days + 4).rem_euclid(7) as i32,
        yday: yday as i32,
        isdst: 0,
    }
}

/// mktime's arithmetic: a struct tm (fields may be out of range) as
/// seconds, normalising it.
pub fn from_tm(tm: &mut Tm) -> i64 {
    let mut y = tm.year as i64 + 1900;
    let mut m = tm.mon as i64;
    y += m.div_euclid(12);
    m = m.rem_euclid(12);
    let days = days_from_civil(y, m + 1, 1) + tm.mday as i64 - 1;
    let t = days * 86400 + tm.hour as i64 * 3600 + tm.min as i64 * 60 + tm.sec as i64;
    let isdst = tm.isdst;
    *tm = to_tm(t);
    let _ = isdst;
    tm.isdst = 0;
    t
}

static mut TM: Tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0 };

fn static_tm(v: Tm) -> *mut Tm {
    unsafe {
        TM = v;
        &raw mut TM
    }
}

unsafe fn read_t32(p: *const i32) -> i64 {
    if p.is_null() { 0 } else { (unsafe { p.read_unaligned() }) as i64 }
}

unsafe fn read_t64(p: *const i64) -> i64 {
    if p.is_null() { 0 } else { unsafe { p.read_unaligned() } }
}

pub fn local(t: i64) -> Tm {
    to_tm(t - bias_secs())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn localtime(p: *const i32) -> *mut Tm {
    static_tm(local(unsafe { read_t32(p) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _localtime32(p: *const i32) -> *mut Tm {
    static_tm(local(unsafe { read_t32(p) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _localtime64(p: *const i64) -> *mut Tm {
    static_tm(local(unsafe { read_t64(p) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _localtime32_s(out: *mut Tm, p: *const i32) -> i32 {
    unsafe { *out = local(read_t32(p)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _localtime64_s(out: *mut Tm, p: *const i64) -> i32 {
    unsafe { *out = local(read_t64(p)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gmtime(p: *const i32) -> *mut Tm {
    static_tm(to_tm(unsafe { read_t32(p) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _gmtime32(p: *const i32) -> *mut Tm {
    static_tm(to_tm(unsafe { read_t32(p) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _gmtime64(p: *const i64) -> *mut Tm {
    static_tm(to_tm(unsafe { read_t64(p) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _gmtime32_s(out: *mut Tm, p: *const i32) -> i32 {
    unsafe { *out = to_tm(read_t32(p)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _gmtime64_s(out: *mut Tm, p: *const i64) -> i32 {
    unsafe { *out = to_tm(read_t64(p)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mktime(tm: *mut Tm) -> i32 {
    (from_tm(unsafe { &mut *tm }) + bias_secs()) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _mktime32(tm: *mut Tm) -> i32 {
    unsafe { mktime(tm) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _mktime64(tm: *mut Tm) -> i64 {
    from_tm(unsafe { &mut *tm }) + bias_secs()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _mkgmtime(tm: *mut Tm) -> i32 {
    from_tm(unsafe { &mut *tm }) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _mkgmtime32(tm: *mut Tm) -> i32 {
    from_tm(unsafe { &mut *tm }) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _mkgmtime64(tm: *mut Tm) -> i64 {
    from_tm(unsafe { &mut *tm })
}

#[unsafe(no_mangle)]
pub extern "C" fn difftime(a: i32, b: i32) -> f64 {
    a as f64 - b as f64
}

#[unsafe(no_mangle)]
pub extern "C" fn _difftime32(a: i32, b: i32) -> f64 {
    a as f64 - b as f64
}

#[unsafe(no_mangle)]
pub extern "C" fn _difftime64(a: i64, b: i64) -> f64 {
    a as f64 - b as f64
}

static START: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// clock(): milliseconds since the process started (CLOCKS_PER_SEC 1000).
#[unsafe(no_mangle)]
pub extern "C" fn clock() -> i32 {
    let t = unsafe { k::GetTickCount() };
    let s = START.load(core::sync::atomic::Ordering::Relaxed);
    if s == 0 {
        START.store(t, core::sync::atomic::Ordering::Relaxed);
        return 0;
    }
    t.wrapping_sub(s) as i32
}

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn asc(tm: &Tm) -> alloc::string::String {
    alloc::format!(
        "{} {} {:2} {:02}:{:02}:{:02} {}\n",
        &DAYS[tm.wday.rem_euclid(7) as usize][..3],
        &MONTHS[tm.mon.rem_euclid(12) as usize][..3],
        tm.mday,
        tm.hour,
        tm.min,
        tm.sec,
        tm.year + 1900
    )
}

static mut ASC: [u8; 32] = [0; 32];

fn static_str(s: &str) -> *mut u8 {
    unsafe {
        let b = &mut *(&raw mut ASC);
        let n = s.len().min(31);
        b[..n].copy_from_slice(&s.as_bytes()[..n]);
        b[n] = 0;
        b.as_mut_ptr()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn asctime(tm: *const Tm) -> *mut u8 {
    static_str(&asc(unsafe { &*tm }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ctime(p: *const i32) -> *mut u8 {
    static_str(&asc(&local(unsafe { read_t32(p) })))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ctime32(p: *const i32) -> *mut u8 {
    unsafe { ctime(p) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ctime64(p: *const i64) -> *mut u8 {
    static_str(&asc(&local(unsafe { read_t64(p) })))
}

/// strftime with the C locale.
pub fn strftime_impl(fmt: &[u8], tm: &Tm) -> alloc::vec::Vec<u8> {
    use alloc::format;
    let mut out = alloc::vec::Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' || i + 1 >= fmt.len() {
            out.push(fmt[i]);
            i += 1;
            continue;
        }
        i += 1;
        // Microsoft's '#' flag: no leading zeros.
        let alt = fmt[i] == b'#';
        if alt {
            i += 1;
        }
        let c = fmt[i];
        i += 1;
        let num = |v: i32, w: usize| if alt { format!("{v}") } else { format!("{v:0w$}") };
        let s = match c {
            b'a' => DAYS[tm.wday.rem_euclid(7) as usize][..3].into(),
            b'A' => DAYS[tm.wday.rem_euclid(7) as usize].into(),
            b'b' | b'h' => MONTHS[tm.mon.rem_euclid(12) as usize][..3].into(),
            b'B' => MONTHS[tm.mon.rem_euclid(12) as usize].into(),
            b'c' => format!("{:02}/{:02}/{:02} {:02}:{:02}:{:02}", tm.mon + 1, tm.mday, (tm.year + 1900) % 100, tm.hour, tm.min, tm.sec),
            b'd' => num(tm.mday, 2),
            b'e' => format!("{:2}", tm.mday),
            b'D' | b'x' => format!("{:02}/{:02}/{:02}", tm.mon + 1, tm.mday, (tm.year + 1900) % 100),
            b'F' => format!("{}-{:02}-{:02}", tm.year + 1900, tm.mon + 1, tm.mday),
            b'H' => num(tm.hour, 2),
            b'I' => num((tm.hour + 11) % 12 + 1, 2),
            b'j' => num(tm.yday + 1, 3),
            b'm' => num(tm.mon + 1, 2),
            b'M' => num(tm.min, 2),
            b'n' => "\n".into(),
            b'p' => (if tm.hour < 12 { "AM" } else { "PM" }).into(),
            b'r' => format!("{:02}:{:02}:{:02} {}", (tm.hour + 11) % 12 + 1, tm.min, tm.sec, if tm.hour < 12 { "AM" } else { "PM" }),
            b'R' => format!("{:02}:{:02}", tm.hour, tm.min),
            b'S' => num(tm.sec, 2),
            b't' => "\t".into(),
            b'T' | b'X' => format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec),
            b'u' => format!("{}", if tm.wday == 0 { 7 } else { tm.wday }),
            b'w' => format!("{}", tm.wday),
            b'U' => num((tm.yday + 7 - tm.wday) / 7, 2),
            b'W' => num((tm.yday + 7 - (tm.wday + 6) % 7) / 7, 2),
            b'y' => num((tm.year + 1900) % 100, 2),
            b'Y' => format!("{}", tm.year + 1900),
            b'C' => num((tm.year + 1900) / 100, 2),
            b'z' => {
                let b = -bias_secs() / 60;
                format!("{}{:02}{:02}", if b < 0 { '-' } else { '+' }, b.abs() / 60, b.abs() % 60)
            }
            b'Z' => "UTC".into(),
            b'%' => "%".into(),
            o => format!("%{}", o as char),
        };
        out.extend_from_slice(s.as_bytes());
    }
    out
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strftime(buf: *mut u8, max: usize, fmt: *const u8, tm: *const Tm) -> usize {
    let s = strftime_impl(unsafe { cstr(fmt) }, unsafe { &*tm });
    if s.len() + 1 > max {
        if max > 0 {
            unsafe { *buf = 0 };
        }
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
    }
    s.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsftime(buf: *mut u16, max: usize, fmt: *const u16, tm: *const Tm) -> usize {
    let f = crate::narrow(unsafe { crate::wstr(fmt) });
    let s = crate::widen(&strftime_impl(&f, unsafe { &*tm }));
    if s.len() + 1 > max {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
    }
    s.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strftime_l(buf: *mut u8, max: usize, fmt: *const u8, tm: *const Tm, _l: u32) -> usize {
    unsafe { strftime(buf, max, fmt, tm) }
}

/// _timeb: time (32-bit), millitm, timezone, dstflag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ftime(tb: *mut u8) {
    let mut ft = 0u64;
    unsafe { k::GetSystemTimeAsFileTime(&mut ft) };
    unsafe {
        (tb as *mut i32).write_unaligned(crate::fs::ft_to_time(ft) as i32);
        (tb.add(4) as *mut u16).write_unaligned(((ft / 10_000) % 1000) as u16);
        (tb.add(6) as *mut i16).write_unaligned((bias_secs() / 60) as i16);
        (tb.add(8) as *mut i16).write_unaligned(0);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ftime32(tb: *mut u8) {
    unsafe { _ftime(tb) }
}

/// __timeb64: time (64-bit), millitm, timezone, dstflag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ftime64(tb: *mut u8) {
    let mut ft = 0u64;
    unsafe { k::GetSystemTimeAsFileTime(&mut ft) };
    unsafe {
        (tb as *mut i64).write_unaligned(crate::fs::ft_to_time(ft));
        (tb.add(8) as *mut u16).write_unaligned(((ft / 10_000) % 1000) as u16);
        (tb.add(10) as *mut i16).write_unaligned((bias_secs() / 60) as i16);
        (tb.add(12) as *mut i16).write_unaligned(0);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strdate(buf: *mut u8) -> *mut u8 {
    let tm = local(now());
    let s = alloc::format!("{:02}/{:02}/{:02}\0", tm.mon + 1, tm.mday, (tm.year + 1900) % 100);
    unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len()) };
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _strtime(buf: *mut u8) -> *mut u8 {
    let tm = local(now());
    let s = alloc::format!("{:02}:{:02}:{:02}\0", tm.hour, tm.min, tm.sec);
    unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len()) };
    buf
}

#[allow(dead_code)]
fn unused() {
    let _ = is_leap;
}
