//! Buttons (WC_BUTTON): push buttons, check boxes, radio buttons and
//! 3-state boxes, with their automatic variants.

use alloc::vec::Vec;

use gpi::Rect;

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{post, WinDefWindowProc};
use crate::state::st;
use crate::win::{WM_BUTTON1DOWN, WM_BUTTON1UP, WM_CHAR, WM_MOUSEMOVE, WM_PAINT, WM_SETFOCUS, WM_SYSCOMMAND};

const WM_CREATE: u32 = 0x0001;

pub const BS_PUSHBUTTON: u32 = 0;
pub const BS_AUTOCHECKBOX: u32 = 2;
pub const BS_RADIOBUTTON: u32 = 3;
pub const BS_AUTORADIOBUTTON: u32 = 4;
pub const BS_AUTO3STATE: u32 = 6;
pub const BS_USERBUTTON: u32 = 7;
const BS_HELP: u32 = 0x0100;
const BS_SYSCOMMAND: u32 = 0x0200;
pub const BS_DEFAULT: u32 = 0x0400;
const BS_NOPOINTERFOCUS: u32 = 0x0800;
const BS_NOBORDER: u32 = 0x1000;

const BM_CLICK: u32 = 0x0120;
const BM_QUERYCHECKINDEX: u32 = 0x0121;
const BM_QUERYHILITE: u32 = 0x0122;
const BM_SETHILITE: u32 = 0x0123;
const BM_QUERYCHECK: u32 = 0x0124;
const BM_SETCHECK: u32 = 0x0125;
const BM_SETDEFAULT: u32 = 0x0126;

const BN_CLICKED: u32 = 1;

wndproc! {
    button_entry => button_proc;
}

#[derive(Default)]
struct Button {
    check: u32,
    hilite: bool,
    tracking: bool,
}

fn data(h: u32) -> &'static mut Button {
    if ctl::data::<Button>(h).is_none() {
        ctl::set_data(h, Button::default());
    }
    ctl::data::<Button>(h).expect("button data")
}

pub fn kind(h: u32) -> u32 {
    ctl::style(h) & 0x0f
}

fn is_radio(h: u32) -> bool {
    matches!(kind(h), BS_RADIOBUTTON | BS_AUTORADIOBUTTON)
}

/// The radio buttons of the group containing `h`.
fn group(h: u32) -> Vec<u32> {
    crate::dlg::group(h).into_iter().filter(|&b| crate::ctl::data::<Button>(b).is_some() && is_radio(b)).collect()
}

/// The button was clicked (mouse, space bar, mnemonic or BM_CLICK).
pub fn click(h: u32, pointer: bool) {
    let style = ctl::style(h);
    match kind(h) {
        BS_PUSHBUTTON | BS_USERBUTTON => {
            let msg = if style & BS_HELP != 0 {
                WM_HELP
            } else if style & BS_SYSCOMMAND != 0 {
                WM_SYSCOMMAND
            } else {
                WM_COMMAND
            };
            post(ctl::owner(h), msg, ctl::id(h), mp(CMDSRC_PUSHBUTTON, pointer as u32));
            return;
        }
        BS_AUTOCHECKBOX => {
            set_check(h, (data(h).check == 0) as u32);
        }
        BS_AUTO3STATE => {
            set_check(h, (data(h).check + 1) % 3);
        }
        BS_AUTORADIOBUTTON => {
            for b in group(h) {
                if b != h && kind(b) == BS_AUTORADIOBUTTON {
                    set_check(b, 0);
                }
            }
            set_check(h, 1);
        }
        _ => {}
    }
    ctl::notify(h, BN_CLICKED);
}

fn set_check(h: u32, v: u32) -> u32 {
    let b = data(h);
    let old = core::mem::replace(&mut b.check, v);
    if old != v {
        ctl::invalidate(h);
    }
    old
}

fn set_hilite(h: u32, on: bool) {
    let b = data(h);
    if b.hilite != on {
        b.hilite = on;
        ctl::invalidate(h);
    }
}

pub fn is_default(h: u32) -> bool {
    kind(h) == BS_PUSHBUTTON && ctl::style(h) & BS_DEFAULT != 0
}

// ---- Painting ----

