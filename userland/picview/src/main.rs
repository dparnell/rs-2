//! PICVIEW.EXE: rs2's Picture Viewer, a re-implementation of OS/2's.
//!
//! As OS/2's (studied in its binary and resources): it shows a metafile
//! (.MET) or a spool file of the standard kind (.SPL: a metafile whose
//! pages end with NEWFRAME) fitted to the window, the picture's own
//! extent ("Fit picture in window") or its whole page ("Fit page in
//! window"), on the default or a black background. File: Open (OS/2's own
//! Open window, which can take several files and show them one after
//! the other, interactively with Ctrl+N or automatically after a delay:
//! the Sequence menu), Save as (a metafile), Print (copies; every page).
//! Edit: Cut, Copy and Paste through the clipboard as CF_METAFILE. Page:
//! Forward, Backward, Go to page. `PICVIEW file` opens a file;
//! `PICVIEW /P file` prints it and ends ("Picture Printer"); `/S` does not
//! use or keep the saved window position. Settings are OS2.INI
//! `PicView`: WinPos (the SWP), BestFit and BlkBackg (there or not),
//! as OS/2 keeps them. Menu ids, accelerators, dialog ids and help panel
//! numbers are OS/2's; the help is PICVIEW.HLP.
//!
//! PIF files (OS/2 converts them with PMPIC.DLL) are not read: opening
//! one says that it is not a metafile.

#![no_std]
#![no_main]

extern crate alloc;

mod dialogs;
mod doc;
mod files;
mod print;
mod selftest;
mod view;

use alloc::vec::Vec;

use pmkit::help::{Help, HelpWindow};
use pmkit::*;

use doc::Doc;

os2::entry!(main);

// Menu ids (OS/2's).
pub const IDM_FILE: u16 = 1;
pub const IDM_OPEN: u16 = 10;
pub const IDM_SAVEAS: u16 = 11;
pub const IDM_PRINT: u16 = 12;
pub const IDM_EDIT: u16 = 2;
pub const IDM_CUT: u16 = 21;
pub const IDM_COPY: u16 = 22;
pub const IDM_PASTE: u16 = 23;
pub const IDM_PAGE: u16 = 3;
pub const IDM_FORWARD: u16 = 31;
pub const IDM_BACKWARD: u16 = 32;
pub const IDM_GOTO: u16 = 33;
pub const IDM_OPTIONS: u16 = 4;
pub const IDM_FITPICTURE: u16 = 41;
pub const IDM_FITPAGE: u16 = 42;
pub const IDM_DEFBACK: u16 = 43;
pub const IDM_BLACKBACK: u16 = 44;
pub const IDM_CODEPAGE: u16 = 45;
pub const IDM_SEQUENCE: u16 = 8;
pub const IDM_NEXT: u16 = 81;
pub const IDM_QUIT: u16 = 82;
pub const IDM_HELP: u16 = 5;
pub const IDM_USINGHELP: u16 = 6;
pub const IDM_PRODUCT: u16 = 7;
pub const IDM_GENERALHELP: u16 = 711;
pub const IDM_KEYSHELP: u16 = 712;
pub const IDM_HELPINDEX: u16 = 713;

pub const FRAME_ID: u32 = 257;
pub const DLG_PRODUCT: u16 = 71;
pub const PANEL_KEYS: u32 = 7244;
pub const TITLE: &[u8] = b"Picture Viewer";
const HELP_FILE: &[u8] = b"PICVIEW.HLP";
const INI_APP: &[u8] = b"PicView";
const TIMER_SEQUENCE: u32 = 1;

pub struct App {
    pub hab: u32,
    pub frame: u32,
    pub client: u32,
    pub menu: u32,
    /// The window's PS (a normal one: metafiles are played on it).
    pub hps: u32,
    pub help: Option<&'static mut Help>,
    pub doc: Option<Doc>,
    pub fit_picture: bool,
    pub black: bool,
    /// The files of a sequence and the one shown.
    pub sequence: Vec<Vec<u8>>,
    pub seq_index: usize,
    pub automatic: bool,
    pub delay: u32,
    /// `/P`: print and end.
    pub print_only: bool,
    /// `/S`: no saved window position.
    pub no_position: bool,
    pub copies: u32,
}

