//! GLOCALE.EXE: rs2's Locale program (OS/2's "Country Palette"), a
//! re-implementation of the one OS/2 Warp 4.52 puts in System Setup.
//!
//! As OS/2's (studied in its binary and resources): the Country Palette
//! window shows the user locales (UniQueryLocaleList) as globe icons in a
//! container; on the first start the process's locale is made a user
//! locale (UniMakeUserLocale, written to \LANGUAGE\LOCALE\USER.LOC by
//! UniCompleteUserLocale). Opening a locale (double click, Open as >
//! Locale, Properties) shows its notebook (book.rs): Country/Region,
//! Collate, Numeric, Monetary, Date, Time, Language and Advanced pages,
//! with samples formatted through LIBUNI; every change is a
//! UniSetUserLocaleItem and the notebook's closing writes USER.LOC.
//! Create another (Open as > New locale) asks for a name and makes a user
//! locale on the selected one's base, Delete deletes one (the last one
//! is made again from the system's locale, as OS/2's does), Mark as
//! default writes the locale's conventions to OS2.INI (`PM_National`,
//! `PM_Default_National`), \LANGUAGE\LOCALE\DEFAULT.LOC and, with
//! WIN-OS/2 installed, WIN.INI's [intl]. The menus, window and control
//! ids and help panel numbers are OS/2's; the help is GLOCALE.HLP.
//!
//! `GLOCALE /SELFTEST` checks the logic without windows: it makes a user
//! locale, changes it as the pages do, checks the items and USER.LOC,
//! and deletes it ("glocale selftest done: N failure(s)").

#![no_std]
#![no_main]

extern crate alloc;

mod book;
mod fmt;
mod icon;
mod selftest;
mod uni;

use alloc::string::String;
use alloc::vec::Vec;

use pmkit::help::{Help, HelpWindow};
use pmkit::*;

os2::entry!(main);

// Menu ids (OS/2's).
pub const IDM_NEWLOCALE: u16 = 501;
pub const IDM_DELETE: u16 = 502;
pub const IDM_OPEN: u16 = 504;
pub const IDM_DEFAULT: u16 = 505;
pub const IDM_OPENAS: u16 = 506;
pub const IDM_LOCALE: u16 = 508;
pub const IDM_EXIT: u16 = 509;
pub const IDM_HELP: u16 = 510;
pub const IDM_HELPINDEX: u16 = 511;
pub const IDM_GENERALHELP: u16 = 512;
pub const IDM_USINGHELP: u16 = 513;
pub const IDM_PRODUCT: u16 = 514;
pub const IDM_ACTION: u16 = 515;
pub const IDM_JAVA: u16 = 516;
/// The pop-up menu's Help item.
pub const IDM_POPUPHELP: u16 = 419;

pub const FRAME_ID: u32 = 106;
pub const CNR_ID: u32 = 600;
pub const DLG_NAME: u16 = 112;
pub const DLG_PRODUCT: u16 = 114;
pub const ID_NAME: u32 = 200;
pub const ID_OK: u32 = 400;
pub const ID_CANCEL: u32 = 401;
pub const ID_HELP: u32 = 419;

// Help panels (res= numbers of GLOCALE.HLP).
pub const PANEL_PALETTE: u16 = 5010;
pub const PANEL_NAME: u16 = 5020;
pub const PANEL_LOCALEMENU: u16 = 5011;
pub const PANEL_ACTIONMENU: u16 = 5012;
pub const PANEL_CREATE: u16 = 5013;
pub const PANEL_DELETE: u16 = 5014;
pub const PANEL_MARKDEFAULT: u16 = 5015;
pub const PANEL_JAVA: u16 = 5016;
pub const PANEL_PRODUCT: u16 = 5017;
pub const PANEL_OPEN: u16 = 5018;

pub const TITLE: &[u8] = b"Country Palette";
pub const ERROR_TITLE: &[u8] = b"Country Palette Error";
const HELP_FILE: &[u8] = b"GLOCALE.HLP";

