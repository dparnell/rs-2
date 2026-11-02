//! A locale's notebook, as OS/2's Locale program shows it: one window
//! (made once, hidden between uses) with a tabbed notebook of eight
//! pages: Country/Region, Collate, Numeric, Monetary, Date, Time,
//! Language and Advanced. Undo, Default and Help are the notebook's
//! buttons. Each change on a page is made at once
//! (UniSetUserLocaleItem) and the page is filled again from the locale,
//! so the samples follow; closing the notebook writes USER.LOC.
//! Undo puts back the values the locale had when the notebook was
//! opened, Default those of its base locale.

use alloc::string::String;
use alloc::vec::Vec;

use pmkit::*;

use crate::fmt;
use crate::uni::{self, Lo};
use crate::{app, error, trace};

pub const DLG_BOOK: u16 = 101;
pub const ID_NOTEBOOK: u32 = 103;

// Pages (OS/2's dialog ids).
pub const PG_COLLATE: u16 = 104;
pub const PG_COUNTRY: u16 = 105;
pub const PG_LANGUAGE: u16 = 107;
pub const PG_MONETARY: u16 = 108;
pub const PG_NUMERIC: u16 = 109;
pub const PG_DATE: u16 = 110;
pub const PG_TIME: u16 = 111;
pub const PG_ADVANCED: u16 = 113;

// Their help panels.
pub const PANEL_COUNTRY: u16 = 5030;
pub const PANEL_COLLATE: u16 = 5040;
pub const PANEL_NUMERIC: u16 = 5050;
pub const PANEL_MONETARY: u16 = 5060;
pub const PANEL_DATE: u16 = 5070;
pub const PANEL_LANGUAGE: u16 = 5080;
pub const PANEL_TIME: u16 = 5090;
pub const PANEL_ADVANCED: u16 = 5100;

// Controls (OS/2's ids).
const ID_TIMESEP: u32 = 201;
const ID_LOCALENAME: u32 = 202;
const ID_AM: u32 = 203;
const ID_PM: u32 = 204;
const ID_THOUSAND: u32 = 205;
const ID_CURRENCY: u32 = 206;
const ID_DATESEP: u32 = 207;
const ID_DIGITS: u32 = 208;
const ID_DECIMAL: u32 = 209;
const ID_LIST: u32 = 210;
const ID_MONDECIMAL: u32 = 211;
const ID_MONTHOUSAND: u32 = 212;
const ID_NEGSIGN: u32 = 213;
const ID_POSSIGN: u32 = 214;
const ID_TIMEFMT: u32 = 215;
const ID_DATEFMT: u32 = 216;
const ID_VALUE: u32 = 217;
const ID_COLLATE: u32 = 300;
const ID_BASE: u32 = 301;
const ID_MEASURE: u32 = 303;
const ID_LANGUAGE: u32 = 304;
const ID_NEGNUMBER: u32 = 305;
const ID_CURRPOS: u32 = 306;
const ID_NEGCURR: u32 = 307;
const ID_12H: u32 = 403;
const ID_24H: u32 = 404;
const ID_DMY: u32 = 405;
const ID_MDY: u32 = 406;
const ID_YDM: u32 = 410;
const ID_YMD: u32 = 411;
pub const ID_UNDO: u32 = 417;
pub const ID_DEFAULT: u32 = 418;
const ID_HELPBTN: u32 = 419;
const ID_ITEMS: u32 = 601;
const ID_LZERO: u32 = 800;
const ID_COUNTRYNAME: u32 = 1030;
const ID_LANGNAME: u32 = 1031;
const ID_DATESAMPLE: u32 = 1033;
const ID_TIMESAMPLE: u32 = 1034;

const BKM_INSERTPAGE: u32 = 0x0355;
const BKM_TURNTOPAGE: u32 = 0x0357;
const BKM_QUERYPAGEID: u32 = 0x0359;
const BKM_SETPAGEWINDOWHWND: u32 = 0x0360;
const BKM_SETSTATUSLINETEXT: u32 = 0x0361;
const BKM_SETTABTEXT: u32 = 0x0363;
const BKM_SETNOTEBOOKBUTTONS: u32 = 0x0375;
const BKN_HELP: u32 = 132;
const BKA_TOP: u32 = 0x0020;
const BKA_FIRST: u32 = 0x0004;
const BKA_LAST: u32 = 0x0002;
const BKA_MAJOR: u32 = 0x0040;
const BKA_STATUSTEXTON: u32 = 0x0001;
const BKA_AUTOPAGESIZE: u32 = 0x0100;
const BKS_BACKPAGESBR: u32 = 0x0001;
const BKS_MAJORTABTOP: u32 = 0x0040;
const BKS_BUTTONAREA: u32 = 0x0200;
const BKS_TABBEDDIALOG: u32 = 0x0800;
const BKS_TABTEXTCENTER: u32 = 0x8000;
const CBN_LBSELECT: u32 = 4;
const WM_WINDOWPOSCHANGED: u32 = 0x0055;
const EM_SETREADONLY: u32 = 0x014b;

