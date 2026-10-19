//! Multi-line entry fields (WC_MLE): the window around the text engine
//! of `pmwin::mle` (crates/pmwin): painting with the control's font and
//! colours, keyboard and mouse editing, scroll bars (WC_SCROLLBAR
//! children), the clipboard, and the MLM_* messages and MLN_*
//! notifications.

use alloc::vec::Vec;

use gpi::Rect;
use pmwin::mle::{Edit, Format, Mle, OP_TYPING};

use crate::ctl::{self, *};
use crate::draw::{self, Canvas};
use crate::lock::wndproc;
use crate::msg::{send, WinDefWindowProc};
use crate::state::st;
use crate::win::{self, WM_BUTTON1DOWN, WM_BUTTON1UP, WM_CHAR, WM_MOUSEMOVE, WM_PAINT, WM_SETFOCUS, WM_SIZE};

const WM_CREATE: u32 = 0x0001;

const MLS_WORDWRAP: u32 = 0x0001;
const MLS_BORDER: u32 = 0x0002;
const MLS_VSCROLL: u32 = 0x0004;
const MLS_HSCROLL: u32 = 0x0008;
const MLS_READONLY: u32 = 0x0010;
const MLS_IGNORETAB: u32 = 0x0020;
const MLS_DISABLEUNDO: u32 = 0x0040;

const MLM_SETTEXTLIMIT: u32 = 0x01b0;
const MLM_QUERYTEXTLIMIT: u32 = 0x01b1;
const MLM_SETFORMATRECT: u32 = 0x01b2;
const MLM_QUERYFORMATRECT: u32 = 0x01b3;
const MLM_SETWRAP: u32 = 0x01b4;
const MLM_QUERYWRAP: u32 = 0x01b5;
const MLM_SETTABSTOP: u32 = 0x01b6;
const MLM_QUERYTABSTOP: u32 = 0x01b7;
const MLM_SETREADONLY: u32 = 0x01b8;
const MLM_QUERYREADONLY: u32 = 0x01b9;
const MLM_QUERYCHANGED: u32 = 0x01ba;
const MLM_SETCHANGED: u32 = 0x01bb;
const MLM_QUERYLINECOUNT: u32 = 0x01bc;
const MLM_CHARFROMLINE: u32 = 0x01bd;
const MLM_LINEFROMCHAR: u32 = 0x01be;
const MLM_QUERYLINELENGTH: u32 = 0x01bf;
const MLM_QUERYTEXTLENGTH: u32 = 0x01c0;
const MLM_FORMAT: u32 = 0x01c1;
const MLM_SETIMPORTEXPORT: u32 = 0x01c2;
const MLM_IMPORT: u32 = 0x01c3;
const MLM_EXPORT: u32 = 0x01c4;
const MLM_DELETE: u32 = 0x01c6;
const MLM_QUERYFORMATLINELENGTH: u32 = 0x01c7;
const MLM_QUERYFORMATTEXTLENGTH: u32 = 0x01c8;
const MLM_INSERT: u32 = 0x01c9;
const MLM_SETSEL: u32 = 0x01ca;
const MLM_QUERYSEL: u32 = 0x01cb;
const MLM_QUERYSELTEXT: u32 = 0x01cc;
const MLM_QUERYUNDO: u32 = 0x01cd;
const MLM_UNDO: u32 = 0x01ce;
const MLM_RESETUNDO: u32 = 0x01cf;
const MLM_QUERYFONT: u32 = 0x01d0;
const MLM_SETFONT: u32 = 0x01d1;
const MLM_SETTEXTCOLOR: u32 = 0x01d2;
const MLM_QUERYTEXTCOLOR: u32 = 0x01d3;
const MLM_SETBACKCOLOR: u32 = 0x01d4;
const MLM_QUERYBACKCOLOR: u32 = 0x01d5;
const MLM_QUERYFIRSTCHAR: u32 = 0x01d6;
const MLM_SETFIRSTCHAR: u32 = 0x01d7;
const MLM_CUT: u32 = 0x01d8;
const MLM_COPY: u32 = 0x01d9;
const MLM_PASTE: u32 = 0x01da;
const MLM_CLEAR: u32 = 0x01db;
const MLM_ENABLEREFRESH: u32 = 0x01dc;
const MLM_DISABLEREFRESH: u32 = 0x01dd;
const MLM_SEARCH: u32 = 0x01de;
const MLM_QUERYIMPORTEXPORT: u32 = 0x01df;

const MLN_OVERFLOW: u32 = 0x0001;
const MLN_VSCROLL: u32 = 0x0005;
const MLN_HSCROLL: u32 = 0x0006;
const MLN_CHANGE: u32 = 0x0007;
const MLN_SETFOCUS: u32 = 0x0008;
const MLN_KILLFOCUS: u32 = 0x0009;
const MLN_CLPBDFAIL: u32 = 0x000f;

const MLFETL_TEXTBYTES: u32 = 0x0020;
const MLFFMTRECT_LIMITHORZ: u32 = 0x0001;
const MLFFMTRECT_MATCHWINDOW: u32 = 0x0004;
const MLFSEARCH_CASESENSITIVE: u32 = 0x0001;
const MLFSEARCH_SELECTMATCH: u32 = 0x0002;
const MLFSEARCH_CHANGEALL: u32 = 0x0004;

const SBM_SETSCROLLBAR: u32 = 0x01a0;
const SBM_SETTHUMBSIZE: u32 = 0x01a6;

const SCROLL_W: i32 = 16;
/// Pels between the border and the text.
const INSET: i32 = 2;

const FATTR_SEL_UNDERSCORE: u16 = 0x0002;
const FATTR_SEL_BOLD: u16 = 0x0020;
const FATTR_FONTUSE_OUTLINE: u16 = 0x0004;

wndproc! {
    mle_entry => mle_proc;
}

