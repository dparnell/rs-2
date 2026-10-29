//! The EDIT control: single-line (scrolling sideways) and multi-line
//! (lines ending in CR LF, word-wrapped unless it scrolls sideways),
//! selection with the mouse and the shift keys, the clipboard (Ctrl+X,
//! Ctrl+C, Ctrl+V, Shift+Del, Ctrl+Ins, Shift+Ins) through PM's, one
//! level of undo, the window's scroll bars, and the EM_ messages and EN_
//! notifications. The text is kept here (EM_GETHANDLE has no local
//! block to give; EM_SETHANDLE's block is copied).

use alloc::vec::Vec;

use crate::caret;
use crate::ctl::{ctl_color, CTLCOLOR_EDIT};
use crate::dialog::*;
use crate::gdi;
use crate::user::*;

const ES_CENTER: u32 = 0x0001;
const ES_RIGHT: u32 = 0x0002;
const ES_MULTILINE: u32 = 0x0004;
const ES_UPPERCASE: u32 = 0x0008;
const ES_LOWERCASE: u32 = 0x0010;
const ES_PASSWORD: u32 = 0x0020;
const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_NOHIDESEL: u32 = 0x0100;
const ES_READONLY: u32 = 0x0800;
const ES_WANTRETURN: u32 = 0x1000;

const EN_SETFOCUS: u16 = 0x0100;
const EN_KILLFOCUS: u16 = 0x0200;
const EN_CHANGE: u16 = 0x0300;
const EN_UPDATE: u16 = 0x0400;
const EN_MAXTEXT: u16 = 0x0501;
const EN_HSCROLL: u16 = 0x0601;
const EN_VSCROLL: u16 = 0x0602;

const EM_GETSEL: u16 = W_USER;
const EM_SETSEL: u16 = W_USER + 1;
const EM_GETRECT: u16 = W_USER + 2;
const EM_SETRECT: u16 = W_USER + 3;
const EM_SETRECTNP: u16 = W_USER + 4;
const EM_LINESCROLL: u16 = W_USER + 6;
const EM_GETMODIFY: u16 = W_USER + 8;
const EM_SETMODIFY: u16 = W_USER + 9;
const EM_GETLINECOUNT: u16 = W_USER + 10;
const EM_LINEINDEX: u16 = W_USER + 11;
const EM_SETHANDLE: u16 = W_USER + 12;
const EM_GETHANDLE: u16 = W_USER + 13;
const EM_LINELENGTH: u16 = W_USER + 17;
const EM_REPLACESEL: u16 = W_USER + 18;
const EM_SETFONT: u16 = W_USER + 19;
const EM_GETLINE: u16 = W_USER + 20;
const EM_LIMITTEXT: u16 = W_USER + 21;
const EM_CANUNDO: u16 = W_USER + 22;
const EM_UNDO: u16 = W_USER + 23;
const EM_FMTLINES: u16 = W_USER + 24;
const EM_LINEFROMCHAR: u16 = W_USER + 25;
const EM_SETTABSTOPS: u16 = W_USER + 27;
const EM_SETPASSWORDCHAR: u16 = W_USER + 28;
const EM_EMPTYUNDOBUFFER: u16 = W_USER + 29;
const EM_GETFIRSTVISIBLELINE: u16 = W_USER + 30;
const EM_SETREADONLY: u16 = W_USER + 31;
const EM_GETPASSWORDCHAR: u16 = W_USER + 34;

pub struct Edit {
    text: Vec<u8>,
    anchor: usize,
    caret: usize,
    font: u16,
    widths: [u8; 256],
    lh: i32,
    avew: i32,
    /// Lines: (start, end); the end excludes the line break.
    lines: Vec<(usize, usize)>,
    top: usize,
    xoff: i32,
    limit: usize,
    modified: bool,
    undo: Option<(Vec<u8>, usize, usize)>,
    pw: u8,
    tabs: Vec<i32>,
    focus: bool,
    captured: bool,
    rect: Option<[i32; 4]>,
}

fn ed(h: u16) -> Option<&'static mut Edit> {
    data::<Edit>(h)
}

fn style(h: u16) -> u32 {
    wnd(h).map_or(0, |w| w.style)
}

fn multi(h: u16) -> bool {
    style(h) & ES_MULTILINE != 0
}

/// The formatting rectangle.
fn rect(h: u16) -> [i32; 4] {
    if let Some(r) = ed(h).and_then(|e| e.rect) {
        return r;
    }
    let (cx, cy) = client_size(h);
    if multi(h) {
        [2, 1, (cx - 2).max(3), cy]
    } else {
        let lh = ed(h).map_or(16, |e| e.lh);
        let t = ((cy - lh) / 2).max(0);
        [2, t, (cx - 2).max(3), (t + lh).min(cy.max(lh))]
    }
}