/// The collations the Collate page offers (their sCollate codes).
const COLLATIONS: [(&[u8], &str); 21] = [
    (b"Unicode", ""),
    (b"C", "c"),
    (b"Extended", "ext"),
    (b"Danish", "da"),
    (b"Chinese", "cn"),
    (b"Czech", "cs"),
    (b"Finnish", "fi"),
    (b"French", "fr"),
    (b"Icelandic", "is"),
    (b"Japanese", "ja"),
    (b"Korean", "ko"),
    (b"Latvian", "lv"),
    (b"Lithuanian", "lt"),
    (b"Norwegian", "no"),
    (b"Polish", "pl"),
    (b"Spanish", "es"),
    (b"Spanish (traditional)", "estrad"),
    (b"Swedish", "sv"),
    (b"Taiwanese", "tw"),
    (b"Turkish", "tr"),
    (b"User defined", "!!"),
];

/// The languages of the Language page (sLanguageID codes).
const LANGUAGES: [(&str, &[u8]); 38] = [
    ("sq", b"Albanian"),
    ("ar", b"Arabic"),
    ("eu", b"Basque"),
    ("be", b"Belarusian"),
    ("bg", b"Bulgarian"),
    ("ca", b"Catalan"),
    ("zh", b"Chinese"),
    ("hr", b"Croatian"),
    ("cs", b"Czech"),
    ("da", b"Danish"),
    ("nl", b"Dutch"),
    ("en", b"English"),
    ("et", b"Estonian"),
    ("fi", b"Finnish"),
    ("fr", b"French"),
    ("de", b"German"),
    ("el", b"Greek"),
    ("he", b"Hebrew"),
    ("hu", b"Hungarian"),
    ("is", b"Icelandic"),
    ("in", b"Indonesian"),
    ("it", b"Italian"),
    ("ja", b"Japanese"),
    ("ko", b"Korean"),
    ("lv", b"Latvian"),
    ("lt", b"Lithuanian"),
    ("no", b"Norwegian"),
    ("pl", b"Polish"),
    ("pt", b"Portuguese"),
    ("ru", b"Russian"),
    ("sr", b"Serbian"),
    ("sk", b"Slovak"),
    ("sl", b"Slovenian"),
    ("es", b"Spanish"),
    ("sv", b"Swedish"),
    ("th", b"Thai"),
    ("tr", b"Turkish"),
    ("uk", b"Ukrainian"),
];

/// The pages in the notebook's order: (dialog id, tab, status line, help
/// panel).
const PAGES: [(u16, &[u8], &[u8], u16); 8] = [
    (PG_COUNTRY, b"Country/Region", b"Country or region", PANEL_COUNTRY),
    (PG_COLLATE, b"Collate", b"How text is sorted", PANEL_COLLATE),
    (PG_NUMERIC, b"Numeric", b"How numbers are written", PANEL_NUMERIC),
    (PG_MONETARY, b"Monetary", b"How amounts of money are written", PANEL_MONETARY),
    (PG_DATE, b"Date", b"How dates are written", PANEL_DATE),
    (PG_TIME, b"Time", b"How times are written", PANEL_TIME),
    (PG_LANGUAGE, b"Language", b"The language of programs", PANEL_LANGUAGE),
    (PG_ADVANCED, b"Advanced", b"Every item of the locale", PANEL_ADVANCED),
];

struct Book {
    frame: u32,
    nb: u32,
    pages: [u32; 8],
    page_ids: [u32; 8],
    /// The locale shown.
    cur: String,
    /// Its items when the notebook was opened (for Undo).
    saved: Vec<Vec<u16>>,
    /// Filling the controls: their notifications are not changes.
    filling: bool,
    systems: Vec<String>,
    /// The Advanced page's item.
    adv_item: i32,
}

static mut BOOK: Book = Book { frame: 0, nb: 0, pages: [0; 8], page_ids: [0; 8], cur: String::new(), saved: Vec::new(), filling: false, systems: Vec::new(), adv_item: 1 };

fn book() -> &'static mut Book {
    unsafe { &mut *core::ptr::addr_of_mut!(BOOK) }
}

/// The page window of a dialog id.
fn page(id: u16) -> u32 {
    let b = book();
    PAGES.iter().position(|p| p.0 == id).map_or(0, |i| b.pages[i])
}

/// The dialog id of a page window.
fn page_kind(h: u32) -> u16 {
    let b = book();
    b.pages.iter().position(|&p| p == h).map_or(0, |i| PAGES[i].0)
}

/// A page's frame: no border or title (the notebook shows it).
fn page_frame(id: u16) -> Item {
    pmkit::tpl::item(WC_FRAME, b"", WS_CLIPSIBLINGS | FS_NOBYTEALIGN, (0, 0, 187, 150), id)
}

fn small_entry(r: (i16, i16, i16, i16), id: u32) -> Item {
    entry(r, id, 0)
}

