//! Icons and cursors of programs: from their RT_GROUP_ICON/RT_ICON and
//! RT_GROUP_CURSOR/RT_CURSOR resources (LoadIcon, LoadCursor,
//! ExtractIcon) or made with CreateIcon. An icon is a monochrome AND mask
//! and a colour XOR image (GDI bitmaps, bitmap.rs), drawn by DrawIcon as
//! Windows draws them (the mask with SRCAND, the image with SRCINVERT).
//! The system icons and cursors (IDI_*, IDC_*) keep their IDs as handles
//! and are rs2's own art (ctl.rs draw_icon, PM's system pointers).

use alloc::vec::Vec;

use crate::gdi;
use crate::res::{self, Name};
use crate::Global;

use crate::res::{RT_CURSOR, RT_GROUP_CURSOR, RT_GROUP_ICON, RT_ICON};

const SRCAND: u32 = 0x0088_00c6;
const SRCINVERT: u32 = 0x0066_0046;

pub struct Icon {
    pub w: i32,
    pub h: i32,
    pub hot: (i32, i32),
    /// The AND mask and the XOR image (GDI bitmap handles).
    pub and: u16,
    pub xor: u16,
    pub cursor: bool,
    /// The PM pointer made for SetCursor (0: not yet).
    pub pm: u32,
}

/// Icons by handle: 0xE000 + 2 * index (no selector, window or GDI
/// handle is in that range).
static ICONS: Global<Vec<Option<Icon>>> = Global::new(Vec::new());
const BASE: u16 = 0xe000;

pub fn get(h: u32) -> Option<&'static Icon> {
    if h > 0xffff {
        return None;
    }
    let i = (h as u16).checked_sub(BASE)? as usize / 2;
    ICONS.get().get(i)?.as_ref()
}

pub fn add(i: Icon) -> u16 {
    let v = ICONS.get();
    let k = match v.iter().position(|x| x.is_none()) {
        Some(k) => {
            v[k] = Some(i);
            k
        }
        None => {
            v.push(Some(i));
            v.len() - 1
        }
    };
    BASE + 2 * k as u16
}

pub fn destroy(h: u32) -> bool {
    if h > 0xffff {
        return false;
    }
    let Some(i) = (h as u16).checked_sub(BASE).map(|i| i as usize / 2) else { return false };
    let Some(icon) = ICONS.get().get_mut(i).and_then(|x| x.take()) else { return false };
    gdi::delete_object(icon.and);
    gdi::delete_object(icon.xor);
    true
}

fn rd16(b: &[u8], o: usize) -> u16 {
    res::w16(b, o)
}

/// An icon or cursor image (a DIB whose height counts the XOR image and
/// the AND mask, the XOR image first) as an icon.
pub fn from_image(dib: &[u8], hot: (i32, i32), cursor: bool) -> u16 {
    if dib.len() < 40 {
        return 0;
    }
    let cb = res::l32(dib, 0) as usize;
    if cb < 40 {
        return 0;
    }
    let w = res::l32(dib, 4) as i32;
    let h = res::l32(dib, 8) as i32 / 2;
    let bpp = rd16(dib, 14);
    let used = res::l32(dib, 32) as usize;
    let colors = if bpp > 8 { 0 } else if used != 0 { used } else { 1 << bpp };
    let hdr_len = cb + colors * 4;
    let xor_len = (w as usize * bpp as usize).div_ceil(32) * 4 * h as usize;
    let and_len = (w as usize).div_ceil(32) * 4 * h as usize;
    if w <= 0 || h <= 0 || hdr_len + xor_len > dib.len() {
        return 0;
    }
    // The XOR image: the header with the real height.
    let mut x = dib[..hdr_len + xor_len].to_vec();
    x[8..12].copy_from_slice(&(h as u32).to_le_bytes());
    x[20..24].copy_from_slice(&0u32.to_le_bytes());
    // The AND mask: a 1-bit DIB, black and white.
    let mut m = Vec::with_capacity(48 + and_len);
    m.extend_from_slice(&40u32.to_le_bytes());
    m.extend_from_slice(&(w as u32).to_le_bytes());
    m.extend_from_slice(&(h as u32).to_le_bytes());
    m.extend_from_slice(&1u16.to_le_bytes());
    m.extend_from_slice(&1u16.to_le_bytes());
    m.extend_from_slice(&[0; 24]);
    m.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0]);
    let at = hdr_len + xor_len;
    match dib.get(at..at + and_len) {
        Some(b) => m.extend_from_slice(b),
        None => m.resize(m.len() + and_len, 0),
    }
    let xor = crate::bitmap::from_dib(&x);
    let and = crate::bitmap::from_dib(&m);
    if xor == 0 || and == 0 {
        return 0;
    }
    add(Icon { w, h, hot, and, xor, cursor, pm: 0 })
}

