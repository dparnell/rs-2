//! LIBUNI.DLL: Unicode locales, character classification and UCS-2
//! strings. The locale data and logic are in crates/uls (rs2's own system
//! locales, user locales in `\LANGUAGE\LOCALE\USER.LOC`); this module is
//! the OS/2 face: locale objects, memory and files.
//!
//! As in IBM's LIBUNI, item strings of system locales are returned from
//! the static tables (UniFreeMem accepts and ignores them); everything
//! else is allocated and freed with UniFreeMem. User locales are read from
//! USER.LOC when a process first needs them and again when it creates a
//! locale object or lists locales while it has no unsaved changes (IBM
//! keeps them in shared memory instead).

#![no_std]
#![no_main]
#![allow(non_snake_case, non_upper_case_globals, clippy::missing_safety_doc)]
// The exports take pointers from OS/2 callers, as the API defines them.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate alloc;
extern crate os2;

mod exports;
mod table;
mod strings;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use uls::{
    LOCI_NOUSEROVERRIDE, Locale as Loc, MAXITEM, N_CATEGORIES, ULS_BADOBJECT, ULS_BUFFERFULL, ULS_INVALID,
    ULS_NOMEMORY, ULS_SUCCESS, ULS_UNSUPPORTED, UserLocales, ascii, from_ptr, ucs,
};

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.start")]
pub extern "C" fn _start(_hmod: u32, _flag: u32) -> u32 {
    1
}

// ---- process state ----------------------------------------------------------

static LOCK: AtomicBool = AtomicBool::new(false);
static mut USERS: Option<UserLocales> = None;

/// Run `f` with the process's user locales (read from USER.LOC the first
/// time, and again when `reload` and nothing is unsaved).
fn with_users<R>(reload: bool, f: impl FnOnce(&mut UserLocales) -> R) -> R {
    while LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        unsafe { os2::DosSleep(0) };
    }
    let users = unsafe { &mut *core::ptr::addr_of_mut!(USERS) };
    let fresh = match users {
        None => true,
        Some(u) => reload && !u.changed,
    };
    if fresh {
        *users = Some(UserLocales::parse(&read_user_loc().unwrap_or_default()));
    }
    let r = f(users.as_mut().unwrap());
    LOCK.store(false, Ordering::Release);
    r
}

/// `X:\LANGUAGE\LOCALE` on the boot drive, NUL-terminated, with the file
/// name appended when given.
fn user_loc_path(file: &str) -> Vec<u8> {
    let mut drive = 3u32;
    unsafe { os2::DosQuerySysInfo(5, 5, &mut drive, 4) };
    let mut p = Vec::new();
    p.push(b'@' + drive as u8);
    p.extend_from_slice(b":\\LANGUAGE\\LOCALE");
    p.extend_from_slice(file.as_bytes());
    p.push(0);
    p
}

fn read_user_loc() -> Option<Vec<u8>> {
    let path = user_loc_path("\\USER.LOC");
    let (mut hf, mut action) = (0u32, 0u32);
    let rc = unsafe { os2::DosOpen(path.as_ptr(), &mut hf, &mut action, 0, 0, 0x0001, 0x0040, core::ptr::null_mut()) };
    if rc != 0 {
        return None;
    }
    let mut data = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let mut got = 0u32;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut got) } != 0 || got == 0 {
            break;
        }
        data.extend_from_slice(&buf[..got as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(data)
}

/// Write USER.LOC (creating `\LANGUAGE\LOCALE` if needed): 0 or the
/// DosOpen/DosWrite error.
fn write_user_loc(text: &[u8]) -> u32 {
    let lang = user_loc_path("");
    let mut top = lang.clone();
    top.truncate(lang.len() - 1 - "\\LOCALE".len());
    top.push(0);
    unsafe {
        os2::DosCreateDir(top.as_ptr(), core::ptr::null_mut());
        os2::DosCreateDir(lang.as_ptr(), core::ptr::null_mut());
    }
    let path = user_loc_path("\\USER.LOC");
    let (mut hf, mut action) = (0u32, 0u32);
    let rc = unsafe { os2::DosOpen(path.as_ptr(), &mut hf, &mut action, 0, 0, 0x0012, 0x0021, core::ptr::null_mut()) };
    if rc != 0 {
        return rc;
    }
    let mut done = 0u32;
    let rc = unsafe { os2::DosWrite(hf, text.as_ptr(), text.len() as u32, &mut done) };
    unsafe { os2::DosClose(hf) };
    rc
}

/// An environment variable.
fn env(name: &[u8]) -> Option<String> {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(name.as_ptr(), &mut v) } != 0 || v.is_null() {
        return None;
    }
    let mut s = String::new();
    let mut p = v;
    unsafe {
        while *p != 0 {
            s.push(*p as char);
            p = p.add(1);
        }
    }
    let s = String::from(s.trim());
    (!s.is_empty()).then_some(s)
}

