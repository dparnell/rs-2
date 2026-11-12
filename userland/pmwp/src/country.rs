//! WPCONFIG's Country object (WPCountry, `<WP_CNTRY>`), rs2's own
//! implementation after IBM's (studied with Ghidra: cou_wpAddSettingsPages,
//! CountryDlg, SetCountryData, WriteIntlWin): four pages, Country, Time,
//! Date and Numbers, that edit the national conventions PM programs read
//! from OS2.INI's `PM_National` (iCountry, iDate, iCurrency, iDigits,
//! iTime, iLzero, s1159, s2359, sCurrency, sThousand, sDecimal, sDate,
//! sTime, sList). Choosing a country takes its conventions from rs2's
//! country table (DosQueryCtryInfo, the kernel's NLS data, as COUNTRY.SYS
//! has them); Undo goes back to what they were when the notebook opened,
//! Default to the chosen country's own. Every change is written at once;
//! with WIN-OS/2 installed, WIN.INI's `[intl]` follows, as IBM's does.
//! Warp 4's Locale program (GLOCALE) edits the same keys through its
//! locales.

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio, spin, spin_init, spin_value};
use crate::som;
use crate::syssetup::{entry, group, listbox, text, undo_default, ID_DEFAULT, ID_UNDO, SPBN_CHANGE};
use crate::util::{parse_dec, prf_read, prf_write, HINI_USER};
use crate::wps::{self, Impl};

/// Countries by their telephone codes, as COUNTRY.SYS numbers them (the
/// ones rs2's country table has).
pub const NAMES: &[(u16, &[u8])] = &[
    (1, b"United States"), (2, b"Canada (French)"), (3, b"Latin America"), (7, b"Russia"),
    (20, b"Egypt"), (27, b"South Africa"), (30, b"Greece"), (31, b"Netherlands"), (32, b"Belgium"),
    (33, b"France"), (34, b"Spain"), (36, b"Hungary"), (38, b"Yugoslavia"), (39, b"Italy"),
    (40, b"Romania"), (41, b"Switzerland"), (42, b"Czechoslovakia"), (43, b"Austria"),
    (44, b"United Kingdom"), (45, b"Denmark"), (46, b"Sweden"), (47, b"Norway"), (48, b"Poland"),
    (49, b"Germany"), (51, b"Peru"), (52, b"Mexico"), (54, b"Argentina"), (55, b"Brazil"),
    (57, b"Colombia"), (58, b"Venezuela"), (61, b"Australia"), (62, b"Indonesia"),
    (64, b"New Zealand"), (65, b"Singapore"), (66, b"Thailand"), (81, b"Japan"), (82, b"Korea"),
    (86, b"China"), (88, b"Taiwan"), (90, b"Turkey"), (91, b"India"), (92, b"Pakistan"),
    (99, b"Asia (English)"), (212, b"Morocco"), (213, b"Algeria"), (216, b"Tunisia"),
    (351, b"Portugal"), (352, b"Luxembourg"), (353, b"Ireland"), (354, b"Iceland"),
    (355, b"Albania"), (358, b"Finland"), (359, b"Bulgaria"), (370, b"Lithuania"),
    (371, b"Latvia"), (372, b"Estonia"), (375, b"Belarus"), (380, b"Ukraine"), (381, b"Serbia"),
    (385, b"Croatia"), (386, b"Slovenia"), (387, b"Bosnia and Herzegovina"), (389, b"Macedonia"),
    (421, b"Czech Republic"), (422, b"Slovakia"), (502, b"Guatemala"), (503, b"El Salvador"),
    (504, b"Honduras"), (505, b"Nicaragua"), (506, b"Costa Rica"), (507, b"Panama"),
    (591, b"Bolivia"), (593, b"Ecuador"), (595, b"Paraguay"), (598, b"Uruguay"),
    (785, b"Arabic speaking"), (961, b"Lebanon"), (962, b"Jordan"), (963, b"Syria"),
    (965, b"Kuwait"), (966, b"Saudi Arabia"), (967, b"Yemen"), (968, b"Oman"),
    (971, b"United Arab Emirates"), (972, b"Israel"), (974, b"Qatar"),
];