pub struct MleWin {
    m: Mle,
    vscroll: u32,
    hscroll: u32,
    /// FATTRS of the font, its cell size and look.
    fattrs: [u8; 56],
    cw: i32,
    ch: i32,
    bold: bool,
    underline: bool,
    /// Colours as set (index or RGB) and resolved.
    fg_val: i32,
    bg_val: i32,
    /// Import/export buffer.
    ie_buf: u32,
    ie_len: u32,
    /// MLM_DISABLEREFRESH depth.
    no_refresh: u32,
    tracking: bool,
    /// Format rectangle set by the application (pels; None: the window).
    fmt_width: Option<i32>,
    fmt_flags: u32,
    tab_set: bool,
}

fn data(h: u32) -> Option<&'static mut MleWin> {
    ctl::data::<MleWin>(h)
}

/// The window is an MLE.
pub fn is_mle(h: u32) -> bool {
    data(h).is_some()
}

fn color_rgb(v: i32) -> u32 {
    if (-50..=15).contains(&v) { crate::color::resolve(v) } else { v as u32 & 0xff_ffff }
}

fn system_fattrs() -> [u8; 56] {
    let f = draw::font();
    let mut v = [0u8; 56];
    v[0..2].copy_from_slice(&56u16.to_le_bytes());
    v[4..8].copy_from_slice(&1i32.to_le_bytes());
    v[8..18].copy_from_slice(b"System VIO");
    v[42..44].copy_from_slice(&850u16.to_le_bytes());
    v[44..48].copy_from_slice(&f.height.to_le_bytes());
    v[48..52].copy_from_slice(&f.width.to_le_bytes());
    v
}

/// Cell size and look of a FATTRS: an image font at its size, an outline
/// face scaled to lMaxBaselineExt (the glyphs are the system font's).
fn font_from(fa: &[u8; 56]) -> (i32, i32, bool, bool) {
    let sel = u16::from_le_bytes([fa[2], fa[3]]);
    let use_ = u16::from_le_bytes([fa[54], fa[55]]);
    let face = &fa[8..40];
    let face = &face[..face.iter().position(|&b| b == 0).unwrap_or(32)];
    let h = i32::from_le_bytes([fa[44], fa[45], fa[46], fa[47]]);
    let w = i32::from_le_bytes([fa[48], fa[49], fa[50], fa[51]]);
    let f = draw::font();
    let outline = use_ & FATTR_FONTUSE_OUTLINE != 0 || !face.starts_with(b"System");
    let ch = if h > 0 { h.clamp(6, 96) } else { f.height };
    let cw = if w > 0 && !outline { w.clamp(3, 64) } else if outline { (ch / 2).max(3) } else { f.width };
    let bold = sel & FATTR_SEL_BOLD != 0 || face.ends_with(b"Bold");
    (cw, ch, bold, sel & FATTR_SEL_UNDERSCORE != 0)
}

/// The text area (window coordinates), inside the border and the scroll
/// bars.
fn text_area(h: u32) -> Rect {
    let (cx, cy) = ctl::size(h);
    let style = ctl::style(h);
    let b = if style & MLS_BORDER != 0 { 2 } else { 0 };
    let r = if style & MLS_VSCROLL != 0 { SCROLL_W } else { 0 };
    let bt = if style & MLS_HSCROLL != 0 { SCROLL_W } else { 0 };
    Rect::new(b + INSET, b + bt + 1, (cx - b - r - INSET).max(b + INSET), (cy - b - 1).max(b + bt + 1))
}

fn rows(h: u32) -> usize {
    let ch = data(h).map_or(16, |d| d.ch);
    (text_area(h).height() / ch).max(1) as usize
}

/// Lay the text out for the current window size and font.
fn layout(h: u32) {
    let area = text_area(h);
    let wrap = ctl::style(h) & MLS_WORDWRAP != 0;
    let Some(d) = data(h) else { return };
    let width = d.fmt_width.unwrap_or(area.width());
    let tab = if d.tab_set { d.m.tab_stop() } else { d.cw * 8 };
    d.m.set_layout(width, d.cw, tab, wrap);
}

fn refresh(h: u32) {
    if data(h).is_some_and(|d| d.no_refresh == 0) {
        ctl::invalidate(h);
    }
}

/// Scroll bar ranges and positions.
fn sync_scroll(h: u32) {
    let rows = rows(h);
    let aw = text_area(h).width();
    let Some(d) = data(h) else { return };
    let n = d.m.line_count();
    let max_first = n.saturating_sub(rows);
    if d.m.first_line > max_first && d.m.first_line > 0 {
        d.m.first_line = max_first;
    }
    let (vs, hs, first) = (d.vscroll, d.hscroll, d.m.first_line);
    let maxw = if hs != 0 { d.m.max_width() + d.cw } else { 0 };
    let xo = d.m.x_offset;
    if vs != 0 {
        send(vs, SBM_SETSCROLLBAR, first as u32, mp(0, max_first.min(0x7fff) as u32));
        send(vs, SBM_SETTHUMBSIZE, mp(rows as u32, n.min(0x7fff) as u32), 0);
    }
    if hs != 0 {
        let maxx = (maxw - aw).max(0);
        send(hs, SBM_SETSCROLLBAR, xo as u32, mp(0, maxx.min(0x7fff) as u32));
        send(hs, SBM_SETTHUMBSIZE, mp(aw.max(1) as u32, maxw.clamp(1, 0x7fff) as u32), 0);
    }
}

fn place_scrolls(h: u32) {
    let (cx, cy) = ctl::size(h);
    let style = ctl::style(h);
    let b = if style & MLS_BORDER != 0 { 2 } else { 0 };
    let Some(d) = data(h) else { return };
    let (vs, hs) = (d.vscroll, d.hscroll);
    let bt = if hs != 0 { SCROLL_W } else { 0 };
    let r = if vs != 0 { SCROLL_W } else { 0 };
    if vs != 0 {
        st().desk.set_pos(vs, cx - b - SCROLL_W, b + bt, SCROLL_W, (cy - 2 * b - bt).max(0));
    }
    if hs != 0 {
        st().desk.set_pos(hs, b, b, (cx - 2 * b - r).max(0), SCROLL_W);
    }
}