// Container messages, notifications and flags.
pub const CM_ALLOCRECORD: u32 = 0x0331;
pub const CM_ARRANGE: u32 = 0x0332;
pub const CM_INSERTRECORD: u32 = 0x0339;
pub const CM_INVALIDATERECORD: u32 = 0x033b;
pub const CM_QUERYRECORD: u32 = 0x0340;
pub const CM_QUERYRECORDEMPHASIS: u32 = 0x0341;
pub const CM_REMOVERECORD: u32 = 0x0346;
pub const CM_SETCNRINFO: u32 = 0x0349;
pub const CM_SETRECORDEMPHASIS: u32 = 0x034a;
pub const CN_ENTER: u32 = 106;
pub const CN_CONTEXTMENU: u32 = 119;
pub const CN_HELP: u32 = 118;
pub const CCS_AUTOPOSITION: u32 = 0x0008;
pub const CCS_SINGLESEL: u32 = 0x0001;
pub const CV_ICON: u32 = 0x0004;
pub const CA_CONTAINERTITLE: u32 = 0x0200;
pub const CA_TITLESEPARATOR: u32 = 0x0400;
pub const CMA_CNRTITLE: u32 = 0x0001;
pub const CMA_FREE: u32 = 0x0001;
pub const CMA_INVALIDATE: u32 = 0x0002;
pub const CMA_FLWINDOWATTR: u32 = 0x0004;
pub const CMA_FIRST: u32 = 0x0010;
pub const CMA_NEXT: u32 = 0x0100;
pub const CMA_END: u32 = 0x0040;
pub const CRA_SELECTED: u32 = 0x0001;
pub const CRA_CURSORED: u32 = 0x0004;
const R_PSZICON: usize = 20;
const R_HPTR: usize = 24;
const R_HPTRMINI: usize = 28;
const R_PSZTEXT: usize = 44;
const R_PSZNAME: usize = 48;
const R_PSZTREE: usize = 52;

/// A locale's record in the container.
pub struct Rec {
    pub rec: u32,
    pub name: String,
    /// The record's text (the container points at it).
    _title: Vec<u8>,
}

pub struct App {
    pub hab: u32,
    pub hmq: u32,
    pub frame: u32,
    pub client: u32,
    pub cnr: u32,
    pub icon: u32,
    pub help: Option<&'static mut Help>,
    pub recs: Vec<Rec>,
    /// The language names are shown in (the process locale's).
    pub lang: String,
    /// The locale a menu command acts on (the record the pop-up menu
    /// was shown for).
    pub target: Option<String>,
}

static mut APP: App = App { hab: 0, hmq: 0, frame: 0, client: 0, cnr: 0, icon: 0, help: None, recs: Vec::new(), lang: String::new(), target: None };

pub fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

pub fn trace(s: &[u8]) {
    pmkit::trace("glocale", s);
}

/// An error box (OS/2's "Country Palette Error").
pub fn error(owner: u32, text: &[u8]) {
    trace(&[&b"error: "[..], text].concat());
    message(owner, text, ERROR_TITLE, MB_OK | MB_ERROR);
}

fn rd(p: u32, off: usize) -> u32 {
    unsafe { ((p as usize + off) as *const u32).read_unaligned() }
}

fn wr(p: u32, off: usize, v: u32) {
    unsafe { ((p as usize + off) as *mut u32).write_unaligned(v) };
}

