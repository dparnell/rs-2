//! Resources of the program and its DLLs, through W32KERN's
//! FindResource: a resource's bytes, string tables, and the resource
//! names USER's calls take (an ID or a string, A or W).

use alloc::vec::Vec;

use crate::{cstr, wstr};

pub const RT_CURSOR: u16 = 1;
pub const RT_BITMAP: u16 = 2;
pub const RT_ICON: u16 = 3;
pub const RT_MENU: u16 = 4;
pub const RT_DIALOG: u16 = 5;
pub const RT_STRING: u16 = 6;
pub const RT_ACCELERATOR: u16 = 9;
pub const RT_GROUP_CURSOR: u16 = 12;
pub const RT_GROUP_ICON: u16 = 14;


/// A resource name: an ID or a string (as UTF-16, as the resource
/// directory has it).
#[derive(Clone, PartialEq, Debug)]
pub enum Name {
    Id(u16),
    Str(Vec<u16>),
}

impl Name {
    pub unsafe fn a(p: *const u8) -> Option<Name> {
        if p.is_null() {
            return None;
        }
        if (p as u32) < 0x10000 {
            return Some(Name::Id(p as u16));
        }
        Some(Name::Str(crate::to_wide(unsafe { cstr(p) })))
    }

    pub unsafe fn w(p: *const u16) -> Option<Name> {
        if p.is_null() {
            return None;
        }
        if (p as u32) < 0x10000 {
            return Some(Name::Id(p as u16));
        }
        Some(Name::Str(unsafe { wstr(p) }.to_vec()))
    }

    /// From an A or W pointer.
    pub unsafe fn of(p: u32, wide: bool) -> Option<Name> {
        unsafe { if wide { Name::w(p as *const u16) } else { Name::a(p as *const u8) } }
    }
}

/// A resource's bytes.
pub fn find(hinst: u32, ty: u16, name: &Name) -> Option<&'static [u8]> {
    let mut z: Vec<u16>;
    let np = match name {
        Name::Id(i) => *i as usize as *const u16,
        Name::Str(s) => {
            z = s.clone();
            z.push(0);
            z.as_ptr()
        }
    };
    crate::kern::resource(hinst, np, ty as usize as *const u16)
}

/// A string of a string table (blocks of 16 counted UTF-16 strings).
pub fn string(hinst: u32, id: u32) -> Option<&'static [u16]> {
    let block = find(hinst, RT_STRING, &Name::Id((id / 16 + 1) as u16))?;
    let mut p = 0;
    for i in 0..16 {
        let n = w16(block, p) as usize;
        p += 2;
        if i == id % 16 {
            let s = block.get(p..p + 2 * n)?;
            return Some(unsafe { core::slice::from_raw_parts(s.as_ptr() as *const u16, n) });
        }
        p += 2 * n;
    }
    None
}

pub fn w16(b: &[u8], p: usize) -> u16 {
    b.get(p..p + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}

pub fn l32(b: &[u8], p: usize) -> u32 {
    b.get(p..p + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// A NUL-terminated UTF-16 string at `*p` in a resource (moving past it).
pub fn wsz(b: &[u8], p: &mut usize) -> Vec<u16> {
    let mut v = Vec::new();
    while *p + 1 < b.len() {
        let c = w16(b, *p);
        *p += 2;
        if c == 0 {
            break;
        }
        v.push(c);
    }
    v
}

/// A name or ordinal in a dialog or menu template: 0xFFFF and an ID, or a
/// string.
pub fn sz_or_ord(b: &[u8], p: &mut usize) -> Name {
    if w16(b, *p) == 0xffff {
        let id = w16(b, *p + 2);
        *p += 4;
        return Name::Id(id);
    }
    Name::Str(wsz(b, p))
}
