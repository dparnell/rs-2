//! The help window's menu bar and the services behind it, as IBM's help
//! manager has them (its menus and dialogs are in HPMGRMRI.DLL: menu 101
//! for help windows, 102 for books, dialog 20001 Bookmark; strings 6001
//! on):
//!
//! - Services: Search, Print (print.rs; disabled without a printer),
//!   Bookmark (books),
//!   New window, Copy (the panel's text to the clipboard), Copy to file
//!   and Append to file (`text.tmp` in the current directory, as IBM's
//!   WriteToFile), Exit (books).
//! - Options: expanding and collapsing the contents, Contents, Index
//!   (books), Viewed pages, Libraries, Previous.
//! - Help: Help index, General help, Using help, Keys help, Product
//!   information (books).
//!
//! Bookmarks belong to books: a list of names and panels kept beside the
//! book in `NAME.cp` (in the first directory of BOOKMARK if it is set,
//! else the book's), as IBM's IOSaveCPBookMark writes it: a 14-byte
//! header (its size, the list's offset, the count) and for each mark the
//! panel's TOC index, the name's length and the name with its NUL.

use crate::pm::*;
use alloc::vec;
use alloc::vec::Vec;
use ipf::layout::{Page, RunKind};
use os2::dos::*;

unsafe extern "C" {
    #[link_name = "PMWIN.907"]
    fn WinCreateMenu(parent: u32, pmt: *const u8) -> u32;
    #[link_name = "PMWIN.713"]
    fn WinCreateAccelTable(hab: u32, pacct: *const u8) -> u32;
    #[link_name = "PMWIN.850"]
    fn WinSetAccelTable(hab: u32, haccel: u32, frame: u32) -> u32;
    #[link_name = "PMWIN.793"]
    fn WinOpenClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.733"]
    fn WinEmptyClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.854"]
    fn WinSetClipbrdData(hab: u32, data: u32, fmt: u32, fl: u32) -> u32;
    #[link_name = "PMWIN.707"]
    fn WinCloseClipbrd(hab: u32) -> u32;
    #[link_name = "PMWIN.922"]
    fn WinCreateDlg(parent: u32, owner: u32, proc_: WndProc, pdlgt: *const u8, params: u32) -> u32;
    #[link_name = "PMWIN.796"]
    fn WinProcessDlg(h: u32) -> u32;
    #[link_name = "PMWIN.729"]
    fn WinDismissDlg(h: u32, r: u32) -> u32;
    #[link_name = "PMWIN.910"]
    fn WinDefDlgProc(h: u32, m: u32, a: u32, b: u32) -> u32;
}

// The menu's commands (IBM's ids).
pub const IDM_SEARCH: u16 = 32513;
pub const IDM_PRINT: u16 = 32514;
pub const IDM_BOOKMARK: u16 = 32515;
pub const IDM_EXIT: u16 = 32516;
pub const IDM_NEWWINDOW: u16 = 32517;
pub const IDM_COPY: u16 = 32529;
pub const IDM_COPYFILE: u16 = 32530;
pub const IDM_APPENDFILE: u16 = 32531;
pub const IDM_LIBRARIES: u16 = 32532;
pub const IDM_VIEWED: u16 = 32545;
pub const IDM_INDEX: u16 = 32546;
pub const IDM_CONTENTS: u16 = 32547;
pub const IDM_EXPAND1: u16 = 32548;
pub const IDM_EXPANDBRANCH: u16 = 32549;
pub const IDM_EXPANDALL: u16 = 32550;
pub const IDM_COLLAPSEBRANCH: u16 = 32551;
pub const IDM_COLLAPSEALL: u16 = 32552;
pub const IDM_PREVIOUS: u16 = 32553;
pub const IDM_USINGHELP: u16 = 32561;
pub const IDM_GENERALHELP: u16 = 32562;
pub const IDM_KEYSHELP: u16 = 32563;
pub const IDM_HELPINDEX: u16 = 32564;
pub const IDM_PRODUCTINFO: u16 = 32565;
pub const IDM_BOOKKEYSHELP: u16 = 32566;

