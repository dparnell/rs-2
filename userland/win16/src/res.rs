//! The resources of the program and its DLLs, from the resource table of
//! the NE file
//! (Windows' format: an alignment shift, then TYPEINFO records of
//! NAMEINFO entries; IDs with bit 15 set are numbers, others offsets of
//! names in the table). A module's file is read once, when a resource is
//! first needed. KERNEL's FindResource, LoadResource, LockResource,
//! SizeofResource, FreeResource and USER's LoadString are here; menus,
//! accelerators and dialog templates are parsed by menu.rs and
//! dialog.rs from `find`.

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::{far_str, Global};

pub const RT_MENU: u16 = 4;
pub const RT_DIALOG: u16 = 5;
pub const RT_STRING: u16 = 6;
pub const RT_ACCELERATOR: u16 = 9;

/// A resource or type name: a number or a string (upper case).
#[derive(Clone, PartialEq, Eq)]
pub enum Name {
    Id(u16),
    Str(Vec<u8>),
}

impl Name {
    /// A name argument: MAKEINTRESOURCE (selector 0), "#123" or a string.
    pub fn of(far: u32) -> Option<Name> {
        if far == 0 {
            return None;
        }
        match far_str(far) {
            None => Some(Name::Id(far as u16)),
            Some(s) => Some(Name::from_bytes(s)),
        }
    }

    pub fn from_bytes(s: &[u8]) -> Name {
        if let Some(n) = s.strip_prefix(b"#") {
            if !n.is_empty() && n.iter().all(u8::is_ascii_digit) {
                return Name::Id(n.iter().fold(0u16, |a, &d| a.wrapping_mul(10).wrapping_add((d - b'0') as u16)));
            }
        }
        Name::Str(s.to_ascii_uppercase())
    }
}

pub struct Image {
    file: Vec<u8>,
    /// The resource table's offset in the file (0: none).
    table: usize,
    shift: u32,
}

/// Resources handed out by FindResource (HRSRC = index + 1): the module,
/// offset and length in its file.
static FOUND: Global<Vec<(u16, usize, usize)>> = Global::new(Vec::new());

/// The resources of the module with instance `hinst` (0: the program).
fn image(hinst: u16) -> Option<&'static Image> {
    let m = crate::module::find(hinst)?;
    if m.image.is_none() {
        let img = crate::module::path_of(m.hmod).and_then(|p| read_image(&p));
        m.image = Some(img.unwrap_or(Image { file: Vec::new(), table: 0, shift: 0 }));
    }
    m.image.as_ref().filter(|i| i.table != 0)
}

fn read_image(path: &[u8]) -> Option<Image> {
    let file = crate::module::read_file(path, usize::MAX)?;
    let w = |o: usize| file.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    let ne = file.get(0x3c..0x40).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) as usize;
    if w(ne) != 0x454e {
        return None;
    }
    let (rt, rn) = (w(ne + 0x24) as usize, w(ne + 0x26) as usize);
    if rt == rn {
        return Some(Image { file, table: 0, shift: 0 });
    }
    let table = ne + rt;
    let shift = w(table) as u32;
    Some(Image { file, table, shift })
}

