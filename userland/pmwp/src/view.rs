//! Views of objects, in the process running the Workplace Shell:
//!
//! - folder windows: a frame with a container (WC_CONTAINER) showing the
//!   folder's objects in icon, details or tree view (each record keeps
//!   its object in R_KEY); the desktop (WPDesktop's view) covers the
//!   screen below every other window;
//! - settings notebooks: a frame with a notebook (WC_NOTEBOOK) the
//!   object fills with wpAddSettingsPages.
//!
//! The containers' notifications become method calls: Enter
//! (wpViewObject), the context menu (wpDisplayMenu), direct editing
//! (wpSetTitle), dragging (wpFormatDragItem, wpDragOver, wpDrop; dnd.rs).
//! Folders tell the views when their content changes (added, removed)
//! and objects when they change (changed).

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::obj::{self, OPEN_CONTENTS, OPEN_DEFAULT, OPEN_DETAILS, OPEN_SETTINGS, OPEN_TREE};
use crate::pm::*;
use crate::{som, wps};

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}

pub struct Rec {
    pub obj: usize,
    pub rec: u32,
    /// The title the record shows (with a NUL; the record points at it).
    pub title: Vec<u8>,
}

pub struct Win {
    /// The folder (or, for a notebook, the object).
    pub obj: usize,
    /// OPEN_CONTENTS, OPEN_DETAILS, OPEN_TREE or OPEN_SETTINGS.
    pub view: u32,
    pub frame: u32,
    pub client: u32,
    /// The container or the notebook.
    pub child: u32,
    pub recs: Vec<Rec>,
    pub desktop: bool,
    pub fields: bool,
}

static mut WINS: Vec<Win> = Vec::new();
static mut CLASS_DONE: bool = false;
/// The pop-up menu on show and its object.
pub static mut POPUP: (u32, usize) = (0, 0);
/// Direct editing: the buffer the container copies the new title to.
static mut EDIT: Vec<u8> = Vec::new();

const CLASS: &[u8] = b"WPS_View\0";
pub const ID_CHILD: u32 = 100;

#[allow(static_mut_refs)]
pub fn wins() -> &'static mut Vec<Win> {
    unsafe { &mut WINS }
}

fn hab() -> u32 {
    unsafe { WinQueryAnchorBlock(HWND_DESKTOP) }
}

fn ensure_class() {
    unsafe {
        if !CLASS_DONE {
            WinRegisterClass(hab(), CLASS.as_ptr(), client_proc, 0, 4);
            CLASS_DONE = true;
        }
    }
}

pub fn find_client(client: u32) -> Option<usize> {
    wins().iter().position(|w| w.client == client)
}

pub fn find(obj: usize, view: u32) -> Option<usize> {
    wins().iter().position(|w| w.obj == obj && w.view == view)
}

/// The window an object's view has (0: none).
pub fn frame_of(obj: usize, view: u32) -> u32 {
    find(obj, view).map_or(0, |i| wins()[i].frame)
}

/// Bring a view to the front.
pub fn activate(frame: u32) {
    unsafe {
        let style = WinQueryWindowULong(frame, QWL_STYLE);
        WinSetWindowPos(frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER | if style & WS_MINIMIZED != 0 { SWP_RESTORE } else { 0 });
    }
}

fn new_frame(title: &[u8], desktop: bool) -> (u32, u32) {
    ensure_class();
    let t = z(title);
    let mut fcf = if desktop { 0 } else { FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST };
    let mut client = 0;
    let frame = unsafe {
        WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), if desktop { b"\0".as_ptr() } else { t.as_ptr() }, WS_CLIPCHILDREN, 0, 1, &mut client)
    };
    (frame, client)
}

fn place(frame: u32, n: i32, w: i32, h: i32) {
    let (sw, sh) = screen();
    let (cx, cy) = (w.min(sw - 40), h.min(sh - 40));
    let x = (40 + n * 28) % (sw - cx).max(1);
    let y = (sh - cy - 40 - n * 28).max(0);
    unsafe { WinSetWindowPos(frame, 0, x, y, cx, cy, SWP_SIZE | SWP_MOVE) };
}

fn size_child(w: usize) {
    let (client, child) = (wins()[w].client, wins()[w].child);
    let mut r = RectL::default();
    unsafe {
        WinQueryWindowRect(client, &mut r);
        WinSetWindowPos(child, 0, 0, 0, r.r, r.t, SWP_SIZE | SWP_MOVE);
    }
    if wins()[w].view == OPEN_CONTENTS {
        arrange(w);
    }
}