fn main(s: &os2::Startup) -> u32 {
    let a = app();
    a.hab = unsafe { WinInitialize(0) };
    a.hmq = unsafe { WinCreateMsgQueue(a.hab, 0) };
    if a.hmq == 0 {
        return 1;
    }
    if pmkit::arg(s).eq_ignore_ascii_case(b"/SELFTEST") {
        let fails = selftest::run();
        unsafe {
            WinDestroyMsgQueue(a.hmq);
            WinTerminate(a.hab);
        }
        return fails;
    }
    a.lang = uni::Lo::new("").map(|lo| uni::ascii(&lo.item(uni::S_LANGUAGEID))).filter(|l| l.len() == 2).unwrap_or_else(|| String::from("en"));
    let fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let (frame, client) = std_window(a.hab, b"CountryPalette", client_proc, CS_SIZEREDRAW, 0, fcf, TITLE, FRAME_ID);
    if frame == 0 {
        return 1;
    }
    a.frame = frame;
    a.client = client;
    set_menu(frame, &menu_items());
    a.icon = icon::globe();
    if a.icon != 0 {
        send(frame, 0x0047 /* WM_SETICON */, a.icon, 0);
    }
    a.cnr = unsafe { WinCreateWindow(client, wc(WC_CONTAINER), core::ptr::null(), WS_VISIBLE | CCS_SINGLESEL, 0, 0, 0, 0, client, HWND_TOP, CNR_ID, core::ptr::null(), core::ptr::null()) };
    // Icon view with an (empty) title and its separator, as OS/2's.
    let mut ci = [0u32; 23];
    ci[0] = 92;
    ci[4] = b"\0".as_ptr() as u32;
    ci[5] = CV_ICON | CA_CONTAINERTITLE | CA_TITLESEPARATOR;
    send(a.cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR | CMA_CNRTITLE);
    create_help();
    book::create();
    if !first_start() {
        error(frame, b"The system or user locales or both are missing");
    }
    fill();
    // Where the shell puts programs, 300 by 260 pels.
    let mut swp = Swp::default();
    unsafe {
        WinQueryTaskSizePos(a.hab, 0, &mut swp);
        WinSetWindowPos(frame, HWND_TOP, swp.x, swp.y, 300, 260, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, a.cnr);
    }
    message_loop(a.hab);
    book::close();
    uni::complete();
    unsafe {
        if let Some(h) = a.help.as_mut() {
            h.destroy(frame);
        }
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(a.hmq);
        WinTerminate(a.hab);
    }
    0
}

fn menu_items() -> Vec<Menu<'static>> {
    alloc::vec![
        Menu::Sub(IDM_LOCALE, b"~Locale", alloc::vec![
            Menu::Sub(IDM_OPENAS, b"~Open as", alloc::vec![Menu::Item(IDM_NEWLOCALE, b"~New locale"), Menu::Item(IDM_OPEN, b"~Locale")]),
            Menu::Item(IDM_EXIT, b"E~xit Country Palette"),
        ]),
        Menu::Sub(IDM_ACTION, b"~Action", alloc::vec![
            Menu::Item(IDM_JAVA, b"Create ~java locale"),
            Menu::Item(IDM_DELETE, b"~Delete..."),
            Menu::Item(IDM_DEFAULT, b"~Mark as default..."),
        ]),
        Menu::Sub(IDM_HELP, b"~Help", alloc::vec![
            Menu::Item(IDM_HELPINDEX, b"Help ~index"),
            Menu::Item(IDM_GENERALHELP, b"~General help"),
            Menu::Item(IDM_USINGHELP, b"~Using help"),
            Menu::Sep,
            Menu::Item(IDM_PRODUCT, b"~Product information"),
        ]),
    ]
}

fn popup_items() -> Vec<Menu<'static>> {
    alloc::vec![
        Menu::Sub(IDM_OPENAS, b"~Open as", alloc::vec![Menu::Item(IDM_NEWLOCALE, b"~New locale"), Menu::Item(IDM_OPEN, b"~Locale")]),
        Menu::Item(IDM_OPEN, b"Propertie~s"),
        Menu::Item(IDM_POPUPHELP, b"~Help"),
        Menu::Sep,
        Menu::Item(IDM_NEWLOCALE, b"~Create another"),
        Menu::Item(IDM_JAVA, b"Create ~java locale"),
        Menu::Item(IDM_DELETE, b"~Delete..."),
        Menu::Item(IDM_DEFAULT, b"~Mark as default..."),
    ]
}