const MIS_TEXT: u16 = 0x0001;
const MIS_SEPARATOR: u16 = 0x0004;
const MIS_SUBMENU: u16 = 0x0010;
const MIA_DISABLED: u16 = 0x4000;

enum M {
    Item(u16, &'static [u8], u16),
    Sep,
    Sub(u16, &'static [u8], Vec<M>),
}

/// A menu template (cb, version, code page, reserved, count, the items).
fn template(items: &[M]) -> Vec<u8> {
    let mut b = vec![0u8; 10];
    b[4..6].copy_from_slice(&437u16.to_le_bytes());
    b[8..10].copy_from_slice(&(items.len() as u16).to_le_bytes());
    let item = |b: &mut Vec<u8>, style: u16, attr: u16, id: u16| {
        for v in [style, attr, id] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    };
    for m in items {
        match m {
            M::Item(id, text, attr) => {
                item(&mut b, MIS_TEXT, *attr, *id);
                b.extend_from_slice(text);
                b.push(0);
            }
            M::Sep => item(&mut b, MIS_SEPARATOR, 0, 0xffff),
            M::Sub(id, text, sub) => {
                item(&mut b, MIS_TEXT | MIS_SUBMENU, 0, *id);
                b.extend_from_slice(text);
                b.push(0);
                b.extend_from_slice(&template(sub));
            }
        }
    }
    let n = b.len() as u16;
    b[0..2].copy_from_slice(&n.to_le_bytes());
    b
}

/// IBM's menu 101 (help windows) or 102 (books).
fn menu(book: bool) -> Vec<M> {
    // Print needs a printer (print.rs).
    let print = if crate::print::default_printer().is_some() { 0 } else { MIA_DISABLED };
    let mut services = vec![M::Item(IDM_SEARCH, b"~Search...\tCtrl+S", 0), M::Item(IDM_PRINT, b"~Print...", print)];
    if book {
        services.push(M::Item(IDM_BOOKMARK, b"~Bookmark...\tCtrl+B", 0));
    }
    services.extend([
        M::Sep,
        M::Item(IDM_NEWWINDOW, b"~New window\tCtrl+N", 0),
        M::Sep,
        M::Item(IDM_COPY, b"C~opy\tCtrl+Ins", 0),
        M::Sep,
        M::Item(IDM_COPYFILE, b"~Copy to file\tCtrl+F", 0),
        M::Item(IDM_APPENDFILE, b"~Append to file\tCtrl+A", 0),
    ]);
    if book {
        services.extend([M::Sep, M::Item(IDM_EXIT, b"E~xit \tF3", 0)]);
    }
    let mut options = vec![
        M::Item(IDM_EXPAND1, b"E~xpand one level\t+", 0),
        M::Item(IDM_EXPANDBRANCH, b"Expand ~branch\t*", 0),
        M::Item(IDM_EXPANDALL, b"Expand ~all\tCtrl+*", 0),
        M::Item(IDM_COLLAPSEBRANCH, b"~Collapse branch\t-", 0),
        M::Item(IDM_COLLAPSEALL, b"C~ollapse all\tCtrl+-", 0),
        M::Sep,
        M::Item(IDM_CONTENTS, b"Con~tents\tCtrl+C", 0),
        M::Sep,
    ];
    if book {
        options.extend([M::Item(IDM_INDEX, b"~Index\tCtrl+I", 0), M::Sep]);
    }
    options.extend([M::Item(IDM_VIEWED, b"~Viewed pages\tCtrl+H", 0), M::Sep, M::Item(IDM_LIBRARIES, b"~Libraries\tCtrl+L", 0), M::Sep, M::Item(IDM_PREVIOUS, b"~Previous", 0)]);
    let help = if book {
        vec![
            M::Item(IDM_HELPINDEX, b"Help ~index", 0),
            M::Item(IDM_GENERALHELP, b"~General help", 0),
            M::Item(IDM_USINGHELP, b"~Using help", 0),
            M::Item(IDM_BOOKKEYSHELP, b"~Keys help", 0),
            M::Sep,
            M::Item(IDM_PRODUCTINFO, b"~Product information", 0),
        ]
    } else {
        vec![
            M::Item(IDM_HELPINDEX, b"Help ~index\tF11", 0),
            M::Item(IDM_GENERALHELP, b"~General help\tF2", 0),
            M::Item(IDM_USINGHELP, b"~Using help\tShift+F10", 0),
            M::Item(IDM_KEYSHELP, b"~Keys help\tF9", 0),
        ]
    };
    vec![M::Sub(32512, b"Ser~vices", services), M::Sub(32544, b"~Options", options), M::Sub(32560, b"~Help", help)]
}

const AF_CHAR: u16 = 0x0001;
const AF_VIRTUALKEY: u16 = 0x0002;
const AF_SHIFT: u16 = 0x0008;
const AF_CONTROL: u16 = 0x0010;
const VK_INSERT: u16 = 0x09;
const VK_F2: u16 = 0x21;
const VK_F3: u16 = 0x22;
const VK_F9: u16 = 0x28;
const VK_F10: u16 = 0x29;
const VK_F11: u16 = 0x2a;

/// The keys the menu shows.
fn accels(book: bool) -> Vec<u8> {
    let ctrl = AF_CHAR | AF_CONTROL;
    let mut a: Vec<(u16, u16, u16)> = vec![
        (ctrl, b's' as u16, IDM_SEARCH),
        (ctrl, b'n' as u16, IDM_NEWWINDOW),
        (AF_VIRTUALKEY | AF_CONTROL, VK_INSERT, IDM_COPY),
        (ctrl, b'f' as u16, IDM_COPYFILE),
        (ctrl, b'a' as u16, IDM_APPENDFILE),
        (ctrl, b'*' as u16, IDM_EXPANDALL),
        (ctrl, b'-' as u16, IDM_COLLAPSEALL),
        (ctrl, b'c' as u16, IDM_CONTENTS),
        (ctrl, b'h' as u16, IDM_VIEWED),
        (ctrl, b'l' as u16, IDM_LIBRARIES),
    ];
    if book {
        a.extend([(ctrl, b'b' as u16, IDM_BOOKMARK), (ctrl, b'i' as u16, IDM_INDEX), (AF_VIRTUALKEY, VK_F3, IDM_EXIT)]);
    } else {
        a.extend([
            (AF_VIRTUALKEY, VK_F11, IDM_HELPINDEX),
            (AF_VIRTUALKEY, VK_F2, IDM_GENERALHELP),
            (AF_VIRTUALKEY | AF_SHIFT, VK_F10, IDM_USINGHELP),
            (AF_VIRTUALKEY, VK_F9, IDM_KEYSHELP),
        ]);
    }
    let mut b = Vec::new();
    b.extend_from_slice(&(a.len() as u16).to_le_bytes());
    b.extend_from_slice(&437u16.to_le_bytes());
    for (fs, key, cmd) in a {
        for v in [fs, key, cmd] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    b
}

/// Give a help window its menu bar and keys.
pub fn attach_menu(hab: u32, frame: u32, book: bool) {
    let t = template(&menu(book));
    unsafe {
        WinCreateMenu(frame, t.as_ptr());
        let a = accels(book);
        let h = WinCreateAccelTable(hab, a.as_ptr());
        WinSetAccelTable(hab, h, frame);
        WinSendMsg(frame, WM_UPDATEFRAME, FCF_MENU, 0);
    }
}

// ---- Copying ----

/// The text of a laid-out page, words placed by columns of `cw` pixels
/// (IBM's LoadMarkedText), lines ending in CR LF.
pub fn page_text(page: &Page, cw: i32) -> Vec<u8> {
    let mut words: Vec<(i32, i32, i32, i32, &[u8])> = page.runs.iter().filter_map(|r| if let RunKind::Word(w) = r.kind { Some((r.y, r.x, r.height, r.width, w)) } else { None }).collect();
    words.sort_by_key(|w| (w.0, w.1));
    let cw = cw.max(1);
    let mut out = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    let mut last: Option<(i32, i32)> = None;
    // Where the last word on the line ended (pixels).
    let mut end = 0;
    for (y, x, h, width, w) in words {
        if let Some((ly, lh)) = last {
            if y != ly {
                out.extend_from_slice(&line);
                out.extend_from_slice(b"\r\n");
                line.clear();
                // A gap of more than a line: an empty one.
                if y - ly > lh * 3 / 2 {
                    out.extend_from_slice(b"\r\n");
                }
                end = 0;
            }
        }
        // A word touching the last one (punctuation) follows it.
        let col = (x / cw) as usize;
        if line.is_empty() || x > end + 1 {
            if line.len() < col {
                line.resize(col, b' ');
            } else if !line.is_empty() {
                line.push(b' ');
            }
        }
        line.extend_from_slice(w);
        end = x + width;
        last = Some((y, h.max(1)));
    }
    if !line.is_empty() {
        out.extend_from_slice(&line);
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// The text on the clipboard (CF_TEXT).
pub fn copy_to_clipboard(hab: u32, text: &[u8]) -> bool {
    let mut p: *mut u8 = core::ptr::null_mut();
    // PAG_READ | PAG_WRITE | PAG_COMMIT | OBJ_GIVEABLE
    if unsafe { DosAllocSharedMem(&mut p, core::ptr::null(), text.len() as u32 + 1, 0x213) } != 0 {
        return false;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), p, text.len());
        *p.add(text.len()) = 0;
        if WinOpenClipbrd(hab) == 0 {
            DosFreeMem(p);
            return false;
        }
        WinEmptyClipbrd(hab);
        // CF_TEXT, CFI_POINTER
        let ok = WinSetClipbrdData(hab, p as u32, 1, 0x400) != 0;
        WinCloseClipbrd(hab);
        ok
    }
}

/// IBM's file for Copy to file and Append to file (string 6001).
const TEXT_FILE: &[u8] = b"text.tmp\0";

/// The text to `text.tmp` in the current directory: replacing it, or at
/// its end.
pub fn copy_to_file(text: &[u8], append: bool) -> bool {
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS (append) or
    // REPLACE_IF_EXISTS; write, deny none.
    let flags = if append { 0x11 } else { 0x12 };
    if unsafe { DosOpen(TEXT_FILE.as_ptr(), &mut hf, &mut action, 0, 0, flags, 0x42, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut pos = 0u32;
    let mut done = 0u32;
    let ok = unsafe { DosSetFilePtr(hf, 0, 2, &mut pos) == 0 && DosWrite(hf, text.as_ptr(), text.len() as u32, &mut done) == 0 && done == text.len() as u32 };
    unsafe { DosClose(hf) };
    ok
}

// ---- The library list ----

/// The directories of HELP (help) or BOOKSHELF (books), as IBM's
/// BuildLibraryListItem lists them.
pub fn library_dirs(book: bool) -> Vec<Vec<u8>> {
    let var: &[u8] = if book { b"BOOKSHELF\0" } else { b"HELP\0" };
    let mut p: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(var.as_ptr(), &mut p) } != 0 {
        return Vec::new();
    }
    let s = unsafe { cstr(p) };
    s.split(|&c| c == b';').map(|d| d.trim_ascii()).filter(|d| !d.is_empty()).map(|d| d.to_ascii_uppercase()).collect()
}

/// The help files (books: `.INF`, help: `.HLP`) in a directory.
pub fn library_files(dir: &[u8], book: bool) -> Vec<Vec<u8>> {
    let mut d = dir.to_vec();
    if d.last() != Some(&b'\\') {
        d.push(b'\\');
    }
    let mut pat = d.clone();
    pat.extend_from_slice(if book { b"*.INF\0" } else { b"*.HLP\0" });
    let mut out = Vec::new();
    let mut h = 0xffff_ffffu32;
    let mut buf = [0u8; 300];
    let mut n = 1u32;
    // FILEFINDBUF3: name length at 0x1A, name at 0x1B.
    if unsafe { DosFindFirst(pat.as_ptr(), &mut h, 0x21, buf.as_mut_ptr(), buf.len() as u32, &mut n, 1) } != 0 {
        return out;
    }
    loop {
        let len = buf[0x1a] as usize;
        let mut f = d.clone();
        f.extend_from_slice(&buf[0x1b..0x1b + len]);
        out.push(f);
        n = 1;
        if unsafe { DosFindNext(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 {
            break;
        }
    }
    unsafe { DosFindClose(h) };
    out
}

// ---- Bookmarks ----

/// A bookmark: its name and the panel (TOC index in its book).
#[derive(Clone)]
pub struct Mark {
    pub toc: u16,
    pub name: Vec<u8>,
}

/// `NAME.cp` for book `path` (IBM's MscGenerateCPFileName).
fn cp_file(path: &[u8]) -> Vec<u8> {
    let cut = path.iter().rposition(|&c| c == b'\\' || c == b':').map_or(0, |p| p + 1);
    let (dir, file) = path.split_at(cut);
    let stem = file.split(|&c| c == b'.').next().unwrap_or(file);
    let mut p: *const u8 = core::ptr::null();
    let mut out = if unsafe { DosScanEnv(b"BOOKMARK\0".as_ptr(), &mut p) } == 0 {
        let s = unsafe { cstr(p) };
        let first = s.split(|&c| c == b';' || c == b' ').find(|d| !d.is_empty()).unwrap_or(b"");
        let mut d = first.to_vec();
        if !d.is_empty() && d.last() != Some(&b'\\') {
            d.push(b'\\');
        }
        d
    } else {
        dir.to_vec()
    };
    out.extend_from_slice(stem);
    out.extend_from_slice(b".cp");
    out
}

fn read_all(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut action) = (0u32, 0u32);
    if unsafe { DosOpen(p.as_ptr(), &mut hf, &mut action, 0, 0, 0x0001, 0x0040, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut size = 0u32;
    unsafe { DosSetFilePtr(hf, 0, 2, &mut size) };
    unsafe { DosSetFilePtr(hf, 0, 0, &mut 0) };
    let mut buf = vec![0u8; size as usize];
    let mut got = 0u32;
    let rc = unsafe { DosRead(hf, buf.as_mut_ptr(), size, &mut got) };
    unsafe { DosClose(hf) };
    (rc == 0).then(|| {
        buf.truncate(got as usize);
        buf
    })
}

/// The bookmarks kept for book `path` (IBM's IOLoadCPBookMark).
pub fn load_marks(path: &[u8]) -> Vec<Mark> {
    let Some(b) = read_all(&cp_file(path)) else { return Vec::new() };
    let u16_at = |o: usize| b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]));
    let (Some(off), Some(n)) = (b.get(2..6).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize), u16_at(6)) else { return Vec::new() };
    let mut at = off;
    let mut out = Vec::new();
    for _ in 0..n {
        let (Some(toc), Some(len)) = (u16_at(at), u16_at(at + 2)) else { break };
        let Some(name) = b.get(at + 4..at + 4 + len as usize) else { break };
        out.push(Mark { toc, name: name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())].to_vec() });
        at += 4 + len as usize;
    }
    out
}

/// Keep the bookmarks of book `path`; none: the file goes (IBM's
/// MscSaveCPInfo).
pub fn save_marks(path: &[u8], marks: &[Mark]) {
    let f = z(&cp_file(path));
    if marks.is_empty() {
        unsafe { DosDelete(f.as_ptr()) };
        return;
    }
    let mut b = vec![0u8; 14];
    b[0..2].copy_from_slice(&14u16.to_le_bytes());
    b[2..6].copy_from_slice(&14u32.to_le_bytes());
    b[6..8].copy_from_slice(&(marks.len() as u16).to_le_bytes());
    for m in marks {
        b.extend_from_slice(&m.toc.to_le_bytes());
        b.extend_from_slice(&(m.name.len() as u16 + 1).to_le_bytes());
        b.extend_from_slice(&m.name);
        b.push(0);
    }
    let (mut hf, mut action, mut done) = (0u32, 0u32, 0u32);
    if unsafe { DosOpen(f.as_ptr(), &mut hf, &mut action, 0, 0, 0x12, 0x12, core::ptr::null_mut()) } == 0 {
        unsafe {
            DosWrite(hf, b.as_ptr(), b.len() as u32, &mut done);
            DosClose(hf);
        }
    }
}

// ---- The Bookmark dialog (IBM's dialog 20001, BMBookMarkDlgProc) ----

const ID_NAME: u32 = 1000;
const ID_HELP: u32 = 1005;
const ID_OK: u32 = 1006;
const ID_PLACE: u32 = 1012;
const ID_VIEW: u32 = 1013;
const ID_REMOVE: u32 = 1014;
const ID_REMOVEALL: u32 = 1015;
const ID_LIST: u32 = 1016;
const DID_CANCEL: u32 = 2;

const WM_INITDLG: u32 = 0x003b;
const BM_SETCHECK: u32 = 0x0125;
const BN_CLICKED: u32 = 1;
const MB_OKCANCEL: u32 = 0x0001;
const MB_QUERY: u32 = 0x0030;
const MB_MOVEABLE: u32 = 0x4000;
const MBID_CANCEL: u32 = 2;

/// One control of a dialog template: class (WC_* number), text, style,
/// position and size (dialog units), id.
pub(crate) struct Ctl(pub u16, pub &'static [u8], pub u32, pub [i16; 4], pub u16);

/// A dialog template (as `parse_dialog` reads it): the frame, then its
/// controls; FCF flags as the frame's control data.
pub(crate) fn dialog(title: &[u8], frame: Ctl, fcf: u32, ctls: &[Ctl]) -> Vec<u8> {
    let n = ctls.len() + 1;
    let head = 14;
    let mut b = vec![0u8; head + 30 * n];
    b[2..4].copy_from_slice(&0u16.to_le_bytes());
    b[4..6].copy_from_slice(&437u16.to_le_bytes());
    b[6..8].copy_from_slice(&(head as u16).to_le_bytes());
    b[8..10].copy_from_slice(&1u16.to_le_bytes());
    b[10..12].copy_from_slice(&0xffffu16.to_le_bytes());
    let all: Vec<(&Ctl, &[u8])> = core::iter::once((&frame, title)).chain(ctls.iter().map(|c| (c, c.1))).collect();
    for (i, (c, text)) in all.iter().enumerate() {
        let at = head + 30 * i;
        let off = b.len() as u16;
        b.extend_from_slice(text);
        b.push(0);
        let ctl = if i == 0 {
            let o = b.len() as u16;
            b.extend_from_slice(&fcf.to_le_bytes());
            o
        } else {
            0xffff
        };
        let mut w = |o: usize, v: u16| b[at + o..at + o + 2].copy_from_slice(&v.to_le_bytes());
        w(2, if i == 0 { ctls.len() as u16 } else { 0 });
        w(6, c.0);
        w(8, text.len() as u16 + 1);
        w(10, off);
        for (k, v) in c.3.iter().enumerate() {
            w(16 + 2 * k, *v as u16);
        }
        w(24, c.4);
        w(26, 0xffff);
        w(28, ctl);
        b[at + 12..at + 16].copy_from_slice(&c.2.to_le_bytes());
    }
    let cb = b.len() as u16;
    b[0..2].copy_from_slice(&cb.to_le_bytes());
    b
}

/// What the Bookmark dialog works on.
pub struct BookmarkDlg<'a> {
    pub marks: &'a mut Vec<(usize, Mark)>,
    /// The panel shown (library, TOC index) and its title.
    pub current: Option<(usize, u16, Vec<u8>)>,
    /// The bookmark to show (View), set by the dialog.
    pub view: Option<(usize, u16)>,
    choice: u32,
}

impl<'a> BookmarkDlg<'a> {
    pub fn new(marks: &'a mut Vec<(usize, Mark)>, current: Option<(usize, u16, Vec<u8>)>) -> BookmarkDlg<'a> {
        BookmarkDlg { marks, current, view: None, choice: 0 }
    }
}

/// Run the Bookmark dialog over `owner`.
pub fn bookmark_dialog(owner: u32, d: &mut BookmarkDlg) {
    const RADIO: u32 = 0x8000_0004;
    let t = dialog(
        b"Bookmark",
        Ctl(1, b"", 0x9400_0090, [41, 21, 292, 114], 20001),
        FCF_TITLEBAR | FCF_SYSMENU,
        &[
            Ctl(3, b"~Place", RADIO | WS_GROUP | WS_TABSTOP, [17, 89, 43, 10], ID_PLACE as u16),
            Ctl(3, b"~View", RADIO, [17, 60, 32, 10], ID_VIEW as u16),
            Ctl(3, b"~Remove", RADIO, [17, 46, 57, 10], ID_REMOVE as u16),
            Ctl(3, b"Remove ~all", RADIO, [17, 33, 64, 10], ID_REMOVEALL as u16),
            Ctl(6, b"", 0x8003_000c, [81, 89, 201, 9], ID_NAME as u16),
            Ctl(7, b"", 0x8003_0008, [85, 31, 198, 43], ID_LIST as u16),
            Ctl(3, b"OK", 0x8003_0400, [15, 6, 55, 12], ID_OK as u16),
            Ctl(3, b"Cancel", 0x8000_0000, [105, 6, 55, 12], DID_CANCEL as u16),
            Ctl(3, b"Help", 0x8000_0900, [195, 6, 55, 12], ID_HELP as u16),
        ],
    );
    let h = unsafe { WinCreateDlg(HWND_DESKTOP, owner, bookmark_proc, t.as_ptr(), d as *mut BookmarkDlg as u32) };
    if h != 0 {
        unsafe {
            WinProcessDlg(h);
            WinDestroyWindow(h);
        }
    }
}

fn dlg_of(h: u32) -> Option<&'static mut BookmarkDlg<'static>> {
    let p = unsafe { WinQueryWindowULong(h, QWL_USER) } as *mut BookmarkDlg<'static>;
    unsafe { p.as_mut() }
}

fn item(h: u32, id: u32) -> u32 {
    unsafe { WinWindowFromID(h, id) }
}

fn check(h: u32, id: u32, d: &mut BookmarkDlg) {
    send(item(h, id), BM_SETCHECK, 1, 0);
    d.choice = id;
}

extern "C" fn bookmark_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    crate::stack::deep(|| bookmark_proc_(h, msg, mp1, mp2))
}

fn bookmark_proc_(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == WM_INITDLG {
        unsafe { WinSetWindowULong(h, QWL_USER, mp2) };
        let Some(d) = dlg_of(h) else { return 0 };
        send(item(h, ID_NAME), EM_SETTEXTLIMIT, 0xfa, 0);
        if let Some((_, _, title)) = &d.current {
            let t = z(title);
            unsafe { WinSetWindowText(item(h, ID_NAME), t.as_ptr()) };
            check(h, ID_PLACE, d);
        } else {
            unsafe {
                WinEnableWindow(item(h, ID_PLACE), 0);
                WinEnableWindow(item(h, ID_NAME), 0);
            }
            check(h, ID_VIEW, d);
        }
        if d.marks.is_empty() {
            for id in [ID_VIEW, ID_REMOVE, ID_REMOVEALL] {
                unsafe { WinEnableWindow(item(h, id), 0) };
            }
        } else {
            let list = item(h, ID_LIST);
            for (_, m) in d.marks.iter() {
                let t = z(&m.name);
                send(list, LM_INSERTITEM, LIT_END, t.as_ptr() as u32);
            }
            send(list, LM_SELECTITEM, 0, 1);
        }
        // The focus on the choice (IBM clicks it).
        unsafe { WinSetFocus(HWND_DESKTOP, item(h, d.choice)) };
        return 1;
    }
    let Some(d) = dlg_of(h) else { return unsafe { WinDefDlgProc(h, msg, mp1, mp2) } };
    match msg {
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            if code == BN_CLICKED && (ID_PLACE..=ID_REMOVEALL).contains(&id) {
                d.choice = id;
            } else if id == ID_LIST && code == LN_ENTER && d.choice != ID_PLACE && d.choice != ID_REMOVEALL && process(h, d) == 2 {
                unsafe { WinDismissDlg(h, 1) };
            }
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                ID_OK => {
                    if d.choice == ID_PLACE {
                        let mut buf = [0u8; 256];
                        let n = unsafe { WinQueryWindowText(item(h, ID_NAME), 255, buf.as_mut_ptr()) }.max(0) as usize;
                        if n == 0 {
                            unsafe {
                                WinMessageBox(HWND_DESKTOP, h, b"Invalid bookmark string!\0".as_ptr(), b"Bookmark\0".as_ptr(), 0, MB_MOVEABLE);
                                WinSetFocus(HWND_DESKTOP, item(h, ID_NAME));
                            }
                            return 0;
                        }
                        if let Some((lib, toc, _)) = d.current {
                            d.marks.push((lib, Mark { toc, name: buf[..n].to_vec() }));
                        }
                    } else if unsafe { crate::pm::WinIsWindowEnabled(item(h, ID_VIEW)) } != 0 && process(h, d) == 3 {
                        return 0;
                    }
                    unsafe { WinDismissDlg(h, 1) };
                }
                DID_CANCEL => unsafe {
                    WinDismissDlg(h, 0);
                },
                _ => {}
            }
            0
        }
        // Help on the dialog: the help window's own help.
        WM_HELP => 0,
        _ => unsafe { WinDefDlgProc(h, msg, mp1, mp2) },
    }
}