// ---- The conventions (PM_National) ----

const APP: &[u8] = b"PM_National";

#[derive(Clone, Default, PartialEq, Eq)]
pub struct National {
    pub country: u32,
    /// 0 month-day-year, 1 day-month-year, 2 year-month-day.
    pub date: u32,
    /// 0 $1, 1 1$, 2 $ 1, 3 1 $.
    pub currency: u32,
    pub digits: u32,
    /// 0 12-hour, 1 24-hour.
    pub time: u32,
    /// Numbers below one with a 0 before the decimal separator.
    pub lzero: u32,
    pub s1159: Vec<u8>,
    pub s2359: Vec<u8>,
    pub s_currency: Vec<u8>,
    pub s_thousand: Vec<u8>,
    pub s_decimal: Vec<u8>,
    pub s_date: Vec<u8>,
    pub s_time: Vec<u8>,
    pub s_list: Vec<u8>,
}

fn sep(b: [u8; 2]) -> Vec<u8> {
    b.iter().take_while(|&&c| c != 0).copied().collect()
}

/// A country's own conventions (rs2's country table; 0 = the system's).
pub fn of_country(code: u32) -> National {
    let cc = os2::CountryCode { country: code, codepage: 0 };
    let mut ci: os2::CountryInfo = unsafe { core::mem::zeroed() };
    let mut cb = 0u32;
    unsafe { os2::DosQueryCtryInfo(core::mem::size_of::<os2::CountryInfo>() as u32, &cc, &mut ci, &mut cb) };
    let twelve = ci.time_fmt & 1 == 0;
    National {
        country: if code != 0 { code } else { ci.country },
        date: ci.date_fmt.min(2),
        currency: (ci.currency_fmt & 3) as u32,
        digits: ci.decimals as u32,
        time: !twelve as u32,
        lzero: 0,
        s1159: if twelve { b"AM".to_vec() } else { Vec::new() },
        s2359: if twelve { b"PM".to_vec() } else { Vec::new() },
        s_currency: ci.currency.iter().take_while(|&&c| c != 0).copied().collect(),
        s_thousand: sep(ci.thousands),
        s_decimal: sep(ci.decimal),
        s_date: sep(ci.date_sep),
        s_time: sep(ci.time_sep),
        s_list: sep(ci.data_sep),
    }
}

fn read_str(key: &[u8]) -> Option<Vec<u8>> {
    let v = prf_read(HINI_USER, APP, key)?;
    Some(v.strip_suffix(&[0]).unwrap_or(&v).to_vec())
}

fn read_int(key: &[u8]) -> Option<u32> {
    parse_dec(read_str(key)?.trim_ascii())
}

/// The conventions as OS2.INI has them (the system country's for keys
/// it lacks).
pub fn load() -> National {
    let country = read_int(b"iCountry").unwrap_or(0);
    let d = of_country(country);
    National {
        country: if country != 0 { country } else { d.country },
        date: read_int(b"iDate").unwrap_or(d.date),
        currency: read_int(b"iCurrency").unwrap_or(d.currency),
        digits: read_int(b"iDigits").unwrap_or(d.digits),
        time: read_int(b"iTime").unwrap_or(d.time),
        lzero: read_int(b"iLzero").unwrap_or(d.lzero),
        s1159: read_str(b"s1159").unwrap_or(d.s1159),
        s2359: read_str(b"s2359").unwrap_or(d.s2359),
        s_currency: read_str(b"sCurrency").unwrap_or(d.s_currency),
        s_thousand: read_str(b"sThousand").unwrap_or(d.s_thousand),
        s_decimal: read_str(b"sDecimal").unwrap_or(d.s_decimal),
        s_date: read_str(b"sDate").unwrap_or(d.s_date),
        s_time: read_str(b"sTime").unwrap_or(d.s_time),
        s_list: read_str(b"sList").unwrap_or(d.s_list),
    }
}