unsafe extern "C" {
    #[link_name = "NLS.5"]
    fn DosQueryCtryInfo(cb: u32, pcc: *mut u32, pci: *mut u8, actual: *mut u32) -> u32;
    #[link_name = "DOSCALLS.291"]
    fn DosQueryCp(cb: u32, cps: *mut u32, actual: *mut u32) -> u32;
}

/// The locale when no environment variable names one: the system
/// country's (rs2 does not set LANG; IBM's installation does).
fn default_locale() -> String {
    let mut cc = [0u32; 2];
    let mut info = [0u8; 44];
    let mut got = 0u32;
    let country = if unsafe { DosQueryCtryInfo(44, cc.as_mut_ptr(), info.as_mut_ptr(), &mut got) } == 0 {
        u32::from_le_bytes(info[..4].try_into().unwrap())
    } else {
        1
    };
    let i = uls::locale_for_country(country).unwrap_or_else(|| uls::find_system("en_US").unwrap());
    String::from(uls::system_name(i))
}

/// The process code page.
fn process_cp() -> u16 {
    let mut cps = [0u32; 4];
    let mut got = 0u32;
    if unsafe { DosQueryCp(16, cps.as_mut_ptr(), &mut got) } == 0 && cps[0] != 0 { cps[0] as u16 } else { 850 }
}

const CAT_VARS: [&[u8]; N_CATEGORIES] =
    [b"LC_COLLATE\0", b"LC_CTYPE\0", b"LC_NUMERIC\0", b"LC_MONETARY\0", b"LC_TIME\0", b"LC_MESSAGES\0"];

/// A category's locale from the environment: LC_ALL, LC_category, LANG.
fn env_locale(cat: usize) -> String {
    env(b"LC_ALL\0").or_else(|| env(CAT_VARS[cat])).or_else(|| env(b"LANG\0")).unwrap_or_else(default_locale)
}

// ---- memory ----------------------------------------------------------------

/// Allocate `cb` bytes that UniFreeMem frees (the size is kept in front).
fn alloc_bytes(cb: usize) -> *mut u8 {
    let layout = core::alloc::Layout::from_size_align(cb + 8, 8).unwrap();
    let p = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if p.is_null() {
        return p;
    }
    unsafe {
        (p as *mut usize).write(cb);
        p.add(8)
    }
}

fn free_bytes(p: *mut u8) {
    unsafe {
        let base = p.sub(8);
        let cb = (base as *const usize).read();
        alloc::alloc::dealloc(base, core::alloc::Layout::from_size_align(cb + 8, 8).unwrap());
    }
}

/// A NUL-terminated copy of a UCS-2 string for the caller to free.
fn alloc_ucs(s: &[u16]) -> *mut u16 {
    let p = alloc_bytes((s.len() + 1) * 2) as *mut u16;
    if !p.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len()) };
    }
    p
}

/// UniFreeMem: memory LIBUNI returned (static strings are left alone).
#[unsafe(no_mangle)]
pub extern "C" fn UniFreeMem(p: *mut u8) -> i32 {
    if p.is_null() {
        return ULS_INVALID;
    }
    if !uls::pool_range().contains(&(p as usize)) {
        free_bytes(p);
    }
    ULS_SUCCESS
}

// ---- locale objects ----------------------------------------------------------

const MAGIC: u32 = 0x4c53_4f42;

/// A locale object: the locale name of each category.
struct LocaleObject {
    magic: u32,
    names: [String; N_CATEGORIES],
}

