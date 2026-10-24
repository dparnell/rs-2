//! Resources: loading with DosGetResource and the binary formats of menu
//! templates, dialog templates, accelerator and string tables.

use alloc::vec::Vec;

pub const RT_BITMAP: u32 = 2;
pub const RT_MENU: u32 = 3;
pub const RT_DIALOG: u32 = 4;
pub const RT_STRING: u32 = 5;
pub const RT_ACCELTABLE: u32 = 8;
pub const RT_MESSAGE: u32 = 10;

unsafe extern "C" {
    #[link_name = "DOSCALLS.352"]
    fn DosGetResource(hmod: u32, ty: u32, id: u32, ppb: *mut *const u8) -> u32;
    #[link_name = "DOSCALLS.572"]
    fn DosQueryResourceSize(hmod: u32, ty: u32, id: u32, pcb: *mut u32) -> u32;
}

/// A resource of the module `hmod` (0: the executable). Resources stay
/// loaded for the life of the process.
pub fn load(hmod: u32, ty: u32, id: u32) -> Option<&'static [u8]> {
    let mut p: *const u8 = core::ptr::null();
    let mut cb = 0;
    unsafe {
        if DosGetResource(hmod, ty, id, &mut p) != 0 || p.is_null() {
            return None;
        }
        if DosQueryResourceSize(hmod, ty, id, &mut cb) != 0 {
            cb = 0;
        }
        Some(core::slice::from_raw_parts(p, cb as usize))
    }
}

pub fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u16_at(b, at) as u32 | (u16_at(b, at + 2) as u32) << 16
}

/// NUL-terminated string at `at`, and the offset after its NUL.
fn string_at(b: &[u8], at: usize) -> (Vec<u8>, usize) {
    let rest = b.get(at..).unwrap_or(&[]);
    let n = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
    (rest[..n].to_vec(), at + n + 1)
}

/// String `id` of a string or message table: bundles of 16 entries
/// (resource id = id / 16 + 1): a code page word, then for each entry a
/// length byte (including the NUL) and the text.
pub fn string(hmod: u32, ty: u32, id: u32) -> Option<Vec<u8>> {
    let b = load(hmod, ty, id / 16 + 1)?;
    let mut at = 2;
    for _ in 0..id % 16 {
        at += 1 + *b.get(at)? as usize;
    }
    let len = *b.get(at)? as usize;
    let text = b.get(at + 1..at + 1 + len.saturating_sub(1))?;
    Some(text[..text.iter().position(|&c| c == 0).unwrap_or(text.len())].to_vec())
}

// ---- Menu templates ----

/// A menu item of a template, with its submenu.
pub struct MenuItemT {
    pub style: u16,
    pub attr: u16,
    pub id: u16,
    pub text: Vec<u8>,
    pub sub: Option<Vec<MenuItemT>>,
}

pub const MIS_TEXT: u16 = 0x0001;
pub const MIS_BITMAP: u16 = 0x0002;
pub const MIS_SUBMENU: u16 = 0x0010;

/// Parse a menu template: cb, version, code page, reserved, item count,
/// then the items (style, attribute, id, text for MIS_TEXT/MIS_BITMAP,
/// and a nested template for MIS_SUBMENU).
pub fn parse_menu(b: &[u8]) -> Option<Vec<MenuItemT>> {
    let cb = (u16_at(b, 0) as usize).min(b.len());
    let b = &b[..cb];
    let count = u16_at(b, 8) as usize;
    let mut at = 10;
    let mut items = Vec::new();
    for _ in 0..count {
        if at + 6 > b.len() {
            break;
        }
        let (style, attr, id) = (u16_at(b, at), u16_at(b, at + 2), u16_at(b, at + 4));
        at += 6;
        let mut text = Vec::new();
        if style & (MIS_TEXT | MIS_BITMAP) != 0 {
            (text, at) = string_at(b, at);
        }
        let mut sub = None;
        if style & MIS_SUBMENU != 0 {
            let rest = b.get(at..)?;
            sub = Some(parse_menu(rest)?);
            at += (u16_at(rest, 0) as usize).max(10);
        }
        items.push(MenuItemT { style, attr, id, text, sub });
    }
    Some(items)
}

// ---- Dialog templates ----

/// One window of a dialog template (the first is the dialog itself).
pub struct DlgItemT {
    /// Class name as WinCreateWindow takes it ("#n" for WC_* classes).
    pub class: Vec<u8>,
    pub text: Vec<u8>,
    pub style: u32,
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
    pub id: u32,
    /// Control data (null when there is none).
    pub ctl: *const u8,
    /// Presentation parameters (PRESPARAMS; null when there are none).
    pub pres: *const u8,
    pub children: Vec<DlgItemT>,
}

/// Parse a dialog template: a header (cbTemplate, type, codepage,
/// offadlgti, fsTemplateStatus, iItemFocus, coffPresParams) and an array
/// of 30-byte items, each followed by its children; strings and control
/// data are at offsets from the start of the template.
pub fn parse_dialog(b: &[u8]) -> Option<DlgItemT> {
    let first = u16_at(b, 6) as usize;
    let mut at = first;
    item(b, &mut at)
}

fn item(b: &[u8], at: &mut usize) -> Option<DlgItemT> {
    let p = *at;
    if p + 30 > b.len() {
        return None;
    }
    *at += 30;
    let children = u16_at(b, p + 2) as usize;
    let (ccls, ocls) = (u16_at(b, p + 4) as usize, u16_at(b, p + 6) as usize);
    let (ctext, otext) = (u16_at(b, p + 8) as usize, u16_at(b, p + 10) as usize);
    let class = if ccls == 0 { alloc::format!("#{ocls}").into_bytes() } else { b.get(ocls..ocls + ccls)?.to_vec() };
    let text = if ctext == 0 { Vec::new() } else { b.get(otext..otext + ctext)?.to_vec() };
    // Icon and bitmap statics have 0xFF and a resource id as their text.
    let text = if text.first() == Some(&0xff) { text } else { text[..text.iter().position(|&c| c == 0).unwrap_or(text.len())].to_vec() };
    let octl = u16_at(b, p + 28) as usize;
    let ctl = if octl == 0xffff || octl >= b.len() { core::ptr::null() } else { b[octl..].as_ptr() };
    let opres = u16_at(b, p + 26) as usize;
    let pres = if opres == 0xffff || opres == 0 || opres >= b.len() { core::ptr::null() } else { b[opres..].as_ptr() };
    let mut d = DlgItemT {
        class,
        text,
        style: u32_at(b, p + 12),
        x: u16_at(b, p + 16) as i16 as i32,
        y: u16_at(b, p + 18) as i16 as i32,
        cx: u16_at(b, p + 20) as i16 as i32,
        cy: u16_at(b, p + 22) as i16 as i32,
        id: u16_at(b, p + 24) as u32,
        ctl,
        pres,
        children: Vec::new(),
    };
    for _ in 0..children {
        d.children.push(item(b, at)?);
    }
    Some(d)
}

// ---- Accelerator tables ----

#[derive(Clone, Copy)]
pub struct Accel {
    pub fs: u16,
    pub key: u16,
    pub cmd: u16,
}

/// Parse an accelerator table: count, code page, then (fs, key, cmd).
pub fn parse_accel(b: &[u8]) -> Vec<Accel> {
    let n = u16_at(b, 0) as usize;
    (0..n).map(|i| 4 + i * 6).filter(|&o| o + 6 <= b.len()).map(|o| Accel { fs: u16_at(b, o), key: u16_at(b, o + 2), cmd: u16_at(b, o + 4) }).collect()
}