// ---- Folder windows ----

fn cnr_mode(view: u32) -> u32 {
    match view {
        OPEN_DETAILS => CV_DETAIL,
        OPEN_TREE => CV_TREE,
        _ => CV_ICON,
    }
}

/// Open a folder's view (or bring it to the front); returns its frame.
pub fn open_folder(folder: usize, _cnr: u32, view: u32, _param: u32) -> u32 {
    let view = match view {
        OPEN_DETAILS | OPEN_TREE => view,
        _ => OPEN_CONTENTS,
    };
    if let Some(i) = find(folder, view) {
        let f = wins()[i].frame;
        if !wins()[i].desktop {
            activate(f);
        }
        return f;
    }
    let desktop = view == OPEN_CONTENTS && wps::is(folder, "WPDesktop") && !wins().iter().any(|w| w.desktop);
    let mut title = obj::title(folder);
    if !desktop {
        title.extend_from_slice(match view {
            OPEN_DETAILS => b" - Details View",
            OPEN_TREE => b" - Tree View",
            _ => b" - Icon View",
        });
    }
    let (frame, client) = new_frame(&title, desktop);
    if frame == 0 {
        return 0;
    }
    let cnr = unsafe {
        WinCreateWindow(client, WC_CONTAINER, b"\0".as_ptr(), WS_VISIBLE | CCS_EXTENDSEL, 0, 0, 10, 10, client, HWND_TOP, ID_CHILD, core::ptr::null(), core::ptr::null())
    };
    wins().push(Win { obj: folder, view, frame, client, child: cnr, recs: Vec::new(), desktop, fields: false });
    let w = wins().len() - 1;
    let mut ci = [0u32; 23];
    ci[0] = 92;
    ci[5] = match cnr_mode(view) {
        CV_DETAIL => CV_DETAIL | CV_MINI | CA_DETAILSVIEWTITLES,
        CV_TREE => CV_TREE | CV_ICON | CV_MINI | CA_TREELINE,
        _ => CV_ICON,
    };
    send(cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR);
    if view == OPEN_DETAILS {
        add_fields(w);
    }
    let (sw, sh) = screen();
    if desktop {
        let bg: u32 = 0x2e6e8e;
        let fg: u32 = 0xffffff;
        unsafe {
            WinSetPresParam(cnr, 3, 4, &bg);
            WinSetPresParam(cnr, 1, 4, &fg);
            WinSetWindowPos(frame, HWND_BOTTOM, 0, 0, sw, sh, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER);
            Rs2SetDesktopWindow(frame);
        }
    } else {
        unsafe { send(frame, 0x0047, obj::call(m::WPObject::wpQueryIcon, &[folder]) as u32, 0) };
        place(frame, wins().len() as i32, sw * 11 / 20, sh / 2);
    }
    size_child(w);
    fill(w);
    if !desktop {
        activate(frame);
    }
    unsafe { WinSetFocus(HWND_DESKTOP, cnr) };
    frame
}

/// Details view columns: icon and title.
fn add_fields(w: usize) {
    let cnr = wins()[w].child;
    let first = send(cnr, CM_ALLOCDETAILFIELDINFO, 2, 0);
    if first == 0 {
        return;
    }
    let cols: [(u32, &'static [u8], usize); 2] =
        [(CFA_BITMAPORICON | CFA_VCENTER, b"Icon\0", R_HPTR), (CFA_STRING | CFA_VCENTER | CFA_SEPARATOR | CFA_FIREADONLY, b"Title\0", R_PSZNAME)];
    let mut f = first;
    for c in cols {
        wr(f, 4, c.0);
        wr(f, 8, CFA_LEFT | CFA_VCENTER);
        wr(f, 12, c.1.as_ptr() as u32);
        wr(f, 16, c.2 as u32);
        f = rd(f, 24);
        if f == 0 {
            break;
        }
    }
    let fi: [u32; 4] = [16, CMA_END, 1, 2];
    send(cnr, CM_INSERTDETAILFIELDINFO, first, fi.as_ptr() as u32);
    wins()[w].fields = true;
}

/// The objects a folder view shows (not the invisible ones).
fn visible_content(folder: usize) -> Vec<usize> {
    let mut v = Vec::new();
    let mut o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[folder, 0, 0]);
    while o != 0 && v.len() < 10000 {
        if obj::call(m::WPObject::wpQueryStyle, &[o]) as u32 & obj::OBJSTYLE_NOTVISIBLE == 0 {
            v.push(o);
        }
        o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[folder, o, 1]);
    }
    v
}

