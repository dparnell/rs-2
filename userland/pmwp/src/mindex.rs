//! Mindex: the Help Index and Glossary objects (IBM's MINXOBJ.DLL, whose
//! class rs2's PMWP provides). Setup `INDEX=NAME` names an environment
//! variable: the directories on it (`HELP`: the help files,
//! `GLOSSARY`: `\OS2\HELP\GLOSS`) hold the help files (`*.HLP` that start
//! with "HS") whose indexes merge into one; without such a variable,
//! NAME is the help files themselves, separated by `+` (as
//! GetGlobalIndexFileNames in IBM's MINXOBJ.DLL does it). Opening the
//! object shows the merged index, sorted, with a search field; an entry
//! that several panels share gets the panel's title after it (IBM's
//! MindexAppendDupIndexTitle). Choosing an entry shows its panel in a
//! help window (a help instance for its file and HM_SHOW_TOC, IBM's
//! private message for a TOC entry). Second-level entries show indented
//! under theirs; letter tabs down the right (as on IBM's notebook) jump
//! to the first entry with that letter.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OPEN_SETTINGS};
use crate::pm::*;
use crate::som;
use crate::util::*;
use crate::util::{cstr, z};
use crate::wps::{self, Impl};

#[repr(C)]
pub struct MindexData {
    pub index: Str,
}

fn md<'a>(obj: usize) -> &'a mut MindexData {
    som::data(wps::ccd("Mindex"), obj)
}

const CLASS: &[u8] = b"WPS_MIndex\0";
const ID_SEARCH: u32 = 10;
const ID_LIST: u32 = 11;
const ID_LABEL: u32 = 12;
const WC_STATIC_W: u32 = 0xffff_0005;
const WC_ENTRYFIELD_W: u32 = 0xffff_0006;
const WC_LISTBOX_W: u32 = 0xffff_0007;
const LM_SETTOPINDEX: u32 = 0x0162;
const EN_CHANGE: u32 = 4;
const HM_SHOW_TOC: u32 = 0x235;
/// The letter tabs ("#": the top) and their width.
const LETTERS: &[u8] = b"#ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const ID_TAB: u32 = 100;
const TAB_W: i32 = 22;
const WC_BUTTON_W: u32 = 0xffff_0003;

unsafe extern "C" {
    #[link_name = "HELPMGR.51"]
    fn WinCreateHelpInstance(hab: u32, init: *mut u32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}

/// An index entry: its text, the help file and the TOC entry.
struct Entry {
    text: Vec<u8>,
    file: usize,
    toc: u16,
    /// A second-level entry (shown indented under the one before it).
    sub: bool,
}

struct IndexWin {
    obj: usize,
    frame: u32,
    client: u32,
    files: Vec<Vec<u8>>,
    /// A help instance per file, made when first needed.
    helps: Vec<u32>,
    entries: Vec<Entry>,
}

static mut WINS: Vec<IndexWin> = Vec::new();

#[allow(static_mut_refs)]
fn wins() -> &'static mut Vec<IndexWin> {
    unsafe { &mut WINS }
}

