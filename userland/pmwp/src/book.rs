//! Settings notebooks: wpAddSettingsPages puts an object's pages in the
//! notebook, each through wpInsertSettingsPage (a PAGEINFO: a dialog from
//! a module, or a window, with its tab text). WPObject's General page
//! (the title) and WPProgram's Program page (path, parameters, working
//! directory) are rs2's own dialogs; classes add theirs from their DLLs.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::{abs, som, wps};

unsafe extern "C" {
    #[link_name = "PMWIN.924"]
    fn WinLoadDlg(parent: u32, owner: u32, proc_: u32, hmod: u32, id: u32, params: u32) -> u32;
}

const BKA_LAST: u16 = 0x0002;
const BKA_FIRST: u16 = 0x0004;
const BKA_MAJOR: u16 = 0x0040;
const BKA_AUTOPAGESIZE: u16 = 0x0100;
const BKA_STATUSTEXTON: u16 = 0x0001;

const ID_TITLE: u16 = 501;
const ID_TEMPLATE: u16 = 509;
const ID_EXE: u16 = 511;
const ID_PARAMS: u16 = 512;
const ID_DIR: u16 = 513;
const ID_ICON: u16 = 521;
const ID_DETAILS: u16 = 522;
const ID_TREE: u16 = 523;
const ID_COLOR: u16 = 531;
const ID_DEFAULTCOLOR: u16 = 532;
const ID_PRESET: u16 = 540;

const PRESETS: [(&[u8], u32); 6] = [
    (b"~Teal", 0x2e6e8e),
    (b"~Blue", 0x3a6ea5),
    (b"~Green", 0x2f6f4f),
    (b"G~rey", 0xc0c0c0),
    (b"~White", 0xffffff),
    (b"~Sand", 0xd8ccb0),
];

/// wpInsertSettingsPage(hwndNotebook, PAGEINFO): the page's id.
extern "C" fn wpInsertSettingsPage(_o: usize, nb: u32, pi: *const u8) -> u32 {
    if pi.is_null() {
        return 0;
    }
    let p = pi as u32;
    let r16 = |off: usize| (rd(p, off & !3) >> ((off & 2) * 8)) & 0xffff;
    let mut page = rd(p, 4);
    if page == 0 {
        let (pfnwp, hmod, params, dlgid) = (rd(p, 8), rd(p, 12), rd(p, 16), r16(20));
        if dlgid != 0 {
            page = unsafe { WinLoadDlg(nb, nb, pfnwp, hmod, dlgid, params) };
        }
    }
    let style = match r16(22) as u16 {
        0 => BKA_MAJOR | BKA_STATUSTEXTON,
        s => s,
    } | BKA_AUTOPAGESIZE;
    let order = match r16(24) as u16 {
        0 => BKA_FIRST,
        o => o,
    };
    let id = send(nb, BKM_INSERTPAGE, rd(p, 48), mp(style as u32, order as u32));
    if id == 0 {
        return 0;
    }
    if page != 0 {
        send(nb, BKM_SETPAGEWINDOWHWND, id, page);
    }
    let name = rd(p, 28);
    if name != 0 && style & BKA_MAJOR != 0 {
        send(nb, BKM_SETTABTEXT, id, name);
    }
    if name != 0 {
        // The status line without the mnemonic's tilde (kept while the
        // notebook has the page).
        let t: Vec<u8> = cstr(name as *const u8).into_iter().filter(|&c| c != b'~').chain(core::iter::once(0)).collect();
        let t = alloc::boxed::Box::leak(t.into_boxed_slice());
        send(nb, BKM_SETSTATUSLINETEXT, id, t.as_ptr() as u32);
    }
    id
}

/// A PAGEINFO for one of rs2's own pages (a window already made).
pub fn insert(o: usize, nb: u32, page: u32, name: &'static [u8]) -> u32 {
    let mut pi = [0u32; 13];
    pi[0] = 52;
    pi[1] = page;
    // dlgid 0, usPageStyleFlags (offset 22), usPageInsertFlags 0 (first).
    pi[5] = ((BKA_MAJOR | BKA_STATUSTEXTON) as u32) << 16;
    pi[7] = name.as_ptr() as u32;
    obj::call(m::WPObject::wpInsertSettingsPage, &[o, nb as usize, pi.as_ptr() as usize]) as u32
}