fn measure(h: u16) {
    let font = ed(h).map_or(0, |e| e.font);
    let hdc = get_dc(h);
    gdi::select_font(hdc, font);
    let w = gdi::char_widths(hdc);
    let lh = gdi::line_height(hdc);
    let a = gdi::ave_width(hdc);
    gdi::release_dc(hdc);
    if let Some(e) = ed(h) {
        e.widths = w;
        e.lh = lh.max(1);
        e.avew = a.max(1);
    }
}

impl Edit {
    fn cw(&self, c: u8) -> i32 {
        self.widths[if self.pw != 0 { self.pw } else { c } as usize] as i32
    }

    fn next_tab(&self, x: i32) -> i32 {
        for &t in &self.tabs {
            if t > x {
                return t;
            }
        }
        let step = match self.tabs.last() {
            Some(&l) if self.tabs.len() == 1 && l > 0 => l,
            _ => self.avew * 8,
        }
        .max(1);
        (x / step + 1) * step
    }

    /// The width of text[a..b] starting at x 0 of its line.
    fn width(&self, a: usize, b: usize) -> i32 {
        let mut x = 0;
        for &c in &self.text[a..b] {
            x = if c == b'\t' && self.pw == 0 { self.next_tab(x) } else { x + self.cw(c) };
        }
        x
    }

    fn line_of(&self, i: usize) -> usize {
        match self.lines.iter().rposition(|&(s, _)| s <= i) {
            Some(l) => l,
            None => 0,
        }
    }

    /// The character index nearest to x on a line.
    fn index_at(&self, line: usize, x: i32) -> usize {
        let Some(&(s, e)) = self.lines.get(line) else { return self.text.len() };
        let mut cx = 0;
        for i in s..e {
            let c = self.text[i];
            let nx = if c == b'\t' && self.pw == 0 { self.next_tab(cx) } else { cx + self.cw(c) };
            if x < (cx + nx) / 2 {
                return i;
            }
            cx = nx;
        }
        e
    }

    fn sel(&self) -> (usize, usize) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }

    /// Break the text into lines: at line breaks, and at the width
    /// (words first) when it wraps.
    fn reflow(&mut self, multi: bool, wrap: Option<i32>) {
        self.lines.clear();
        let n = self.text.len();
        if !multi {
            self.lines.push((0, n));
            return;
        }
        let mut s = 0;
        loop {
            let e = self.text[s..].iter().position(|&c| c == b'\r' || c == b'\n').map_or(n, |i| s + i);
            match wrap {
                Some(w) if w > 0 => {
                    let mut a = s;
                    loop {
                        let mut x = 0;
                        let mut i = a;
                        let mut last_space = None;
                        while i < e {
                            let c = self.text[i];
                            x = if c == b'\t' { self.next_tab(x) } else { x + self.cw(c) };
                            if x > w && i > a {
                                break;
                            }
                            if c == b' ' {
                                last_space = Some(i);
                            }
                            i += 1;
                        }
                        if i >= e {
                            self.lines.push((a, e));
                            break;
                        }
                        let brk = last_space.map_or(i, |sp| sp + 1);
                        self.lines.push((a, brk));
                        a = brk;
                    }
                }
                _ => self.lines.push((s, e)),
            }
            if e >= n {
                break;
            }
            s = if self.text[e] == b'\r' && self.text.get(e + 1) == Some(&b'\n') { e + 2 } else { e + 1 };
            if s >= n {
                self.lines.push((n, n));
                break;
            }
        }
    }

    /// Positions are never inside a CR LF.
    fn fix(&self, i: usize) -> usize {
        let i = i.min(self.text.len());
        if i > 0 && i < self.text.len() && self.text[i - 1] == b'\r' && self.text[i] == b'\n' { i - 1 } else { i }
    }

    fn prev(&self, i: usize) -> usize {
        if i == 0 {
            return 0;
        }
        if i >= 2 && self.text[i - 2] == b'\r' && self.text[i - 1] == b'\n' { i - 2 } else { i - 1 }
    }

    fn next(&self, i: usize) -> usize {
        let n = self.text.len();
        if i >= n {
            return n;
        }
        if self.text[i] == b'\r' && self.text.get(i + 1) == Some(&b'\n') { i + 2 } else { i + 1 }
    }

    fn word_left(&self, i: usize) -> usize {
        let mut i = i;
        while i > 0 && !self.text[i - 1].is_ascii_alphanumeric() {
            i = self.prev(i);
        }
        while i > 0 && self.text[i - 1].is_ascii_alphanumeric() {
            i -= 1;
        }
        i
    }

    fn word_right(&self, i: usize) -> usize {
        let n = self.text.len();
        let mut i = i;
        while i < n && self.text[i].is_ascii_alphanumeric() {
            i += 1;
        }
        while i < n && !self.text[i].is_ascii_alphanumeric() {
            i = self.next(i);
        }
        i
    }
}

