//! A folder's view settings: the three pages of its View tab, as IBM's
//! (wpAddFolderView1Page, wpAddFolderView2Page, wpAddFolderView3Page),
//! and what they make of its views.
//!
//! - Icon view: the view it opens in, the format (icons where they were
//!   placed, flowed into columns, or one column) and the icons (normal,
//!   small, or none: names only);
//! - Tree view: lines, and the icons (small, normal, none);
//! - Details view: which columns it shows (Icon, Title, Object class,
//!   Real name, Size, Last write date, Last write time).
//!
//! The settings are one number in the folder's state (fs.rs `view_flags`):
//! bits 0-1 the icon format, 2-3 the icon size, 4 no tree lines, 5-6 the
//! tree icons, 8-15 the hidden details columns.

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_CONTENTS, OPEN_DETAILS, OPEN_TREE};
use crate::pm::*;


/// The details columns: (title, shown by default).
pub const COLUMNS: [&[u8]; 7] = [b"Icon\0", b"Title\0", b"Object class\0", b"Real name\0", b"Size\0", b"Last write date\0", b"Last write time\0"];

pub fn icon_format(f: u32) -> u32 {
    f & 3
}
pub fn icon_size(f: u32) -> u32 {
    (f >> 2) & 3
}
pub fn tree_lines(f: u32) -> bool {
    f & 0x10 == 0
}
pub fn tree_icons(f: u32) -> u32 {
    (f >> 5) & 3
}
pub fn hidden_columns(f: u32) -> u32 {
    (f >> 8) & 0xff
}

/// The container's window attributes for a view of a folder.
pub fn attr(folder: usize, view: u32, desktop: bool) -> u32 {
    let f = crate::fs::view_flags(folder);
    match view {
        OPEN_DETAILS => CV_DETAIL | CV_MINI | CA_DETAILSVIEWTITLES,
        OPEN_TREE => {
            let icons = match tree_icons(f) {
                1 => CV_ICON,
                2 => CV_TEXT,
                _ => CV_ICON | CV_MINI,
            };
            CV_TREE | icons | if tree_lines(f) { CA_TREELINE } else { 0 }
        }
        _ if desktop => CV_ICON,
        _ => {
            let size = icon_size(f);
            let base = match (icon_format(f), size) {
                (_, 2) => CV_TEXT,
                (0, _) => CV_ICON,
                _ => CV_NAME,
            };
            let flow = if icon_format(f) == 1 { CV_FLOW } else { 0 };
            base | flow | if size == 1 { CV_MINI } else { 0 }
        }
    }
}

fn set_flags(o: usize, keep: u32, set: u32) {
    let f = crate::fs::view_flags(o);
    crate::fs::set_view_flags(o, (f & !keep) | set);
    crate::view::restyle(o);
}

fn check(h: u32, id: u16, on: bool) {
    unsafe { WinSendDlgItemMsg(h, id as u32, BM_SETCHECK, on as u32, 0) };
}

fn checked(h: u32, id: u16) -> bool {
    unsafe { WinSendDlgItemMsg(h, id as u32, BM_QUERYCHECK, 0, 0) != 0 }
}

// BS_AUTORADIOBUTTON, BS_AUTOCHECKBOX
fn radio(t: &[u8], (x, y, cx): (i16, i16, i16), id: u16, first: bool) -> Item {
    item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (x, y, cx, 10), id)
}
fn checkbox(t: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (x, y, cx, 10), id)
}