fn entry(text: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    // ES_AUTOSCROLL | ES_MARGIN
    item(WC_ENTRYFIELD, text, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (x, y, cx, 8), id)
}

pub fn label(text: &[u8], (x, y, cx): (i16, i16, i16)) -> Item {
    item(WC_STATIC, text, WS_VISIBLE | 0x0001, (x, y, cx, 8), 0xffff)
}

pub fn page_frame() -> Item {
    item(WC_FRAME, b"", 0, (0, 0, 236, 120), 300)
}

pub fn make_page(nb: u32, proc_: extern "C" fn(u32, u32, u32, u32) -> u32, tpl: &[u8], o: usize) -> u32 {
    unsafe { WinCreateDlg(nb, nb, proc_ as *const () as u32, tpl.as_ptr(), o as u32) }
}

// ---- General (the title) ----

extern "C" fn general_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            unsafe { WinSendDlgItemMsg(h, ID_TITLE as u32, EM_SETTEXTLIMIT, 259, 0) };
            let t: Vec<u8> = obj::title(b as usize).into_iter().map(|c| if c == b'\n' { b'^' } else { c }).collect();
            set_item_text(h, ID_TITLE as u32, &t);
            let tpl = obj::call(m::WPObject::wpQueryStyle, &[b as usize]) as u32 & obj::OBJSTYLE_TEMPLATE != 0;
            unsafe { WinSendDlgItemMsg(h, ID_TEMPLATE as u32, BM_SETCHECK, tpl as u32, 0) };
            0
        }
        WM_CONTROL if a & 0xffff == ID_TEMPLATE as u32 && (a >> 16) == 1 => {
            // BN_CLICKED: the Template check box.
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            if o != 0 {
                let on = unsafe { WinSendDlgItemMsg(h, ID_TEMPLATE as u32, BM_QUERYCHECK, 0, 0) } != 0;
                let s = obj::call(m::WPObject::wpQueryStyle, &[o]) as u32;
                let s = if on { s | obj::OBJSTYLE_TEMPLATE } else { s & !obj::OBJSTYLE_TEMPLATE };
                obj::call(m::WPObject::wpSetStyle, &[o, s as usize]);
            }
            0
        }
        WM_DESTROY => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let t: Vec<u8> = item_text(h, ID_TITLE as u32).into_iter().map(|c| if c == b'^' { b'\n' } else { c }).collect();
            if o != 0 && !t.is_empty() && t != obj::title(o) {
                let zt = z(&t);
                obj::call(m::WPObject::wpSetTitle, &[o, zt.as_ptr() as usize]);
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddObjectGeneralPage(o: usize, nb: u32) -> u32 {
    // BS_AUTOCHECKBOX
    let tpl = template(page_frame(), alloc::vec![
        label(b"Title (^ starts a new line):", (6, 104, 220)),
        entry(b"", (8, 92, 216), ID_TITLE),
        item(WC_BUTTON, b"~Template (opening or dragging it makes a new object)", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (8, 70, 220, 10), ID_TEMPLATE),
    ]);
    let page = make_page(nb, general_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    insert(o, nb, page, b"~General\0")
}

extern "C" fn wpAddSettingsPages(o: usize, nb: u32) -> u32 {
    obj::call(m::WPObject::wpAddObjectGeneralPage, &[o, nb as usize]);
    1
}

// ---- Program (WPProgram) ----

extern "C" fn program_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let mut buf = alloc::vec![0u8; 2048];
            let mut cb = buf.len() as u32;
            if wps::call("WPProgram", m::WPProgram::wpQueryProgDetails, &[b as usize, buf.as_mut_ptr() as usize, &mut cb as *mut u32 as usize]) != 0 {
                let w = buf.as_ptr() as u32;
                for (id, i) in [(ID_EXE, 4), (ID_PARAMS, 5), (ID_DIR, 6)] {
                    unsafe { WinSendDlgItemMsg(h, id as u32, EM_SETTEXTLIMIT, 259, 0) };
                    set_item_text(h, id as u32, &cstr(rd(w, i * 4) as *const u8));
                }
            }
            0
        }
        WM_DESTROY => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            if o != 0 {
                let (e, p, d) = (z(&item_text(h, ID_EXE as u32)), z(&item_text(h, ID_PARAMS as u32)), z(&item_text(h, ID_DIR as u32)));
                // PROGDETAILS with the three strings (the rest unchanged).
                let mut pd = [0u32; 18];
                let mut buf = alloc::vec![0u8; 2048];
                let mut cb = buf.len() as u32;
                if wps::call("WPProgram", m::WPProgram::wpQueryProgDetails, &[o, buf.as_mut_ptr() as usize, &mut cb as *mut u32 as usize]) != 0 {
                    pd[1] = rd(buf.as_ptr() as u32, 4);
                }
                pd[0] = 72;
                pd[4] = e.as_ptr() as u32;
                pd[5] = p.as_ptr() as u32;
                pd[6] = d.as_ptr() as u32;
                wps::call("WPProgram", m::WPProgram::wpSetProgDetails, &[o, pd.as_ptr() as usize]);
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddProgramPage(o: usize, nb: u32) -> u32 {
    let tpl = template(page_frame(), alloc::vec![
        label(b"Path and file name:", (6, 104, 200)),
        entry(b"", (8, 92, 216), ID_EXE),
        label(b"Parameters:", (6, 74, 200)),
        entry(b"", (8, 62, 216), ID_PARAMS),
        label(b"Working directory:", (6, 44, 200)),
        entry(b"", (8, 32, 216), ID_DIR),
    ]);
    let page = make_page(nb, program_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    insert(o, nb, page, b"~Program\0")
}

extern "C" fn program_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPProgram"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPProgram", m::WPProgram::wpAddProgramPage, &[o, nb as usize]);
    1
}

// ---- Folders: View and Background ----

extern "C" fn view_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let v = obj::call(m::WPObject::wpQueryDefaultView, &[b as usize]) as u32;
            let id = match v {
                obj::OPEN_DETAILS => ID_DETAILS,
                obj::OPEN_TREE => ID_TREE,
                _ => ID_ICON,
            };
            unsafe { WinSendDlgItemMsg(h, id as u32, BM_SETCHECK, 1, 0) };
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            // BN_CLICKED on a radio button: the default view.
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let v = match (a & 0xffff) as u16 {
                ID_DETAILS => obj::OPEN_DETAILS,
                ID_TREE => obj::OPEN_TREE,
                _ => obj::OPEN_CONTENTS,
            };
            if o != 0 {
                obj::call(m::WPObject::wpSetDefaultView, &[o, v as usize]);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderView1Page(o: usize, nb: u32) -> u32 {
    // BS_AUTORADIOBUTTON
    let radio = |t: &[u8], y: i16, id: u16, first: bool| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (10, y, 200, 10), id);
    let tpl = template(page_frame(), alloc::vec![
        label(b"Open the folder in:", (6, 104, 200)),
        radio(b"~Icon view", 88, ID_ICON, true),
        radio(b"~Details view", 74, ID_DETAILS, false),
        radio(b"~Tree view", 60, ID_TREE, false),
    ]);
    let page = make_page(nb, view_page_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    insert(o, nb, page, b"~View\0")
}

fn show_color(h: u32, rgb: u32) {
    set_item_text(h, ID_COLOR as u32, alloc::format!("{:06X}", rgb).as_bytes());
}

extern "C" fn background_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let o = b as usize;
            let cur = crate::fs::folder_background(o);
            let desktop = wps::is(o, "WPDesktop");
            show_color(h, cur.unwrap_or(if desktop { crate::view::DESKTOP_BG } else { 0xffffff }));
            unsafe { WinSendDlgItemMsg(h, ID_COLOR as u32, EM_SETTEXTLIMIT, 6, 0) };
            0
        }
        WM_COMMAND => {
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            let id = (a & 0xffff) as u16;
            if (ID_PRESET..ID_PRESET + PRESETS.len() as u16).contains(&id) && o != 0 {
                let c = PRESETS[(id - ID_PRESET) as usize].1;
                show_color(h, c);
                crate::fs::set_folder_background(o, Some(c));
            } else if id == ID_DEFAULTCOLOR && o != 0 {
                crate::fs::set_folder_background(o, None);
                let desktop = wps::is(o, "WPDesktop");
                show_color(h, if desktop { crate::view::DESKTOP_BG } else { 0xffffff });
            }
            0
        }
        WM_DESTROY => {
            // A colour typed in (hex RGB).
            let o = unsafe { WinQueryWindowULong(h, QWL_USER) } as usize;
            if let Some(c) = crate::util::parse_hex(&item_text(h, ID_COLOR as u32)) {
                if o != 0 && crate::fs::folder_background(o) != Some(c) {
                    let desktop = wps::is(o, "WPDesktop");
                    let default = if desktop { crate::view::DESKTOP_BG } else { 0xffffff };
                    if crate::fs::folder_background(o).is_some() || c != default {
                        crate::fs::set_folder_background(o, Some(c));
                    }
                }
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderBackgroundPage(o: usize, nb: u32) -> u32 {
    let mut items = alloc::vec![
        label(b"Colour (red, green, blue in hex):", (6, 104, 200)),
        entry(b"", (8, 92, 60), ID_COLOR),
        item(WC_BUTTON, b"~Default", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (80, 89, 50, 13), ID_DEFAULTCOLOR),
        label(b"Or one of these:", (6, 72, 200)),
    ];
    for (i, (name, _)) in PRESETS.iter().enumerate() {
        let (col, row) = (i as i16 % 3, i as i16 / 3);
        items.push(item(WC_BUTTON, name, WS_VISIBLE | WS_TABSTOP | if i == 0 { WS_GROUP } else { 0 }, (8 + col * 56, 54 - row * 18, 50, 13), ID_PRESET + i as u16));
    }
    let tpl = template(page_frame(), items);
    let page = make_page(nb, background_page_proc, &tpl, o);
    if page == 0 {
        return 0;
    }
    insert(o, nb, page, b"~Background\0")
}

extern "C" fn folder_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPFolder"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPFolder", m::WPFolder::wpAddFolderBackgroundPage, &[o, nb as usize]);
    wps::call("WPFolder", m::WPFolder::wpAddFolderIncludePage, &[o, nb as usize]);
    wps::call("WPFolder", m::WPFolder::wpAddFolderSortPage, &[o, nb as usize]);
    wps::call("WPFolder", m::WPFolder::wpAddFolderView1Page, &[o, nb as usize]);
    1
}

methods! {
    __m_wpAddFolderView1Page => wpAddFolderView1Page, 2;
    __m_wpAddFolderBackgroundPage => wpAddFolderBackgroundPage, 2;
    __m_folder_add_pages => folder_add_pages, 2;
}

pub fn folder_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
        ("wpAddFolderView1Page", __m_wpAddFolderView1Page as usize),
        ("wpAddFolderBackgroundPage", __m_wpAddFolderBackgroundPage as usize),
    ]
}

pub fn folder_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddSettingsPages", __m_folder_add_pages as usize)]
}

methods! {
    __m_wpInsertSettingsPage => wpInsertSettingsPage, 3;
    __m_wpAddObjectGeneralPage => wpAddObjectGeneralPage, 2;
    __m_wpAddSettingsPages => wpAddSettingsPages, 2;
    __m_wpAddProgramPage => wpAddProgramPage, 2;
    __m_program_add_pages => program_add_pages, 2;
}

pub fn object_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
        ("wpInsertSettingsPage", __m_wpInsertSettingsPage as usize),
        ("wpAddObjectGeneralPage", __m_wpAddObjectGeneralPage as usize),
        ("wpAddSettingsPages", __m_wpAddSettingsPages as usize),
    ]
}

pub fn program_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddProgramPage", __m_wpAddProgramPage as usize)]
}

pub fn program_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddSettingsPages", __m_program_add_pages as usize)]
}

pub fn _unused() -> u16 {
    let _ = abs::fill_prog_details;
    BKA_LAST
}