fn read_all(path: &[u8]) -> Option<Vec<u8>> {
    let p = z(path);
    let (mut hf, mut act) = (0u32, 0u32);
    // Read only, deny write.
    if unsafe { os2::DosOpen(p.as_ptr(), &mut hf, &mut act, 0, 0, 0x01, 0x20, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut v = Vec::new();
    let mut buf = alloc::vec![0u8; 0x10000];
    loop {
        let mut n = 0u32;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        v.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(v)
}

/// The help files an INDEX value names.
fn index_files(index: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let name = z(&index.to_ascii_uppercase());
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(name.as_ptr(), &mut v) } != 0 || v.is_null() {
        // Not a variable: the files themselves, separated by +.
        for f in index.split(|&c| c == b'+').map(|f| f.trim_ascii()).filter(|f| !f.is_empty()) {
            out.push(f.to_vec());
        }
        return out;
    }
    for dir in cstr(v).split(|&c| c == b';').map(|d| d.trim_ascii()).filter(|d| !d.is_empty()) {
        let mut names: Vec<Vec<u8>> = list_dir(dir)
            .into_iter()
            .filter(|(n, attr)| attr & 0x10 == 0 && n.len() > 4 && n[n.len() - 4..].eq_ignore_ascii_case(b".HLP"))
            .map(|(n, _)| n)
            .collect();
        names.sort();
        for n in names {
            out.push(join(dir, &n));
        }
    }
    out
}

/// The merged index of the files (help files only: "HS"), sorted; an
/// entry that several panels share (in any of the files) gets the
/// panel's title after it; second-level entries stay under theirs.
fn merged(files: &mut Vec<Vec<u8>>) -> Vec<Entry> {
    // (entry, its panel's title, its second-level entries)
    let mut tops: Vec<(Entry, Vec<u8>, Vec<Entry>)> = Vec::new();
    let mut kept = Vec::new();
    for path in files.iter() {
        let Some(data) = read_all(path) else { continue };
        if data.len() < 2 || &data[..2] != b"HS" {
            continue;
        }
        let Ok(doc) = ipf::Doc::parse(&data) else { continue };
        let f = kept.len();
        kept.push(path.clone());
        let mut have_top = false;
        for e in doc.index() {
            if e.level() == 1 {
                let title = doc.toc(e.toc).map(|t| t.title).unwrap_or_default();
                tops.push((Entry { text: e.text, file: f, toc: e.toc, sub: false }, title, Vec::new()));
                have_top = true;
            } else if have_top {
                if let Some(t) = tops.last_mut() {
                    t.2.push(Entry { text: e.text, file: f, toc: e.toc, sub: true });
                }
            }
        }
    }
    *files = kept;
    tops.sort_by(|a, b| a.0.text.to_ascii_uppercase().cmp(&b.0.text.to_ascii_uppercase()));
    let n = tops.len();
    let same = |a: &Entry, b: &Entry| a.text.eq_ignore_ascii_case(&b.text);
    let dup: Vec<bool> = (0..n).map(|i| (i > 0 && same(&tops[i - 1].0, &tops[i].0)) || (i + 1 < n && same(&tops[i + 1].0, &tops[i].0))).collect();
    let mut out = Vec::new();
    for ((mut e, title, subs), d) in tops.into_iter().zip(dup) {
        if d && !title.is_empty() {
            e.text.extend_from_slice(b" - ");
            e.text.extend_from_slice(&title);
        }
        out.push(e);
        out.extend(subs);
    }
    out
}

/// The first top entry at or after `t` (upper case) in the sorted list.
fn first_from(entries: &[Entry], t: &[u8], exact: bool) -> Option<usize> {
    entries.iter().position(|e| {
        let u = e.text.to_ascii_uppercase();
        !e.sub && if exact { u.starts_with(t) } else { u.as_slice() >= t }
    })
}

/// Show an entry's panel.
fn show(w: usize, i: usize) {
    let iw = &mut wins()[w];
    let Some(e) = iw.entries.get(i) else { return };
    let f = e.file;
    let toc = e.toc as u32;
    if iw.helps[f] == 0 {
        let lib = z(&iw.files[f]);
        let title = z(&obj::title(iw.obj));
        // HELPINIT: cb, ulReturnCode, pszTutorialName, phtHelpTable,
        // hmodHelpTableModule, hmodAccelActionBarModule, idAccelTable,
        // idActionBar, pszHelpWindowTitle, fShowPanelId,
        // pszHelpLibraryName.
        let mut hi: [u32; 11] = [44, 0, 0, 0, 0, 0, 0, 0, title.as_ptr() as u32, 0, lib.as_ptr() as u32];
        iw.helps[f] = unsafe { WinCreateHelpInstance(WinQueryAnchorBlock(HWND_DESKTOP), hi.as_mut_ptr()) };
    }
    let h = iw.helps[f];
    if h == 0 || send(h, HM_SHOW_TOC, toc, 0) != 0 {
        message(iw.frame, b"The help panel cannot be shown.", b"Index", MB_OK | MB_ERROR);
    }
}

fn find_win(client: u32) -> Option<usize> {
    wins().iter().position(|w| w.client == client)
}

extern "C" fn index_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_SIZE => {
            let (cx, cy) = ((b & 0xffff) as i32, (b >> 16) as i32);
            unsafe {
                WinSetWindowPos(WinWindowFromID(h, ID_LABEL), 0, 6, cy - 26, 60, 18, SWP_SIZE | SWP_MOVE);
                WinSetWindowPos(WinWindowFromID(h, ID_SEARCH), 0, 70, cy - 26, (cx - 78).max(10), 18, SWP_SIZE | SWP_MOVE);
                WinSetWindowPos(WinWindowFromID(h, ID_LIST), 0, 4, 4, (cx - 8 - TAB_W).max(10), (cy - 36).max(10), SWP_SIZE | SWP_MOVE);
                // The letter tabs down the right, as on IBM's notebook.
                let th = ((cy - 36) / LETTERS.len() as i32).max(8);
                for i in 0..LETTERS.len() {
                    let y = cy - 32 - th * (i as i32 + 1);
                    WinSetWindowPos(WinWindowFromID(h, ID_TAB + i as u32), 0, cx - TAB_W - 2, y, TAB_W, th, SWP_SIZE | SWP_MOVE);
                }
            }
            0
        }
        WM_COMMAND if (ID_TAB..ID_TAB + LETTERS.len() as u32).contains(&(a & 0xffff)) => {
            // A letter tab: the first entry from that letter on.
            if let Some(w) = find_win(h) {
                let c = LETTERS[((a & 0xffff) - ID_TAB) as usize];
                let at = if c == b'#' { Some(0) } else { first_from(&wins()[w].entries, &[c], false) };
                if let Some(i) = at {
                    unsafe {
                        WinSendDlgItemMsg(h, ID_LIST, LM_SELECTITEM, i as u32, 1);
                        WinSendDlgItemMsg(h, ID_LIST, LM_SETTOPINDEX, i as u32, 0);
                    }
                }
            }
            0
        }
        WM_COMMAND => 0,
        WM_CONTROL => {
            let (id, code) = (a & 0xffff, a >> 16);
            let Some(w) = find_win(h) else { return 0 };
            if id == ID_SEARCH && code == EN_CHANGE {
                // The first entry that starts with what is typed.
                let t = item_text(h, ID_SEARCH).to_ascii_uppercase();
                if let Some(i) = first_from(&wins()[w].entries, &t, true) {
                    unsafe {
                        WinSendDlgItemMsg(h, ID_LIST, LM_SELECTITEM, i as u32, 1);
                        WinSendDlgItemMsg(h, ID_LIST, LM_SETTOPINDEX, i as u32, 0);
                    }
                }
            } else if id == ID_LIST && code == LN_ENTER {
                let i = unsafe { WinSendDlgItemMsg(h, ID_LIST, LM_QUERYSELECTION, LIT_NONE, 0) } as i32 as i16;
                if i >= 0 {
                    show(w, i as usize);
                }
            }
            0
        }
        WM_CHAR => {
            // Enter in the search field shows the selected entry.
            if a & KC_VIRTUALKEY != 0 && (b >> 16) & 0xff == 0x08 && a & KC_KEYUP == 0 {
                if let Some(w) = find_win(h) {
                    let i = unsafe { WinSendDlgItemMsg(h, ID_LIST, LM_QUERYSELECTION, LIT_NONE, 0) } as i32 as i16;
                    if i >= 0 {
                        show(w, i as usize);
                    }
                }
                return 1;
            }
            unsafe { WinDefWindowProc(h, msg, a, b) }
        }
        WM_PAINT => {
            let mut r = RectL::default();
            unsafe {
                let hps = WinBeginPaint(h, 0, &mut r);
                WinFillRect(hps, &r, 15);
                WinEndPaint(hps);
            }
            0
        }
        WM_CLOSE => {
            if let Some(w) = find_win(h) {
                let iw = wins().remove(w);
                for hh in iw.helps.into_iter().filter(|&x| x != 0) {
                    unsafe { WinDestroyHelpInstance(hh) };
                }
                unsafe { WinDestroyWindow(iw.frame) };
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

unsafe extern "C" {
    #[link_name = "HELPMGR.52"]
    fn WinDestroyHelpInstance(h: u32) -> u32;
}

/// Open the index window (or bring it up).
fn open_index(o: usize) -> u32 {
    if let Some(w) = wins().iter().find(|w| w.obj == o) {
        crate::view::activate(w.frame);
        return w.frame;
    }
    let hab = unsafe { WinQueryAnchorBlock(HWND_DESKTOP) };
    static mut REGISTERED: bool = false;
    unsafe {
        if !REGISTERED {
            WinRegisterClass(hab, CLASS.as_ptr(), index_proc, 0, 0);
            REGISTERED = true;
        }
    }
    let index = str_bytes(&md(o).index).to_vec();
    let mut files = index_files(if index.is_empty() { b"HELP" } else { &index });
    // Reading the help files takes a moment.
    // SPTR_WAIT, then SPTR_ARROW.
    unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, 3, 0)) };
    let entries = merged(&mut files);
    unsafe { WinSetPointer(HWND_DESKTOP, WinQuerySysPointer(HWND_DESKTOP, 1, 0)) };
    let title = z(&obj::title(o).into_iter().map(|c| if c == b'\n' { b' ' } else { c }).collect::<Vec<u8>>());
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST;
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), title.as_ptr(), WS_CLIPCHILDREN, 0, 1, &mut client) };
    if frame == 0 {
        return 0;
    }
    unsafe {
        WinCreateWindow(client, WC_STATIC_W, b"Search:\0".as_ptr(), WS_VISIBLE | 0x0001 | 0x0100, 0, 0, 10, 10, client, HWND_TOP, ID_LABEL, core::ptr::null(), core::ptr::null());
        // ES_AUTOSCROLL | ES_MARGIN
        WinCreateWindow(client, WC_ENTRYFIELD_W, b"\0".as_ptr(), WS_VISIBLE | WS_TABSTOP | 0x0004 | 0x0008, 0, 0, 10, 10, client, HWND_TOP, ID_SEARCH, core::ptr::null(), core::ptr::null());
        // LS_NOADJUSTPOS
        WinCreateWindow(client, WC_LISTBOX_W, b"\0".as_ptr(), WS_VISIBLE | WS_TABSTOP | 0x0004, 0, 0, 10, 10, client, HWND_TOP, ID_LIST, core::ptr::null(), core::ptr::null());
    }
    if entries.is_empty() {
        let t = z(b"(No help files with an index were found.)");
        unsafe { WinSendDlgItemMsg(client, ID_LIST, LM_INSERTITEM, LIT_END, t.as_ptr() as u32) };
    }
    for (i, &c) in LETTERS.iter().enumerate() {
        let t = [c, 0];
        unsafe { WinCreateWindow(client, WC_BUTTON_W, t.as_ptr(), WS_VISIBLE, 0, 0, 10, 10, client, HWND_TOP, ID_TAB + i as u32, core::ptr::null(), core::ptr::null()) };
    }
    for e in &entries {
        let mut text = if e.sub { b"     ".to_vec() } else { Vec::new() };
        text.extend_from_slice(&e.text);
        let t = z(&text);
        unsafe { WinSendDlgItemMsg(client, ID_LIST, LM_INSERTITEM, LIT_END, t.as_ptr() as u32) };
    }
    let n = files.len();
    wins().push(IndexWin { obj: o, frame, client, files, helps: alloc::vec![0; n], entries });
    let (sw, sh) = screen();
    unsafe {
        WinSetWindowPos(frame, HWND_TOP, sw / 6, sh / 6, sw * 5 / 12, sh * 2 / 3, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
        WinSetFocus(HWND_DESKTOP, WinWindowFromID(client, ID_SEARCH));
    }
    frame
}

