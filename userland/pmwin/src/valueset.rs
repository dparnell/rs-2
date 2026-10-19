//! Value sets (WC_VALUESET): a grid of choices (colours, text, icons or
//! bitmaps) with one selected; VM_* messages and VN_* notifications.
//! Rows and columns count from 1. The grid geometry is
//! `pmwin::controls::Grid`.

use alloc::vec::Vec;

use gpi::Rect;
use pmwin::controls::Grid;

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{send, WinDefWindowProc};
use crate::win::{WM_BUTTON1DOWN, WM_CHAR, WM_PAINT, WM_SETFOCUS, WM_SIZE};

const WM_CREATE: u32 = 0x0001;

const VS_BITMAP: u32 = 0x0001;
const VS_ICON: u32 = 0x0002;
const VS_TEXT: u32 = 0x0004;
const VS_RGB: u32 = 0x0008;
const VS_COLORINDEX: u32 = 0x0010;
const VS_BORDER: u32 = 0x0020;
const VS_ITEMBORDER: u32 = 0x0040;

const VIA_BITMAP: u32 = 0x0001;
const VIA_ICON: u32 = 0x0002;
const VIA_TEXT: u32 = 0x0004;
const VIA_RGB: u32 = 0x0008;
const VIA_COLORINDEX: u32 = 0x0010;
const VIA_OWNERDRAW: u32 = 0x0020;
const VIA_DISABLED: u32 = 0x0040;

const VM_QUERYITEM: u32 = 0x0375;
const VM_QUERYITEMATTR: u32 = 0x0376;
const VM_QUERYMETRICS: u32 = 0x0377;
const VM_QUERYSELECTEDITEM: u32 = 0x0378;
const VM_SELECTITEM: u32 = 0x0379;
const VM_SETITEM: u32 = 0x037a;
const VM_SETITEMATTR: u32 = 0x037b;
const VM_SETMETRICS: u32 = 0x037c;

const VN_SELECT: u32 = 120;
const VN_ENTER: u32 = 121;
const VN_SETFOCUS: u32 = 127;
const VN_KILLFOCUS: u32 = 128;

const VMA_ITEMSIZE: u32 = 1;
const VMA_ITEMSPACING: u32 = 2;

wndproc! {
    valueset_entry => valueset_proc;
}

#[derive(Clone, Default)]
struct Item {
    attr: u32,
    /// Colour, index, handle or text pointer as set.
    value: u32,
    text: Vec<u8>,
}

struct ValueSet {
    rows: u16,
    cols: u16,
    items: Vec<Item>,
    sel: (u16, u16),
    space: i32,
}

fn data(h: u32) -> Option<&'static mut ValueSet> {
    ctl::data::<ValueSet>(h)
}

fn border(h: u32) -> i32 {
    if ctl::style(h) & VS_BORDER != 0 { 2 } else { 0 }
}

fn grid(h: u32) -> Grid {
    let (cx, cy) = ctl::size(h);
    let b = border(h);
    let d = data(h);
    Grid { rows: d.as_ref().map_or(1, |d| d.rows), cols: d.as_ref().map_or(1, |d| d.cols), w: cx - 2 * b, h: cy - 2 * b, space: d.map_or(2, |d| d.space) }
}

fn index(h: u32, row: u16, col: u16) -> Option<usize> {
    let d = data(h)?;
    (row >= 1 && col >= 1 && row <= d.rows && col <= d.cols).then(|| (row as usize - 1) * d.cols as usize + col as usize - 1)
}

fn select(h: u32, row: u16, col: u16, notify: bool) -> bool {
    let Some(i) = index(h, row, col) else { return false };
    let Some(d) = data(h) else { return false };
    if d.items[i].attr & VIA_DISABLED != 0 {
        return false;
    }
    let changed = d.sel != (row, col);
    d.sel = (row, col);
    ctl::invalidate(h);
    if notify && changed {
        send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), VN_SELECT), mp(row as u32, col as u32));
    }
    true
}

