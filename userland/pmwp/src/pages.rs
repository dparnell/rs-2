//! The settings pages IBM's classes add that the other modules do not
//! (laid out after IBM's, PMWPMRI's dialogs):
//!
//! - WPObject's **Window** page (wpAddObjectWindowPage): the object open
//!   behavior (wpQuery/SetConcurrentView, CCVIEW=). WPCONFIG's objects
//!   and the Shredder leave it out (SETTINGS_PAGE_REMOVED), as IBM's do.
//! - WPFileSystem's three **File** pages: the subject (`.SUBJECT`) and
//!   the physical name; the dates, flags (read-only, archive, hidden,
//!   system: changed at once) and sizes, of a folder its objects; the
//!   comments, key phrases and history (`.COMMENTS`, `.KEYPHRASES`,
//!   `.HISTORY`, multi-valued EAs).
//! - WPDataFile's **Type** page (`.TYPE`, wpQueryType/wpSetType): the
//!   file's types, from the types OS2.INI knows (`PMWP_ASSOC_TYPE`) and
//!   IBM's standard ones.
//! - WPProgram's and WPProgramFile's **Association** page: the types
//!   (`PMWP_ASSOC_TYPE`) and names (`PMWP_ASSOC_FILTER`, ASSOCFILTER=)
//!   whose files the program opens (ASSOCTYPE=,
//!   wpQuery/SetAssociationType and -Filter); WPProgram's **Session**
//!   page (OS/2 or DOS, full screen or window).
//! - WPDisk's **Details** page (wpAddDiskDetailsPage): the file system,
//!   label, serial number and space, from DosQueryFSInfo.

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::som;
use crate::obj::{self, CCVIEW_DEFAULT, CCVIEW_OFF, CCVIEW_ON};
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio};
use crate::util::{cstr, ea_read_raw, ea_write_raw, parse_dec, prf_keys, prf_read, prf_write, z, EAT_ASCII, HINI_USER};
use crate::wps;

pub const SETTINGS_PAGE_REMOVED: u32 = 0xffff_ffff;
const EAT_MVMT: u16 = 0xffdf;
const WC_MLE: u16 = 10;

fn page_obj(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

fn init_obj(h: u32, b: u32) -> usize {
    unsafe { WinSetWindowULong(h, QWL_USER, b) };
    b as usize
}

fn static_text(t: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    item(WC_STATIC, t, WS_VISIBLE | 0x0001, (x, y, cx, 8), id)
}

fn button(t: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP, (x, y, cx, 12), id)
}

fn listbox((x, y, cx, cy): (i16, i16, i16, i16), id: u16) -> Item {
    item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (x, y, cx, cy), id)
}

fn mle((x, y, cx, cy): (i16, i16, i16, i16), id: u16, read_only: bool) -> Item {
    // MLS_BORDER | MLS_VSCROLL | MLS_WORDWRAP (| MLS_READONLY)
    item(WC_MLE, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0002 | 0x0004 | 0x0001 | if read_only { 0x0010 } else { 0 }, (x, y, cx, cy), id)
}

fn long_text(h: u32, id: u16) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; 8192];
    let n = unsafe { WinQueryDlgItemText(h, id as u32, buf.len() as i32, buf.as_mut_ptr()) } as usize;
    buf.truncate(n.min(buf.len()));
    buf
}

fn lines(t: &[u8]) -> Vec<Vec<u8>> {
    t.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).filter(|l| !l.is_empty()).collect()
}

// ---- Multi-valued EAs ----