/// A locale object handle, or None if it is not one.
fn object<'a>(lo: u32) -> Option<&'a LocaleObject> {
    let o = lo as *const LocaleObject;
    (!o.is_null() && unsafe { (*o).magic } == MAGIC).then(|| unsafe { &*o })
}

/// The category names a locale spec names (empty: the environment's;
/// one name: all categories; up to six: one per category).
fn spec_names(spec: &str) -> [String; N_CATEGORIES] {
    let words: Vec<&str> = spec.split_ascii_whitespace().collect();
    core::array::from_fn(|c| match words.len() {
        0 => env_locale(c),
        1 => String::from(words[0]),
        n => String::from(words[c.min(n - 1)]),
    })
}

/// The locale for a category of an object (0: the default locale).
fn resolve(u: &UserLocales, lo: u32, cat: usize) -> Option<Loc> {
    if lo == 0 {
        return u.find(&env_locale(cat)).or_else(|| uls::find_system("en_US").map(Loc::System));
    }
    let o = object(lo)?;
    let n = &o.names[cat];
    u.find(n).or_else(|| uls::find_system(n).map(Loc::System))
}

/// An item (with its LOCI_NOUSEROVERRIDE flag) of a locale object.
fn item_of<R>(lo: u32, item: i32, f: impl FnOnce(Option<&[u16]>, bool) -> R) -> R {
    let n = (item & 0x7fff) as usize;
    if !(1..=MAXITEM).contains(&n) {
        return f(None, false);
    }
    with_users(false, |u| match resolve(u, lo, uls::item_category(n)) {
        Some(l) => {
            let v = u.item(l, n, item & LOCI_NOUSEROVERRIDE != 0);
            // System values can be handed out from the static tables.
            f(Some(v), uls::pool_range().contains(&(v.as_ptr() as usize)))
        }
        None => f(None, false),
    })
}

/// Item `n` of a locale object as an owned string.
fn item_vec(lo: u32, n: usize) -> Vec<u16> {
    item_of(lo, n as i32, |v, _| v.map(|v| v.to_vec()).unwrap_or_default())
}

/// The UCS-2 or ASCII string a spec pointer points to.
unsafe fn spec_string(ty: i32, spec: *const u8) -> Option<String> {
    if spec.is_null() {
        return Some(String::new());
    }
    match ty {
        2 => {
            let mut s = String::new();
            let mut p = spec;
            unsafe {
                while *p != 0 {
                    s.push(*p as char);
                    p = p.add(1);
                }
            }
            Some(s)
        }
        3 => ascii(unsafe { from_ptr(spec as *const u16) }),
        _ => None,
    }
}

/// UniCreateLocaleObject(spec type, spec, &object): 1 a locale token, 2
/// a char string, 3 a UniChar string; an empty name takes the locale
/// from LC_ALL, LC_category or LANG.
#[unsafe(no_mangle)]
pub extern "C" fn UniCreateLocaleObject(ty: i32, spec: *const u8, out: *mut u32) -> i32 {
    if out.is_null() {
        return ULS_INVALID;
    }
    unsafe { out.write_unaligned(0) };
    let spec = match ty {
        1 if spec.is_null() => Some(String::from("univ")),
        1 => token_name(spec as u32).map(String::from),
        _ => unsafe { spec_string(ty, spec) },
    };
    let Some(spec) = spec else { return ULS_INVALID };
    let names = spec_names(&spec);
    let ok = with_users(true, |u| names.iter().all(|n| u.find(n).is_some()));
    if !ok {
        return ULS_UNSUPPORTED;
    }
    let o = Box::new(LocaleObject { magic: MAGIC, names });
    unsafe { out.write_unaligned(Box::into_raw(o) as u32) };
    ULS_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn UniFreeLocaleObject(lo: u32) -> i32 {
    if object(lo).is_none() {
        return ULS_BADOBJECT;
    }
    let mut o = unsafe { Box::from_raw(lo as *mut LocaleObject) };
    o.magic = 0;
    ULS_SUCCESS
}

/// UniQueryLocaleObject(object, category, spec type, &spec): the locale
/// name of a category; for LC_ALL (-1) the six names separated by blanks.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLocaleObject(lo: u32, cat: i32, ty: i32, out: *mut *mut u8) -> i32 {
    if out.is_null() || !(1..=3).contains(&ty) {
        return ULS_INVALID;
    }
    unsafe { out.write_unaligned(core::ptr::null_mut()) };
    if !(-1..=5).contains(&cat) {
        return ULS_UNSUPPORTED;
    }
    let names: [String; N_CATEGORIES] = match object(lo) {
        Some(o) => o.names.clone(),
        None if lo == 0 => core::array::from_fn(env_locale),
        None => return ULS_BADOBJECT,
    };
    let name = if cat < 0 { names.join(" ") } else { names[cat as usize].clone() };
    let p = match ty {
        1 => {
            let first = name.split(' ').next().unwrap_or("");
            match token_of(first) {
                Some(t) => t as *mut u8,
                None => return ULS_UNSUPPORTED,
            }
        }
        2 => {
            let p = alloc_bytes(name.len() + 1);
            if !p.is_null() {
                unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), p, name.len()) };
            }
            p
        }
        _ => alloc_ucs(&ucs(&name)) as *mut u8,
    };
    if p.is_null() {
        return ULS_NOMEMORY;
    }
    unsafe { out.write_unaligned(p) };
    ULS_SUCCESS
}