/// Scroll so the cursor is visible; tells the owner when it scrolled.
fn reveal(h: u32) {
    let rows = rows(h);
    let aw = text_area(h).width();
    let Some(d) = data(h) else { return };
    let line = d.m.line_of(d.m.cursor);
    let (of, ox) = (d.m.first_line, d.m.x_offset);
    if line < d.m.first_line {
        d.m.first_line = line;
    } else if line >= d.m.first_line + rows {
        d.m.first_line = line + 1 - rows;
    }
    let x = d.m.x_of(d.m.cursor);
    if ctl::style(h) & MLS_WORDWRAP == 0 {
        if x < d.m.x_offset {
            d.m.x_offset = (x - aw / 4).max(0);
        } else if x >= d.m.x_offset + aw - d.cw {
            d.m.x_offset = x - aw + aw / 4;
        }
    } else {
        d.m.x_offset = 0;
    }
    let (vs, hs) = (d.m.first_line != of, d.m.x_offset != ox);
    sync_scroll(h);
    if vs || hs {
        refresh(h);
    }
    if vs {
        ctl::notify(h, MLN_VSCROLL);
    }
    if hs {
        ctl::notify(h, MLN_HSCROLL);
    }
}

/// After the text changed: layout is done by the engine; scroll, repaint
/// and tell the owner.
fn changed(h: u32) {
    reveal(h);
    sync_scroll(h);
    refresh(h);
    ctl::notify(h, MLN_CHANGE);
}

fn edit_result(h: u32, r: Edit) {
    match r {
        Edit::Done => changed(h),
        Edit::Overflow(n) => overflow(h, n),
        Edit::ReadOnly => {
            crate::misc::WinAlarm(1, 0);
        }
        Edit::Nothing => {}
    }
}

/// The text limit was hit: MLN_OVERFLOW with an MLEOVERFLOW.
fn overflow(h: u32, over: usize) {
    let ov: [u32; 4] = [MLFETL_TEXTBYTES, over as u32, 0, 0];
    let id = ctl::id(h);
    send(ctl::owner(h), WM_CONTROL, mp(id, MLN_OVERFLOW), ov.as_ptr() as u32);
    crate::misc::WinAlarm(1, 0);
}

/// Text in the MLE's import/export format converted to the engine's form.
fn internal(h: u32, t: &[u8]) -> Vec<u8> {
    let f = data(h).map_or(Format::CfText, |d| d.m.format);
    Mle::from_format(if f == Format::NoTrans { Format::CfText } else { f }, t).0
}

// ---- Clipboard ----

fn copy(h: u32) -> bool {
    let Some(d) = data(h) else { return false };
    let (a, b) = d.m.selection();
    if a == b {
        return false;
    }
    let (t, _) = d.m.export(a, b - a, Format::CfText, usize::MAX);
    if !crate::clip::set_text(&t) {
        ctl::notify(h, MLN_CLPBDFAIL);
        return false;
    }
    true
}

fn clear(h: u32) -> bool {
    let Some(d) = data(h) else { return false };
    let (a, b) = d.m.selection();
    if a == b {
        return false;
    }
    let r = d.m.replace(a, b, b"", MLM_CLEAR);
    edit_result(h, r);
    r == Edit::Done
}

fn paste(h: u32) -> bool {
    let Some(t) = crate::clip::text() else { return false };
    let t = Mle::from_format(Format::CfText, &t).0;
    let Some(d) = data(h) else { return false };
    let r = d.m.insert(&t, MLM_PASTE);
    edit_result(h, r);
    r == Edit::Done
}

// ---- Keyboard and mouse ----

fn key(h: u32, mp1: u32, mp2: u32) -> bool {
    let k = Key::from(mp1, mp2);
    if k.up() {
        return false;
    }
    let style = ctl::style(h);
    let (shift, ctrl, alt) = (k.shift(), k.ctrl(), k.alt());
    let rows = rows(h) as isize;
    // The clipboard keys (Shift+Ins, Ctrl+Ins, Shift+Del).
    match k.vk {
        VK_INSERT if shift && !ctrl && !alt => {
            paste(h);
            return true;
        }
        VK_INSERT if ctrl && !shift && !alt => {
            copy(h);
            return true;
        }
        VK_DELETE if shift && !ctrl && !alt => {
            if copy(h) {
                clear(h);
            }
            return true;
        }
        _ => {}
    }
    let Some(d) = data(h) else { return false };
    let m = &mut d.m;
    match k.vk {
        VK_LEFT if !alt => {
            if ctrl { m.word_left(shift) } else { m.left(shift) }
        }
        VK_RIGHT if !alt => {
            if ctrl { m.word_right(shift) } else { m.right(shift) }
        }
        VK_UP if !alt && !ctrl => m.vertical(-1, shift),
        VK_DOWN if !alt && !ctrl => m.vertical(1, shift),
        VK_PAGEUP if !alt => {
            m.first_line = m.first_line.saturating_sub(rows as usize);
            m.vertical(-rows, shift);
        }
        VK_PAGEDOWN if !alt => {
            let max = m.line_count().saturating_sub(rows as usize);
            m.first_line = (m.first_line + rows as usize).min(max);
            m.vertical(rows, shift);
        }
        VK_HOME if !alt => {
            if ctrl { m.doc_start(shift) } else { m.home(shift) }
        }
        VK_END if !alt => {
            if ctrl { m.doc_end(shift) } else { m.end(shift) }
        }
        VK_INSERT if !alt => {
            m.insert = !m.insert;
        }
        VK_DELETE if !alt => {
            let r = m.delete_forward();
            edit_result(h, r);
            return true;
        }
        VK_BACKSPACE if alt && !ctrl => {
            if m.undo() {
                changed(h);
            }
            return true;
        }
        VK_BACKSPACE if !alt => {
            let r = m.delete_back();
            edit_result(h, r);
            return true;
        }
        VK_TAB if style & MLS_IGNORETAB == 0 && !alt && !ctrl && !shift => {
            let r = m.type_char(b'\t');
            edit_result(h, r);
            return true;
        }
        VK_NEWLINE | VK_ENTER if !alt && !ctrl => {
            let r = m.type_char(b'\n');
            edit_result(h, r);
            return true;
        }
        _ => {
            let c = k.ch;
            if k.fs & KC_CHAR != 0 && c >= 0x20 && !ctrl && !alt && k.vk != VK_TAB && k.vk != VK_ESC && k.vk != VK_BACKSPACE {
                let r = m.type_char(c);
                edit_result(h, r);
                return true;
            }
            return false;
        }
    }
    // A cursor movement.
    reveal(h);
    refresh(h);
    true
}