static mut APP: App = App {
    hab: 0,
    frame: 0,
    client: 0,
    menu: 0,
    hps: 0,
    help: None,
    doc: None,
    fit_picture: true,
    black: false,
    sequence: Vec::new(),
    seq_index: 0,
    automatic: false,
    delay: 5,
    print_only: false,
    no_position: false,
    copies: 1,
};

pub fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

pub fn trace(s: &[u8]) {
    pmkit::trace("picview", s);
}

fn main(s: &os2::Startup) -> u32 {
    let a = app();
    let mut file: Option<Vec<u8>> = None;
    let mut extra = false;
    let mut selftest = false;
    for w in words(s.args()) {
        if w.first() == Some(&b'/') || w.first() == Some(&b'-') {
            for &c in &w[1..] {
                match c.to_ascii_uppercase() {
                    b'P' => a.print_only = true,
                    b'S' => a.no_position = true,
                    _ => extra = true,
                }
            }
            if w.eq_ignore_ascii_case(b"/SELFTEST") {
                selftest = true;
                extra = false;
                a.print_only = false;
                a.no_position = false;
            }
        } else if file.is_none() {
            file = Some(w);
        } else {
            extra = true;
        }
    }
    if a.print_only {
        a.no_position = true;
    }
    a.hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if hmq == 0 {
        return 1;
    }
    // `/SELFTEST`: the checks; with a file, the program then goes on
    // with it (the checks leave C:\PVPAGES.SPL, a spool file of three
    // pages, to look at).
    if selftest {
        let fails = selftest::run(a.hab);
        if file.is_some() {
            extra = false;
        } else {
            unsafe {
                WinDestroyMsgQueue(hmq);
                WinTerminate(a.hab);
            }
            return fails;
        }
    }
    a.fit_picture = ini::read(INI_APP, b"BestFit").is_some_and(|v| v.len() == 4) || ini::read(INI_APP, b"BestFit").is_none();
    a.black = ini::read(INI_APP, b"BlkBackg").is_some_and(|v| v.len() == 4);
    if !create_window() {
        message(0, b"The program cannot create a standard window.", TITLE, MB_OK | MB_ERROR);
        return 1;
    }
    if a.print_only {
        set_text(a.frame, b"Picture Printer");
    } else {
        show_window();
    }
    if extra {
        message(a.frame, b"Extra input on the command line was ignored.", TITLE, MB_OK | MB_WARNING);
    }
    if let Some(f) = file {
        a.sequence = alloc::vec![files::full_name(&f)];
        a.seq_index = 0;
        show_file(0);
        if a.print_only {
            if a.doc.is_some() {
                do_print();
            }
            post(a.frame, WM_QUIT, 0, 0);
        }
    } else if a.print_only {
        post(a.frame, WM_QUIT, 0, 0);
    }
    message_loop(a.hab);
    if !a.no_position {
        save_position(a.frame, INI_APP, b"WinPos");
    }
    unsafe {
        if let Some(h) = a.help.as_mut() {
            h.destroy(a.frame);
        }
        a.doc = None;
        GpiAssociate(a.hps, 0);
        GpiDestroyPS(a.hps);
        WinDestroyWindow(a.frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(a.hab);
    }
    0
}

fn create_window() -> bool {
    let a = app();
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let (frame, client) = std_window(a.hab, b"Meta.client", client_proc, CS_SIZEREDRAW, 0, fcf, TITLE, FRAME_ID);
    if frame == 0 {
        return false;
    }
    a.frame = frame;
    a.client = client;
    let hdc = unsafe { WinOpenWindowDC(client) };
    a.hps = unsafe { GpiCreatePS(a.hab, hdc, &SizeL { cx: 0, cy: 0 }, PU_PELS | GPIF_DEFAULT | GPIT_NORMAL | GPIA_ASSOC) };
    a.menu = set_menu(frame, &menu_items());
    // OS/2's accelerators: Ctrl+O, S, N, Q, F, B (with or without Shift),
    // Shift+Delete, Ctrl+Insert, Shift+Insert and Shift+Print Screen.
    let mut acc: Vec<(u16, u16, u16)> = Vec::new();
    for (ch, cmd) in [(b'o', IDM_OPEN), (b's', IDM_SAVEAS), (b'n', IDM_NEXT), (b'q', IDM_QUIT), (b'f', IDM_FORWARD), (b'b', IDM_BACKWARD)] {
        for fs in [AF_CHAR | AF_CONTROL, AF_CHAR | AF_CONTROL | AF_SHIFT] {
            acc.push((fs, ch as u16, cmd));
            acc.push((fs, ch.to_ascii_uppercase() as u16, cmd));
        }
    }
    acc.push((AF_VIRTUALKEY | AF_SHIFT, VK_DELETE, IDM_CUT));
    acc.push((AF_VIRTUALKEY | AF_CONTROL, VK_INSERT, IDM_COPY));
    acc.push((AF_VIRTUALKEY | AF_SHIFT, VK_INSERT, IDM_PASTE));
    acc.push((AF_VIRTUALKEY | AF_SHIFT, 0x19 /* VK_PRINTSCRN */, IDM_PRINT));
    set_accels(a.hab, frame, &acc);
    let help = HELP_TABLE;
    a.help = Help::create(a.hab, frame, HELP_FILE, b"Picture Viewer Help", &help_windows(help));
    true
}

/// The help table (OS/2's numbers): the main window's menu items and the
/// dialogs.
const HELP_TABLE: &[(u16, u16)] = &[
    (IDM_FILE, 7201),
    (IDM_OPEN, 7202),
    (IDM_SAVEAS, 7203),
    (IDM_PRINT, 7204),
    (IDM_EDIT, 7206),
    (IDM_CUT, 7207),
    (IDM_COPY, 7208),
    (IDM_PASTE, 7209),
    (IDM_PAGE, 7210),
    (IDM_FORWARD, 7211),
    (IDM_BACKWARD, 7212),
    (IDM_GOTO, 7213),
    (IDM_OPTIONS, 7214),
    (IDM_FITPICTURE, 7215),
    (IDM_FITPAGE, 7216),
    (IDM_DEFBACK, 7217),
    (IDM_BLACKBACK, 7218),
    (IDM_CODEPAGE, 7219),
    (IDM_SEQUENCE, 7250),
    (IDM_NEXT, 7220),
    (IDM_QUIT, 7221),
    (IDM_HELP, 6422),
    (IDM_USINGHELP, 6423),
    (IDM_GENERALHELP, 6424),
    (IDM_KEYSHELP, 6425),
    (IDM_HELPINDEX, 6426),
    (IDM_PRODUCT, 6496),
];

fn help_windows(main: &'static [(u16, u16)]) -> [HelpWindow<'static>; 7] {
    const OPEN: &[(u16, u16)] = &[(402, 7222), (403, 7222), (404, 7222), (405, 7222), (407, 7222), (408, 7222), (409, 7222), (410, 7222), (411, 7222), (412, 7222), (413, 7222), (1, 7222), (2, 7222)];
    const SAVE: &[(u16, u16)] = &[(402, 7231), (403, 7231), (404, 7231), (405, 7231), (410, 7231), (1, 7231), (2, 7231)];
    const PRINT: &[(u16, u16)] = &[(304, 7236), (410, 7236), (1, 7236), (2, 7236)];
    const PAGE: &[(u16, u16)] = &[(303, 7239), (410, 7239), (1, 7239), (2, 7239)];
    const CODEPAGE: &[(u16, u16)] = &[(551, 7240), (552, 7240), (1, 7240), (553, 7240), (2, 7240), (410, 7240)];
    const PRODUCT: &[(u16, u16)] = &[(1, 6496)];
    [
        HelpWindow { id: FRAME_ID as u16, ext: 7200, items: main },
        HelpWindow { id: files::DLG_OPEN, ext: 7222, items: OPEN },
        HelpWindow { id: files::DLG_SAVEAS, ext: 7231, items: SAVE },
        HelpWindow { id: dialogs::DLG_PRINT, ext: 7236, items: PRINT },
        HelpWindow { id: dialogs::DLG_PAGE, ext: 7239, items: PAGE },
        HelpWindow { id: dialogs::DLG_CODEPAGE, ext: 7240, items: CODEPAGE },
        HelpWindow { id: DLG_PRODUCT, ext: 6496, items: PRODUCT },
    ]
}

fn menu_items() -> Vec<Menu<'static>> {
    alloc::vec![
        Menu::Sub(IDM_FILE, b"~File", alloc::vec![
            Menu::Item(IDM_OPEN, b"~Open...\tCtrl+O"),
            Menu::Item(IDM_SAVEAS, b"Save ~as...\tCtrl+S"),
            Menu::Item(IDM_PRINT, b"~Print...\tShift+Print Screen"),
        ]),
        Menu::Sub(IDM_EDIT, b"~Edit", alloc::vec![
            Menu::Item(IDM_CUT, b"Cu~t\tShift+Delete"),
            Menu::Item(IDM_COPY, b"~Copy\tCtrl+Insert"),
            Menu::Item(IDM_PASTE, b"~Paste\tShift+Insert"),
        ]),
        Menu::Sub(IDM_PAGE, b"~Page", alloc::vec![
            Menu::Item(IDM_FORWARD, b"~Forward\tCtrl+F"),
            Menu::Item(IDM_BACKWARD, b"~Backward\tCtrl+B"),
            Menu::Item(IDM_GOTO, b"~Go to page..."),
        ]),
        Menu::Sub(IDM_OPTIONS, b"~Options", alloc::vec![
            Menu::Item(IDM_FITPICTURE, b"Fit ~picture in window"),
            Menu::Item(IDM_FITPAGE, b"Fit p~age in window"),
            Menu::Sep,
            Menu::Item(IDM_DEFBACK, b"~Default background"),
            Menu::Item(IDM_BLACKBACK, b"~Black background"),
            Menu::Sep,
            Menu::Item(IDM_CODEPAGE, b"~Code page..."),
        ]),
        Menu::Sub(IDM_SEQUENCE, b"~Sequence", alloc::vec![
            Menu::Item(IDM_NEXT, b"~Next picture\tCtrl+N"),
            Menu::Item(IDM_QUIT, b"~Quit sequence\tCtrl+Q"),
        ]),
        Menu::Sub(IDM_HELP, b"~Help", alloc::vec![
            Menu::Item(IDM_HELPINDEX, b"Help ~index"),
            Menu::Item(IDM_GENERALHELP, b"~General help"),
            Menu::Item(IDM_USINGHELP, b"~Using help"),
            Menu::Item(IDM_KEYSHELP, b"~Keys help"),
            Menu::Sep,
            Menu::Item(IDM_PRODUCT, b"~Product information"),
        ]),
    ]
}