/// The strings of an EA: EAT_ASCII, or EAT_MVMT of EAT_ASCII values.
pub fn ea_strings(path: &[u8], name: &[u8]) -> Vec<Vec<u8>> {
    let Some(v) = ea_read_raw(path, name) else { return Vec::new() };
    let w = |i: usize| v.get(i..i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize);
    match w(0) as u16 {
        EAT_ASCII => alloc::vec![v.get(4..4 + w(2)).unwrap_or(&[]).to_vec()],
        EAT_MVMT => {
            let mut out = Vec::new();
            let mut at = 6;
            for _ in 0..w(4) {
                let (ty, n) = (w(at) as u16, w(at + 2));
                let s = v.get(at + 4..at + 4 + n).unwrap_or(&[]);
                if ty == EAT_ASCII && !s.is_empty() {
                    out.push(s.to_vec());
                }
                at += 4 + n;
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Write strings as an EAT_MVMT EA (none removes it).
pub fn set_ea_strings(path: &[u8], name: &[u8], list: &[Vec<u8>]) -> bool {
    if list.is_empty() {
        return ea_write_raw(path, name, None);
    }
    let mut v = Vec::new();
    v.extend_from_slice(&EAT_MVMT.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&(list.len() as u16).to_le_bytes());
    for s in list {
        v.extend_from_slice(&EAT_ASCII.to_le_bytes());
        v.extend_from_slice(&(s.len() as u16).to_le_bytes());
        v.extend_from_slice(s);
    }
    ea_write_raw(path, name, Some(&v))
}

fn set_ea_ascii(path: &[u8], name: &[u8], s: &[u8]) -> bool {
    if s.is_empty() {
        return ea_write_raw(path, name, None);
    }
    let mut v = Vec::new();
    v.extend_from_slice(&EAT_ASCII.to_le_bytes());
    v.extend_from_slice(&(s.len() as u16).to_le_bytes());
    v.extend_from_slice(s);
    ea_write_raw(path, name, Some(&v))
}

// ---- WPObject: the Window page ----

const ID_CC_DEFAULT: u16 = 800;
const ID_CC_EXISTING: u16 = 801;
const ID_CC_NEW: u16 = 802;

extern "C" fn window_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = init_obj(h, b);
            let id = match obj::call(m::WPObject::wpQueryConcurrentView, &[o]) as u32 {
                CCVIEW_ON => ID_CC_NEW,
                CCVIEW_OFF => ID_CC_EXISTING,
                _ => ID_CC_DEFAULT,
            };
            check(h, id, true);
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let v = match (a & 0xffff) as u16 {
                ID_CC_DEFAULT => CCVIEW_DEFAULT,
                ID_CC_EXISTING => CCVIEW_OFF,
                ID_CC_NEW => CCVIEW_ON,
                _ => return 0,
            };
            let o = page_obj(h);
            obj::call(m::WPObject::wpSetConcurrentView, &[o, v as usize]);
            obj::call(m::WPObject::wpSaveDeferred, &[o]);
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddObjectWindowPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, window_page_proc, alloc::vec![
        book::label(b"Object open behavior", (6, 106, 220)),
        radio(b"As the ~System object says", (12, 92, 210), ID_CC_DEFAULT, true),
        radio(b"Display ~existing window", (12, 80, 210), ID_CC_EXISTING, false),
        radio(b"~Create new window", (12, 68, 210), ID_CC_NEW, false),
        book::label(b"The minimize button follows the System object.", (6, 46, 220)),
    ], b"~Window\0")
}

extern "C" fn no_window_page(_o: usize, _nb: u32) -> u32 {
    SETTINGS_PAGE_REMOVED
}

// ---- WPFileSystem: the File pages ----

const ID_SUBJECT: u16 = 810;
const ID_PATH: u16 = 811;
const ID_NAME: u16 = 812;
const ID_CREATED: u16 = 813;
const ID_CHANGED: u16 = 814;
const ID_ACCESSED: u16 = 815;
const ID_RO: u16 = 816;
const ID_ARCHIVE: u16 = 817;
const ID_HIDDEN: u16 = 818;
const ID_SYSTEM: u16 = 819;
const ID_SIZE: u16 = 820;
const ID_EASIZE: u16 = 821;
const ID_COMMENTS: u16 = 822;
const ID_KEYS: u16 = 823;
const ID_HISTORY: u16 = 824;

fn obj_path(o: usize) -> Vec<u8> {
    wps::path_of(o).unwrap_or_default()
}

extern "C" fn file1_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = init_obj(h, b);
            let p = obj_path(o);
            let (dir, name) = crate::util::split_path(&p);
            set_item_text(h, ID_PATH as u32, &dir);
            set_item_text(h, ID_NAME as u32, &name);
            unsafe { WinSendDlgItemMsg(h, ID_SUBJECT as u32, EM_SETTEXTLIMIT, 40, 0) };
            let s = ea_strings(&p, b".SUBJECT").into_iter().next().unwrap_or_default();
            set_item_text(h, ID_SUBJECT as u32, &s);
            0
        }
        WM_DESTROY => {
            let o = page_obj(h);
            if o != 0 {
                let p = obj_path(o);
                let s = item_text(h, ID_SUBJECT as u32);
                if s != ea_strings(&p, b".SUBJECT").into_iter().next().unwrap_or_default() {
                    set_ea_ascii(&p, b".SUBJECT", &s);
                }
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFile1Page(o: usize, nb: u32) -> u32 {
    // ES_AUTOSCROLL | ES_MARGIN
    add_page(o, nb, file1_proc, alloc::vec![
        book::label(b"Subject", (6, 106, 220)),
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 94, 216, 8), ID_SUBJECT),
        book::label(b"Physical name", (6, 74, 220)),
        book::label(b"Path:", (12, 62, 40)),
        static_text(b"", (54, 62, 176), ID_PATH),
        book::label(b"Name:", (12, 50, 40)),
        static_text(b"", (54, 50, 176), ID_NAME),
    ], b"~File\0")
}

/// FILESTATUS3 (level 1) and, from level 2, the EAs' size.
fn file_status(p: &[u8]) -> Option<([u8; 24], u32)> {
    let zp = z(p);
    let mut fs4 = [0u8; 28];
    if unsafe { os2::DosQueryPathInfo(zp.as_ptr(), 2, fs4.as_mut_ptr(), 28) } != 0 {
        return None;
    }
    let mut fs3 = [0u8; 24];
    fs3.copy_from_slice(&fs4[..24]);
    Some((fs3, u32::from_le_bytes([fs4[24], fs4[25], fs4[26], fs4[27]])))
}

fn date_time(fs: &[u8], at: usize) -> Vec<u8> {
    let d = u16::from_le_bytes([fs[at], fs[at + 1]]);
    let t = u16::from_le_bytes([fs[at + 2], fs[at + 3]]);
    if d == 0 {
        return b"-".to_vec();
    }
    alloc::format!("{:02}-{:02}-{} {:02}:{:02}:{:02}", (d >> 5) & 15, d & 31, 1980 + (d >> 9), t >> 11, (t >> 5) & 63, (t & 31) * 2).into_bytes()
}

const ATTR_BITS: [(u16, u32); 4] = [(ID_RO, 0x01), (ID_HIDDEN, 0x02), (ID_SYSTEM, 0x04), (ID_ARCHIVE, 0x20)];

extern "C" fn file2_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = init_obj(h, b);
            let p = obj_path(o);
            if let Some((fs, ea)) = file_status(&p) {
                set_item_text(h, ID_CREATED as u32, &date_time(&fs, 0));
                set_item_text(h, ID_ACCESSED as u32, &date_time(&fs, 4));
                set_item_text(h, ID_CHANGED as u32, &date_time(&fs, 8));
                let attr = u32::from_le_bytes([fs[20], fs[21], fs[22], fs[23]]);
                for (id, bit) in ATTR_BITS {
                    check(h, id, attr & bit != 0);
                }
                let size = if wps::is(o, "WPFolder") {
                    let n = crate::util::list_dir(&p).iter().filter(|(_, a)| a & 0x02 == 0).count();
                    alloc::format!("{n} objects")
                } else {
                    alloc::format!("{} bytes", u32::from_le_bytes([fs[12], fs[13], fs[14], fs[15]]))
                };
                set_item_text(h, ID_SIZE as u32, size.as_bytes());
                set_item_text(h, ID_EASIZE as u32, alloc::format!("{ea} bytes").as_bytes());
            }
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let id = (a & 0xffff) as u16;
            let Some(&(_, bit)) = ATTR_BITS.iter().find(|e| e.0 == id) else { return 0 };
            let p = obj_path(page_obj(h));
            if let Some((mut fs, _)) = file_status(&p) {
                let mut attr = u32::from_le_bytes([fs[20], fs[21], fs[22], fs[23]]);
                attr = if checked(h, id) { attr | bit } else { attr & !bit };
                // Zero dates and times leave them as they are.
                fs[..12].fill(0);
                fs[20..24].copy_from_slice(&(attr & 0x27).to_le_bytes());
                let zp = z(&p);
                if unsafe { os2::DosSetPathInfo(zp.as_ptr(), 1, fs.as_ptr(), 24, 0) } != 0 {
                    check(h, id, !checked(h, id));
                }
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFile2Page(o: usize, nb: u32) -> u32 {
    // BS_AUTOCHECKBOX
    let cb = |t: &[u8], (x, y): (i16, i16), id: u16| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | 0x0001, (x, y, 80, 10), id);
    add_more(o, nb, file2_proc, alloc::vec![
        book::label(b"Date/Time", (6, 106, 220)),
        book::label(b"Created", (12, 94, 70)),
        static_text(b"", (86, 94, 140), ID_CREATED),
        book::label(b"Last changed", (12, 84, 70)),
        static_text(b"", (86, 84, 140), ID_CHANGED),
        book::label(b"Last accessed", (12, 74, 70)),
        static_text(b"", (86, 74, 140), ID_ACCESSED),
        book::label(b"Flags", (6, 60, 220)),
        cb(b"~Read-only", (12, 48), ID_RO),
        cb(b"~Archive", (110, 48), ID_ARCHIVE),
        cb(b"~Hidden", (12, 36), ID_HIDDEN),
        cb(b"~System", (110, 36), ID_SYSTEM),
        book::label(b"Size", (6, 22, 220)),
        book::label(b"Contents", (12, 12, 70)),
        static_text(b"", (86, 12, 140), ID_SIZE),
        book::label(b"Extended attributes", (12, 2, 74)),
        static_text(b"", (86, 2, 140), ID_EASIZE),
    ], b"File - Page 2 of 3\0")
}

extern "C" fn file3_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = init_obj(h, b);
            let p = obj_path(o);
            for (id, ea) in [(ID_COMMENTS, &b".COMMENTS"[..]), (ID_KEYS, b".KEYPHRASES"), (ID_HISTORY, b".HISTORY")] {
                let t = ea_strings(&p, ea).join(&b"\n"[..]);
                set_item_text(h, id as u32, &t);
            }
            0
        }
        WM_DESTROY => {
            let o = page_obj(h);
            if o != 0 {
                let p = obj_path(o);
                for (id, ea) in [(ID_COMMENTS, &b".COMMENTS"[..]), (ID_KEYS, b".KEYPHRASES")] {
                    let l = lines(&long_text(h, id));
                    if l != ea_strings(&p, ea) {
                        set_ea_strings(&p, ea, &l);
                    }
                }
            }
            unsafe { WinDefDlgProc(h, msg, a, b) }
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFile3Page(o: usize, nb: u32) -> u32 {
    add_more(o, nb, file3_proc, alloc::vec![
        book::label(b"Comments", (6, 108, 220)),
        mle((8, 80, 220, 28), ID_COMMENTS, false),
        book::label(b"Key phrases", (6, 70, 220)),
        mle((8, 44, 220, 26), ID_KEYS, false),
        book::label(b"History", (6, 34, 220)),
        mle((8, 2, 220, 32), ID_HISTORY, true),
    ], b"File - Page 3 of 3\0")
}

/// A page under the tab before it (no tab of its own).
fn add_more(o: usize, nb: u32, proc_: extern "C" fn(u32, u32, u32, u32) -> u32, items: Vec<Item>, status: &'static [u8]) -> u32 {
    let tpl = template(book::page_frame(), items);
    let page = book::make_page(nb, proc_, &tpl, o);
    if page == 0 {
        return 0;
    }
    book::insert_more(o, nb, page, status)
}

/// WPFileSystem's pages: the File tab (page 1, then 2 and 3 under it;
/// inserted in front, so the last first).
extern "C" fn fs_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPFileSystem"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPFileSystem", m::WPFileSystem::wpAddFile3Page, &[o, nb as usize]);
    wps::call("WPFileSystem", m::WPFileSystem::wpAddFile2Page, &[o, nb as usize]);
    wps::call("WPFileSystem", m::WPFileSystem::wpAddFile1Page, &[o, nb as usize]);
    1
}