fn title_bytes(o: usize) -> Vec<u8> {
    let mut t = obj::title(o);
    t.push(0);
    t
}

fn set_rec(r: u32, rec: &Rec, icon: u32) {
    let name = rec.title.as_ptr() as u32;
    for off in [R_PSZICON, R_PSZTEXT, R_PSZNAME, R_PSZTREE] {
        wr(r, off, name);
    }
    wr(r, R_HPTR, icon);
    wr(r, R_HPTRMINI, icon);
    wr(r, R_KEY, rec.obj as u32);
}

/// Insert records for objects (at the end).
fn insert(w: usize, objs: &[usize]) {
    if objs.is_empty() {
        return;
    }
    let cnr = wins()[w].child;
    let first = send(cnr, CM_ALLOCRECORD, R_EXTRA, objs.len() as u32);
    if first == 0 {
        return;
    }
    let mut r = first;
    for &o in objs {
        let rec = Rec { obj: o, rec: r, title: title_bytes(o) };
        let icon = obj::call(m::WPObject::wpQueryIcon, &[o]) as u32;
        set_rec(r, &rec, icon);
        wins()[w].recs.push(rec);
        r = rd(r, R_NEXT);
        if r == 0 {
            break;
        }
    }
    let ri: [u32; 6] = [24, CMA_END, 0, 1, 1, objs.len() as u32];
    send(cnr, CM_INSERTRECORD, first, ri.as_ptr() as u32);
    arrange(w);
}

/// Fill a folder view from the folder's content.
pub fn fill(w: usize) {
    let cnr = wins()[w].child;
    send(cnr, CM_REMOVERECORD, 0, mp(0, CMA_FREE | CMA_INVALIDATE));
    wins()[w].recs.clear();
    let objs = visible_content(wins()[w].obj);
    insert(w, &objs);
}

/// Icons on a grid: rows from the top left (the desktop: columns), with
/// room for the titles centred under them (as RS2SHELL places them).
pub fn arrange(w: usize) {
    if wins()[w].view != OPEN_CONTENTS {
        return;
    }
    let cnr = wins()[w].child;
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(cnr, &mut r) };
    let (aw, ah) = (r.r.max(100), (r.t - 2).max(100));
    let desktop = wins()[w].desktop;
    let (cw, ch) = if desktop { (96, 86) } else { (88, 70) };
    for (i, rec) in wins()[w].recs.iter().enumerate() {
        let i = i as i32;
        let (x, y) = if desktop {
            let per_col = ((ah - 16) / ch).max(1);
            let (col, row) = (i / per_col, i % per_col);
            (16 + col * cw + (cw - 32) / 2 - 16, ah - 12 - 32 - row * ch)
        } else {
            let per_row = ((aw - 8) / cw).max(1);
            let (row, col) = (i / per_row, i % per_row);
            (4 + col * cw + (cw - 32) / 2, ah - 8 - 32 - row * ch)
        };
        wr(rec.rec, R_X, x as u32);
        wr(rec.rec, R_Y, y as u32);
    }
    send(cnr, CM_INVALIDATERECORD, 0, mp(0, CMA_REPOSITION));
}

/// An object came into a folder: the folder's views show it.
pub fn added(folder: usize, o: usize) {
    if obj::call(m::WPObject::wpQueryStyle, &[o]) as u32 & obj::OBJSTYLE_NOTVISIBLE != 0 {
        return;
    }
    for w in 0..wins().len() {
        if wins()[w].obj == folder && wins()[w].view != OPEN_SETTINGS && !wins()[w].recs.iter().any(|r| r.obj == o) {
            insert(w, &[o]);
        }
    }
}

/// An object left a folder (or went).
pub fn removed(folder: usize, o: usize) {
    for w in 0..wins().len() {
        if (folder == 0 || wins()[w].obj == folder) && wins()[w].view != OPEN_SETTINGS {
            if let Some(i) = wins()[w].recs.iter().position(|r| r.obj == o) {
                let rec = wins()[w].recs.remove(i);
                let list = [rec.rec];
                send(wins()[w].child, CM_REMOVERECORD, list.as_ptr() as u32, mp(1, CMA_FREE | CMA_INVALIDATE));
                arrange(w);
            }
        }
    }
    // Its own views close.
    let own: Vec<u32> = wins().iter().filter(|w| w.obj == o && !w.desktop).map(|w| w.frame).collect();
    for f in own {
        close_frame(f);
    }
}