/// Insertion point under a window position.
fn ipt_at(h: u32, x: i32, y: i32) -> usize {
    let area = text_area(h);
    let Some(d) = data(h) else { return 0 };
    let row = if y >= area.top { -1 } else { (area.top - 1 - y) / d.ch };
    let line = (d.m.first_line as i32 + row).max(0) as usize;
    d.m.ipt_at(line.min(d.m.line_count() - 1), x - area.left + d.m.x_offset)
}

// ---- Painting ----

/// Draw `t` with the glyphs scaled to cells of cw x ch, the baseline
/// cell bottom at y.
fn cells_text(c: &mut Canvas, x: i32, bottom: i32, t: &[u8], col: u32, clip: Rect, cw: i32, ch: i32, bold: bool) {
    let f = draw::font();
    if cw == f.width && ch == f.height {
        c.text(x, bottom + f.descender, t, col, clip);
        if bold {
            c.text(x + 1, bottom + f.descender, t, col, clip);
        }
        return;
    }
    for (i, &b) in t.iter().enumerate() {
        let x0 = x + i as i32 * cw;
        if x0 >= clip.right || x0 + cw <= clip.left {
            continue;
        }
        for row in 0..f.height {
            let bits = f.row(b, row as usize);
            if bits == 0 {
                continue;
            }
            let y1 = bottom + ch - row * ch / f.height;
            let y0 = bottom + ch - (row + 1) * ch / f.height;
            if y1 <= y0 {
                continue;
            }
            let mut col0 = 0;
            while col0 < 8 {
                if bits & (0x80 >> col0) == 0 {
                    col0 += 1;
                    continue;
                }
                let mut col1 = col0;
                while col1 < 8 && bits & (0x80 >> col1) != 0 {
                    col1 += 1;
                }
                let xa = x0 + col0 * cw / f.width;
                let xb = (x0 + col1 * cw / f.width).max(xa + 1) + bold as i32;
                c.fill(Rect::new(xa, y0, xb, y1).intersect(&clip), col);
                col0 = col1;
            }
        }
    }
}

fn paint(h: u32) {
    let style = ctl::style(h);
    let focus = ctl::has_focus(h);
    let enabled = ctl::enabled(h);
    let area = text_area(h);
    let (vs, hs) = (style & MLS_VSCROLL != 0, style & MLS_HSCROLL != 0);
    let Some(d) = data(h) else { return };
    let (cw, ch, bold, underline) = (d.cw, d.ch, d.bold, d.underline);
    let fg = if enabled { color_rgb(d.fg_val) } else { draw::GRAY_TEXT };
    let bg = color_rgb(d.bg_val);
    let m = &d.m;
    let rows = (area.height() + ch - 1) / ch;
    let (sa, sb) = m.selection();
    let first = m.first_line;
    let xo = m.x_offset;
    let cursor = m.cursor;
    let text = m.text();
    // What each visible line shows.
    let mut lines = Vec::new();
    for n in first..(first + rows as usize).min(m.line_count()) {
        let l = m.lines()[n];
        lines.push((l, m.cells(n)));
    }
    let cursor_line = m.line_of(cursor);
    let cursor_x = m.x_of(cursor);
    let insert = m.insert;
    let cursor_w = if !insert && cursor < text.len() && text[cursor] != b'\n' { cw } else { 2 };
    draw::paint(h, |c| {
        let r = c.rect();
        c.fill(r, bg);
        if style & MLS_BORDER != 0 {
            c.sunken(r);
            c.bevel(Rect::new(r.left + 1, r.bottom + 1, r.right - 1, r.top - 1), draw::DARK, draw::FACE);
        }
        if vs && hs {
            let b = if style & MLS_BORDER != 0 { 2 } else { 0 };
            c.fill(Rect::new(r.right - b - SCROLL_W, b, r.right - b, b + SCROLL_W), draw::FACE);
        }
        let clip = Rect::new(area.left - INSET, area.bottom, area.right + INSET, area.top);
        for (i, (l, cells)) in lines.iter().enumerate() {
            let top = area.top - i as i32 * ch;
            let bottom = top - ch;
            let x0 = area.left - xo;
            // Runs of plain characters (tabs are blanks).
            let mut run: Vec<u8> = Vec::new();
            let mut run_x = 0;
            let mut sel_run: Option<(i32, i32)> = None;
            for &(i2, x, w) in cells {
                let b = text[i2];
                let selected = focus_sel(i2, sa, sb);
                if selected {
                    let (a, e) = sel_run.unwrap_or((x, x));
                    sel_run = Some((a.min(x), e.max(x + w)));
                }
                if b == b'\t' {
                    if !run.is_empty() {
                        cells_text(c, x0 + run_x, bottom, &run, fg, clip, cw, ch, bold);
                        run.clear();
                    }
                    continue;
                }
                if run.is_empty() {
                    run_x = x;
                }
                run.push(b);
            }
            if !run.is_empty() {
                cells_text(c, x0 + run_x, bottom, &run, fg, clip, cw, ch, bold);
            }
            if underline {
                let w = cells.last().map_or(0, |&(_, x, w)| x + w);
                c.fill(Rect::new(x0, bottom + 1, x0 + w, bottom + 2).intersect(&clip), fg);
            }
            // The selection: highlight and draw again inverted; a selected
            // line break shows as a small block.
            let brk = l.hard && focus_sel(l.end, sa, sb);
            let lw = cells.last().map_or(0, |&(_, x, w)| x + w);
            if let Some((a, e)) = sel_run.or(brk.then_some((lw, lw))) {
                let e = if brk { e.max(lw) + cw / 2 } else { e };
                let sr = Rect::new(x0 + a, bottom, x0 + e, top).intersect(&clip);
                c.fill(sr, draw::HILITE);
                let sel_text: Vec<(usize, i32)> = cells.iter().filter(|&&(i2, _, _)| focus_sel(i2, sa, sb) && text[i2] != b'\t').map(|&(i2, x, _)| (i2, x)).collect();
                for (i2, x) in sel_text {
                    cells_text(c, x0 + x, bottom, &text[i2..i2 + 1], draw::HILITE_TEXT, sr, cw, ch, bold);
                }
            }
        }
        if focus && cursor_line >= first && cursor_line < first + rows as usize {
            let top = area.top - (cursor_line - first) as i32 * ch;
            let x = area.left - xo + cursor_x;
            let cr = Rect::new(x, top - ch, x + cursor_w, top).intersect(&clip);
            if cursor_w > 2 {
                c.fill(Rect::new(cr.left, cr.bottom, cr.right, cr.bottom + 2), fg);
            } else {
                c.fill(cr, fg);
            }
        }
    });
}