// ---- Types (`.TYPE`) ----

/// IBM's standard types.
const STANDARD_TYPES: [&[u8]; 18] = [
    b"Plain Text", b"OS/2 Command File", b"DOS Command File", b"Executable", b"Metafile", b"Bitmap", b"Icon", b"Binary Data",
    b"Dynamic Link Library", b"C Code", b"Pascal Code", b"BASIC Code", b"COBOL Code", b"FORTRAN Code", b"Assembler Code", b"Library",
    b"Resource File", b"Object Code",
];

/// Every type: the standard ones and those OS2.INI associates.
fn all_types() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = STANDARD_TYPES.iter().map(|t| t.to_vec()).collect();
    for k in prf_keys(HINI_USER, b"PMWP_ASSOC_TYPE") {
        if !v.iter().any(|t| t.eq_ignore_ascii_case(&k)) {
            v.push(k);
        }
    }
    v.sort_by_key(|a| a.to_ascii_uppercase());
    v
}

/// A file's types.
pub fn types_of(o: usize) -> Vec<Vec<u8>> {
    ea_strings(&obj_path(o), b".TYPE")
}

/// wpQueryType: the types, separated by line feeds (NULL without any).
extern "C" fn wpQueryType(o: usize) -> *const u8 {
    static mut LAST: Vec<u8> = Vec::new();
    let t = types_of(o);
    if t.is_empty() {
        return core::ptr::null();
    }
    let mut s = t.join(&b"\n"[..]);
    s.push(0);
    unsafe {
        *core::ptr::addr_of_mut!(LAST) = s;
        (*core::ptr::addr_of!(LAST)).as_ptr()
    }
}

