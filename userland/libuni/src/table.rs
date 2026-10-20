//! Export 151, `Locale`: the table IBM's LIBUNI gets from LOCALE.DLL
//! (locale_init) and exports as is. GLOCALE reads its first word (the
//! highest item number) and calls its functions directly, with the
//! arguments in EAX, EDX (and ECX) and the result in EAX:
//!
//! | Offset | Contents |
//! |--------|----------|
//! | 0x00 | highest item number (136) |
//! | 0x04, 0x08 | first and last address of the static item strings (a string outside them was allocated) |
//! | 0x0C | locale table header (rs2: none) |
//! | 0x10 | locale_name(char *name): locale handle or 0 |
//! | 0x14 | locale_uniname(UniChar *name): locale handle or 0 |
//! | 0x18 | locale_token(token): locale handle or 0 |
//! | 0x1C | locale_item(handle, item): UniChar * (an empty string on errors) |
//! | 0x20 | locale_value(handle, item): the number, -1 if it is none |
//! | 0x24 | locale_data(handle, &data): sets data to 0, returns 0 |
//! | 0x28 | locale_itemname(item): char * or 0 |
//! | 0x2C | locale_itemnum(char *name): item number or -1 |
//! | 0x30 | locale_itemcategory(item): category digit |
//!
//! A handle is a system locale (rs2: the address of its item offsets in
//! crates/uls's table); the functions know nothing of user locales.

use uls::data::{LOCALES, POOL};
use uls::{MAXITEM, ascii, find_system, from_ptr};

/// A `_Optlink`-style entry: EAX, EDX, ECX become the C arguments.
macro_rules! regcall {
    ($($name:ident => $f:path;)*) => {$(
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 4",
                "push ecx", "push edx", "push eax",
                "call {f}", "mov esp, ebp", "pop ebp", "ret",
                f = sym $f,
            );
        }
    )*};
}

regcall! {
    r_name => locale_name;
    r_uniname => locale_uniname;
    r_token => locale_token;
    r_item => locale_item;
    r_value => locale_value;
    r_data => locale_data;
    r_itemname => locale_itemname;
    r_itemnum => locale_itemnum;
    r_itemcategory => locale_itemcategory;
}

#[repr(C)]
pub struct Table {
    max_item: u32,
    min: *const u16,
    max: *const u16,
    header: *const u8,
    functions: [unsafe extern "C" fn(); 9],
    reserved: [u32; 3],
}

unsafe impl Sync for Table {}

#[unsafe(no_mangle)]
pub static Locale: Table = Table {
    max_item: MAXITEM as u32,
    min: POOL.as_ptr(),
    max: unsafe { POOL.as_ptr().add(POOL.len()) },
    header: core::ptr::null(),
    functions: [r_name, r_uniname, r_token, r_item, r_value, r_data, r_itemname, r_itemnum, r_itemcategory],
    reserved: [0; 3],
};

/// The empty string (the last NUL of the pool).
fn null_uni() -> *const u16 {
    &POOL[POOL.len() - 1]
}

fn handle(i: usize) -> u32 {
    LOCALES[i].1.as_ptr() as u32
}

fn index(h: u32) -> Option<usize> {
    (0..LOCALES.len()).find(|&i| handle(i) == h)
}

fn by_name(name: &str) -> u32 {
    find_system(name).map_or(0, handle)
}

extern "C" fn locale_name(p: *const u8) -> u32 {
    if p.is_null() {
        return 0;
    }
    let mut s = alloc::string::String::new();
    let mut q = p;
    unsafe {
        while *q != 0 && s.len() < 64 {
            s.push(*q as char);
            q = q.add(1);
        }
    }
    by_name(&s)
}

extern "C" fn locale_uniname(p: *const u16) -> u32 {
    if p.is_null() {
        return 0;
    }
    ascii(unsafe { from_ptr(p) }).map_or(0, |s| by_name(&s))
}

extern "C" fn locale_token(token: u32) -> u32 {
    (0..LOCALES.len())
        .find(|&i| uls::item_value(50, uls::system_item(i, 50)) == Some(token as i32))
        .map_or(0, handle)
}

extern "C" fn locale_item(h: u32, item: u32) -> *const u16 {
    let n = (item & 0x1fff) as usize;
    match index(h) {
        Some(i) if (1..=MAXITEM).contains(&n) => uls::system_item_z(i, n).as_ptr(),
        _ => null_uni(),
    }
}

extern "C" fn locale_value(h: u32, item: u32) -> i32 {
    let n = (item & 0x1fff) as usize;
    match index(h) {
        Some(i) if (1..=MAXITEM).contains(&n) => uls::item_value(n, uls::system_item(i, n)).unwrap_or(-1),
        _ => -1,
    }
}

extern "C" fn locale_data(_h: u32, out: *mut u32) -> u32 {
    if !out.is_null() {
        unsafe { out.write_unaligned(0) };
    }
    0
}

extern "C" fn locale_itemname(item: i32) -> *const u8 {
    match uls::ITEM_NAMES_Z.get((item as usize).wrapping_sub(1)) {
        Some(n) if item > 0 => n.as_ptr(),
        _ => core::ptr::null(),
    }
}

extern "C" fn locale_itemnum(p: *const u8) -> i32 {
    if p.is_null() {
        return -1;
    }
    let mut s = alloc::string::String::new();
    let mut q = p;
    unsafe {
        while *q != 0 && s.len() < 64 {
            s.push(*q as char);
            q = q.add(1);
        }
    }
    uls::item_by_name(&s).map_or(-1, |i| i as i32)
}

extern "C" fn locale_itemcategory(item: i32) -> u32 {
    uls::item_category_digit(item as usize).unwrap_or(0) as u32
}
