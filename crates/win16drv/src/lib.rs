//! RS2DISP.DRV, rs2's paravirtual Windows 3.1 display driver
//! (docs/WINOS2.md): a 16-bit NE library whose exported entry points are
//! each a trap the DOS machine recognises, `FE 38 n n` (callback
//! `CB_DISP + ordinal`) followed by `RETF size`. The machine
//! (`dos::disp`) reads the Pascal-order arguments from the stack, does
//! the work in Rust, sets DX:AX and lets the RETF return.
//!
//! rs2 does not ship the driver: [`build`] makes it from the user's
//! installed VGA.DRV (APPINST does it when it installs Win-OS/2), whose
//! resources (USER's system bitmaps, cursors and icons, the OEMBIN
//! table), names and description it takes over. Its code is all new.

#![no_std]

extern crate alloc;

pub mod surface;

use alloc::vec;
use alloc::vec::Vec;

/// Callback number of entry `ordinal`: `CB_DISP + ordinal`.
pub const CB_DISP: u16 = 0x800;
pub const CB_DISP_LAST: u16 = CB_DISP + 0x1ff;

/// The exported entry points: ordinal, name, bytes of arguments (the
/// RETF's operand), as Windows 3.1's DDK has them and VGA.DRV exports
/// them.
pub const ENTRIES: &[(u16, &str, u16)] = &[
    (1, "BITBLT", 32),
    (2, "COLORINFO", 12),
    (3, "CONTROL", 14),
    (4, "DISABLE", 4),
    (5, "ENABLE", 18),
    (6, "ENUMDFONTS", 16),
    (7, "ENUMOBJ", 14),
    (8, "OUTPUT", 28),
    (9, "PIXEL", 16),
    (10, "REALIZEOBJECT", 18),
    (11, "STRBLT", 30),
    (12, "SCANLR", 14),
    (13, "DEVICEMODE", 12),
    (14, "EXTTEXTOUT", 40),
    (15, "GETCHARWIDTH", 24),
    (16, "DEVICEBITMAP", 14),
    (17, "FASTBORDER", 28),
    (18, "SETATTRIBUTE", 12),
    (19, "DEVICEBITMAPBITS", 26),
    (20, "CREATEBITMAP", 0),
    (21, "DIBSCREENBLT", 32),
    (92, "SAVESCREENBITMAP", 6),
    (101, "INQUIRE", 4),
    (102, "SETCURSOR", 4),
    (103, "MOVECURSOR", 4),
    (104, "CHECKCURSOR", 0),
    (500, "USERREPAINTDISABLE", 2),
];

/// The module's description (the non-resident name), whose first part
/// GDI and the Control Panel read as the aspect ratio and resolution.
pub const DESCRIPTION: &[u8] = b"DISPLAY : 100, 96, 96 : rs2 Display (paravirtual)";

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?]))
}

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// The code segment: the library's initialization (MOV AX,1; RETF) at 0,
/// then 8 bytes per entry point.
fn code() -> Vec<u8> {
    let mut c = vec![0xb8, 0x01, 0x00, 0xcb, 0x90, 0x90, 0x90, 0x90];
    for &(ord, _, args) in ENTRIES {
        let n = CB_DISP + ord;
        c.extend_from_slice(&[0xfe, 0x38, n as u8, (n >> 8) as u8, 0xca, args as u8, (args >> 8) as u8, 0x90]);
    }
    c
}

/// The NE entry table for the entries, all in segment 1 (fixed).
fn entry_table() -> Vec<u8> {
    let mut t = Vec::new();
    let mut next = 1u16;
    let mut i = 0;
    while i < ENTRIES.len() {
        let ord = ENTRIES[i].0;
        // Unused ordinals: empty bundles.
        let mut gap = ord - next;
        while gap > 0 {
            let n = gap.min(255);
            t.extend_from_slice(&[n as u8, 0]);
            gap -= n;
        }
        // A bundle of consecutive ordinals.
        let mut j = i;
        while j + 1 < ENTRIES.len() && ENTRIES[j + 1].0 == ENTRIES[j].0 + 1 && j + 1 - i < 255 {
            j += 1;
        }
        t.extend_from_slice(&[(j - i + 1) as u8, 1]);
        for k in i..=j {
            t.push(1); // exported
            put16(&mut t, (8 + k * 8) as u16);
        }
        next = ENTRIES[j].0 + 1;
        i = j + 1;
    }
    t.push(0);
    t
}