extern "C" fn wpSetType(o: usize, types: *const u8, _eas: u32) -> u32 {
    let l = lines(cstr(types));
    set_ea_strings(&obj_path(o), b".TYPE", &l) as u32
}

const ID_AVAIL: u16 = 830;
const ID_CURRENT: u16 = 831;
const ID_ADD: u16 = 832;
const ID_REMOVE: u16 = 833;
const ID_NEWNAME: u16 = 834;
const ID_ADDNAME: u16 = 835;
const ID_NAMES: u16 = 836;
const ID_REMOVENAME: u16 = 837;

fn lb_fill(h: u32, id: u16, items: &[Vec<u8>]) {
    let lb = unsafe { WinWindowFromID(h, id as u32) };
    // LM_DELETEALL
    send(lb, 0x016e, 0, 0);
    for it in items {
        let t = z(it);
        send(lb, LM_INSERTITEM, 0xffff, t.as_ptr() as u32);
    }
}

fn lb_selected(h: u32, id: u16) -> Option<Vec<u8>> {
    let lb = unsafe { WinWindowFromID(h, id as u32) };
    // LM_QUERYSELECTION (LIT_FIRST), LM_QUERYITEMTEXT
    let i = send(lb, 0x0165, 0xffff, 0) as i32;
    if i < 0 {
        return None;
    }
    let mut buf = [0u8; 128];
    let n = send(lb, 0x0168, mp(i as u32, buf.len() as u32), buf.as_mut_ptr() as u32) as usize;
    Some(buf[..n.min(buf.len())].to_vec())
}