fn show_window() {
    let a = app();
    if a.no_position {
        show_at_saved(a.hab, a.frame, b"\x01", b"\x01", None);
    } else {
        show_at_saved(a.hab, a.frame, INI_APP, b"WinPos", None);
    }
}

/// The title: "Picture Viewer - NAME", and the page of several.
pub fn set_title() {
    let a = app();
    let mut t = if a.print_only { b"Picture Printer".to_vec() } else { TITLE.to_vec() };
    if let Some(d) = &a.doc {
        t.extend_from_slice(b" - ");
        if d.path.is_empty() {
            t.extend_from_slice(b"Clipboard");
        } else {
            t.extend_from_slice(file_name(&d.path));
        }
        if d.pages.len() > 1 {
            t.extend_from_slice(alloc::format!(" - page {} of {}", d.page + 1, d.pages.len()).as_bytes());
        }
    }
    set_text(a.frame, &t);
}

/// Show a document (or nothing), with a message for a file that could
/// not be shown.
pub fn set_doc(r: Result<Doc, doc::Error>) -> bool {
    let a = app();
    let ok = match r {
        Ok(d) => {
            trace(alloc::format!("{} page(s), size {}x{} units {}", d.pages.len(), d.size.0, d.size.1, d.units).as_bytes());
            a.doc = Some(d);
            true
        }
        Err(e) => {
            a.doc = None;
            let t: &[u8] = match e {
                doc::Error::Open => b"The file could not be opened. Select another one in the Open window.",
                doc::Error::NotMetafile => b"The file is not a valid metafile. Select another one in the Open window.",
                doc::Error::Empty => b"The metafile contains no graphics.",
            };
            trace(t);
            if !a.print_only {
                message(a.frame, t, TITLE, MB_OK | MB_WARNING);
            }
            false
        }
    };
    set_title();
    invalidate(a.client);
    ok
}