fn paint(h: u32) {
    let g = grid(h);
    let b = border(h);
    let style = ctl::style(h);
    let focus = ctl::has_focus(h);
    let Some(d) = data(h) else { return };
    let items = d.items.clone();
    let (rows, cols, sel) = (d.rows, d.cols, d.sel);
    draw::paint(h, |c: &mut Canvas| {
        let r = c.rect();
        c.fill(r, draw::dialog_bg());
        if b > 0 {
            c.sunken(r);
        }
        for row in 1..=rows {
            for col in 1..=cols {
                let (l, bt, rt, tp) = g.cell(row, col);
                let cell = Rect::new(l + b, bt + b, rt + b, tp + b);
                let it = &items[(row as usize - 1) * cols as usize + col as usize - 1];
                if (row, col) == sel {
                    c.fill(Rect::new(cell.left - 2, cell.bottom - 2, cell.right + 2, cell.top + 2), draw::HILITE);
                }
                let inner = if style & VS_ITEMBORDER != 0 {
                    c.sunken(cell);
                    Rect::new(cell.left + 1, cell.bottom + 1, cell.right - 1, cell.top - 1)
                } else {
                    cell
                };
                if it.attr & VIA_RGB != 0 {
                    c.fill(inner, it.value & 0xff_ffff);
                } else if it.attr & VIA_COLORINDEX != 0 {
                    c.fill(inner, crate::color::resolve(it.value as i32));
                } else if it.attr & VIA_TEXT != 0 {
                    let col_t = if it.attr & VIA_DISABLED != 0 { draw::GRAY_TEXT } else { draw::TEXT };
                    c.text_in(inner, &it.text, col_t, draw::DT_CENTER | draw::DT_VCENTER);
                } else if it.attr & (VIA_ICON | VIA_BITMAP) != 0 && it.value != 0 {
                    // Icons and bitmaps are not drawn yet: a framed square.
                    let s = inner.width().min(inner.height()) - 4;
                    let (mx, my) = (inner.left + inner.width() / 2, inner.bottom + inner.height() / 2);
                    let ir = Rect::new(mx - s / 2, my - s / 2, mx - s / 2 + s, my - s / 2 + s);
                    c.fill(ir, draw::WINDOW);
                    c.frame(ir, draw::TEXT);
                }
                if focus && (row, col) == sel {
                    c.focus_rect(Rect::new(cell.left - 1, cell.bottom - 1, cell.right + 1, cell.top + 1));
                }
            }
        }
    });
}

