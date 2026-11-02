//! The LIBUNI and UCONV calls the Locale program uses (Unicode locale
//! objects, items, user locales, strftime) and conversions between the
//! process code page and UCS-2.

use alloc::string::String;
use alloc::vec::Vec;

/// struct tm as UniStrftime takes it.
#[repr(C)]
#[derive(Default, Clone, Copy)]
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

unsafe extern "C" {
    #[link_name = "LIBUNI.20"]
    fn UniCreateLocaleObject(ty: i32, spec: *const u8, out: *mut u32) -> i32;
    #[link_name = "LIBUNI.21"]
    fn UniFreeLocaleObject(lo: u32) -> i32;
    #[link_name = "LIBUNI.19"]
    fn UniQueryLocaleObject(lo: u32, cat: i32, ty: i32, out: *mut *mut u16) -> i32;
    #[link_name = "LIBUNI.22"]
    fn UniFreeMem(p: *mut u16) -> i32;
    #[link_name = "LIBUNI.30"]
    fn UniQueryLocaleItem(lo: u32, item: i32, out: *mut *mut u16) -> i32;
    #[link_name = "LIBUNI.156"]
    fn UniQueryLocaleValue(lo: u32, item: i32, out: *mut i32) -> i32;
    #[link_name = "LIBUNI.39"]
    fn UniStrftime(lo: u32, buf: *mut u16, max: u32, fmt: *const u16, tm: *const Tm) -> u32;
    #[link_name = "LIBUNI.157"]
    fn UniQueryLocaleList(flags: u32, buf: *mut u16, cch: i32) -> i32;
    #[link_name = "LIBUNI.158"]
    fn UniQueryLanguageName(lang: *const u16, iso: *const u16, out: *mut *const u16) -> i32;
    #[link_name = "LIBUNI.159"]
    fn UniQueryCountryName(ctry: *const u16, iso: *const u16, out: *mut *const u16) -> i32;
    #[link_name = "LIBUNI.152"]
    fn UniMakeUserLocale(name: *const u16, base: *const u16) -> i32;
    #[link_name = "LIBUNI.153"]
    fn UniSetUserLocaleItem(locale: *const u16, item: i32, ty: i32, value: *const u8) -> i32;
    #[link_name = "LIBUNI.154"]
    fn UniDeleteUserLocale(locale: *const u16) -> i32;
    #[link_name = "LIBUNI.155"]
    fn UniCompleteUserLocale() -> i32;
    #[link_name = "UCONV.1"]
    fn UniCreateUconvObject(name: *const u16, out: *mut u32) -> i32;
    #[link_name = "UCONV.11"]
    fn UniStrFromUcs(u: u32, target: *mut u8, source: *const u16, len: i32) -> i32;
    #[link_name = "UCONV.12"]
    fn UniStrToUcs(u: u32, target: *mut u16, source: *const u8, len: i32) -> i32;
}

pub const ULS_INVALID: i32 = 0x2040e;
pub const ULS_BADOBJECT: i32 = 0x2040f;
pub const ULS_NOMATCH: i32 = 0x20411;

/// Highest item number (LOCI_MAXITEM).
pub const MAXITEM: i32 = 136;
/// Item flag: the base (system) locale's value.
pub const LOCI_NOUSEROVERRIDE: i32 = 0x8000;

