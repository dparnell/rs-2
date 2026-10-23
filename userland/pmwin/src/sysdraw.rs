//! WinDrawBorder and the system bitmaps (WinGetSysBitmap).

#![allow(non_snake_case)]

use alloc::vec;

use gpi::bitmap::Bitmap;
use gpi::{Point, Ps, Rect, Surface};

use crate::draw::{self, Canvas};
use crate::gpiapi::{self, RectL};
use crate::state::PsEntry;
use crate::gpiobj;
use crate::misc::WinQuerySysValue;

const DB_PATINVERT: u32 = 0x0001;
const DB_DESTINVERT: u32 = 0x0002;
const DB_AREAMIXMODE: u32 = 0x0003;
const DB_ROP: u32 = 0x0007;
const DB_INTERIOR: u32 = 0x0008;
const DB_AREAATTRS: u32 = 0x0010;
const DB_STANDARD: u32 = 0x0100;
const DB_DLGBORDER: u32 = 0x0200;
/// Warp 4's 3D borders (not in the Toolkit headers; IBM's PMWIN draws
/// them as mitred, trough/fence and field borders).
const DB_RAISED: u32 = 0x0400;
const DB_DEPRESSED: u32 = 0x0800;
const DB_TROUGH: u32 = 0x1000;
const DB_FENCE: u32 = 0x2000;
const DB_FIELD: u32 = 0x4000;

const SYSCLR_WINDOWFRAME: i32 = -19;
const SYSCLR_INACTIVETITLE: i32 = -22;
const SYSCLR_ACTIVETITLE: i32 = -23;
const SYSCLR_BUTTONLIGHT: i32 = -41;
const SYSCLR_BUTTONDARK: i32 = -39;
const FM_XOR: i32 = 4;
const FM_INVERT: i32 = 6;

/// Fill `r` (PS coordinates) with colour `c` under a DB_ROP mode.
fn fill(e: &mut PsEntry, s: &mut Surface, r: Rect, c: i32, rop: u32) {
    if r.is_empty() {
        return;
    }
    let rop = match rop {
        DB_AREAMIXMODE => match e.ps.attrs.area.mix {
            FM_XOR => DB_PATINVERT,
            FM_INVERT => DB_DESTINVERT,
            _ => 0,
        },
        r => r,
    };
    match rop {
        DB_PATINVERT | DB_DESTINVERT => {
            let save = e.ps.attrs.area;
            e.ps.attrs.area.color = c;
            e.ps.blit(s, r, None, if rop == DB_PATINVERT { 0x5a } else { 0x55 }, false, None);
            e.ps.attrs.area = save;
        }
        _ => {
            let rgb = gpiapi::rgb(e, c);
            e.ps.fill_rect(s, r, rgb);
        }
    }
}