fn refresh_types(h: u32, current: &[Vec<u8>]) {
    let avail: Vec<Vec<u8>> = all_types().into_iter().filter(|t| !current.iter().any(|c| c.eq_ignore_ascii_case(t))).collect();
    lb_fill(h, ID_AVAIL, &avail);
    lb_fill(h, ID_CURRENT, current);
}

extern "C" fn type_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = init_obj(h, b);
            refresh_types(h, &types_of(o));
            0
        }
        WM_COMMAND => {
            let o = page_obj(h);
            let mut cur = types_of(o);
            match (a & 0xffff) as u16 {
                ID_ADD => {
                    if let Some(t) = lb_selected(h, ID_AVAIL) {
                        cur.push(t);
                    }
                }
                ID_REMOVE => {
                    if let Some(t) = lb_selected(h, ID_CURRENT) {
                        cur.retain(|c| *c != t);
                    }
                }
                _ => return 0,
            }
            set_ea_strings(&obj_path(o), b".TYPE", &cur);
            refresh_types(h, &cur);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn df_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPDataFile"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPDataFile", m::WPDataFile::wpAddFileTypePage, &[o, nb as usize]);
    1
}

extern "C" fn wpAddFileTypePage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, type_proc, alloc::vec![
        book::label(b"Available types", (6, 106, 100)),
        listbox((6, 8, 88, 96), ID_AVAIL),
        button(b"~Add >>", (98, 70, 40), ID_ADD),
        button(b"<< ~Remove", (98, 52, 40), ID_REMOVE),
        book::label(b"Current types", (142, 106, 90)),
        listbox((142, 8, 88, 96), ID_CURRENT),
    ], b"~Type\0")
}

// ---- Programs: associations ----

/// The handles an OS2.INI association key lists (NUL separated, decimal).
fn assoc_handles(app: &[u8], key: &[u8]) -> Vec<u32> {
    prf_read(HINI_USER, app, key).map_or(Vec::new(), |v| v.split(|&c| c == 0).filter_map(parse_dec).collect())
}

fn set_assoc(app: &[u8], key: &[u8], h: u32, on: bool) {
    let mut l = assoc_handles(app, key);
    l.retain(|&x| x != h);
    if on {
        l.push(h);
    }
    if l.is_empty() {
        prf_write(HINI_USER, app, key, None);
        return;
    }
    let mut v = Vec::new();
    for x in l {
        v.extend_from_slice(alloc::format!("{x}").as_bytes());
        v.push(0);
    }
    prf_write(HINI_USER, app, key, Some(&v));
}

/// The keys of an association application that list the program.
fn assoc_of(app: &[u8], h: u32) -> Vec<Vec<u8>> {
    prf_keys(HINI_USER, app).into_iter().filter(|k| assoc_handles(app, k).contains(&h)).collect()
}

/// ASSOCTYPE=Plain Text,...: the program opens files of these types.
pub fn associate_types(o: usize, list: &[u8]) {
    let h = obj::handle(o);
    for t in list.split(|&c| c == b',').map(|t| t.trim_ascii()).filter(|t| !t.is_empty()) {
        set_assoc(b"PMWP_ASSOC_TYPE", t, h, true);
    }
}

