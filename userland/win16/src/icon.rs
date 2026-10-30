//! Icons and cursors of programs: from their RT_GROUP_ICON/RT_ICON and
//! RT_GROUP_CURSOR/RT_CURSOR resources (LoadIcon, LoadCursor,
//! ExtractIcon) or made with CreateIcon. An icon is a monochrome AND mask
//! and a colour XOR image (GDI bitmaps, bitmap.rs), drawn by DrawIcon as
//! Windows draws them (the mask with SRCAND, the image with SRCINVERT).
//! The system icons and cursors (IDI_*, IDC_*) keep their IDs as handles
//! and are rs2's own art (ctl.rs draw_icon, PM's system pointers).

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::gdi;
use crate::res::{self, Name};
use crate::Global;

const RT_CURSOR: u16 = 1;
const RT_ICON: u16 = 3;
const RT_GROUP_CURSOR: u16 = 12;
const RT_GROUP_ICON: u16 = 14;

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
}

/// Icons by handle: 0xE000 + 2 * index (no selector, window or GDI
/// handle is in that range).
static ICONS: Global<Vec<Option<Icon>>> = Global::new(Vec::new());
const BASE: u16 = 0xe000;

pub fn get(h: u16) -> Option<&'static Icon> {
    let i = h.checked_sub(BASE)? as usize / 2;
    ICONS.get().get(i)?.as_ref()
}

fn add(i: Icon) -> u16 {
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

fn destroy(h: u16) -> bool {
    let Some(i) = h.checked_sub(BASE).map(|i| i as usize / 2) else { return false };
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
fn from_image(dib: &[u8], hot: (i32, i32), cursor: bool) -> u16 {
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
    add(Icon { w, h, hot, and, xor, cursor })
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

/// LoadIcon / LoadCursor from a module's resources.
pub fn load(hinst: u16, name: &Name, cursor: bool) -> u16 {
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

/// ExtractIcon(hinst, file, index): the index-th icon group of an NE
/// file (-1: how many there are).
fn extract(path: &[u8], index: u16) -> u16 {
    let Some(file) = crate::module::read_file(path, usize::MAX) else { return 0 };
    let groups = res::list_type(&file, RT_GROUP_ICON);
    if index == 0xffff {
        return groups.len() as u16;
    }
    let Some(&(off, len)) = groups.get(index as usize) else { return 0 };
    let dir = &file[off..off + len];
    let Some(id) = pick(dir, false) else { return 0 };
    let Some((o, l)) = res::find_in_file(&file, RT_ICON, &Name::Id(id)) else { return 0 };
    from_image(&file[o..o + l], (0, 0), false)
}

/// CreateIcon(hinst, w, h, planes, bpp, and bits, xor bits): DDB bits.
fn create(w: i32, h: i32, planes: u16, bpp: u16, and: *const u8, xor: *const u8) -> u16 {
    if w <= 0 || h <= 0 {
        return 0;
    }
    let a = crate::bitmap::create_ddb(w, h, 1, and);
    let x = crate::bitmap::create_ddb(w, h, planes * bpp, xor);
    if a == 0 || x == 0 {
        return 0;
    }
    add(Icon { w, h, hot: (w / 2, h / 2), and: a, xor: x, cursor: false })
}

/// DrawIcon(hdc, x, y, hicon)
pub fn draw(hdc: u16, x: i32, y: i32, h: u16) -> bool {
    let Some(i) = get(h) else {
        crate::ctl::draw_icon(hdc, x, y);
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

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // LoadCursor(hinst, name), LoadIcon(hinst, name)
        2173 | 2174 => {
            let (hi, n) = (a.w(), a.l());
            let cursor = ord == 2173;
            if n >> 16 == 0 && (hi == 0 || (32512..=32651).contains(&(n as u16))) {
                // The built-in ones (IDC_*, IDI_*).
                return Some(n & 0xffff);
            }
            let Some(name) = Name::of(n) else { return Some(0) };
            match load(hi, &name, cursor) {
                0 if cursor => 32512,
                0 => 0,
                h => h as u32,
            }
        }
        // DrawIcon(hdc, x, y, hicon)
        2084 => {
            let (hdc, x, y, h) = (a.w(), a.i(), a.i(), a.w());
            draw(hdc, x, y, h) as u32
        }
        // CreateIcon(hinst, w, h, planes, bpp, and, xor)
        2407 => {
            let (_hi, w, h, planes, bpp, and, xor) = (a.w(), a.i(), a.i(), a.w() & 0xff, a.w() & 0xff, a.p(), a.p());
            create(w, h, planes, bpp, and, xor) as u32
        }
        // CreateCursor(hinst, x, y, w, h, and, xor)
        2406 => {
            let (_hi, hx, hy, w, h, and, xor) = (a.w(), a.i(), a.i(), a.i(), a.i(), a.p(), a.p());
            let c = create(w, h, 1, 1, and, xor);
            if let Some(Some(i)) = ICONS.get().get_mut((c.wrapping_sub(BASE) / 2) as usize) {
                i.hot = (hx, hy);
                i.cursor = true;
            }
            c as u32
        }
        // CopyIcon(hinst, hicon), CopyCursor(hinst, hcursor)
        2368 | 2369 => {
            let (_hi, h) = (a.w(), a.w());
            let Some(i) = get(h) else { return Some(h as u32) };
            let (w, ih, hot, cursor, and, xor) = (i.w, i.h, i.hot, i.cursor, i.and, i.xor);
            let (a2, x2) = (crate::bitmap::copy_bitmap(and), crate::bitmap::copy_bitmap(xor));
            add(Icon { w, h: ih, hot, and: a2, xor: x2, cursor }) as u32
        }
        // DestroyIcon(hicon), DestroyCursor(hcursor)
        2457 | 2458 => {
            let h = a.w();
            destroy(h);
            1
        }
        // ExtractIcon(hinst, file, index)
        6534 => {
            let (_hi, f, i) = (a.w(), a.l(), a.w());
            let Some(path) = crate::far_str(f) else { return Some(0) };
            extract(path, i) as u32
        }
        _ => return None,
    })
}