/// WinDrawBorder(hps, prcl, cx, cy, clrFore, clrBack, flCmd): a border
/// `cx` pels wide at the sides and `cy` at the top and bottom inside the
/// rectangle in the foreground colour (IBM's GRE splits it into four
/// bands; a border wider than half the rectangle fills it), the
/// interior in the background colour with DB_INTERIOR. DB_AREAATTRS
/// takes both colours from the PS's area attributes, DB_STANDARD counts
/// cx and cy in SV_CXBORDER/SV_CYBORDER units, DB_DLGBORDER draws a
/// dialog border (active with DB_PATINVERT).
#[unsafe(no_mangle)]
pub extern "C" fn WinDrawBorder(hps: u32, prcl: *const RectL, cx: i32, cy: i32, fore: i32, back: i32, fl: u32) -> u32 {
    if prcl.is_null() || fl & 0xffff_00c0 != 0 || cx < 0 || cy < 0 {
        return 0;
    }
    let rc = unsafe { prcl.read_unaligned() };
    if rc.right <= rc.left || rc.top <= rc.bottom {
        return 1;
    }
    let r = Rect::new(rc.left, rc.bottom, rc.right, rc.top);
    let (mut cx, mut cy) = (cx, cy);
    if fl & DB_STANDARD != 0 {
        cx *= WinQuerySysValue(1, 26);
        cy *= WinQuerySysValue(1, 27);
    }
    gpiapi::draw(hps, |e, s| {
        let (fore, back) = if fl & DB_AREAATTRS != 0 { (e.ps.attrs.area.color, e.ps.attrs.area.back) } else { (fore, back) };
        let rop = fl & DB_ROP;
        let inner = |cx: i32, cy: i32| Rect::new(r.left + cx, r.bottom + cy, r.right - cx, r.top - cy);
        if fl & DB_DLGBORDER != 0 {
            // Outline, then the band in the title colour.
            let (cx, cy) = (cx.max(1), cy.max(1));
            bands(e, s, r, 1, 1, SYSCLR_WINDOWFRAME, 0);
            let band = if fl & DB_PATINVERT != 0 { SYSCLR_ACTIVETITLE } else { SYSCLR_INACTIVETITLE };
            bands(e, s, inner(1, 1), cx - 1, cy - 1, band, 0);
            if fl & DB_INTERIOR != 0 {
                fill(e, s, inner(cx, cy), back, 0);
            }
            return;
        }
        if fl & (DB_RAISED | DB_DEPRESSED | DB_TROUGH | DB_FENCE | DB_FIELD) != 0 {
            let (light, dark) = (SYSCLR_BUTTONLIGHT, SYSCLR_BUTTONDARK);
            let (tl, br) = if fl & (DB_DEPRESSED | DB_TROUGH | DB_FIELD) != 0 { (dark, light) } else { (light, dark) };
            if fl & (DB_TROUGH | DB_FENCE) != 0 {
                // Two grooves: outer half one way, inner half the other.
                let (hx, hy) = ((cx + 1) / 2, (cy + 1) / 2);
                mitred(e, s, r, hx, hy, tl, br);
                mitred(e, s, inner(hx, hy), cx - hx, cy - hy, br, tl);
            } else {
                mitred(e, s, r, cx, cy, tl, br);
            }
            if fl & DB_INTERIOR != 0 {
                fill(e, s, inner(cx, cy), back, rop);
            }
            return;
        }
        if 2 * cx < r.width() && 2 * cy < r.height() {
            bands(e, s, r, cx, cy, fore, rop);
            if fl & DB_INTERIOR != 0 {
                fill(e, s, inner(cx, cy), back, rop);
            }
        } else if cx != 0 || cy != 0 {
            fill(e, s, r, fore, rop);
        } else if fl & DB_INTERIOR != 0 {
            fill(e, s, r, back, rop);
        }
    })
    .is_some() as u32
}

/// The four bands of a border, as IBM's GRE splits it: top (left to
/// right - cx), bottom (left + cx to right), left and right between them.
fn bands(e: &mut PsEntry, s: &mut Surface, r: Rect, cx: i32, cy: i32, c: i32, rop: u32) {
    if cy > 0 {
        fill(e, s, Rect::new(r.left, r.top - cy, r.right - cx, r.top), c, rop);
        fill(e, s, Rect::new(r.left + cx, r.bottom, r.right, r.bottom + cy), c, rop);
    }
    if cx > 0 {
        fill(e, s, Rect::new(r.left, r.bottom, r.left + cx, r.top - cy), c, rop);
        fill(e, s, Rect::new(r.right - cx, r.bottom + cy, r.right, r.top), c, rop);
    }
}

/// A 3D border: `tl` on the top and left, `br` on the bottom and right,
/// the corners mitred pel by pel.
fn mitred(e: &mut PsEntry, s: &mut Surface, r: Rect, cx: i32, cy: i32, tl: i32, br: i32) {
    let n = cx.max(cy);
    for i in 0..n {
        let (dx, dy) = (i.min(cx), i.min(cy));
        let o = Rect::new(r.left + dx, r.bottom + dy, r.right - dx, r.top - dy);
        if o.is_empty() {
            break;
        }
        if i < cy {
            fill(e, s, Rect::new(o.left, o.top - 1, o.right - 1, o.top), tl, 0);
            fill(e, s, Rect::new(o.left + 1, o.bottom, o.right, o.bottom + 1), br, 0);
        }
        if i < cx {
            fill(e, s, Rect::new(o.left, o.bottom, o.left + 1, o.top), tl, 0);
            fill(e, s, Rect::new(o.right - 1, o.bottom, o.right, o.top - 1), br, 0);
        }
    }
}

// ---- System bitmaps ----

const SBMP_OLD_SYSMENU: u32 = 1;
const SBMP_OLD_SBUPARROW: u32 = 2;
const SBMP_OLD_SBLFARROW: u32 = 5;
const SBMP_MENUCHECK: u32 = 6;
const SBMP_OLD_CHECKBOXES: u32 = 7;
const SBMP_BTNCORNERS: u32 = 8;
const SBMP_OLD_MINBUTTON: u32 = 9;
const SBMP_OLD_CHILDSYSMENU: u32 = 12;
const SBMP_DRIVE: u32 = 15;
const SBMP_FILE: u32 = 16;
const SBMP_FOLDER: u32 = 17;
const SBMP_TREEPLUS: u32 = 18;
const SBMP_TREEMINUS: u32 = 19;
const SBMP_PROGRAM: u32 = 22;
const SBMP_MENUATTACHED: u32 = 23;
const SBMP_SIZEBOX: u32 = 24;
const SBMP_SYSMENU: u32 = 25;
const SBMP_RESTOREBUTTONDEP: u32 = 33;
const SBMP_SBUPARROW: u32 = 35;
const SBMP_SBRGARROWDIS: u32 = 46;
const SBMP_COMBODOWN: u32 = 47;
const SBMP_CHECKBOXES: u32 = 48;
const SBMP_HIDE: u32 = 50;
const SBMP_CLOSEDEP: u32 = 53;