// Items (LOCI_*).
pub const S_SHORTDATE: i32 = 2;
pub const S_TIMEFORMAT: i32 = 3;
pub const S_1159: i32 = 4;
pub const S_2359: i32 = 5;
pub const S_DECIMAL: i32 = 44;
pub const S_THOUSAND: i32 = 45;
pub const S_CURRENCY: i32 = 48;
pub const S_DATE: i32 = 63;
pub const S_TIME: i32 = 64;
pub const S_LIST: i32 = 65;
pub const S_MONDECIMALSEP: i32 = 66;
pub const S_MONTHOUSANDSEP: i32 = 67;
pub const I_MEASURE: i32 = 70;
pub const I_DIGITS: i32 = 72;
pub const I_TIME: i32 = 73;
pub const I_DATE: i32 = 74;
pub const I_CURRENCY: i32 = 75;
pub const I_CURRDIGITS: i32 = 76;
pub const I_LZERO: i32 = 77;
pub const I_NEGNUMBER: i32 = 78;
pub const I_NEGCURR: i32 = 83;
pub const I_TLZERO: i32 = 84;
pub const S_COLLATE: i32 = 89;
pub const S_POSITIVESIGN: i32 = 92;
pub const S_NEGATIVESIGN: i32 = 93;
pub const S_NAME: i32 = 100;
pub const S_LANGUAGEID: i32 = 101;
pub const S_COUNTRYID: i32 = 102;
pub const I_COUNTRY: i32 = 108;

/// The names of the items as USER.LOC and the Advanced page show them.
pub static ITEM_NAMES: [&str; 136] = [
    "sDateTime", "sShortDate", "sTimeFormat", "s1159", "s2359", "sAbbrevDayName7", "sAbbrevDayName1",
    "sAbbrevDayName2", "sAbbrevDayName3", "sAbbrevDayName4", "sAbbrevDayName5", "sAbbrevDayName6",
    "sDayName7", "sDayName1", "sDayName2", "sDayName3", "sDayName4", "sDayName5", "sDayName6",
    "sAbbrevMonthName1", "sAbbrevMonthName2", "sAbbrevMonthName3", "sAbbrevMonthName4", "sAbbrevMonthName5",
    "sAbbrevMonthName6", "sAbbrevMonthName7", "sAbbrevMonthName8", "sAbbrevMonthName9", "sAbbrevMonthName10",
    "sAbbrevMonthName11", "sAbbrevMonthName12", "sMonthName1", "sMonthName2", "sMonthName3", "sMonthName4",
    "sMonthName5", "sMonthName6", "sMonthName7", "sMonthName8", "sMonthName9", "sMonthName10", "sMonthName11",
    "sMonthName12", "sDecimal", "sThousand", "sYesString", "sNoString", "sCurrency", "sCodeSet",
    "xLocaleToken", "xWinLocale", "iLocaleResnum", "sNativeDigits", "iMaxItem", "sTimeMark", "sEra",
    "sAltShortDate", "sAltDateTime", "sAltTimeFormat", "sAltDigits", "sYesExpr", "sNoExpr", "sDate", "sTime",
    "sList", "sMonDecimalSep", "sMonThousandSep", "sGrouping", "sMonGrouping", "iMeasure", "iPaper",
    "iDigits", "iTime", "iDate", "iCurrency", "iCurrDigits", "iLzero", "iNegNumber", "iLDate",
    "iCalendarType", "iFirstDayOfWeek", "iFirstWeekOfYear", "iNegCurr", "iTLzero", "iTimePrefix",
    "iOptionalCalendar", "sIntlSymbol", "sAbbrevLangName", "sCollate", "iUpperType", "iUpperMissing",
    "sPositiveSign", "sNegativeSign", "sLeftNegative", "sRightNegative", "sLongDate", "sAltLongDate",
    "sMonthName13", "sAbbrevMonthName13", "sName", "sLanguageID", "sCountryID", "sEngLanguage", "sLanguage",
    "sEngCountry", "sCountry", "sNativeCtryName", "iCountry", "sISOCodepage", "iAnsiCodepage", "iCodepage",
    "iAltCodepage", "iMacCodepage", "iEbcdicCodepage", "sOtherCodepages", "sSetCodepage", "sKeyboard",
    "sAltKeyboard", "sSetKeyboard", "sDebit", "sCredit", "sLatin1Locale", "wTimeFormat", "wShortDate",
    "wLongDate", "jISO3CountryName", "jPercentPattern", "jPercentSign", "jExponent", "jFullTimeFormat",
    "jLongTimeFormat", "jShortTimeFormat", "jFullDateFormat", "jMediumDateFormat", "jDateTimePattern",
    "jEraStrings",
];

