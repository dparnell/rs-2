//! Views of objects, in the process running the Workplace Shell:
//!
//! - folder windows: a frame with a container (WC_CONTAINER) showing the
//!   folder's objects in icon or details view (each record keeps its
//!   object in R_KEY), or its folders in tree view (expanding as they
//!   are opened); the desktop (WPDesktop's view) covers the screen below
//!   every other window;
//! - settings notebooks: a frame with a notebook (WC_NOTEBOOK) the
//!   object fills with wpAddSettingsPages.
//!
//! The containers' notifications become method calls: Enter
//! (wpViewObject), the context menu (wpDisplayMenu), direct editing
//! (wpSetTitle), dragging (wpFormatDragItem, wpDragOver, wpDrop; dnd.rs);
//! from the keyboard Delete deletes, F2 renames, F5 refreshes and
//! Shift+F10 shows the menu. Folders tell the views when their content
//! changes (added, removed) and objects when they change (changed).
//!
//! Icon positions are kept in the folder's .ICONPOS extended attribute
//! (rs2's records: x, y, the name: a file's name or `#` and an abstract
//! object's handle in hex; `@` records the height the positions are
//! from, so that icons keep their distance from the top). Shadows' titles
//! are drawn in the shadow colour (the container's owner drawing).

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::obj::{self, OPEN_CONTENTS, OPEN_DEFAULT, OPEN_DETAILS, OPEN_SETTINGS, OPEN_TREE};
use crate::pm::*;
use crate::{abs, util, wps};

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}

pub struct Rec {
    /// The object (0: a tree placeholder).
    pub obj: usize,
    pub rec: u32,
    /// The title the record shows (with a NUL; the record points at it).
    pub title: Vec<u8>,
    /// Tree view: the parent record (0: a root).
    pub parent: u32,
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
    /// The container's height (icon positions are from its bottom).
    pub height: i32,
}

static mut WINS: Vec<Win> = Vec::new();
static mut CLASS_DONE: bool = false;
/// The pop-up menu on show and its object.
pub static mut POPUP: (u32, usize) = (0, 0);
/// Direct editing: the buffer the container copies the new title to.
static mut EDIT: Vec<u8> = Vec::new();
static mut OLD_CNR: usize = 0;

const CLASS: &[u8] = b"WPS_View\0";
pub const ID_CHILD: u32 = 100;
const CN_EXPANDTREE_: u32 = 117;
const VK_F2: u32 = 0x21;
const VK_F5: u32 = 0x24;
const VK_F10: u32 = 0x29;

const KC_SHIFT: u32 = 0x0008;

/// The desktop's background unless set otherwise.
pub const DESKTOP_BG: u32 = 0x2e6e8e;

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

fn find_cnr(cnr: u32) -> Option<usize> {
    wins().iter().position(|w| w.child == cnr && w.view != OPEN_SETTINGS)
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

fn cnr_height(w: usize) -> i32 {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(wins()[w].child, &mut r) };
    (r.t - 2).max(100)
}

fn size_child(w: usize) {
    let (client, child) = (wins()[w].client, wins()[w].child);
    let mut r = RectL::default();
    unsafe {
        WinQueryWindowRect(client, &mut r);
        WinSetWindowPos(child, 0, 0, 0, r.r, r.t, SWP_SIZE | SWP_MOVE);
    }
    // A taller or shorter window: the icons keep their distance from the
    // top (positions are from the bottom).
    let (old, new) = (wins()[w].height, cnr_height(w));
    wins()[w].height = new;
    if old != 0 && old != new && wins()[w].view == OPEN_CONTENTS {
        let dy = new - old;
        for r in wins()[w].recs.iter() {
            wr(r.rec, R_Y, (rd(r.rec, R_Y) as i32 + dy) as u32);
        }
        send(child, CM_INVALIDATERECORD, 0, mp(0, CMA_REPOSITION));
    }
}

// ---- Colours ----

