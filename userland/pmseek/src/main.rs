//! PMSEEK.EXE: rs2's Seek and Scan Files, a re-implementation of OS/2's
//! PMSEEK.
//!
//! As OS/2's (studied in its binary and its resource DLL PMSEEK.DLL): one
//! window, a dialog with a menu bar, holding the file name to search
//! for, the text to search for (if any), the editor's file name, the
//! drives to search ("All fixed disks" or a check box per drive), a
//! status line, the list of files found and the Search, Stop, Open and
//! Help buttons. Search runs on a second thread, so the window stays
//! responsive and Stop works; each file found is a line with its date,
//! time, size and full name, followed with "Display found text" by the
//! lines holding the text ("-------> ..."). The end of a search reads
//! "Search Complete... N file(s) found.  Search Time= S second(s).".
//! The menus are OS/2's: File (Save, Save as: the list to a file),
//! Selected (Open in the editor, Process: run the program, Command...:
//! a command line for the file), Edit (Copy the selected line, Clear
//! list), Options (Search subdirectories, Display found text, Ignore
//! case, Clear on search, Set defaults) and Help; OS/2's accelerators
//! (F2 Save, F4 Clear list, F5 Search, F6 Stop, Ctrl+Insert Copy) and
//! help panel numbers. A double click opens a file in the editor. The
//! settings are in OS2.INI, application `PMSEEK` (Position, Editor,
//! Saveas, Options), as IBM's keys.
//!
//! `PMSEEK /SELFTEST` checks the pattern and text matching and searches
//! the boot drive; it prints PASS/FAIL lines and "pmseek selftest done:
//! N failure(s)" on the serial port.

#![no_std]
#![no_main]

extern crate alloc;

mod search;
mod selftest;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use pmkit::help::{Help, HelpWindow};
use pmkit::tpl::{self, Item};
use pmkit::*;

os2::entry!(main);

// Menu and control ids (OS/2's).
const IDM_FILE: u16 = 293;
const IDM_SAVE: u16 = 294;
const IDM_SAVEAS: u16 = 295;
const IDM_SELECTED: u16 = 808;
const IDM_OPEN: u16 = 810;
const IDM_PROCESS: u16 = 806;
const IDM_COMMAND: u16 = 807;
const IDM_EDIT: u16 = 805;
const IDM_COPY: u16 = 292;
const IDM_CLEAR: u16 = 296;
const IDM_OPTIONS: u16 = 200;
const IDM_SUBDIRS: u16 = 201;
const IDM_FOUNDTEXT: u16 = 202;
const IDM_IGNORECASE: u16 = 203;
const IDM_CLEARONSEARCH: u16 = 204;
const IDM_SEARCHKEY: u16 = 205;
const IDM_STOPKEY: u16 = 206;
const IDM_DEFAULTS: u16 = 291;
const IDM_HELP: u16 = 502;
const IDM_USINGHELP: u16 = 504;
const IDM_GENERALHELP: u16 = 505;
const IDM_KEYSHELP: u16 = 506;
const IDM_HELPINDEX: u16 = 507;
const IDM_PRODUCT: u16 = 508;

const DLG_MAIN: u16 = 250;
const DLG_COMMAND: u16 = 156;
const DLG_SAVEAS: u16 = 295;
const DLG_PRODUCT: u16 = 804;

const ID_NAME: u32 = 100;
const ID_TEXT: u32 = 101;
const ID_CMDLINE: u32 = 102;
const ID_LIST: u32 = 103;
const ID_EDITOR: u32 = 110;
const ID_STATUS: u32 = 60;
const ID_SEARCH: u32 = 11;
const ID_STOP: u32 = 12;
const ID_HELPBTN: u32 = 13;
const ID_OPENBTN: u32 = 14;
const ID_SAVEBTN: u32 = 111;
const ID_CANCEL: u32 = 18;
const ID_ALLFIXED: u32 = 300;
/// Drive A's check box; Z's is 326.
const ID_DRIVE0: u32 = 301;

const PANEL_GENERAL: u16 = 21247;
const PANEL_KEYS: u16 = 21239;

const WM_SEEK_STATUS: u32 = WM_USER + 1;
const WM_SEEK_FILE: u32 = WM_USER + 2;
const WM_SEEK_DONE: u32 = WM_USER + 3;

const TITLE: &[u8] = b"Seek and Scan Files";
const HELP_FILE: &[u8] = b"PMSEEK.HLP";
const INI_APP: &[u8] = b"PMSEEK";
/// A PM list box holds at most this many lines.
const LIST_MAX: u32 = 32000;