/// Show file `i` of the sequence.
fn show_file(i: usize) {
    let a = app();
    let Some(f) = a.sequence.get(i).cloned() else { return };
    a.seq_index = i;
    unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, SPTR_WAIT, 0)) };
    trace(&[&b"open "[..], &f].concat());
    a.doc = None;
    set_doc(doc::open(a.hab, &f));
    unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, SPTR_ARROW, 0)) };
}

fn in_sequence() -> bool {
    app().sequence.len() > 1
}

fn next_picture() {
    let a = app();
    if a.seq_index + 1 < a.sequence.len() {
        show_file(a.seq_index + 1);
    }
    if a.seq_index + 1 >= a.sequence.len() {
        end_sequence();
    }
}

fn end_sequence() {
    let a = app();
    unsafe { WinStopTimer(a.hab, a.client, TIMER_SEQUENCE) };
    if a.sequence.len() > 1 {
        let cur = a.sequence[a.seq_index].clone();
        a.sequence = alloc::vec![cur];
        a.seq_index = 0;
    }
}

fn do_open() {
    let a = app();
    let Some(c) = files::open(a.frame) else { return };
    end_sequence();
    a.sequence = c.files;
    a.automatic = c.automatic;
    a.delay = c.delay;
    show_file(0);
    if a.sequence.len() > 1 && a.automatic {
        unsafe { WinStartTimer(a.hab, a.client, TIMER_SEQUENCE, a.delay * 1000) };
    }
}