/// View, Remove or Remove all on the selected bookmark (IBM's
/// BMProcessEnter): 2 when done, 1 to go on, 3 when the user changed
/// their mind.
fn process(h: u32, d: &mut BookmarkDlg) -> u32 {
    let list = item(h, ID_LIST);
    let i = send(list, LM_QUERYSELECTION, LIT_FIRST, 0) as i16 as i32;
    let i = i.max(0) as usize;
    match d.choice {
        ID_VIEW => {
            if let Some((lib, m)) = d.marks.get(i) {
                d.view = Some((*lib, m.toc));
            }
            2
        }
        ID_REMOVE => {
            if i < d.marks.len() {
                d.marks.remove(i);
                send(list, LM_DELETEITEM, i as u32, 0);
            }
            if d.marks.is_empty() {
                return 2;
            }
            send(list, LM_SELECTITEM, i.min(d.marks.len() - 1) as u32, 1);
            1
        }
        ID_REMOVEALL => {
            let r = unsafe {
                WinMessageBox(HWND_DESKTOP, h, b"Are you sure you want to delete all bookmarks?\0".as_ptr(), b"Delete All Bookmarks\0".as_ptr(), 1, MB_MOVEABLE | MB_QUERY | MB_OKCANCEL)
            };
            if r == MBID_CANCEL {
                return 3;
            }
            send(list, LM_DELETEALL, 0, 0);
            d.marks.clear();
            2
        }
        _ => 1,
    }
}