struct App {
    hab: u32,
    dlg: u32,
    menu: u32,
    help: Option<&'static mut Help>,
    /// The full name of each file listed (the list box items' handles
    /// are indexes into it, plus one).
    paths: Vec<Vec<u8>>,
    searching: bool,
    started_ms: u32,
    subdirs: bool,
    found_text: bool,
    ignore_case: bool,
    clear_on_search: bool,
    saveas: Vec<u8>,
    /// The controls' places when the window was made (id, x, y, cx, cy)
    /// and the frame's size then.
    layout: Vec<(u32, Swp)>,
    size0: (i32, i32),
    /// Fixed disks (letters).
    fixed: Vec<u8>,
}

static mut APP: App = App {
    hab: 0,
    dlg: 0,
    menu: 0,
    help: None,
    paths: Vec::new(),
    searching: false,
    started_ms: 0,
    subdirs: true,
    found_text: false,
    ignore_case: true,
    clear_on_search: true,
    saveas: Vec::new(),
    layout: Vec::new(),
    size0: (0, 0),
    fixed: Vec::new(),
};

fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

static STOP: AtomicBool = AtomicBool::new(false);
static mut SPEC: Option<search::Spec> = None;

pub fn trace(s: &[u8]) {
    pmkit::trace("pmseek", s);
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.228"]
    fn DosSearchPath(flags: u32, path: *const u8, name: *const u8, buf: *mut u8, cb: u32) -> u32;
}

fn ms() -> u32 {
    let mut v = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(14, 14, v.as_mut_ptr(), 4) };
    v[0]
}

fn main(s: &os2::Startup) -> u32 {
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if hmq == 0 {
        return 1;
    }
    // No hard-error pop-ups for drives without a disk.
    unsafe { os2::DosError(0) };
    if pmkit::arg(s).eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest::run();
        unsafe {
            WinDestroyMsgQueue(hmq);
            WinTerminate(a.hab);
        }
        return fails;
    }
    load_settings();
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, 0, main_proc as *const () as u32, main_template().as_ptr(), 0) };
    if dlg == 0 {
        trace(b"no window");
        return 1;
    }
    a.dlg = dlg;
    a.menu = set_menu(dlg, &menu_items());
    set_accels(
        a.hab,
        dlg,
        &[
            (AF_VIRTUALKEY, VK_F2, IDM_SAVE),
            (AF_VIRTUALKEY, VK_F4, IDM_CLEAR),
            (AF_VIRTUALKEY, VK_F5, IDM_SEARCHKEY),
            (AF_VIRTUALKEY, 0x25 /* VK_F6 */, IDM_STOPKEY),
            (AF_VIRTUALKEY | AF_CONTROL, VK_INSERT, IDM_COPY),
        ],
    );
    a.help = Help::create(a.hab, dlg, HELP_FILE, b"Help for Seek and Scan Files", &help_windows());
    update_menu();
    show_window();
    message_loop(a.hab);
    STOP.store(true, Ordering::SeqCst);
    save_position();
    unsafe {
        if let Some(h) = a.help.as_mut() {
            h.destroy(dlg);
        }
        WinDestroyWindow(dlg);
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

fn help_windows() -> [HelpWindow<'static>; 4] {
    const MAIN: &[(u16, u16)] = &[
        (IDM_FILE, 21253),
        (IDM_SAVE, 21254),
        (IDM_SAVEAS, 21255),
        (IDM_SELECTED, 34463),
        (IDM_OPEN, 21233),
        (IDM_PROCESS, 21234),
        (IDM_COMMAND, 21235),
        (IDM_EDIT, 21258),
        (IDM_COPY, 21236),
        (IDM_CLEAR, 21251),
        (IDM_HELP, 64022),
        (IDM_HELPINDEX, 64026),
        (IDM_GENERALHELP, 64024),
        (IDM_KEYSHELP, 64025),
        (IDM_USINGHELP, 64023),
        (IDM_PRODUCT, 64096),
        (ID_NAME as u16, 21230),
        (ID_TEXT as u16, 21231),
        (ID_EDITOR as u16, 21238),
        (ID_LIST as u16, 21249),
        (ID_SEARCH as u16, 21250),
        (ID_STOP as u16, 21245),
        (ID_OPENBTN as u16, 21233),
        (ID_HELPBTN as u16, 21252),
        (ID_ALLFIXED as u16, 21260),
        (301, 21261), (302, 21261), (303, 21261), (304, 21261), (305, 21261), (306, 21261), (307, 21261),
        (308, 21261), (309, 21261), (310, 21261), (311, 21261), (312, 21261), (313, 21261), (314, 21261),
        (315, 21261), (316, 21261), (317, 21261), (318, 21261), (319, 21261), (320, 21261), (321, 21261),
        (322, 21261), (323, 21261), (324, 21261), (325, 21261), (326, 21261),
        (IDM_OPTIONS, 21259),
        (IDM_SUBDIRS, 21240),
        (IDM_FOUNDTEXT, 21241),
        (IDM_IGNORECASE, 21242),
        (IDM_CLEARONSEARCH, 21232),
        (IDM_DEFAULTS, 21237),
    ];
    [
        HelpWindow { id: DLG_MAIN, ext: PANEL_GENERAL, items: MAIN },
        HelpWindow { id: DLG_COMMAND, ext: 21243, items: &[(ID_CMDLINE as u16, 21243), (ID_SEARCH as u16, 21244), (ID_CANCEL as u16, 21246), (ID_HELPBTN as u16, 21243)] },
        HelpWindow { id: DLG_SAVEAS, ext: 21256, items: &[(ID_NAME as u16, 21256), (ID_SAVEBTN as u16, 21257), (ID_CANCEL as u16, 21246), (ID_HELPBTN as u16, 21256)] },
        HelpWindow { id: DLG_PRODUCT, ext: 64096, items: &[(1, 64096)] },
    ]
}

fn menu_items() -> Vec<Menu<'static>> {
    alloc::vec![
        Menu::Sub(IDM_FILE, b"~File", alloc::vec![Menu::Item(IDM_SAVE, b"~Save\tF2"), Menu::Item(IDM_SAVEAS, b"Save ~as...")]),
        Menu::Sub(
            IDM_SELECTED,
            b"~Selected",
            alloc::vec![Menu::Disabled(IDM_OPEN, b"~Open"), Menu::Disabled(IDM_PROCESS, b"~Process"), Menu::Disabled(IDM_COMMAND, b"~Command...")]
        ),
        Menu::Sub(IDM_EDIT, b"~Edit", alloc::vec![Menu::Disabled(IDM_COPY, b"~Copy\tCtrl+Insert"), Menu::Item(IDM_CLEAR, b"C~lear list\tF4")]),
        Menu::Sub(
            IDM_OPTIONS,
            b"~Options",
            alloc::vec![
                Menu::Item(IDM_SUBDIRS, b"~Search subdirectories"),
                Menu::Item(IDM_FOUNDTEXT, b"Display ~found text"),
                Menu::Item(IDM_IGNORECASE, b"~Ignore case"),
                Menu::Item(IDM_CLEARONSEARCH, b"~Clear on search"),
                Menu::Sep,
                Menu::Item(IDM_DEFAULTS, b"Set ~defaults"),
            ]
        ),
        Menu::Sub(
            IDM_HELP,
            b"~Help",
            alloc::vec![
                Menu::Item(IDM_HELPINDEX, b"Help ~index"),
                Menu::Item(IDM_GENERALHELP, b"~General help"),
                Menu::Item(IDM_USINGHELP, b"~Using help"),
                Menu::Item(IDM_KEYSHELP, b"~Keys help"),
                Menu::Sep,
                Menu::Item(IDM_PRODUCT, b"~Product information"),
            ]
        ),
    ]
}

