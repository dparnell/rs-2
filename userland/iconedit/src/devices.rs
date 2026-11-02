//! Predefined device forms: the displays a figure can have a form for,
//! with their icon and pointer sizes and colours. OS/2's editor keeps a
//! list it ships with and lets the user change it (Device > Edit
//! predefined); rs2's keeps the changed list in OS2.INI (ICONEDIT
//! DEVICES, one line per device).

use alloc::vec::Vec;

use crate::format::{Form, Kind};

#[derive(Clone, PartialEq, Debug)]
pub struct Device {
    pub name: Vec<u8>,
    /// The display's size in pels (0 by 0: device independent).
    pub display: (u16, u16),
    /// Its resolution (pels per inch).
    pub res: (u16, u16),
    pub icon: (u16, u16),
    pub pointer: (u16, u16),
    /// Bitmaps' size relative to the independent form, in percent.
    pub bitmap: (u16, u16),
    /// Colour bits: 1, 4, 8 or 24.
    pub bpp: u16,
    /// Part of a new icon or pointer.
    pub recommended: bool,
    /// The form keeps the colours of the figure it is made from (else the
    /// device's default palette).
    pub inherit: bool,
}

fn d(name: &str, display: (u16, u16), res: (u16, u16), icon: u16, bpp: u16, recommended: bool) -> Device {
    Device { name: name.as_bytes().to_vec(), display, res, icon: (icon, icon), pointer: (icon.max(32), icon.max(32)), bitmap: (100, 100), bpp, recommended, inherit: true }
}

/// The list rs2 starts with.
pub fn builtin() -> Vec<Device> {
    alloc::vec![
        d("Independent Color Form (=VGA)", (0, 0), (96, 96), 32, 4, true),
        d("Independent Black and White Form", (0, 0), (96, 96), 32, 1, true),
        d("Independent Half-size Color Form", (0, 0), (96, 96), 16, 4, true),
        d("Independent Half-size Black and White Form", (0, 0), (96, 96), 16, 1, true),
        d("8514/A and XGA Color Form", (1024, 768), (120, 120), 40, 4, true),
        d("8514/A and XGA Black and White Form", (1024, 768), (120, 120), 40, 1, false),
        d("8514/A and XGA Half-size Color Form", (1024, 768), (120, 120), 20, 4, true),
        d("8514/A and XGA Half-size Black and White Form", (1024, 768), (120, 120), 20, 1, true),
        d("VGA 256 Color Form", (640, 480), (96, 96), 32, 8, false),
        d("SVGA 256 Color Form", (1024, 768), (120, 120), 40, 8, false),
        d("True Color Form", (0, 0), (96, 96), 32, 24, false),
    ]
}

/// The list as kept in OS2.INI: one line per device, fields separated
/// by `|`.
pub fn to_text(list: &[Device]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in list {
        out.extend_from_slice(&v.name);
        let n = [v.display.0, v.display.1, v.res.0, v.res.1, v.icon.0, v.icon.1, v.pointer.0, v.pointer.1, v.bitmap.0, v.bitmap.1, v.bpp, v.recommended as u16, v.inherit as u16];
        for x in n {
            out.extend_from_slice(alloc::format!("|{x}").as_bytes());
        }
        out.push(b'\n');
    }
    out
}

pub fn from_text(t: &[u8]) -> Vec<Device> {
    let mut list = Vec::new();
    for line in t.split(|&c| c == b'\n') {
        let f: Vec<&[u8]> = line.split(|&c| c == b'|').collect();
        if f.len() != 14 || f[0].is_empty() {
            continue;
        }
        let n: Vec<u16> = f[1..].iter().map(|s| pmkit::parse_num(s).unwrap_or(0) as u16).collect();
        if !matches!(n[10], 1 | 4 | 8 | 24) || n[4] == 0 || n[5] == 0 {
            continue;
        }
        list.push(Device {
            name: f[0].to_vec(),
            display: (n[0], n[1]),
            res: (n[2], n[3]),
            icon: (n[4], n[5]),
            pointer: (n[6], n[7]),
            bitmap: (n[8], n[9]),
            bpp: n[10],
            recommended: n[11] != 0,
            inherit: n[12] != 0,
        });
    }
    list
}

/// The size a device gives a figure of this kind (bitmaps: relative to
/// the first form's).
pub fn size_for(dev: &Device, kind: Kind, base: (u32, u32)) -> (u32, u32) {
    match kind {
        Kind::Icon => (dev.icon.0 as u32, dev.icon.1 as u32),
        Kind::Pointer => (dev.pointer.0 as u32, dev.pointer.1 as u32),
        Kind::Bitmap => ((base.0 * dev.bitmap.0 as u32 / 100).max(1), (base.1 * dev.bitmap.1 as u32 / 100).max(1)),
    }
}

/// The device a form is for, if one in the list matches it.
pub fn find<'a>(list: &'a [Device], kind: Kind, f: &Form, base: (u32, u32)) -> Option<&'a Device> {
    list.iter().find(|d| d.display == f.display && d.bpp == f.bpp && size_for(d, kind, base) == (f.cx, f.cy))
}
