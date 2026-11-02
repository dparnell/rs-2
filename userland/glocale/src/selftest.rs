//! `GLOCALE /SELFTEST`: checks of the samples, the format strings and
//! what the pages do to a user locale (through LIBUNI, without windows),
//! of USER.LOC as written and of Mark as default. Prints PASS/FAIL lines
//! (also on the serial port) and "glocale selftest done: N failure(s)".

use alloc::vec::Vec;

use crate::book;
use crate::fmt;
use crate::uni::{self, Lo};

static mut FAILS: u32 = 0;

fn check(name: &str, ok: bool) {
    let line = alloc::format!("{} {name}", if ok { "PASS" } else { "FAIL" });
    os2::println!("{line}");
    crate::trace(line.as_bytes());
    if !ok {
        unsafe { FAILS += 1 };
    }
}

fn eq(name: &str, got: &[u8], want: &[u8]) {
    let ok = got == want;
    if !ok {
        os2::println!("  got {:?}, want {:?}", core::str::from_utf8(got), core::str::from_utf8(want));
    }
    check(name, ok);
}

fn user_loc() -> Vec<u8> {
    let p = alloc::format!("{}:\\LANGUAGE\\LOCALE\\USER.LOC", pmkit::boot_drive() as char);
    pmkit::read_file(p.as_bytes()).unwrap_or_default()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

pub fn run() -> u32 {
    // Samples and format strings.
    let n = fmt::number(b".", b",", 2);
    eq("number sample", &n, b"1,000.11");
    eq("number sample, no digits", &fmt::number(b",", b".", 0), b"1.000");
    eq("negative number (1.1)", &fmt::neg_number(0, b"-", &n), b"(1,000.11)");
    eq("negative number 1.1 -", &fmt::neg_number(4, b"-", &n), b"1,000.11 -");
    eq("currency $1", &fmt::currency(0, b"$", &n), b"$1,000.11");
    eq("currency 1 $", &fmt::currency(3, b"EUR", &n), b"1,000.11 EUR");
    eq("negative currency ($1)", &fmt::neg_currency(0, b"$", b"-", &n), b"($1,000.11)");
    eq("negative currency -1 $", &fmt::neg_currency(8, b"$", b"-", &n), b"-1,000.11 $");
    eq("date format DMY", &fmt::date_format(1, b"."), b"%d.%m.%y");
    eq("date format YDM", &fmt::date_format(3, b"-"), b"%y-%d-%m");
    check("date order of %y/%m/%d", fmt::date_order(b"%y/%m/%d") == Some(2));
    check("date order of %d.%m.%Y", fmt::date_order(b"%d.%m.%Y") == Some(1));
    eq("time format 12 hours", &fmt::time_format(false, b":"), b"%I:%M:%S %p");
    eq("separator replaced", &fmt::replace_separator(b"%I:%M:%S %p", b"."), b"%I.%M.%S %p");
    eq("date separator replaced", &fmt::replace_separator(b"%m/%d/%y", b"-"), b"%m-%d-%y");

    // A user locale on de_DE, changed as the pages change it.
    let name = "rs2_glotest";
    uni::delete_user(name);
    check("make user locale", uni::make_user(name, "de_DE", false) == 0);
    check("user locale listed", uni::locale_list(2).unwrap_or_default().iter().any(|n| n == name));
    book::set_current(name);
    check("date order YMD", book::set_date_order(2) == 0);
    check("12-hour clock", book::set_clock(false) == 0);
    check("currency symbol", uni::set_text(name, uni::S_CURRENCY, &uni::from_cp(b"XYZ")) == 0);
    check("measure U.S.", uni::set_value(name, uni::I_MEASURE, 1) == 0);
    match Lo::new(name) {
        Some(lo) => {
            check("iDate 2", lo.value(uni::I_DATE) == 2);
            eq("sShortDate", &lo.text(uni::S_SHORTDATE), b"%y.%m.%d");
            let sep = Lo::new("de_DE").map(|b| b.text(uni::S_TIME)).unwrap_or_default();
            eq("sTimeFormat", &lo.text(uni::S_TIMEFORMAT), &fmt::time_format(false, &sep));
            check("iTime 0", lo.value(uni::I_TIME) == 0);
            eq("sCurrency", &lo.text(uni::S_CURRENCY), b"XYZ");
            eq("sName is the base", &lo.text(uni::S_NAME), b"de_DE");
            let tm = uni::Tm { sec: 5, min: 4, hour: 15, mday: 2, mon: 10, year: 126, wday: 1, yday: 305, isdst: 0 };
            let d = lo.strftime(&uni::wz("%x")[..2], &tm).map(|s| uni::to_cp(&s)).unwrap_or_default();
            eq("date sample", &d, b"26.11.02");
            let t = lo.strftime(&uni::wz("%X")[..2], &tm).map(|s| uni::to_cp(&s)).unwrap_or_default();
            check("time sample", t.starts_with(b"03") && contains(&t, b"04") && contains(&t, b"05"));
        }
        None => check("user locale object", false),
    }
    check("USER.LOC written", uni::complete() == 0);
    let text = user_loc();
    check("USER.LOC has the locale", contains(&text, b"[locale rs2_glotest de_DE]"));
    check("USER.LOC has sCurrency", contains(&text, b"sCurrency") && contains(&text, b"\"XYZ\""));
    check("USER.LOC has iDate", contains(&text, b"iDate"));

    // Default: on its base again.
    check("default", uni::make_user(name, "de_DE", true) == 0);
    if let Some(lo) = Lo::new(name) {
        eq("currency back to the base's", &lo.text(uni::S_CURRENCY), &Lo::new("de_DE").map(|b| b.text(uni::S_CURRENCY)).unwrap_or_default());
    }
    // An override of a system locale keeps the name.
    check("system locale name refused as a new base", uni::make_user("fr_FR", "de_DE", false) == uni::ULS_BADOBJECT);

    // Mark as default: OS2.INI's national settings.
    let cur = crate::current_locale();
    check("mark as default", crate::mark_default(&cur));
    let mut buf = [0u8; 16];
    let n = unsafe { pmkit::PrfQueryProfileString(pmkit::HINI_USERPROFILE, b"PM_National\0".as_ptr(), b"sDecimal\0".as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), 16) };
    let want = Lo::new(&cur).map(|l| l.text(uni::S_DECIMAL)).unwrap_or_default();
    eq("PM_National sDecimal", pmkit::until_nul(&buf[..n as usize]), &want);
    let p = alloc::format!("{}:\\LANGUAGE\\LOCALE\\DEFAULT.LOC", pmkit::boot_drive() as char);
    check("DEFAULT.LOC written", pmkit::read_file(p.as_bytes()).is_some_and(|t| contains(&t, b"sDecimal")));

    // Delete.
    check("delete", uni::delete_user(name) == 0 && uni::complete() == 0);
    check("USER.LOC without it", !contains(&user_loc(), b"rs2_glotest"));
    let fails = unsafe { FAILS };
    let line = alloc::format!("glocale selftest done: {fails} failure(s)");
    os2::println!("{line}");
    crate::trace(line.as_bytes());
    fails
}