/// UniQueryLocaleItem(object, item, &string): free it with UniFreeMem.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLocaleItem(lo: u32, item: i32, out: *mut *mut u16) -> i32 {
    if out.is_null() {
        return ULS_INVALID;
    }
    if lo != 0 && object(lo).is_none() {
        return ULS_BADOBJECT;
    }
    item_of(lo, item, |v, is_static| {
        let Some(v) = v else { return ULS_INVALID };
        let p = if is_static {
            // Point into the static table (the string is NUL-terminated there).
            v.as_ptr() as *mut u16
        } else {
            alloc_ucs(v)
        };
        if p.is_null() {
            return ULS_NOMEMORY;
        }
        unsafe { out.write_unaligned(p) };
        ULS_SUCCESS
    })
}

/// UniQueryLocaleValue(object, item, &value): numeric items.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLocaleValue(lo: u32, item: i32, out: *mut i32) -> i32 {
    if out.is_null() {
        return ULS_INVALID;
    }
    if lo != 0 && object(lo).is_none() {
        return ULS_BADOBJECT;
    }
    match item_of(lo, item, |v, _| v.and_then(|v| uls::item_value((item & 0x1fff) as usize, v))) {
        Some(v) => {
            unsafe { out.write_unaligned(v) };
            ULS_SUCCESS
        }
        None => ULS_INVALID,
    }
}

/// The UniLconv structure (unidef.h).
#[repr(C)]
struct UniLconv {
    strings: [*mut u16; 10],
    shorts: [i16; 10],
    debit: *mut u16,
    credit: *mut u16,
    left: *mut u16,
    right: *mut u16,
}

/// UniQueryLocaleInfo(object, &lconv): one block (strings follow the
/// structure) freed with UniFreeLocaleInfo.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLocaleInfo(lo: u32, out: *mut *mut u8) -> i32 {
    if out.is_null() {
        return ULS_INVALID;
    }
    if lo != 0 && object(lo).is_none() {
        return ULS_BADOBJECT;
    }
    // decimal_point, thousands_sep, grouping, int_curr_symbol,
    // currency_symbol, mon_decimal_point, mon_thousands_sep, mon_grouping,
    // positive_sign, negative_sign; debit, credit, parentheses.
    const ITEMS: [usize; 14] = [44, 45, 68, 87, 48, 66, 67, 69, 92, 93, 120, 121, 94, 95];
    let vals: Vec<Vec<u16>> = ITEMS.iter().map(|&n| item_vec(lo, n)).collect();
    let head = core::mem::size_of::<UniLconv>();
    let total = head + vals.iter().map(|v| (v.len() + 1) * 2).sum::<usize>();
    let p = alloc_bytes(total);
    if p.is_null() {
        return ULS_NOMEMORY;
    }
    let num = |n: usize| uls::item_value(n, &item_vec(lo, n)).unwrap_or(0);
    let mut ptrs = [core::ptr::null_mut::<u16>(); 14];
    let mut s = unsafe { p.add(head) } as *mut u16;
    for (k, v) in vals.iter().enumerate() {
        ptrs[k] = s;
        unsafe {
            core::ptr::copy_nonoverlapping(v.as_ptr(), s, v.len());
            s = s.add(v.len() + 1);
        }
    }
    let pos = uls::lconv_positions(num(75), num(83));
    let digits = num(76) as i16;
    let l = UniLconv {
        strings: ptrs[..10].try_into().unwrap(),
        shorts: [digits, digits, pos[0], pos[1], pos[2], pos[3], pos[4], pos[5], pos[6], 0],
        debit: ptrs[10],
        credit: ptrs[11],
        left: ptrs[12],
        right: ptrs[13],
    };
    unsafe {
        (p as *mut UniLconv).write(l);
        out.write_unaligned(p);
    }
    ULS_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn UniFreeLocaleInfo(p: *mut u8) -> i32 {
    UniFreeMem(p)
}