/// UCS-2 of ASCII text, NUL-terminated.
pub fn wz(s: &str) -> Vec<u16> {
    s.bytes().map(|b| b as u16).chain(core::iter::once(0)).collect()
}

/// A NUL-terminated copy of UCS-2 text.
pub fn wzs(s: &[u16]) -> Vec<u16> {
    s.iter().copied().chain(core::iter::once(0)).collect()
}

/// UCS-2 text up to its NUL.
///
/// # Safety
/// `p` points at NUL-terminated UCS-2 (or is null).
pub unsafe fn from_ptr<'a>(p: *const u16) -> &'a [u16] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

/// ASCII of UCS-2 (locale names are ASCII).
pub fn ascii(s: &[u16]) -> String {
    s.iter().map(|&c| if c < 0x80 { c as u8 as char } else { '?' }).collect()
}

static mut CONV: u32 = 0;

/// The process code page's conversion object.
fn conv() -> u32 {
    unsafe {
        if CONV == 0 {
            let n = wz("@path=no");
            UniCreateUconvObject(n.as_ptr(), &raw mut CONV);
        }
        CONV
    }
}

/// UCS-2 text in the process code page (for the controls).
pub fn to_cp(s: &[u16]) -> Vec<u8> {
    let src = wzs(s);
    let mut out = alloc::vec![0u8; s.len() * 2 + 2];
    let u = conv();
    if u == 0 || unsafe { UniStrFromUcs(u, out.as_mut_ptr(), src.as_ptr(), out.len() as i32) } != 0 {
        return s.iter().map(|&c| if c < 0x100 { c as u8 } else { b'?' }).collect();
    }
    pmkit::until_nul(&out).to_vec()
}

/// Process code page text as UCS-2 (without a NUL).
pub fn from_cp(s: &[u8]) -> Vec<u16> {
    let src = pmkit::z(s);
    let mut out = alloc::vec![0u16; s.len() + 1];
    let u = conv();
    if u == 0 || unsafe { UniStrToUcs(u, out.as_mut_ptr(), src.as_ptr(), out.len() as i32) } != 0 {
        return s.iter().map(|&c| c as u16).collect();
    }
    unsafe { from_ptr(out.as_ptr()) }.to_vec()
}

/// A locale object.
pub struct Lo(pub u32);

impl Lo {
    /// The locale of that name (empty: the process's locale).
    pub fn new(name: &str) -> Option<Lo> {
        let n = wz(name);
        let mut lo = 0;
        let rc = unsafe { UniCreateLocaleObject(3, n.as_ptr() as *const u8, &mut lo) };
        (rc == 0 && lo != 0).then_some(Lo(lo))
    }

    /// An item's text (UCS-2).
    pub fn item(&self, item: i32) -> Vec<u16> {
        let mut p: *mut u16 = core::ptr::null_mut();
        if unsafe { UniQueryLocaleItem(self.0, item, &mut p) } != 0 || p.is_null() {
            return Vec::new();
        }
        let v = unsafe { from_ptr(p) }.to_vec();
        unsafe { UniFreeMem(p) };
        v
    }

    /// An item in the process code page.
    pub fn text(&self, item: i32) -> Vec<u8> {
        to_cp(&self.item(item))
    }

    /// A numeric item (-1 when it has none).
    pub fn value(&self, item: i32) -> i32 {
        let mut v = -1;
        if unsafe { UniQueryLocaleValue(self.0, item, &mut v) } != 0 {
            return -1;
        }
        v
    }

    /// The locale's names by category (`UniQueryLocaleObject(LC_ALL)`),
    /// the first one.
    pub fn name(&self) -> String {
        let mut p: *mut u16 = core::ptr::null_mut();
        if unsafe { UniQueryLocaleObject(self.0, -1, 3, &mut p) } != 0 || p.is_null() {
            return String::new();
        }
        let s = ascii(unsafe { from_ptr(p) });
        unsafe { UniFreeMem(p) };
        s.split(' ').next().unwrap_or("").into()
    }

