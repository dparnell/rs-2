//! The shell's dialogs: the settings notebook of program objects (also
//! for creating one), file properties and System Setup (the desktop's
//! colour, where minimized windows go).

use alloc::vec::Vec;

use crate::objects::{self, Program};
use crate::pm::*;
use crate::sh;
use crate::view;

unsafe extern "C" {
    #[link_name = "PMCTLS.4"]
    fn WinFileDlg(parent: u32, owner: u32, fild: *mut u8) -> u32;
}

pub fn register(_hab: u32) {}

const ID_NOTEBOOK: u16 = 50;
const ID_PATH: u16 = 201;
const ID_PARAMS: u16 = 202;
const ID_DIR: u16 = 203;
const ID_FIND: u16 = 204;
const ID_DEFAULT: u16 = 211;
const ID_WINDOW: u16 = 212;
const ID_FULL: u16 = 213;
const ID_PM: u16 = 214;
const ID_TITLE: u16 = 221;

/// The notebook being edited: (program handle or 0 for a new one, its
/// group, the page dialogs).
static mut EDIT: (u32, u32, [u32; 3]) = (0, 0, [0; 3]);

fn edit() -> &'static mut (u32, u32, [u32; 3]) {
    unsafe { &mut *core::ptr::addr_of_mut!(EDIT) }
}

fn entry(text: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    // ES_AUTOSCROLL | ES_MARGIN
    item(WC_ENTRYFIELD, text, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (x, y, cx, 8), id)
}

fn label(text: &[u8], (x, y, cx): (i16, i16, i16)) -> Item {
    // SS_TEXT | DT_LEFT
    item(WC_STATIC, text, WS_VISIBLE | 0x0001, (x, y, cx, 8), 0xffff)
}

fn page_frame() -> Item {
    item(WC_FRAME, b"", 0, (0, 0, 236, 120), 300)
}

fn program_page() -> Vec<u8> {
    template(page_frame(), alloc::vec![
        label(b"Path and file name:", (6, 104, 200)),
        entry(b"", (8, 92, 216), ID_PATH),
        item(WC_BUTTON, b"~Find...", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (8, 74, 50, 13), ID_FIND),
        label(b"Parameters:", (6, 58, 200)),
        entry(b"", (8, 46, 216), ID_PARAMS),
        label(b"Working directory:", (6, 30, 200)),
        entry(b"", (8, 18, 216), ID_DIR),
    ])
}

fn session_page() -> Vec<u8> {
    // BS_AUTORADIOBUTTON
    let radio = |t: &[u8], y: i16, id: u16, first: bool| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if first { WS_GROUP } else { 0 } | 0x0004, (10, y, 200, 10), id);
    template(page_frame(), alloc::vec![
        label(b"Run the program:", (6, 104, 200)),
        radio(b"As the ~program says (default)", 88, ID_DEFAULT, true),
        radio(b"In an OS/2 ~window", 74, ID_WINDOW, false),
        radio(b"OS/2 ~full screen", 60, ID_FULL, false),
        radio(b"Presentation ~Manager", 46, ID_PM, false),
    ])
}

fn general_page() -> Vec<u8> {
    template(page_frame(), alloc::vec![label(b"Title:", (6, 104, 200)), entry(b"", (8, 92, 216), ID_TITLE)])
}

extern "C" fn page_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_COMMAND if a & 0xffff == ID_FIND as u32 => {
            find_program(h);
            0
        }
        // The notebook's dialog takes Enter and Esc.
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// The file dialog for the program's path.
fn find_program(page: u32) {
    let mut fd = alloc::vec![0u8; 328];
    let p = fd.as_mut_ptr() as u32;
    wr(p, 0, 328);
    // FDS_OPEN_DIALOG | FDS_CENTER
    wr(p, 4, 0x0100 | 0x0001);
    let title = b"Find Program\0";
    wr(p, 20, title.as_ptr() as u32);
    let cur = item_text(page, ID_PATH as u32);
    let start: Vec<u8> = if cur.is_empty() { objects::sys_path(b"*.EXE") } else { cur };
    let n = start.len().min(259);
    fd[52..52 + n].copy_from_slice(&start[..n]);
    if unsafe { WinFileDlg(HWND_DESKTOP, page, fd.as_mut_ptr()) } != 0 && rd(fd.as_ptr() as u32, 12) == DID_OK {
        let f = until_nul(&fd[52..312]).to_vec();
        set_item_text(page, ID_PATH as u32, &f);
        let dlg_dir = crate::launch::dir_of(&f);
        let pages = edit().2;
        if item_text(pages[0], ID_DIR as u32).is_empty() {
            set_item_text(pages[0], ID_DIR as u32, &dlg_dir);
        }
        if item_text(pages[2], ID_TITLE as u32).is_empty() {
            let name = crate::launch::file_name(&f);
            let t = &name[..name.iter().rposition(|&c| c == b'.').unwrap_or(name.len())];
            set_item_text(pages[2], ID_TITLE as u32, t);
        }
    }
}