/// An object's title or icon changed: the records showing it.
pub fn changed(o: usize) {
    for w in 0..wins().len() {
        let cnr = wins()[w].child;
        if let Some(i) = wins()[w].recs.iter().position(|r| r.obj == o) {
            let icon = obj::call(m::WPObject::wpQueryIcon, &[o]) as u32;
            wins()[w].recs[i].title = title_bytes(o);
            let rec = &wins()[w].recs[i];
            set_rec(rec.rec, rec, icon);
            let list = [rec.rec];
            send(cnr, CM_INVALIDATERECORD, list.as_ptr() as u32, mp(1, CMA_TEXTCHANGED));
        }
        if wins()[w].obj == o && !wins()[w].desktop {
            let mut t = obj::title(o);
            t.extend_from_slice(match wins()[w].view {
                OPEN_DETAILS => b" - Details View",
                OPEN_TREE => b" - Tree View",
                OPEN_SETTINGS => b" - Settings",
                _ => b" - Icon View",
            });
            let t = z(&t);
            unsafe { WinSetWindowText(wins()[w].frame, t.as_ptr()) };
        }
    }
}

/// Refresh a folder's views (its content read again).
pub fn refresh(folder: usize) {
    wps::call("WPFolder", m::WPFolder::wpPopulate, &[folder, 0, 0, 0]);
    for w in 0..wins().len() {
        if wins()[w].obj == folder && wins()[w].view != OPEN_SETTINGS {
            fill(w);
        }
    }
}

pub fn close_frame(frame: u32) {
    if let Some(i) = wins().iter().position(|w| w.frame == frame) {
        let w = wins().remove(i);
        if w.view != OPEN_SETTINGS {
            send(w.child, CM_REMOVERECORD, 0, mp(0, CMA_FREE));
        }
        unsafe { WinDestroyWindow(w.frame) };
    }
}

/// The object a container record stands for.
pub fn obj_of_rec(w: usize, rec: u32) -> usize {
    if rec == 0 {
        return 0;
    }
    let o = rd(rec, R_KEY) as usize;
    if wins()[w].recs.iter().any(|r| r.obj == o) { o } else { 0 }
}

/// The selected objects (with `on`, the one under the mouse, if it is
/// not selected: it alone).
pub fn selected(w: usize, on: usize) -> Vec<usize> {
    let sel: Vec<usize> = wins()[w].recs.iter().filter(|r| rd(r.rec, R_ATTR) & CRA_SELECTED != 0).map(|r| r.obj).collect();
    if on != 0 && !sel.contains(&on) { alloc::vec![on] } else { sel }
}

pub fn record_of(w: usize, o: usize) -> u32 {
    wins()[w].recs.iter().find(|r| r.obj == o).map_or(0, |r| r.rec)
}

// ---- Settings notebooks ----