/// The window: a dialog with a sizing border, laid out as OS/2's.
fn main_template() -> Vec<u8> {
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let mut frame = tpl::item(WC_FRAME, TITLE, WS_CLIPSIBLINGS | FS_NOBYTEALIGN, (5, 5, 300, 216), DLG_MAIN);
    frame.ctl = fcf.to_le_bytes().to_vec();
    let mut v: Vec<Item> = alloc::vec![
        label(b"File name to search for:", (10, 185, 120, 8), DT_LEFT),
        entry((12, 174, 110, 8), ID_NAME, 0).group(),
        label(b"Text to search for (if any):", (10, 162, 130, 8), DT_LEFT),
        entry((12, 151, 110, 8), ID_TEXT, 0),
        label(b"Editor filespec:", (10, 139, 94, 8), DT_LEFT),
        entry((12, 128, 108, 8), ID_EDITOR, 0),
        label(b"Drives to search:", (147, 180, 100, 8), DT_LEFT),
        check_box(b"All fixed disks", (147, 170, 77, 10), ID_ALLFIXED).group(),
    ];
    for i in 0..26u32 {
        let (row, col) = (i / 7, i % 7);
        let letter = [b'A' + i as u8];
        // Shown for the drives that exist (WM_INITDLG).
        v.push(tpl::item(WC_BUTTON, &letter, BS_AUTOCHECKBOX | BS_NOPOINTERFOCUS, (147 + 20 * col as i16, 160 - 10 * row as i16, 20, 10), (ID_DRIVE0 + i) as u16));
    }
    v.extend([
        text_id(b"", (10, 116, 285, 8), ID_STATUS, DT_LEFT),
        label(b"Files found:", (10, 107, 280, 8), DT_LEFT),
        list_box((10, 24, 285, 82), ID_LIST, LS_HORZSCROLL | LS_NOADJUSTPOS).group(),
        button(b"~Search", (24, 4, 47), ID_SEARCH, BS_DEFAULT).group(),
        button(b"S~top", (93, 4, 47), ID_STOP, WS_DISABLED),
        button(b"~Open", (162, 4, 47), ID_OPENBTN, WS_DISABLED),
        help_button((231, 4, 47), ID_HELPBTN),
    ]);
    template(frame, v)
}

// ---- Settings ----