fn page_template(id: u16) -> Vec<u8> {
    let v: Vec<Item> = match id {
        PG_COUNTRY => alloc::vec![
            label(b"Locale name", (3, 129, 165, 10), DT_LEFT),
            small_entry((4, 115, 165, 10), ID_LOCALENAME),
            label(b"Base locale", (3, 99, 165, 10), DT_LEFT),
            text_id(b"", (3, 88, 170, 10), ID_LANGNAME, DT_LEFT),
            text_id(b"", (3, 77, 170, 10), ID_COUNTRYNAME, DT_LEFT),
            drop_list((3, 2, 167, 74), ID_BASE),
            label(b"Measurement", (3, 44, 165, 10), DT_LEFT),
            drop_list((3, 9, 167, 34), ID_MEASURE),
            label(b"List separator", (3, 19, 165, 10), DT_LEFT),
            small_entry((4, 6, 20, 10), ID_LIST),
        ],
        PG_COLLATE => alloc::vec![label(b"Sorting type", (3, 78, 165, 10), DT_LEFT), drop_list((3, 7, 167, 70), ID_COLLATE)],
        PG_LANGUAGE => alloc::vec![label(b"Language", (3, 78, 165, 10), DT_LEFT), drop_list((3, 7, 167, 70), ID_LANGUAGE)],
        PG_NUMERIC => alloc::vec![
            group_box(b"Number format", (6, 62, 172, 79)),
            label(b"Decimal symbol", (10, 119, 90, 10), DT_LEFT),
            small_entry((105, 121, 20, 10), ID_DECIMAL),
            label(b"Digit grouping symbol", (10, 100, 90, 10), DT_LEFT),
            small_entry((105, 102, 20, 10), ID_THOUSAND),
            label(b"Positive sign", (10, 80, 90, 10), DT_LEFT),
            small_entry((105, 82, 20, 10), ID_POSSIGN),
            label(b"Decimal digits", (10, 65, 90, 10), DT_LEFT),
            spin((104, 65, 30, 12), ID_DIGITS),
            group_box(b"Negative numbers", (6, 7, 172, 49)),
            label(b"Negative sign", (10, 34, 90, 10), DT_LEFT),
            small_entry((105, 36, 20, 10), ID_NEGSIGN),
            label(b"Format", (10, 15, 90, 10), DT_LEFT),
            drop_list((104, -13, 70, 40), ID_NEGNUMBER),
        ],
        PG_MONETARY => alloc::vec![
            group_box(b"Currency format", (5, 43, 172, 99)),
            label(b"Symbol", (8, 116, 90, 10), DT_LEFT),
            small_entry((105, 118, 65, 10), ID_CURRENCY),
            label(b"Symbol placement", (8, 100, 90, 10), DT_LEFT),
            drop_list((104, 72, 70, 40), ID_CURRPOS),
            label(b"Decimal symbol", (8, 84, 90, 10), DT_LEFT),
            small_entry((105, 86, 20, 10), ID_MONDECIMAL),
            label(b"Decimal digits", (8, 70, 90, 10), DT_LEFT),
            spin((104, 70, 30, 12), ID_DIGITS),
            label(b"Digit grouping symbol", (8, 56, 90, 10), DT_LEFT),
            small_entry((105, 56, 20, 10), ID_MONTHOUSAND),
            check_box(b"Leading zero", (8, 45, 90, 10), ID_LZERO),
            group_box(b"Negative amounts", (5, 6, 172, 32)),
            label(b"Format", (8, 14, 90, 10), DT_LEFT),
            drop_list((104, -11, 70, 37), ID_NEGCURR),
        ],
        PG_DATE => alloc::vec![
            group_box(b"Date order", (6, 7, 173, 130)),
            radio(b"Month-Day-Year", (9, 118, 165, 10), ID_MDY).group(),
            radio(b"Day-Month-Year", (9, 106, 165, 10), ID_DMY),
            radio(b"Year-Month-Day", (9, 94, 165, 10), ID_YMD),
            radio(b"Year-Day-Month", (9, 82, 165, 10), ID_YDM),
            label(b"Separator", (10, 69, 165, 10), DT_LEFT),
            small_entry((11, 56, 20, 10), ID_DATESEP).group(),
            label(b"Format string", (10, 42, 165, 10), DT_LEFT),
            small_entry((11, 28, 164, 10), ID_DATEFMT),
            text_id(b"", (11, 9, 165, 10), ID_DATESAMPLE, DT_LEFT),
        ],
        PG_TIME => alloc::vec![
            group_box(b"Time format", (6, 7, 173, 130)),
            radio(b"12 hours", (9, 118, 165, 10), ID_12H).group(),
            radio(b"24 hours", (9, 107, 165, 10), ID_24H),
            label(b"AM string", (9, 95, 90, 10), DT_LEFT),
            small_entry((104, 97, 45, 10), ID_AM).group(),
            label(b"PM string", (9, 80, 90, 10), DT_LEFT),
            small_entry((104, 81, 45, 10), ID_PM),
            label(b"Separator", (9, 69, 165, 10), DT_LEFT),
            small_entry((10, 56, 20, 10), ID_TIMESEP),
            label(b"Format string", (9, 42, 165, 10), DT_LEFT),
            small_entry((10, 28, 164, 10), ID_TIMEFMT),
            text_id(b"", (10, 9, 165, 10), ID_TIMESAMPLE, DT_LEFT),
        ],
        _ => alloc::vec![
            label(b"Item", (6, 130, 165, 10), DT_LEFT),
            list_box((6, 63, 167, 65), ID_ITEMS, 0),
            label(b"Value", (6, 42, 165, 10), DT_LEFT),
            small_entry((7, 25, 165, 12), ID_VALUE),
        ],
    };
    template(page_frame(id), v)
}