/// UniStrftime(object, buffer, size, format, tm): the number of characters
/// written (without the NUL), 0 if they do not fit.
#[unsafe(no_mangle)]
pub extern "C" fn UniStrftime(lo: u32, buf: *mut u16, max: u32, fmt: *const u16, tm: *const uls::Tm) -> u32 {
    if buf.is_null() || fmt.is_null() || tm.is_null() {
        return 0;
    }
    let items: Vec<Vec<u16>> = (1..=MAXITEM).map(|n| item_vec(lo, n)).collect();
    let get = |n: usize| items[n - 1].as_slice();
    let out = uls::strftime(&get, unsafe { from_ptr(fmt) }, unsafe { &*tm });
    if out.len() + 1 > max as usize {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), buf, out.len());
        buf.add(out.len()).write(0);
    }
    out.len() as u32
}

/// UniStrptime(object, buf, format, tm): parses `buf` by `format` into
/// `tm`; a pointer to the first character not used, NULL if it does not
/// match.
#[unsafe(no_mangle)]
pub extern "C" fn UniStrptime(lo: u32, buf: *const u16, fmt: *const u16, tm: *mut uls::Tm) -> *const u16 {
    if buf.is_null() || fmt.is_null() || tm.is_null() {
        return core::ptr::null();
    }
    let items: Vec<Vec<u16>> = (1..=MAXITEM).map(|n| item_vec(lo, n)).collect();
    let get = |n: usize| items[n - 1].as_slice();
    match uls::strptime(&get, unsafe { from_ptr(buf) }, unsafe { from_ptr(fmt) }, unsafe { &mut *tm }) {
        Some(n) => unsafe { buf.add(n) },
        None => core::ptr::null(),
    }
}

// ---- locale names and tokens -----------------------------------------------------

/// The token of a locale (its xLocaleToken item).
fn token_of(name: &str) -> Option<u32> {
    with_users(false, |u| u.find(name).map(|l| u.item(l, 50, true).to_vec()))
        .and_then(|v| uls::item_value(50, &v))
        .map(|v| v as u32)
        .filter(|&t| t != 0)
}

fn token_name(token: u32) -> Option<&'static str> {
    (0..uls::system_count())
        .find(|&i| uls::item_value(50, uls::system_item(i, 50)) == Some(token as i32))
        .map(uls::system_name)
}

/// UniLocaleStrToToken(string type, name, &token).
#[unsafe(no_mangle)]
pub extern "C" fn UniLocaleStrToToken(ty: i32, s: *const u8, out: *mut u32) -> i32 {
    if out.is_null() {
        return ULS_INVALID;
    }
    let Some(name) = (unsafe { spec_string(ty, s) }) else { return ULS_INVALID };
    match token_of(&name) {
        Some(t) => {
            unsafe { out.write_unaligned(t) };
            ULS_SUCCESS
        }
        None => uls::ULS_NOMATCH,
    }
}

/// UniLocaleTokenToStr(token, string type, &name).
#[unsafe(no_mangle)]
pub extern "C" fn UniLocaleTokenToStr(token: u32, ty: i32, out: *mut *mut u8) -> i32 {
    if out.is_null() || !(2..=3).contains(&ty) {
        return ULS_INVALID;
    }
    let Some(name) = token_name(token) else { return uls::ULS_NOMATCH };
    name_out(name, ty, out)
}