/// Open an object's settings notebook (or bring it to the front).
pub fn open_settings(o: usize) -> u32 {
    if let Some(i) = find(o, OPEN_SETTINGS) {
        activate(wins()[i].frame);
        return wins()[i].frame;
    }
    let mut title = obj::title(o);
    title.extend_from_slice(b" - Settings");
    let (frame, client) = new_frame(&title, false);
    if frame == 0 {
        return 0;
    }
    // BKS_BACKPAGESBR | BKS_MAJORTABRIGHT | BKS_SQUARETABS | BKS_STATUSTEXTLEFT
    let nb = unsafe {
        WinCreateWindow(client, WC_NOTEBOOK, b"\0".as_ptr(), WS_VISIBLE | 0x0001 | 0x0010, 0, 0, 10, 10, client, HWND_TOP, ID_CHILD, core::ptr::null(), core::ptr::null())
    };
    wins().push(Win { obj: o, view: OPEN_SETTINGS, frame, client, child: nb, recs: Vec::new(), desktop: false, fields: false });
    let w = wins().len() - 1;
    let (sw, sh) = screen();
    unsafe { send(frame, 0x0047, obj::call(m::WPObject::wpQueryIcon, &[o]) as u32, 0) };
    place(frame, wins().len() as i32, (sw * 3 / 5).max(480), (sh * 3 / 5).max(360));
    size_child(w);
    obj::call(m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    // The first page shows.
    // BKM_QUERYPAGEID (BKA_FIRST), BKM_TURNTOPAGE.
    let first = send(nb, 0x0359, 0, mp(0x0004, 0));
    if first != 0 {
        send(nb, 0x0357, first, 0);
    }
    activate(frame);
    frame
}

// ---- The window procedure ----

extern "C" fn client_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_SIZE => {
            if let Some(w) = find_client(h) {
                size_child(w);
            }
            0
        }
        WM_CONTROL if a & 0xffff == ID_CHILD => {
            let Some(w) = find_client(h) else { return 0 };
            if wins()[w].view == OPEN_SETTINGS {
                return 0;
            }
            match a >> 16 {
                CN_ENTER => {
                    // NOTIFYRECORDENTER: hwndCnr, fKey, pRecord.
                    let o = obj_of_rec(w, rd(b, 8));
                    if o != 0 {
                        obj::call(m::WPObject::wpViewObject, &[o, wins()[w].child as usize, OPEN_DEFAULT as usize, 0]);
                    }
                    0
                }
                CN_CONTEXTMENU => {
                    let o = obj_of_rec(w, b);
                    let (target, kind) = if o != 0 { (o, crate::menu::MENU_OBJECTPOPUP) } else { (wins()[w].obj, crate::menu::MENU_OPENVIEWPOPUP) };
                    let p = pointer_pos();
                    obj::call(m::WPObject::wpDisplayMenu, &[target, wins()[w].frame as usize, h as usize, &p as *const PointL as usize, kind as usize, 0]);
                    0
                }
                CN_INITDRAG => crate::dnd::begin(w, b),
                CN_DRAGOVER => crate::dnd::drag_over(w, b),
                CN_DROP => {
                    crate::dnd::drop(w, b);
                    0
                }
                CN_DRAGLEAVE | CN_DROPHELP => 0,
                CN_REALLOCPSZ => {
                    // CNREDITDATA: cb, hwndCnr, pRecord, pFieldInfo, ppszText, cbText.
                    let cb = rd(b, 20);
                    unsafe {
                        EDIT = alloc::vec![0u8; cb as usize + 1];
                        #[allow(static_mut_refs)]
                        wr(rd(b, 16), 0, EDIT.as_mut_ptr() as u32);
                    }
                    1
                }
                CN_ENDEDIT => {
                    let o = obj_of_rec(w, rd(b, 8));
                    let pp = rd(b, 16);
                    if o != 0 && pp != 0 {
                        let t = cstr(rd(pp, 0) as *const u8);
                        let t: Vec<u8> = t.into_iter().filter(|&c| c != b'\r').collect();
                        if !t.is_empty() && t != obj::title(o) {
                            let zt = z(&t);
                            obj::call(m::WPObject::wpSetTitle, &[o, zt.as_ptr() as usize]);
                        }
                        changed(o);
                    }
                    0
                }
                _ => 0,
            }
        }
        WM_COMMAND => {
            let (menu, o) = unsafe { POPUP };
            unsafe { POPUP = (0, 0) };
            let _ = menu;
            if o != 0 {
                let w = find_client(h);
                if a & 0xffff == crate::menu::MENUID_RENAME {
                    if let Some(w) = w {
                        rename(w, o);
                    }
                    return 0;
                }
                let frame = w.map_or(0, |w| wins()[w].frame);
                obj::call(m::WPObject::wpMenuItemSelected, &[o, frame as usize, (a & 0xffff) as usize]);
            }
            0
        }
        DM_RENDERPREPARE => 1,
        DM_ENDCONVERSATION => 0,
        DM_DRAGOVERNOTIFY => 0,
        WM_CLOSE => {
            if let Some(w) = find_client(h) {
                if !wins()[w].desktop {
                    let f = wins()[w].frame;
                    close_frame(f);
                }
            }
            0
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
        _ => unsafe { WinDefWindowProc(h, msg, a, b) },
    }
}

/// Start editing a record's title in place.
pub fn rename(w: usize, o: usize) {
    let rec = record_of(w, o);
    if rec == 0 {
        return;
    }
    // CNREDITDATA: cb, hwndCnr, pRecord, pFieldInfo, ppszText, cbText, id.
    let ed: [u32; 7] = [28, wins()[w].child, rec, 0, 0, 0, 0];
    send(wins()[w].child, CM_OPENEDIT, ed.as_ptr() as u32, 0);
}

pub fn _unused() -> u32 {
    som::ret0 as usize as u32
}

pub const _VIEWS: [u32; 4] = [OPEN_CONTENTS, OPEN_DETAILS, OPEN_TREE, OPEN_SETTINGS];