// ---- The class ----

extern "C" fn mindex_setup(obj: usize, setup: *const u8) -> u32 {
    som::call_parent(wps::ccd("Mindex"), wps::cd("WPObject"), m::WPObject::wpSetup, &[obj, setup as usize]);
    for (k, v) in obj::setup_pairs(cstr(setup)) {
        if k == b"INDEX" {
            md(obj).index = str_new(&v);
        }
    }
    1
}

extern "C" fn mindex_save_state(obj: usize) -> u32 {
    som::call_parent(wps::ccd("Mindex"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[obj]);
    let s = z(str_bytes(&md(obj).index));
    obj::wpSaveString(obj, b"Mindex\0".as_ptr(), 1, s.as_ptr());
    1
}

extern "C" fn mindex_restore_state(obj: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("Mindex"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[obj, r as usize]);
    if let Some(v) = obj::restored_string(obj, b"Mindex", 1) {
        md(obj).index = str_new(v.strip_suffix(&[0]).unwrap_or(&v));
    }
    1
}

extern "C" fn mindex_uninit(obj: usize) {
    md(obj).index = None;
    som::call_parent(wps::ccd("Mindex"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[obj]);
}

extern "C" fn mindex_open(obj: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view == OPEN_SETTINGS {
        return som::call_parent(wps::ccd("Mindex"), wps::cd("WPObject"), m::WPObject::wpOpen, &[obj, cnr as usize, view as usize, param as usize]) as u32;
    }
    open_index(obj)
}

methods! {
    __m_mindex_setup => mindex_setup, 2;
    __m_mindex_save_state => mindex_save_state, 1;
    __m_mindex_restore_state => mindex_restore_state, 2;
    __m_mindex_uninit => mindex_uninit, 1;
    __m_mindex_open => mindex_open, 4;
}

pub fn impl_mindex() -> Impl {
    Impl {
        data: core::mem::size_of::<MindexData>() as u32,
        methods: Vec::new(),
        overrides: alloc::vec![
            ("wpSetup", __m_mindex_setup as usize),
            ("wpSaveState", __m_mindex_save_state as usize),
            ("wpRestoreState", __m_mindex_restore_state as usize),
            ("wpUnInitData", __m_mindex_uninit as usize),
            ("wpOpen", __m_mindex_open as usize),
        ],
    }
}