pub extern "C" fn valueset_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            // VSCDATA: cbSize, usRowCount, usColumnCount.
            let (rows, cols) = if mp1 != 0 {
                unsafe { (((mp1 + 4) as *const u16).read_unaligned(), ((mp1 + 6) as *const u16).read_unaligned()) }
            } else {
                (1, 1)
            };
            let (rows, cols) = (rows.max(1), cols.max(1));
            let style = ctl::style(h);
            let attr = [VS_BITMAP, VS_ICON, VS_TEXT, VS_RGB, VS_COLORINDEX].iter().fold(0, |a, &f| a | (style & f));
            let items = alloc::vec![Item { attr, value: 0, text: Vec::new() }; rows as usize * cols as usize];
            ctl::set_data(h, ValueSet { rows, cols, items, sel: (1, 1), space: 2 });
            0
        }
        WM_SIZE => {
            ctl::invalidate(h);
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
            crate::win::WinSetFocus(1, h);
            let p = ctl::mouse_pos(mp1);
            let b = border(h);
            if let Some((row, col)) = grid(h).hit(p.x - b, p.y - b) {
                if select(h, row, col, true) && msg == WM_BUTTON1DBLCLK {
                    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), VN_ENTER), mp(row as u32, col as u32));
                }
            }
            1
        }
        WM_CHAR => {
            let k = Key::from(mp1, mp2);
            if k.up() {
                return WinDefWindowProc(h, msg, mp1, mp2);
            }
            let Some(d) = data(h) else { return 0 };
            let (row, col, rows, cols) = (d.sel.0, d.sel.1, d.rows, d.cols);
            let to = match k.vk {
                VK_LEFT => (row, col.saturating_sub(1).max(1)),
                VK_RIGHT => (row, (col + 1).min(cols)),
                VK_UP => (row.saturating_sub(1).max(1), col),
                VK_DOWN => ((row + 1).min(rows), col),
                VK_HOME => (row, 1),
                VK_END => (row, cols),
                VK_ENTER | VK_NEWLINE | VK_SPACE => {
                    send(ctl::owner(h), WM_CONTROL, mp(ctl::id(h), VN_ENTER), mp(row as u32, col as u32));
                    return 1;
                }
                _ => return WinDefWindowProc(h, msg, mp1, mp2),
            };
            select(h, to.0, to.1, true);
            1
        }
        WM_SETFOCUS => {
            ctl::invalidate(h);
            ctl::notify(h, if mp2 & 0xffff != 0 { VN_SETFOCUS } else { VN_KILLFOCUS });
            0
        }
        WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        WM_QUERYDLGCODE => 0,
        VM_SETITEM => {
            let Some(i) = index(h, lo(mp1) as u16, hi(mp1) as u16) else { return 0 };
            let Some(d) = data(h) else { return 0 };
            let it = &mut d.items[i];
            it.value = mp2;
            if it.attr & VIA_TEXT != 0 {
                it.text = ctl::cstr(mp2 as *const u8);
            }
            ctl::invalidate(h);
            1
        }
        VM_QUERYITEM => {
            let Some(i) = index(h, lo(mp1) as u16, hi(mp1) as u16) else { return 0 };
            let Some(d) = data(h) else { return 0 };
            let it = &d.items[i];
            if it.attr & VIA_TEXT != 0 {
                // mp2: (cchText, pszText) as MPFROM2SHORT? No: VM_QUERYITEM
                // with text takes a VSTEXT {pszItemText, ulBufLen}.
                if mp2 != 0 {
                    let (p, n) = unsafe { ((mp2 as *const u32).read_unaligned(), ((mp2 + 4) as *const u32).read_unaligned()) };
                    if p != 0 {
                        ctl::copy_out(&it.text, n as usize, p as *mut u8);
                    }
                }
                return it.text.len() as u32;
            }
            it.value
        }
        VM_SETITEMATTR => {
            let Some(i) = index(h, lo(mp1) as u16, hi(mp1) as u16) else { return 0 };
            let Some(d) = data(h) else { return 0 };
            let (attr, on) = (lo(mp2), hi(mp2) != 0);
            let it = &mut d.items[i];
            // The kinds of item exclude each other.
            const KINDS: u32 = VIA_BITMAP | VIA_ICON | VIA_TEXT | VIA_RGB | VIA_COLORINDEX | VIA_OWNERDRAW;
            if on {
                if attr & KINDS != 0 {
                    it.attr &= !KINDS;
                }
                it.attr |= attr;
            } else {
                it.attr &= !attr;
            }
            ctl::invalidate(h);
            1
        }
        VM_QUERYITEMATTR => index(h, lo(mp1) as u16, hi(mp1) as u16).and_then(|i| data(h).map(|d| d.items[i].attr)).unwrap_or(0),
        VM_SELECTITEM => select(h, lo(mp1) as u16, hi(mp1) as u16, true) as u32,
        VM_QUERYSELECTEDITEM => data(h).map_or(0, |d| mp(d.sel.0 as u32, d.sel.1 as u32)),
        VM_SETMETRICS => {
            let Some(d) = data(h) else { return 0 };
            match mp1 {
                VMA_ITEMSPACING => d.space = (lo(mp2) as i32).clamp(0, 32),
                VMA_ITEMSIZE => {}
                _ => return 0,
            }
            ctl::invalidate(h);
            1
        }
        VM_QUERYMETRICS => {
            let g = grid(h);
            match mp1 {
                VMA_ITEMSIZE => {
                    let (l, b, r, t) = g.cell(1, 1);
                    mp((r - l) as u32, (t - b) as u32)
                }
                VMA_ITEMSPACING => mp(g.space as u32, g.space as u32),
                _ => 0,
            }
        }
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
