//! ODSettingsExtension: a part's settings notebook.
//!
//! ShowSettings(frame) (release index 23; IBM's parts call it for their
//! Settings menu command 0x501a/0x500f and on a double click) opens a PM
//! window holding a notebook and asks the extension for its pages with
//! AddNotebookPages(notebook, flags, reserved) (index 18) through its
//! token, so a subclass adds its own pages and calls its parent for the
//! standard ones (IBM's MMSettingsExtension passes flags | 0x1f). rs2's
//! standard pages, by flag:
//!
//! | Flag | Page |
//! |---|---|
//! | 0x01 | Name: the part's name (`OpenDoc:Metadata:Name`, IntlText); Apply stores it and calls TitleChanged(name) (index 19) |
//! | 0x02 | Type: the part's kind and editor |
//! | 0x04 | View: the frame's view type (frame, large/small icon, thumbnail) |
//! | 0x08 | Storage: the document file and the part's storage unit |
//! | 0x10 | Presentation: the frame's presentation |
//!
//! A part without a settings extension of its own gets a plain
//! ODSettingsExtension from the docshell (all pages).

use alloc::vec::Vec;

use crate::obj::{acquire, release};
use crate::rt::*;
use crate::storage;
use crate::*;

unsafe extern "C" {
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: usize, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32,
                       owner: u32, behind: u32, id: u32, ctl: usize, pres: usize) -> u32;
    #[link_name = "PMWIN.908"]
    fn WinCreateStdWindow(parent: u32, style: u32, flags: *mut u32, class: *const u8, title: *const u8, cstyle: u32, hmod: u32, id: u32, client: *mut u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, class: *const u8, proc_: usize, style: u32, cb: u32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(hwnd: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(hwnd: u32, rect: *mut [i32; 4]) -> u32;
    #[link_name = "PMWIN.841"]
    fn WinQueryWindowText(hwnd: u32, cb: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(hwnd: u32, id: u32) -> u32;
    #[link_name = "PMWIN.834"]
    fn WinQueryWindow(hwnd: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(hwnd: u32) -> u32;
    #[link_name = "PMWIN.851"]
    fn WinSetActiveWindow(desktop: u32, hwnd: u32) -> u32;
    #[link_name = "PMWIN.860"]
    fn WinSetFocus(desktop: u32, hwnd: u32) -> u32;
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(hwnd: u32, hps: u32, rect: *mut [i32; 4]) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, rect: *const [i32; 4], color: i32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(hwnd: u32) -> u32;
}

const WC_BUTTON: usize = 0xffff_0003;
const WC_STATIC: usize = 0xffff_0005;
const WC_ENTRYFIELD: usize = 0xffff_0006;
const WC_NOTEBOOK: usize = 0xffff_0028;
const WS_VISIBLE: u32 = 0x8000_0000;
const BKM_INSERTPAGE: u32 = 0x0355;
const BKM_SETDIMENSIONS: u32 = 0x035e;
const BKM_SETPAGEWINDOWHWND: u32 = 0x0360;
const BKM_SETSTATUSLINETEXT: u32 = 0x0361;
const BKM_SETTABTEXT: u32 = 0x0363;
const BKA_MAJOR: usize = 0x0040;
const BKA_STATUSTEXTON: usize = 0x0001;
const BKA_AUTOPAGESIZE: usize = 0x0100;
const BKA_LAST: usize = 0x0002;
const BKA_MAJORTAB: usize = 0x0001;
const EM_SETTEXTLIMIT: u32 = 0x0143;

pub mod ix {
    pub const INIT_SETTINGS_EXTENSION: usize = 17;
    pub const ADD_NOTEBOOK_PAGES: usize = 18;
    pub const TITLE_CHANGED: usize = 19;
    pub const ICON_CHANGED: usize = 21;
    pub const SHOW_SETTINGS: usize = 23;
}

pub const NAME_EXTENSION: &[u8] = b"OpenDoc Extension Info Settings";
const PROP_NAME: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Metadata:Name";
const TYPE_INTL_TEXT: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Type:IntlText";

/// An open settings notebook.
struct Book {
    ext: usize,
    frame: usize,
    window: u32,
    notebook: u32,
    pages: Vec<(u32, u32)>,
}

static mut BOOKS: Vec<Book> = Vec::new();
static mut REGISTERED: bool = false;
const CLIENT_CLASS: &[u8] = b"rs2ODSettings\0";
const PAGE_CLASS: &[u8] = b"rs2ODSettingsPage\0";

#[allow(static_mut_refs)]
fn book_of(hwnd: u32) -> Option<&'static mut Book> {
    unsafe { BOOKS.iter_mut().find(|b| b.window == hwnd || b.notebook == hwnd || b.pages.iter().any(|p| p.1 == hwnd)) }
}

fn base_part(ext: usize, ev: usize) -> usize {
    let p = call(cd!(ODExtensionClassData), 1, &[ext, ev]);
    release(p, ev);
    p
}

/// The part's name (IntlText: script, language, text).
pub fn part_name(part: usize) -> Vec<u8> {
    let su = crate::obj::su_of(part);
    if su == 0 {
        return Vec::new();
    }
    let s = storage::sus(su);
    let Some(p) = s.props.iter().find(|p| p.name == PROP_NAME) else { return Vec::new() };
    let Some(v) = p.vals.iter().find(|v| v.ty == TYPE_INTL_TEXT) else { return Vec::new() };
    v.data.get(8..).unwrap_or(&[]).iter().copied().take_while(|&b| b != 0).collect()
}

pub fn set_part_name(part: usize, name: &[u8]) {
    let su = crate::obj::su_of(part);
    if su == 0 {
        return;
    }
    let mut d = Vec::new();
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(name);
    d.push(0);
    let s = storage::sus(su);
    let v = storage::Value { ty: TYPE_INTL_TEXT.to_vec(), data: d };
    match s.props.iter_mut().find(|p| p.name == PROP_NAME) {
        Some(p) => p.vals = alloc::vec![v],
        None => s.props.push(storage::Prop { name: PROP_NAME.to_vec(), vals: alloc::vec![v] }),
    }
}

fn text(t: &[u8]) -> Vec<u8> {
    z(t)
}

fn child(parent: u32, class: usize, t: &[u8], style: u32, x: i32, y: i32, cx: i32, cy: i32, id: u32) -> u32 {
    let t = text(t);
    unsafe { WinCreateWindow(parent, class, t.as_ptr(), style | WS_VISIBLE, x, y, cx, cy, parent, 3, id, 0, 0) }
}

/// Insert a page (a window of class rs2ODSettingsPage) with a tab text.
pub fn insert_page(nb: u32, tab: &[u8], kind: u32) -> u32 {
    let id = unsafe { WinSendMsg(nb, BKM_INSERTPAGE, 0, (BKA_MAJOR | BKA_STATUSTEXTON | BKA_AUTOPAGESIZE) | (BKA_LAST << 16)) } as u32;
    let page = unsafe { WinCreateWindow(nb, PAGE_CLASS.as_ptr() as usize, b"\0".as_ptr(), 0, 0, 0, 300, 200, nb, 3, kind, 0, 0) };
    let t = z(tab);
    unsafe {
        WinSendMsg(nb, BKM_SETPAGEWINDOWHWND, id as usize, page as usize);
        WinSendMsg(nb, BKM_SETTABTEXT, id as usize, t.as_ptr() as usize);
        WinSendMsg(nb, BKM_SETSTATUSLINETEXT, id as usize, t.as_ptr() as usize);
    }
    if let Some(b) = book_of(nb) {
        b.pages.push((kind, page));
    }
    page
}

const PAGE_NAME: u32 = 1;
const PAGE_TYPE: u32 = 2;
const PAGE_VIEW: u32 = 3;
const PAGE_STORAGE: u32 = 4;
const PAGE_PRESENTATION: u32 = 5;
const ID_NAME: u32 = 100;
const ID_APPLY: u32 = 101;
const ID_VIEW: u32 = 110;

const VIEWS: [(&[u8], &[u8]); 4] = [
    (b"~Frame", b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:ViewType:Frame"),
    (b"~Large icon", b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:ViewType:LargeIcon"),
    (b"~Small icon", b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:ViewType:SmallIcon"),
    (b"~Thumbnail", b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:ViewType:Thumbnail"),
];

fn type_name(token: usize) -> Vec<u8> {
    crate::services::type_of(crate::services::session(), token).unwrap_or_default()
}

fn fill_page(b: &Book, kind: u32, page: u32, ev: usize) {
    let part = base_part(b.ext, ev);
    match kind {
        PAGE_NAME => {
            child(page, WC_STATIC, b"Name:", 0x0001, 8, 150, 60, 20, 0);
            let e = child(page, WC_ENTRYFIELD, &part_name(part), 0x0008 | 0x0004, 70, 150, 210, 20, ID_NAME);
            unsafe { WinSendMsg(e, EM_SETTEXTLIMIT, 255, 0) };
            unsafe { WinSetFocus(1, e) };
            child(page, WC_BUTTON, b"~Apply", 0, 8, 110, 80, 28, ID_APPLY);
        }
        PAGE_TYPE => {
            let su = crate::obj::su_of(part);
            let kind = if su != 0 { storage::get_value(su, storage::PROP_PART_KIND).unwrap_or_default() } else { Vec::new() };
            let editor = if su != 0 { storage::get_value(su, storage::PROP_EDITOR).unwrap_or_default() } else { Vec::new() };
            let cls = if part != 0 { cstr(somsup::class_name(somsup::class_of(part)) as usize).to_vec() } else { Vec::new() };
            let mut y = 150;
            for (l, v) in [(&b"Kind:"[..], kind), (b"Editor:", editor), (b"Class:", cls)] {
                child(page, WC_STATIC, l, 1, 8, y, 60, 20, 0);
                child(page, WC_STATIC, if v.is_empty() { b"-" } else { &v }, 1, 70, y, 220, 20, 0);
                y -= 26;
            }
        }
        PAGE_VIEW => {
            let cur = if b.frame != 0 { type_name(crate::layout::frame(b.frame).view) } else { Vec::new() };
            let mut y = 150;
            for (i, (label, ty)) in VIEWS.iter().enumerate() {
                let r = child(page, WC_BUTTON, label, 0x0004 | if i == 0 { 0x0002_0000 } else { 0 }, 8, y, 200, 22, ID_VIEW + i as u32);
                if cur == *ty {
                    unsafe { WinSendMsg(r, 0x0125, 1, 0) }; // BM_SETCHECK
                }
                y -= 26;
            }
        }
        PAGE_STORAGE => {
            let su = crate::obj::su_of(part);
            let id = if su != 0 { storage::su_id(su) } else { 0 };
            let dr = if su != 0 { storage::sus(su).draft } else { 0 };
            let doc = if dr != 0 { storage::draft(dr).document } else { 0 };
            let cont = if doc != 0 { storage::doc(doc).container } else { 0 };
            let file = if cont != 0 { storage::cont(cont).file.clone() } else { Vec::new() };
            child(page, WC_STATIC, b"Document:", 1, 8, 150, 80, 20, 0);
            child(page, WC_STATIC, if file.is_empty() { b"-" } else { &file }, 1, 90, 150, 200, 20, 0);
            child(page, WC_STATIC, b"Storage unit:", 1, 8, 124, 80, 20, 0);
            child(page, WC_STATIC, alloc::format!("{id}").as_bytes(), 1, 90, 124, 200, 20, 0);
        }
        PAGE_PRESENTATION => {
            let p = if b.frame != 0 { type_name(crate::layout::frame(b.frame).pres) } else { Vec::new() };
            child(page, WC_STATIC, b"Presentation:", 1, 8, 150, 90, 20, 0);
            child(page, WC_STATIC, if p.is_empty() { b"-" } else { &p }, 1, 8, 124, 290, 20, 0);
        }
        _ => {}
    }
}

fn page_command(b: &mut Book, kind: u32, page: u32, id: u32, ev: usize) {
    if kind == PAGE_NAME && id == ID_APPLY {
        let mut buf = [0u8; 256];
        let e = unsafe { WinWindowFromID(page, ID_NAME) };
        let n = unsafe { WinQueryWindowText(e, buf.len() as i32, buf.as_mut_ptr()) }.max(0) as usize;
        let name = buf[..n].to_vec();
        let part = base_part(b.ext, ev);
        set_part_name(part, &name);
        let z = z(&name);
        call(cd!(ODSettingsExtensionClassData), ix::TITLE_CHANGED, &[b.ext, ev, z.as_ptr() as usize]);
    }
    if kind == PAGE_VIEW && (ID_VIEW..ID_VIEW + 4).contains(&id) && b.frame != 0 {
        let ty = z(VIEWS[(id - ID_VIEW) as usize].1);
        let t = crate::services::tokenize(crate::services::session(), &ty[..ty.len() - 1]);
        crate::layout::ChangeViewType(b.frame, ev, t);
    }
}

fn env() -> somsup::Environment {
    somsup::Environment { major: 0, exception_name: core::ptr::null(), params: core::ptr::null_mut(), anchor: 0 }
}

/// Paint a window in the dialog background colour.
fn paint_background(hwnd: u32) {
    let mut r = [0i32; 4];
    unsafe {
        let hps = WinBeginPaint(hwnd, 0, &mut r);
        WinQueryWindowRect(hwnd, &mut r);
        WinFillRect(hps, &r, 15); // CLR_PALEGRAY
        WinEndPaint(hps);
    }
}

extern "C" fn page_proc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize {
    match msg {
        0x23 => {
            paint_background(hwnd);
            0
        }
        0x20 | 0x30 => {
            // WM_COMMAND (Apply), WM_CONTROL (BN_CLICKED of the radios).
            let id = (mp1 & 0xffff) as u32;
            if msg == 0x30 && (mp1 >> 16) != 1 {
                return 0;
            }
            if let Some(b) = book_of(hwnd) {
                let kind = b.pages.iter().find(|p| p.1 == hwnd).map_or(0, |p| p.0);
                let mut e = env();
                page_command(b, kind, hwnd, id, &raw mut e as usize);
            }
            0
        }
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}

extern "C" fn client_proc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize {
    match msg {
        0x23 => {
            paint_background(hwnd);
            0
        }
        0x07 => {
            // WM_SIZE: the notebook fills the client.
            if let Some(b) = book_of(unsafe { WinQueryWindow(hwnd, 5) }) {
                let mut r = [0i32; 4];
                unsafe { WinQueryWindowRect(hwnd, &mut r) };
                unsafe { WinSetWindowPos(b.notebook, 0, 0, 0, r[2], r[3], 0x3) };
            }
            0
        }
        0x02 => {
            // WM_DESTROY
            let frame = unsafe { WinQueryWindow(hwnd, 5) };
            unsafe {
                #[allow(static_mut_refs)]
                if let Some(i) = BOOKS.iter().position(|b| b.window == frame) {
                    let b = BOOKS.remove(i);
                    release(b.ext, 0);
                }
            }
            unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) }
        }
        _ => unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) },
    }
}

crate::tramp!(page_proc_t => page_proc, 4);
crate::tramp!(client_proc_t => client_proc, 4);

/// Open (or bring forward) the settings notebook of an extension.
#[allow(static_mut_refs)]
pub fn show(ext: usize, ev: usize, frame: usize) -> usize {
    unsafe {
        if let Some(b) = BOOKS.iter().find(|b| b.ext == ext) {
            WinSetActiveWindow(1, b.window);
            return 1;
        }
        let hab = WinQueryAnchorBlock(1);
        if !REGISTERED {
            WinRegisterClass(hab, CLIENT_CLASS.as_ptr(), client_proc_t as usize, 4, 0);
            WinRegisterClass(hab, PAGE_CLASS.as_ptr(), page_proc_t as usize, 0, 0);
            REGISTERED = true;
        }
    }
    let part = base_part(ext, ev);
    let mut title = part_name(part);
    if title.is_empty() && part != 0 {
        title = cstr(somsup::class_name(somsup::class_of(part)) as usize).to_vec();
    }
    title.extend_from_slice(b" - Settings");
    let t = z(&title);
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINBUTTON | FCF_MAXBUTTON | FCF_TASKLIST
    let mut fl = 0x1 | 0x2 | 0x8 | 0x10 | 0x20 | 0x800u32;
    let mut client = 0u32;
    let window = unsafe { WinCreateStdWindow(1, 0, &mut fl, CLIENT_CLASS.as_ptr(), t.as_ptr(), WS_VISIBLE, 0, 0x8008, &mut client) };
    if window == 0 {
        return 0;
    }
    let nb = unsafe { WinCreateWindow(client, WC_NOTEBOOK, b"\0".as_ptr(), WS_VISIBLE | 0x1 | 0x10, 0, 0, 420, 300, client, 3, 1, 0, 0) };
    unsafe {
        WinSendMsg(nb, BKM_SETDIMENSIONS, 100 | (24 << 16), BKA_MAJORTAB);
        BOOKS.push(Book { ext: acquire(ext, ev), frame, window, notebook: nb, pages: Vec::new() });
    }
    // The pages, through the extension's token (a subclass adds its own).
    call(cd!(ODSettingsExtensionClassData), ix::ADD_NOTEBOOK_PAGES, &[ext, ev, nb as usize, 0, 0]);
    unsafe {
        let b = BOOKS.iter().find(|b| b.ext == ext).unwrap();
        let pages = b.pages.clone();
        for (kind, page) in pages {
            fill_page(b, kind, page, ev);
        }
        WinSetWindowPos(window, 3, 160, 140, 460, 340, 0x1 | 0x2 | 0x4 | 0x8 | 0x80);
        if let Some(&(_, page)) = b.pages.iter().find(|p| p.0 == PAGE_NAME) {
            WinSetFocus(1, WinWindowFromID(page, ID_NAME));
        }
    }
    1
}

/// The book open for an extension (its frame window), for tests.
#[allow(static_mut_refs)]
pub fn open_book(ext: usize) -> Option<(u32, u32, usize)> {
    unsafe { BOOKS.iter().find(|b| b.ext == ext).map(|b| (b.window, b.notebook, b.pages.len())) }
}

methods! {
    fn AddNotebookPages(this: usize, ev: usize, nb: usize, flags: usize, reserved: usize) -> usize {
        let flags = if flags & 0x1f == 0 { 0x1f } else { flags };
        for (bit, tab, kind) in [(1, &b"Name"[..], PAGE_NAME), (2, b"Type", PAGE_TYPE), (4, b"View", PAGE_VIEW), (8, b"Storage", PAGE_STORAGE), (0x10, b"Presentation", PAGE_PRESENTATION)] {
            if flags & bit != 0 {
                insert_page(nb as u32, tab, kind);
            }
        }
        1
    }
    fn TitleChanged(this: usize, ev: usize, title: usize) -> usize { 0 }
    fn IconChanged(this: usize, ev: usize, icon: usize) -> usize { 0 }
    fn ShowSettings(this: usize, ev: usize, frame: usize) -> usize {
        // IBM's parts pass the facet (from their menu command) or the frame.
        let frame = if frame != 0 && somsup::is_a(frame, unsafe { *cd!(ODFacetClassData) }) {
            crate::layout::facet(frame).frame
        } else if frame != 0 && somsup::is_a(frame, unsafe { *cd!(ODFrameClassData) }) {
            frame
        } else {
            0
        };
        show(this, ev, frame)
    }
    fn CloseSettings(this: usize, ev: usize) -> usize {
        if let Some((w, _, _)) = open_book(this) {
            unsafe { WinDestroyWindow(w) };
        }
        0
    }
}
