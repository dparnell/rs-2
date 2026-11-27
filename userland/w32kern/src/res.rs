//! Resources of PE modules: the resource directory (data directory 2), a
//! tree of type, name and language. An HRSRC is the address of the
//! leaf's IMAGE_RESOURCE_DATA_ENTRY (RVA, size, code page); an HGLOBAL
//! from LoadResource is the data's address (the image is mapped, so
//! LockResource has nothing to do).
//!
//! A name or type is an integer (MAKEINTRESOURCE: below 0x10000) or a
//! string, "#123" meaning 123; strings compare without case.

use alloc::vec::Vec;

use crate::process::exe_base;
use crate::text::{to_wide, wupper};
use crate::{cstr, err, fail, pe, wstr, FALSE, TRUE};

/// A resource name: an ID or an upper-cased UTF-16 string.
#[derive(Clone, PartialEq)]
pub enum Name {
    Id(u16),
    Str(Vec<u16>),
}

impl Name {
    fn from_wide(s: &[u16]) -> Name {
        if s.first() == Some(&(b'#' as u16)) {
            let mut v: u32 = 0;
            for &c in &s[1..] {
                if !(b'0' as u16..=b'9' as u16).contains(&c) {
                    break;
                }
                v = v * 10 + (c - b'0' as u16) as u32;
            }
            return Name::Id(v as u16);
        }
        Name::Str(s.iter().map(|&c| wupper(c)).collect())
    }

    pub unsafe fn a(p: *const u8) -> Name {
        if (p as u32) < 0x10000 {
            return Name::Id(p as u16);
        }
        Name::from_wide(&to_wide(unsafe { cstr(p) }))
    }

    pub unsafe fn w(p: *const u16) -> Name {
        if (p as u32) < 0x10000 {
            return Name::Id(p as u16);
        }
        Name::from_wide(unsafe { wstr(p) })
    }
}

fn rd32(a: u32) -> u32 {
    unsafe { (a as *const u32).read_unaligned() }
}

fn rd16(a: u32) -> u16 {
    unsafe { (a as *const u16).read_unaligned() }
}

/// The image base of a module handle (0: the program).
fn base_of(h: u32) -> u32 {
    if h == 0 {
        exe_base()
    } else if h < 0x10000 {
        unsafe { crate::Rs2W32BaseOfHmte(h) }
    } else {
        h
    }
}

/// The entries of a directory: (name, offset in the resource section
/// with the subdirectory bit).
fn entries(root: u32, dir: u32) -> Vec<(Name, u32)> {
    let d = root + dir;
    let n = rd16(d + 12) as u32 + rd16(d + 14) as u32;
    (0..n.min(4096))
        .map(|i| {
            let e = d + 16 + i * 8;
            let (nm, off) = (rd32(e), rd32(e + 4));
            let name = if nm & 0x8000_0000 != 0 {
                let s = root + (nm & 0x7fff_ffff);
                let len = rd16(s) as u32;
                Name::Str((0..len).map(|k| wupper(rd16(s + 2 + 2 * k))).collect())
            } else {
                Name::Id(nm as u16)
            };
            (name, off)
        })
        .collect()
}

/// The resource directory of a module: (its address, the image base).
fn root(h: u32) -> Option<(u32, u32)> {
    let base = base_of(h);
    let (rva, _) = unsafe { pe::dir(base, 2) }?;
    Some((base + rva, base))
}

/// Find a resource: its data entry's address. `lang` None takes the
/// first language.
pub fn find(h: u32, ty: &Name, name: &Name, lang: Option<u16>) -> Option<u32> {
    let (r, _) = root(h)?;
    let (_, t) = entries(r, 0).into_iter().find(|e| e.0 == *ty)?;
    if t & 0x8000_0000 == 0 {
        return None;
    }
    let (_, n) = entries(r, t & 0x7fff_ffff).into_iter().find(|e| e.0 == *name)?;
    if n & 0x8000_0000 == 0 {
        return Some(r + n);
    }
    let langs = entries(r, n & 0x7fff_ffff);
    let pick = lang
        .and_then(|l| langs.iter().find(|e| e.0 == Name::Id(l)))
        .or_else(|| langs.iter().find(|e| matches!(e.0, Name::Id(0))))
        .or_else(|| langs.first())?;
    (pick.1 & 0x8000_0000 == 0).then_some(r + pick.1)
}