/// WinGetSysBitmap(hwndDesktop, ibm): a new bitmap (the caller deletes
/// it) of system image SBMP_*. IBM's PMWIN loads them from the display
/// driver's resources; rs2 draws them as its controls look.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetSysBitmap(_desk: u32, ibm: u32) -> u32 {
    let t = crate::frame::TITLE_H;
    let (w, h) = match ibm {
        SBMP_OLD_SYSMENU..=SBMP_OLD_SBLFARROW | SBMP_OLD_MINBUTTON..=SBMP_OLD_CHILDSYSMENU => (t, t),
        SBMP_SYSMENU..=SBMP_RESTOREBUTTONDEP | SBMP_HIDE..=SBMP_CLOSEDEP => (t, t),
        SBMP_SBUPARROW..=SBMP_SBRGARROWDIS | SBMP_COMBODOWN => (16, 16),
        SBMP_MENUCHECK | SBMP_MENUATTACHED => (12, 12),
        SBMP_TREEPLUS | SBMP_TREEMINUS => (9, 9),
        // 4 columns (unchecked, checked, indeterminate, ... ) x 3 rows
        // (normal, pressed, ...) of check boxes and radio buttons.
        SBMP_OLD_CHECKBOXES | SBMP_CHECKBOXES => (4 * 13, 3 * 13),
        SBMP_BTNCORNERS => (6, 6),
        SBMP_DRIVE | SBMP_FILE | SBMP_FOLDER | SBMP_PROGRAM => (16, 16),
        SBMP_SIZEBOX => (16, 16),
        _ => return 0,
    };
    let mut bm = Bitmap::new(w, h, 1, 24);
    let surface = bm.surface();
    let mut ps = Ps::new(Point::new(0, 0), vec![Rect::new(0, 0, w, h)]);
    let mut c = Canvas::new(&mut ps, surface, w, h);
    paint_sys_bitmap(&mut c, ibm, w, h);
    gpiobj::new_bitmap(bm)
}