fn focus_sel(i: usize, a: usize, b: usize) -> bool {
    i >= a && i < b
}

// ---- Search ----

/// MLM_SEARCH(flags, pSearchData): MLE_SEARCHDATA is cb, pchFind,
/// pchReplace, cchFind, cchReplace, iptStart, iptStop, cchFound.
fn search(h: u32, fl: u32, p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let rd32 = |o: u32| unsafe { ((p + o) as *const u32).read_unaligned() };
    let rd16 = |o: u32| unsafe { ((p + o) as *const i16).read_unaligned() };
    let (pf, pr) = (rd32(4), rd32(8));
    let (cf, cr) = (rd16(12), rd16(14));
    let (start, stop) = (rd32(16) as i32, rd32(20) as i32);
    let take = |ptr: u32, n: i16| -> Vec<u8> {
        if ptr == 0 {
            return Vec::new();
        }
        if n < 0 { ctl::cstr(ptr as *const u8) } else { unsafe { core::slice::from_raw_parts(ptr as *const u8, n as usize) }.to_vec() }
    };
    let find = internal(h, &take(pf, cf));
    let repl = internal(h, &take(pr, cr));
    let case = fl & MLFSEARCH_CASESENSITIVE != 0;
    let Some(d) = data(h) else { return 0 };
    let start = if start < 0 { d.m.cursor } else { start as usize };
    let mut stop = if stop < 0 { usize::MAX } else { stop as usize };
    if fl & MLFSEARCH_CHANGEALL != 0 {
        let mut at = start;
        let mut n = 0;
        while let Some(i) = d.m.find(&find, at, stop, case) {
            if d.m.replace(i, i + find.len(), &repl, MLM_SEARCH) != Edit::Done {
                break;
            }
            n += 1;
            at = i + repl.len();
            if stop != usize::MAX {
                stop = (stop + repl.len()).saturating_sub(find.len());
            }
        }
        if n > 0 {
            changed(h);
        }
        return (n > 0) as u32;
    }
    let Some(i) = d.m.find(&find, start, stop, case) else { return 0 };
    unsafe {
        ((p + 16) as *mut u32).write_unaligned(i as u32);
        ((p + 24) as *mut u16).write_unaligned(find.len() as u16);
    }
    if fl & MLFSEARCH_SELECTMATCH != 0 {
        d.m.set_sel(i, i + find.len());
        reveal(h);
        refresh(h);
    }
    1
}

// ---- Window text (WinSetWindowText / WinQueryWindowText) ----

/// The text as CF_TEXT (CR LF line ends).
pub fn text(h: u32) -> Option<Vec<u8>> {
    let d = data(h)?;
    Some(d.m.export(0, usize::MAX, Format::CfText, usize::MAX).0)
}

pub fn set_text(h: u32, t: &[u8]) {
    let t = Mle::from_format(Format::CfText, t).0;
    let Some(d) = data(h) else { return };
    d.m.set_text(&t);
    d.m.changed = true;
    sync_scroll(h);
    refresh(h);
    ctl::notify(h, MLN_CHANGE);
}

// ---- The window procedure ----