fn create_help() {
    let a = app();
    let page = |id: u16, ext: u16| HelpWindow { id, ext, items: &[] };
    a.help = Help::create(
        a.hab,
        a.frame,
        HELP_FILE,
        b"Country Palette Help",
        &[
            HelpWindow {
                id: FRAME_ID as u16,
                ext: PANEL_PALETTE,
                items: &[
                    (IDM_LOCALE, PANEL_LOCALEMENU),
                    (IDM_OPENAS, PANEL_OPEN),
                    (IDM_OPEN, PANEL_OPEN),
                    (IDM_NEWLOCALE, PANEL_CREATE),
                    (IDM_EXIT, PANEL_LOCALEMENU),
                    (IDM_ACTION, PANEL_ACTIONMENU),
                    (IDM_JAVA, PANEL_JAVA),
                    (IDM_DELETE, PANEL_DELETE),
                    (IDM_DEFAULT, PANEL_MARKDEFAULT),
                    (IDM_HELP, 5019),
                    (IDM_HELPINDEX, 5019),
                    (IDM_GENERALHELP, 5019),
                    (IDM_USINGHELP, 5019),
                    (IDM_PRODUCT, PANEL_PRODUCT),
                ],
            },
            HelpWindow { id: DLG_NAME, ext: PANEL_NAME, items: &[(ID_NAME as u16, PANEL_NAME), (ID_OK as u16, PANEL_NAME), (ID_CANCEL as u16, PANEL_NAME), (ID_HELP as u16, PANEL_NAME)] },
            HelpWindow { id: DLG_PRODUCT, ext: PANEL_PRODUCT, items: &[] },
            page(book::DLG_BOOK, book::PANEL_COUNTRY),
            page(book::PG_COUNTRY, book::PANEL_COUNTRY),
            page(book::PG_COLLATE, book::PANEL_COLLATE),
            page(book::PG_NUMERIC, book::PANEL_NUMERIC),
            page(book::PG_MONETARY, book::PANEL_MONETARY),
            page(book::PG_DATE, book::PANEL_DATE),
            page(book::PG_LANGUAGE, book::PANEL_LANGUAGE),
            page(book::PG_TIME, book::PANEL_TIME),
            page(book::PG_ADVANCED, book::PANEL_ADVANCED),
        ],
    );
    if a.help.is_none() {
        trace(b"no help (GLOCALE.HLP not found)");
    }
}

/// Show a help panel (or say help is missing).
pub fn show_help(panel: u16) {
    let a = app();
    match a.help.as_deref() {
        Some(h) => h.show(panel),
        None => {
            message(a.frame, b"The help manager was not initialized", ERROR_TITLE, MB_OK | MB_INFORMATION);
        }
    }
}

fn help_request(msg: u32) {
    let a = app();
    pmkit::help::request(a.help.as_deref(), a.frame, HELP_FILE, TITLE, msg, 0, 0);
}

/// The process's locale (the first of its category names).
pub fn current_locale() -> String {
    uni::Lo::new("").map(|lo| lo.name()).filter(|n| !n.is_empty()).unwrap_or_else(|| String::from("en_US"))
}

/// OS/2's first start: the process's locale becomes a user locale when
/// it is not one yet.
fn first_start() -> bool {
    let Some(users) = uni::locale_list(2) else { return false };
    let cur = current_locale();
    if users.iter().any(|u| u.eq_ignore_ascii_case(&cur)) {
        return true;
    }
    let rc = uni::make_user(&cur, &cur, false);
    let rc2 = uni::complete();
    trace(alloc::format!("made user locale {cur} ({rc:#x}, {rc2:#x})").as_bytes());
    rc == 0 && rc2 == 0
}

/// Fill the container with the user locales.
pub fn fill() {
    let a = app();
    send(a.cnr, CM_REMOVERECORD, 0, mp(0, CMA_FREE | CMA_INVALIDATE));
    a.recs.clear();
    let users = uni::locale_list(2).unwrap_or_default();
    for u in users {
        add_record(&u);
    }
    send(a.cnr, CM_ARRANGE, 0, 0);
    send(a.cnr, CM_INVALIDATERECORD, 0, 0);
    let names: Vec<&str> = a.recs.iter().map(|r| r.name.as_str()).collect();
    trace(alloc::format!("palette: {}", names.join(" ")).as_bytes());
}

fn add_record(name: &str) {
    let a = app();
    let r = send(a.cnr, CM_ALLOCRECORD, 0, 1);
    if r == 0 {
        return;
    }
    let title = z(name.as_bytes());
    for off in [R_PSZICON, R_PSZTEXT, R_PSZNAME, R_PSZTREE] {
        wr(r, off, title.as_ptr() as u32);
    }
    wr(r, R_HPTR, a.icon);
    wr(r, R_HPTRMINI, a.icon);
    // RECORDINSERT: cb, pRecordOrder, pRecordParent, fInvalidateRecord,
    // zOrder, cRecordsInsert.
    let ri: [u32; 6] = [24, CMA_END, 0, 1, 1, 1];
    send(a.cnr, CM_INSERTRECORD, r, ri.as_ptr() as u32);
    a.recs.push(Rec { rec: r, name: name.into(), _title: title });
}