/// The wrap width (None: the control scrolls sideways).
fn wrap_width(h: u16) -> Option<i32> {
    let s = style(h);
    if s & ES_MULTILINE == 0 || s & (ES_AUTOHSCROLL | WS_HSCROLL) != 0 {
        return None;
    }
    let r = rect(h);
    Some(r[2] - r[0])
}

fn reflow(h: u16) {
    let (m, w) = (multi(h), wrap_width(h));
    if let Some(e) = ed(h) {
        e.reflow(m, w);
    }
}

fn visible_lines(h: u16) -> usize {
    let r = rect(h);
    let lh = ed(h).map_or(16, |e| e.lh);
    (((r[3] - r[1]) / lh).max(1)) as usize
}

/// The caret's position in the client area.
fn caret_xy(h: u16) -> (i32, i32) {
    let r = rect(h);
    let Some(e) = ed(h) else { return (0, 0) };
    let l = e.line_of(e.caret);
    let (s, _) = e.lines.get(l).copied().unwrap_or((0, 0));
    let x = r[0] - e.xoff + align_off(h, e, l, r) + e.width(s, e.caret.max(s));
    let y = r[1] + (l as i32 - e.top as i32) * e.lh;
    (x, y)
}

/// The offset of a line for ES_CENTER and ES_RIGHT (multi-line).
fn align_off(h: u16, e: &Edit, l: usize, r: [i32; 4]) -> i32 {
    let st = style(h);
    if st & ES_MULTILINE == 0 || st & (ES_CENTER | ES_RIGHT) == 0 {
        return 0;
    }
    let (s, en) = e.lines.get(l).copied().unwrap_or((0, 0));
    let w = e.width(s, en);
    let room = (r[2] - r[0]) - w;
    if st & ES_RIGHT != 0 { room.max(0) } else { (room / 2).max(0) }
}

fn place_caret(h: u16) {
    let Some(e) = ed(h) else { return };
    if !e.focus {
        return;
    }
    let (x, y) = caret_xy(h);
    caret::set_pos(x, y);
}

/// Scroll so the caret shows: whether it scrolled.
fn ensure_visible(h: u16) -> bool {
    let r = rect(h);
    let vis = visible_lines(h);
    let m = multi(h);
    let (cx, _) = caret_xy(h);
    let Some(e) = ed(h) else { return false };
    let (old_top, old_x) = (e.top, e.xoff);
    if m {
        let l = e.line_of(e.caret);
        if l < e.top {
            e.top = l;
        } else if l >= e.top + vis {
            e.top = l + 1 - vis;
        }
    }
    let w = r[2] - r[0];
    if cx < r[0] {
        e.xoff -= r[0] - cx + w / 3;
    } else if cx >= r[2] {
        e.xoff += cx - r[2] + 1 + w / 3;
    }
    e.xoff = e.xoff.max(0);
    let scrolled_v = e.top != old_top;
    let scrolled_h = e.xoff != old_x;
    if scrolled_v {
        notify(h, EN_VSCROLL);
    }
    if scrolled_h {
        notify(h, EN_HSCROLL);
    }
    scrolled_v || scrolled_h
}

/// The window scroll bars follow the text.
fn update_bars(h: u16) {
    let st = style(h);
    if st & (WS_VSCROLL | WS_HSCROLL) == 0 {
        return;
    }
    let r = rect(h);
    let Some(e) = ed(h) else { return };
    let (n, top, avew) = (e.lines.len(), e.top, e.avew);
    let widest = e.lines.iter().map(|&(s, en)| e.width(s, en)).max().unwrap_or(0);
    let xoff = e.xoff;
    if st & WS_VSCROLL != 0 {
        crate::scroll::set_range_keep(h, 1, 0, n.saturating_sub(1) as i32);
        crate::scroll::set_pos(h, 1, top as i32);
    }
    if st & WS_HSCROLL != 0 {
        let max = ((widest - (r[2] - r[0])) / avew + 1).max(0);
        crate::scroll::set_range_keep(h, 0, 0, max.max(xoff / avew));
        crate::scroll::set_pos(h, 0, xoff / avew);
    }
}

