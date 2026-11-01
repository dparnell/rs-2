//! Keys that map their values, as in IBM's REGISTRY (its rbmap.c): a key
//! whose only values are OS2_INI_FILE_NAME, OS2_INI_APP_NAME and
//! OS2_INI_ACCESS_METHOD (REG_SZ; names compared without case) shows
//! other data as its values, read only:
//!
//! - "profile": the keys of application OS2_INI_APP_NAME in the OS/2
//!   profile OS2_INI_FILE_NAME, as REG_BINARY;
//! - "profile_sz": the same as REG_SZ (IBM's REGCONV maps Windows' font
//!   list, SOFTWARE\MICROSOFT\WINDOWS\CURRENTVERSION\FONTS, to OS2.INI's
//!   PM_Fonts this way);
//! - "uls": the items of the process's locale (LIBUNI), named as in
//!   ulsitem.h without LOCI_ (sShortDate, iCountry...), as REG_SZ
//!   (REGCONV's CONTROL PANEL\INTERNATIONAL);
//! - "spooler_key" and "spooler_value": the print queues (spool.rs).
//!
//! Errors as IBM's: a profile that cannot be opened or a key not in it
//! ERROR_INVALID_NAME, a missing file or application value
//! ERROR_FILE_NOT_FOUND, no access method ERROR_GEN_FAILURE.

use alloc::vec::Vec;

use crate::{ERROR_FILE_NOT_FOUND, ERROR_GEN_FAILURE, ERROR_INVALID_NAME, ERROR_NO_MORE_ITEMS, REG_BINARY, REG_SZ, z};

unsafe extern "C" {
    #[link_name = "PMSHAPI.102"]
    fn PrfOpenProfile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMSHAPI.103"]
    fn PrfCloseProfile(hini: u32) -> u32;
    #[link_name = "PMSHAPI.107"]
    fn PrfQueryProfile(hab: u32, prf: *mut u32) -> u32;
    #[link_name = "LIBUNI.20"]
    fn UniCreateLocaleObject(ty: i32, spec: *const u16, out: *mut u32) -> i32;
    #[link_name = "LIBUNI.21"]
    fn UniFreeLocaleObject(lo: u32) -> i32;
    #[link_name = "LIBUNI.22"]
    fn UniFreeMem(p: *mut u16) -> i32;
    #[link_name = "LIBUNI.30"]
    fn UniQueryLocaleItem(lo: u32, item: i32, out: *mut *mut u16) -> i32;
}

pub const FILE_NAME: &[u8] = b"OS2_INI_FILE_NAME";
pub const APP_NAME: &[u8] = b"OS2_INI_APP_NAME";
pub const ACCESS_METHOD: &[u8] = b"OS2_INI_ACCESS_METHOD";

#[derive(Clone, Copy, PartialEq)]
pub enum Method {
    Profile,
    ProfileSz,
    Uls,
    SpoolerValue,
    SpoolerKey,
}

/// What a key maps, from its OS2_INI_* values.
pub struct Map {
    file: Option<Vec<u8>>,
    app: Option<Vec<u8>>,
    pub method: Option<Method>,
    /// The key's name (spooler_value: the printer's).
    pub name: Vec<u8>,
}

pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn is_map_name(n: &[u8]) -> bool {
    same(n, FILE_NAME) || same(n, APP_NAME) || same(n, ACCESS_METHOD)
}

/// A string value's data without its NUL(s).
fn text(d: &[u8]) -> &[u8] {
    let n = d.iter().position(|&c| c == 0).unwrap_or(d.len());
    &d[..n]
}

/// The access method an OS2_INI_ACCESS_METHOD value names.
pub fn method(d: &[u8]) -> Option<Method> {
    let t = text(d);
    [(&b"profile"[..], Method::Profile), (b"profile_sz", Method::ProfileSz), (b"uls", Method::Uls), (b"spooler_value", Method::SpoolerValue), (b"spooler_key", Method::SpoolerKey)]
        .iter()
        .find(|m| same(t, m.0))
        .map(|m| m.1)
}