/// A folder view's background (RGB): the folder's own, else the
/// desktop's teal or white.
fn background_of(w: usize) -> u32 {
    crate::fs::folder_background(wins()[w].obj).unwrap_or(if wins()[w].desktop { DESKTOP_BG } else { 0xffffff })
}

fn dark(bg: u32) -> bool {
    let (r, g, b) = ((bg >> 16) & 0xff, (bg >> 8) & 0xff, bg & 0xff);
    r * 3 + g * 6 + b < 1280
}

fn set_colors(w: usize) {
    let cnr = wins()[w].child;
    let bg = background_of(w);
    let fg: u32 = if dark(bg) { 0xffffff } else { 0 };
    unsafe {
        WinSetPresParam(cnr, 3, 4, &bg);
        WinSetPresParam(cnr, 1, 4, &fg);
    }
}

/// A folder's background changed: its views show it.
pub fn background_changed(folder: usize) {
    for w in 0..wins().len() {
        if wins()[w].obj == folder && wins()[w].view != OPEN_SETTINGS {
            set_colors(w);
            unsafe { WinInvalidateRect(wins()[w].child, core::ptr::null(), 1) };
        }
    }
}

// ---- Folder windows ----

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
        title.extend_from_slice(view_suffix(view));
    }
    let (frame, client) = new_frame(&title, desktop);
    if frame == 0 {
        return 0;
    }
    let cnr = unsafe {
        WinCreateWindow(client, WC_CONTAINER, b"\0".as_ptr(), WS_VISIBLE | CCS_EXTENDSEL, 0, 0, 10, 10, client, HWND_TOP, ID_CHILD, core::ptr::null(), core::ptr::null())
    };
    unsafe { OLD_CNR = WinSubclassWindow(cnr, cnr_proc) as usize };
    wins().push(Win { obj: folder, view, frame, client, child: cnr, recs: Vec::new(), desktop, fields: false, height: 0 });
    let w = wins().len() - 1;
    let mut ci = [0u32; 23];
    ci[0] = 92;
    // Owner drawing: shadows' titles (see draw_item).
    ci[5] = CA_OWNERDRAW
        | match view {
            OPEN_DETAILS => CV_DETAIL | CV_MINI | CA_DETAILSVIEWTITLES,
            OPEN_TREE => CV_TREE | CV_ICON | CV_MINI | CA_TREELINE,
            _ => CV_ICON,
        };
    send(cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR);
    if view == OPEN_DETAILS {
        add_fields(w);
    }
    set_colors(w);
    let (sw, sh) = screen();
    if desktop {
        unsafe {
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

fn view_suffix(view: u32) -> &'static [u8] {
    match view {
        OPEN_DETAILS => b" - Details View",
        OPEN_TREE => b" - Tree View",
        OPEN_SETTINGS => b" - Settings",
        _ => b" - Icon View",
    }
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

fn visible(o: usize) -> bool {
    obj::call(m::WPObject::wpQueryStyle, &[o]) as u32 & obj::OBJSTYLE_NOTVISIBLE == 0
}

/// The objects a folder view shows (not the invisible ones).
fn visible_content(folder: usize) -> Vec<usize> {
    let mut v = Vec::new();
    let mut o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[folder, 0, 0]);
    while o != 0 && v.len() < 10000 {
        if visible(o) {
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

/// Insert records (each under its parent, or at the top level).
fn insert_recs(w: usize, list: &[(usize, u32)]) -> Vec<u32> {
    let cnr = wins()[w].child;
    let mut made = Vec::new();
    for &(o, parent) in list {
        let r = send(cnr, CM_ALLOCRECORD, R_EXTRA, 1);
        if r == 0 {
            break;
        }
        let rec = Rec { obj: o, rec: r, title: if o == 0 { alloc::vec![0] } else { title_bytes(o) }, parent };
        let icon = if o == 0 { 0 } else { obj::call(m::WPObject::wpQueryIcon, &[o]) as u32 };
        set_rec(r, &rec, icon);
        wins()[w].recs.push(rec);
        made.push(r);
        let ri: [u32; 6] = [24, CMA_END, parent, 0, 1, 1];
        send(cnr, CM_INSERTRECORD, r, ri.as_ptr() as u32);
    }
    unsafe { WinInvalidateRect(cnr, core::ptr::null(), 1) };
    made
}

/// Insert records for objects of an icon or details view (at free
/// places, or kept ones).
fn insert(w: usize, objs: &[usize]) {
    if objs.is_empty() {
        return;
    }
    let list: Vec<(usize, u32)> = objs.iter().map(|&o| (o, 0)).collect();
    let made = insert_recs(w, &list);
    if wins()[w].view == OPEN_CONTENTS {
        place_icons(w, &made);
    }
}

/// Fill a folder view from the folder's content.
pub fn fill(w: usize) {
    let cnr = wins()[w].child;
    send(cnr, CM_REMOVERECORD, 0, mp(0, CMA_FREE | CMA_INVALIDATE));
    wins()[w].recs.clear();
    if wins()[w].view == OPEN_TREE {
        fill_tree(w);
        return;
    }
    let objs = visible_content(wins()[w].obj);
    insert(w, &objs);
}

// ---- Tree views ----

/// What a tree record opens into: a folder, or a drive's root folder.
fn tree_folder(o: usize) -> usize {
    if wps::is(o, "WPFolder") {
        o
    } else if wps::is(o, "WPDisk") {
        abs::root_of(o)
    } else if wps::is(o, "WPShadow") {
        tree_folder(abs::linked(o))
    } else {
        0
    }
}

/// The folder's own record (open), its folders under it, each with a
/// placeholder until it is opened.
fn fill_tree(w: usize) {
    let folder = wins()[w].obj;
    let root = insert_recs(w, &[(folder, 0)]);
    let Some(&root) = root.first() else { return };
    add_subfolders(w, folder, root);
    send(wins()[w].child, CM_EXPANDTREE, root, 0);
}

fn add_subfolders(w: usize, folder: usize, parent: u32) {
    let kids: Vec<usize> = visible_content(folder).into_iter().filter(|&o| tree_folder(o) != 0).collect();
    let list: Vec<(usize, u32)> = kids.iter().map(|&o| (o, parent)).collect();
    let made = insert_recs(w, &list);
    // A placeholder under each: it may have folders of its own.
    let ph: Vec<(usize, u32)> = made.iter().map(|&r| (0, r)).collect();
    insert_recs(w, &ph);
}

/// CN_EXPANDTREE: a record opened for the first time gets its folders in
/// place of its placeholder.
fn expand(w: usize, rec: u32) {
    let Some(ph) = wins()[w].recs.iter().position(|r| r.parent == rec && r.obj == 0) else { return };
    let prec = wins()[w].recs[ph].rec;
    wins()[w].recs.remove(ph);
    let arr = [prec];
    send(wins()[w].child, CM_REMOVERECORD, arr.as_ptr() as u32, mp(1, CMA_FREE | CMA_INVALIDATE));
    let o = rd(rec, R_KEY) as usize;
    let f = tree_folder(o);
    if f != 0 {
        add_subfolders(w, f, rec);
    }
}

// ---- Icon positions ----

/// The name an object's position is kept under.
fn identity(o: usize) -> Vec<u8> {
    match wps::path_of(o) {
        Some(p) if wps::is(o, "WPFileSystem") => util::split_path(&p).1.to_ascii_uppercase(),
        _ => {
            let mut v = b"#".to_vec();
            v.extend_from_slice(&util::hex(obj::handle(o)));
            v
        }
    }
}

fn positions(folder: usize) -> Vec<(Vec<u8>, i32, i32)> {
    let Some(path) = wps::path_of(folder) else { return Vec::new() };
    let Some((_, b)) = util::ea_read(&path, b".ICONPOS") else { return Vec::new() };
    let mut out = Vec::new();
    let mut i = 0;
    while i + 9 <= b.len() {
        let x = i32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let y = i32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]);
        let n = b[i + 8] as usize;
        let Some(name) = b.get(i + 9..i + 9 + n) else { break };
        out.push((name.to_vec(), x, y));
        i += 9 + n;
    }
    out
}

/// Keep the icon positions of a view's folder.
pub fn save_positions(w: usize) {
    if wins()[w].view != OPEN_CONTENTS {
        return;
    }
    let Some(path) = wps::path_of(wins()[w].obj) else { return };
    let mut b = Vec::new();
    let mut put = |name: &[u8], x: i32, y: i32| {
        b.extend_from_slice(&x.to_le_bytes());
        b.extend_from_slice(&y.to_le_bytes());
        b.push(name.len().min(255) as u8);
        b.extend_from_slice(&name[..name.len().min(255)]);
    };
    put(b"@", 0, cnr_height(w));
    for r in wins()[w].recs.iter() {
        put(&identity(r.obj), rd(r.rec, R_X) as i32, rd(r.rec, R_Y) as i32);
    }
    util::ea_write(&path, b".ICONPOS", Some((util::EAT_BINARY, &b)));
}

fn cell(w: usize) -> (i32, i32) {
    if wins()[w].desktop { (96, 86) } else { (88, 70) }
}

/// The grid place `i`: rows from the top left (the desktop: columns),
/// with room for the titles centred under the icons.
fn slot(w: usize, i: i32) -> (i32, i32) {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(wins()[w].child, &mut r) };
    let (aw, ah) = (r.r.max(100), (r.t - 2).max(100));
    let (cw, ch) = cell(w);
    if wins()[w].desktop {
        let per_col = ((ah - 16) / ch).max(1);
        let (col, row) = (i / per_col, i % per_col);
        (16 + col * cw + (cw - 32) / 2 - 16, ah - 12 - 32 - row * ch)
    } else {
        let per_row = ((aw - 8) / cw).max(1);
        let (row, col) = (i / per_row, i % per_row);
        (4 + col * cw + (cw - 32) / 2, ah - 8 - 32 - row * ch)
    }
}

/// Place records: where the folder keeps them, else on free grid places.
fn place_icons(w: usize, recs: &[u32]) {
    let kept = positions(wins()[w].obj);
    let dy = kept.iter().find(|p| p.0 == b"@").map_or(0, |p| cnr_height(w) - p.2);
    let (cw, ch) = cell(w);
    let mut used: Vec<(i32, i32)> = wins()[w].recs.iter().filter(|r| !recs.contains(&r.rec)).map(|r| (rd(r.rec, R_X) as i32, rd(r.rec, R_Y) as i32)).collect();
    let mut unplaced = Vec::new();
    for &r in recs {
        let o = rd(r, R_KEY) as usize;
        let id = identity(o);
        match kept.iter().find(|p| p.0 == id) {
            Some(&(_, x, y)) => {
                wr(r, R_X, x as u32);
                wr(r, R_Y, (y + dy) as u32);
                used.push((x, y + dy));
            }
            None => unplaced.push(r),
        }
    }
    for r in unplaced {
        let mut p = slot(w, 0);
        for i in 0..2000 {
            let s = slot(w, i);
            if !used.iter().any(|u| (u.0 - s.0).abs() < cw / 2 && (u.1 - s.1).abs() < ch / 2) {
                p = s;
                break;
            }
        }
        wr(r, R_X, p.0 as u32);
        wr(r, R_Y, p.1 as u32);
        used.push(p);
    }
    send(wins()[w].child, CM_INVALIDATERECORD, 0, mp(0, CMA_REPOSITION));
}

/// Arrange: every icon on the grid (kept).
pub fn arrange(w: usize) {
    if wins()[w].view != OPEN_CONTENTS {
        return;
    }
    for i in 0..wins()[w].recs.len() {
        let p = slot(w, i as i32);
        let r = wins()[w].recs[i].rec;
        wr(r, R_X, p.0 as u32);
        wr(r, R_Y, p.1 as u32);
    }
    send(wins()[w].child, CM_INVALIDATERECORD, 0, mp(0, CMA_REPOSITION));
    save_positions(w);
}

// ---- Following changes ----

/// An object came into a folder: the folder's views show it.
pub fn added(folder: usize, o: usize) {
    if !visible(o) {
        return;
    }
    for w in 0..wins().len() {
        if wins()[w].view == OPEN_SETTINGS || wins()[w].recs.iter().any(|r| r.obj == o && r.parent == 0) {
            continue;
        }
        if wins()[w].obj == folder && wins()[w].view != OPEN_TREE {
            insert(w, &[o]);
        } else if wins()[w].view == OPEN_TREE && tree_folder(o) != 0 {
            // Under the folder's record, if it is open.
            let parent = wins()[w].recs.iter().find(|r| r.obj == folder).map(|r| r.rec);
            if let Some(p) = parent {
                if !wins()[w].recs.iter().any(|r| r.parent == p && r.obj == 0) && !wins()[w].recs.iter().any(|r| r.parent == p && r.obj == o) {
                    let made = insert_recs(w, &[(o, p)]);
                    let ph: Vec<(usize, u32)> = made.iter().map(|&r| (0, r)).collect();
                    insert_recs(w, &ph);
                }
            }
        }
    }
}

/// An object left a folder (or went).
pub fn removed(folder: usize, o: usize) {
    for w in 0..wins().len() {
        if wins()[w].view == OPEN_SETTINGS {
            continue;
        }
        let tree = wins()[w].view == OPEN_TREE;
        if !(folder == 0 || wins()[w].obj == folder || tree) {
            continue;
        }
        // The object's records (in a tree, with what is under them).
        let mut gone: Vec<u32> = wins()[w].recs.iter().filter(|r| r.obj == o && (folder == 0 || !tree || parent_obj(w, r.parent) == folder)).map(|r| r.rec).collect();
        let mut i = 0;
        while i < gone.len() {
            let p = gone[i];
            gone.extend(wins()[w].recs.iter().filter(|r| r.parent == p).map(|r| r.rec));
            i += 1;
        }
        if gone.is_empty() {
            continue;
        }
        // The top ones are removed (the container removes their children).
        let tops: Vec<u32> = gone.iter().copied().filter(|&r| wins()[w].recs.iter().find(|x| x.rec == r).is_none_or(|x| !gone.contains(&x.parent))).collect();
        send(wins()[w].child, CM_REMOVERECORD, tops.as_ptr() as u32, mp(tops.len() as u32, CMA_FREE | CMA_INVALIDATE));
        wins()[w].recs.retain(|r| !gone.contains(&r.rec));
    }
    // Its own views close.
    let own: Vec<u32> = wins().iter().filter(|w| w.obj == o && !w.desktop).map(|w| w.frame).collect();
    for f in own {
        close_frame(f);
    }
}

fn parent_obj(w: usize, parent: u32) -> usize {
    if parent == 0 {
        return wins()[w].obj;
    }
    wins()[w].recs.iter().find(|r| r.rec == parent).map_or(0, |r| tree_folder(r.obj))
}

/// An object's title or icon changed: the records showing it.
pub fn changed(o: usize) {
    for w in 0..wins().len() {
        let cnr = wins()[w].child;
        let icon = obj::call(m::WPObject::wpQueryIcon, &[o]) as u32;
        for i in 0..wins()[w].recs.len() {
            if wins()[w].recs[i].obj != o {
                continue;
            }
            wins()[w].recs[i].title = title_bytes(o);
            let rec = &wins()[w].recs[i];
            set_rec(rec.rec, rec, icon);
            let list = [rec.rec];
            send(cnr, CM_INVALIDATERECORD, list.as_ptr() as u32, mp(1, CMA_TEXTCHANGED));
        }
        if wins()[w].obj == o && !wins()[w].desktop {
            let mut t = obj::title(o);
            t.extend_from_slice(view_suffix(wins()[w].view));
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
    wins()[w].recs.iter().find(|r| r.rec == rec).map_or(0, |r| r.obj)
}

/// The selected objects (with `on`, the one under the mouse, if it is
/// not selected: it alone).
pub fn selected(w: usize, on: usize) -> Vec<usize> {
    let sel: Vec<usize> = wins()[w].recs.iter().filter(|r| r.obj != 0 && rd(r.rec, R_ATTR) & CRA_SELECTED != 0).map(|r| r.obj).collect();
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
    // BKS_BACKPAGESBR | BKS_MAJORTABRIGHT
    let nb = unsafe {
        WinCreateWindow(client, WC_NOTEBOOK, b"\0".as_ptr(), WS_VISIBLE | 0x0001 | 0x0010, 0, 0, 10, 10, client, HWND_TOP, ID_CHILD, core::ptr::null(), core::ptr::null())
    };
    wins().push(Win { obj: o, view: OPEN_SETTINGS, frame, client, child: nb, recs: Vec::new(), desktop: false, fields: false, height: 0 });
    let w = wins().len() - 1;
    let (sw, sh) = screen();
    unsafe { send(frame, 0x0047, obj::call(m::WPObject::wpQueryIcon, &[o]) as u32, 0) };
    place(frame, wins().len() as i32, (sw * 3 / 5).max(480), (sh * 3 / 5).max(360));
    size_child(w);
    obj::call(m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    // The first page shows: BKM_QUERYPAGEID (BKA_FIRST), BKM_TURNTOPAGE.
    let first = send(nb, 0x0359, 0, mp(0x0004, 0));
    if first != 0 {
        send(nb, 0x0357, first, 0);
    }
    activate(frame);
    frame
}

// ---- Shadows' titles ----

/// WM_DRAWITEM from a container: shadows' titles in the shadow colour
/// (blue; light blue on dark backgrounds; grey if their object is
/// gone), as the Workplace Shell shows them. Other records keep the
/// container's drawing.
fn draw_item(w: usize, oi: u32) -> u32 {
    // OWNERITEM: hwnd, hps, fsState, fsAttribute, .., rclItem, idItem,
    // hItem (CNRDRAWITEMINFO: pRecord, pFieldInfo).
    let (hps, attr) = (rd(oi, 4), rd(oi, 12));
    let rect = RectL { l: rd(oi, 24) as i32, b: rd(oi, 28) as i32, r: rd(oi, 32) as i32, t: rd(oi, 36) as i32 };
    let info = rd(oi, 44);
    if info == 0 || rd(oi, 40) != CMA_TEXT || attr & CRA_SELECTED != 0 {
        return 0;
    }
    let rec = rd(info, 0);
    let Some(r) = wins()[w].recs.iter().find(|r| r.rec == rec) else { return 0 };
    if r.obj == 0 || !wps::is(r.obj, "WPShadow") {
        return 0;
    }
    let broken = abs::linked(r.obj) == 0;
    let bg = background_of(w);
    let fg: u32 = match (broken, dark(bg)) {
        (true, true) => 0xa0a0a0,
        (true, false) => 0x808080,
        (false, true) => 0x9cd8ff,
        (false, false) => 0x0000c0,
    };
    let name = r.title[..r.title.len().saturating_sub(1)].to_vec();
    let icon_view = wins()[w].view == OPEN_CONTENTS;
    unsafe {
        // LCOLF_RGB
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
        WinFillRect(hps, &rect, bg as i32);
        if !icon_view {
            // One line, centred vertically.
            let mut lr = RectL { l: rect.l + 2, ..rect };
            WinDrawText(hps, name.len() as i32, name.as_ptr(), &mut lr, fg as i32, bg as i32, 0x0400);
            return 1;
        }
        // Lines as the container breaks them (at blanks, and at line
        // ends), centred, from the top.
        const LINE: i32 = 16;
        let mut top = rect.t;
        for part in name.split(|&c| c == b'\n') {
            let mut rest: &[u8] = part;
            while !rest.is_empty() && top > rect.b {
                let mut lr = RectL { l: rect.l, r: rect.r, t: top, b: top - LINE };
                // DT_CENTER | DT_WORDBREAK: the characters that fit.
                let n = WinDrawText(hps, rest.len() as i32, rest.as_ptr(), &mut lr, fg as i32, bg as i32, 0x0100 | 0x4000);
                if n <= 0 {
                    break;
                }
                rest = &rest[(n as usize).min(rest.len())..];
                while rest.first() == Some(&b' ') {
                    rest = &rest[1..];
                }
                top -= LINE;
            }
        }
    }
    1
}

// ---- The keyboard ----

/// The containers: Delete, F2, F5 and Shift+F10 from the keyboard.
extern "C" fn cnr_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    if msg == WM_CHAR && a & KC_VIRTUALKEY != 0 && a & KC_KEYUP == 0 {
        if let Some(w) = find_cnr(h) {
            let vk = b >> 16;
            let cur = send(h, CM_QUERYRECORDEMPHASIS, CMA_FIRST, CRA_CURSORED);
            let o = if cur != 0 && cur != 0xffff_ffff { obj_of_rec(w, cur) } else { 0 };
            let frame = wins()[w].frame;
            match vk {
                VK_DELETE if o != 0 => {
                    for x in selected(w, o) {
                        obj::call(m::WPObject::wpMenuItemSelected, &[x, frame as usize, crate::menu::WPMENUID_DELETE as usize]);
                    }
                    return 1;
                }
                VK_F2 if o != 0 => {
                    rename(w, o);
                    return 1;
                }
                VK_F5 => {
                    let f = wins()[w].obj;
                    refresh(f);
                    return 1;
                }
                VK_F10 if a & KC_SHIFT != 0 => {
                    let (target, kind) = if o != 0 { (o, crate::menu::MENU_OBJECTPOPUP) } else { (wins()[w].obj, crate::menu::MENU_OPENVIEWPOPUP) };
                    let mut p = [PointL { x: 40, y: 40 }];
                    if cur != 0 && cur != 0xffff_ffff {
                        // QUERYRECORDRECT: cb, pRecord, fRightSplitWindow, fsExtent.
                        let q: [u32; 4] = [16, cur, 0, 1];
                        let mut r = RectL::default();
                        if send(h, CM_QUERYRECORDRECT, &mut r as *mut RectL as u32, q.as_ptr() as u32) != 0 {
                            p[0] = PointL { x: (r.l + r.r) / 2, y: (r.b + r.t) / 2 };
                        }
                    }
                    unsafe { WinMapWindowPoints(h, HWND_DESKTOP, p.as_mut_ptr(), 1) };
                    let client = wins()[w].client;
                    obj::call(m::WPObject::wpDisplayMenu, &[target, frame as usize, client as usize, p.as_ptr() as usize, kind as usize, 0]);
                    return 1;
                }
                _ => {}
            }
        }
    }
    let old: WndProc = unsafe { core::mem::transmute(OLD_CNR) };
    old(h, msg, a, b)
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
                CN_EXPANDTREE_ => {
                    expand(w, b);
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
        WM_DRAWITEM if a & 0xffff == ID_CHILD => find_client(h).map_or(0, |w| draw_item(w, b)),
        WM_COMMAND => {
            let (_menu, o) = unsafe { POPUP };
            unsafe { POPUP = (0, 0) };
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
        WM_TIMER if a == crate::shutdown::TID_SHUTDOWN => {
            crate::shutdown::tick();
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