/// Make RS2DISP.DRV, with the resources of `vga` (VGA.DRV).
pub fn build(vga: &[u8]) -> Result<Vec<u8>, &'static str> {
    let bad = "VGA.DRV is not a Windows display driver";
    let ne = u16_at(vga, 0x3c).ok_or(bad)? as usize | (u16_at(vga, 0x3e).ok_or(bad)? as usize) << 16;
    if vga.get(ne..ne + 2) != Some(b"NE") {
        return Err(bad);
    }
    let h = |o: usize| u16_at(vga, ne + o).ok_or(bad);
    let rsrc = ne + h(0x24)? as usize;
    let resident = ne + h(0x26)? as usize;
    if rsrc >= resident || resident > vga.len() {
        return Err(bad);
    }
    // The resource table and the resources it points to.
    let mut table = vga[rsrc..resident].to_vec();
    let shift = u16_at(&table, 0).ok_or(bad)?;
    if shift > 12 {
        return Err(bad);
    }
    let mut res_data: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut p = 2;
    while u16_at(&table, p).ok_or(bad)? != 0 {
        let count = u16_at(&table, p + 2).ok_or(bad)? as usize;
        p += 8;
        for _ in 0..count {
            let off = (u16_at(&table, p).ok_or(bad)? as usize) << shift;
            let len = (u16_at(&table, p + 2).ok_or(bad)? as usize) << shift;
            let data = vga.get(off..off + len).ok_or(bad)?.to_vec();
            res_data.push((p, data));
            p += 12;
        }
    }

    // The layout: MZ header, NE header and tables, then the code segment
    // and the resources, each aligned to 1 << shift.
    let mut names = Vec::new();
    let name = |v: &mut Vec<u8>, s: &[u8], ord: u16| {
        v.push(s.len() as u8);
        v.extend_from_slice(s);
        put16(v, ord);
    };
    name(&mut names, b"DISPLAY", 0);
    for &(ord, n, _) in ENTRIES {
        name(&mut names, n.as_bytes(), ord);
    }
    names.push(0);
    let entries = entry_table();
    let mut nonres = Vec::new();
    name(&mut nonres, DESCRIPTION, 0);
    nonres.push(0);
    let code = code();

    let ne_at = 0x40usize;
    let seg_table = 0x40usize;
    let res_table = seg_table + 8;
    let res_names = res_table + table.len();
    let modref = res_names + names.len();
    let imports = modref;
    let entry_at = imports + 1;
    let nonres_at = ne_at + entry_at + entries.len();
    let align = |x: usize| (x + (1 << shift) - 1) & !((1usize << shift) - 1);
    let code_at = align(nonres_at + nonres.len());
    let mut at = align(code_at + code.len());
    for (pos, data) in &res_data {
        let units = (at >> shift) as u16;
        table[*pos..*pos + 2].copy_from_slice(&units.to_le_bytes());
        at = align(at + data.len());
    }

    let mut f = vec![0u8; ne_at];
    f[0..2].copy_from_slice(b"MZ");
    f[2..4].copy_from_slice(&(ne_at as u16).to_le_bytes()); // bytes in the last page
    f[4..6].copy_from_slice(&1u16.to_le_bytes()); // pages
    f[8..10].copy_from_slice(&4u16.to_le_bytes()); // header paragraphs
    f[0x18..0x1a].copy_from_slice(&0x40u16.to_le_bytes());
    f[0x3c..0x40].copy_from_slice(&(ne_at as u32).to_le_bytes());
    let mut n = Vec::with_capacity(0x40);
    n.extend_from_slice(b"NE");
    n.extend_from_slice(&[5, 10]); // linker version
    put16(&mut n, entry_at as u16);
    put16(&mut n, entries.len() as u16);
    n.extend_from_slice(&[0; 4]); // CRC
    put16(&mut n, 0x8300); // library, Windows API, no automatic data
    put16(&mut n, 0); // automatic data segment
    put16(&mut n, 0); // heap
    put16(&mut n, 0); // stack
    put16(&mut n, 0); // IP: the initialization routine
    put16(&mut n, 1); // CS
    n.extend_from_slice(&[0; 4]); // SS:SP
    put16(&mut n, 1); // segments
    put16(&mut n, 0); // module references
    put16(&mut n, nonres.len() as u16);
    put16(&mut n, seg_table as u16);
    put16(&mut n, res_table as u16);
    put16(&mut n, res_names as u16);
    put16(&mut n, modref as u16);
    put16(&mut n, imports as u16);
    n.extend_from_slice(&(nonres_at as u32).to_le_bytes());
    put16(&mut n, 0); // movable entries
    put16(&mut n, shift);
    put16(&mut n, 0); // resource segments
    n.push(2); // Windows
    n.push(0x08); // proportional fonts, protected mode
    n.extend_from_slice(&[0; 4]); // fast load area
    put16(&mut n, 0); // minimum code swap
    put16(&mut n, 0x030a); // Windows 3.10
    // The segment table: one fixed, preloaded, pure code segment.
    put16(&mut n, (code_at >> shift) as u16);
    put16(&mut n, code.len() as u16);
    put16(&mut n, 0x0c60);
    put16(&mut n, code.len() as u16);
    n.extend_from_slice(&table);
    n.extend_from_slice(&names);
    n.push(0); // imported names: none
    n.extend_from_slice(&entries);
    f.extend_from_slice(&n);
    f.extend_from_slice(&nonres);
    f.resize(code_at, 0);
    f.extend_from_slice(&code);
    for (_, data) in &res_data {
        f.resize(align(f.len()), 0);
        f.extend_from_slice(data);
    }
    Ok(f)
}
