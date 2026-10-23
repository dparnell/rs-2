//! Folders' Sort and Include settings: the folder's views order their
//! objects by a sort attribute (name, type, real name, size or last
//! write date; or as they were placed) and, with "Always maintain sort
//! order" (setup ALWAYSSORT=YES, as INI.RC gives the Drives folder and
//! the Minimized Window Viewer), lay icons out in that order whenever
//! the content changes. The Include page's patterns (wildcards,
//! separated by `;`; `*` is everything) say which objects other than
//! folders a folder shows; a pattern matches an object's title or real
//! name. The settings are the folder's own state (fs.rs); the Sort page,
//! the Include page (wpAddFolderSortPage, wpAddFolderIncludePage) and
//! the view's Sort menu change them.

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::util::eq_nocase;
use crate::wps;

pub const SORT_NONE: u32 = 0;
pub const SORT_NAME: u32 = 1;
pub const SORT_TYPE: u32 = 2;
pub const SORT_REALNAME: u32 = 3;
pub const SORT_SIZE: u32 = 4;
pub const SORT_DATE: u32 = 5;

/// The sort attributes: (value, menu and page text).
pub const ATTRIBUTES: [(u32, &[u8]); 5] = [
    (SORT_NAME, b"~Name"),
    (SORT_TYPE, b"~Type"),
    (SORT_REALNAME, b"~Real name"),
    (SORT_SIZE, b"~Size"),
    (SORT_DATE, b"Last ~write date"),
];

/// The Sort submenu's items: SORT_MENU + the attribute (rs2's ids).
pub const SORT_MENU: u32 = 0x6410;

