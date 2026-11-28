//! Windows 3.x resources in their Win32 forms, for W32USER: dialog
//! templates, menu templates, string tables and accelerator tables (the
//! rest, icons, cursors and bitmaps, are the same in both). Text goes to
//! UTF-16 through W32USER's own table (Rs2W32Widen), which W32USER
//! narrows back to the same bytes.

use alloc::vec::Vec;

use crate::res::{w16, Name};

/// Bytes of the process code page in UTF-16.
pub fn widen(s: &[u8]) -> Vec<u16> {
    let mut w = alloc::vec![0u16; s.len()];
    if !s.is_empty() {
        unsafe { crate::w32::Rs2W32Widen(s.as_ptr() as u32, s.len() as u32, w.as_mut_ptr() as u32) };
    }
    w
}

/// Dialog items' creation data in 16-bit memory, kept (once for each
/// different one) for the controls of the dialogs made from them.
static CREATION: crate::Global<Vec<(Vec<u8>, u32)>> = crate::Global::new(Vec::new());

fn creation_data(data: &[u8]) -> Option<u32> {
    if data.is_empty() {
        return None;
    }
    if let Some(e) = CREATION.get().iter().find(|e| e.0 == data) {
        return Some(e.1);
    }
    let h = crate::kernel::global_alloc(0, data.len() as u32);
    if h == 0 {
        return None;
    }
    let far_ = ((h | 7) as u32) << 16;
    let q = thunk16::flat(far_);
    if q.is_null() {
        return None;
    }
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), q, data.len()) };
    CREATION.get().push((data.to_vec(), far_));
    Some(far_)
}

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

/// A NUL-terminated UTF-16 string.
fn put_wstr(b: &mut Vec<u8>, s: &[u8]) {
    for c in widen(s) {
        put16(b, c);
    }
    put16(b, 0);
}

fn align4(b: &mut Vec<u8>) {
    while b.len() % 4 != 0 {
        b.push(0);
    }
}

/// A NUL-terminated byte string at `*p` (moved past it).
fn sz<'a>(t: &'a [u8], p: &mut usize) -> &'a [u8] {
    let s = &t[(*p).min(t.len())..];
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    *p += n + 1;
    &s[..n]
}

/// A Win16 dialog template as a Win32 DLGTEMPLATE: style, item count,
/// position, menu, class, caption and (DS_SETFONT) the font; then the
/// items, each DWORD-aligned.
pub fn dialog(t: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(t.len() * 2 + 64);
    if t.len() < 13 {
        return o;
    }
    let style = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
    let n = t[4];
    put32(&mut o, style);
    put32(&mut o, 0);
    put16(&mut o, n as u16);
    for i in 0..4 {
        put16(&mut o, w16(t, 5 + 2 * i));
    }
    let mut p = 13;
    // The menu: none, 0xFF and an ID, or a name.
    match t.get(p) {
        Some(0xff) => {
            put16(&mut o, 0xffff);
            put16(&mut o, w16(t, p + 1));
            p += 3;
        }
        _ => {
            let s = sz(t, &mut p);
            put_wstr(&mut o, s);
        }
    }
    let class = sz(t, &mut p);
    put_wstr(&mut o, class);
    let caption = sz(t, &mut p);
    put_wstr(&mut o, caption);
    // DS_SETFONT
    if style & 0x40 != 0 {
        put16(&mut o, w16(t, p));
        p += 2;
        let face = sz(t, &mut p);
        put_wstr(&mut o, face);
    }
    for _ in 0..n {
        if p + 14 > t.len() {
            break;
        }
        align4(&mut o);
        let (x, y, cx, cy, id) = (w16(t, p), w16(t, p + 2), w16(t, p + 4), w16(t, p + 6), w16(t, p + 8));
        let st = u32::from_le_bytes([t[p + 10], t[p + 11], t[p + 12], t[p + 13]]);
        p += 14;
        put32(&mut o, st);
        put32(&mut o, 0);
        for v in [x, y, cx, cy, id] {
            put16(&mut o, v);
        }
        // The class: a predefined one (0x80 BUTTON ... 0x85 COMBOBOX) or
        // a name.
        match t.get(p) {
            Some(&c) if c & 0x80 != 0 => {
                put16(&mut o, 0xffff);
                put16(&mut o, c as u16);
                p += 1;
            }
            _ => {
                let s = sz(t, &mut p);
                put_wstr(&mut o, s);
            }
        }
        // The text: 0xFF and an ID (an icon's), or a string.
        match t.get(p) {
            Some(0xff) => {
                put16(&mut o, 0xffff);
                put16(&mut o, w16(t, p + 1));
                p += 3;
            }
            _ => {
                let s = sz(t, &mut p);
                put_wstr(&mut o, s);
            }
        }
        // The creation data: a byte count and the bytes, which Windows
        // gives the control as lpCreateParams (Borland's BorBtnII buttons
        // find their bitmaps in it). W32USER gets the 16:16 address of a
        // copy in 16-bit memory as the item's 4 bytes of creation data.
        let k = t.get(p).copied().unwrap_or(0) as usize;
        p += 1;
        let data = &t[p.min(t.len())..(p + k).min(t.len())];
        p += k;
        match creation_data(data) {
            Some(far_) => {
                put16(&mut o, 4);
                put32(&mut o, far_);
            }
            None => put16(&mut o, 0),
        }
    }
    o
}