fn name_out(name: &str, ty: i32, out: *mut *mut u8) -> i32 {
    let p = if ty == 2 {
        let p = alloc_bytes(name.len() + 1);
        if !p.is_null() {
            unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), p, name.len()) };
        }
        p
    } else {
        alloc_ucs(&ucs(name)) as *mut u8
    };
    if p.is_null() {
        return ULS_NOMEMORY;
    }
    unsafe { out.write_unaligned(p) };
    ULS_SUCCESS
}

/// UniMapCtryToLocale(country, buffer, size): the locale name of a
/// country code.
#[unsafe(no_mangle)]
pub extern "C" fn UniMapCtryToLocale(country: u32, buf: *mut u16, n: u32) -> i32 {
    if buf.is_null() {
        return ULS_INVALID;
    }
    let Some(i) = uls::locale_for_country(country) else { return ULS_INVALID };
    let name = ucs(uls::system_name(i));
    if name.len() + 1 > n as usize {
        return ULS_BUFFERFULL;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), buf, name.len());
        buf.add(name.len()).write(0);
    }
    ULS_SUCCESS
}

/// UniQueryLocaleList(flags, buffer, size): the locale names, each
/// NUL-terminated, then an empty one. 1 system, 2 user, 0 or 3 both.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLocaleList(flags: u32, buf: *mut u16, cch: i32) -> i32 {
    if buf.is_null() || cch < 1 {
        return ULS_INVALID;
    }
    match with_users(true, |u| u.locale_list(flags, cch as usize)) {
        Ok(v) => {
            unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), buf, v.len()) };
            ULS_SUCCESS
        }
        Err(e) => {
            unsafe { buf.write(0) };
            e
        }
    }
}

/// The first two characters of a UniChar string as ASCII.
unsafe fn code2(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { from_ptr(p) };
    ascii(&s[..s.len().min(2)])
}

/// UniQueryLanguageName(language, display language, &name).
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLanguageName(lang: *const u16, iso: *const u16, out: *mut *const u16) -> i32 {
    let (Some(lang), Some(iso)) = (unsafe { code2(lang) }, unsafe { code2(iso) }) else { return ULS_INVALID };
    if out.is_null() {
        return ULS_INVALID;
    }
    match uls::language_name(&lang, &iso) {
        Some(n) => {
            unsafe { out.write_unaligned(n.as_ptr()) };
            ULS_SUCCESS
        }
        None => ULS_INVALID,
    }
}

/// UniQueryCountryName(country, display language, &name).
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryCountryName(ctry: *const u16, iso: *const u16, out: *mut *const u16) -> i32 {
    let (Some(ctry), Some(iso)) = (unsafe { code2(ctry) }, unsafe { code2(iso) }) else { return ULS_INVALID };
    if out.is_null() {
        return ULS_INVALID;
    }
    match uls::country_name(&ctry, &iso) {
        Some(n) => {
            unsafe { out.write_unaligned(n.as_ptr()) };
            ULS_SUCCESS
        }
        None => ULS_INVALID,
    }
}

// ---- user locales --------------------------------------------------------------

/// UniMakeUserLocale(name, base): a user locale based on a system locale
/// (no base: the environment's locale). Base characters with 0xFF00 set
/// replace an existing user locale of that name.
#[unsafe(no_mangle)]
pub extern "C" fn UniMakeUserLocale(name: *const u16, base: *const u16) -> i32 {
    if name.is_null() {
        return ULS_INVALID;
    }
    let name = unsafe { from_ptr(name) };
    let base = if base.is_null() { &[][..] } else { unsafe { from_ptr(base) } };
    if base.iter().any(|&c| c & 0xff00 != 0 && c & 0xff00 != 0xff00) {
        return ULS_INVALID;
    }
    let force = base.iter().any(|&c| c & 0xff00 == 0xff00);
    let base: String = if base.is_empty() {
        env(b"LC_ALL\0").or_else(|| env(b"LANG\0")).unwrap_or_else(default_locale)
    } else {
        base.iter().map(|&c| (c & 0xff) as u8 as char).collect()
    };
    with_users(false, |u| u.make(name, &base, force))
}