fn pairs(n: &National) -> Vec<(&'static [u8], Vec<u8>)> {
    let i = |v: u32| alloc::format!("{v}").into_bytes();
    alloc::vec![
        (&b"iCountry"[..], i(n.country)),
        (b"iDate", i(n.date)),
        (b"iCurrency", i(n.currency)),
        (b"iDigits", i(n.digits)),
        (b"iTime", i(n.time)),
        (b"iLzero", i(n.lzero)),
        (b"s1159", n.s1159.clone()),
        (b"s2359", n.s2359.clone()),
        (b"sCurrency", n.s_currency.clone()),
        (b"sThousand", n.s_thousand.clone()),
        (b"sDecimal", n.s_decimal.clone()),
        (b"sDate", n.s_date.clone()),
        (b"sTime", n.s_time.clone()),
        (b"sList", n.s_list.clone()),
    ]
}

/// Keep the conventions: OS2.INI's `PM_National`, and WIN.INI's `[intl]`
/// where WIN-OS/2 is installed.
pub fn store(n: &National) {
    for (k, v) in pairs(n) {
        let mut z = v;
        z.push(0);
        prf_write(HINI_USER, APP, k, Some(&z));
    }
    crate::wincfg::write_intl(&pairs(n));
}

// ---- Samples ----

fn two(v: u32, lead: bool) -> Vec<u8> {
    if v < 10 && !lead { alloc::format!("{v}").into_bytes() } else { alloc::format!("{v:02}").into_bytes() }
}

/// The time as the conventions show it.
pub fn format_time(n: &National, h: u32, mi: u32, s: u32) -> Vec<u8> {
    let mut out = Vec::new();
    if n.time == 0 {
        let h12 = if h % 12 == 0 { 12 } else { h % 12 };
        out.extend(two(h12, false));
    } else {
        out.extend(two(h, true));
    }
    for v in [mi, s] {
        out.extend_from_slice(&n.s_time);
        out.extend(two(v, true));
    }
    if n.time == 0 {
        out.push(b' ');
        out.extend_from_slice(if h < 12 { &n.s1159 } else { &n.s2359 });
    }
    out
}

/// The date as the conventions show it.
pub fn format_date(n: &National, y: u32, mo: u32, d: u32) -> Vec<u8> {
    let (a, b, c) = match n.date {
        1 => (two(d, true), two(mo, true), alloc::format!("{y:04}").into_bytes()),
        2 => (alloc::format!("{y:04}").into_bytes(), two(mo, true), two(d, true)),
        _ => (two(mo, true), two(d, true), alloc::format!("{y:04}").into_bytes()),
    };
    [a, n.s_date.clone(), b, n.s_date.clone(), c].concat()
}

/// A number (the integer part, and the fraction in `digits` places) as
/// the conventions show it.
pub fn format_number(n: &National, int: u64, frac: u64, digits: u32) -> Vec<u8> {
    let s = alloc::format!("{int}").into_bytes();
    let mut out = Vec::new();
    if !(int == 0 && n.lzero == 0 && digits > 0) {
        for (i, &c) in s.iter().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.extend_from_slice(&n.s_thousand);
            }
            out.push(c);
        }
    }
    if digits > 0 {
        out.extend_from_slice(&n.s_decimal);
        out.extend(alloc::format!("{:0width$}", frac, width = digits as usize).into_bytes());
    }
    out
}

pub fn format_money(n: &National, int: u64, frac: u64) -> Vec<u8> {
    let v = format_number(&National { lzero: 1, ..n.clone() }, int, frac, n.digits);
    let sym = n.s_currency.clone();
    match n.currency {
        1 => [v, sym].concat(),
        2 => [sym, b" ".to_vec(), v].concat(),
        3 => [v, b" ".to_vec(), sym].concat(),
        _ => [sym, v].concat(),
    }
}