extern "C" fn settings_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            let nb = unsafe { WinWindowFromID(h, ID_NOTEBOOK as u32) };
            let tpls = [program_page(), session_page(), general_page()];
            let tabs: [&[u8]; 3] = [b"~Program\0", b"~Session\0", b"~General\0"];
            for (i, t) in tpls.iter().enumerate() {
                let page = unsafe { WinCreateDlg(nb, h, page_proc as *const () as u32, t.as_ptr(), 0) };
                edit().2[i] = page;
                let id = send(nb, BKM_INSERTPAGE, 0, mp(BKA_MAJOR | BKA_AUTOPAGESIZE, BKA_LAST));
                send(nb, BKM_SETTABTEXT, id, tabs[i].as_ptr() as u32);
                send(nb, BKM_SETPAGEWINDOWHWND, id, page);
            }
            let pages = edit().2;
            for (pg, id) in [(0, ID_PATH), (0, ID_PARAMS), (0, ID_DIR), (2, ID_TITLE)] {
                unsafe { WinSendDlgItemMsg(pages[pg], id as u32, EM_SETTEXTLIMIT, 259, 0) };
            }
            let p = if edit().0 != 0 { objects::definition(edit().0).unwrap_or_default() } else { Program::default() };
            set_item_text(pages[0], ID_PATH as u32, &p.exe);
            set_item_text(pages[0], ID_PARAMS as u32, &p.params);
            set_item_text(pages[0], ID_DIR as u32, &p.dir);
            set_item_text(pages[2], ID_TITLE as u32, &p.title);
            let radio = match p.progc {
                PROG_WINDOWABLEVIO => ID_WINDOW,
                PROG_FULLSCREEN => ID_FULL,
                PROG_PM => ID_PM,
                _ => ID_DEFAULT,
            };
            unsafe { WinSendDlgItemMsg(pages[1], radio as u32, BM_SETCHECK, 1, 0) };
            center(h);
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(pages[0], ID_PATH as u32)) };
            1
        }
        WM_COMMAND => {
            match a & 0xffff {
                DID_OK => {
                    if save_settings(h) {
                        unsafe { WinDismissDlg(h, DID_OK) };
                    }
                }
                _ => unsafe {
                    WinDismissDlg(h, DID_CANCEL);
                },
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

fn save_settings(dlg: u32) -> bool {
    let (h, group, pages) = *edit();
    let exe = item_text(pages[0], ID_PATH as u32);
    if exe.is_empty() {
        message(dlg, b"Type the path and file name of the program (or * for a command prompt).", b"Program Settings", MB_OK | MB_WARNING);
        return false;
    }
    let mut p = if h != 0 { objects::definition(h).unwrap_or_default() } else { Program::default() };
    p.exe = exe;
    p.params = item_text(pages[0], ID_PARAMS as u32);
    p.dir = item_text(pages[0], ID_DIR as u32);
    p.title = item_text(pages[2], ID_TITLE as u32);
    if p.title.is_empty() {
        let name = crate::launch::file_name(&p.exe);
        p.title = name[..name.iter().rposition(|&c| c == b'.').unwrap_or(name.len())].to_vec();
    }
    p.progc = [(ID_WINDOW, PROG_WINDOWABLEVIO), (ID_FULL, PROG_FULLSCREEN), (ID_PM, PROG_PM)]
        .into_iter()
        .find(|(id, _)| unsafe { WinSendDlgItemMsg(pages[1], *id as u32, BM_QUERYCHECK, 0, 0) } != 0)
        .map_or(PROG_DEFAULT, |x| x.1);
    let ok = if h != 0 { objects::change_program(h, &p) } else { objects::add_program(group, &p) != 0 };
    if !ok {
        message(dlg, b"The program object could not be saved.", b"Program Settings", MB_OK | MB_ERROR);
        return false;
    }
    true
}

/// The settings notebook of a program object (`h` 0: a new one in
/// `group`).
pub fn program_settings(h: Option<u32>, group: u32) {
    *edit() = (h.unwrap_or(0), group, [0; 3]);
    let title: &[u8] = if h.is_some() { b"Program Settings" } else { b"Create Program Object" };
    // A tabbed notebook over OK and Cancel.
    let tpl = template(dialog_frame(title, 252, 170), alloc::vec![
        item(WC_NOTEBOOK_ID, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | BKS_TABBEDDIALOG, (4, 24, 244, 142), ID_NOTEBOOK),
        item(WC_BUTTON, b"OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 5, 50, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (62, 5, 50, 14), DID_CANCEL as u16),
    ]);
    let owner = sh().views.first().map_or(HWND_DESKTOP, |v| v.frame);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, settings_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    if r == DID_OK {
        view::refresh_group(group);
    }
}

/// A file's properties.
pub fn properties(path: &[u8]) {
    let c = z(path);
    let mut st = os2::FileStatus3::default();
    let rc = unsafe { os2::DosQueryPathInfo(c.as_ptr(), 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<os2::FileStatus3>() as u32) };
    if rc != 0 {
        view::error(&alloc::format!("Cannot read the file's properties (error {rc}).").into_bytes());
        return;
    }
    let (d, t) = (st.fdate_write, st.ftime_write);
    let mut attrs = Vec::new();
    for (bit, name) in [(0x10, "directory"), (0x01, "read-only"), (0x02, "hidden"), (0x04, "system"), (0x20, "archived")] {
        if st.attr & bit != 0 {
            attrs.push(name);
        }
    }
    let text = alloc::format!(
        "Name: {}\nLocation: {}\nSize: {} bytes\nLast written: {:04}-{:02}-{:02} {:02}:{:02}\nAttributes: {}",
        os2::path::from_bytes(crate::launch::file_name(path)),
        os2::path::from_bytes(&crate::launch::dir_of(path)),
        st.cb_file,
        (d >> 9) as u32 + 1980,
        (d >> 5) & 15,
        d & 31,
        t >> 11,
        (t >> 5) & 63,
        if attrs.is_empty() { alloc::string::String::from("none") } else { attrs.join(", ") }
    );
    message(HWND_DESKTOP, &os2::text_bytes(&text), b"Properties", MB_OK | MB_INFORMATION);
}

// ---- System setup ----

const ID_COLOR: u16 = 401;
const ID_PRESET: u16 = 410;
const ID_VIEWER: u16 = 420;

const PRESETS: [(&[u8], u32); 6] = [
    (b"~Teal", 0x2e6e8e),
    (b"~Blue", 0x3a6ea5),
    (b"~Green", 0x2f6f4f),
    (b"G~rey", 0x808080),
    (b"Blac~k", 0x101010),
    (b"~Sand", 0xd8ccb0),
];

extern "C" fn setup_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            let cur = objects::query(objects::APP_DESKTOP, b"Background").unwrap_or(b"2E6E8E".to_vec());
            set_item_text(h, ID_COLOR as u32, &cur);
            let viewer = objects::query(objects::APP_DESKTOP, b"MinimizeTo").is_none_or(|v| v != b"DESKTOP");
            unsafe { WinSendDlgItemMsg(h, ID_VIEWER as u32, BM_SETCHECK, viewer as u32, 0) };
            center(h);
            1
        }
        WM_COMMAND => {
            let id = a & 0xffff;
            if (ID_PRESET as u32..ID_PRESET as u32 + PRESETS.len() as u32).contains(&id) {
                let c = PRESETS[(id - ID_PRESET as u32) as usize].1;
                set_item_text(h, ID_COLOR as u32, &alloc::format!("{c:06X}").into_bytes());
                return 0;
            }
            if id == DID_OK {
                let t = item_text(h, ID_COLOR as u32);
                let Some(c) = objects::parse_hex(&t).filter(|_| t.len() == 6) else {
                    message(h, b"Type the colour as six hex digits RRGGBB (red, green, blue).", b"System Setup", MB_OK | MB_WARNING);
                    return 0;
                };
                objects::write(objects::APP_DESKTOP, b"Background", Some(&alloc::format!("{c:06X}").into_bytes()));
                if let Some(d) = sh().views.first() {
                    view::set_colors(d.cnr, c);
                }
                let viewer = unsafe { WinSendDlgItemMsg(h, ID_VIEWER as u32, BM_QUERYCHECK, 0, 0) } != 0;
                objects::write(objects::APP_DESKTOP, b"MinimizeTo", Some(if viewer { b"VIEWER" } else { b"DESKTOP" }));
                crate::apply_minimize_setting();
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

pub fn system_setup() {
    let mut v = alloc::vec![
        label(b"Desktop background colour (RRGGBB):", (6, 110, 200)),
        entry(b"", (8, 98, 80), ID_COLOR),
    ];
    for (i, (t, _)) in PRESETS.iter().enumerate() {
        let (x, y) = (8 + (i as i16 % 3) * 62, 76 - (i as i16 / 3) * 18);
        v.push(item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | if i == 0 { WS_GROUP } else { 0 }, (x, y, 56, 13), ID_PRESET + i as u16));
    }
    // BS_AUTOCHECKBOX
    v.push(item(WC_BUTTON, b"Minimized windows go to the ~Minimized Window Viewer", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0005, (8, 30, 220, 10), ID_VIEWER));
    v.push(item(WC_BUTTON, b"OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 5, 50, 14), DID_OK as u16));
    v.push(item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (62, 5, 50, 14), DID_CANCEL as u16));
    let tpl = template(dialog_frame(b"System Setup", 236, 126), v);
    let owner = sh().views.first().map_or(HWND_DESKTOP, |v| v.frame);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, setup_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg != 0 {
        unsafe {
            WinProcessDlg(dlg);
            WinDestroyWindow(dlg);
        }
    }
}