/// A Win16 menu template as a Win32 one (the same, with UTF-16 text).
/// Windows 2.x's templates have no header and their flags are a byte:
/// their first byte is the first item's flags (a pop-up's MF_POPUP),
/// never 3.x's version 0.
pub fn menu(t: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(t.len() * 2);
    if t.len() < 4 {
        return o;
    }
    put16(&mut o, 0);
    put16(&mut o, 0);
    let old = t[0] != 0;
    let mut p = if old { 0 } else { 4 + w16(t, 2) as usize };
    menu_level(t, &mut p, &mut o, 0, old);
    o
}

/// One level of a menu: items until the one with MF_END (a pop-up's
/// items follow it, a level of their own).
fn menu_level(t: &[u8], p: &mut usize, o: &mut Vec<u8>, depth: u32, old: bool) {
    while *p + 2 <= t.len() && depth < 16 {
        let fl = if old { t[*p] as u16 } else { w16(t, *p) };
        *p += if old { 1 } else { 2 };
        put16(o, fl);
        let popup = fl & 0x10 != 0;
        if !popup {
            put16(o, w16(t, *p));
            *p += 2;
        }
        let s = sz(t, p);
        put_wstr(o, s);
        if popup {
            menu_level(t, p, o, depth + 1, old);
        }
        if fl & 0x80 != 0 {
            return;
        }
    }
}

/// A Win16 string table block (16 strings: a length byte, the bytes) as
/// Win32's (a length word, UTF-16).
pub fn strings(t: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(t.len() * 2 + 32);
    let mut p = 0;
    for _ in 0..16 {
        let n = t.get(p).copied().unwrap_or(0) as usize;
        p += 1;
        let s = &t[p.min(t.len())..(p + n).min(t.len())];
        p += n;
        put16(&mut o, s.len() as u16);
        for c in widen(s) {
            put16(&mut o, c);
        }
    }
    o
}

/// A Win16 accelerator table (flags byte, key, command; 0x80 on the
/// last) as Win32's resource form (flags, key, command, padding words).
pub fn accel(t: &[u8]) -> Vec<u8> {
    let mut o = Vec::new();
    let mut p = 0;
    while p + 5 <= t.len() {
        let fl = t[p];
        put16(&mut o, fl as u16);
        put16(&mut o, w16(t, p + 1));
        put16(&mut o, w16(t, p + 3));
        put16(&mut o, 0);
        p += 5;
        if fl & 0x80 != 0 {
            break;
        }
    }
    o
}

// ---- W32USER's resource lookups (kern.rs `resource`) ----

/// Converted resources, kept (W32USER holds on to what it is given).
static CACHE: crate::Global<Vec<((u16, u16, Name), Vec<u8>)>> = crate::Global::new(Vec::new());

/// The host's resource call: a resource of an NE module in Win32's form.
pub unsafe extern "C" fn host_resource(hinst: u32, ty: u32, name: u32, len: *mut u32) -> u32 {
    let name = if name >> 16 == 0 {
        Name::Id(name as u16)
    } else {
        // A UTF-16 name: its bytes (W32USER passes upper case).
        let mut s = Vec::new();
        let mut q = name as *const u16;
        loop {
            let c = unsafe { q.read_unaligned() };
            if c == 0 {
                break;
            }
            s.push(if c < 0x100 { c as u8 } else { b'?' });
            q = unsafe { q.add(1) };
        }
        Name::from_bytes(&s)
    };
    let (hinst, ty) = (hinst as u16, ty as u16);
    let Some(raw) = crate::res::find(hinst, ty, &name) else { return 0 };
    let conv = match ty {
        4 => Some(menu as fn(&[u8]) -> Vec<u8>),
        5 => Some(dialog as fn(&[u8]) -> Vec<u8>),
        6 => Some(strings as fn(&[u8]) -> Vec<u8>),
        9 => Some(accel as fn(&[u8]) -> Vec<u8>),
        _ => None,
    };
    let Some(conv) = conv else {
        unsafe { *len = raw.len() as u32 };
        return raw.as_ptr() as u32;
    };
    let key = (hinst, ty, name);
    let cache = CACHE.get();
    if let Some((_, v)) = cache.iter().find(|(k, _)| *k == key) {
        unsafe { *len = v.len() as u32 };
        return v.as_ptr() as u32;
    }
    let v = conv(raw);
    unsafe { *len = v.len() as u32 };
    let p = v.as_ptr() as u32;
    cache.push((key, v));
    p
}