fn refresh(h: u16) {
    ensure_visible(h);
    update_bars(h);
    invalidate(h, None);
    place_caret(h);
}

fn readonly(h: u16) -> bool {
    style(h) & ES_READONLY != 0
}

/// Replace the selection with text (typing, pasting, EM_REPLACESEL).
fn replace_sel(h: u16, t: &[u8], user: bool) -> bool {
    if user && readonly(h) {
        crate::user::call_beep();
        return false;
    }
    let st = style(h);
    let mut t: Vec<u8> = t.to_vec();
    if st & ES_MULTILINE == 0 {
        if let Some(i) = t.iter().position(|&c| c == b'\r' || c == b'\n') {
            t.truncate(i);
        }
    }
    if st & ES_UPPERCASE != 0 {
        t.make_ascii_uppercase();
    } else if st & ES_LOWERCASE != 0 {
        t.make_ascii_lowercase();
    }
    let Some(e) = ed(h) else { return false };
    let (lo, hi) = e.sel();
    let room = e.limit.saturating_sub(e.text.len() - (hi - lo));
    let mut maxed = false;
    if t.len() > room {
        t.truncate(room);
        maxed = true;
    }
    if t.is_empty() && lo == hi {
        if maxed {
            notify(h, EN_MAXTEXT);
        }
        return false;
    }
    e.undo = Some((e.text.clone(), e.anchor, e.caret));
    e.text.splice(lo..hi, t.iter().copied());
    e.caret = lo + t.len();
    e.anchor = e.caret;
    e.modified = true;
    reflow(h);
    notify(h, EN_UPDATE);
    refresh(h);
    notify(h, EN_CHANGE);
    if maxed {
        notify(h, EN_MAXTEXT);
    }
    true
}

fn set_sel(h: u16, a: usize, c: usize) {
    let Some(e) = ed(h) else { return };
    let (a, c) = (e.fix(a), e.fix(c));
    if (e.anchor, e.caret) != (a, c) {
        e.anchor = a;
        e.caret = c;
        refresh(h);
    }
}

fn copy(h: u16) {
    let Some(e) = ed(h) else { return };
    if e.pw != 0 {
        return;
    }
    let (lo, hi) = e.sel();
    if lo < hi {
        let t = e.text[lo..hi].to_vec();
        caret::set_clip_text(&t);
    }
}

fn paste(h: u16) {
    if let Some(t) = caret::clip_text() {
        replace_sel(h, &t, true);
    }
}

fn undo(h: u16) -> bool {
    let Some(e) = ed(h) else { return false };
    let Some((t, a, c)) = e.undo.take() else { return false };
    let cur = (core::mem::replace(&mut e.text, t), e.anchor, e.caret);
    e.undo = Some(cur);
    e.anchor = a;
    e.caret = c;
    e.modified = true;
    reflow(h);
    refresh(h);
    notify(h, EN_CHANGE);
    true
}

fn key(h: u16, vk: u16) {
    let shift = key_down(0x10);
    let ctrl = key_down(0x11);
    let m = multi(h);
    // Up, down and F4 in a combo box's edit control are the combo's.
    let parent = wnd(h).map_or(0, |w| w.parent);
    if matches!(vk, 0x26 | 0x28 | 0x73) && crate::listbox::is_combo(parent) {
        send(parent, W_KEYDOWN, vk, 0);
        return;
    }
    let vis = visible_lines(h);
    let r = rect(h);
    let Some(e) = ed(h) else { return };
    let (lo, hi) = e.sel();
    let line = e.line_of(e.caret);
    let x_now = {
        let (s, _) = e.lines.get(line).copied().unwrap_or((0, 0));
        e.width(s, e.caret.max(s))
    };
    let pos = match vk {
        // VK_LEFT, VK_RIGHT
        0x25 => {
            if !shift && lo != hi {
                lo
            } else if ctrl {
                e.word_left(e.caret)
            } else {
                e.prev(e.caret)
            }
        }
        0x27 => {
            if !shift && lo != hi {
                hi
            } else if ctrl {
                e.word_right(e.caret)
            } else {
                e.next(e.caret)
            }
        }
        // VK_UP, VK_DOWN, VK_PRIOR, VK_NEXT
        0x26 | 0x28 | 0x21 | 0x22 if m => {
            let d = match vk {
                0x26 => -1,
                0x28 => 1,
                0x21 => -(vis as i32),
                _ => vis as i32,
            };
            let nl = (line as i32 + d).clamp(0, e.lines.len() as i32 - 1) as usize;
            if vk == 0x21 || vk == 0x22 {
                e.top = (e.top as i32 + d).clamp(0, (e.lines.len() as i32 - vis as i32).max(0)) as usize;
            }
            e.index_at(nl, x_now)
        }
        // VK_HOME, VK_END
        0x24 => {
            if ctrl || !m {
                0
            } else {
                e.lines.get(line).map_or(0, |l| l.0)
            }
        }
        0x23 => {
            if ctrl || !m {
                e.text.len()
            } else {
                e.lines.get(line).map_or(0, |l| l.1)
            }
        }
        // VK_DELETE
        0x2e => {
            if shift {
                copy(h);
                replace_sel(h, b"", true);
            } else if lo != hi {
                replace_sel(h, b"", true);
            } else if !readonly(h) {
                let n = e.next(e.caret);
                if n > e.caret {
                    e.anchor = n;
                    replace_sel(h, b"", true);
                }
            }
            return;
        }
        // VK_INSERT
        0x2d => {
            if shift {
                paste(h);
            } else if ctrl {
                copy(h);
            }
            return;
        }
        _ => return,
    };
    let _ = r;
    let Some(e) = ed(h) else { return };
    e.caret = pos;
    if !shift {
        e.anchor = pos;
    }
    refresh(h);
}