/// A resource's data and size from its HRSRC.
pub fn data(h: u32, hrsrc: u32) -> Option<(u32, u32)> {
    if hrsrc == 0 {
        return None;
    }
    let base = base_of(h);
    Some((base + rd32(hrsrc), rd32(hrsrc + 4)))
}

/// The names of a module's resources of a type.
pub fn names(h: u32, ty: &Name) -> Vec<Name> {
    let Some((r, _)) = root(h) else { return Vec::new() };
    let Some((_, t)) = entries(r, 0).into_iter().find(|e| e.0 == *ty) else { return Vec::new() };
    if t & 0x8000_0000 == 0 {
        return Vec::new();
    }
    entries(r, t & 0x7fff_ffff).into_iter().map(|e| e.0).collect()
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FindResourceA(h: u32, name: *const u8, ty: *const u8) -> u32 {
    let (n, t) = unsafe { (Name::a(name), Name::a(ty)) };
    find(h, &t, &n, None).unwrap_or_else(|| fail(err::RESOURCE_NAME_NOT_FOUND, 0))
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FindResourceW(h: u32, name: *const u16, ty: *const u16) -> u32 {
    let (n, t) = unsafe { (Name::w(name), Name::w(ty)) };
    find(h, &t, &n, None).unwrap_or_else(|| fail(err::RESOURCE_NAME_NOT_FOUND, 0))
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FindResourceExA(h: u32, ty: *const u8, name: *const u8, lang: u16) -> u32 {
    let (n, t) = unsafe { (Name::a(name), Name::a(ty)) };
    find(h, &t, &n, Some(lang)).unwrap_or_else(|| fail(err::RESOURCE_NAME_NOT_FOUND, 0))
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn FindResourceExW(h: u32, ty: *const u16, name: *const u16, lang: u16) -> u32 {
    let (n, t) = unsafe { (Name::w(name), Name::w(ty)) };
    find(h, &t, &n, Some(lang)).unwrap_or_else(|| fail(err::RESOURCE_NAME_NOT_FOUND, 0))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LoadResource(h: u32, hrsrc: u32) -> u32 {
    data(h, hrsrc).map_or_else(|| fail(err::INVALID_HANDLE, 0), |d| d.0)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LockResource(hg: u32) -> u32 {
    hg
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn FreeResource(_hg: u32) -> i32 {
    FALSE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn SizeofResource(h: u32, hrsrc: u32) -> u32 {
    data(h, hrsrc).map_or(0, |d| d.1)
}

/// EnumResourceNamesA/W(h, type, proc, lparam): proc(h, type, name,
/// lparam) for each name until it returns FALSE.
fn enum_names(h: u32, ty: &Name, ty_arg: u32, f: u32, lp: u32, wide: bool) -> i32 {
    let list = names(h, ty);
    if list.is_empty() {
        return fail(err::RESOURCE_TYPE_NOT_FOUND, FALSE);
    }
    for n in list {
        let mut buf: Vec<u8> = Vec::new();
        let arg = match &n {
            Name::Id(i) => *i as u32,
            Name::Str(s) => {
                if wide {
                    for &c in s.iter().chain([0u16].iter()) {
                        buf.extend_from_slice(&c.to_le_bytes());
                    }
                } else {
                    buf = crate::text::from_wide(s);
                    buf.push(0);
                }
                buf.as_ptr() as u32
            }
        };
        let r = unsafe {
            let p: extern "stdcall" fn(u32, u32, u32, u32) -> i32 = core::mem::transmute(f);
            p(h, ty_arg, arg, lp)
        };
        if r == 0 {
            break;
        }
    }
    TRUE
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn EnumResourceNamesA(h: u32, ty: *const u8, f: u32, lp: u32) -> i32 {
    enum_names(h, &unsafe { Name::a(ty) }, ty as u32, f, lp, false)
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn EnumResourceNamesW(h: u32, ty: *const u16, f: u32, lp: u32) -> i32 {
    enum_names(h, &unsafe { Name::w(ty) }, ty as u32, f, lp, true)
}