/// The programs (handles) that open files of a type.
pub fn programs_for_type(t: &[u8]) -> Vec<u32> {
    let key = prf_keys(HINI_USER, b"PMWP_ASSOC_TYPE").into_iter().find(|k| k.eq_ignore_ascii_case(t));
    key.map_or(Vec::new(), |k| assoc_handles(b"PMWP_ASSOC_TYPE", &k))
}

static mut ASSOC_BUF: Vec<u8> = Vec::new();

fn joined(list: Vec<Vec<u8>>, sep: u8) -> *const u8 {
    if list.is_empty() {
        return core::ptr::null();
    }
    let mut s = list.join(&[sep][..]);
    s.push(0);
    unsafe {
        *core::ptr::addr_of_mut!(ASSOC_BUF) = s;
        (*core::ptr::addr_of!(ASSOC_BUF)).as_ptr()
    }
}

extern "C" fn wpQueryAssociationType(o: usize) -> *const u8 {
    joined(assoc_of(b"PMWP_ASSOC_TYPE", obj::handle(o)), b'\n')
}

extern "C" fn wpQueryAssociationFilter(o: usize) -> *const u8 {
    joined(assoc_of(b"PMWP_ASSOC_FILTER", obj::handle(o)), b',')
}

/// wpSetAssociationType/Filter: the list replaces the program's own.
fn set_assocs(o: usize, app: &[u8], list: &[u8], sep: u8) -> u32 {
    let h = obj::handle(o);
    for k in assoc_of(app, h) {
        set_assoc(app, &k, h, false);
    }
    for t in list.split(|&c| c == sep || c == b'\n').map(|t| t.trim_ascii()).filter(|t| !t.is_empty()) {
        let t = if app == b"PMWP_ASSOC_FILTER" { t.to_ascii_uppercase() } else { t.to_vec() };
        set_assoc(app, &t, h, true);
    }
    1
}

extern "C" fn wpSetAssociationType(o: usize, list: *const u8) -> u32 {
    set_assocs(o, b"PMWP_ASSOC_TYPE", cstr(list), b'\n')
}

extern "C" fn wpSetAssociationFilter(o: usize, list: *const u8) -> u32 {
    set_assocs(o, b"PMWP_ASSOC_FILTER", cstr(list), b',')
}

fn refresh_assoc(h: u32, o: usize) {
    let hd = obj::handle(o);
    let cur = assoc_of(b"PMWP_ASSOC_TYPE", hd);
    let avail: Vec<Vec<u8>> = all_types().into_iter().filter(|t| !cur.iter().any(|c| c.eq_ignore_ascii_case(t))).collect();
    lb_fill(h, ID_AVAIL, &avail);
    lb_fill(h, ID_CURRENT, &cur);
    lb_fill(h, ID_NAMES, &assoc_of(b"PMWP_ASSOC_FILTER", hd));
}

extern "C" fn assoc_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = init_obj(h, b);
            refresh_assoc(h, o);
            0
        }
        WM_COMMAND => {
            let o = page_obj(h);
            let hd = obj::handle(o);
            match (a & 0xffff) as u16 {
                ID_ADD => {
                    if let Some(t) = lb_selected(h, ID_AVAIL) {
                        set_assoc(b"PMWP_ASSOC_TYPE", &t, hd, true);
                    }
                }
                ID_REMOVE => {
                    if let Some(t) = lb_selected(h, ID_CURRENT) {
                        set_assoc(b"PMWP_ASSOC_TYPE", &t, hd, false);
                    }
                }
                ID_ADDNAME => {
                    let n = item_text(h, ID_NEWNAME as u32).trim_ascii().to_ascii_uppercase();
                    if !n.is_empty() {
                        set_assoc(b"PMWP_ASSOC_FILTER", &n, hd, true);
                        set_item_text(h, ID_NEWNAME as u32, b"");
                    }
                }
                ID_REMOVENAME => {
                    if let Some(n) = lb_selected(h, ID_NAMES) {
                        set_assoc(b"PMWP_ASSOC_FILTER", &n, hd, false);
                    }
                }
                _ => return 0,
            }
            refresh_assoc(h, o);
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn pf_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPProgramFile"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPProgramFile", m::WPProgramFile::wpAddProgramAssociationPage, &[o, nb as usize]);
    1
}

extern "C" fn wpAddProgramAssociationPage(o: usize, nb: u32) -> u32 {
    add_page(o, nb, assoc_proc, alloc::vec![
        book::label(b"Available types", (6, 106, 100)),
        listbox((6, 60, 88, 44), ID_AVAIL),
        button(b"~Add >>", (98, 88, 40), ID_ADD),
        button(b"<< ~Remove", (98, 72, 40), ID_REMOVE),
        book::label(b"Associated types", (142, 106, 90)),
        listbox((142, 60, 88, 44), ID_CURRENT),
        book::label(b"Enter new name:", (6, 46, 88)),
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 34, 84, 8), ID_NEWNAME),
        button(b"A~dd >>", (98, 32, 40), ID_ADDNAME),
        button(b"Re~move", (98, 14, 40), ID_REMOVENAME),
        book::label(b"Associated names", (142, 46, 90)),
        listbox((142, 2, 88, 42), ID_NAMES),
    ], b"~Association\0")
}