fn char_in(h: u16, c: u16) {
    let ctrl = key_down(0x11);
    let m = multi(h);
    match c {
        // Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+Z
        0x03 if ctrl => copy(h),
        0x18 if ctrl => {
            copy(h);
            replace_sel(h, b"", true);
        }
        0x16 if ctrl => paste(h),
        0x1a if ctrl => {
            undo(h);
        }
        0x08 => {
            let Some(e) = ed(h) else { return };
            let (lo, hi) = e.sel();
            if lo == hi {
                if lo == 0 {
                    return;
                }
                e.anchor = e.prev(lo);
            }
            replace_sel(h, b"", true);
        }
        0x0d | 0x0a => {
            if m {
                replace_sel(h, b"\r\n", true);
            }
        }
        0x09 => {
            if m {
                replace_sel(h, b"\t", true);
            }
        }
        c if c >= 0x20 && c < 0x100 => {
            replace_sel(h, &[c as u8], true);
        }
        _ => {}
    }
}

/// The character at a point of the client area.
fn hit(h: u16, x: i32, y: i32) -> usize {
    let r = rect(h);
    let Some(e) = ed(h) else { return 0 };
    let line = if multi(h) {
        let l = (y - r[1]).div_euclid(e.lh) + e.top as i32;
        l.clamp(0, e.lines.len() as i32 - 1) as usize
    } else {
        0
    };
    let off = align_off(h, e, line, r);
    e.index_at(line, x - r[0] + e.xoff - off)
}

fn paint(h: u16) {
    let hdc = begin_paint_rs(h);
    let (cx, cy) = client_size(h);
    let font = ed(h).map_or(0, |e| e.font);
    gdi::select_font(hdc, font);
    let br = ctl_color(h, hdc, CTLCOLOR_EDIT);
    gdi::fill_rect(hdc, [0, 0, cx, cy], br);
    let st = style(h);
    let r = rect(h);
    let fg = if st & WS_DISABLED != 0 { gdi::sys_color(gdi::COLOR_GRAYTEXT) } else { gdi::text_color(hdc) };
    let (hfg, hbg) = (gdi::sys_color(gdi::COLOR_HIGHLIGHTTEXT), gdi::sys_color(gdi::COLOR_HIGHLIGHT));
    gdi::set_bk_mode(hdc, gdi::TRANSPARENT);
    gdi::clip(hdc, [r[0], 0, r[2], cy]);
    if let Some(e) = ed(h) {
        let (lo, hi) = e.sel();
        let show_sel = (e.focus || st & ES_NOHIDESEL != 0) && lo != hi;
        let vis = visible_lines(h) + 1;
        for l in e.top..(e.top + vis).min(e.lines.len()) {
            let (s, en) = e.lines[l];
            let y = r[1] + (l - e.top) as i32 * e.lh;
            if y >= cy {
                break;
            }
            let x0 = r[0] - e.xoff + align_off(h, e, l, r);
            // Runs of tab-free text, the selected ones highlighted.
            let mut x = 0;
            let mut i = s;
            while i < en {
                let selected = show_sel && i >= lo && i < hi;
                let mut j = i;
                while j < en && e.text[j] != b'\t' && (show_sel && j >= lo && j < hi) == selected {
                    j += 1;
                }
                if j == i {
                    // A tab.
                    let nx = if e.pw != 0 { x + e.cw(b'\t') } else { e.next_tab(x) };
                    if selected {
                        gdi::fill(hdc, [x0 + x, y, x0 + nx, y + e.lh], hbg);
                    }
                    x = nx;
                    i += 1;
                    continue;
                }
                let run: Vec<u8> = if e.pw != 0 { alloc::vec![e.pw; j - i] } else { e.text[i..j].to_vec() };
                let w: i32 = run.iter().map(|&c| e.widths[c as usize] as i32).sum();
                if x0 + x < r[2] && x0 + x + w > r[0] {
                    if selected {
                        gdi::fill(hdc, [x0 + x, y, x0 + x + w, y + e.lh], hbg);
                        gdi::set_text_color(hdc, hfg);
                    } else {
                        gdi::set_text_color(hdc, fg);
                    }
                    gdi::text_at(hdc, x0 + x, y, &run);
                }
                x += w;
                i = j;
            }
            // A selected line break shows as a small block.
            if show_sel && en < e.text.len() && en >= lo && en < hi && multi(h) {
                gdi::fill(hdc, [x0 + x, y, x0 + x + e.avew / 2, y + e.lh], hbg);
            }
        }
    }
    gdi::unclip(hdc);
    gdi::release_dc(hdc);
}