// ---- The notebook ----

/// The conventions when the notebook opened (Undo).
static mut UNDO: Option<National> = None;
/// The open pages, to show a change made on another.
static mut PAGES: Vec<(u32, fn(u32))> = Vec::new();
/// Pages being filled: their controls' notifications are not changes.
static mut FILLING: u32 = 0;

fn opened(h: u32, fill: fn(u32)) {
    unsafe {
        if PAGES.is_empty() {
            UNDO = Some(load());
        }
        PAGES.push((h, fill));
    }
    fill_page(h, fill);
}

fn closed(h: u32) {
    unsafe { PAGES.retain(|&(p, _)| p != h) };
}

fn fill_page(h: u32, fill: fn(u32)) {
    unsafe { FILLING += 1 };
    fill(h);
    unsafe { FILLING -= 1 };
}

fn filling() -> bool {
    unsafe { FILLING != 0 }
}

/// Keep a change and show it on the other pages.
fn changed(from: u32, n: &National) {
    store(n);
    let pages: Vec<(u32, fn(u32))> = unsafe { PAGES.clone() };
    for (h, fill) in pages {
        if h != from {
            fill_page(h, fill);
        }
    }
    // Its own samples.
    refresh_samples(from);
}

fn refresh_samples(h: u32) {
    let n = load();
    let dt = now();
    set_item_text(h, ID_TIMESAMPLE as u32, &format_time(&n, dt.hours as u32, dt.minutes as u32, dt.seconds as u32));
    set_item_text(h, ID_DATESAMPLE as u32, &format_date(&n, dt.year as u32, dt.month as u32, dt.day as u32));
    set_item_text(h, ID_NUMSAMPLE as u32, &format_number(&n, 1234567, 89, n.digits.min(2)));
    set_item_text(h, ID_MONEYSAMPLE as u32, &format_money(&n, 1234, 56 % 10u64.pow(n.digits.min(2))));
}

fn now() -> os2::DateTime {
    let mut dt = os2::DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    dt
}

/// Undo and Default (every page has them).
fn undo_or_default(h: u32, id: u16) {
    let n = if id == ID_UNDO {
        unsafe { UNDO.clone() }.unwrap_or_else(load)
    } else {
        of_country(load().country)
    };
    store(&n);
    let pages: Vec<(u32, fn(u32))> = unsafe { PAGES.clone() };
    for (p, fill) in pages {
        fill_page(p, fill);
    }
    let _ = h;
}

/// The page procedures' common part.
fn page_proc(h: u32, msg: u32, a: u32, b: u32, fill: fn(u32), control: fn(u32, u16, u32)) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            opened(h, fill);
            0
        }
        WM_CONTROL => {
            if !filling() {
                control(h, (a & 0xffff) as u16, a >> 16);
            }
            0
        }
        WM_COMMAND => {
            let id = (a & 0xffff) as u16;
            if id == ID_UNDO || id == ID_DEFAULT {
                undo_or_default(h, id);
            }
            0
        }
        WM_DESTROY => {
            closed(h);
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

const EN_CHANGE: u32 = 0x0004;

/// An entry field's text, at most `max` characters.
fn field(h: u32, id: u16, max: usize) -> Vec<u8> {
    let mut t = item_text(h, id as u32);
    t.truncate(max);
    t
}

// Country page.
const ID_COUNTRIES: u16 = 1135;
const ID_COUNTRYCODE: u16 = 1136;

fn country_fill(h: u32) {
    let n = load();
    let lb = id_window(h, ID_COUNTRIES);
    if unsafe { WinSendMsg(lb, LM_QUERYITEMCOUNT, 0, 0) } == 0 {
        for &(_, name) in NAMES {
            let z = crate::util::z(name);
            unsafe { WinSendMsg(lb, LM_INSERTITEM, LIT_END, z.as_ptr() as u32) };
        }
    }
    if let Some(i) = NAMES.iter().position(|&(c, _)| c as u32 == n.country) {
        unsafe { WinSendMsg(lb, LM_SELECTITEM, i as u32, 1) };
    }
    set_item_text(h, ID_COUNTRYCODE as u32, &alloc::format!("Country code: {}", n.country).into_bytes());
    refresh_samples(h);
}

fn id_window(h: u32, id: u16) -> u32 {
    unsafe { WinWindowFromID(h, id as u32) }
}

fn country_control(h: u32, id: u16, code: u32) {
    if id != ID_COUNTRIES || code != LN_SELECT {
        return;
    }
    let i = unsafe { WinSendMsg(id_window(h, ID_COUNTRIES), LM_QUERYSELECTION, LIT_NONE, 0) };
    let Some(&(c, _)) = NAMES.get(i as usize) else { return };
    if c as u32 == load().country {
        return;
    }
    // A country brings its conventions (IBM's SetCountryData).
    let n = of_country(c as u32);
    changed(h, &n);
    set_item_text(h, ID_COUNTRYCODE as u32, &alloc::format!("Country code: {c}").into_bytes());
}

extern "C" fn country_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    page_proc(h, msg, a, b, country_fill, country_control)
}

