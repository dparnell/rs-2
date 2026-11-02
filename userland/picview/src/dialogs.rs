//! The Print (copies), Go to page and Code page windows.

use alloc::vec::Vec;

use pmkit::*;

use crate::TITLE;

pub const DLG_PAGE: u16 = 301;
pub const DLG_PRINT: u16 = 302;
pub const DLG_CODEPAGE: u16 = 550;
const ID_PAGENUM: u32 = 303;
const ID_COPIES: u32 = 304;
const ID_COUNTRIES: u32 = 551;
const ID_EBCDIC: u32 = 552;
const ID_DEFAULT: u32 = 553;
const ID_HELP: u32 = 410;

static mut VALUE: u32 = 0;
static mut LIMIT: u32 = 0;

fn value() -> &'static mut u32 {
    unsafe { &mut *core::ptr::addr_of_mut!(VALUE) }
}

fn limit() -> u32 {
    unsafe { LIMIT }
}

/// A dialog asking for a number (copies or a page) in entry field `id`.
fn number_proc(h: u32, m: u32, mp1: u32, mp2: u32, id: u32, bad: &[u8]) -> u32 {
    match m {
        WM_INITDLG => {
            dlg_msg(h, id, EM_SETTEXTLIMIT, 3, 0);
            set_item_text(h, id, &num(*value() as i64));
            dlg_msg(h, id, EM_SETSEL, mp(0, 3), 0);
            unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, id)) };
            1
        }
        WM_COMMAND => {
            match lo(mp1) {
                DID_OK => match parse_num(&item_text(h, id)) {
                    Some(n) if n >= 1 && (n as u32) <= limit() => {
                        *value() = n as u32;
                        unsafe { WinDismissDlg(h, DID_OK) };
                    }
                    _ => {
                        message(h, bad, TITLE, MB_OK | MB_WARNING);
                        unsafe { WinSetFocus(HWND_DESKTOP, pmkit::item(h, id)) };
                    }
                },
                DID_CANCEL => unsafe {
                    WinDismissDlg(h, DID_CANCEL);
                },
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

extern "C" fn print_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    number_proc(h, m, mp1, mp2, ID_COPIES, b"The number of copies is not valid. Type a number from 1 to 99.")
}

extern "C" fn page_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    number_proc(h, m, mp1, mp2, ID_PAGENUM, b"That page number does not exist. Type another.")
}

/// Print: the number of copies.
pub fn print_dialog(owner: u32, copies: u32) -> Option<u32> {
    *value() = copies.max(1);
    unsafe { LIMIT = 99 };
    let tpl = template(
        dialog_frame(b"Picture Viewer - Print", DLG_PRINT, 170, 60),
        alloc::vec![
            label(b"Copies:", (6, 38, 40, 8), DT_LEFT),
            entry((48, 38, 24, 8), ID_COPIES, 0),
            button(b"~OK", (8, 8, 40), DID_OK, BS_DEFAULT),
            button(b"Cancel", (56, 8, 50), DID_CANCEL, 0),
            help_button((114, 8, 48), ID_HELP),
        ],
    );
    (run_dialog(owner, print_proc, &tpl, 0) == DID_OK).then(|| *value())
}

/// Go to page: the page (0-based) of `pages`.
pub fn page_dialog(owner: u32, pages: usize) -> Option<usize> {
    let cur = crate::app().doc.as_ref().map_or(0, |d| d.page);
    *value() = cur as u32 + 1;
    unsafe { LIMIT = pages as u32 };
    let text = alloc::format!("Page number (1-{pages}):").into_bytes();
    let tpl = template(
        dialog_frame(b"Picture Viewer - Page", DLG_PAGE, 160, 65),
        alloc::vec![
            label(&text, (8, 38, 96, 10), DT_LEFT),
            entry((108, 38, 24, 10), ID_PAGENUM, 0),
            button(b"~OK", (8, 8, 40), DID_OK, BS_DEFAULT),
            button(b"Cancel", (54, 8, 46), DID_CANCEL, 0),
            help_button((106, 8, 46), ID_HELP),
        ],
    );
    (run_dialog(owner, page_proc, &tpl, 0) == DID_OK).then(|| *value() as usize - 1)
}