/// Make the notebook window (hidden) and its pages.
pub fn create() {
    let a = app();
    let b = book();
    // A sizeable frame with a title and the notebook (OS/2's dialog 101).
    let mut f = pmkit::tpl::item(WC_FRAME, b"Locale Configuration", WS_CLIPSIBLINGS | WS_SAVEBITS | FS_NOBYTEALIGN, (40, 20, 320, 221), DLG_BOOK);
    f.ctl = (FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX).to_le_bytes().to_vec();
    let nb = pmkit::tpl::item(WC_NOTEBOOK, b"", WS_VISIBLE | WS_TABSTOP | BKS_TABBEDDIALOG | BKS_BUTTONAREA | BKS_MAJORTABTOP | BKS_BACKPAGESBR | BKS_TABTEXTCENTER, (2, 2, 315, 207), ID_NOTEBOOK as u16);
    let tpl = template(f, alloc::vec![nb]);
    b.frame = unsafe { WinCreateDlg(HWND_DESKTOP, a.frame, book_proc as *const () as u32, tpl.as_ptr(), 0) };
    if b.frame == 0 {
        trace(b"notebook not made");
        return;
    }
    b.nb = item(b.frame, ID_NOTEBOOK);
    b.systems = uni::locale_list(1).unwrap_or_default();
    for (i, &(id, tab, status, _)) in PAGES.iter().enumerate() {
        let t = page_template(id);
        let w = unsafe { WinCreateDlg(b.nb, b.nb, page_proc as *const () as u32, t.as_ptr(), 0) };
        b.pages[i] = w;
        let pid = send(b.nb, BKM_INSERTPAGE, 0, mp(BKA_MAJOR | BKA_STATUSTEXTON | BKA_AUTOPAGESIZE, BKA_LAST));
        b.page_ids[i] = pid;
        send(b.nb, BKM_SETPAGEWINDOWHWND, pid, w);
        let (tt, ss): (&'static [u8], &'static [u8]) = (alloc::boxed::Box::leak(z(tab).into_boxed_slice()), alloc::boxed::Box::leak(z(status).into_boxed_slice()));
        send(b.nb, BKM_SETTABTEXT, pid, tt.as_ptr() as u32);
        send(b.nb, BKM_SETSTATUSLINETEXT, pid, ss.as_ptr() as u32);
    }
    // NOTEBOOKBUTTON: pszText, idButton, hImage, flStyle.
    static UNDO: &[u8] = b"~Undo\0";
    static DEFAULT: &[u8] = b"~Default\0";
    static HELP: &[u8] = b"Help\0";
    let buttons: [u32; 12] = [UNDO.as_ptr() as u32, ID_UNDO, 0, 0, DEFAULT.as_ptr() as u32, ID_DEFAULT, 0, 0, HELP.as_ptr() as u32, ID_HELPBTN, 0, BS_HELP];
    send(b.nb, BKM_SETNOTEBOOKBUTTONS, 3, buttons.as_ptr() as u32);
    init_lists();
    let tp = page(PG_TIME);
    unsafe { WinStartTimer(a.hab, tp, 1, 1000) };
}

/// The lists that do not depend on the locale.
fn init_lists() {
    let b = book();
    let p = page(PG_COUNTRY);
    for s in &b.systems {
        lb_add(p, ID_BASE, s.as_bytes());
    }
    lb_add(p, ID_MEASURE, b"Metric");
    lb_add(p, ID_MEASURE, b"U.S.");
    dlg_msg(p, ID_LOCALENAME, EM_SETREADONLY, 1, 0);
    enable(p, ID_LOCALENAME, false);
    dlg_msg(p, ID_LIST, EM_SETTEXTLIMIT, 3, 0);
    let p = page(PG_COLLATE);
    for (n, _) in COLLATIONS {
        lb_add(p, ID_COLLATE, n);
    }
    let p = page(PG_LANGUAGE);
    for (_, n) in LANGUAGES {
        lb_add(p, ID_LANGUAGE, n);
    }
    lb_add(p, ID_LANGUAGE, b"User defined");
    for (pg, ids) in [(PG_NUMERIC, &[ID_DECIMAL, ID_THOUSAND, ID_POSSIGN, ID_NEGSIGN][..]), (PG_MONETARY, &[ID_MONDECIMAL, ID_MONTHOUSAND][..])] {
        let p = page(pg);
        for &id in ids {
            dlg_msg(p, id, EM_SETTEXTLIMIT, 3, 0);
        }
        dlg_msg(p, ID_DIGITS, SPBM_SETLIMITS, 9, 0);
    }
    dlg_msg(page(PG_MONETARY), ID_CURRENCY, EM_SETTEXTLIMIT, 15, 0);
    let p = page(PG_ADVANCED);
    for (i, n) in uni::ITEM_NAMES.iter().enumerate() {
        let _ = i;
        lb_add(p, ID_ITEMS, n.as_bytes());
    }
    let p = page(PG_TIME);
    for id in [ID_AM, ID_PM, ID_TIMEFMT] {
        dlg_msg(p, id, EM_SETTEXTLIMIT, 63, 0);
    }
    dlg_msg(p, ID_TIMESEP, EM_SETTEXTLIMIT, 3, 0);
    dlg_msg(page(PG_DATE), ID_DATESEP, EM_SETTEXTLIMIT, 3, 0);
    dlg_msg(page(PG_DATE), ID_DATEFMT, EM_SETTEXTLIMIT, 63, 0);
    dlg_msg(page(PG_ADVANCED), ID_VALUE, EM_SETTEXTLIMIT, 255, 0);
}