/// The mapping of a key with these stored values, if it is one (it has
/// an OS2_INI_* REG_SZ value). `name`: the key's name.
pub fn mapping(vals: &[(Vec<u8>, u32, Vec<u8>)], name: &[u8]) -> Option<Map> {
    let mut m = Map { file: None, app: None, method: None, name: name.to_vec() };
    let mut any = false;
    for (n, t, d) in vals {
        if *t != REG_SZ {
            continue;
        }
        if same(n, FILE_NAME) {
            m.file = Some(text(d).to_vec());
        } else if same(n, APP_NAME) {
            m.app = Some(text(d).to_vec());
        } else if same(n, ACCESS_METHOD) {
            m.method = method(d);
        } else {
            continue;
        }
        any = true;
    }
    any.then_some(m)
}

impl Map {
    /// Whether the key's values come from elsewhere (a spooler_key key
    /// keeps its own values; its subkeys are the queues).
    pub fn maps_values(&self) -> bool {
        self.method != Some(Method::SpoolerKey)
    }

    /// A value by name: (type, data).
    pub fn get(&self, name: &[u8]) -> Result<(u32, Vec<u8>), u32> {
        match self.method {
            Some(Method::Profile | Method::ProfileSz) => {
                let (file, app) = self.profile()?;
                let p = Profile::open(file)?;
                let d = crate::get(p.hini, app, name).ok_or(ERROR_INVALID_NAME)?;
                Ok(self.profile_value(d))
            }
            Some(Method::Uls) => {
                let i = LOCI.iter().position(|n| same(n.as_bytes(), name)).ok_or(ERROR_FILE_NOT_FOUND)?;
                Ok((REG_SZ, uls_item(i)?))
            }
            Some(Method::SpoolerValue) => crate::spool::value(&self.name, name),
            _ => Err(ERROR_GEN_FAILURE),
        }
    }

    /// The i-th value: (name, type, data).
    pub fn ith(&self, i: u32) -> Result<(Vec<u8>, u32, Vec<u8>), u32> {
        match self.method {
            Some(Method::Profile | Method::ProfileSz) => {
                let (file, app) = self.profile()?;
                let p = Profile::open(file)?;
                let a = z(app);
                let keys = crate::names(p.hini, a.as_ptr());
                if keys.is_empty() {
                    return Err(ERROR_INVALID_NAME);
                }
                let k = keys.into_iter().nth(i as usize).ok_or(ERROR_NO_MORE_ITEMS)?;
                let d = crate::get(p.hini, app, &k).ok_or(ERROR_GEN_FAILURE)?;
                let (t, d) = self.profile_value(d);
                Ok((k, t, d))
            }
            Some(Method::Uls) => {
                let n = LOCI.get(i as usize).ok_or(ERROR_NO_MORE_ITEMS)?;
                Ok((n.as_bytes().to_vec(), REG_SZ, uls_item(i as usize)?))
            }
            Some(Method::SpoolerValue) => crate::spool::ith(&self.name, i),
            _ => Err(ERROR_GEN_FAILURE),
        }
    }

    fn profile(&self) -> Result<(&[u8], &[u8]), u32> {
        match (&self.file, &self.app) {
            (Some(f), Some(a)) => Ok((f, a)),
            _ => Err(ERROR_FILE_NOT_FOUND),
        }
    }

    fn profile_value(&self, mut d: Vec<u8>) -> (u32, Vec<u8>) {
        if self.method == Some(Method::Profile) {
            return (REG_BINARY, d);
        }
        if d.last() != Some(&0) {
            d.push(0);
        }
        (REG_SZ, d)
    }
}

/// An OS/2 profile opened for one call, as IBM's REGISTRY does. The
/// user and system profiles (IBM's REGCONV maps to OS2.INI) are not
/// opened again: their handles are used.
struct Profile {
    hini: u32,
    close: bool,
}

impl Profile {
    fn open(file: &[u8]) -> Result<Profile, u32> {
        let mut user = [0u8; 260];
        let mut system = [0u8; 260];
        let mut prf = [user.len() as u32, user.as_mut_ptr() as u32, system.len() as u32, system.as_mut_ptr() as u32];
        if unsafe { PrfQueryProfile(0, prf.as_mut_ptr()) } != 0 {
            for (name, hini) in [(&user, 0xffff_ffffu32), (&system, 0xffff_fffe)] {
                if same(text(name), file) {
                    return Ok(Profile { hini, close: false });
                }
            }
        }
        let f = z(file);
        match unsafe { PrfOpenProfile(0, f.as_ptr()) } {
            0 => Err(ERROR_INVALID_NAME),
            hini => Ok(Profile { hini, close: true }),
        }
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        if self.close {
            unsafe { PrfCloseProfile(self.hini) };
        }
    }
}