extern "C" fn wpAddCountryPage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        book::label(b"Country:", (6, 108, 120)),
        listbox((6, 40, 124, 66), ID_COUNTRIES),
        text(b"", (6, 28, 124), ID_COUNTRYCODE),
        group(b"Samples", (136, 40, 96, 74)),
        text(b"", (140, 94, 88), ID_TIMESAMPLE),
        text(b"", (140, 80, 88), ID_DATESAMPLE),
        text(b"", (140, 66, 88), ID_NUMSAMPLE),
        text(b"", (140, 52, 88), ID_MONEYSAMPLE),
    ];
    items.extend(undo_default(2));
    add_page(o, nb, country_proc, items, b"~Country\0")
}

// Time page.
const ID_12H: u16 = 1140;
const ID_24H: u16 = 1141;
const ID_TIMESEP: u16 = 1142;
const ID_AM: u16 = 1143;
const ID_PM: u16 = 1144;
const ID_TIMESAMPLE: u16 = 1145;

fn time_fill(h: u32) {
    let n = load();
    check(h, if n.time == 0 { ID_12H } else { ID_24H }, true);
    set_item_text(h, ID_TIMESEP as u32, &n.s_time);
    set_item_text(h, ID_AM as u32, &n.s1159);
    set_item_text(h, ID_PM as u32, &n.s2359);
    crate::syssetup::enable(h, ID_AM, n.time == 0);
    crate::syssetup::enable(h, ID_PM, n.time == 0);
    refresh_samples(h);
}

fn time_control(h: u32, id: u16, code: u32) {
    let mut n = load();
    match (id, code) {
        (ID_12H | ID_24H, 1) => {
            n.time = checked(h, ID_24H) as u32;
            if n.time == 0 && n.s1159.is_empty() {
                n.s1159 = b"AM".to_vec();
                n.s2359 = b"PM".to_vec();
                set_item_text(h, ID_AM as u32, &n.s1159);
                set_item_text(h, ID_PM as u32, &n.s2359);
            }
            crate::syssetup::enable(h, ID_AM, n.time == 0);
            crate::syssetup::enable(h, ID_PM, n.time == 0);
        }
        (ID_TIMESEP, EN_CHANGE) => n.s_time = field(h, ID_TIMESEP, 1),
        (ID_AM, EN_CHANGE) => n.s1159 = field(h, ID_AM, 3),
        (ID_PM, EN_CHANGE) => n.s2359 = field(h, ID_PM, 3),
        _ => return,
    }
    changed(h, &n);
}

extern "C" fn time_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    page_proc(h, msg, a, b, time_fill, time_control)
}