impl Image {
    fn w(&self, o: usize) -> u16 {
        self.file.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// A type or resource ID: a number, or a Pascal string in the table.
    fn name_is(&self, id: u16, n: &Name) -> bool {
        match n {
            Name::Id(i) => id & 0x8000 != 0 && id & 0x7fff == *i,
            Name::Str(s) => {
                if id & 0x8000 != 0 {
                    return false;
                }
                let o = self.table + id as usize;
                let len = self.file.get(o).copied().unwrap_or(0) as usize;
                self.file.get(o + 1..o + 1 + len).is_some_and(|t| t.eq_ignore_ascii_case(s))
            }
        }
    }

    /// The resource's (offset, length) in the file.
    fn find(&self, ty: &Name, name: &Name) -> Option<(usize, usize)> {
        self.find_direct(ty, name).or_else(|| {
            // Windows 3.0 programs name resources in an RT_NAMETABLE (15):
            // entries of a length, the type and the resource's IDs, the
            // type's name and the resource's name.
            let (Name::Str(n), Name::Id(t)) = (name, ty) else { return None };
            let (off, len) = self.find_direct(&Name::Id(15), &Name::Id(1)).or_else(|| self.first_of(15))?;
            let tab = &self.file[off..off + len];
            let mut p = 0;
            while p + 6 < tab.len() {
                let n_len = w16(tab, p) as usize;
                if n_len < 7 {
                    break;
                }
                let (ety, eid) = (w16(tab, p + 2), w16(tab, p + 4));
                let mut q = p + 6;
                let _ty_name = sz(tab, &mut q);
                let id_name = sz(tab, &mut q);
                if ety & 0x7fff == *t && id_name.eq_ignore_ascii_case(n) {
                    return self.find_direct(ty, &Name::Id(eid & 0x7fff));
                }
                p += n_len;
            }
            None
        })
    }

    /// The first resource of a type.
    fn first_of(&self, ty: u16) -> Option<(usize, usize)> {
        let mut p = self.table + 2;
        loop {
            let tid = self.w(p);
            if tid == 0 || p >= self.file.len() {
                return None;
            }
            let count = self.w(p + 2) as usize;
            if tid == 0x8000 | ty && count > 0 {
                let off = (self.w(p + 8) as usize) << self.shift;
                let len = (self.w(p + 10) as usize) << self.shift;
                return (off + len <= self.file.len()).then_some((off, len));
            }
            p += 8 + 12 * count;
        }
    }

    fn find_direct(&self, ty: &Name, name: &Name) -> Option<(usize, usize)> {
        let mut p = self.table + 2;
        loop {
            let tid = self.w(p);
            if tid == 0 || p >= self.file.len() {
                return None;
            }
            let count = self.w(p + 2) as usize;
            p += 8;
            let this_type = self.name_is(tid, ty);
            for _ in 0..count {
                if this_type && self.name_is(self.w(p + 6), name) {
                    let off = (self.w(p) as usize) << self.shift;
                    let len = (self.w(p + 2) as usize) << self.shift;
                    return (off + len <= self.file.len()).then_some((off, len));
                }
                p += 12;
            }
        }
    }
}

/// The resources of one type in an NE file's bytes: (offset, length) in
/// table order.
pub fn list_type(file: &[u8], ty: u16) -> Vec<(usize, usize)> {
    let Some(img) = table_of(file) else { return Vec::new() };
    let mut v = Vec::new();
    let mut p = img.0 + 2;
    let rd = |o: usize| w16(file, o);
    loop {
        let tid = rd(p);
        if tid == 0 || p >= file.len() {
            return v;
        }
        let count = rd(p + 2) as usize;
        p += 8;
        for _ in 0..count {
            if tid == 0x8000 | ty {
                let off = (rd(p) as usize) << img.1;
                let len = (rd(p + 2) as usize) << img.1;
                if off + len <= file.len() {
                    v.push((off, len));
                }
            }
            p += 12;
        }
    }
}

/// A resource in an NE file's bytes: (offset, length).
pub fn find_in_file(file: &[u8], ty: u16, name: &Name) -> Option<(usize, usize)> {
    let (table, shift) = table_of(file)?;
    let img = Image { file: file.to_vec(), table, shift };
    img.find(&Name::Id(ty), name)
}

/// The resource table's offset and alignment shift in an NE file.
fn table_of(file: &[u8]) -> Option<(usize, u32)> {
    let ne = l32(file, 0x3c) as usize;
    if w16(file, ne) != 0x454e {
        return None;
    }
    let (rt, rn) = (w16(file, ne + 0x24) as usize, w16(file, ne + 0x26) as usize);
    if rt == rn {
        return None;
    }
    Some((ne + rt, w16(file, ne + rt) as u32))
}

/// The bytes of a resource of the module `hinst` (0: the program).
pub fn find(hinst: u16, ty: u16, name: &Name) -> Option<&'static [u8]> {
    let img = image(hinst)?;
    let (off, len) = img.find(&Name::Id(ty), name)?;
    Some(&img.file[off..off + len])
}

