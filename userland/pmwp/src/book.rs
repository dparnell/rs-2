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
const ID_EXE: u16 = 511;
const ID_PARAMS: u16 = 512;
const ID_DIR: u16 = 513;

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
fn insert(o: usize, nb: u32, page: u32, name: &'static [u8]) -> u32 {
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

fn label(text: &[u8], (x, y, cx): (i16, i16, i16)) -> Item {
    item(WC_STATIC, text, WS_VISIBLE | 0x0001, (x, y, cx, 8), 0xffff)
}

fn page_frame() -> Item {
    item(WC_FRAME, b"", 0, (0, 0, 236, 120), 300)
}

fn make_page(nb: u32, proc_: extern "C" fn(u32, u32, u32, u32) -> u32, tpl: &[u8], o: usize) -> u32 {
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
    let tpl = template(page_frame(), alloc::vec![label(b"Title (^ starts a new line):", (6, 104, 220)), entry(b"", (8, 92, 216), ID_TITLE)]);
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