fn load_settings() {
    let a = app();
    if let Some(o) = ini::read_str(INI_APP, b"Options") {
        let w: Vec<i64> = words(&o).iter().filter_map(|x| parse_num(x)).collect();
        if w.len() >= 4 {
            a.subdirs = w[0] != 0;
            a.found_text = w[1] != 0;
            a.ignore_case = w[2] != 0;
            a.clear_on_search = w[3] != 0;
        }
    }
    a.saveas = ini::read_str(INI_APP, b"Saveas").filter(|s| !s.is_empty()).unwrap_or_else(|| b"PMSEEK.LST".to_vec());
}

/// Options > Set defaults: the options and the editor.
fn save_defaults() {
    let a = app();
    let o = alloc::format!("{} {} {} {}", a.subdirs as u8, a.found_text as u8, a.ignore_case as u8, a.clear_on_search as u8);
    ini::write_str(INI_APP, b"Options", o.as_bytes());
    ini::write_str(INI_APP, b"Editor", &item_text(a.dlg, ID_EDITOR));
    ini::write_str(INI_APP, b"Saveas", &a.saveas);
}

fn show_window() {
    let a = app();
    let (sw, sh) = screen();
    let mut swp = window_pos(a.dlg);
    if let Some(p) = ini::read_str(INI_APP, b"Position") {
        let w: Vec<i64> = words(&p).iter().filter_map(|x| parse_num(x)).collect();
        if w.len() == 4 && w[2] > 100 && w[3] > 100 {
            swp.x = (w[0] as i32).clamp(0, sw - 40);
            swp.y = (w[1] as i32).clamp(0, sh - 40);
            swp.cx = (w[2] as i32).min(sw);
            swp.cy = (w[3] as i32).min(sh);
        }
    } else {
        swp.x = ((sw - swp.cx) / 2).max(0);
        swp.y = ((sh - swp.cy) / 2).max(0);
    }
    unsafe {
        WinSetWindowPos(a.dlg, HWND_TOP, swp.x, swp.y, swp.cx, swp.cy, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, pmkit::item(a.dlg, ID_NAME));
    }
}

fn save_position() {
    let a = app();
    let swp = window_pos(a.dlg);
    if swp.fl & (SWP_MINIMIZE | SWP_MAXIMIZE) != 0 || swp.cx <= 0 {
        return;
    }
    ini::write_str(INI_APP, b"Position", alloc::format!("{} {} {} {}", swp.x, swp.y, swp.cx, swp.cy).as_bytes());
}

fn update_menu() {
    let a = app();
    menu_check(a.menu, IDM_SUBDIRS, a.subdirs);
    menu_check(a.menu, IDM_FOUNDTEXT, a.found_text);
    menu_check(a.menu, IDM_IGNORECASE, a.ignore_case);
    menu_check(a.menu, IDM_CLEARONSEARCH, a.clear_on_search);
}

/// The selected list line and its file.
fn selection() -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let a = app();
    let sel = dlg_msg(a.dlg, ID_LIST, LM_QUERYSELECTION, LIT_FIRST, 0);
    if sel == LIT_NONE || sel == 0xffff_ffff {
        return None;
    }
    let text = lb_text(a.dlg, ID_LIST, sel);
    let h = dlg_msg(a.dlg, ID_LIST, LM_QUERYITEMHANDLE, sel, 0) as usize;
    let path = (h >= 1 && h <= a.paths.len()).then(|| a.paths[h - 1].clone());
    Some((text, path))
}

fn selection_changed() {
    let a = app();
    let s = selection();
    let file = s.as_ref().is_some_and(|s| s.1.is_some());
    for id in [IDM_OPEN, IDM_PROCESS, IDM_COMMAND] {
        menu_enable(a.menu, id, file);
    }
    menu_enable(a.menu, IDM_COPY, s.is_some());
    enable(a.dlg, ID_OPENBTN, file);
}

// ---- Searching ----

fn status(t: &[u8]) {
    set_item_text(app().dlg, ID_STATUS, t);
}

/// The drives the window has, and the fixed ones.
fn init_drives(dlg: u32) {
    let a = app();
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut cur, &mut map) };
    a.fixed.clear();
    for i in 0..26u32 {
        if map & (1 << i) == 0 {
            continue;
        }
        let letter = b'A' + i as u8;
        unsafe { WinShowWindow(pmkit::item(dlg, ID_DRIVE0 + i), 1) };
        if letter >= b'C' && !is_removable(letter) {
            a.fixed.push(letter);
        }
    }
    set_check(dlg, ID_ALLFIXED, true);
    all_fixed(dlg, true);
}