extern "C" fn wpAddCountryTimePage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        group(b"Clock", (4, 72, 110, 44)),
        radio(b"~12-hour clock", (10, 94, 100), ID_12H, true),
        radio(b"~24-hour clock", (10, 80, 100), ID_24H, false),
        book::label(b"Time separator:", (122, 100, 70)),
        entry((196, 100, 16), ID_TIMESEP),
        book::label(b"Morning (AM):", (122, 86, 70)),
        entry((196, 86, 28), ID_AM),
        book::label(b"Afternoon (PM):", (122, 72, 70)),
        entry((196, 72, 28), ID_PM),
        group(b"Sample", (4, 26, 228, 36)),
        text(b"", (10, 36, 216), ID_TIMESAMPLE),
    ];
    items.extend(undo_default(2));
    add_page(o, nb, time_proc, items, b"~Time\0")
}

// Date page.
const ID_MDY: u16 = 1150;
const ID_DMY: u16 = 1151;
const ID_YMD: u16 = 1152;
const ID_DATESEP: u16 = 1153;
const ID_DATESAMPLE: u16 = 1154;

fn date_fill(h: u32) {
    let n = load();
    check(h, [ID_MDY, ID_DMY, ID_YMD][n.date.min(2) as usize], true);
    set_item_text(h, ID_DATESEP as u32, &n.s_date);
    refresh_samples(h);
}

fn date_control(h: u32, id: u16, code: u32) {
    let mut n = load();
    match (id, code) {
        (ID_MDY | ID_DMY | ID_YMD, 1) => n.date = [ID_MDY, ID_DMY, ID_YMD].iter().position(|&i| checked(h, i)).unwrap_or(0) as u32,
        (ID_DATESEP, EN_CHANGE) => n.s_date = field(h, ID_DATESEP, 1),
        _ => return,
    }
    changed(h, &n);
}

extern "C" fn date_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    page_proc(h, msg, a, b, date_fill, date_control)
}

extern "C" fn wpAddCountryDatePage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        group(b"Order", (4, 58, 130, 58)),
        radio(b"~Month-Day-Year", (10, 94, 120), ID_MDY, true),
        radio(b"~Day-Month-Year", (10, 80, 120), ID_DMY, false),
        radio(b"~Year-Month-Day", (10, 66, 120), ID_YMD, false),
        book::label(b"Date separator:", (140, 100, 70)),
        entry((212, 100, 16), ID_DATESEP),
        group(b"Sample", (4, 26, 228, 26)),
        text(b"", (10, 32, 216), ID_DATESAMPLE),
    ];
    items.extend(undo_default(2));
    add_page(o, nb, date_proc, items, b"~Date\0")
}

// Numbers page.
const ID_THOUSAND: u16 = 1160;
const ID_DECIMAL: u16 = 1161;
const ID_DIGITS: u16 = 1162;
const ID_LIST: u16 = 1163;
const ID_LZERO: u16 = 1164;
const ID_CURRENCY: u16 = 1165;
const ID_CURPOS: u16 = 1166;
const ID_NUMSAMPLE: u16 = 1170;
const ID_MONEYSAMPLE: u16 = 1171;

fn numbers_fill(h: u32) {
    let n = load();
    set_item_text(h, ID_THOUSAND as u32, &n.s_thousand);
    set_item_text(h, ID_DECIMAL as u32, &n.s_decimal);
    set_item_text(h, ID_LIST as u32, &n.s_list);
    set_item_text(h, ID_CURRENCY as u32, &n.s_currency);
    spin_init(h, ID_DIGITS, 0, 9, n.digits as i32);
    check(h, ID_LZERO, n.lzero != 0);
    check(h, ID_CURPOS + n.currency.min(3) as u16, true);
    refresh_samples(h);
}