// ---- WPProgram: the Session page ----

const SESSIONS: [(u16, &[u8], u32); 7] = [
    (840, b"~OS/2 full screen", crate::launch::PROG_FULLSCREEN),
    (841, b"OS/2 ~window", crate::launch::PROG_WINDOWABLEVIO),
    (842, b"DOS ~full screen", 4),
    (843, b"DOS wi~ndow", 7),
    // Win-OS/2 (PROG_31_ENHSEAMLESSCOMMON, PROG_31_ENHSEAMLESSVDM,
    // PROG_31_ENH): seamless in the common session, in one of its own,
    // full screen.
    (845, b"WIN-OS/2 window (~seamless)", 18),
    (846, b"WIN-OS/2 window, se~parate session", 17),
    (847, b"WIN-OS/2 full scr~een", 19),
];

fn progc(o: usize) -> u32 {
    let mut buf = alloc::vec![0u8; 2048];
    let mut cb = buf.len() as u32;
    if wps::call("WPProgram", m::WPProgram::wpQueryProgDetails, &[o, buf.as_mut_ptr() as usize, &mut cb as *mut u32 as usize]) == 0 {
        return 0;
    }
    u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]])
}

extern "C" fn session_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = init_obj(h, b);
            let c = progc(o);
            for (id, _, pc) in SESSIONS {
                check(h, id, c == pc);
                if c == crate::launch::PROG_PM {
                    unsafe { WinEnableWindow(WinWindowFromID(h, id as u32), 0) };
                }
            }
            if c == crate::launch::PROG_PM {
                set_item_text(h, 844, b"A Presentation Manager program runs in its own windows.");
            }
            0
        }
        WM_CONTROL if (a >> 16) == 1 => {
            let id = (a & 0xffff) as u16;
            if let Some(&(_, _, pc)) = SESSIONS.iter().find(|s| s.0 == id) {
                // PROGDETAILS with only the type changed.
                let mut pd = [0u32; 18];
                pd[0] = 72;
                pd[1] = pc;
                wps::call("WPProgram", m::WPProgram::wpSetProgDetails, &[page_obj(h), pd.as_ptr() as usize]);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddProgramSessionPage(o: usize, nb: u32) -> u32 {
    let mut items: Vec<Item> = SESSIONS.iter().enumerate().map(|(i, &(id, t, _))| radio(t, (12, 92 - 11 * i as i16, 200), id, i == 0)).collect();
    items.insert(0, book::label(b"Session", (6, 106, 220)));
    items.push(static_text(b"", (6, 6, 224), 844));
    add_page(o, nb, session_proc, items, b"~Session\0")
}

// ---- WPDisk: the Details page ----

extern "C" fn details_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let o = init_obj(h, b);
            let d = wps::call("WPDisk", m::WPDisk::wpQueryLogicalDrive, &[o]) as u32;
            let mut alloc_ = [0u8; 18];
            let mut info = [0u8; 20];
            let ok = unsafe { os2::DosQueryFSInfo(d, 1, alloc_.as_mut_ptr(), 18) } == 0;
            let have_info = unsafe { os2::DosQueryFSInfo(d, 2, info.as_mut_ptr(), 20) } == 0;
            let fs = crate::icons::fs_name(d).unwrap_or_else(|| b"Unknown type".to_vec());
            let r = |i: usize| u32::from_le_bytes([alloc_[i], alloc_[i + 1], alloc_[i + 2], alloc_[i + 3]]) as u64;
            let (spu, units, avail, bps) = (r(4), r(8), r(12), u16::from_le_bytes([alloc_[16], alloc_[17]]) as u64);
            let label = if have_info && info[4] > 0 { info[5..5 + (info[4] as usize).min(11)].to_vec() } else { b"No label".to_vec() };
            let serial = if have_info { u32::from_le_bytes([info[0], info[1], info[2], info[3]]) } else { 0 };
            let unk = || b"Unknown".to_vec();
            let vals: [Vec<u8>; 8] = [
                fs,
                label,
                if serial != 0 { alloc::format!("{:04X}-{:04X}", serial >> 16, serial & 0xffff).into_bytes() } else { b"No serial number".to_vec() },
                if ok { alloc::format!("{units}").into_bytes() } else { unk() },
                if ok { alloc::format!("{avail}").into_bytes() } else { unk() },
                if ok { alloc::format!("{}", spu * bps).into_bytes() } else { unk() },
                if ok { alloc::format!("{}", units * spu * bps).into_bytes() } else { unk() },
                if ok { alloc::format!("{}", avail * spu * bps).into_bytes() } else { unk() },
            ];
            for (i, v) in vals.iter().enumerate() {
                set_item_text(h, 850 + i as u32, v);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn disk_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPDisk"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    wps::call("WPDisk", m::WPDisk::wpAddDiskDetailsPage, &[o, nb as usize]);
    1
}

extern "C" fn wpAddDiskDetailsPage(o: usize, nb: u32) -> u32 {
    let names: [&[u8]; 8] = [
        b"File system type", b"Volume label", b"Volume serial number", b"Total allocation units", b"Available allocation units",
        b"Bytes in each allocation unit", b"Total bytes on disk", b"Available bytes on disk",
    ];
    let mut items = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let y = 104 - 12 * i as i16;
        items.push(book::label(n, (6, y, 118)));
        items.push(static_text(b"", (128, y, 104), 850 + i as u16));
    }
    add_page(o, nb, details_proc, items, b"~Details\0")
}

methods! {
    __m_wpAddObjectWindowPage => wpAddObjectWindowPage, 2;
    __m_no_window_page => no_window_page, 2;
    __m_wpAddFile1Page => wpAddFile1Page, 2;
    __m_wpAddFile2Page => wpAddFile2Page, 2;
    __m_wpAddFile3Page => wpAddFile3Page, 2;
    __m_wpQueryType => wpQueryType, 1;
    __m_wpSetType => wpSetType, 3;
    __m_wpAddFileTypePage => wpAddFileTypePage, 2;
    __m_wpQueryAssociationType => wpQueryAssociationType, 1;
    __m_wpQueryAssociationFilter => wpQueryAssociationFilter, 1;
    __m_wpSetAssociationType => wpSetAssociationType, 2;
    __m_wpSetAssociationFilter => wpSetAssociationFilter, 2;
    __m_wpAddProgramAssociationPage => wpAddProgramAssociationPage, 2;
    __m_wpAddProgramSessionPage => wpAddProgramSessionPage, 2;
    __m_wpAddDiskDetailsPage => wpAddDiskDetailsPage, 2;
    __m_fs_add_pages => fs_add_pages, 2;
    __m_df_add_pages => df_add_pages, 2;
    __m_pf_add_pages => pf_add_pages, 2;
    __m_disk_add_pages => disk_add_pages, 2;
}

pub fn object_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddObjectWindowPage", __m_wpAddObjectWindowPage as usize)]
}