fn scroll(h: u16, vert: bool, code: u16, pos: i32) {
    let vis = visible_lines(h);
    let r = rect(h);
    let Some(e) = ed(h) else { return };
    if vert {
        let n = e.lines.len() as i32;
        let t = e.top as i32;
        let nt = match code {
            0 => t - 1,
            1 => t + 1,
            2 => t - vis as i32,
            3 => t + vis as i32,
            4 | 5 => pos,
            6 => 0,
            7 => n,
            _ => return,
        };
        e.top = nt.clamp(0, (n - 1).max(0)) as usize;
        notify(h, EN_VSCROLL);
    } else {
        let w = r[2] - r[0];
        let nx = match code {
            0 => e.xoff - e.avew,
            1 => e.xoff + e.avew,
            2 => e.xoff - w / 2,
            3 => e.xoff + w / 2,
            4 | 5 => pos * e.avew,
            6 => 0,
            _ => return,
        };
        e.xoff = nx.max(0);
        notify(h, EN_HSCROLL);
    }
    update_bars(h);
    invalidate(h, None);
    place_caret(h);
}

pub fn proc_(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    match m {
        W_CREATE => {
            let Some(w) = wnd(h) else { return 0 };
            let text = core::mem::take(&mut w.text);
            let pw = if w.style & ES_PASSWORD != 0 { b'*' } else { 0 };
            let multi = w.style & ES_MULTILINE != 0;
            w.data = Some(alloc::boxed::Box::new(Edit {
                text: Vec::new(),
                anchor: 0,
                caret: 0,
                font: 0,
                widths: [8; 256],
                lh: 16,
                avew: 8,
                lines: Vec::new(),
                top: 0,
                xoff: 0,
                limit: if multi { 0xfffe } else { 30000 },
                modified: false,
                undo: None,
                pw,
                tabs: Vec::new(),
                focus: false,
                captured: false,
                rect: None,
            }));
            measure(h);
            if let Some(e) = ed(h) {
                e.text = text;
            }
            reflow(h);
            update_bars(h);
            0
        }
        W_ERASEBKGND => 1,
        W_PAINT => {
            paint(h);
            0
        }
        W_SIZE => {
            reflow(h);
            refresh(h);
            0
        }
        W_SETFONT | EM_SETFONT => {
            if let Some(e) = ed(h) {
                e.font = wp;
            }
            measure(h);
            reflow(h);
            if lp & 0xffff != 0 {
                refresh(h);
            }
            0
        }
        W_GETFONT => ed(h).map_or(0, |e| e.font as u32),
        W_SETTEXT => {
            let t = crate::far_str(lp).unwrap_or(b"").to_vec();
            let Some(e) = ed(h) else { return 0 };
            e.text = t;
            let n = e.limit;
            e.text.truncate(n);
            e.anchor = 0;
            e.caret = 0;
            e.top = 0;
            e.xoff = 0;
            e.modified = false;
            e.undo = None;
            reflow(h);
            refresh(h);
            if !multi(h) {
                notify(h, EN_CHANGE);
            }
            1
        }
        W_GETTEXT => {
            let t = ed(h).map(|e| e.text.clone()).unwrap_or_default();
            copy_out(&t, thunk16::flat(lp), wp as i32) as u32
        }
        W_GETTEXTLENGTH => ed(h).map_or(0, |e| e.text.len() as u32),
        W_GETDLGCODE => {
            let mut c = DLGC_WANTCHARS | DLGC_HASSETSEL | DLGC_WANTARROWS;
            if multi(h) && style(h) & ES_WANTRETURN != 0 {
                c |= DLGC_WANTALLKEYS;
            }
            c
        }
        W_SETFOCUS => {
            let lh = ed(h).map_or(16, |e| e.lh);
            if let Some(e) = ed(h) {
                e.focus = true;
            }
            caret::create(h, false, 2, lh);
            place_caret(h);
            caret::show(h, true);
            invalidate(h, None);
            notify(h, EN_SETFOCUS);
            0
        }
        W_KILLFOCUS => {
            if let Some(e) = ed(h) {
                e.focus = false;
                if e.captured {
                    e.captured = false;
                    unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, 0) };
                }
            }
            caret::destroy();
            invalidate(h, None);
            notify(h, EN_KILLFOCUS);
            0
        }
        W_ENABLE => {
            invalidate(h, None);
            0
        }
        W_KEYDOWN => {
            key(h, wp);
            0
        }
        W_CHAR => {
            char_in(h, wp);
            0
        }
        W_LBUTTONDOWN => {
            if focus() != h {
                set_focus(h);
            }
            let (x, y) = (lp as i16 as i32, (lp >> 16) as i16 as i32);
            let i = hit(h, x, y);
            let shift = wp & 0x0004 != 0;
            if let Some(c) = wnd(h).map(|w| w.client) {
                unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, c) };
            }
            let Some(e) = ed(h) else { return 0 };
            e.captured = true;
            e.caret = i;
            if !shift {
                e.anchor = i;
            }
            refresh(h);
            0
        }
        W_MOUSEMOVE => {
            if ed(h).is_some_and(|e| e.captured) {
                let (x, y) = (lp as i16 as i32, (lp >> 16) as i16 as i32);
                let i = hit(h, x, y);
                if let Some(e) = ed(h) {
                    if e.caret != i {
                        e.caret = i;
                        refresh(h);
                    }
                }
            }
            0
        }
        W_LBUTTONUP => {
            if let Some(e) = ed(h) {
                if e.captured {
                    e.captured = false;
                    unsafe { crate::pm::WinSetCapture(crate::pm::HWND_DESKTOP, 0) };
                }
            }
            0
        }
        W_LBUTTONDBLCLK => {
            let (x, y) = (lp as i16 as i32, (lp >> 16) as i16 as i32);
            let i = hit(h, x, y);
            let Some(e) = ed(h) else { return 0 };
            let mut a = i;
            while a > 0 && e.text[a - 1].is_ascii_alphanumeric() {
                a -= 1;
            }
            let mut b = i;
            while b < e.text.len() && e.text[b].is_ascii_alphanumeric() {
                b += 1;
            }
            set_sel(h, a, b);
            0
        }
        W_VSCROLL | W_HSCROLL => {
            scroll(h, m == W_VSCROLL, wp, lp as u16 as i16 as i32);
            0
        }
        W_CUT => {
            copy(h);
            replace_sel(h, b"", true);
            0
        }
        W_COPY => {
            copy(h);
            0
        }
        W_PASTE => {
            paste(h);
            0
        }
        W_CLEAR => {
            replace_sel(h, b"", true);
            0
        }
        W_UNDO | EM_UNDO => undo(h) as u32,
        EM_GETSEL => ed(h).map_or(0, |e| {
            let (lo, hi) = e.sel();
            (hi as u32) << 16 | lo as u32
        }),
        EM_SETSEL => {
            let (a, b) = (lp as u16, (lp >> 16) as u16);
            let n = ed(h).map_or(0, |e| e.text.len());
            if a == 0xffff {
                let c = ed(h).map_or(0, |e| e.caret);
                set_sel(h, c, c);
            } else {
                let b = if b == 0xffff || b as usize > n { n } else { b as usize };
                set_sel(h, (a as usize).min(n), b);
            }
            1
        }
        EM_GETRECT => {
            crate::write_rect(lp, rect(h));
            0
        }
        EM_SETRECT | EM_SETRECTNP => {
            if let Some(r) = crate::read_rect(lp) {
                if let Some(e) = ed(h) {
                    e.rect = Some(r);
                }
                reflow(h);
                if m == EM_SETRECT {
                    refresh(h);
                }
            }
            0
        }
        EM_LINESCROLL => {
            let (dv, dh) = (lp as u16 as i16 as i32, (lp >> 16) as u16 as i16 as i32);
            let Some(e) = ed(h) else { return 0 };
            let n = e.lines.len() as i32;
            e.top = (e.top as i32 + dv).clamp(0, (n - 1).max(0)) as usize;
            e.xoff = (e.xoff + dh * e.avew).max(0);
            update_bars(h);
            invalidate(h, None);
            place_caret(h);
            1
        }
        EM_GETMODIFY => ed(h).is_some_and(|e| e.modified) as u32,
        EM_SETMODIFY => {
            if let Some(e) = ed(h) {
                e.modified = wp != 0;
            }
            0
        }
        EM_GETLINECOUNT => ed(h).map_or(1, |e| e.lines.len().max(1) as u32),
        EM_LINEINDEX => {
            let Some(e) = ed(h) else { return 0xffff_ffff };
            let l = if wp == 0xffff { e.line_of(e.caret) } else { wp as usize };
            e.lines.get(l).map_or(0xffff_ffff, |&(s, _)| s as u32)
        }
        EM_LINELENGTH => {
            let Some(e) = ed(h) else { return 0 };
            if wp == 0xffff {
                let (lo, hi) = e.sel();
                let (l0, l1) = (e.line_of(lo), e.line_of(hi));
                let (s0, _) = e.lines[l0];
                let (_, e1) = e.lines[l1];
                return ((lo - s0) + (e1 - hi.min(e1))) as u32;
            }
            let l = e.line_of(wp as usize);
            e.lines.get(l).map_or(0, |&(s, en)| (en - s) as u32)
        }
        EM_REPLACESEL => {
            let t = crate::far_str(lp).unwrap_or(b"").to_vec();
            replace_sel(h, &t, false);
            0
        }
        EM_GETLINE => {
            let p = thunk16::flat(lp);
            let Some(e) = ed(h) else { return 0 };
            if p.is_null() {
                return 0;
            }
            let max = unsafe { (p as *const u16).read_unaligned() } as usize;
            let Some(&(s, en)) = e.lines.get(wp as usize) else { return 0 };
            let n = (en - s).min(max);
            unsafe { core::ptr::copy_nonoverlapping(e.text[s..].as_ptr(), p, n) };
            n as u32
        }
        EM_LIMITTEXT => {
            if let Some(e) = ed(h) {
                e.limit = if wp == 0 { 0xfffe } else { wp as usize };
            }
            0
        }
        EM_CANUNDO => ed(h).is_some_and(|e| e.undo.is_some()) as u32,
        EM_EMPTYUNDOBUFFER => {
            if let Some(e) = ed(h) {
                e.undo = None;
            }
            0
        }
        EM_FMTLINES => wp as u32,
        EM_LINEFROMCHAR => {
            let Some(e) = ed(h) else { return 0 };
            let i = if wp == 0xffff { e.sel().0 } else { wp as usize };
            e.line_of(i) as u32
        }
        EM_SETTABSTOPS => {
            let (bx, _) = base_units();
            let Some(e) = ed(h) else { return 0 };
            e.tabs.clear();
            let p = thunk16::flat(lp) as *const i16;
            if wp == 1 && !p.is_null() {
                e.tabs.push(unsafe { p.read_unaligned() } as i32 * bx / 4);
            } else if !p.is_null() {
                for i in 0..wp as usize {
                    e.tabs.push(unsafe { p.add(i).read_unaligned() } as i32 * bx / 4);
                }
            }
            reflow(h);
            invalidate(h, None);
            1
        }
        EM_SETPASSWORDCHAR => {
            if let Some(e) = ed(h) {
                e.pw = wp as u8;
            }
            reflow(h);
            invalidate(h, None);
            0
        }
        EM_GETPASSWORDCHAR => ed(h).map_or(0, |e| e.pw as u32),
        EM_GETFIRSTVISIBLELINE => ed(h).map_or(0, |e| e.top as u32),
        EM_SETREADONLY => {
            if let Some(w) = wnd(h) {
                if wp != 0 {
                    w.style |= ES_READONLY;
                } else {
                    w.style &= !ES_READONLY;
                }
            }
            1
        }
        EM_GETHANDLE => 0,
        // EM_SETHANDLE(hlocal): the text in a local block of the control's
        // instance segment (a local handle is its offset here); programs
        // make that a segment of its own with LocalInit (Delphi's
        // installer shows its README so).
        EM_SETHANDLE => {
            let seg = wnd(h).map_or(0, |w| w.hinst);
            if seg != 0 && wp != 0 {
                let t = crate::far_str((seg as u32) << 16 | wp as u32).unwrap_or(b"").to_vec();
                let Some(e) = ed(h) else { return 0 };
                e.text = t;
                e.anchor = 0;
                e.caret = 0;
                e.top = 0;
                e.xoff = 0;
                e.modified = false;
                e.undo = None;
                reflow(h);
                refresh(h);
            }
            0
        }
        _ => def_window_proc(h, m, wp, lp),
    }
}