/// A CD-ROM (CDFS) or network drive is not a fixed disk.
fn is_removable(letter: u8) -> bool {
    let dev = [letter, b':', 0];
    let mut buf = [0u8; 256];
    let mut cb = buf.len() as u32;
    // FSAIL_QUERYNAME
    if unsafe { os2::DosQueryFSAttach(dev.as_ptr(), 0, 1, buf.as_mut_ptr(), &mut cb) } != 0 {
        return true;
    }
    // FSQBUFFER2: iType, cbName, cbFSDName, cbFSAData, szName, szFSDName.
    let ty = rd16(&buf, 0);
    let cb_name = rd16(&buf, 2) as usize;
    let fsd = until_nul(&buf[8 + cb_name + 1..]);
    ty == 4 /* FSAT_REMOTEDRV */ || fsd.eq_ignore_ascii_case(b"CDFS") || fsd.eq_ignore_ascii_case(b"UDF")
}

fn all_fixed(dlg: u32, on: bool) {
    for &d in &app().fixed {
        set_check(dlg, ID_DRIVE0 + (d - b'A') as u32, on);
    }
}

fn start_search() {
    let a = app();
    if a.searching {
        return;
    }
    let mut pattern = item_text(a.dlg, ID_NAME);
    while pattern.last() == Some(&b' ') {
        pattern.pop();
    }
    if pattern.is_empty() {
        pattern = b"*.*".to_vec();
        set_item_text(a.dlg, ID_NAME, &pattern);
    }
    let drives: Vec<u8> = (0..26u32).filter(|&i| checked(a.dlg, ID_DRIVE0 + i)).map(|i| b'A' + i as u8).collect();
    let (only, _, _) = search::split_pattern(&pattern);
    if drives.is_empty() && only.is_none() {
        message(a.dlg, b"Unable to access drive.  Perhaps you specified an incorrect drive letter.", TITLE, MB_OK | MB_WARNING);
        return;
    }
    if a.clear_on_search {
        clear_list();
    }
    let spec = search::Spec {
        drives,
        pattern: pattern.clone(),
        text: item_text(a.dlg, ID_TEXT),
        subdirs: a.subdirs,
        ignore_case: a.ignore_case,
        found_text: a.found_text,
    };
    trace(&[&b"search "[..], &pattern, b" text '", &spec.text, b"'"].concat());
    unsafe { SPEC = Some(spec) };
    STOP.store(false, Ordering::SeqCst);
    a.searching = true;
    a.started_ms = ms();
    status(&[&b"Searching for "[..], &pattern, b" ..."].concat());
    enable(a.dlg, ID_SEARCH, false);
    enable(a.dlg, ID_STOP, true);
    let mut tid = 0;
    if unsafe { os2::DosCreateThread(&mut tid, search_thread, a.dlg, 0, 256 * 1024) } != 0 {
        a.searching = false;
        enable(a.dlg, ID_SEARCH, true);
        enable(a.dlg, ID_STOP, false);
    }
}

unsafe extern "C" fn search_thread(dlg: u32) {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let spec = unsafe { (*core::ptr::addr_of!(SPEC)).as_ref().unwrap() };
    let n = search::run(spec, &mut |e| {
        if STOP.load(Ordering::SeqCst) {
            return false;
        }
        match e {
            search::Event::Dir(p) => {
                let t = [&b"Searching "[..], p].concat();
                send(dlg, WM_SEEK_STATUS, t.as_ptr() as u32, t.len() as u32);
            }
            search::Event::File(f) => {
                let b = alloc::boxed::Box::new(f);
                send(dlg, WM_SEEK_FILE, &*b as *const search::Found as u32, 0);
            }
            search::Event::Error(p, _rc) => {
                let t = alloc::format!("Error accessing drive {}:", p[0] as char).into_bytes();
                send(dlg, WM_SEEK_STATUS, t.as_ptr() as u32, t.len() as u32);
            }
        }
        !STOP.load(Ordering::SeqCst)
    });
    post(dlg, WM_SEEK_DONE, n, STOP.load(Ordering::SeqCst) as u32);
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
}

/// A file found (on the window's thread).
fn add_found(f: &search::Found) {
    let a = app();
    let count = dlg_msg(a.dlg, ID_LIST, LM_QUERYITEMCOUNT, 0, 0);
    if count + 1 + f.lines.len() as u32 > LIST_MAX {
        STOP.store(true, Ordering::SeqCst);
        status(b"PM Listbox capacity exceeded. ");
        return;
    }
    a.paths.push(f.path.clone());
    let h = a.paths.len() as u32;
    let i = lb_add(a.dlg, ID_LIST, &search::entry_line(f));
    dlg_msg(a.dlg, ID_LIST, LM_SETITEMHANDLE, i, h);
    for l in &f.lines {
        let i = lb_add(a.dlg, ID_LIST, &[search::FOUND_PREFIX, l].concat());
        dlg_msg(a.dlg, ID_LIST, LM_SETITEMHANDLE, i, h);
    }
}