fn paint(h: u32) {
    let style = ctl::style(h);
    let text = ctl::text(h);
    let focus = ctl::has_focus(h);
    let enabled = ctl::enabled(h);
    let (check, hilite) = (data(h).check, data(h).hilite);
    let k = kind(h);
    draw::paint(h, |c| {
        let r = c.rect();
        let fg = if enabled { draw::TEXT } else { draw::GRAY_TEXT };
        match k {
            BS_PUSHBUTTON | BS_USERBUTTON => push(c, r, style, &text, fg, hilite, focus),
            BS_RADIOBUTTON | BS_AUTORADIOBUTTON => {
                c.fill(r, draw::dialog_bg());
                radio(c, r, check != 0, hilite);
                label(c, r, &text, fg, focus);
            }
            _ => {
                c.fill(r, draw::dialog_bg());
                check_box(c, r, check, hilite);
                label(c, r, &text, fg, focus);
            }
        }
    });
}

fn push(c: &mut Canvas, r: Rect, style: u32, text: &[u8], fg: u32, down: bool, focus: bool) {
    c.fill(r, draw::FACE);
    let mut b = r;
    if style & BS_NOBORDER == 0 {
        if style & BS_DEFAULT != 0 {
            c.frame(b, draw::TEXT);
            b = Rect::new(b.left + 1, b.bottom + 1, b.right - 1, b.top - 1);
        }
        if down {
            c.frame(b, draw::SHADOW);
        } else {
            c.bevel(b, draw::LIGHT, draw::DARK);
            c.bevel(Rect::new(b.left + 1, b.bottom + 1, b.right - 1, b.top - 1), draw::LIGHT, draw::SHADOW);
        }
    }
    let off = down as i32;
    let t = Rect::new(r.left + 3 + off, r.bottom + 2 - off, r.right - 3 + off, r.top - 2 - off);
    c.text_in(t, text, fg, draw::DT_CENTER | draw::DT_VCENTER | draw::DT_MNEMONIC);
    if focus {
        let (s, _) = draw::strip_mnemonic(text);
        let w = draw::text_width(&s).min(r.width() - 8);
        let f = draw::font();
        let x = r.left + (r.width() - w) / 2 - 2;
        let y = r.bottom + (r.height() - f.height) / 2 - 1;
        c.focus_rect(Rect::new(x, y, x + w + 4, y + f.height + 2));
    }
}

const BOX: i32 = 13;

fn box_rect(r: Rect) -> Rect {
    let y = r.bottom + (r.height() - BOX) / 2;
    Rect::new(r.left + 1, y, r.left + 1 + BOX, y + BOX)
}

fn check_box(c: &mut Canvas, r: Rect, check: u32, down: bool) {
    let b = box_rect(r);
    c.fill(b, if check == 2 || down { draw::FACE } else { draw::WINDOW });
    c.sunken(b);
    c.sunken(Rect::new(b.left + 1, b.bottom + 1, b.right - 1, b.top - 1));
    if check == 1 {
        let (x, y) = (b.left + 3, b.bottom + 5);
        for i in 0..3 {
            c.fill(Rect::new(x + i, y - i + 1, x + i + 1, y - i + 4), draw::TEXT);
        }
        for i in 0..5 {
            c.fill(Rect::new(x + 3 + i, y - 2 + i, x + 4 + i, y + i + 1), draw::TEXT);
        }
    }
}

fn radio(c: &mut Canvas, r: Rect, on: bool, down: bool) {
    let b = box_rect(r);
    let (cx, cy) = (b.left + BOX / 2, b.bottom + BOX / 2);
    // Filled disc, then the 3D ring, row by row.
    for dy in -6i32..=6 {
        let w = isqrt(36 - dy * dy);
        c.fill(Rect::new(cx - w, cy + dy, cx + w + 1, cy + dy + 1), if down { draw::FACE } else { draw::WINDOW });
        let col = if dy > 0 { draw::SHADOW } else { draw::LIGHT };
        c.fill(Rect::new(cx - w, cy + dy, cx - w + 1, cy + dy + 1), if dy >= 0 { draw::SHADOW } else { col });
        c.fill(Rect::new(cx + w, cy + dy, cx + w + 1, cy + dy + 1), if dy > 0 { draw::SHADOW } else { draw::LIGHT });
    }
    c.hline(cx - 2, cx + 3, cy + 6, draw::SHADOW);
    c.hline(cx - 2, cx + 3, cy - 6, draw::LIGHT);
    if on {
        for dy in -2i32..=2 {
            let w = isqrt(6 - dy * dy);
            c.fill(Rect::new(cx - w, cy + dy, cx + w + 1, cy + dy + 1), draw::TEXT);
        }
    }
}