/// The locale items, LOCI_sDateTime (1) to LOCI_wLongDate (125), as the
/// "uls" method names them.
const LOCI: [&str; 125] = [
    "sDateTime", "sShortDate", "sTimeFormat", "s1159", "s2359", "sAbbrevDayName7", "sAbbrevDayName1", "sAbbrevDayName2",
    "sAbbrevDayName3", "sAbbrevDayName4", "sAbbrevDayName5", "sAbbrevDayName6", "sDayName7", "sDayName1", "sDayName2",
    "sDayName3", "sDayName4", "sDayName5", "sDayName6", "sAbbrevMonthName1", "sAbbrevMonthName2", "sAbbrevMonthName3",
    "sAbbrevMonthName4", "sAbbrevMonthName5", "sAbbrevMonthName6", "sAbbrevMonthName7", "sAbbrevMonthName8",
    "sAbbrevMonthName9", "sAbbrevMonthName10", "sAbbrevMonthName11", "sAbbrevMonthName12", "sMonthName1", "sMonthName2",
    "sMonthName3", "sMonthName4", "sMonthName5", "sMonthName6", "sMonthName7", "sMonthName8", "sMonthName9",
    "sMonthName10", "sMonthName11", "sMonthName12", "sDecimal", "sThousand", "sYesString", "sNoString", "sCurrency",
    "sCodeSet", "xLocaleToken", "xWinLocale", "iLocaleResnum", "sNativeDigits", "iMaxItem", "sTimeMark", "sEra",
    "sAltShortDate", "sAltDateTime", "sAltTimeFormat", "sAltDigits", "sYesExpr", "sNoExpr", "sDate", "sTime", "sList",
    "sMonDecimalSep", "sMonThousandSep", "sGrouping", "sMonGrouping", "iMeasure", "iPaper", "iDigits", "iTime", "iDate",
    "iCurrency", "iCurrDigits", "iLzero", "iNegNumber", "iLDate", "iCalendarType", "iFirstDayOfWeek", "iFirstWeekOfYear",
    "iNegCurr", "iTLzero", "iTimePrefix", "iOptionalCalendar", "sIntlSymbol", "sAbbrevLangName", "sCollate", "iUpperType",
    "iUpperMissing", "sPositiveSign", "sNegativeSign", "sLeftNegative", "sRightNegative", "sLongDate", "sAltLongDate",
    "sMonthName13", "sAbbrevMonthName13", "sName", "sLanguageID", "sCountryID", "sEngLanguage", "sLanguage", "sEngCountry",
    "sCountry", "sNativeCtryName", "iCountry", "sISOCodepage", "iAnsiCodepage", "iCodepage", "iAltCodepage",
    "iMacCodepage", "iEbcdicCodepage", "sOtherCodepages", "sSetCodepage", "sKeyboard", "sAltKeyboard", "sSetKeyboard",
    "sDebit", "sCredit", "sLatin1Locale", "wTimeFormat", "wShortDate", "wLongDate",
];

/// Locale item i + 1 of the default locale in the process code page,
/// with its NUL.
fn uls_item(i: usize) -> Result<Vec<u8>, u32> {
    let mut lo = 0;
    // UNI_UCS_STRING_POINTER, "": the default locale.
    if unsafe { UniCreateLocaleObject(3, [0u16].as_ptr(), &mut lo) } != 0 {
        return Err(ERROR_GEN_FAILURE);
    }
    let mut p = core::ptr::null_mut();
    let rc = unsafe { UniQueryLocaleItem(lo, i as i32 + 1, &mut p) };
    let r = if rc != 0 || p.is_null() {
        Err(ERROR_GEN_FAILURE)
    } else {
        let mut v = crate::wide::to_cp(&crate::wide::wstr(p));
        v.push(0);
        unsafe { UniFreeMem(p) };
        Ok(v)
    };
    unsafe { UniFreeLocaleObject(lo) };
    r
}