fn numbers_control(h: u32, id: u16, code: u32) {
    let mut n = load();
    match (id, code) {
        (ID_THOUSAND, EN_CHANGE) => n.s_thousand = field(h, ID_THOUSAND, 1),
        (ID_DECIMAL, EN_CHANGE) => n.s_decimal = field(h, ID_DECIMAL, 1),
        (ID_LIST, EN_CHANGE) => n.s_list = field(h, ID_LIST, 1),
        (ID_CURRENCY, EN_CHANGE) => n.s_currency = field(h, ID_CURRENCY, 4),
        (ID_DIGITS, SPBN_CHANGE) => match spin_value(h, ID_DIGITS).filter(|v| (0..=9).contains(v)) {
            Some(v) => n.digits = v as u32,
            None => return,
        },
        (ID_LZERO, 1) => n.lzero = checked(h, ID_LZERO) as u32,
        (i, 1) if (ID_CURPOS..ID_CURPOS + 4).contains(&i) => n.currency = (0..4u16).find(|&k| checked(h, ID_CURPOS + k)).unwrap_or(0) as u32,
        _ => return,
    }
    changed(h, &n);
}

extern "C" fn numbers_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    page_proc(h, msg, a, b, numbers_fill, numbers_control)
}

extern "C" fn wpAddCountryNumbersPage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        book::label(b"Thousands separator:", (6, 106, 90)),
        entry((100, 106, 16), ID_THOUSAND),
        book::label(b"Decimal separator:", (6, 92, 90)),
        entry((100, 92, 16), ID_DECIMAL),
        book::label(b"Decimal places:", (6, 78, 90)),
        spin((100, 76, 30), ID_DIGITS),
        book::label(b"List separator:", (6, 64, 90)),
        entry((100, 64, 16), ID_LIST),
        crate::syssetup::checkbox(b"~Leading zero (0.5)", (6, 50, 120), ID_LZERO),
        group(b"Currency", (132, 40, 100, 76)),
        book::label(b"Symbol:", (136, 98, 40)),
        entry((180, 98, 40), ID_CURRENCY),
        radio(b"Before (~$1)", (136, 84, 92), ID_CURPOS, true),
        radio(b"After (1~$)", (136, 72, 92), ID_CURPOS + 1, false),
        radio(b"Before, ~spaced ($ 1)", (136, 60, 92), ID_CURPOS + 2, false),
        radio(b"After, s~paced (1 $)", (136, 48, 92), ID_CURPOS + 3, false),
        text(b"", (6, 34, 120), ID_NUMSAMPLE),
        text(b"", (6, 22, 120), ID_MONEYSAMPLE),
    ];
    items.extend(undo_default(2));
    add_page(o, nb, numbers_proc, items, b"~Numbers\0")
}

/// IBM's order of the tabs: Country, Time, Date, Numbers (each inserted
/// in front of those before it).
extern "C" fn country_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPCountry"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPCountry", m::WPCountry::wpAddCountryNumbersPage, &[o, nb as usize]);
    wps::call("WPCountry", m::WPCountry::wpAddCountryDatePage, &[o, nb as usize]);
    wps::call("WPCountry", m::WPCountry::wpAddCountryTimePage, &[o, nb as usize]);
    wps::call("WPCountry", m::WPCountry::wpAddCountryPage, &[o, nb as usize]);
    1
}

methods! {
    __m_wpAddCountryPage => wpAddCountryPage, 2;
    __m_wpAddCountryTimePage => wpAddCountryTimePage, 2;
    __m_wpAddCountryDatePage => wpAddCountryDatePage, 2;
    __m_wpAddCountryNumbersPage => wpAddCountryNumbersPage, 2;
    __m_country_add_pages => country_add_pages, 2;
}

pub fn impl_country() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![
            ("wpAddCountryPage", __m_wpAddCountryPage as usize),
            ("wpAddCountryTimePage", __m_wpAddCountryTimePage as usize),
            ("wpAddCountryDatePage", __m_wpAddCountryDatePage as usize),
            ("wpAddCountryNumbersPage", __m_wpAddCountryNumbersPage as usize),
        ],
        overrides: alloc::vec![("wpAddSettingsPages", __m_country_add_pages as usize), crate::pages::no_window_page_override()],
    }
}