/// The best image of an icon or cursor group (the one nearest 32 x 32
/// with the most colours up to 256): its resource ID.
fn pick(dir: &[u8], cursor: bool) -> Option<u16> {
    let n = rd16(dir, 4) as usize;
    let mut best: Option<(i32, u16)> = None;
    for k in 0..n {
        let e = 6 + 14 * k;
        let (w, colors, bits) = if cursor {
            (rd16(dir, e) as i32, 2, rd16(dir, e + 6) as i32)
        } else {
            (dir.get(e).copied().unwrap_or(0) as i32, dir.get(e + 2).copied().unwrap_or(0) as i32, rd16(dir, e + 6) as i32)
        };
        let w = if w == 0 { 256 } else { w };
        let depth = if bits > 0 { bits } else if colors == 0 { 8 } else { (colors as u32).trailing_zeros() as i32 };
        let score = -(w - 32).abs() * 100 + if depth <= 8 { depth } else { -depth };
        let id = rd16(dir, e + 12);
        if best.is_none_or(|b| score > b.0) {
            best = Some((score, id));
        }
    }
    best.map(|b| b.1)
}

/// The PM pointer of a cursor or icon (made when first set): its AND
/// mask over its image, as PM's monochrome pointers are, with the hot
/// spot from the bottom.
pub fn pm_pointer(h: u32) -> u32 {
    if h > 0xffff {
        return 0;
    }
    let Some(i) = (h as u16).checked_sub(BASE).map(|i| i as usize / 2) else { return 0 };
    let Some(Some(ic)) = ICONS.get().get(i) else { return 0 };
    if ic.pm != 0 {
        return ic.pm;
    }
    let (and, xor, hot, ih) = (ic.and, ic.xor, ic.hot, ic.h);
    let (Some((w, h1, a)), Some((_, h2, x))) = (crate::bitmap::mono_rows(and), crate::bitmap::mono_rows(xor)) else { return 0 };
    if h1 != h2 {
        return 0;
    }
    // PM's pointer bitmap: the XOR image below, the AND mask above.
    let mut bits = x;
    bits.extend_from_slice(&a);
    let hbm = crate::bitmap::pm_bitmap_from(w, 2 * h1, 1, &bits, &[0, 0xffffff]);
    if hbm == 0 {
        return 0;
    }
    let p = unsafe { crate::pm::WinCreatePointer(crate::pm::HWND_DESKTOP, hbm, 1, hot.0, ih - 1 - hot.1) };
    unsafe { crate::pm::GpiDeleteBitmap(hbm) };
    if let Some(Some(ic)) = ICONS.get().get_mut(i) {
        ic.pm = p;
    }
    p
}

/// LoadIcon / LoadCursor from a module's resources.
pub fn load(hinst: u32, name: &Name, cursor: bool) -> u16 {
    let (gt, it) = if cursor { (RT_GROUP_CURSOR, RT_CURSOR) } else { (RT_GROUP_ICON, RT_ICON) };
    let Some(dir) = res::find(hinst, gt, name) else { return 0 };
    let Some(id) = pick(dir, cursor) else { return 0 };
    let Some(img) = res::find(hinst, it, &Name::Id(id)) else { return 0 };
    if cursor {
        // A cursor image starts with its hot spot.
        let hot = (rd16(img, 0) as i32, rd16(img, 2) as i32);
        img.get(4..).map_or(0, |d| from_image(d, hot, true))
    } else {
        from_image(img, (0, 0), false)
    }
}