fn create(h: u32, ctl_data: u32) {
    let style = ctl::style(h);
    let mut m = Mle::new();
    m.readonly = style & MLS_READONLY != 0;
    m.undo_enabled = style & MLS_DISABLEUNDO == 0;
    let mut fmt_width = None;
    let mut fmt_flags = MLFFMTRECT_MATCHWINDOW;
    let (mut anchor, mut cursor) = (0, 0);
    if ctl_data != 0 {
        // MLECTLDATA: cbCtlData, afIEFormat, cchText, iptAnchor,
        // iptCursor, cxFormat, cyFormat, afFormatFlags.
        let p = ctl_data;
        let cb = unsafe { (p as *const u16).read_unaligned() } as u32;
        let rd32 = |o: u32| if o + 4 <= cb { unsafe { ((p + o) as *const u32).read_unaligned() } } else { 0 };
        if cb >= 4 {
            m.format = Format::from_mlfie(unsafe { ((p + 2) as *const u16).read_unaligned() } as u32);
        }
        let limit = rd32(4) as i32;
        m.limit = if limit > 0 { Some(limit as usize) } else { None };
        (anchor, cursor) = (rd32(8) as usize, rd32(12) as usize);
        let cx = rd32(16) as i32;
        fmt_flags = rd32(28);
        if fmt_flags & MLFFMTRECT_MATCHWINDOW == 0 && fmt_flags & MLFFMTRECT_LIMITHORZ != 0 && cx > 0 {
            fmt_width = Some(cx);
        }
    }
    let f = draw::font();
    let text = ctl::text(h);
    let mut vscroll = 0;
    let mut hscroll = 0;
    if style & MLS_VSCROLL != 0 {
        vscroll = win::create(h, b"#8", b"", pmwin::ws::VISIBLE | 1, Rect::default(), h, 0xfffe, 0);
    }
    if style & MLS_HSCROLL != 0 {
        hscroll = win::create(h, b"#8", b"", pmwin::ws::VISIBLE, Rect::default(), h, 0xfffd, 0);
    }
    ctl::set_data(
        h,
        MleWin {
            m,
            vscroll,
            hscroll,
            fattrs: system_fattrs(),
            cw: f.width,
            ch: f.height,
            bold: false,
            underline: false,
            fg_val: crate::color::SYSCLR_WINDOWTEXT,
            bg_val: crate::color::SYSCLR_WINDOW,
            ie_buf: 0,
            ie_len: 0,
            no_refresh: 0,
            tracking: false,
            fmt_width,
            fmt_flags,
            tab_set: false,
        },
    );
    layout(h);
    if !text.is_empty() {
        let t = Mle::from_format(Format::CfText, &text).0;
        if let Some(d) = data(h) {
            d.m.set_text(&t);
        }
    }
    if let Some(d) = data(h) {
        d.m.set_sel(anchor, cursor);
        d.m.changed = false;
    }
    place_scrolls(h);
    sync_scroll(h);
}