fn isqrt(v: i32) -> i32 {
    let mut r = 0;
    while (r + 1) * (r + 1) <= v {
        r += 1;
    }
    r
}

fn label(c: &mut Canvas, r: Rect, text: &[u8], fg: u32, focus: bool) {
    let t = Rect::new(r.left + BOX + 6, r.bottom, r.right, r.top);
    c.text_in(t, text, fg, draw::DT_VCENTER | draw::DT_MNEMONIC);
    if focus {
        let w = draw::text_width(&draw::strip_mnemonic(text).0).min(t.width());
        let f = draw::font();
        let y = r.bottom + (r.height() - f.height) / 2 - 1;
        c.focus_rect(Rect::new(t.left - 2, y.max(r.bottom), t.left + w + 2, (y + f.height + 2).min(r.top)));
    }
}

// ---- The window procedure ----

pub extern "C" fn button_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            ctl::set_data(h, Button::default());
            0
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON1DBLCLK => {
            if !ctl::enabled(h) {
                return 1;
            }
            if ctl::style(h) & BS_NOPOINTERFOCUS == 0 {
                crate::win::WinSetFocus(1, h);
            }
            st().capture = h;
            data(h).tracking = true;
            set_hilite(h, true);
            1
        }
        WM_MOUSEMOVE => {
            if data(h).tracking {
                let (cx, cy) = ctl::size(h);
                let p = ctl::mouse_pos(mp1);
                set_hilite(h, p.x >= 0 && p.y >= 0 && p.x < cx && p.y < cy);
            }
            1
        }
        WM_BUTTON1UP => {
            let b = data(h);
            if b.tracking {
                b.tracking = false;
                let hit = b.hilite;
                if st().capture == h {
                    st().capture = 0;
                }
                set_hilite(h, false);
                if hit {
                    click(h, true);
                }
            }
            1
        }
        WM_CHAR => {
            let k = Key::from(mp1, mp2);
            if k.vk == VK_SPACE && !k.alt() && !k.ctrl() {
                if !k.up() {
                    click(h, false);
                }
                return 1;
            }
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_SETFOCUS | WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        WM_QUERYDLGCODE => match kind(h) {
            BS_PUSHBUTTON | BS_USERBUTTON => DLGC_BUTTON | if is_default(h) { DLGC_DEFAULT } else { DLGC_PUSHBUTTON },
            BS_RADIOBUTTON | BS_AUTORADIOBUTTON => DLGC_BUTTON | DLGC_RADIOBUTTON,
            _ => DLGC_BUTTON | DLGC_CHECKBOX,
        },
        WM_MATCHMNEMONIC => (draw::mnemonic(&ctl::text(h)) == Some((mp1 as u8).to_ascii_lowercase())) as u32,
        BM_CLICK => {
            click(h, false);
            1
        }
        BM_QUERYCHECK => data(h).check,
        BM_SETCHECK => set_check(h, mp1),
        BM_QUERYHILITE => data(h).hilite as u32,
        BM_SETHILITE => {
            let old = data(h).hilite;
            set_hilite(h, mp1 != 0);
            old as u32
        }
        BM_QUERYCHECKINDEX => {
            let g = group(h);
            g.iter().position(|&b| ctl::data::<Button>(b).is_some_and(|x| x.check != 0)).map_or(u32::MAX, |i| i as u32)
        }
        BM_SETDEFAULT => {
            if let Some(w) = st().desk.get_mut(h) {
                if mp1 != 0 {
                    w.style |= BS_DEFAULT;
                } else {
                    w.style &= !BS_DEFAULT;
                }
            }
            ctl::invalidate(h);
            1
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
