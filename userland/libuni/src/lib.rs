//! LIBUNI.DLL: Unicode support. For now the locale object calls, with one
//! locale (en_US, code page 850) whatever name is asked for.
//!
//! Strings returned by UniQueryLocaleItem are UniChar (UCS-2) strings in
//! memory the caller frees with UniFreeMem.

#![no_std]
#![no_main]
#![allow(non_snake_case)]

extern crate os2;

mod exports;

const ULS_SUCCESS: i32 = 0;
const ULS_NOMEMORY: i32 = 0x20400 | 13;
const ULS_INVALID: i32 = 0x20400 | 14;
const ULS_BADOBJECT: i32 = 0x20400 | 15;

/// The locale object handle (the only locale).
const LOCALE: u32 = 0x4c53_0001;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

/// String items of en_US (LOCI_* number, value).
const STRINGS: &[(i32, &str)] = &[
    (1, "%a %e %b %H:%M:%S %Y"),
    (2, "%m/%d/%y"),
    (3, "%H:%M:%S"),
    (4, "AM"),
    (5, "PM"),
    (6, "Sun"),
    (7, "Mon"),
    (8, "Tue"),
    (9, "Wed"),
    (10, "Thu"),
    (11, "Fri"),
    (12, "Sat"),
    (13, "Sunday"),
    (14, "Monday"),
    (15, "Tuesday"),
    (16, "Wednesday"),
    (17, "Thursday"),
    (18, "Friday"),
    (19, "Saturday"),
    (20, "Jan"),
    (21, "Feb"),
    (22, "Mar"),
    (23, "Apr"),
    (24, "May"),
    (25, "Jun"),
    (26, "Jul"),
    (27, "Aug"),
    (28, "Sep"),
    (29, "Oct"),
    (30, "Nov"),
    (31, "Dec"),
    (32, "January"),
    (33, "February"),
    (34, "March"),
    (35, "April"),
    (36, "May"),
    (37, "June"),
    (38, "July"),
    (39, "August"),
    (40, "September"),
    (41, "October"),
    (42, "November"),
    (43, "December"),
    (44, "."),
    (45, ","),
    (46, "yes"),
    (47, "no"),
    (48, "$"),
    (49, "IBM-850"),
    (53, "0123456789"),
    (63, "/"),
    (64, ":"),
    (65, ","),
    (66, "."),
    (67, ","),
    (68, "3;0"),
    (69, "3;0"),
    (87, "USD"),
    (88, "ENU"),
    (92, ""),
    (93, "-"),
    (96, "%A, %B %d, %Y"),
    (100, "en_US"),
    (101, "en"),
    (102, "US"),
    (103, "English"),
    (104, "English"),
    (105, "United States"),
    (106, "United States"),
    (107, "United States"),
    (109, "ISO8859-1"),
    (115, "437"),
    (116, "850"),
    (117, "US"),
    (119, "US"),
];

/// Numeric items (LOCI_* number, value).
const VALUES: &[(i32, i32)] = &[
    (54, 136),
    (70, 2),
    (71, 1),
    (72, 2),
    (73, 0),
    (74, 0),
    (75, 0),
    (76, 2),
    (77, 1),
    (78, 1),
    (79, 0),
    (81, 6),
    (84, 0),
    (85, 0),
    (108, 1),
    (110, 1252),
    (111, 850),
    (112, 437),
    (113, 10000),
    (114, 37),
];

/// UniCreateLocaleObject(locale_spec_type, locale_spec, &locale_object)
#[unsafe(no_mangle)]
pub extern "C" fn UniCreateLocaleObject(_ty: i32, _spec: *const u8, out: *mut u32) -> i32 {
    if out.is_null() {
        return ULS_INVALID;
    }
    unsafe { out.write_unaligned(LOCALE) };
    ULS_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn UniFreeLocaleObject(lo: u32) -> i32 {
    if lo == LOCALE { ULS_SUCCESS } else { ULS_BADOBJECT }
}

/// UniQueryLocaleItem(locale_object, item, &info_item): a UniChar string
/// to free with UniFreeMem.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLocaleItem(lo: u32, item: i32, out: *mut *mut u16) -> i32 {
    if lo != LOCALE {
        return ULS_BADOBJECT;
    }
    if out.is_null() {
        return ULS_INVALID;
    }
    let item = item & 0x7fff;
    let mut buf = [0u8; 16];
    let s: &str = match STRINGS.iter().find(|(i, _)| *i == item) {
        Some((_, s)) => s,
        None => match VALUES.iter().find(|(i, _)| *i == item) {
            Some((_, v)) => decimal(*v, &mut buf),
            None if (1..=136).contains(&item) => "",
            None => return ULS_INVALID,
        },
    };
    let mut p: *mut u8 = core::ptr::null_mut();
    let cb = (s.len() as u32 + 1) * 2;
    if unsafe { os2::DosAllocMem(&mut p, cb, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT) } != 0 {
        return ULS_NOMEMORY;
    }
    let u = p as *mut u16;
    for (i, b) in s.bytes().enumerate() {
        unsafe { u.add(i).write(b as u16) };
    }
    unsafe {
        u.add(s.len()).write(0);
        out.write_unaligned(u);
    }
    ULS_SUCCESS
}

fn decimal(v: i32, buf: &mut [u8; 16]) -> &str {
    let mut n = v.unsigned_abs();
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if v < 0 {
        i -= 1;
        buf[i] = b'-';
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("")
}

/// UniQueryLocaleValue(locale_object, item, &value): numeric items, and
/// string items that hold a number (code pages).
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLocaleValue(lo: u32, item: i32, out: *mut i32) -> i32 {
    if lo != LOCALE {
        return ULS_BADOBJECT;
    }
    if out.is_null() {
        return ULS_INVALID;
    }
    let item = item & 0x7fff;
    let v = VALUES.iter().find(|(i, _)| *i == item).map(|(_, v)| *v).or_else(|| {
        let (_, s) = STRINGS.iter().find(|(i, _)| *i == item)?;
        let mut n: i32 = 0;
        for b in s.bytes() {
            if !b.is_ascii_digit() {
                return None;
            }
            n = n * 10 + (b - b'0') as i32;
        }
        (!s.is_empty()).then_some(n)
    });
    match v {
        Some(v) => {
            unsafe { out.write_unaligned(v) };
            ULS_SUCCESS
        }
        None => ULS_INVALID,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn UniFreeMem(p: *mut u8) -> i32 {
    if p.is_null() {
        return ULS_INVALID;
    }
    unsafe { os2::DosFreeMem(p) };
    ULS_SUCCESS
}