/// CreateIcon(hinst, w, h, planes, bpp, and bits, xor bits): DDB bits.
pub fn create(w: i32, h: i32, planes: u16, bpp: u16, and: *const u8, xor: *const u8) -> u16 {
    if w <= 0 || h <= 0 {
        return 0;
    }
    let a = crate::bitmap::create_ddb(w, h, 1, and);
    let x = crate::bitmap::create_ddb(w, h, planes * bpp, xor);
    if a == 0 || x == 0 {
        return 0;
    }
    add(Icon { w, h, hot: (w / 2, h / 2), and: a, xor: x, cursor: false, pm: 0 })
}

/// DrawIcon(hdc, x, y, hicon)
pub fn draw(hdc: u16, x: i32, y: i32, h: u32) -> bool {
    let Some(i) = get(h) else {
        system_icon(hdc, x, y, h);
        return true;
    };
    let (w, ih, and, xor) = (i.w, i.h, i.and, i.xor);
    let mem = crate::bitmap::create_memory_dc();
    let (text, bk) = (gdi::set_text_color(hdc, 0), gdi::set_bk_color(hdc, 0xffffff));
    crate::bitmap::select(mem, and);
    crate::bitmap::blit(hdc, x, y, w, ih, mem, 0, 0, SRCAND);
    crate::bitmap::select(mem, xor);
    crate::bitmap::blit(hdc, x, y, w, ih, mem, 0, 0, SRCINVERT);
    gdi::set_text_color(hdc, text);
    gdi::set_bk_color(hdc, bk);
    crate::bitmap::deselect(mem);
    gdi::release_dc(mem);
    true
}

/// The PM system pointer of a built-in cursor (IDC_*).
pub fn system_pointer(c: u32) -> u32 {
    match c {
        32513 => 2,         // IDC_IBEAM: SPTR_TEXT
        32514 | 32650 => 3, // IDC_WAIT, IDC_APPSTARTING
        32640 | 32646 => 5, // IDC_SIZE, IDC_SIZEALL: SPTR_MOVE
        32642 => 6,         // IDC_SIZENWSE
        32643 => 7,         // IDC_SIZENESW
        32644 => 8,         // IDC_SIZEWE
        32645 => 9,         // IDC_SIZENS
        32648 => 10,        // IDC_NO: SPTR_ILLEGAL
        _ => 1,             // IDC_ARROW and the rest
    }
}

/// The built-in icons (IDI_*): rs2's own drawing, 32 x 32.
pub fn system_icon(hdc: u16, x: i32, y: i32, id: u32) {
    let (bg, mark): (u32, &[u8]) = match id {
        32513 => (0x0000ff, b"X"), // IDI_HAND
        32514 => (0xff0000, b"?"), // IDI_QUESTION
        32515 => (0x00ffff, b"!"), // IDI_EXCLAMATION
        32516 => (0xff0000, b"i"), // IDI_ASTERISK
        _ => (0xc0c0c0, b""),      // IDI_APPLICATION, IDI_WINLOGO
    };
    gdi::fill(hdc, [x + 2, y + 2, x + 30, y + 30], bg);
    gdi::frame(hdc, [x + 2, y + 2, x + 30, y + 30], 0);
    if !mark.is_empty() {
        let old = gdi::set_bk_mode(hdc, gdi::TRANSPARENT);
        let oc = gdi::set_text_color(hdc, 0xffffff);
        gdi::draw_text_bytes(hdc, mark, [x + 2, y + 2, x + 30, y + 30], gdi::DT_CENTER | gdi::DT_VCENTER | gdi::DT_SINGLELINE | gdi::DT_NOPREFIX);
        gdi::set_text_color(hdc, oc);
        gdi::set_bk_mode(hdc, old);
    }
}