// ---- Code page ----

/// The countries whose PIF text code pages OS/2's converter knows
/// (the country of origin of a PIF file decides how its text is read).
const COUNTRIES: &[&[u8]] = &[
    b"Australia, Belgium, Canada, Netherlands, Switzerland",
    b"Denmark, Norway",
    b"Finland, Sweden",
    b"France",
    b"Germany",
    b"Italy",
    b"Latin America, Spain",
    b"Brazil, Portugal, United States",
    b"United Kingdom",
    b"Japan (Katakana/Kanji)",
    b"Japan (Alphanumeric/Kanji)",
    b"Japan (SAA)",
    b"Korea",
    b"Korea (SAA)",
    b"People's Republic of China",
    b"Taiwan",
    b"Taiwan (SAA)",
    b"Albania",
    b"Central Europe (Latin 2)",
    b"Cyrillic (855)",
    b"Cyrillic (915)",
    b"Russia",
    b"Latvia, Lithuania",
    b"Estonia",
    b"Turkey",
    b"Arabic",
    b"Hebrew",
    b"Greece (813)",
    b"Greece (869)",
    b"Thailand",
];

/// The default: Brazil, Portugal, United States.
const DEFAULT_COUNTRY: u32 = 7;

extern "C" fn code_page_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_INITDLG => {
            for c in COUNTRIES {
                lb_add(h, ID_COUNTRIES, c);
            }
            let v = ini::read(b"PicView", b"CodePage").filter(|v| v.len() >= 8);
            let (country, ebcdic) = v.map_or((DEFAULT_COUNTRY, false), |v| (rd32(&v, 0), rd32(&v, 4) != 0));
            dlg_msg(h, ID_COUNTRIES, LM_SELECTITEM, country.min(COUNTRIES.len() as u32 - 1), 1);
            dlg_msg(h, ID_COUNTRIES, LM_SETTOPINDEX, country.saturating_sub(2), 0);
            set_check(h, ID_EBCDIC, ebcdic);
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                DID_OK => {
                    let sel = dlg_msg(h, ID_COUNTRIES, LM_QUERYSELECTION, LIT_FIRST, 0) & 0xffff;
                    let sel = if sel as usize >= COUNTRIES.len() { DEFAULT_COUNTRY } else { sel };
                    let mut v: Vec<u8> = sel.to_le_bytes().to_vec();
                    v.extend_from_slice(&(checked(h, ID_EBCDIC) as u32).to_le_bytes());
                    ini::write(b"PicView", b"CodePage", &v);
                    unsafe { WinDismissDlg(h, DID_OK) };
                }
                ID_DEFAULT => {
                    dlg_msg(h, ID_COUNTRIES, LM_SELECTITEM, DEFAULT_COUNTRY, 1);
                    set_check(h, ID_EBCDIC, false);
                }
                DID_CANCEL => unsafe {
                    WinDismissDlg(h, DID_CANCEL);
                },
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, mp1, mp2) },
    }
}

/// Options > Code page: the country of origin of PIF files.
pub fn code_page_dialog(owner: u32) {
    let tpl = template(
        dialog_frame(b"Picture Viewer - Code Page", DLG_CODEPAGE, 288, 180),
        alloc::vec![
            label(b"Country of origin of PIF files", (24, 164, 240, 8), DT_CENTER),
            list_box((24, 72, 240, 90), ID_COUNTRIES, 0),
            check_box(b"Use an EBCDIC code page for metafile text", (40, 44, 220, 12), ID_EBCDIC),
            button(b"~OK", (24, 8, 48), DID_OK, BS_DEFAULT),
            button(b"~Default", (88, 8, 48), ID_DEFAULT, 0),
            button(b"Cancel", (152, 8, 48), DID_CANCEL, 0),
            help_button((216, 8, 48), ID_HELP),
        ],
    );
    run_dialog(owner, code_page_proc, &tpl, 0);
}