fn paint_sys_bitmap(c: &mut Canvas, ibm: u32, w: i32, h: i32) {
    let all = Rect::new(0, 0, w, h);
    let button = |c: &mut Canvas, down: bool| {
        c.fill(all, draw::face());
        if down { c.sunken(all) } else { c.raised(all) }
    };
    let (mx, my) = (w / 2, h / 2);
    match ibm {
        SBMP_SBUPARROW..=SBMP_SBRGARROWDIS | SBMP_OLD_SBUPARROW..=SBMP_OLD_SBLFARROW | SBMP_COMBODOWN => {
            // Up, down, left, right; normal, pressed, disabled.
            let (dir, kind) = match ibm {
                SBMP_COMBODOWN => (1, 0),
                SBMP_OLD_SBUPARROW..=SBMP_OLD_SBLFARROW => ([0, 1, 3, 2][(ibm - SBMP_OLD_SBUPARROW) as usize], 0),
                _ => ([0, 1, 2, 3][((ibm - SBMP_SBUPARROW) % 4) as usize], (ibm - SBMP_SBUPARROW) / 4),
            };
            button(c, kind == 1);
            let col = if kind == 2 { draw::shadow() } else { draw::text() };
            for i in 0..4 {
                let r = match dir {
                    0 => Rect::new(mx - i, my + 2 - i, mx + i + 1, my + 3 - i),
                    1 => Rect::new(mx - i, my - 2 + i, mx + i + 1, my - 1 + i),
                    2 => Rect::new(mx - 2 + i, my - i, mx - 1 + i, my + i + 1),
                    _ => Rect::new(mx + 2 - i, my - i, mx + 3 - i, my + i + 1),
                };
                c.fill(r, col);
            }
        }
        SBMP_SYSMENU..=SBMP_RESTOREBUTTONDEP | SBMP_HIDE..=SBMP_CLOSEDEP | SBMP_OLD_SYSMENU | SBMP_OLD_MINBUTTON..=SBMP_OLD_CHILDSYSMENU => {
            // sysmenu, min, max, restore, child sysmenu, then the same
            // pressed; hide, hide pressed, close, close pressed.
            let (kind, down) = match ibm {
                SBMP_SYSMENU..=SBMP_RESTOREBUTTONDEP => ((ibm - SBMP_SYSMENU) % 5, ibm >= SBMP_SYSMENU + 5),
                SBMP_HIDE..=SBMP_CLOSEDEP => (if ibm < SBMP_HIDE + 2 { 5 } else { 6 }, (ibm - SBMP_HIDE) % 2 == 1),
                SBMP_OLD_SYSMENU => (0, false),
                _ => ([1, 2, 3, 4][(ibm - SBMP_OLD_MINBUTTON) as usize], false),
            };
            button(c, down);
            let k = draw::text();
            match kind {
                0 | 4 => c.fill(Rect::new(4, my - 1, w - 4, my + 2), k),
                1 | 5 => c.fill(Rect::new(mx - 2, my - 2, mx + 3, my + 3), k),
                2 => c.frame(Rect::new(4, 4, w - 4, h - 4), k),
                3 => {
                    c.frame(Rect::new(4, 4, w - 7, h - 7), k);
                    c.frame(Rect::new(7, 7, w - 4, h - 4), k);
                }
                _ => {
                    for i in 0..w - 10 {
                        c.fill(Rect::new(5 + i, 5 + i, 7 + i, 6 + i), k);
                        c.fill(Rect::new(5 + i, h - 6 - i, 7 + i, h - 5 - i), k);
                    }
                }
            }
        }
        SBMP_MENUCHECK => {
            for i in 0..3 {
                c.fill(Rect::new(2 + i, 5 - i, 3 + i, 8 - i), draw::text());
            }
            for i in 0..5 {
                c.fill(Rect::new(5 + i, 3 + i, 6 + i, 6 + i), draw::text());
            }
        }
        SBMP_MENUATTACHED => {
            for i in 0..4 {
                c.fill(Rect::new(4 + i, my - 3 + i, 5 + i, my + 4 - i), draw::text());
            }
        }
        SBMP_TREEPLUS | SBMP_TREEMINUS => {
            c.fill(all, draw::window());
            c.frame(all, draw::shadow());
            c.fill(Rect::new(2, my, w - 2, my + 1), draw::text());
            if ibm == SBMP_TREEPLUS {
                c.fill(Rect::new(mx, 2, mx + 1, h - 2), draw::text());
            }
        }
        SBMP_OLD_CHECKBOXES | SBMP_CHECKBOXES => {
            c.fill(all, draw::face());
            for row in 0..3 {
                for col in 0..4 {
                    let b = Rect::new(col * 13, (2 - row) * 13, col * 13 + 13, (2 - row) * 13 + 13);
                    c.fill(b, if row == 1 { draw::face() } else { draw::window() });
                    c.sunken(b);
                    if col == 1 {
                        for i in 0..3 {
                            c.fill(Rect::new(b.left + 3 + i, b.bottom + 6 - i, b.left + 4 + i, b.bottom + 9 - i), draw::text());
                        }
                        for i in 0..5 {
                            c.fill(Rect::new(b.left + 6 + i, b.bottom + 4 + i, b.left + 7 + i, b.bottom + 7 + i), draw::text());
                        }
                    } else if col == 2 {
                        c.fill(Rect::new(b.left + 3, b.bottom + 3, b.right - 3, b.top - 3), draw::shadow());
                    }
                }
            }
        }
        SBMP_SIZEBOX => {
            c.fill(all, draw::face());
            for i in (2..w).step_by(4) {
                for j in 0..2 {
                    c.fill(Rect::new(i + j, 1, i + j + 1, 1 + w - i - j), if j == 0 { draw::light() } else { draw::shadow() });
                }
            }
        }
        SBMP_FILE | SBMP_PROGRAM => {
            c.fill(all, draw::face());
            c.fill(Rect::new(3, 1, 13, 15), draw::window());
            c.frame(Rect::new(3, 1, 13, 15), draw::text());
            if ibm == SBMP_PROGRAM {
                c.fill(Rect::new(3, 12, 13, 15), draw::hilite());
            }
        }
        SBMP_FOLDER => {
            c.fill(all, draw::face());
            c.fill(Rect::new(1, 2, 15, 12), 0xffff80);
            c.fill(Rect::new(1, 12, 7, 14), 0xffff80);
            c.frame(Rect::new(1, 2, 15, 12), draw::text());
        }
        SBMP_DRIVE => {
            c.fill(all, draw::face());
            c.fill(Rect::new(1, 4, 15, 11), 0xc0c0c0);
            c.frame(Rect::new(1, 4, 15, 11), draw::text());
            c.fill(Rect::new(11, 6, 13, 8), 0x00c000);
        }
        _ => button(c, false),
    }
}