    /// strftime with this locale.
    pub fn strftime(&self, fmt: &[u16], tm: &Tm) -> Option<Vec<u16>> {
        let f = wzs(fmt);
        let mut buf = alloc::vec![0u16; 256];
        let n = unsafe { UniStrftime(self.0, buf.as_mut_ptr(), buf.len() as u32, f.as_ptr(), tm) };
        if n == 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }
}

impl Drop for Lo {
    fn drop(&mut self) {
        unsafe { UniFreeLocaleObject(self.0) };
    }
}

/// The locales: 1 system, 2 user.
pub fn locale_list(flags: u32) -> Option<Vec<String>> {
    let mut buf = alloc::vec![0u16; 8192];
    if unsafe { UniQueryLocaleList(flags, buf.as_mut_ptr(), buf.len() as i32) } != 0 {
        return None;
    }
    Some(buf.split(|&c| c == 0).take_while(|s| !s.is_empty()).map(ascii).collect())
}

/// A language's name (`en`) in a language.
pub fn language_name(lang: &[u16], iso: &str) -> Vec<u8> {
    let (l, i) = (wzs(lang), wz(iso));
    let mut p: *const u16 = core::ptr::null();
    if unsafe { UniQueryLanguageName(l.as_ptr(), i.as_ptr(), &mut p) } != 0 {
        return Vec::new();
    }
    to_cp(unsafe { from_ptr(p) })
}

/// A country's name (`US`) in a language.
pub fn country_name(ctry: &[u16], iso: &str) -> Vec<u8> {
    let (c, i) = (wzs(ctry), wz(iso));
    let mut p: *const u16 = core::ptr::null();
    if unsafe { UniQueryCountryName(c.as_ptr(), i.as_ptr(), &mut p) } != 0 {
        return Vec::new();
    }
    to_cp(unsafe { from_ptr(p) })
}

/// UniMakeUserLocale; `replace` makes it over an existing user locale of
/// that name (the base's characters marked 0xFF00, as IBM's LIBUNI
/// takes it).
pub fn make_user(name: &str, base: &str, replace: bool) -> i32 {
    let n = wz(name);
    let mut b = wz(base);
    if replace {
        for c in b.iter_mut() {
            if *c != 0 {
                *c |= 0xff00;
            }
        }
    }
    unsafe { UniMakeUserLocale(n.as_ptr(), b.as_ptr()) }
}

/// UniSetUserLocaleItem with UCS-2 text.
pub fn set_text(name: &str, item: i32, v: &[u16]) -> i32 {
    let n = wz(name);
    let v = wzs(v);
    unsafe { UniSetUserLocaleItem(n.as_ptr(), item, 3, v.as_ptr() as *const u8) }
}

/// UniSetUserLocaleItem with a number.
pub fn set_value(name: &str, item: i32, v: i32) -> i32 {
    let n = wz(name);
    unsafe { UniSetUserLocaleItem(n.as_ptr(), item, 1, &v as *const i32 as *const u8) }
}

pub fn delete_user(name: &str) -> i32 {
    let n = wz(name);
    unsafe { UniDeleteUserLocale(n.as_ptr()) }
}

/// Write the user locales (USER.LOC).
pub fn complete() -> i32 {
    unsafe { UniCompleteUserLocale() }
}

/// The time now, as a struct tm.
pub fn now() -> Tm {
    let mut dt = os2::DateTime::default();
    unsafe { os2::DosGetDateTime(&mut dt) };
    let (y, m, d) = (dt.year as i32, dt.month as i32, dt.day as i32);
    // Day of the year.
    const DAYS: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let yday = DAYS[((m - 1).clamp(0, 11)) as usize] + d - 1 + (leap && m > 2) as i32;
    Tm {
        sec: dt.seconds as i32,
        min: dt.minutes as i32,
        hour: dt.hours as i32,
        mday: d,
        mon: m - 1,
        year: y - 1900,
        wday: dt.weekday as i32,
        yday,
        isdst: 0,
    }
}