/// For classes without a Window page.
pub fn no_window_page_override() -> (&'static str, usize) {
    ("wpAddObjectWindowPage", __m_no_window_page as usize)
}

pub fn file_system_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
        ("wpAddFile1Page", __m_wpAddFile1Page as usize),
        ("wpAddFile2Page", __m_wpAddFile2Page as usize),
        ("wpAddFile3Page", __m_wpAddFile3Page as usize),
        ("wpQueryType", __m_wpQueryType as usize),
        ("wpSetType", __m_wpSetType as usize),
    ]
}

pub fn file_system_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddSettingsPages", __m_fs_add_pages as usize)]
}

pub fn data_file_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddSettingsPages", __m_df_add_pages as usize)]
}

pub fn program_file_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddSettingsPages", __m_pf_add_pages as usize)]
}

pub fn disk_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddSettingsPages", __m_disk_add_pages as usize)]
}

pub fn data_file_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddFileTypePage", __m_wpAddFileTypePage as usize)]
}

/// WPProgram's and WPProgramFile's (both introduce them).
pub fn program_methods(session: bool) -> Vec<(&'static str, usize)> {
    let mut v = alloc::vec![
        ("wpQueryAssociationType", __m_wpQueryAssociationType as usize),
        ("wpQueryAssociationFilter", __m_wpQueryAssociationFilter as usize),
        ("wpSetAssociationType", __m_wpSetAssociationType as usize),
        ("wpSetAssociationFilter", __m_wpSetAssociationFilter as usize),
        ("wpAddProgramAssociationPage", __m_wpAddProgramAssociationPage as usize),
    ];
    if session {
        v.push(("wpAddProgramSessionPage", __m_wpAddProgramSessionPage as usize));
    }
    v
}

pub fn disk_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddDiskDetailsPage", __m_wpAddDiskDetailsPage as usize)]
}