/// `*` and `?` wildcards, without regard to case.
pub fn wildcard(pat: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while n < name.len() {
        if p < pat.len() && (pat[p] == b'?' || pat[p].eq_ignore_ascii_case(&name[n])) {
            p += 1;
            n += 1;
        } else if p < pat.len() && pat[p] == b'*' {
            star = p;
            mark = n;
            p += 1;
        } else if star != usize::MAX {
            p = star + 1;
            mark += 1;
            n = mark;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == b'*' {
        p += 1;
    }
    p == pat.len()
}

fn real_name(o: usize) -> Vec<u8> {
    wps::path_of(o).map(|p| crate::fileops::name_of(&p).to_vec()).unwrap_or_default()
}

fn title(o: usize) -> Vec<u8> {
    obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect()
}

/// Whether a folder shows an object (its Include patterns).
pub fn included(folder: usize, o: usize) -> bool {
    let pats = crate::fs::include_of(folder);
    if pats.is_empty() || wps::is(o, "WPFolder") {
        return true;
    }
    let (t, r) = (title(o), real_name(o));
    pats.split(|&c| c == b';' || c == b',')
        .map(|p| p.trim_ascii())
        .filter(|p| !p.is_empty())
        .any(|p| wildcard(p, &t) || (!r.is_empty() && wildcard(p, &r)))
}

/// (size, write date and time) of a file system object.
fn file_info(o: usize) -> (u32, u32) {
    let Some(p) = wps::path_of(o) else { return (0, 0) };
    let c = z(&p);
    let mut st = os2::FileStatus3::default();
    if unsafe { os2::DosQueryPathInfo(c.as_ptr(), 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<os2::FileStatus3>() as u32) } != 0 {
        return (0, 0);
    }
    (st.cb_file, (st.fdate_write as u32) << 16 | st.ftime_write as u32)
}

fn upper(v: Vec<u8>) -> Vec<u8> {
    v.to_ascii_uppercase()
}

/// Order objects by a sort attribute (names break ties).
pub fn order(by: u32, objs: &mut [usize]) {
    if by == SORT_NONE {
        return;
    }
    let mut keyed: Vec<(Vec<u8>, u32, Vec<u8>, usize)> = objs
        .iter()
        .map(|&o| {
            let name = upper(title(o));
            match by {
                SORT_TYPE => (upper(som_class_title(o)), 0, name, o),
                SORT_REALNAME => (upper(real_name(o)), 0, name, o),
                SORT_SIZE => (Vec::new(), file_info(o).0, name, o),
                SORT_DATE => (Vec::new(), file_info(o).1, name, o),
                _ => (name.clone(), 0, name, o),
            }
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (i, k) in keyed.into_iter().enumerate() {
        objs[i] = k.3;
    }
}

/// The class's title (the Type sort): folders first, then by class.
fn som_class_title(o: usize) -> Vec<u8> {
    let cls = crate::som::class_of(o);
    let t = wps::call("M_WPObject", m::M_WPObject::wpclsQueryTitle, &[cls]) as *const u8;
    let mut v = if t.is_null() { Vec::new() } else { crate::util::cstr(t).to_vec() };
    if wps::is(o, "WPFolder") {
        v.insert(0, b' ');
    }
    v
}

/// The Sort submenu of a folder's view.
pub fn menu(folder: usize) -> Menu<'static> {
    let (by, _) = crate::fs::sort_of(folder);
    let items = ATTRIBUTES.iter().map(|&(v, t)| Menu::Check((SORT_MENU + v) as u16, t, by == v)).collect();
    Menu::SubId(crate::menu::WPMENUID_SORT as u16, b"S~ort", items)
}

/// A Sort menu item: sort by it now (and from now on).
pub fn menu_selected(folder: usize, id: u32) -> bool {
    let by = id.wrapping_sub(SORT_MENU);
    if !ATTRIBUTES.iter().any(|a| a.0 == by) {
        return false;
    }
    let (_, always) = crate::fs::sort_of(folder);
    crate::fs::set_sort(folder, by, always);
    crate::view::sorted(folder);
    true
}

// ---- The Sort page ----

const ID_NONE: u16 = 440;
const ID_ALWAYS: u16 = 449;

extern "C" fn sort_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let (by, always) = crate::fs::sort_of(b as usize);
            unsafe {
                WinSendDlgItemMsg(h, (ID_NONE as u32) + by, BM_SETCHECK, 1, 0);
                WinSendDlgItemMsg(h, ID_ALWAYS as u32, BM_SETCHECK, always as u32, 0);
            }
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            // BN_CLICKED: a sort attribute or the check box.
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let id = (a & 0xffff) as u16;
            if o == 0 {
                return 0;
            }
            let (mut by, mut always) = crate::fs::sort_of(o);
            if id == ID_ALWAYS {
                always = unsafe { WinSendDlgItemMsg(h, ID_ALWAYS as u32, BM_QUERYCHECK, 0, 0) } != 0;
            } else if (ID_NONE..ID_NONE + 6).contains(&id) {
                by = (id - ID_NONE) as u32;
            } else {
                return 0;
            }
            crate::fs::set_sort(o, by, always);
            crate::view::sorted(o);
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderSortPage(o: usize, nb: u32) -> u32 {
    // BS_AUTORADIOBUTTON, BS_AUTOCHECKBOX
    let radio = |t: &[u8], y: i16, id: u16, first: bool| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (10, y, 200, 10), id);
    let mut v = alloc::vec![book::label(b"Sort the folder's objects by:", (6, 108, 220)), radio(b"~Where they were placed", 94, ID_NONE, true)];
    for (i, &(val, t)) in ATTRIBUTES.iter().enumerate() {
        v.push(radio(t, 82 - 12 * i as i16, ID_NONE + val as u16, false));
    }
    v.push(item(WC_BUTTON, b"~Always maintain sort order", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (10, 12, 210, 10), ID_ALWAYS));
    let tpl = template(book::page_frame(), v);
    let page = book::make_page(nb, sort_page_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, b"~Sort\0")
}

// ---- The Include page ----

const ID_PATTERNS: u16 = 450;

extern "C" fn include_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            unsafe { WinSendDlgItemMsg(h, ID_PATTERNS as u32, EM_SETTEXTLIMIT, 255, 0) };
            let p = crate::fs::include_of(b as usize);
            set_item_text(h, ID_PATTERNS as u32, if p.is_empty() { b"*" } else { &p });
            0
        }
        WM_DESTROY => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let mut t = item_text(h, ID_PATTERNS as u32);
            if t.trim_ascii() == b"*" {
                t.clear();
            }
            if o != 0 && !eq_nocase(&t, &crate::fs::include_of(o)) {
                crate::fs::set_include(o, &t);
                crate::view::refresh(o);
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderIncludePage(o: usize, nb: u32) -> u32 {
    let tpl = template(book::page_frame(), alloc::vec![
        book::label(b"Include the objects whose names match:", (6, 104, 220)),
        // ES_AUTOSCROLL | ES_MARGIN
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 92, 216, 8), ID_PATTERNS),
        book::label(b"Wildcards * and ?; separate patterns with ;", (6, 78, 220)),
        book::label(b"(Folders are always included.)", (6, 66, 220)),
    ]);
    let page = book::make_page(nb, include_page_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert(o, nb, page, b"~Include\0")
}

methods! {
    __m_wpAddFolderSortPage => wpAddFolderSortPage, 2;
    __m_wpAddFolderIncludePage => wpAddFolderIncludePage, 2;
}

pub fn folder_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddFolderSortPage", __m_wpAddFolderSortPage as usize), ("wpAddFolderIncludePage", __m_wpAddFolderIncludePage as usize)]
}