fn search_done(n: u32, aborted: bool) {
    let a = app();
    a.searching = false;
    let secs = ms().wrapping_sub(a.started_ms).div_ceil(1000);
    let head: &[u8] = if aborted { b"Search Aborted... " } else { b"Search Complete... " };
    let t = [head, alloc::format!("{} file(s) found.  Search Time= {} second(s).", n, secs).as_bytes()].concat();
    status(&t);
    trace(&t);
    enable(a.dlg, ID_SEARCH, true);
    enable(a.dlg, ID_STOP, false);
}

fn clear_list() {
    let a = app();
    dlg_msg(a.dlg, ID_LIST, LM_DELETEALL, 0, 0);
    a.paths.clear();
    selection_changed();
}

// ---- Selected ----

/// A program's full name: found along PATH when it names no directory.
fn find_program(name: &[u8]) -> Vec<u8> {
    if name.contains(&b'\\') || name.contains(&b':') {
        return name.to_vec();
    }
    let n = z(name);
    let mut buf = [0u8; 260];
    // SEARCH_ENVIRONMENT | SEARCH_CUR_DIRECTORY
    if unsafe { DosSearchPath(3, b"PATH\0".as_ptr(), n.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0 {
        return until_nul(&buf).to_vec();
    }
    name.to_vec()
}

/// Start a program in a session of its own; the error code.
fn start_session(program: &[u8], args: &[u8], session_type: u16) -> u32 {
    let (p, i) = (z(program), z(args));
    let sd = os2::StartData {
        length: core::mem::size_of::<os2::StartData>() as u16,
        related: 0,
        fgbg: 0,
        trace: 0,
        title: core::ptr::null(),
        name: p.as_ptr(),
        inputs: if args.is_empty() { core::ptr::null() } else { i.as_ptr() },
        termq: core::ptr::null(),
        env: core::ptr::null(),
        inherit: 0,
        session_type,
    };
    let (mut sid, mut pid) = (0, 0);
    let rc = unsafe { os2::DosStartSession(&sd, &mut sid, &mut pid) };
    trace(&[&b"start "[..], program, b" ", args, alloc::format!(" rc {rc}").as_bytes()].concat());
    rc
}

fn quoted(p: &[u8]) -> Vec<u8> {
    if p.contains(&b' ') { [&b"\""[..], p, b"\""].concat() } else { p.to_vec() }
}

/// Open the selected file in the editor.
fn open_selected() {
    let a = app();
    let Some((_, Some(path))) = selection() else { return };
    let mut editor = item_text(a.dlg, ID_EDITOR);
    if editor.is_empty() {
        editor = b"E.EXE".to_vec();
    }
    let prog = find_program(&editor);
    let rc = start_session(&prog, &quoted(&path), 0);
    if rc != 0 && rc != 457 {
        let t = [alloc::format!("RC= {rc} from DosStartSession. Probable cause: Editor ").as_bytes(), &editor, b" not found."].concat();
        message(a.dlg, &t, TITLE, MB_OK | MB_ERROR);
    }
}

/// Selected > Process: run the program (a command file through CMD.EXE).
fn process_selected() {
    let a = app();
    let Some((_, Some(path))) = selection() else { return };
    let ext = extension(&path);
    let rc = if ext == b"EXE" || ext == b"COM" {
        start_session(&path, b"", 0)
    } else {
        start_session(&find_program(b"CMD.EXE"), &[&b"/C "[..], &quoted(&path)].concat(), 2)
    };
    if rc != 0 && rc != 457 {
        let t = [alloc::format!("RC= {rc} from DosStartSession ").as_bytes(), &path].concat();
        message(a.dlg, &t, TITLE, MB_OK | MB_ERROR);
    }
}

/// Text for the small dialogs' entry field (in and out).
static mut ENTRY_TEXT: Vec<u8> = Vec::new();

extern "C" fn entry_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let text = unsafe { &mut *core::ptr::addr_of_mut!(ENTRY_TEXT) };
    match m {
        WM_INITDLG => {
            let id = if pmkit::item(h, ID_CMDLINE) != 0 { ID_CMDLINE } else { ID_NAME };
            dlg_msg(h, id, EM_SETTEXTLIMIT, 259, 0);
            set_item_text(h, id, text);
            dlg_msg(h, id, EM_SETSEL, mp(0, 0xffff), 0);
            unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, id)) };
            1
        }
        WM_COMMAND => {
            let id = lo(mp1);
            if id == ID_SEARCH || id == ID_SAVEBTN {
                let f = if pmkit::item(h, ID_CMDLINE) != 0 { ID_CMDLINE } else { ID_NAME };
                *text = item_text(h, f);
                unsafe { WinDismissDlg(h, DID_OK) };
            } else {
                unsafe { WinDismissDlg(h, DID_CANCEL) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Selected > Command...: a command line for the file, run by CMD.EXE.
fn command_selected() {
    let a = app();
    let Some((_, Some(path))) = selection() else { return };
    unsafe { ENTRY_TEXT = quoted(&path) };
    let tpl = template(
        dialog_frame(b"File Command", DLG_COMMAND, 232, 52),
        alloc::vec![
            label(b"Enter the command you wish to execute:", (10, 39, 217, 8), DT_LEFT),
            entry((12, 25, 212, 8), ID_CMDLINE, 0),
            button(b"~OK", (10, 6, 42), ID_SEARCH, BS_DEFAULT),
            button(b"Cancel", (67, 6, 45), ID_CANCEL, 0),
            help_button((128, 6, 45), ID_HELPBTN),
        ],
    );
    if run_dialog(a.dlg, entry_proc, &tpl, 0) != DID_OK {
        return;
    }
    let cmd = unsafe { (*core::ptr::addr_of!(ENTRY_TEXT)).clone() };
    if cmd.is_empty() {
        return;
    }
    let rc = start_session(&find_program(b"CMD.EXE"), &[&b"/C "[..], &cmd].concat(), 2);
    if rc != 0 && rc != 457 {
        let t = [alloc::format!("RC= {rc} from DosStartSession ").as_bytes(), &cmd].concat();
        message(a.dlg, &t, TITLE, MB_OK | MB_ERROR);
    }
}

// ---- File and Edit ----

/// Write the list to a file.
fn save_list(name: &[u8]) {
    let a = app();
    let n = dlg_msg(a.dlg, ID_LIST, LM_QUERYITEMCOUNT, 0, 0);
    let mut out = Vec::new();
    for i in 0..n {
        out.extend_from_slice(&lb_text(a.dlg, ID_LIST, i));
        out.extend_from_slice(b"\r\n");
    }
    match write_file(name, &out) {
        Ok(()) => {
            let t = [alloc::format!("{n} list entries saved in ").as_bytes(), name].concat();
            status(&t);
            trace(&t);
        }
        Err(_) => {
            message(a.dlg, &[&b"Unable to Open File "[..], name].concat(), TITLE, MB_OK | MB_ERROR);
        }
    }
}

fn save_as() {
    let a = app();
    unsafe { ENTRY_TEXT = a.saveas.clone() };
    let tpl = template(
        dialog_frame(b"Save As", DLG_SAVEAS, 170, 44),
        alloc::vec![
            label(b"Save as file name:", (10, 30, 90, 8), DT_LEFT),
            entry((12, 20, 145, 8), ID_NAME, 0),
            button(b"~Save", (10, 4, 47), ID_SAVEBTN, BS_DEFAULT),
            button(b"Cancel", (62, 4, 47), ID_CANCEL, 0),
            help_button((114, 4, 47), ID_HELPBTN),
        ],
    );
    if run_dialog(a.dlg, entry_proc, &tpl, 0) != DID_OK {
        return;
    }
    let name = unsafe { (*core::ptr::addr_of!(ENTRY_TEXT)).clone() };
    if name.is_empty() {
        return;
    }
    a.saveas = name.clone();
    ini::write_str(INI_APP, b"Saveas", &name);
    save_list(&name);
}

/// Edit > Copy: the selected line to the clipboard as text.
fn copy_line() {
    let a = app();
    let Some((text, _)) = selection() else {
        message(a.dlg, b"Cannot copy to Clipboard.  You must first select a Listbox entry with Button 1.", TITLE, MB_OK | MB_INFORMATION);
        return;
    };
    let mut mem: *mut u8 = core::ptr::null_mut();
    // PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GIVEABLE
    if unsafe { os2::DosAllocSharedMem(&mut mem, core::ptr::null(), text.len() as u32 + 1, 0x13 | 0x200) } != 0 {
        return;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), mem, text.len());
        *mem.add(text.len()) = 0;
        if WinOpenClipbrd(a.hab) != 0 {
            WinEmptyClipbrd(a.hab);
            WinSetClipbrdData(a.hab, mem as u32, CF_TEXT, CFI_POINTER);
            WinCloseClipbrd(a.hab);
        }
    }
}

// ---- Sizing ----

const LAYOUT_IDS: &[u32] = &[ID_NAME, ID_TEXT, ID_EDITOR, ID_ALLFIXED, ID_STATUS, ID_LIST];

/// Note where everything is (the controls above the list keep their
/// distance from the top, the list grows).
fn record_layout(dlg: u32) {
    let a = app();
    a.layout.clear();
    let mut h = unsafe { WinQueryWindow(dlg, 2 /* QW_TOP */) };
    while h != 0 {
        let id = unsafe { WinQueryWindowUShort(h, -1 /* QWS_ID */) } as u32;
        if id < 0x8000 {
            a.layout.push((h, window_pos(h)));
        }
        h = unsafe { WinQueryWindow(h, 0 /* QW_NEXT */) };
    }
    let s = window_pos(dlg);
    a.size0 = (s.cx, s.cy);
    let _ = LAYOUT_IDS;
}

fn relayout(cx: i32, cy: i32) {
    let a = app();
    if a.size0.0 == 0 {
        return;
    }
    let (dx, dy) = (cx - a.size0.0, cy - a.size0.1);
    let list = pmkit::item(a.dlg, ID_LIST);
    let list0 = a.layout.iter().find(|e| e.0 == list).map(|e| e.1);
    let Some(l0) = list0 else { return };
    for &(h, s) in &a.layout {
        if h == list {
            unsafe { WinSetWindowPos(h, 0, s.x, s.y, (s.cx + dx).max(40), (s.cy + dy).max(20), SWP_SIZE) };
        } else if s.y >= l0.y + l0.cy {
            unsafe { WinSetWindowPos(h, 0, s.x, s.y + dy.max(20 - l0.cy), 0, 0, SWP_MOVE) };
        }
    }
}

// ---- The window ----

extern "C" fn main_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match m {
        WM_INITDLG => {
            a.dlg = h;
            for id in [ID_NAME, ID_TEXT, ID_EDITOR] {
                dlg_msg(h, id, EM_SETTEXTLIMIT, 259, 0);
            }
            set_item_text(h, ID_NAME, b"*.*");
            let editor = ini::read_str(INI_APP, b"Editor").filter(|e| !e.is_empty()).unwrap_or_else(|| b"E.EXE".to_vec());
            set_item_text(h, ID_EDITOR, &editor);
            init_drives(h);
            record_layout(h);
            0
        }
        WM_SIZE => {
            let r = unsafe { WinDefDlgProc(h, m, mp1, mp2) };
            relayout(lo(mp2) as i16 as i32, hi(mp2) as i16 as i32);
            r
        }
        WM_SEEK_STATUS => {
            let t = unsafe { core::slice::from_raw_parts(mp1 as *const u8, mp2 as usize) };
            status(t);
            0
        }
        WM_SEEK_FILE => {
            let f = unsafe { &*(mp1 as *const search::Found) };
            add_found(f);
            0
        }
        WM_SEEK_DONE => {
            search_done(mp1, mp2 != 0);
            0
        }
        WM_CONTROL => {
            let (id, code) = (lo(mp1), hi(mp1));
            if id == ID_LIST {
                match code {
                    LN_SELECT => selection_changed(),
                    LN_ENTER => open_selected(),
                    _ => {}
                }
            } else if id == ID_ALLFIXED && (code == BN_CLICKED || code == BN_DBLCLICKED) {
                all_fixed(h, checked(h, ID_ALLFIXED));
            } else if (ID_DRIVE0..ID_DRIVE0 + 26).contains(&id) && code == BN_CLICKED && !checked(h, id) {
                set_check(h, ID_ALLFIXED, false);
            }
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_SEARCH => start_search(),
                ID_STOP => STOP.store(true, Ordering::SeqCst),
                ID_OPENBTN => open_selected(),
                x => command(x as u16),
            }
            0
        }
        WM_CLOSE => {
            STOP.store(true, Ordering::SeqCst);
            post(h, WM_QUIT, 0, 0);
            0
        }
        HM_QUERY_KEYS_HELP => PANEL_KEYS as u32,
        HM_ERROR => {
            trace(alloc::format!("help error {:#x}", mp1).as_bytes());
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

fn command(id: u16) {
    let a = app();
    match id {
        IDM_SAVE => {
            let n = a.saveas.clone();
            save_list(&n);
        }
        IDM_SAVEAS => save_as(),
        IDM_OPEN => open_selected(),
        IDM_PROCESS => process_selected(),
        IDM_COMMAND => command_selected(),
        IDM_COPY => copy_line(),
        IDM_CLEAR => clear_list(),
        IDM_SEARCHKEY => start_search(),
        IDM_STOPKEY => STOP.store(true, Ordering::SeqCst),
        IDM_SUBDIRS => a.subdirs = !a.subdirs,
        IDM_FOUNDTEXT => a.found_text = !a.found_text,
        IDM_IGNORECASE => a.ignore_case = !a.ignore_case,
        IDM_CLEARONSEARCH => a.clear_on_search = !a.clear_on_search,
        IDM_DEFAULTS => {
            save_defaults();
            status(b"Set PMSEEK Defaults");
        }
        IDM_HELPINDEX => help(HM_HELP_INDEX),
        IDM_GENERALHELP => help(HM_EXT_HELP),
        IDM_USINGHELP => help(HM_DISPLAY_HELP),
        IDM_KEYSHELP => help(HM_KEYS_HELP),
        IDM_PRODUCT => product_info(a.dlg, DLG_PRODUCT, b"Seek and Scan Files - Product Information", b"Seek and Scan Files", &[b"rs2 (OS/2 Warp 4.52 compatible)", b"Finds files by name and by the text in them."]),
        DID_CANCEL_U16 => {}
        _ => {}
    }
    update_menu();
}

const DID_CANCEL_U16: u16 = DID_CANCEL as u16;

fn help(msg: u32) {
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.dlg, HELP_FILE, TITLE, msg, 0, 0);
}