pub extern "C" fn mle_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_CREATE => {
            create(h, mp1);
            0
        }
        WM_SIZE => {
            place_scrolls(h);
            layout(h);
            reveal(h);
            sync_scroll(h);
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
            if !ctl::has_focus(h) {
                crate::win::WinSetFocus(1, h);
            }
            let p = ctl::mouse_pos(mp1);
            let shift = (mp2 >> 16) & KC_SHIFT != 0;
            let i = ipt_at(h, p.x, p.y);
            let Some(d) = data(h) else { return 1 };
            if msg == WM_BUTTON1DBLCLK {
                let (a, b) = d.m.word_at(i);
                d.m.set_sel(a, b);
            } else {
                let anchor = if shift { d.m.anchor } else { i };
                d.m.set_sel(anchor, i);
                d.tracking = true;
                st().capture = h;
            }
            d.m.seal_undo();
            reveal(h);
            refresh(h);
            1
        }
        WM_MOUSEMOVE => {
            crate::pointer::set_system(pmwin::sysptr::SPTR_TEXT);
            if data(h).is_some_and(|d| d.tracking) {
                let p = ctl::mouse_pos(mp1);
                let area = text_area(h);
                // Dragging past the edge scrolls a line at a time.
                let y = if p.y >= area.top { area.top + 1 } else if p.y < area.bottom { area.bottom - data(h).map_or(16, |d| d.ch) } else { p.y };
                let i = ipt_at(h, p.x, y.min(area.top + 1));
                if let Some(d) = data(h) {
                    if d.m.cursor != i {
                        let a = d.m.anchor;
                        d.m.set_sel(a, i);
                        reveal(h);
                        refresh(h);
                    }
                }
            }
            1
        }
        WM_BUTTON1UP => {
            if let Some(d) = data(h) {
                d.tracking = false;
            }
            if st().capture == h {
                st().capture = 0;
            }
            1
        }
        WM_CHAR => {
            if key(h, mp1, mp2) {
                return 1;
            }
            WinDefWindowProc(h, msg, mp1, mp2)
        }
        WM_VSCROLL | WM_HSCROLL => {
            let rows = rows(h);
            let aw = text_area(h).width();
            let Some(d) = data(h) else { return 0 };
            let pos = lo(mp2) as i16 as i32;
            if msg == WM_VSCROLL {
                let first = d.m.first_line;
                let max = d.m.line_count().saturating_sub(rows);
                let new = match hi(mp2) {
                    1 => first.saturating_sub(1),
                    2 => first + 1,
                    3 => first.saturating_sub(rows.saturating_sub(1).max(1)),
                    4 => first + rows.saturating_sub(1).max(1),
                    5 | 6 => pos.max(0) as usize,
                    _ => first,
                }
                .min(max);
                if new != first {
                    d.m.first_line = new;
                    sync_scroll(h);
                    refresh(h);
                    ctl::notify(h, MLN_VSCROLL);
                }
            } else {
                let x = d.m.x_offset;
                let maxw = d.m.max_width() + d.cw;
                let max = (maxw - aw).max(0);
                let new = match hi(mp2) {
                    1 => x - d.cw,
                    2 => x + d.cw,
                    3 => x - aw / 2,
                    4 => x + aw / 2,
                    5 | 6 => pos,
                    _ => x,
                }
                .clamp(0, max);
                if new != x {
                    d.m.x_offset = new;
                    sync_scroll(h);
                    refresh(h);
                    ctl::notify(h, MLN_HSCROLL);
                }
            }
            0
        }
        WM_SETFOCUS => {
            ctl::invalidate(h);
            ctl::notify(h, if mp2 & 0xffff != 0 { MLN_SETFOCUS } else { MLN_KILLFOCUS });
            0
        }
        WM_ENABLE => {
            ctl::invalidate(h);
            0
        }
        WM_QUERYDLGCODE => 0x0400, // DLGC_MLE
        MLM_SETTEXTLIMIT => {
            let n = mp1 as i32;
            let Some(d) = data(h) else { return 1 };
            if n >= 0 && d.m.len() > n as usize {
                return 1;
            }
            d.m.limit = if n < 0 { None } else { Some(n as usize) };
            0
        }
        MLM_QUERYTEXTLIMIT => data(h).map_or(0, |d| d.m.limit.map_or(u32::MAX, |l| l as u32)),
        MLM_SETFORMATRECT => {
            let Some(d) = data(h) else { return 0 };
            d.fmt_flags = mp2;
            d.fmt_width = if mp2 & MLFFMTRECT_MATCHWINDOW == 0 && mp2 & MLFFMTRECT_LIMITHORZ != 0 && mp1 != 0 {
                Some(unsafe { (mp1 as *const i32).read_unaligned() }.max(1))
            } else {
                None
            };
            layout(h);
            sync_scroll(h);
            refresh(h);
            1
        }
        MLM_QUERYFORMATRECT => {
            let area = text_area(h);
            let Some(d) = data(h) else { return 0 };
            if mp1 != 0 {
                let w = d.fmt_width.unwrap_or(area.width());
                unsafe {
                    (mp1 as *mut i32).write_unaligned(w);
                    ((mp1 + 4) as *mut i32).write_unaligned(area.height());
                }
            }
            if mp2 != 0 {
                unsafe { (mp2 as *mut u32).write_unaligned(d.fmt_flags) };
            }
            1
        }
        MLM_SETWRAP => {
            if let Some(w) = st().desk.get_mut(h) {
                if mp1 & 0xffff != 0 {
                    w.style |= MLS_WORDWRAP;
                } else {
                    w.style &= !MLS_WORDWRAP;
                }
            }
            layout(h);
            if let Some(d) = data(h) {
                d.m.x_offset = 0;
            }
            reveal(h);
            sync_scroll(h);
            refresh(h);
            1
        }
        MLM_QUERYWRAP => (ctl::style(h) & MLS_WORDWRAP != 0) as u32,
        MLM_SETTABSTOP => {
            let Some(d) = data(h) else { return 0 };
            let t = (mp1 as i32).max(1);
            d.tab_set = true;
            let (w, cw, wrap) = (d.m.width(), d.cw, d.m.wrap);
            d.m.set_layout(w, cw, t, wrap);
            sync_scroll(h);
            refresh(h);
            t as u32
        }
        MLM_QUERYTABSTOP => data(h).map_or(0, |d| d.m.tab_stop() as u32),
        MLM_SETREADONLY => {
            let Some(d) = data(h) else { return 0 };
            let old = d.m.readonly;
            d.m.readonly = mp1 & 0xffff != 0;
            if let Some(w) = st().desk.get_mut(h) {
                if mp1 & 0xffff != 0 {
                    w.style |= MLS_READONLY;
                } else {
                    w.style &= !MLS_READONLY;
                }
            }
            old as u32
        }
        MLM_QUERYREADONLY => data(h).map_or(0, |d| d.m.readonly as u32),
        MLM_QUERYCHANGED => data(h).map_or(0, |d| d.m.changed as u32),
        MLM_SETCHANGED => data(h).map_or(0, |d| core::mem::replace(&mut d.m.changed, mp1 & 0xffff != 0) as u32),
        MLM_QUERYLINECOUNT => data(h).map_or(0, |d| d.m.line_count() as u32),
        MLM_CHARFROMLINE => {
            let Some(d) = data(h) else { return 0 };
            let n = if mp1 as i32 == -1 { d.m.line_of(d.m.cursor) } else { mp1 as usize };
            if n >= d.m.line_count() {
                return u32::MAX;
            }
            d.m.line_start(n) as u32
        }
        MLM_LINEFROMCHAR => {
            let Some(d) = data(h) else { return 0 };
            let i = if mp1 as i32 == -1 { d.m.cursor } else { mp1 as usize };
            if i > d.m.len() {
                return u32::MAX;
            }
            d.m.line_of(i) as u32
        }
        MLM_QUERYLINELENGTH => {
            let Some(d) = data(h) else { return 0 };
            let i = if mp1 as i32 == -1 { d.m.cursor } else { mp1 as usize };
            let l = d.m.lines()[d.m.line_of(i)];
            (l.end - l.start) as u32
        }
        MLM_QUERYTEXTLENGTH => data(h).map_or(0, |d| d.m.len() as u32),
        MLM_FORMAT => {
            let Some(d) = data(h) else { return 0 };
            d.m.format = Format::from_mlfie(mp1);
            1
        }
        MLM_QUERYIMPORTEXPORT => {
            let Some(d) = data(h) else { return 0 };
            if mp1 != 0 {
                unsafe { (mp1 as *mut u32).write_unaligned(d.ie_buf) };
            }
            if mp2 != 0 {
                unsafe { (mp2 as *mut u32).write_unaligned(d.ie_len) };
            }
            1
        }
        MLM_SETIMPORTEXPORT => {
            let Some(d) = data(h) else { return 0 };
            d.ie_buf = mp1;
            d.ie_len = mp2;
            1
        }
        MLM_IMPORT => {
            // (piptInsert, cbToImport): bytes imported.
            if mp1 == 0 {
                return 0;
            }
            let Some(d) = data(h) else { return 0 };
            if d.ie_buf == 0 {
                return 0;
            }
            let n = mp2.min(d.ie_len) as usize;
            let src = unsafe { core::slice::from_raw_parts(d.ie_buf as *const u8, n) }.to_vec();
            let at = unsafe { (mp1 as *const i32).read_unaligned() };
            let at = if at < 0 { d.m.cursor } else { at as usize };
            let f = d.m.format;
            match d.m.import(at, &src, f) {
                Ok((used, chars)) => {
                    unsafe { (mp1 as *mut u32).write_unaligned((at.min(d.m.len()) + chars) as u32) };
                    changed(h);
                    used as u32
                }
                Err(Edit::Overflow(over)) => {
                    overflow(h, over);
                    0
                }
                Err(_) => 0,
            }
        }
        MLM_EXPORT => {
            // (piptStart, pcb): bytes exported.
            if mp1 == 0 || mp2 == 0 {
                return 0;
            }
            let Some(d) = data(h) else { return 0 };
            if d.ie_buf == 0 {
                return 0;
            }
            let start = unsafe { (mp1 as *const i32).read_unaligned() }.max(0) as usize;
            let count = unsafe { (mp2 as *const u32).read_unaligned() } as usize;
            let (out, chars) = d.m.export(start, count, d.m.format, d.ie_len as usize);
            unsafe {
                core::ptr::copy_nonoverlapping(out.as_ptr(), d.ie_buf as *mut u8, out.len());
                (mp1 as *mut u32).write_unaligned((start + chars) as u32);
                (mp2 as *mut u32).write_unaligned((count - chars) as u32);
            }
            out.len() as u32
        }
        MLM_DELETE => {
            let Some(d) = data(h) else { return 0 };
            let n = d.m.delete(mp1 as usize, mp2 as usize);
            if n > 0 {
                changed(h);
            }
            n as u32
        }
        MLM_QUERYFORMATLINELENGTH => {
            let Some(d) = data(h) else { return 0 };
            let i = if mp1 as i32 == -1 { d.m.cursor } else { (mp1 as usize).min(d.m.len()) };
            let l = d.m.lines()[d.m.line_of(i)];
            let end = if l.hard { l.end + 1 } else { l.end };
            d.m.format_len(i, end.saturating_sub(i), d.m.format) as u32
        }
        MLM_QUERYFORMATTEXTLENGTH => data(h).map_or(0, |d| d.m.format_len(mp1 as usize, mp2 as usize, d.m.format) as u32),
        MLM_INSERT => {
            if mp1 == 0 {
                return 0;
            }
            let t = internal(h, &ctl::cstr(mp1 as *const u8));
            let Some(d) = data(h) else { return 0 };
            let r = d.m.insert(&t, MLM_INSERT);
            let ok = r == Edit::Done;
            edit_result(h, r);
            if ok { t.len() as u32 } else { 0 }
        }
        MLM_SETSEL => {
            let Some(d) = data(h) else { return 0 };
            d.m.set_sel(mp1 as usize, mp2 as usize);
            d.m.seal_undo();
            reveal(h);
            refresh(h);
            1
        }
        MLM_QUERYSEL => {
            let Some(d) = data(h) else { return 0 };
            let (a, b) = d.m.selection();
            match mp1 {
                0 => mp(a as u32, b as u32),
                1 => a as u32,
                2 => b as u32,
                3 => d.m.anchor as u32,
                _ => d.m.cursor as u32,
            }
        }
        MLM_QUERYSELTEXT => {
            let Some(d) = data(h) else { return 0 };
            let (a, b) = d.m.selection();
            let (t, _) = d.m.export(a, b - a, d.m.format, usize::MAX);
            if mp1 != 0 {
                unsafe {
                    core::ptr::copy_nonoverlapping(t.as_ptr(), mp1 as *mut u8, t.len());
                    *((mp1 as usize + t.len()) as *mut u8) = 0;
                }
            }
            t.len() as u32
        }
        MLM_QUERYUNDO => data(h).and_then(|d| d.m.undo_op()).map_or(0, |op| {
            let op = if op >= OP_TYPING && op < OP_TYPING + 4 { WM_CHAR } else { op };
            mp(1, op)
        }),
        MLM_UNDO => {
            let Some(d) = data(h) else { return 0 };
            if d.m.undo() {
                changed(h);
                1
            } else {
                0
            }
        }
        MLM_RESETUNDO => {
            if let Some(d) = data(h) {
                d.m.reset_undo();
            }
            1
        }
        MLM_QUERYFONT => {
            let Some(d) = data(h) else { return 0 };
            if mp1 != 0 {
                unsafe { core::ptr::copy_nonoverlapping(d.fattrs.as_ptr(), mp1 as *mut u8, 56) };
            }
            1
        }
        MLM_SETFONT => {
            if mp1 == 0 {
                return 0;
            }
            let mut fa = [0u8; 56];
            unsafe { core::ptr::copy_nonoverlapping(mp1 as *const u8, fa.as_mut_ptr(), 56) };
            let (cw, ch, bold, underline) = font_from(&fa);
            let Some(d) = data(h) else { return 0 };
            (d.fattrs, d.cw, d.ch, d.bold, d.underline) = (fa, cw, ch, bold, underline);
            layout(h);
            reveal(h);
            sync_scroll(h);
            ctl::invalidate(h);
            1
        }
        MLM_SETTEXTCOLOR => {
            let Some(d) = data(h) else { return 0 };
            let old = d.fg_val;
            d.fg_val = mp1 as i32;
            ctl::invalidate(h);
            old as u32
        }
        MLM_QUERYTEXTCOLOR => data(h).map_or(0, |d| d.fg_val as u32),
        MLM_SETBACKCOLOR => {
            let Some(d) = data(h) else { return 0 };
            let old = d.bg_val;
            d.bg_val = mp1 as i32;
            ctl::invalidate(h);
            old as u32
        }
        MLM_QUERYBACKCOLOR => data(h).map_or(0, |d| d.bg_val as u32),
        MLM_QUERYFIRSTCHAR => data(h).map_or(0, |d| d.m.line_start(d.m.first_line) as u32),
        MLM_SETFIRSTCHAR => {
            let Some(d) = data(h) else { return 0 };
            d.m.first_line = d.m.line_of(mp1 as usize);
            sync_scroll(h);
            refresh(h);
            1
        }
        MLM_CUT => {
            if copy(h) {
                clear(h);
                1
            } else {
                0
            }
        }
        MLM_COPY => copy(h) as u32,
        MLM_PASTE => paste(h) as u32,
        MLM_CLEAR => clear(h) as u32,
        MLM_DISABLEREFRESH => {
            if let Some(d) = data(h) {
                d.no_refresh += 1;
            }
            1
        }
        MLM_ENABLEREFRESH => {
            if let Some(d) = data(h) {
                d.no_refresh = d.no_refresh.saturating_sub(1);
            }
            sync_scroll(h);
            ctl::invalidate(h);
            1
        }
        MLM_SEARCH => search(h, mp1, mp2),
        _ => WinDefWindowProc(h, msg, mp1, mp2),
    }
}
