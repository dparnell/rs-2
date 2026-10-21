//! The tree of categories and applications with check boxes: checking a
//! category checks all its applications (a partly checked one shows a
//! filled box), each application can be checked by itself. Mouse: the
//! arrow opens or closes a category, the box (or a double click) checks;
//! keys: Up, Down, PgUp, PgDn, Home, End, Space (check), Left/Right or -/+
//! (close/open). Changes are told to the owner as WM_CONTROL (id,
//! TN_CHANGED).

use crate::pm::*;
use crate::{app, App};
use alloc::vec::Vec;

pub const CLASS: &[u8] = b"AppInstTree\0";
pub const TN_CHANGED: u32 = 1;
const ROW: i32 = 20;
const ID_SCROLL: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Cat(usize),
    App(usize, usize),
}

/// The tree's state.
pub struct Tree {
    pub hwnd: u32,
    scroll: u32,
    pub open: Vec<bool>,
    pub checked: Vec<Vec<bool>>,
    pub installed: Vec<Vec<bool>>,
    pub focus: usize,
    top: usize,
}

impl Tree {
    pub fn new(sizes: &[usize]) -> Self {
        Tree {
            hwnd: 0,
            scroll: 0,
            open: sizes.iter().map(|_| true).collect(),
            checked: sizes.iter().map(|&n| alloc::vec![false; n]).collect(),
            installed: sizes.iter().map(|&n| alloc::vec![false; n]).collect(),
            focus: 0,
            top: 0,
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        let mut v = Vec::new();
        for (c, apps) in self.checked.iter().enumerate() {
            v.push(Row::Cat(c));
            if self.open[c] {
                v.extend((0..apps.len()).map(|a| Row::App(c, a)));
            }
        }
        v
    }

    pub fn focused(&self) -> Option<Row> {
        self.rows().get(self.focus).copied()
    }

    /// 0 none, 1 some, 2 all of a category's applications checked.
    pub fn cat_state(&self, c: usize) -> u8 {
        let n = self.checked[c].iter().filter(|&&b| b).count();
        if n == 0 {
            0
        } else if n == self.checked[c].len() {
            2
        } else {
            1
        }
    }

    fn toggle(&mut self, r: Row) {
        match r {
            Row::Cat(c) => {
                let on = self.cat_state(c) != 2;
                self.checked[c].iter_mut().for_each(|b| *b = on);
            }
            Row::App(c, a) => self.checked[c][a] = !self.checked[c][a],
        }
    }

    fn visible(&self) -> usize {
        (window_rect(self.hwnd).t / ROW).max(1) as usize
    }

    fn show_focus(&mut self) {
        let vis = self.visible();
        if self.focus < self.top {
            self.top = self.focus;
        } else if self.focus >= self.top + vis {
            self.top = self.focus + 1 - vis;
        }
        self.clamp();
    }

    fn clamp(&mut self) {
        let n = self.rows().len();
        let vis = self.visible();
        self.top = self.top.min(n.saturating_sub(vis));
        self.focus = self.focus.min(n.saturating_sub(1));
        let max = n.saturating_sub(vis);
        send(self.scroll, SBM_SETSCROLLBAR, self.top as u32, (max as u32) << 16);
        send(self.scroll, SBM_SETTHUMBSIZE, (vis as u32) | ((n as u32) << 16), 0);
    }
}

pub fn register(hab: u32) {
    unsafe { WinRegisterClass(hab, CLASS.as_ptr(), tree_proc, 0, 0) };
}

/// Lay the scroll bar out after the tree is made or sized.
pub fn placed(t: &mut Tree) {
    let r = window_rect(t.hwnd);
    let sb = sysval(SV_CXVSCROLL).max(12);
    if t.scroll == 0 {
        t.scroll = unsafe {
            WinCreateWindow(t.hwnd, b"#8\0".as_ptr(), b"\0".as_ptr(), WS_VISIBLE | 1, 0, 0, 0, 0, t.hwnd, HWND_TOP, ID_SCROLL, core::ptr::null(), core::ptr::null())
        };
    }
    unsafe { WinSetWindowPos(t.scroll, 0, r.r - sb, 0, sb, r.t, SWP_MOVE | SWP_SIZE) };
    t.clamp();
}

fn changed(a: &mut App) {
    unsafe {
        WinInvalidateRect(a.tree.hwnd, core::ptr::null(), 0);
        WinSendMsg(WinQueryWindow(a.tree.hwnd, QW_OWNER), WM_CONTROL, crate::ID_TREE | (TN_CHANGED << 16), 0);
    }
}

fn text(hps: u32, x: i32, y: i32, s: &[u8], bold: bool) {
    unsafe {
        GpiCharStringAt(hps, &PointL { x, y }, s.len() as i32, s.as_ptr());
        if bold {
            GpiCharStringAt(hps, &PointL { x: x + 1, y }, s.len() as i32, s.as_ptr());
        }
    }
}

/// A check box with its bottom left corner at (x, y): 0 empty, 1 partly,
/// 2 checked.
fn checkbox(hps: u32, x: i32, y: i32, state: u8, fg: i32) {
    unsafe {
        GpiSetColor(hps, CLR_WHITE);
        GpiMove(hps, &PointL { x, y });
        GpiBox(hps, DRO_FILL, &PointL { x: x + 12, y: y + 12 }, 0, 0);
        GpiSetColor(hps, fg);
        GpiMove(hps, &PointL { x, y });
        GpiBox(hps, DRO_OUTLINE, &PointL { x: x + 12, y: y + 12 }, 0, 0);
        match state {
            1 => {
                GpiMove(hps, &PointL { x: x + 3, y: y + 3 });
                GpiBox(hps, DRO_FILL, &PointL { x: x + 9, y: y + 9 }, 0, 0);
            }
            2 => {
                for d in 0..2 {
                    GpiMove(hps, &PointL { x: x + 2, y: y + 6 + d });
                    GpiLine(hps, &PointL { x: x + 5, y: y + 3 + d });
                    GpiLine(hps, &PointL { x: x + 10, y: y + 9 + d });
                }
            }
            _ => {}
        }
    }
}

fn paint(a: &mut App, h: u32) {
    let hps = unsafe { WinBeginPaint(h, 0, core::ptr::null_mut()) };
    let r = window_rect(h);
    let width = r.r - sysval(SV_CXVSCROLL).max(12);
    unsafe {
        WinFillRect(hps, &r, SYSCLR_WINDOW);
        GpiSetBackMix(hps, BM_LEAVEALONE);
    }
    let focused = unsafe { WinQueryFocus(HWND_DESKTOP) } == h;
    let t = &a.tree;
    for (i, row) in t.rows().into_iter().enumerate().skip(t.top) {
        let y = r.t - ((i - t.top) as i32 + 1) * ROW;
        if y + ROW < 0 {
            break;
        }
        let sel = i == t.focus;
        let fg = if sel && focused { SYSCLR_HILITEFOREGROUND } else { SYSCLR_WINDOWTEXT };
        if sel {
            let band = RectL { l: 0, b: y, r: width, t: y + ROW };
            unsafe { WinFillRect(hps, &band, if focused { SYSCLR_HILITEBACKGROUND } else { CLR_PALEGRAY }) };
        }
        let base = y + 5;
        match row {
            Row::Cat(c) => {
                unsafe { GpiSetColor(hps, fg) };
                // Code page 850's arrows: right (closed), down (open).
                text(hps, 4, base, if t.open[c] { b"\x1f" } else { b"\x10" }, false);
                checkbox(hps, 20, y + 4, t.cat_state(c), SYSCLR_WINDOWTEXT);
                let cat = &a.cats[c];
                let mut s = cat.title.to_vec();
                s.extend_from_slice(b" (");
                s.extend_from_slice(&dec(cat.apps.len() as u32));
                s.push(b')');
                unsafe { GpiSetColor(hps, fg) };
                text(hps, 40, base, &s, true);
            }
            Row::App(c, i) => {
                checkbox(hps, 40, y + 4, if t.checked[c][i] { 2 } else { 0 }, SYSCLR_WINDOWTEXT);
                unsafe { GpiSetColor(hps, fg) };
                text(hps, 60, base, a.cats[c].apps[i].name, false);
                if t.installed[c][i] {
                    let tag = b"installed";
                    unsafe { GpiSetColor(hps, if sel && focused { fg } else { CLR_DARKGRAY }) };
                    text(hps, width - 8 * tag.len() as i32 - 8, base, tag, false);
                }
            }
        }
    }
    unsafe { WinEndPaint(hps) };
}

pub fn dec(mut n: u32) -> Vec<u8> {
    let mut v = Vec::new();
    loop {
        v.insert(0, b'0' + (n % 10) as u8);
        n /= 10;
        if n == 0 {
            return v;
        }
    }
}

/// The row at window y, if any.
fn row_at(t: &Tree, y: i32) -> Option<usize> {
    let r = window_rect(t.hwnd);
    if y < 0 || y >= r.t {
        return None;
    }
    let i = t.top + ((r.t - 1 - y) / ROW) as usize;
    (i < t.rows().len()).then_some(i)
}

extern "C" fn tree_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    if a.tree.hwnd != h {
        return unsafe { WinDefWindowProc(h, msg, mp1, mp2) };
    }
    match msg {
        WM_PAINT => {
            paint(a, h);
            0
        }
        WM_SIZE => {
            placed(&mut a.tree);
            0
        }
        WM_SETFOCUS => {
            unsafe { WinInvalidateRect(h, core::ptr::null(), 0) };
            0
        }
        WM_BUTTON1DOWN | WM_BUTTON1DBLCLK => {
            unsafe { WinSetFocus(HWND_DESKTOP, h) };
            let (x, y) = (lo(mp1) as i16 as i32, hi(mp1) as i16 as i32);
            let Some(i) = row_at(&a.tree, y) else { return 1 };
            a.tree.focus = i;
            let row = a.tree.rows()[i];
            match row {
                Row::Cat(c) if x < 18 || (msg == WM_BUTTON1DBLCLK && x >= 36) => a.tree.open[c] = !a.tree.open[c],
                Row::Cat(_) if (18..36).contains(&x) => crate::toggle(a, row),
                Row::App(..) if (38..56).contains(&x) || msg == WM_BUTTON1DBLCLK => crate::toggle(a, row),
                _ => {}
            }
            a.tree.show_focus();
            changed(a);
            1
        }
        WM_VSCROLL => {
            let vis = a.tree.visible();
            let top = a.tree.top as i32;
            let to = match hi(mp2) {
                SB_LINEUP => top - 1,
                SB_LINEDOWN => top + 1,
                SB_PAGEUP => top - vis as i32,
                SB_PAGEDOWN => top + vis as i32,
                SB_SLIDERTRACK | SB_SLIDERPOSITION => lo(mp2) as i32,
                _ => return 0,
            };
            a.tree.top = to.max(0) as usize;
            a.tree.clamp();
            unsafe { WinInvalidateRect(h, core::ptr::null(), 0) };
            0
        }
        WM_CHAR => {
            if mp1 & KC_KEYUP != 0 {
                return 1;
            }
            let n = a.tree.rows().len();
            let vis = a.tree.visible();
            let row = a.tree.focused();
            let ch = if mp1 & KC_CHAR != 0 { lo(mp2) as u8 } else { 0 };
            let vk = if mp1 & KC_VIRTUALKEY != 0 { hi(mp2) } else { 0 };
            match (vk, ch) {
                (VK_UP, _) => a.tree.focus = a.tree.focus.saturating_sub(1),
                (VK_DOWN, _) => a.tree.focus = (a.tree.focus + 1).min(n.saturating_sub(1)),
                (VK_PAGEUP, _) => a.tree.focus = a.tree.focus.saturating_sub(vis),
                (VK_PAGEDOWN, _) => a.tree.focus = (a.tree.focus + vis).min(n.saturating_sub(1)),
                (VK_HOME, _) => a.tree.focus = 0,
                (VK_END, _) => a.tree.focus = n.saturating_sub(1),
                (0x1a, _) | (_, b' ') => {
                    if let Some(r) = row {
                        crate::toggle(a, r);
                    }
                }
                (VK_LEFT, _) | (_, b'-') => match row {
                    Some(Row::Cat(c)) => a.tree.open[c] = false,
                    Some(Row::App(c, _)) => a.tree.focus = a.tree.rows().iter().position(|&r| r == Row::Cat(c)).unwrap_or(0),
                    None => {}
                },
                (VK_RIGHT, _) | (_, b'+') => {
                    if let Some(Row::Cat(c)) = row {
                        a.tree.open[c] = true;
                    }
                }
                _ => return unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
            }
            a.tree.show_focus();
            changed(a);
            1
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

/// Check or uncheck a row with the owner's rules (see `crate::toggle`).
pub fn set(t: &mut Tree, r: Row) {
    t.toggle(r);
}