/// Open the notebook on a locale (`first`: on its first page).
pub fn open(name: &str, first: bool) {
    let a = app();
    let b = book();
    if b.frame == 0 {
        error(a.frame, b"The Country Palette notebook could not be opened");
        return;
    }
    if !b.cur.is_empty() && !b.cur.eq_ignore_ascii_case(name) {
        uni::complete();
    }
    let Some(lo) = Lo::new(name) else {
        error(a.frame, b"The selected locale object could not be opened for editing");
        return;
    };
    b.cur = String::from(name);
    b.saved = (1..=uni::MAXITEM).map(|i| lo.item(i)).collect();
    drop(lo);
    fill_all(0);
    let mut t = b"Locale Configuration - ".to_vec();
    t.extend_from_slice(name.as_bytes());
    set_text(b.frame, &t);
    if first {
        let first_id = send(b.nb, BKM_QUERYPAGEID, 0, BKA_FIRST);
        send(b.nb, BKM_TURNTOPAGE, first_id, 0);
    }
    trace(alloc::format!("notebook {name}").as_bytes());
    unsafe {
        WinSetWindowPos(b.frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
        WinSetFocus(HWND_DESKTOP, b.nb);
    }
}

/// Close the notebook if it shows `name` (before it is deleted).
pub fn closing_for(name: &str) {
    let b = book();
    if b.cur.eq_ignore_ascii_case(name) {
        hide();
        b.cur.clear();
    }
}

fn hide() {
    let b = book();
    if b.frame != 0 {
        unsafe { WinShowWindow(b.frame, 0) };
    }
}

/// The program ends: write what was changed.
pub fn close() {
    hide();
    uni::complete();
}

/// Fill every page from the locale; `skip` is a control being typed in.
fn fill_all(skip: u32) {
    for &(id, ..) in PAGES.iter() {
        fill(id, skip);
    }
}

fn set_entry(p: u32, id: u32, skip: u32, v: &[u8]) {
    if id != skip {
        set_item_text(p, id, v);
    }
}

fn select(p: u32, id: u32, i: i32) {
    if i < 0 {
        dlg_msg(p, id, LM_SELECTITEM, LIT_NONE, 0);
    } else {
        dlg_msg(p, id, LM_SELECTITEM, i as u32, 1);
    }
}

/// Fill a page's controls from the locale shown.
fn fill(id: u16, skip: u32) {
    let b = book();
    let Some(lo) = Lo::new(&b.cur) else { return };
    let p = page(id);
    b.filling = true;
    match id {
        PG_COUNTRY => {
            set_entry(p, ID_LOCALENAME, skip, b.cur.as_bytes());
            let base = uni::ascii(&lo.item(uni::S_NAME));
            let i = b.systems.iter().position(|s| s.eq_ignore_ascii_case(&base)).map_or(-1, |i| i as i32);
            select(p, ID_BASE, i);
            // An override of a system locale keeps its base.
            enable(p, ID_BASE, !b.systems.iter().any(|s| s.eq_ignore_ascii_case(&b.cur)));
            let lang = app().lang.clone();
            let mut l = uni::language_name(&lo.item(uni::S_LANGUAGEID), &lang);
            if l.is_empty() {
                l = uni::language_name(&lo.item(uni::S_LANGUAGEID | uni::LOCI_NOUSEROVERRIDE), &lang);
            }
            let mut c = uni::country_name(&lo.item(uni::S_COUNTRYID), &lang);
            if c.is_empty() {
                c = uni::country_name(&lo.item(uni::S_COUNTRYID | uni::LOCI_NOUSEROVERRIDE), &lang);
            }
            set_item_text(p, ID_LANGNAME, &[&b"Language: "[..], &l].concat());
            set_item_text(p, ID_COUNTRYNAME, &[&b"Country or region: "[..], &c].concat());
            select(p, ID_MEASURE, (lo.value(uni::I_MEASURE) == 1) as i32);
            set_entry(p, ID_LIST, skip, &lo.text(uni::S_LIST));
        }
        PG_COLLATE => {
            let c = uni::ascii(&lo.item(uni::S_COLLATE));
            let i = COLLATIONS.iter().position(|(_, code)| code.eq_ignore_ascii_case(&c)).map_or(COLLATIONS.len() as i32 - 1, |i| i as i32);
            select(p, ID_COLLATE, i);
        }
        PG_LANGUAGE => {
            let l = uni::ascii(&lo.item(uni::S_LANGUAGEID));
            let i = LANGUAGES.iter().position(|(code, _)| code.eq_ignore_ascii_case(&l)).unwrap_or(LANGUAGES.len());
            select(p, ID_LANGUAGE, i as i32);
        }
        PG_NUMERIC => {
            let (dec, th) = (lo.text(uni::S_DECIMAL), lo.text(uni::S_THOUSAND));
            set_entry(p, ID_DECIMAL, skip, &dec);
            set_entry(p, ID_THOUSAND, skip, &th);
            set_entry(p, ID_POSSIGN, skip, &lo.text(uni::S_POSITIVESIGN));
            let neg = lo.text(uni::S_NEGATIVESIGN);
            set_entry(p, ID_NEGSIGN, skip, &neg);
            let digits = lo.value(uni::I_DIGITS).max(0);
            if skip != ID_DIGITS {
                dlg_msg(p, ID_DIGITS, SPBM_SETCURRENTVALUE, digits as u32, 0);
            }
            let n = fmt::number(&dec, &th, digits);
            dlg_msg(p, ID_NEGNUMBER, LM_DELETEALL, 0, 0);
            for f in 0..fmt::NEG_NUMBER_FORMATS {
                lb_add(p, ID_NEGNUMBER, &fmt::neg_number(f, &neg, &n));
            }
            select(p, ID_NEGNUMBER, lo.value(uni::I_NEGNUMBER));
        }
        PG_MONETARY => {
            let (dec, th, sym) = (lo.text(uni::S_MONDECIMALSEP), lo.text(uni::S_MONTHOUSANDSEP), lo.text(uni::S_CURRENCY));
            set_entry(p, ID_CURRENCY, skip, &sym);
            set_entry(p, ID_MONDECIMAL, skip, &dec);
            set_entry(p, ID_MONTHOUSAND, skip, &th);
            let digits = lo.value(uni::I_CURRDIGITS).max(0);
            if skip != ID_DIGITS {
                dlg_msg(p, ID_DIGITS, SPBM_SETCURRENTVALUE, digits as u32, 0);
            }
            let n = fmt::number(&dec, &th, digits);
            let neg = lo.text(uni::S_NEGATIVESIGN);
            dlg_msg(p, ID_CURRPOS, LM_DELETEALL, 0, 0);
            for f in 0..fmt::CURRENCY_FORMATS {
                lb_add(p, ID_CURRPOS, &fmt::currency(f, &sym, &n));
            }
            select(p, ID_CURRPOS, lo.value(uni::I_CURRENCY));
            dlg_msg(p, ID_NEGCURR, LM_DELETEALL, 0, 0);
            for f in 0..fmt::NEG_CURRENCY_FORMATS {
                lb_add(p, ID_NEGCURR, &fmt::neg_currency(f, &sym, &neg, &n));
            }
            select(p, ID_NEGCURR, lo.value(uni::I_NEGCURR));
            set_check(p, ID_LZERO, lo.value(uni::I_LZERO) > 0);
        }
        PG_DATE => {
            let order = lo.value(uni::I_DATE);
            for (id, n) in [(ID_MDY, 0), (ID_DMY, 1), (ID_YMD, 2), (ID_YDM, 3)] {
                set_check(p, id, order == n);
            }
            set_entry(p, ID_DATESEP, skip, &lo.text(uni::S_DATE));
            set_entry(p, ID_DATEFMT, skip, &lo.text(uni::S_SHORTDATE));
            date_sample();
        }
        PG_TIME => {
            let h24 = lo.value(uni::I_TIME) == 1;
            set_check(p, ID_12H, !h24);
            set_check(p, ID_24H, h24);
            set_entry(p, ID_AM, skip, &lo.text(uni::S_1159));
            set_entry(p, ID_PM, skip, &lo.text(uni::S_2359));
            set_entry(p, ID_TIMESEP, skip, &lo.text(uni::S_TIME));
            set_entry(p, ID_TIMEFMT, skip, &lo.text(uni::S_TIMEFORMAT));
            time_sample();
        }
        _ => {
            if skip != ID_VALUE {
                set_item_text(p, ID_VALUE, &lo.text(b.adv_item));
            }
            if skip != ID_ITEMS {
                dlg_msg(p, ID_ITEMS, LM_SELECTITEM, (b.adv_item - 1) as u32, 1);
            }
        }
    }
    b.filling = false;
}

/// The Date page's sample: today in the short date format.
fn date_sample() {
    let b = book();
    let Some(lo) = Lo::new(&b.cur) else { return };
    let s = lo.strftime(&uni::wz("%x")[..2], &uni::now()).map(|s| uni::to_cp(&s)).unwrap_or_default();
    set_item_text(page(PG_DATE), ID_DATESAMPLE, &[&b"Sample: "[..], &s].concat());
}

/// The Time page's sample: the time now (every second).
fn time_sample() {
    let b = book();
    if b.cur.is_empty() {
        return;
    }
    let Some(lo) = Lo::new(&b.cur) else { return };
    let s = lo.strftime(&uni::wz("%X")[..2], &uni::now()).map(|s| uni::to_cp(&s)).unwrap_or_default();
    set_item_text(page(PG_TIME), ID_TIMESAMPLE, &[&b"Sample: "[..], &s].concat());
}

fn set_t(item: i32, v: &[u8]) -> i32 {
    uni::set_text(&book().cur, item, &uni::from_cp(v))
}

fn set_v(item: i32, v: i32) -> i32 {
    uni::set_value(&book().cur, item, v)
}

fn sel(p: u32, id: u32) -> i32 {
    let s = dlg_msg(p, id, LM_QUERYSELECTION, LIT_FIRST, 0);
    if s == 0xffff || s == 0xffff_ffff { -1 } else { s as i32 }
}

/// A control of a page changed: make the change in the locale. Returns
/// the control not to fill again (being typed in), or None when nothing
/// changed.
fn apply(kind: u16, p: u32, id: u32, code: u32) -> Option<u32> {
    let b = book();
    let text = || item_text(p, id);
    let rc = match (kind, id) {
        (PG_COUNTRY, ID_LIST) if code == EN_CHANGE => set_t(uni::S_LIST, &text()),
        (PG_COUNTRY, ID_MEASURE) if code == CBN_LBSELECT => set_v(uni::I_MEASURE, sel(p, id).max(0)),
        (PG_COUNTRY, ID_BASE) if code == CBN_LBSELECT => {
            let i = sel(p, id);
            let Some(base) = (i >= 0).then(|| b.systems.get(i as usize).cloned()).flatten() else { return None };
            let cur = uni::Lo::new(&b.cur).map(|lo| uni::ascii(&lo.item(uni::S_NAME))).unwrap_or_default();
            if cur.eq_ignore_ascii_case(&base) {
                return None;
            }
            let rc = uni::make_user(&b.cur.clone(), &base, true);
            trace(alloc::format!("{} now on {base} ({rc:#x})", b.cur).as_bytes());
            if rc == 0 {
                fill_all(ID_BASE);
                return None;
            }
            rc
        }
        (PG_COLLATE, ID_COLLATE) if code == CBN_LBSELECT => {
            let i = sel(p, id);
            if i < 0 {
                return None;
            }
            set_t(uni::S_COLLATE, COLLATIONS[i as usize].1.as_bytes())
        }
        (PG_LANGUAGE, ID_LANGUAGE) if code == CBN_LBSELECT => {
            let i = sel(p, id);
            if i < 0 || i as usize >= LANGUAGES.len() {
                return None;
            }
            let r = set_t(uni::S_LANGUAGEID, LANGUAGES[i as usize].0.as_bytes());
            fill(PG_COUNTRY, 0);
            r
        }
        (PG_NUMERIC, ID_DECIMAL) if code == EN_CHANGE => set_t(uni::S_DECIMAL, &text()),
        (PG_NUMERIC, ID_THOUSAND) if code == EN_CHANGE => set_t(uni::S_THOUSAND, &text()),
        (PG_NUMERIC, ID_POSSIGN) if code == EN_CHANGE => set_t(uni::S_POSITIVESIGN, &text()),
        (PG_NUMERIC, ID_NEGSIGN) if code == EN_CHANGE => set_t(uni::S_NEGATIVESIGN, &text()),
        (PG_NUMERIC, ID_DIGITS) if code == SPBN_CHANGE => set_v(uni::I_DIGITS, spin_value(p, id)),
        (PG_NUMERIC, ID_NEGNUMBER) if code == CBN_LBSELECT => set_v(uni::I_NEGNUMBER, sel(p, id).max(0)),
        (PG_MONETARY, ID_CURRENCY) if code == EN_CHANGE => set_t(uni::S_CURRENCY, &text()),
        (PG_MONETARY, ID_MONDECIMAL) if code == EN_CHANGE => set_t(uni::S_MONDECIMALSEP, &text()),
        (PG_MONETARY, ID_MONTHOUSAND) if code == EN_CHANGE => set_t(uni::S_MONTHOUSANDSEP, &text()),
        (PG_MONETARY, ID_DIGITS) if code == SPBN_CHANGE => set_v(uni::I_CURRDIGITS, spin_value(p, id)),
        (PG_MONETARY, ID_CURRPOS) if code == CBN_LBSELECT => set_v(uni::I_CURRENCY, sel(p, id).max(0)),
        (PG_MONETARY, ID_NEGCURR) if code == CBN_LBSELECT => set_v(uni::I_NEGCURR, sel(p, id).max(0)),
        (PG_MONETARY, ID_LZERO) if code == BN_CLICKED => set_v(uni::I_LZERO, checked(p, id) as i32),
        (PG_DATE, ID_MDY | ID_DMY | ID_YMD | ID_YDM) if code == BN_CLICKED => {
            let order = match id {
                ID_MDY => 0,
                ID_DMY => 1,
                ID_YMD => 2,
                _ => 3,
            };
            set_date_order(order)
        }
        (PG_DATE, ID_DATESEP) if code == EN_CHANGE => {
            let sep = text();
            let f = item_text(p, ID_DATEFMT);
            let r = set_t(uni::S_DATE, &sep);
            if r == 0 && !sep.is_empty() { set_t(uni::S_SHORTDATE, &fmt::replace_separator(&f, &sep)) } else { r }
        }
        (PG_DATE, ID_DATEFMT) if code == EN_CHANGE => {
            let f = text();
            let r = set_t(uni::S_SHORTDATE, &f);
            match fmt::date_order(&f) {
                Some(o) if r == 0 => set_v(uni::I_DATE, o),
                _ => r,
            }
        }
        (PG_TIME, ID_12H | ID_24H) if code == BN_CLICKED => set_clock(id == ID_24H),
        (PG_TIME, ID_AM) if code == EN_CHANGE => set_t(uni::S_1159, &text()),
        (PG_TIME, ID_PM) if code == EN_CHANGE => set_t(uni::S_2359, &text()),
        (PG_TIME, ID_TIMESEP) if code == EN_CHANGE => {
            let sep = text();
            let f = item_text(p, ID_TIMEFMT);
            let r = set_t(uni::S_TIME, &sep);
            if r == 0 && !sep.is_empty() { set_t(uni::S_TIMEFORMAT, &fmt::replace_separator(&f, &sep)) } else { r }
        }
        (PG_TIME, ID_TIMEFMT) if code == EN_CHANGE => {
            let f = text();
            // A format strftime cannot use: back to 24 hours (OS/2's does
            // so too).
            let ok = uni::Lo::new(&b.cur).is_some_and(|lo| lo.strftime(&uni::from_cp(&f), &uni::now()).is_some());
            if !ok && !f.is_empty() {
                return None;
            }
            set_t(uni::S_TIMEFORMAT, &f)
        }
        (PG_ADVANCED, ID_ITEMS) if code == LN_SELECT => {
            let i = sel(p, id);
            if i >= 0 {
                b.adv_item = i + 1;
            }
            0
        }
        (PG_ADVANCED, ID_VALUE) if code == EN_CHANGE => set_t(b.adv_item, &text()),
        _ => return None,
    };
    if rc != 0 {
        error(app().frame, b"The locale data is incorrect");
    }
    Some(id)
}

fn spin_value(p: u32, id: u32) -> i32 {
    let mut v = 0i32;
    dlg_msg(p, id, SPBM_QUERYVALUE, &mut v as *mut i32 as u32, mp(0, SPBQ_ALWAYSUPDATE));
    v.clamp(0, 9)
}

/// The date order: iDate and the short date format in that order with
/// the page's separator.
pub fn set_date_order(order: i32) -> i32 {
    let b = book();
    let sep = Lo::new(&b.cur).map(|lo| lo.text(uni::S_DATE)).unwrap_or_else(|| b"/".to_vec());
    let r = set_v(uni::I_DATE, order);
    if r != 0 {
        return r;
    }
    set_t(uni::S_SHORTDATE, &fmt::date_format(order, &sep))
}

/// The clock: iTime and the time format for it with the separator.
pub fn set_clock(h24: bool) -> i32 {
    let b = book();
    let sep = Lo::new(&b.cur).map(|lo| lo.text(uni::S_TIME)).unwrap_or_else(|| b":".to_vec());
    let r = set_v(uni::I_TIME, h24 as i32);
    if r != 0 {
        return r;
    }
    set_t(uni::S_TIMEFORMAT, &fmt::time_format(h24, &sep))
}

/// Make the notebook show `name` without windows (the self-check).
pub fn set_current(name: &str) {
    book().cur = String::from(name);
}

/// Undo: the values the locale had when the notebook was opened.
fn undo() {
    let b = book();
    let Some(lo) = Lo::new(&b.cur) else { return };
    let now: Vec<Vec<u16>> = (1..=uni::MAXITEM).map(|i| lo.item(i)).collect();
    drop(lo);
    let mut n = 0;
    for (i, (was, is)) in b.saved.iter().zip(now.iter()).enumerate() {
        if was != is {
            uni::set_text(&b.cur.clone(), i as i32 + 1, was);
            n += 1;
        }
    }
    trace(alloc::format!("undo {}: {n} item(s)", b.cur).as_bytes());
    fill_all(0);
}

/// Default: the base locale's values (the user locale made again on its
/// base).
fn to_default() {
    let b = book();
    let base = Lo::new(&b.cur).map(|lo| uni::ascii(&lo.item(uni::S_NAME | uni::LOCI_NOUSEROVERRIDE))).unwrap_or_default();
    let base = if base.is_empty() { b.cur.clone() } else { base };
    let rc = uni::make_user(&b.cur.clone(), &base, true);
    trace(alloc::format!("default {} on {base} ({rc:#x})", b.cur).as_bytes());
    fill_all(0);
}

fn top_panel() -> u16 {
    let b = book();
    let top = send(b.nb, BKM_QUERYPAGEID, 0, BKA_TOP);
    b.page_ids.iter().position(|&p| p == top).map_or(PANEL_COUNTRY, |i| PAGES[i].3)
}

extern "C" fn book_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let b = book();
    match m {
        WM_WINDOWPOSCHANGED => {
            let r = unsafe { WinDefDlgProc(h, m, mp1, mp2) };
            // SWP: the notebook fills the frame.
            if mp1 != 0 && unsafe { *(mp1 as *const u32) } & SWP_SIZE != 0 && b.nb != 0 {
                let mut rc = window_rect(h);
                unsafe {
                    WinCalcFrameRect(h, &mut rc, 1);
                    WinSetWindowPos(b.nb, 0, rc.l, rc.b, rc.width(), rc.height(), SWP_SIZE | SWP_MOVE);
                }
            }
            r
        }
        WM_CONTROL if lo(mp1) == ID_NOTEBOOK && hi(mp1) == BKN_HELP => {
            crate::show_help(top_panel());
            0
        }
        WM_HELP => {
            crate::show_help(top_panel());
            0
        }
        // The system menu's Close (and Esc) come as DID_CANCEL.
        WM_COMMAND if lo(mp1) != DID_CANCEL => 0,
        WM_COMMAND | WM_CLOSE => {
            if uni::complete() != 0 {
                error(app().frame, b"The Locale database could not be updated");
            }
            hide();
            trace(alloc::format!("notebook {} closed", b.cur).as_bytes());
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

extern "C" fn page_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    let b = book();
    match m {
        WM_INITDLG => 1,
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            unsafe {
                WinFillRect(hps, &r, SYSCLR_DIALOGBACKGROUND);
                WinEndPaint(hps);
            }
            0
        }
        WM_TIMER => {
            if page_kind(h) == PG_TIME && unsafe { WinIsWindowVisible(b.frame) } != 0 {
                time_sample();
            }
            0
        }
        WM_CONTROL => {
            if b.filling || b.cur.is_empty() {
                return 0;
            }
            let kind = page_kind(h);
            if let Some(skip) = apply(kind, h, lo(mp1), hi(mp1)) {
                fill(kind, skip);
                if kind == PG_NUMERIC || kind == PG_MONETARY {
                    // The other page's samples use the signs too.
                    fill(if kind == PG_NUMERIC { PG_MONETARY } else { PG_NUMERIC }, 0);
                }
            }
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_UNDO => undo(),
                ID_DEFAULT => to_default(),
                _ => {}
            }
            0
        }
        WM_HELP => {
            crate::show_help(top_panel());
            0
        }
        // A page is closed with its notebook, not by Esc or Enter.
        WM_CLOSE => 0,
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}