/// UniSetUserLocaleItem(locale, item, type, value): type 1 an int (by
/// reference), 2 a string in the process code page, 3 a UniChar string.
#[unsafe(no_mangle)]
pub extern "C" fn UniSetUserLocaleItem(locale: *const u16, item: i32, ty: i32, value: *const u8) -> i32 {
    if locale.is_null() || value.is_null() || !(1..=MAXITEM as i32).contains(&item) || !(1..=3).contains(&ty) {
        return ULS_INVALID;
    }
    let Some(name) = ascii(unsafe { from_ptr(locale) }) else { return uls::ULS_NOMATCH };
    let v: Vec<u16> = match ty {
        1 => {
            let n = unsafe { (value as *const i32).read_unaligned() };
            let text = uls::value_string(item as usize, n);
            ucs(&text)
        }
        2 => {
            let cp = process_cp();
            let mut v = Vec::new();
            let mut p = value;
            unsafe {
                while *p != 0 {
                    v.push(uls::cp_to_ucs(cp, *p));
                    p = p.add(1);
                }
            }
            v
        }
        _ => unsafe { from_ptr(value as *const u16) }.to_vec(),
    };
    with_users(false, |u| u.set_item(&name, item as usize, &v))
}

/// UniDeleteUserLocale(name).
#[unsafe(no_mangle)]
pub extern "C" fn UniDeleteUserLocale(locale: *const u16) -> i32 {
    if locale.is_null() {
        return ULS_INVALID;
    }
    let Some(name) = ascii(unsafe { from_ptr(locale) }) else { return uls::ULS_NOMATCH };
    with_users(false, |u| u.delete(&name))
}

/// UniCompleteUserLocale(): write the changed user locales to USER.LOC.
#[unsafe(no_mangle)]
pub extern "C" fn UniCompleteUserLocale() -> i32 {
    with_users(false, |u| {
        if !u.changed {
            return ULS_SUCCESS;
        }
        let rc = write_user_loc(&u.serialize());
        if rc == 0 {
            u.changed = false;
        }
        rc as i32
    })
}

// ---- character classification ---------------------------------------------------

fn ctype(uc: u16) -> u16 {
    uls::char_type(uc).itype
}

// UniQueryAlpha(object, character) and the like (locale independent).

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryXdigit(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_XDIGIT != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQuerySpace(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_SPACE != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryPrint(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_PRINT != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryGraph(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_GRAPH != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryCntrl(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_CNTRL != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryAlpha(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_ALPHA != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryPunct(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_PUNCT != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryLower(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_LOWER != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryUpper(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_UPPER != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryDigit(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_DIGIT != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryBlank(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_BLANK != 0) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniQueryAlnum(_lo: u32, uc: u32) -> i32 {
    (ctype(uc as u16) & uls::CT_ALNUM != 0) as i32
}

/// UniQueryChar(character, attributes): whether it has any of them.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryChar(uc: u32, attr: u32) -> i32 {
    (ctype(uc as u16) as u32 & attr != 0) as i32
}

const TYPE_SLOTS: usize = 128;
static mut TYPES: [uls::UniCType; TYPE_SLOTS] =
    [uls::UniCType { itype: 0, bidi: 0, charset: 0, extend: 0, codepage: 0 }; TYPE_SLOTS];
static mut TYPE_COUNT: usize = 0;

/// UniQueryCharType(character): a pointer to its UNICTYPE (shared by all
/// characters of the same type).
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryCharType(uc: u32) -> *const uls::UniCType {
    let t = uls::char_type(uc as u16);
    with_users(false, |_| unsafe {
        let types = &mut *core::ptr::addr_of_mut!(TYPES);
        let count = &mut *core::ptr::addr_of_mut!(TYPE_COUNT);
        if let Some(i) = types[..*count].iter().position(|x| *x == t) {
            return &types[i] as *const _;
        }
        let i = if *count < TYPE_SLOTS {
            *count += 1;
            *count - 1
        } else {
            TYPE_SLOTS - 1
        };
        types[i] = t;
        &types[i] as *const _
    })
}

/// UniQueryNumericValue(character): its decimal digit value, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryNumericValue(uc: u32) -> i32 {
    char::from_u32(uc & 0xffff).and_then(|c| c.to_digit(10)).map_or(-1, |d| d as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn UniToupper(uc: u32) -> u32 {
    uls::to_upper(uc as u16) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniTolower(uc: u32) -> u32 {
    uls::to_lower(uc as u16) as u32
}