fn owner(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

// ---- Page 1: icon view ----

const ID_OPEN: u16 = 521; // .. 523: icon, details, tree
const ID_FORMAT: u16 = 530; // .. 532
const ID_SIZE: u16 = 533; // .. 535

extern "C" fn view1_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let o = b as usize;
            let v = obj::call(m::WPObject::wpQueryDefaultView, &[o]) as u32;
            check(h, match v { OPEN_DETAILS => ID_OPEN + 1, OPEN_TREE => ID_OPEN + 2, _ => ID_OPEN }, true);
            let f = crate::fs::view_flags(o);
            check(h, ID_FORMAT + icon_format(f).min(2) as u16, true);
            check(h, ID_SIZE + icon_size(f).min(2) as u16, true);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = owner(h);
            let id = (a & 0xffff) as u16;
            if o == 0 {
                return 0;
            }
            if (ID_OPEN..ID_OPEN + 3).contains(&id) {
                let v = [OPEN_CONTENTS, OPEN_DETAILS, OPEN_TREE][(id - ID_OPEN) as usize];
                obj::call(m::WPObject::wpSetDefaultView, &[o, v as usize]);
            } else if (ID_FORMAT..ID_FORMAT + 3).contains(&id) {
                set_flags(o, 3, (id - ID_FORMAT) as u32);
            } else if (ID_SIZE..ID_SIZE + 3).contains(&id) {
                set_flags(o, 0xc, ((id - ID_SIZE) as u32) << 2);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderView1Page(o: usize, nb: u32) -> u32 {
    let tpl = template(book::page_frame(), alloc::vec![
        book::label(b"Open the folder in:", (6, 106, 110)),
        radio(b"~Icon view", (10, 94, 100), ID_OPEN, true),
        radio(b"~Details view", (10, 82, 100), ID_OPEN + 1, false),
        radio(b"~Tree view", (10, 70, 100), ID_OPEN + 2, false),
        book::label(b"Icon view format:", (120, 106, 110)),
        radio(b"~As placed", (124, 94, 100), ID_FORMAT, true),
        radio(b"~Flowed", (124, 82, 100), ID_FORMAT + 1, false),
        radio(b"~Non-flowed", (124, 70, 100), ID_FORMAT + 2, false),
        book::label(b"Icons:", (6, 50, 110)),
        radio(b"~Normal size", (10, 38, 70), ID_SIZE, true),
        radio(b"~Small size", (84, 38, 70), ID_SIZE + 1, false),
        radio(b"In~visible", (158, 38, 70), ID_SIZE + 2, false),
    ]);
    let page = book::make_page(nb, view1_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, b"~View\0")
}

// ---- Page 2: tree view ----

const ID_LINES: u16 = 540;
const ID_TICONS: u16 = 541; // .. 543: small, normal, invisible

extern "C" fn view2_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let f = crate::fs::view_flags(b as usize);
            check(h, ID_LINES, tree_lines(f));
            check(h, ID_TICONS + tree_icons(f).min(2) as u16, true);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = owner(h);
            let id = (a & 0xffff) as u16;
            if o == 0 {
                return 0;
            }
            if id == ID_LINES {
                set_flags(o, 0x10, if checked(h, ID_LINES) { 0 } else { 0x10 });
            } else if (ID_TICONS..ID_TICONS + 3).contains(&id) {
                set_flags(o, 0x60, ((id - ID_TICONS) as u32) << 5);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderView2Page(o: usize, nb: u32) -> u32 {
    let tpl = template(book::page_frame(), alloc::vec![
        book::label(b"Tree view:", (6, 106, 200)),
        checkbox(b"Show ~lines", (10, 92, 200), ID_LINES),
        book::label(b"Icons:", (6, 72, 110)),
        radio(b"~Small size", (10, 60, 70), ID_TICONS, true),
        radio(b"~Normal size", (84, 60, 70), ID_TICONS + 1, false),
        radio(b"In~visible", (158, 60, 70), ID_TICONS + 2, false),
    ]);
    let page = book::make_page(nb, view2_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert_more(o, nb, page, b"View - Tree view\0")
}

// ---- Page 3: details view ----

const ID_COLUMN: u16 = 550; // .. 556

extern "C" fn view3_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let hidden = hidden_columns(crate::fs::view_flags(b as usize));
            for i in 0..COLUMNS.len() {
                check(h, ID_COLUMN + i as u16, hidden & (1 << i) == 0);
            }
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let o = owner(h);
            let id = (a & 0xffff) as u16;
            if o != 0 && (ID_COLUMN..ID_COLUMN + COLUMNS.len() as u16).contains(&id) {
                let mut hidden = 0u32;
                for i in 0..COLUMNS.len() {
                    if !checked(h, ID_COLUMN + i as u16) {
                        hidden |= 1 << i;
                    }
                }
                set_flags(o, 0xff00, hidden << 8);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderView3Page(o: usize, nb: u32) -> u32 {
    let mut v: Vec<Item> = alloc::vec![book::label(b"Details view shows:", (6, 106, 200))];
    for (i, c) in COLUMNS.iter().enumerate() {
        let (x, y) = (10 + (i as i16 / 4) * 112, 92 - (i as i16 % 4) * 14);
        v.push(checkbox(&c[..c.len() - 1], (x, y, 106), ID_COLUMN + i as u16));
    }
    let tpl = template(book::page_frame(), v);
    let page = book::make_page(nb, view3_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert_more(o, nb, page, b"View - Details view\0")
}

methods! {
    __m_wpAddFolderView1Page => wpAddFolderView1Page, 2;
    __m_wpAddFolderView2Page => wpAddFolderView2Page, 2;
    __m_wpAddFolderView3Page => wpAddFolderView3Page, 2;
}

pub fn folder_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
        ("wpAddFolderView1Page", __m_wpAddFolderView1Page as usize),
        ("wpAddFolderView2Page", __m_wpAddFolderView2Page as usize),
        ("wpAddFolderView3Page", __m_wpAddFolderView3Page as usize),
    ]
}