/// Select a locale's record.
fn select(name: &str) {
    let a = app();
    for r in &a.recs {
        let on = r.name.eq_ignore_ascii_case(name);
        send(a.cnr, CM_SETRECORDEMPHASIS, r.rec, mp(on as u32, if on { CRA_SELECTED | CRA_CURSORED } else { CRA_SELECTED }));
    }
}

/// The locale a command is for: the pop-up menu's record, else the
/// selected one.
fn target() -> Option<String> {
    let a = app();
    if let Some(t) = a.target.take() {
        return Some(t);
    }
    let r = send(a.cnr, CM_QUERYRECORDEMPHASIS, CMA_FIRST, CRA_SELECTED);
    if r == 0 || r == 0xffff_ffff {
        return a.recs.first().map(|r| r.name.clone());
    }
    a.recs.iter().find(|x| x.rec == r).map(|x| x.name.clone())
}

fn name_of(rec: u32) -> Option<String> {
    app().recs.iter().find(|x| x.rec == rec).map(|x| x.name.clone())
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match msg {
        WM_SIZE => {
            unsafe { WinSetWindowPos(a.cnr, 0, 0, 0, lo(mp2) as i32, hi(mp2) as i32, SWP_SIZE | SWP_MOVE) };
            // The icons are laid out for the window's width.
            send(a.cnr, CM_ARRANGE, 0, 0);
            0
        }
        WM_ERASEBACKGROUND => 1,
        WM_CONTROL if lo(mp1) == CNR_ID => {
            match hi(mp1) {
                CN_ENTER => {
                    // NOTIFYRECORDENTER: hwndCnr, fKey, pRecord.
                    let rec = if mp2 != 0 { rd(mp2, 8) } else { 0 };
                    if let Some(n) = name_of(rec) {
                        book::open(&n, false);
                    }
                }
                CN_CONTEXTMENU => {
                    if let Some(n) = name_of(mp2) {
                        select(&n);
                        a.target = Some(n);
                        let menu = popup_menu(hwnd, &popup_items());
                        let mut p = PointL::default();
                        unsafe {
                            WinQueryPointerPos(HWND_DESKTOP, &mut p);
                            WinPopupMenu(HWND_DESKTOP, hwnd, menu, p.x, p.y, IDM_OPEN as i32, 0x0246);
                        }
                    }
                }
                CN_HELP => show_help(PANEL_PALETTE),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            command(lo(mp1) as u16);
            0
        }
        WM_MENUEND => {
            // The pop-up's command (if any) has come first.
            0
        }
        WM_HELP => {
            show_help(PANEL_PALETTE);
            0
        }
        HM_ERROR => {
            trace(alloc::format!("help error {:#x}", mp1).as_bytes());
            0
        }
        HM_QUERY_KEYS_HELP => 0,
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}

fn command(id: u16) {
    let a = app();
    match id {
        IDM_OPEN => {
            if let Some(n) = target() {
                book::open(&n, false);
            }
        }
        IDM_NEWLOCALE => {
            if let Some(n) = target() {
                create_another(&n);
            }
        }
        IDM_DELETE => {
            if let Some(n) = target() {
                delete(&n);
            }
        }
        IDM_DEFAULT => {
            if let Some(n) = target() {
                mark_default(&n);
            }
        }
        IDM_JAVA => {
            a.target = None;
            error(a.frame, b"The Java locale was not created");
        }
        IDM_POPUPHELP => {
            a.target = None;
            show_help(PANEL_PALETTE);
        }
        IDM_EXIT => {
            post(a.frame, WM_CLOSE, 0, 0);
        }
        IDM_HELPINDEX => help_request(HM_HELP_INDEX),
        IDM_GENERALHELP => help_request(HM_EXT_HELP),
        IDM_USINGHELP => help_request(HM_DISPLAY_HELP),
        IDM_PRODUCT => product_info(a.frame, DLG_PRODUCT, b"Country Palette - Product information", b"Country Palette", &[b"rs2 (OS/2 Warp 4.52 compatible)", b"Shows and changes the locales: country, numbers,", b"money, dates, times and collation."]),
        _ => {}
    }
}

// ---- Create another ----

static mut BASE_OF_NEW: String = String::new();

extern "C" fn name_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            dlg_msg(h, ID_NAME, EM_SETTEXTLIMIT, 31, 0);
            unsafe { WinSetFocus(HWND_DESKTOP, item(h, ID_NAME)) };
            1
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_OK => {
                    let name = item_text(h, ID_NAME);
                    let name: Vec<u8> = name.iter().copied().filter(|&c| c != b' ').collect();
                    if name.is_empty() {
                        return 0;
                    }
                    let base = unsafe { (*core::ptr::addr_of!(BASE_OF_NEW)).clone() };
                    let name: String = name.iter().map(|&c| c as char).collect();
                    if make_new(h, &name, &base) {
                        unsafe { WinDismissDlg(h, DID_OK) };
                    }
                }
                ID_CANCEL | DID_CANCEL => unsafe {
                    WinDismissDlg(h, DID_CANCEL);
                },
                _ => {}
            }
            0
        }
        WM_HELP => {
            show_help(PANEL_NAME);
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Make a user locale `name` on the base of `of` (OS/2's checks: a name
/// already there, a name that is not valid; a system locale's name makes
/// an override of that system locale).
pub fn make_new(owner: u32, name: &str, of: &str) -> bool {
    let a = app();
    if a.recs.iter().any(|r| r.name.eq_ignore_ascii_case(name)) {
        error(owner, b"The locale name already exists");
        return false;
    }
    let base = uni::Lo::new(of).map(|lo| uni::ascii(&lo.item(uni::S_NAME))).unwrap_or_else(|| String::from(of));
    let mut rc = uni::make_user(name, &base, false);
    if rc == uni::ULS_BADOBJECT {
        // The name of a system locale: an override of it.
        rc = uni::make_user(name, name, false);
    }
    if rc == uni::ULS_INVALID || rc == uni::ULS_NOMATCH {
        error(owner, b"The locale name is not valid");
        return false;
    }
    if rc != 0 && rc != 0x20405 {
        error(owner, b"The Locale database could not be updated");
        return false;
    }
    if uni::complete() != 0 {
        error(owner, b"The Locale database could not be updated");
        return false;
    }
    trace(alloc::format!("created {name} on {base}").as_bytes());
    if !a.recs.iter().any(|r| r.name.eq_ignore_ascii_case(name)) {
        add_record(name);
        send(a.cnr, CM_ARRANGE, 0, 0);
        send(a.cnr, CM_INVALIDATERECORD, 0, 0);
    }
    select(name);
    unsafe { MADE = String::from(name) };
    true
}

/// The locale Create another made (its notebook opens once the dialog
/// is gone).
static mut MADE: String = String::new();

fn create_another(of: &str) {
    unsafe { BASE_OF_NEW = String::from(of) };
    let tpl = template(
        dialog_frame(b"Locale Name", DLG_NAME, 148, 52),
        alloc::vec![
            label(b"Enter Locale Name", (2, 40, 141, 8), DT_LEFT),
            entry((4, 25, 139, 9), ID_NAME, 0),
            button(b"OK", (2, 4, 40), ID_OK, BS_DEFAULT),
            button(b"Cancel", (53, 4, 40), ID_CANCEL, 0),
            help_button((105, 4, 40), ID_HELP),
        ],
    );
    unsafe { MADE = String::new() };
    if run_dialog(app().frame, name_proc, &tpl, 0) == DID_OK {
        let made = unsafe { (*core::ptr::addr_of!(MADE)).clone() };
        if !made.is_empty() {
            book::open(&made, true);
        }
    }
}

// ---- Delete ----

fn delete(name: &str) {
    let a = app();
    book::closing_for(name);
    if uni::complete() != 0 || uni::delete_user(name) != 0 || uni::complete() != 0 {
        error(a.frame, b"The Locale database could not be updated");
        return;
    }
    trace(alloc::format!("deleted {name}").as_bytes());
    if let Some(i) = a.recs.iter().position(|r| r.name == name) {
        let r = a.recs.remove(i);
        let one = [r.rec];
        send(a.cnr, CM_REMOVERECORD, one.as_ptr() as u32, mp(1, CMA_FREE | CMA_INVALIDATE));
        send(a.cnr, CM_ARRANGE, 0, 0);
    }
    if a.recs.is_empty() {
        // None left: the system's locale is made a user locale again.
        message(a.frame, b"Restoring system default locale", TITLE, MB_OK | MB_INFORMATION);
        first_start();
        fill();
    }
}

// ---- Mark as default ----

/// OS2.INI's national settings from a locale (the keys OS/2's
/// PM_National holds).
fn national(lo: &uni::Lo) -> Vec<(&'static [u8], Vec<u8>)> {
    let n = |v: i32| alloc::format!("{}", v.max(0)).into_bytes();
    alloc::vec![
        (&b"iCountry"[..], n(lo.value(uni::I_COUNTRY))),
        (b"iMeasurement", n(if lo.value(uni::I_MEASURE) == 1 { 2 } else { 1 })),
        (b"iDate", n(lo.value(uni::I_DATE))),
        (b"iCurrency", n(lo.value(uni::I_CURRENCY))),
        (b"iDigits", n(lo.value(uni::I_DIGITS))),
        (b"iTime", n(lo.value(uni::I_TIME))),
        (b"iLzero", n(lo.value(uni::I_LZERO))),
        (b"s1159", lo.text(uni::S_1159)),
        (b"s2359", lo.text(uni::S_2359)),
        (b"sCurrency", lo.text(uni::S_CURRENCY)),
        (b"sThousand", lo.text(uni::S_THOUSAND)),
        (b"sDecimal", lo.text(uni::S_DECIMAL)),
        (b"sDate", lo.text(uni::S_DATE)),
        (b"sTime", lo.text(uni::S_TIME)),
        (b"sList", lo.text(uni::S_LIST)),
    ]
}

pub fn mark_default(name: &str) -> bool {
    let a = app();
    uni::complete();
    let Some(lo) = uni::Lo::new(name) else {
        error(a.frame, b"The selected locale object could not be marked as the default");
        return false;
    };
    let vals = national(&lo);
    let mut ok = true;
    for app_name in [&b"PM_National"[..], b"PM_Default_National"] {
        for (k, v) in &vals {
            let (ap, kk, vv) = (z(app_name), z(k), z(v));
            ok &= unsafe { PrfWriteProfileString(HINI_USERPROFILE, ap.as_ptr(), kk.as_ptr(), vv.as_ptr()) } != 0;
        }
    }
    if !ok {
        error(a.frame, b"The ini file could not be updated");
        return false;
    }
    // \LANGUAGE\LOCALE\DEFAULT.LOC: the locale's name and the conventions
    // DOS and WIN-OS/2 programs take, as OS/2's writes it.
    let mut text = Vec::new();
    let line = |t: &mut Vec<u8>, k: &str, v: &[u8]| {
        let mut key = k.as_bytes().to_vec();
        while key.len() < 15 {
            key.push(b' ');
        }
        t.extend_from_slice(&key);
        t.extend_from_slice(b"= \"");
        t.extend_from_slice(v);
        t.extend_from_slice(b"\"\r\n");
    };
    text.extend_from_slice(b"[locale ");
    text.extend_from_slice(name.as_bytes());
    text.extend_from_slice(b"]\r\n");
    let get = |k: &[u8]| vals.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    for k in ["iCountry", "sCurrency", "sThousand", "sDecimal", "sDate", "sTime", "iCurrency", "iDigits", "iTime", "sList"] {
        line(&mut text, k, &get(k.as_bytes()));
    }
    let dir = alloc::format!("{}:\\LANGUAGE\\LOCALE", boot_drive() as char);
    let d1 = alloc::format!("{}:\\LANGUAGE\0", boot_drive() as char);
    let d2 = alloc::format!("{dir}\0");
    unsafe {
        os2::DosCreateDir(d1.as_ptr(), core::ptr::null_mut());
        os2::DosCreateDir(d2.as_ptr(), core::ptr::null_mut());
    }
    let path = alloc::format!("{dir}\\DEFAULT.LOC");
    if write_file(path.as_bytes(), &text).is_err() {
        error(a.frame, b"The selected locale object could not be marked as the default");
        return false;
    }
    win_ini(&lo);
    trace(alloc::format!("default {name}").as_bytes());
    true
}

/// WIN-OS/2's WIN.INI [intl], when WIN-OS/2 is installed (OS2.INI
/// PM_INSTALL WINOS2_LOCATION).
fn win_ini(lo: &uni::Lo) {
    let mut buf = [0u8; 260];
    let n = unsafe { PrfQueryProfileString(HINI_USERPROFILE, b"PM_INSTALL\0".as_ptr(), b"WINOS2_LOCATION\0".as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let loc = until_nul(&buf[..n.min(260)]).to_vec();
    if loc.is_empty() {
        return;
    }
    let mut path = loc.clone();
    if path.last() != Some(&b'\\') {
        path.push(b'\\');
    }
    path.extend_from_slice(b"WIN.INI");
    let Some(old) = read_file(&path) else { return };
    let n = |v: i32| alloc::format!("{}", v.max(0)).into_bytes();
    let keys: Vec<(&[u8], Vec<u8>)> = alloc::vec![
        (&b"sLanguage"[..], uni::to_cp(&lo.item(88)).to_ascii_lowercase()),
        (b"sCountry", lo.text(105)),
        (b"iCountry", n(lo.value(uni::I_COUNTRY))),
        (b"iDate", n(lo.value(uni::I_DATE))),
        (b"iTime", n(lo.value(uni::I_TIME))),
        (b"iTLZero", n(lo.value(uni::I_TLZERO))),
        (b"iCurrency", n(lo.value(uni::I_CURRENCY))),
        (b"iCurrDigits", n(lo.value(uni::I_CURRDIGITS))),
        (b"iNegCurr", n(lo.value(uni::I_NEGCURR))),
        (b"iLzero", n(lo.value(uni::I_LZERO))),
        (b"iDigits", n(lo.value(uni::I_DIGITS))),
        (b"iMeasure", n(lo.value(uni::I_MEASURE))),
        (b"s1159", lo.text(uni::S_1159)),
        (b"s2359", lo.text(uni::S_2359)),
        (b"sCurrency", lo.text(uni::S_CURRENCY)),
        (b"sThousand", lo.text(uni::S_THOUSAND)),
        (b"sDecimal", lo.text(uni::S_DECIMAL)),
        (b"sDate", lo.text(uni::S_DATE)),
        (b"sTime", lo.text(uni::S_TIME)),
        (b"sList", lo.text(uni::S_LIST)),
        (b"sShortDate", lo.text(uni::S_SHORTDATE)),
        (b"sLongDate", lo.text(96)),
    ];
    // Drop the old [intl] section; write the new one in its place (or
    // at the end).
    let mut out = Vec::new();
    let mut skipping = false;
    let mut written = false;
    let section = |out: &mut Vec<u8>| {
        out.extend_from_slice(b"[intl]\r\n");
        for (k, v) in &keys {
            out.extend_from_slice(k);
            out.push(b'=');
            out.extend_from_slice(v);
            out.extend_from_slice(b"\r\n");
        }
    };
    for l in old.split(|&c| c == b'\n') {
        let t: Vec<u8> = l.iter().copied().filter(|&c| c != b'\r').collect();
        let tt = t.trim_ascii();
        if tt.starts_with(b"[") {
            skipping = tt.eq_ignore_ascii_case(b"[intl]");
            if skipping && !written {
                section(&mut out);
                written = true;
            }
            if skipping {
                continue;
            }
        }
        if skipping || (tt.is_empty() && l.is_empty()) {
            continue;
        }
        out.extend_from_slice(&t);
        out.extend_from_slice(b"\r\n");
    }
    if !written {
        section(&mut out);
    }
    let _ = write_file(&path, &out);
}