fn do_save_as() {
    let a = app();
    let Some(d) = &a.doc else { return };
    let initial = file_name(&d.path).to_vec();
    let Some(name) = files::save_as(a.frame, &initial) else { return };
    let z = z(&name);
    let mut info = [0u8; 64];
    if unsafe { os2::DosQueryPathInfo(z.as_ptr(), 1, info.as_mut_ptr(), info.len() as u32) } == 0
        && message(a.frame, b"The picture is to be saved in a file that exists. Do you want to replace the file?", TITLE, MB_YESNO | MB_WARNING) != MBID_YES
    {
        return;
    }
    unsafe { os2::DosDelete(z.as_ptr()) };
    // GpiSaveMetaFile deletes the handle: save a copy.
    let copy = unsafe { GpiCopyMetaFile(d.hmf()) };
    if copy == 0 || unsafe { GpiSaveMetaFile(copy, z.as_ptr()) } == 0 {
        message(a.frame, b"The picture could not be saved in that file: the file could not be opened, the disk is full, or the metafile is in use.", TITLE, MB_OK | MB_ERROR);
        return;
    }
    trace(&[&b"saved "[..], &name].concat());
}

pub fn do_print() {
    let a = app();
    let Some(d) = a.doc.as_mut() else { return };
    let name = if d.path.is_empty() { b"Clipboard".to_vec() } else { file_name(&d.path).to_vec() };
    unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, SPTR_WAIT, 0)) };
    let r = print::print(a.hab, d, &name, a.copies);
    unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, SPTR_ARROW, 0)) };
    match r {
        Ok(()) => trace(alloc::format!("printed {} copies", a.copies).as_bytes()),
        Err(print::PrintError::NoPrinter) => {
            trace(b"no printer");
            message(a.frame, b"There is no printer installed in the system.", TITLE, MB_OK | MB_WARNING);
        }
        Err(print::PrintError::Failed(n)) => {
            trace(alloc::format!("printing failed: {n}").as_bytes());
            let t = alloc::format!("The picture could not be printed: the printer could not be reached or printing failed. Error number {n}.");
            message(a.frame, t.as_bytes(), TITLE, MB_OK | MB_ERROR);
        }
    }
}