/// LoadString: the string's length (0 if none).
fn load_string(hinst: u16, id: u16, buf: *mut u8, max: i32) -> i32 {
    if buf.is_null() || max <= 0 {
        return 0;
    }
    let Some(block) = find(hinst, RT_STRING, &Name::Id((id >> 4) + 1)) else {
        unsafe { *buf = 0 };
        return 0;
    };
    let mut p = 0;
    for _ in 0..(id & 15) {
        p += 1 + block.get(p).copied().unwrap_or(0) as usize;
    }
    let len = block.get(p).copied().unwrap_or(0) as usize;
    let s = block.get(p + 1..p + 1 + len).unwrap_or(&[]);
    crate::user::copy_out(s, buf, max) as i32
}

/// The KERNEL resource calls (FindResource 60 .. SizeofResource 65).
pub fn kernel(ord: u32, a: &mut Pascal) -> u32 {
    match ord {
        // FindResource(hinst, name, type): HRSRC.
        60 => {
            let (hi, n, t) = (a.w(), a.l(), a.l());
            let (Some(n), Some(t)) = (Name::of(n), Name::of(t)) else { return 0 };
            let Some(img) = image(hi) else { return 0 };
            let Some((off, len)) = img.find(&t, &n) else { return 0 };
            let r = (hi, off, len);
            let f = FOUND.get();
            let i = f.iter().position(|&x| x == r).unwrap_or_else(|| {
                f.push(r);
                f.len() - 1
            });
            i as u32 + 1
        }
        // LoadResource(hinst, hrsrc): a global block with a copy.
        61 => {
            let (_hi, r) = (a.w(), a.w());
            let Some(&(hi, off, len)) = FOUND.get().get((r as usize).wrapping_sub(1)) else { return 0 };
            let Some(img) = image(hi) else { return 0 };
            let h = crate::kernel::global_alloc(0, len as u32);
            if h != 0 {
                let p = thunk16::flat((h as u32) << 16);
                unsafe { core::ptr::copy_nonoverlapping(img.file[off..].as_ptr(), p, len) };
            }
            h as u32
        }
        // AccessResource(hinst, hrsrc): the module's file, open at the
        // resource (the program reads it with _lread).
        64 => {
            let (_hi, r) = (a.w(), a.w());
            let Some(&(hi, off, _)) = FOUND.get().get((r as usize).wrapping_sub(1)) else { return 0xffff };
            let Some(mut path) = crate::module::find(hi).and_then(|m| crate::module::path_of(m.hmod)) else { return 0xffff };
            path.push(0);
            let (mut hf, mut action, mut pos) = (0u32, 0u32, 0u32);
            if unsafe { os2::DosOpen(path.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x40, core::ptr::null_mut()) } != 0 {
                return 0xffff;
            }
            unsafe { os2::DosSetFilePtr(hf, off as i32, 0, &mut pos) };
            hf
        }
        // LockResource(hglobal): its address.
        62 => {
            let h = a.w();
            if h == 0 { 0 } else { ((h | 7) as u32) << 16 }
        }
        // FreeResource(hglobal)
        63 => {
            let h = a.w();
            crate::kernel::global_free(h);
            0
        }
        // SizeofResource(hinst, hrsrc)
        65 => {
            let (_hi, r) = (a.w(), a.w());
            FOUND.get().get((r as usize).wrapping_sub(1)).map_or(0, |&(_, _, len)| len as u32)
        }
        _ => 0,
    }
}

/// USER's LoadString(hinst, id, buf, max).
pub fn user_load_string(a: &mut Pascal) -> u32 {
    let (hi, id, buf, max) = (a.w(), a.w(), a.p(), a.i());
    load_string(hi, id, buf, max) as u32
}

/// Read a NUL-terminated string at `*p` of `b` (moving past it).
pub fn sz<'a>(b: &'a [u8], p: &mut usize) -> &'a [u8] {
    let start = (*p).min(b.len());
    let n = b[start..].iter().position(|&c| c == 0).unwrap_or(b.len() - start);
    *p = start + n + 1;
    &b[start..start + n]
}

pub fn w16(b: &[u8], p: usize) -> u16 {
    b.get(p..p + 2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]))
}

pub fn l32(b: &[u8], p: usize) -> u32 {
    b.get(p..p + 4).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}