fn copy_to_clipboard() -> bool {
    let a = app();
    let Some(d) = &a.doc else { return false };
    let copy = unsafe { GpiCopyMetaFile(d.hmf()) };
    if copy == 0 {
        message(a.frame, b"The metafile could not be copied: it is in use or too many metafiles are in use.", TITLE, MB_OK | MB_ERROR);
        return false;
    }
    if unsafe { WinOpenClipbrd(a.hab) } == 0 {
        unsafe { GpiDeleteMetaFile(copy) };
        message(a.frame, b"The clipboard could not be opened.", TITLE, MB_OK | MB_ERROR);
        return false;
    }
    unsafe {
        WinEmptyClipbrd(a.hab);
        WinSetClipbrdData(a.hab, copy, CF_METAFILE, CFI_HANDLE);
        WinCloseClipbrd(a.hab);
    }
    trace(b"copied");
    true
}

fn paste() {
    let a = app();
    if unsafe { WinOpenClipbrd(a.hab) } == 0 {
        message(a.frame, b"The clipboard could not be opened.", TITLE, MB_OK | MB_ERROR);
        return;
    }
    let data = unsafe { WinQueryClipbrdData(a.hab, CF_METAFILE) };
    let copy = if data != 0 { unsafe { GpiCopyMetaFile(data) } } else { 0 };
    unsafe { WinCloseClipbrd(a.hab) };
    if copy == 0 {
        return;
    }
    end_sequence();
    a.sequence.clear();
    a.doc = None;
    set_doc(doc::from_handle(a.hab, copy));
    trace(b"pasted");
}

fn clipboard_has_metafile() -> bool {
    let a = app();
    let mut info = 0;
    unsafe { WinQueryClipbrdFmtInfo(a.hab, CF_METAFILE, &mut info) != 0 }
}

pub fn go_to_page(p: usize) {
    let a = app();
    if let Some(d) = a.doc.as_mut() {
        if p < d.pages.len() && p != d.page {
            d.page = p;
            set_title();
            invalidate(a.client);
        }
    }
}

fn init_menu() {
    let a = app();
    let have = a.doc.is_some();
    let pages = a.doc.as_ref().map_or(0, |d| d.pages.len());
    let page = a.doc.as_ref().map_or(0, |d| d.page);
    let m = a.menu;
    menu_enable(m, IDM_SAVEAS, have);
    menu_enable(m, IDM_PRINT, have);
    menu_enable(m, IDM_CUT, have);
    menu_enable(m, IDM_COPY, have);
    menu_enable(m, IDM_PASTE, clipboard_has_metafile());
    menu_enable(m, IDM_FORWARD, page + 1 < pages);
    menu_enable(m, IDM_BACKWARD, page > 0);
    menu_enable(m, IDM_GOTO, pages > 1);
    menu_check(m, IDM_FITPICTURE, a.fit_picture);
    menu_check(m, IDM_FITPAGE, !a.fit_picture);
    menu_check(m, IDM_DEFBACK, !a.black);
    menu_check(m, IDM_BLACKBACK, a.black);
    menu_enable(m, IDM_NEXT, in_sequence() && a.seq_index + 1 < a.sequence.len());
    menu_enable(m, IDM_QUIT, in_sequence());
}

fn command(id: u16) {
    let a = app();
    match id {
        IDM_OPEN => do_open(),
        IDM_SAVEAS => do_save_as(),
        IDM_PRINT => {
            if a.doc.is_some() {
                if default_printer_missing() {
                    message(a.frame, b"There is no printer installed in the system.", TITLE, MB_OK | MB_WARNING);
                } else if let Some(n) = dialogs::print_dialog(a.frame, a.copies) {
                    a.copies = n;
                    do_print();
                }
            }
        }
        IDM_CUT => {
            if copy_to_clipboard() {
                end_sequence();
                a.doc = None;
                set_title();
                invalidate(a.client);
            }
        }
        IDM_COPY => {
            copy_to_clipboard();
        }
        IDM_PASTE => paste(),
        IDM_FORWARD => {
            let p = a.doc.as_ref().map_or(0, |d| d.page + 1);
            go_to_page(p);
        }
        IDM_BACKWARD => {
            let p = a.doc.as_ref().map_or(0, |d| d.page);
            if p > 0 {
                go_to_page(p - 1);
            }
        }
        IDM_GOTO => {
            if let Some(d) = &a.doc {
                if let Some(p) = dialogs::page_dialog(a.frame, d.pages.len()) {
                    go_to_page(p);
                }
            }
        }
        IDM_FITPICTURE | IDM_FITPAGE => {
            a.fit_picture = id == IDM_FITPICTURE;
            // As OS/2: BestFit is there (4 bytes) for "Fit picture".
            if a.fit_picture {
                ini::write(INI_APP, b"BestFit", &1u32.to_le_bytes());
            } else {
                // OS/2 deletes the key (and then starts fitting the
                // picture again); a key of another size keeps the choice.
                ini::write(INI_APP, b"BestFit", &[0]);
            }
            invalidate(a.client);
        }
        IDM_DEFBACK | IDM_BLACKBACK => {
            a.black = id == IDM_BLACKBACK;
            if a.black {
                ini::write(INI_APP, b"BlkBackg", &1u32.to_le_bytes());
            } else {
                ini::delete(INI_APP, b"BlkBackg");
            }
            invalidate(a.client);
        }
        IDM_CODEPAGE => dialogs::code_page_dialog(a.frame),
        IDM_NEXT => {
            if in_sequence() {
                next_picture();
            }
        }
        IDM_QUIT => {
            if in_sequence() && message(a.frame, b"The pictures are being shown in a sequence. Do you want to end the sequence?", TITLE, MB_YESNO | MB_ICONQUESTION) == MBID_YES {
                end_sequence();
            }
        }
        IDM_HELPINDEX => help_request(HM_HELP_INDEX, 0, 0),
        IDM_GENERALHELP => help_request(HM_EXT_HELP, 0, 0),
        IDM_USINGHELP => help_request(HM_DISPLAY_HELP, 0, 0),
        IDM_KEYSHELP => help_request(HM_KEYS_HELP, 0, 0),
        IDM_PRODUCT => product_info(a.frame, DLG_PRODUCT, b"Picture Viewer - Product Information", b"Picture Viewer", &[b"rs2 (OS/2 Warp 4.52 compatible)", b"Shows, prints and copies metafiles and spool files."]),
        _ => {}
    }
}

fn default_printer_missing() -> bool {
    print::default_queue().is_none()
}

fn help_request(msg: u32, mp1: u32, mp2: u32) {
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.frame, HELP_FILE, TITLE, msg, mp1, mp2);
}

fn paint(hwnd: u32) {
    let a = app();
    let mut upd = RectL::default();
    let hps = unsafe { WinBeginPaint(hwnd, a.hps, &mut upd) };
    unsafe { GpiSetPS(hps, &SizeL { cx: 0, cy: 0 }, PU_PELS | GPIF_DEFAULT) };
    let r = window_rect(hwnd);
    unsafe { WinFillRect(hps, &r, if a.black { CLR_BLACK } else { SYSCLR_WINDOW }) };
    if let Some(d) = a.doc.as_mut() {
        let into = r;
        if into.r > into.l && into.t > into.b {
            view::draw(hps, d, into, a.fit_picture);
        }
        unsafe { GpiSetPS(hps, &SizeL { cx: 0, cy: 0 }, PU_PELS | GPIF_DEFAULT) };
    }
    unsafe { WinEndPaint(hps) };
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBACKGROUND => 0,
        WM_INITMENU => {
            init_menu();
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        WM_COMMAND => {
            command(lo(mp1) as u16);
            0
        }
        WM_TIMER if lo(mp1) == TIMER_SEQUENCE => {
            next_picture();
            0
        }
        WM_CHAR => {
            let fl = lo(mp1);
            if fl & KC_KEYUP == 0 && fl & KC_VIRTUALKEY != 0 {
                match hi(mp2) as u16 {
                    VK_PAGEDOWN => command(IDM_FORWARD),
                    VK_PAGEUP => command(IDM_BACKWARD),
                    _ => return unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
                }
                return 1;
            }
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        HM_QUERY_KEYS_HELP => PANEL_KEYS,
        HM_ERROR => {
            trace(alloc::format!("help error {:#x}", mp1).as_bytes());
            0
        }
        WM_HELP => 0,
        _ => {
            let _ = a;
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
    }
}
