//! Folder windows and the desktop: a container of objects each, filled
//! from the program list, the drives, a directory or the minimized
//! windows, with the folder's shadows; the tree view of drives and
//! directories; opening, pop-up menus, renaming, copying, moving,
//! deleting (dragging and dropping is in dnd.rs).

use alloc::vec::Vec;

use pmwin::shellicons as si;

use crate::fileops;
use crate::objects::{self, icon_pos, set_icon_pos};
use crate::pm::*;
use crate::shadows::{self, Target};
use crate::{icon, sh, sys_icon};

/// What a folder window shows.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Desktop,
    Group(u32),
    Drives,
    /// A directory (full path ending with a backslash).
    Dir(Vec<u8>),
    Minimized,
}

/// An object in a folder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Obj {
    Group(u32),
    Program(u32),
    Drives,
    Drive(u8),
    Shutdown,
    Minimized,
    WindowList,
    /// A file or directory of a directory view or a tree (the item's
    /// path is where it is).
    File { dir: bool },
    /// A minimized frame.
    Window(u32),
    /// A shadow (its id in RS2_Shadows).
    Shadow(u32),
    /// The child of a tree record not expanded yet (it has children).
    Placeholder,
}

pub struct Item {
    pub obj: Obj,
    /// Name with a NUL (the records point at it).
    pub name: Vec<u8>,
    pub icon: u32,
    /// The icon came from WinLoadFileIcon (freed with the item).
    pub file_icon: bool,
    pub rec: u32,
    pub key: u32,
    pub size: u32,
    pub date: u32,
    pub time: u32,
    /// Files: the full path.
    pub path: Vec<u8>,
    /// Tree view: the parent's key (0: a root).
    pub parent: u32,
    /// A shadow whose object is gone.
    pub broken: bool,
}

impl Item {
    pub fn name(&self) -> &[u8] {
        until_nul(&self.name)
    }
}

pub struct View {
    pub kind: Kind,
    pub frame: u32,
    pub client: u32,
    pub cnr: u32,
    pub items: Vec<Item>,
    /// CV_ICON, CV_DETAIL or CV_TREE.
    pub view: u32,
    pub fields: bool,
    /// The container's height (icon positions are from its bottom; they
    /// keep their distance from the top when it changes).
    pub height: i32,
}

const ID_CNR: u32 = 100;
const TID_MINWATCH: u32 = 1;
pub const WM_SHELL_FOCUS: u32 = WM_USER + 1;

// Commands.
pub const CMD_OPEN: u32 = 1;
pub const CMD_SETTINGS: u32 = 2;
pub const CMD_DELETE: u32 = 3;
pub const CMD_RENAME: u32 = 4;
pub const CMD_RESTORE: u32 = 5;
pub const CMD_CLOSEWIN: u32 = 6;
pub const CMD_EDIT: u32 = 7;
pub const CMD_VIEW_ICON: u32 = 10;
pub const CMD_VIEW_DETAIL: u32 = 11;
pub const CMD_VIEW_TREE: u32 = 12;
pub const CMD_ARRANGE: u32 = 13;
pub const CMD_REFRESH: u32 = 14;
pub const CMD_NEWPROG: u32 = 15;
pub const CMD_WINLIST: u32 = 16;
pub const CMD_SHUTDOWN: u32 = 17;
pub const CMD_DESKSETUP: u32 = 18;
pub const CMD_NEWFOLDER: u32 = 19;
pub const CMD_CLOSE: u32 = 20;
pub const CMD_PARENT: u32 = 21;
pub const CMD_NEWGROUP: u32 = 22;
pub const CMD_COPY: u32 = 23;
pub const CMD_MOVE: u32 = 24;
pub const CMD_SHADOW: u32 = 25;
pub const CMD_LOCATE: u32 = 26;

// ---- Folder ids (keys in OS2.INI) ----

pub fn folder_id(k: &Kind) -> Vec<u8> {
    match k {
        Kind::Desktop => b"DESKTOP".to_vec(),
        Kind::Group(h) => [&b"G"[..], &objects::hex(*h)].concat(),
        Kind::Drives => b"DRIVES".to_vec(),
        Kind::Dir(p) => [&b"DIR:"[..], p].concat(),
        Kind::Minimized => b"MINIMIZED".to_vec(),
    }
}

pub fn object_id(it: &Item) -> Vec<u8> {
    match it.obj {
        Obj::Group(h) => [&b"G"[..], &objects::hex(h)].concat(),
        Obj::Program(h) => [&b"P"[..], &objects::hex(h)].concat(),
        Obj::Drives => b"DRIVES".to_vec(),
        Obj::Drive(c) => alloc::vec![b'D', b'R', b'I', b'V', b'E', b':', c],
        Obj::Shutdown => b"SHUTDOWN".to_vec(),
        Obj::Minimized => b"MINIMIZED".to_vec(),
        Obj::WindowList => b"WINLIST".to_vec(),
        Obj::File { .. } => it.name().to_vec(),
        Obj::Window(f) => objects::hex(f),
        Obj::Shadow(id) => shadows::key(id),
        Obj::Placeholder => Vec::new(),
    }
}

/// What a shadow of an item points at.
pub fn target_of(it: &Item) -> Option<Target> {
    Some(match it.obj {
        Obj::Group(h) => Target::Group(h),
        Obj::Program(h) => Target::Program(h),
        Obj::Drives => Target::Drives,
        Obj::Drive(c) => Target::Drive(c),
        Obj::Shutdown => Target::Shutdown,
        Obj::Minimized => Target::Minimized,
        Obj::WindowList => Target::WindowList,
        Obj::File { .. } => Target::Path(it.path.clone()),
        Obj::Shadow(id) => shadows::get(id)?.target,
        Obj::Window(_) | Obj::Placeholder => return None,
    })
}

fn title_of(k: &Kind) -> Vec<u8> {
    match k {
        Kind::Desktop => b"Desktop".to_vec(),
        Kind::Group(h) => objects::titles(SGH_ROOT).into_iter().find(|g| g.0 == *h).map_or(b"Folder".to_vec(), |g| g.3),
        Kind::Drives => b"Drives".to_vec(),
        Kind::Dir(p) => {
            let t = p.strip_suffix(b"\\").filter(|t| t.len() > 2).unwrap_or(p);
            t.to_vec()
        }
        Kind::Minimized => b"Minimized Window Viewer".to_vec(),
    }
}

/// Positions of icons are kept for these folders (directories are
/// arranged).
pub fn keeps_positions(k: &Kind) -> bool {
    !matches!(k, Kind::Dir(_) | Kind::Minimized)
}

pub fn find_client(client: u32) -> Option<usize> {
    sh().views.iter().position(|v| v.client == client)
}

pub fn find_kind(k: &Kind) -> Option<usize> {
    sh().views.iter().position(|v| &v.kind == k)
}

/// The view whose container is `h` or one of its ancestors.
pub fn find_cnr_of(mut h: u32) -> Option<usize> {
    for _ in 0..8 {
        if h == 0 || h == HWND_DESKTOP {
            return None;
        }
        if let Some(i) = sh().views.iter().position(|v| v.cnr == h || v.client == h) {
            return Some(i);
        }
        h = unsafe { WinQueryWindow(h, 5) };
    }
    None
}

pub fn item_by_key(v: usize, key: u32) -> Option<usize> {
    sh().views.get(v)?.items.iter().position(|i| i.key == key)
}

// ---- Opening a folder window ----

/// Open a folder (or bring its window to the front).
pub fn open(kind: Kind) -> usize {
    if let Some(i) = find_kind(&kind) {
        let f = sh().views[i].frame;
        if kind != Kind::Desktop {
            unsafe {
                WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER | if WinQueryWindowULong(f, QWL_STYLE) & WS_MINIMIZED != 0 { SWP_RESTORE } else { 0 });
            }
        }
        return i;
    }
    let desktop = kind == Kind::Desktop;
    let title = z(&title_of(&kind));
    let mut fcf = if desktop { 0 } else { FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST };
    let mut client = 0;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, crate::CLASS.as_ptr(), if desktop { b"\0".as_ptr() } else { title.as_ptr() }, WS_CLIPCHILDREN, 0, 1, &mut client) };
    let (sw, sh_) = screen();
    let id = folder_id(&kind);
    let saved = objects::query(objects::APP_WINDOWS, &id).map(|v| objects::numbers(&v)).filter(|v| v.len() >= 5);
    let view_mode = match saved.as_ref().map(|v| v[4] as u32) {
        Some(m) if m == CV_DETAIL || m == CV_TREE => m,
        _ => CV_ICON,
    };
    let style = WS_VISIBLE | CCS_EXTENDSEL;
    let cnr = unsafe { WinCreateWindow(client, WC_CONTAINER, b"\0".as_ptr(), style, 0, 0, 10, 10, client, HWND_TOP, ID_CNR, core::ptr::null(), core::ptr::null()) };
    unsafe { OLD_CNR = WinSubclassWindow(cnr, cnr_proc) as usize };
    sh().views.push(View { kind: kind.clone(), frame, client, cnr, items: Vec::new(), view: view_mode, fields: false, height: 0 });
    let v = sh().views.len() - 1;
    if desktop {
        let bg = objects::query(objects::APP_DESKTOP, b"Background").and_then(|t| objects::parse_hex(&t)).unwrap_or(0x2e6e8e);
        set_colors(cnr, bg);
        unsafe {
            WinSetWindowPos(frame, HWND_BOTTOM, 0, 0, sw, sh_, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ZORDER);
            Rs2SetDesktopWindow(frame);
            WinStartTimer(sh().hab, client, TID_MINWATCH, 2000);
        }
    } else {
        add_menu(frame);
        let ic = folder_icon(&kind);
        send(frame, 0x0047, ic, 0);
        match saved {
            Some(p) => unsafe { WinSetWindowPos(frame, 0, p[0], p[1], p[2], p[3], SWP_SIZE | SWP_MOVE) },
            None => {
                let n = sh().views.len() as i32;
                let (cx, cy) = ((sw * 11 / 20).max(360), (sh_ / 2).max(260));
                let x = (40 + n * 28) % (sw - cx).max(1);
                let y = (sh_ - cy - 40 - n * 28).max(0);
                unsafe { WinSetWindowPos(frame, 0, x, y, cx, cy, SWP_SIZE | SWP_MOVE) }
            }
        };
    }
    resize(v);
    set_view(v, view_mode);
    fill(v);
    if !desktop {
        unsafe { WinSetWindowPos(frame, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER) };
    }
    unsafe { WinSetFocus(HWND_DESKTOP, cnr) };
    v
}

fn folder_icon(k: &Kind) -> u32 {
    match k {
        Kind::Group(h) if *h == sh().groups.prompts => icon(si::PROMPTS),
        Kind::Dir(_) => sys_icon(SPTR_FOLDER),
        Kind::Drives => icon(si::DRIVES),
        Kind::Minimized => icon(si::MINIMIZED),
        _ => sys_icon(SPTR_FOLDER),
    }
}

/// The desktop's background colour, with white text on dark ones.
pub fn set_colors(cnr: u32, bg: u32) {
    let (r, g, b) = ((bg >> 16) & 0xff, (bg >> 8) & 0xff, bg & 0xff);
    let fg: u32 = if r * 3 + g * 6 + b < 1280 { 0xffffff } else { 0 };
    unsafe {
        WinSetPresParam(cnr, 3, 4, &bg);
        WinSetPresParam(cnr, 1, 4, &fg);
    }
}

fn add_menu(frame: u32) {
    let items = [Menu::Sub(b"~View", alloc::vec![
        Menu::Item(CMD_VIEW_ICON as u16, b"~Icon view", true),
        Menu::Item(CMD_VIEW_DETAIL as u16, b"~Details view", true),
        Menu::Item(CMD_VIEW_TREE as u16, b"~Tree view", true),
        Menu::Sep,
        Menu::Item(CMD_ARRANGE as u16, b"~Arrange", true),
        Menu::Item(CMD_REFRESH as u16, b"~Refresh\tF5", true),
        Menu::Sep,
        Menu::Item(CMD_CLOSE as u16, b"~Close", true),
    ])];
    let tpl = menu_template(&items);
    unsafe { WinCreateMenu(frame, tpl.as_ptr()) };
    send(frame, WM_UPDATEFRAME, FCF_MENU, 0);
}

fn resize(v: usize) {
    let View { client, cnr, .. } = sh().views[v];
    let mut r = RectL::default();
    unsafe {
        WinQueryWindowRect(client, &mut r);
        WinSetWindowPos(cnr, 0, 0, 0, r.r, r.t, SWP_SIZE | SWP_MOVE);
    }
    // A taller or shorter window (a folder resized, the desktop at
    // another screen resolution): the icons stay where they were from
    // the top, as the Workplace Shell's do.
    let (old, new) = (sh().views[v].height, area(v).1);
    sh().views[v].height = new;
    if old != 0 && old != new && keeps_positions(&sh().views[v].kind) && sh().views[v].view == CV_ICON {
        let dy = new - old;
        for it in sh().views[v].items.iter() {
            if it.rec != 0 {
                wr(it.rec, R_Y, (rd(it.rec, R_Y) as i32 + dy) as u32);
            }
        }
        reposition(v);
        save_positions(v, None);
    }
}

/// The key under which a folder's positions record the height they are
/// from.
const HEIGHT_KEY: &[u8] = b"@HEIGHT";

/// Remember a folder window's place and view.
fn save_window(v: usize) {
    let w = &sh().views[v];
    if w.kind == Kind::Desktop || w.frame == 0 {
        return;
    }
    let s = window_pos(w.frame);
    if s.fl & SWP_MINIMIZE != 0 || unsafe { WinQueryWindowULong(w.frame, QWL_STYLE) } & WS_MINIMIZED != 0 {
        return;
    }
    let t = alloc::format!("{} {} {} {} {}", s.x, s.y, s.cx, s.cy, w.view);
    objects::write(objects::APP_WINDOWS, &folder_id(&w.kind), Some(t.as_bytes()));
}

fn close(v: usize) {
    save_window(v);
    let View { frame, cnr, .. } = sh().views[v];
    send(cnr, CM_REMOVERECORD, 0, mp(0, CMA_FREE));
    for it in sh().views[v].items.drain(..) {
        if it.file_icon {
            unsafe { WinFreeFileIcon(it.icon) };
        }
    }
    sh().views.remove(v);
    unsafe { WinDestroyWindow(frame) };
}

// ---- Views ----

fn set_view(v: usize, mode: u32) {
    let cnr = sh().views[v].cnr;
    if mode == CV_DETAIL && !sh().views[v].fields {
        add_fields(v);
    }
    // The tree shows other records (folders, nested).
    let refill = (mode == CV_TREE) != (sh().views[v].view == CV_TREE) && tree_kind(&sh().views[v].kind);
    sh().views[v].view = mode;
    // The shell draws shadows' names itself (CA_OWNERDRAW).
    let attr = CA_OWNERDRAW | match mode {
        CV_DETAIL => CV_DETAIL | CV_MINI | CA_DETAILSVIEWTITLES,
        CV_TREE => CV_TREE | CV_ICON | CV_MINI | CA_TREELINE,
        _ => CV_ICON,
    };
    let mut ci = [0u32; 23];
    ci[0] = 92;
    ci[5] = attr;
    send(cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR);
    save_window(v);
    if refill {
        fill(v);
    }
}

/// Folders whose tree view is a tree of directories.
fn tree_kind(k: &Kind) -> bool {
    matches!(k, Kind::Dir(_) | Kind::Drives)
}

/// Detail view columns: icon, name, size, date, time.
fn add_fields(v: usize) {
    let cnr = sh().views[v].cnr;
    let dir = matches!(sh().views[v].kind, Kind::Dir(_));
    let n = if dir { 5 } else { 2 };
    let first = send(cnr, CM_ALLOCDETAILFIELDINFO, n, 0);
    if first == 0 {
        return;
    }
    let cols: [(u32, &'static [u8], usize); 5] = [
        (CFA_BITMAPORICON | CFA_VCENTER, b"Icon\0", R_HPTR),
        (CFA_STRING | CFA_VCENTER | CFA_SEPARATOR | CFA_FIREADONLY, b"Name\0", R_PSZNAME),
        (CFA_ULONG | CFA_RIGHT | CFA_VCENTER | CFA_SEPARATOR, b"Size\0", R_SIZE),
        (CFA_DATE | CFA_VCENTER | CFA_SEPARATOR, b"Date\0", R_DATE),
        (CFA_TIME | CFA_VCENTER, b"Time\0", R_TIME),
    ];
    let mut f = first;
    for c in cols.iter().take(n as usize) {
        wr(f, 4, c.0);
        wr(f, 8, CFA_LEFT | CFA_VCENTER);
        wr(f, 12, c.1.as_ptr() as u32);
        wr(f, 16, c.2 as u32);
        f = rd(f, 24);
        if f == 0 {
            break;
        }
    }
    // FIELDINFOINSERT: cb, pFieldInfoOrder, fInvalidateFieldInfo, cFieldInfoInsert.
    let fi: [u32; 4] = [16, CMA_END, 1, n];
    send(cnr, CM_INSERTDETAILFIELDINFO, first, fi.as_ptr() as u32);
    sh().views[v].fields = true;
}

// ---- Filling ----

fn new_item(obj: Obj, name: &[u8], icon: u32, file_icon: bool) -> Item {
    let key = sh().next_key;
    sh().next_key += 1;
    Item { obj, name: z(name), icon, file_icon, rec: 0, key, size: 0, date: 0, time: 0, path: Vec::new(), parent: 0, broken: false }
}

/// Icon of a program object.
fn program_icon(p: &objects::Program) -> (u32, bool) {
    if p.exe == b"*" || p.exe.is_empty() {
        return (icon(if p.progc == PROG_FULLSCREEN { si::PROMPT_FULL_SCREEN } else { si::PROMPT_WINDOW }), false);
    }
    let src = if p.icon.is_empty() { &p.exe } else { &p.icon };
    let f = z(src);
    let h = unsafe { WinLoadFileIcon(f.as_ptr(), 0) };
    if h != 0 { (h, true) } else { (sys_icon(SPTR_PROGRAM), false) }
}

fn group_items(group: u32) -> Vec<Item> {
    objects::titles(group)
        .into_iter()
        .map(|(h, _, _, title)| {
            let (ic, own) = objects::definition(h).map_or((sys_icon(SPTR_PROGRAM), false), |p| program_icon(&p));
            new_item(Obj::Program(h), &title, ic, own)
        })
        .collect()
}

/// The drives: (letter, file system name, label).
pub fn drives() -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut cur, &mut map) };
    let mut out = Vec::new();
    for d in 0..26u8 {
        if map & (1 << d) == 0 {
            continue;
        }
        let letter = b'A' + d;
        let dev = [letter, b':', 0];
        let mut buf = [0u8; 64];
        let mut cb = buf.len() as u32;
        let fs = if unsafe { os2::DosQueryFSAttach(dev.as_ptr(), 0, 1, buf.as_mut_ptr(), &mut cb) } == 0 {
            let cb_name = u16::from_le_bytes([buf[2], buf[3]]) as usize;
            let at = 8 + cb_name + 1;
            until_nul(buf.get(at..).unwrap_or(&[])).to_vec()
        } else {
            Vec::new()
        };
        // FSINFO level 2: serial, cch, label.
        let mut info = [0u8; 40];
        let label = if unsafe { os2::DosQueryFSInfo(d as u32 + 1, 2, info.as_mut_ptr(), info.len() as u32) } == 0 {
            let n = (info[4] as usize).min(11);
            info[5..5 + n].to_vec()
        } else {
            Vec::new()
        };
        out.push((letter, fs, label));
    }
    out
}

fn drive_items() -> Vec<Item> {
    drives()
        .into_iter()
        .map(|(c, fs, label)| {
            let mut name = alloc::vec![b'D', b'r', b'i', b'v', b'e', b' ', c];
            if !label.is_empty() {
                name.extend_from_slice(b"\n");
                name.extend_from_slice(&label);
            }
            let cd = fs.eq_ignore_ascii_case(b"CDFS") || fs.eq_ignore_ascii_case(b"ISO9660") || fs.eq_ignore_ascii_case(b"UDF");
            new_item(Obj::Drive(c), &name, icon(if cd { si::CD_ROM } else { si::HARD_DISK }), false)
        })
        .collect()
}

pub use fileops::join;

fn dir_items(dir: &[u8]) -> Vec<Item> {
    let spec = z(&join(dir, b"*"));
    let mut buf = alloc::vec![0u8; 8192];
    let mut hdir = 0xffff_ffffu32; // HDIR_CREATE
    let mut count = 64u32;
    let mut out = Vec::new();
    // FILE_DIRECTORY | FILE_ARCHIVED | FILE_READONLY | FILE_SYSTEM | FILE_HIDDEN
    let mut rc = unsafe { os2::DosFindFirst(spec.as_ptr(), &mut hdir, 0x37, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    let mut found = Vec::new();
    while rc == 0 && count > 0 {
        let mut at = 0usize;
        for _ in 0..count {
            let p = buf.as_ptr() as u32 + at as u32;
            let next = rd(p, 0);
            let date_w = u16::from_le_bytes([buf[at + 12], buf[at + 13]]);
            let time_w = u16::from_le_bytes([buf[at + 14], buf[at + 15]]);
            let size = rd(p, 16);
            let attr = rd(p, 24);
            let n = buf[at + 28] as usize;
            let name = buf[at + 29..at + 29 + n].to_vec();
            if name != b"." && name != b".." {
                found.push((name, attr, size, date_w, time_w));
            }
            if next == 0 {
                break;
            }
            at += next as usize;
        }
        count = 64;
        rc = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    }
    unsafe { os2::DosFindClose(hdir) };
    // Folders first, then by name.
    found.sort_by(|a, b| (a.1 & 0x10 == 0).cmp(&(b.1 & 0x10 == 0)).then_with(|| a.0.to_ascii_uppercase().cmp(&b.0.to_ascii_uppercase())));
    for (name, attr, size, dw, tw) in found {
        let isdir = attr & 0x10 != 0;
        let path = z(&join(dir, &name));
        let h = unsafe { WinLoadFileIcon(path.as_ptr(), 0) };
        let (ic, own) = if h != 0 { (h, true) } else { (sys_icon(if isdir { SPTR_FOLDER } else { SPTR_FILE }), false) };
        let mut it = new_item(Obj::File { dir: isdir }, &name, ic, own);
        it.path = join(dir, &name);
        if !isdir {
            it.size = size;
        }
        // FDATE: day 0-4, month 5-8, year-1980 9-15; FTIME: sec/2 0-4,
        // min 5-10, hour 11-15. CDATE: day, month, year; CTIME: h, m, s.
        let (d, m, y) = ((dw & 31) as u32, ((dw >> 5) & 15) as u32, (dw >> 9) as u32 + 1980);
        it.date = d | m << 8 | y << 16;
        let (hh, mm, ss) = ((tw >> 11) as u32, ((tw >> 5) & 63) as u32, ((tw & 31) * 2) as u32);
        it.time = hh | mm << 8 | ss << 16;
        out.push(it);
    }
    out
}

fn minimized_items() -> Vec<Item> {
    let list: Vec<u32> = sh().minimized.clone();
    list.into_iter()
        .filter(|&f| unsafe { WinIsWindow(sh().hab, f) } != 0)
        .map(|f| {
            let mut t = frame_title(f);
            if t.is_empty() {
                t = b"(untitled)".to_vec();
            }
            let ic = send(f, WM_QUERYICON, 0, 0);
            new_item(Obj::Window(f), &t, if ic != 0 { ic } else { sys_icon(SPTR_APPICON) }, false)
        })
        .collect()
}

fn desktop_items() -> Vec<Item> {
    let g = &sh().groups;
    let (prompts, desk) = (g.prompts, g.desktop);
    let mut v = Vec::new();
    for (h, _, vis, title) in objects::titles(SGH_ROOT) {
        if vis & SHE_INVISIBLE != 0 || h == desk {
            continue;
        }
        let ic = if h == prompts { icon(si::PROMPTS) } else { sys_icon(SPTR_FOLDER) };
        v.push(new_item(Obj::Group(h), &title, ic, false));
    }
    v.push(new_item(Obj::Drives, b"Drives", icon(si::DRIVES), false));
    v.push(new_item(Obj::Minimized, b"Minimized\nWindow Viewer", icon(si::MINIMIZED), false));
    v.push(new_item(Obj::WindowList, b"Window List", icon(si::WINDOW_LIST), false));
    v.extend(group_items(desk));
    v.push(new_item(Obj::Shutdown, b"Shutdown", icon(si::SHUTDOWN), false));
    v
}

/// Fill (or refill) a folder window.
pub fn fill(v: usize) {
    let kind = sh().views[v].kind.clone();
    let cnr = sh().views[v].cnr;
    send(cnr, CM_REMOVERECORD, 0, mp(0, CMA_FREE));
    for it in sh().views[v].items.drain(..) {
        if it.file_icon {
            unsafe { WinFreeFileIcon(it.icon) };
        }
    }
    let tree = sh().views[v].view == CV_TREE && tree_kind(&kind);
    let mut items = match &kind {
        _ if tree => tree_items(&kind),
        Kind::Desktop => desktop_items(),
        Kind::Group(h) => group_items(*h),
        Kind::Drives => drive_items(),
        Kind::Dir(p) => dir_items(p),
        Kind::Minimized => minimized_items(),
    };
    if !tree && !matches!(kind, Kind::Drives | Kind::Minimized) {
        items.extend(shadow_items(&folder_id(&kind)));
    }
    sh().views[v].items = items;
    if tree {
        insert_tree(v);
    } else {
        insert_records(v);
    }
}

// ---- Shadows ----

/// The icon of an object a shadow stands for (and whether it is a file
/// icon to free).
fn target_icon(t: &Target) -> (u32, bool) {
    match t {
        Target::Path(p) => {
            let c = z(p);
            let h = unsafe { WinLoadFileIcon(c.as_ptr(), 0) };
            if h != 0 { (h, true) } else { (sys_icon(if fileops::is_dir(p) { SPTR_FOLDER } else { SPTR_FILE }), false) }
        }
        Target::Program(h) => objects::definition(*h).map_or((sys_icon(SPTR_PROGRAM), false), |p| program_icon(&p)),
        Target::Group(h) => (if *h == sh().groups.prompts { icon(si::PROMPTS) } else { sys_icon(SPTR_FOLDER) }, false),
        Target::Drives => (icon(si::DRIVES), false),
        Target::Drive(_) => (icon(si::HARD_DISK), false),
        Target::Minimized => (icon(si::MINIMIZED), false),
        Target::WindowList => (icon(si::WINDOW_LIST), false),
        Target::Shutdown => (icon(si::SHUTDOWN), false),
    }
}

fn shadow_items(folder: &[u8]) -> Vec<Item> {
    shadows::in_folder(folder)
        .into_iter()
        .map(|s| {
            let ok = s.target.exists();
            let (ic, own) = if ok { target_icon(&s.target) } else { (sys_icon(SPTR_FILE), false) };
            let mut it = new_item(Obj::Shadow(s.id), &s.target.title(), ic, own);
            it.broken = !ok;
            if let Target::Path(p) = &s.target {
                it.path = p.clone();
            }
            it
        })
        .collect()
}

/// Refill the folders that may show shadows (their objects changed).
pub fn refresh_shadows() {
    for v in 0..sh().views.len() {
        if !matches!(sh().views[v].kind, Kind::Drives | Kind::Minimized) && sh().views[v].items.iter().any(|i| matches!(i.obj, Obj::Shadow(_))) {
            fill(v);
        }
    }
}

/// Open what a shadow stands for.
pub fn open_target(t: &Target) {
    match t {
        Target::Path(p) => {
            if fileops::is_dir(p) {
                let mut d = p.clone();
                if !d.ends_with(b"\\") {
                    d.push(b'\\');
                }
                open(Kind::Dir(d));
            } else {
                crate::launch::open_file(p);
            }
        }
        Target::Program(h) => {
            if let Some(p) = objects::definition(*h) {
                crate::launch::start_program(&p);
            }
        }
        Target::Group(h) => {
            open(Kind::Group(*h));
        }
        Target::Drives => {
            open(Kind::Drives);
        }
        Target::Drive(c) => {
            open(Kind::Dir(alloc::vec![*c, b':', b'\\']));
        }
        Target::Minimized => {
            open(Kind::Minimized);
        }
        Target::WindowList => {
            post(HWND_DESKTOP, WM_RS2_WINLIST, 0, 0);
        }
        Target::Shutdown => crate::shutdown::ask(),
    }
}

/// The folder an object is in (Locate).
fn locate(t: &Target) {
    match t {
        Target::Path(p) => {
            let d = fileops::parent(p);
            let v = open(if d.is_empty() { Kind::Drives } else { Kind::Dir(d) });
            let name = fileops::name_of(p).to_vec();
            if let Some(i) = sh().views[v].items.iter().position(|it| it.name().eq_ignore_ascii_case(&name)) {
                let (cnr, rec) = (sh().views[v].cnr, sh().views[v].items[i].rec);
                send(cnr, CM_SETRECORDEMPHASIS, rec, mp(1, CRA_SELECTED | CRA_CURSORED));
            }
        }
        Target::Program(h) => {
            if let Some(g) = objects::titles(SGH_ROOT).into_iter().map(|g| g.0).find(|&g| objects::titles(g).iter().any(|x| x.0 == *h)) {
                if g == sh().groups.desktop {
                    open(Kind::Desktop);
                } else {
                    open(Kind::Group(g));
                }
            }
        }
        Target::Drive(_) => {
            open(Kind::Drives);
        }
        _ => {
            open(Kind::Desktop);
        }
    }
}

// ---- The tree view ----

/// The tree of a directory (itself, expanded, with its subfolders) or of
/// the drives (collapsed). Records with subfolders get a placeholder child
/// until they are expanded.
fn tree_items(k: &Kind) -> Vec<Item> {
    let mut out = Vec::new();
    match k {
        Kind::Dir(p) => {
            let mut root = new_item(Obj::File { dir: true }, &title_of(k), sys_icon(SPTR_FOLDER), false);
            root.path = fileops::strip(p).to_vec();
            let key = root.key;
            out.push(root);
            out.extend(subfolders(p, key));
        }
        Kind::Drives => {
            for mut d in drive_items() {
                let Obj::Drive(c) = d.obj else { continue };
                d.path = alloc::vec![c, b':', b'\\'];
                let key = d.key;
                out.push(d);
                let mut ph = new_item(Obj::Placeholder, b"", 0, false);
                ph.parent = key;
                out.push(ph);
            }
        }
        _ => {}
    }
    out
}

/// A directory's subfolders as tree records under `parent` (each with a
/// placeholder if it has subfolders).
fn subfolders(dir: &[u8], parent: u32) -> Vec<Item> {
    let mut names: Vec<Vec<u8>> = fileops::listing(dir).into_iter().filter(|e| e.1).map(|e| e.0).collect();
    names.sort_by_key(|a| a.to_ascii_uppercase());
    let mut out = Vec::new();
    for n in names {
        let path = join(dir, &n);
        let mut it = new_item(Obj::File { dir: true }, &n, sys_icon(SPTR_FOLDER), false);
        it.parent = parent;
        let key = it.key;
        let more = fileops::has_subdirs(&path);
        it.path = path;
        out.push(it);
        if more {
            let mut ph = new_item(Obj::Placeholder, b"", 0, false);
            ph.parent = key;
            out.push(ph);
        }
    }
    out
}

/// Insert the records of items `from..` (parents before children).
fn insert_tree_from(v: usize, from: usize) {
    let cnr = sh().views[v].cnr;
    let n = sh().views[v].items.len();
    for i in from..n {
        let first = send(cnr, CM_ALLOCRECORD, R_EXTRA, 1);
        if first == 0 {
            return;
        }
        let it = &mut sh().views[v].items[i];
        it.rec = first;
        let name = it.name.as_ptr() as u32;
        for off in [R_PSZICON, R_PSZTEXT, R_PSZNAME, R_PSZTREE] {
            wr(first, off, name);
        }
        wr(first, R_HPTR, it.icon);
        wr(first, R_HPTRMINI, it.icon);
        wr(first, R_KEY, it.key);
        // The directory's own record is open.
        if i == 0 && from == 0 && matches!(sh().views[v].kind, Kind::Dir(_)) {
            wr(first, R_ATTR, rd(first, R_ATTR) | CRA_EXPANDED);
        }
        let parent = it.parent;
        let prec = if parent == 0 { 0 } else { item_by_key(v, parent).map_or(0, |p| sh().views[v].items[p].rec) };
        let ri: [u32; 6] = [24, CMA_END, prec, 0, 1, 1];
        send(cnr, CM_INSERTRECORD, first, ri.as_ptr() as u32);
    }
    unsafe { WinInvalidateRect(cnr, core::ptr::null(), 1) };
}

fn insert_tree(v: usize) {
    insert_tree_from(v, 0);
}

/// CN_EXPANDTREE: a record opened for the first time gets its subfolders
/// in place of its placeholder.
fn expand(v: usize, rec: u32) {
    let Some(i) = sh().views[v].items.iter().position(|it| it.rec == rec) else { return };
    let key = sh().views[v].items[i].key;
    let Some(ph) = sh().views[v].items.iter().position(|it| it.parent == key && it.obj == Obj::Placeholder) else { return };
    let cnr = sh().views[v].cnr;
    let prec = sh().views[v].items[ph].rec;
    let arr = [prec];
    send(cnr, CM_REMOVERECORD, arr.as_ptr() as u32, mp(1, CMA_FREE));
    sh().views[v].items.remove(ph);
    let path = sh().views[v].items.iter().find(|it| it.key == key).map(|it| it.path.clone()).unwrap_or_default();
    let from = sh().views[v].items.len();
    let kids = subfolders(&path, key);
    sh().views[v].items.extend(kids);
    insert_tree_from(v, from);
}

fn insert_records(v: usize) {
    let cnr = sh().views[v].cnr;
    let n = sh().views[v].items.len() as u32;
    if n == 0 {
        unsafe { WinInvalidateRect(cnr, core::ptr::null(), 1) };
        return;
    }
    let first = send(cnr, CM_ALLOCRECORD, R_EXTRA, n);
    if first == 0 {
        return;
    }
    let kind = sh().views[v].kind.clone();
    let fid = folder_id(&kind);
    let keep = keeps_positions(&kind);
    // Positions kept at another height (another screen resolution) keep
    // their distance from the top.
    let dy = if keep { icon_pos(&fid, HEIGHT_KEY).map_or(0, |(h, _)| area(v).1 - h) } else { 0 };
    let mut r = first;
    let mut unplaced = Vec::new();
    let mut used = Vec::new();
    for i in 0..n as usize {
        let it = &mut sh().views[v].items[i];
        it.rec = r;
        let name = it.name.as_ptr() as u32;
        for off in [R_PSZICON, R_PSZTEXT, R_PSZNAME, R_PSZTREE] {
            wr(r, off, name);
        }
        wr(r, R_HPTR, it.icon);
        wr(r, R_HPTRMINI, it.icon);
        wr(r, R_KEY, it.key);
        wr(r, R_SIZE, it.size);
        wr(r, R_DATE, it.date);
        wr(r, R_TIME, it.time);
        let id = object_id(it);
        match if keep { icon_pos(&fid, &id) } else { None } {
            Some((x, y)) => {
                let y = y + dy;
                wr(r, R_X, x as u32);
                wr(r, R_Y, y as u32);
                used.push((x, y));
            }
            None => unplaced.push((i, r)),
        }
        r = rd(r, R_NEXT);
        if r == 0 {
            break;
        }
    }
    // Places for the icons without one, before they are inserted (so
    // the container never sees them all at the origin).
    let fresh = used.is_empty();
    // Icons placed now keep their places (new objects go to free ones).
    let placed_keys: Vec<u32> = unplaced.iter().map(|&(_, r)| rd(r, R_KEY)).collect();
    for (i, r) in unplaced {
        let p = if fresh { slot(v, i as i32) } else { free_slot(v, &used) };
        wr(r, R_X, p.0 as u32);
        wr(r, R_Y, p.1 as u32);
        used.push(p);
    }
    // RECORDINSERT: cb, pRecordOrder, pRecordParent, fInvalidateRecord,
    // zOrder (CMA_TOP), cRecordsInsert.
    let ri: [u32; 6] = [24, CMA_END, 0, 1, 1, n];
    send(cnr, CM_INSERTRECORD, first, ri.as_ptr() as u32);
    reposition(v);
    if keep && (!placed_keys.is_empty() || dy != 0) {
        save_positions(v, if dy != 0 { None } else { Some(&placed_keys) });
    }
}

/// The container repaints and fits its scroll bars to the icons.
pub fn reposition(v: usize) {
    send(sh().views[v].cnr, CM_INVALIDATERECORD, 0, mp(0, CMA_REPOSITION));
}

/// Grid cells: the desktop fills columns from the top left, folders
/// rows.
fn cell(v: usize) -> (i32, i32) {
    if sh().views[v].kind == Kind::Desktop { (96, 86) } else { (88, 70) }
}

fn area(v: usize) -> (i32, i32) {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(sh().views[v].cnr, &mut r) };
    (r.r.max(100), (r.t - 2).max(100))
}

/// The grid position of slot `i`.
fn slot(v: usize, i: i32) -> (i32, i32) {
    let (cw, ch) = cell(v);
    let (w, h) = area(v);
    if sh().views[v].kind == Kind::Desktop {
        let per_col = ((h - 16) / ch).max(1);
        let (col, row) = (i / per_col, i % per_col);
        (16 + col * cw + (cw - 32) / 2 - 16, h - 12 - 32 - row * ch)
    } else {
        let per_row = ((w - 8) / cw).max(1);
        let (row, col) = (i / per_row, i % per_row);
        (4 + col * cw + (cw - 32) / 2, h - 8 - 32 - row * ch)
    }
}

/// The first grid slot no icon is near.
fn free_slot(v: usize, used: &[(i32, i32)]) -> (i32, i32) {
    let (cw, ch) = cell(v);
    for i in 0..1000 {
        let p = slot(v, i);
        if !used.iter().any(|u| (u.0 - p.0).abs() < cw / 2 && (u.1 - p.1).abs() < ch / 2) {
            return p;
        }
    }
    slot(v, 0)
}

/// Arrange the icons on the grid (and keep the positions if `save`).
pub fn arrange(v: usize, save: bool) {
    let n = sh().views[v].items.len();
    for i in 0..n {
        let p = slot(v, i as i32);
        let r = sh().views[v].items[i].rec;
        wr(r, R_X, p.0 as u32);
        wr(r, R_Y, p.1 as u32);
    }
    reposition(v);
    if save {
        save_positions(v, None);
    }
}

/// Keep the icon positions of a folder (all items, or some keys).
pub fn save_positions(v: usize, keys: Option<&[u32]>) {
    let kind = sh().views[v].kind.clone();
    if !keeps_positions(&kind) {
        return;
    }
    let fid = folder_id(&kind);
    set_icon_pos(&fid, HEIGHT_KEY, Some((area(v).1, 0)));
    for it in sh().views[v].items.iter() {
        if keys.is_some_and(|k| !k.contains(&it.key)) {
            continue;
        }
        let p = (rd(it.rec, R_X) as i32, rd(it.rec, R_Y) as i32);
        set_icon_pos(&fid, &object_id(it), Some(p));
    }
}

/// The registered message other programs (the application installer)
/// post to the shell's frames when they changed the program list.
pub static PROGRAM_LIST_CHANGED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

pub fn register_messages() {
    let atom = unsafe { WinAddAtom(WinQuerySystemAtomTable(), b"RS2_ProgramListChanged\0".as_ptr()) };
    PROGRAM_LIST_CHANGED.store(atom, core::sync::atomic::Ordering::Relaxed);
}

/// Refresh the desktop and the program group folders.
pub fn refresh_programs() {
    for v in 0..sh().views.len() {
        if matches!(sh().views[v].kind, Kind::Desktop | Kind::Group(_)) {
            fill(v);
        }
    }
}

/// Refresh every open folder showing a kind.
pub fn refresh_kind(k: &Kind) {
    if let Some(v) = find_kind(k) {
        fill(v);
    }
}

/// Refresh the folder that shows a program group's objects.
pub fn refresh_group(g: u32) {
    if g == sh().groups.desktop {
        refresh_kind(&Kind::Desktop);
    } else {
        refresh_kind(&Kind::Group(g));
    }
}

/// Refresh the directory views of a path's directory.
pub fn refresh_dir(dir: &[u8]) {
    let up = dir.to_ascii_uppercase();
    let list: Vec<usize> = sh().views.iter().enumerate().filter(|(_, v)| matches!(&v.kind, Kind::Dir(p) if p.to_ascii_uppercase() == up)).map(|(i, _)| i).collect();
    for v in list {
        fill(v);
    }
}

/// The group a folder view's program objects belong to.
pub fn group_of(k: &Kind) -> Option<u32> {
    match k {
        Kind::Desktop => Some(sh().groups.desktop),
        Kind::Group(h) => Some(*h),
        _ => None,
    }
}

// ---- Opening objects ----

pub fn open_item(v: usize, i: usize) {
    let it = &sh().views[v].items[i];
    let (obj, name) = (it.obj, it.name().to_vec());
    match obj {
        Obj::Group(h) => {
            open(Kind::Group(h));
        }
        Obj::Program(h) => {
            if let Some(p) = objects::definition(h) {
                crate::launch::start_program(&p);
            }
        }
        Obj::Drives => {
            open(Kind::Drives);
        }
        Obj::Drive(c) => {
            open(Kind::Dir(alloc::vec![c, b':', b'\\']));
        }
        Obj::Shutdown => crate::shutdown::ask(),
        Obj::Minimized => {
            open(Kind::Minimized);
        }
        Obj::WindowList => {
            post(HWND_DESKTOP, WM_RS2_WINLIST, 0, 0);
        }
        Obj::File { dir } => {
            let path = sh().views[v].items[i].path.clone();
            if dir {
                let mut p = path;
                if !p.ends_with(b"\\") {
                    p.push(b'\\');
                }
                open(Kind::Dir(p));
            } else {
                crate::launch::open_file(&path);
            }
        }
        Obj::Window(f) => restore_window(f),
        Obj::Shadow(id) => match shadows::get(id) {
            Some(s) if s.target.exists() => open_target(&s.target),
            _ => {
                let mut t = b"The object \"".to_vec();
                t.extend(name.iter().map(|&c| if c == b'\n' { b' ' } else { c }));
                t.extend_from_slice(b"\" that this shadow stands for cannot be found. Delete the shadow?");
                if message(sh().views[v].frame, &t, b"Broken Shadow", MB_YESNO | MB_WARNING) == MBID_YES {
                    shadows::remove(id);
                    fill(v);
                }
            }
        },
        Obj::Placeholder => {}
    }
}

fn restore_window(f: u32) {
    unsafe { WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, SWP_RESTORE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER) };
}

/// The selected items of a view (or the one the menu was opened on, if
/// it is not selected).
pub fn selected(v: usize, on: u32) -> Vec<usize> {
    let items = &sh().views[v].items;
    let sel: Vec<usize> = (0..items.len()).filter(|&i| rd(items[i].rec, R_ATTR) & CRA_SELECTED != 0).collect();
    match item_by_key(v, on) {
        Some(i) if !sel.contains(&i) => alloc::vec![i],
        _ => sel,
    }
}

// ---- Pop-up menus ----

fn context_menu(v: usize, rec: u32) {
    let key = if rec != 0 { rd(rec, R_KEY) } else { 0 };
    let i = item_by_key(v, key);
    let kind = sh().views[v].kind.clone();
    let mut items: Vec<Menu> = Vec::new();
    match i.map(|i| sh().views[v].items[i].obj) {
        Some(obj) => {
            let open_text: &[u8] = match obj {
                Obj::Window(_) => b"~Restore",
                Obj::Shutdown => b"~Shut down...",
                _ => b"~Open",
            };
            items.push(Menu::Item(CMD_OPEN as u16, open_text, true));
            let builtin_group = matches!(obj, Obj::Group(h) if h == sh().groups.prompts || h == sh().groups.programs);
            let copy_move = |items: &mut Vec<Menu>| {
                items.push(Menu::Item(CMD_COPY as u16, b"~Copy...", true));
                items.push(Menu::Item(CMD_MOVE as u16, b"~Move...", true));
            };
            let shadow = Menu::Item(CMD_SHADOW as u16, b"Create s~hadow...", true);
            match obj {
                Obj::Program(_) => {
                    items.push(Menu::Item(CMD_SETTINGS as u16, b"~Settings...", true));
                    items.push(Menu::Item(CMD_RENAME as u16, b"Re~name\tF2", true));
                    items.push(Menu::Sep);
                    copy_move(&mut items);
                    items.push(shadow);
                    items.push(Menu::Item(CMD_DELETE as u16, b"~Delete...\tDel", true));
                }
                Obj::Group(_) => {
                    items.push(Menu::Item(CMD_RENAME as u16, b"Re~name\tF2", true));
                    items.push(Menu::Sep);
                    items.push(shadow);
                    items.push(Menu::Item(CMD_DELETE as u16, b"~Delete...\tDel", !builtin_group));
                }
                Obj::File { dir } => {
                    if !dir {
                        items.push(Menu::Item(CMD_EDIT as u16, b"Open in ~editor", crate::launch::editor().is_some()));
                    }
                    items.push(Menu::Item(CMD_SETTINGS as u16, b"~Properties...", true));
                    items.push(Menu::Item(CMD_RENAME as u16, b"Re~name\tF2", true));
                    items.push(Menu::Sep);
                    copy_move(&mut items);
                    items.push(shadow);
                    items.push(Menu::Item(CMD_DELETE as u16, b"~Delete...\tDel", true));
                }
                Obj::Shadow(_) => {
                    items.push(Menu::Item(CMD_LOCATE as u16, b"Original: ~Locate", true));
                    items.push(Menu::Sep);
                    copy_move(&mut items);
                    items.push(Menu::Item(CMD_DELETE as u16, b"~Delete shadow...\tDel", true));
                }
                Obj::Window(_) => {
                    items.push(Menu::Item(CMD_CLOSEWIN as u16, b"~Close", true));
                }
                Obj::Drives | Obj::Drive(_) | Obj::Minimized | Obj::WindowList | Obj::Shutdown => {
                    items.push(Menu::Sep);
                    items.push(shadow);
                }
                Obj::Placeholder => {}
            }
        }
        None => {
            if kind != Kind::Desktop {
                items.push(Menu::Sub(b"~View", alloc::vec![
                    Menu::Check(CMD_VIEW_ICON as u16, b"~Icon view", sh().views[v].view == CV_ICON),
                    Menu::Check(CMD_VIEW_DETAIL as u16, b"~Details view", sh().views[v].view == CV_DETAIL),
                    Menu::Check(CMD_VIEW_TREE as u16, b"~Tree view", sh().views[v].view == CV_TREE),
                ]));
            }
            items.push(Menu::Item(CMD_ARRANGE as u16, b"~Arrange", true));
            items.push(Menu::Item(CMD_REFRESH as u16, b"~Refresh\tF5", true));
            if group_of(&kind).is_some() {
                items.push(Menu::Sep);
                items.push(Menu::Item(CMD_NEWPROG as u16, b"Create ~program object...", true));
            }
            if kind == Kind::Desktop {
                items.push(Menu::Item(CMD_NEWGROUP as u16, b"Create ~folder", true));
            }
            if let Kind::Dir(d) = &kind {
                items.push(Menu::Sep);
                items.push(Menu::Item(CMD_NEWFOLDER as u16, b"Create ~folder", true));
                items.push(Menu::Item(CMD_PARENT as u16, b"Open ~parent", d.len() > 3));
            }
            if kind == Kind::Desktop {
                items.push(Menu::Sep);
                items.push(Menu::Item(CMD_WINLIST as u16, b"~Window list\tCtrl+Esc", true));
                items.push(Menu::Item(CMD_DESKSETUP as u16, b"~System setup...", true));
                items.push(Menu::Sep);
                items.push(Menu::Item(CMD_SHUTDOWN as u16, b"Shut ~down...", true));
            } else {
                items.push(Menu::Sep);
                items.push(Menu::Item(CMD_CLOSE as u16, b"~Close", true));
            }
        }
    }
    let old = core::mem::replace(&mut sh().popup, 0);
    if old != 0 {
        unsafe { WinDestroyWindow(old) };
    }
    let m = popup_menu(&items);
    if m == 0 {
        return;
    }
    let client = sh().views[v].client;
    sh().popup = m;
    sh().popup_on = (client, key);
    let p = pointer_pos();
    unsafe { WinPopupMenu(HWND_DESKTOP, client, m, p.x, p.y, 0, PU_HCONSTRAIN | PU_VCONSTRAIN | PU_KEYBOARD | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2) };
}

fn command(v: usize, cmd: u32, key: u32) {
    let kind = sh().views[v].kind.clone();
    match cmd {
        CMD_OPEN => {
            for i in selected(v, key) {
                open_item(v, i);
            }
        }
        CMD_SETTINGS => {
            if let Some(i) = item_by_key(v, key) {
                match sh().views[v].items[i].obj {
                    Obj::Program(h) => crate::dialogs::program_settings(Some(h), group_of(&kind).unwrap_or(sh().groups.desktop)),
                    Obj::File { .. } => {
                        let p = sh().views[v].items[i].path.clone();
                        crate::dialogs::properties(&p);
                    }
                    _ => {}
                }
            }
        }
        CMD_DELETE => delete(v, selected(v, key)),
        CMD_COPY | CMD_MOVE | CMD_SHADOW => copy_command(v, cmd, selected(v, key)),
        CMD_LOCATE => {
            if let Some(Obj::Shadow(id)) = item_by_key(v, key).map(|i| sh().views[v].items[i].obj) {
                if let Some(s) = shadows::get(id) {
                    locate(&s.target);
                }
            }
        }
        CMD_RENAME => {
            if let Some(i) = item_by_key(v, key) {
                rename(v, i);
            }
        }
        CMD_EDIT => {
            if let Some(i) = item_by_key(v, key) {
                let p = sh().views[v].items[i].path.clone();
                if let Some(e) = crate::launch::editor() {
                    crate::launch::start(&e, &p, &fileops::parent(&p), b"", PROG_PM);
                }
            }
        }
        CMD_RESTORE => {
            if let Some(Obj::Window(f)) = item_by_key(v, key).map(|i| sh().views[v].items[i].obj) {
                restore_window(f);
            }
        }
        CMD_CLOSEWIN => {
            if let Some(Obj::Window(f)) = item_by_key(v, key).map(|i| sh().views[v].items[i].obj) {
                post(f, WM_CLOSE, 0, 0);
            }
        }
        CMD_VIEW_ICON | CMD_VIEW_DETAIL | CMD_VIEW_TREE => {
            let m = [CV_ICON, CV_DETAIL, CV_TREE][(cmd - CMD_VIEW_ICON) as usize];
            set_view(v, m);
        }
        CMD_ARRANGE => arrange(v, true),
        CMD_REFRESH => fill(v),
        CMD_NEWPROG => {
            if let Some(g) = group_of(&kind) {
                crate::dialogs::program_settings(None, g);
            }
        }
        CMD_NEWGROUP => new_group(),
        CMD_WINLIST => {
            post(HWND_DESKTOP, WM_RS2_WINLIST, 0, 0);
        }
        CMD_SHUTDOWN => crate::shutdown::ask(),
        CMD_DESKSETUP => crate::dialogs::system_setup(),
        CMD_NEWFOLDER => {
            if let Kind::Dir(d) = &kind {
                new_folder(v, d);
            }
        }
        CMD_PARENT => {
            if let Kind::Dir(d) = &kind {
                let t = d.strip_suffix(b"\\").unwrap_or(d);
                if let Some(cut) = t.iter().rposition(|&c| c == b'\\') {
                    open(Kind::Dir(t[..=cut].to_vec()));
                }
            }
        }
        CMD_CLOSE => {
            if kind != Kind::Desktop {
                close(v);
            }
        }
        _ => {}
    }
}

fn new_group() {
    let mut n = 1;
    let title = loop {
        let t = if n == 1 { b"New Folder".to_vec() } else { alloc::format!("New Folder {n}").into_bytes() };
        if !objects::titles(SGH_ROOT).iter().any(|g| g.3 == t) {
            break t;
        }
        n += 1;
    };
    objects::create_group(&title, SHE_VISIBLE);
    refresh_kind(&Kind::Desktop);
    if let Some(v) = find_kind(&Kind::Desktop) {
        if let Some(i) = sh().views[v].items.iter().position(|it| it.name() == title.as_slice()) {
            rename(v, i);
        }
    }
}

fn new_folder(v: usize, dir: &[u8]) {
    // A long name where the file system takes one (HPFS), else 8.3 (FAT).
    let mut rc = 0;
    let mut name = Vec::new();
    for short in [false, true] {
        let mut n = 1;
        name = loop {
            let t = match (short, n) {
                (false, 1) => b"New Folder".to_vec(),
                (false, _) => alloc::format!("New Folder {n}").into_bytes(),
                (true, 1) => b"NEWFOLDR".to_vec(),
                (true, _) => alloc::format!("NEWFOLD{n}").into_bytes(),
            };
            if !objects::exists(&join(dir, &t)) {
                break t;
            }
            n += 1;
        };
        let p = z(&join(dir, &name));
        rc = unsafe { os2::DosCreateDir(p.as_ptr(), core::ptr::null_mut()) };
        // ERROR_FILENAME_EXCED_RANGE: try a short name.
        if rc != 206 {
            break;
        }
    }
    if rc != 0 {
        error(&alloc::format!("Cannot create the folder (error {rc}).").into_bytes());
        return;
    }
    fill(v);
    if let Some(i) = sh().views[v].items.iter().position(|it| it.name() == name.as_slice()) {
        rename(v, i);
    }
}

pub fn error(text: &[u8]) {
    message(HWND_DESKTOP, text, b"Desktop", MB_OK | MB_ERROR);
}

// ---- Renaming (direct editing) ----

fn rename(v: usize, i: usize) {
    let (cnr, rec) = (sh().views[v].cnr, sh().views[v].items[i].rec);
    match sh().views[v].items[i].obj {
        Obj::Program(_) | Obj::Group(_) | Obj::File { .. } => {}
        _ => return,
    }
    // A tree's own folder is the window's.
    if sh().views[v].view == CV_TREE && sh().views[v].items[i].parent == 0 && matches!(sh().views[v].kind, Kind::Dir(_)) {
        return;
    }
    // CNREDITDATA: cb, hwndCnr, pRecord, pFieldInfo, ppszText, cbText, id.
    let ed: [u32; 7] = [28, cnr, rec, 0, rec + R_PSZICON as u32, 0, 0x7ff0];
    send(cnr, CM_OPENEDIT, ed.as_ptr() as u32, 0);
}

/// CN_REALLOCPSZ: the new text goes into the shell's buffer.
fn realloc_psz(ed: u32) -> u32 {
    let need = rd(ed, 20) as usize;
    sh().edit = alloc::vec![0u8; need.max(1) + 1];
    let pp = rd(ed, 16);
    if pp == 0 {
        return 0;
    }
    wr(pp, 0, sh().edit.as_ptr() as u32);
    1
}

/// CN_ENDEDIT: rename the object; the record points at its name again.
fn end_edit(v: usize, ed: u32) {
    let rec = rd(ed, 8);
    let Some(i) = sh().views[v].items.iter().position(|it| it.rec == rec) else { return };
    let new = until_nul(&sh().edit).to_vec();
    sh().edit.clear();
    let old = sh().views[v].items[i].name().to_vec();
    let new: Vec<u8> = new.iter().copied().filter(|&c| c != b'\r' && c != b'\n').collect();
    let ok = if new.is_empty() || new == old {
        false
    } else {
        match sh().views[v].items[i].obj {
            Obj::Program(h) => objects::definition(h).is_some_and(|mut p| {
                p.title = new.clone();
                objects::change_program(h, &p)
            }),
            Obj::Group(h) => rename_group(h, &new),
            Obj::File { .. } => {
                let from = sh().views[v].items[i].path.clone();
                let to = join(&fileops::parent(&from), &new);
                let (a, b) = (z(&from), z(&to));
                let rc = unsafe { os2::DosMove(a.as_ptr(), b.as_ptr()) };
                if rc != 0 {
                    error(&alloc::format!("Cannot rename the file (error {rc}).").into_bytes());
                } else {
                    sh().views[v].items[i].path = to.clone();
                    if shadows::path_moved(&from, &to) > 0 {
                        post(sh().views[0].client, WM_USER + 3, 0, 0);
                    }
                }
                rc == 0
            }
            _ => false,
        }
    };
    if ok && matches!(sh().views[v].items[i].obj, Obj::Program(_)) && shadows::all().iter().any(|s| matches!(s.target, Target::Program(_))) {
        post(sh().views[0].client, WM_USER + 3, 0, 0);
    }
    let it = &mut sh().views[v].items[i];
    if ok {
        it.name = z(&new);
    }
    let name = it.name.as_ptr() as u32;
    for off in [R_PSZICON, R_PSZTEXT, R_PSZNAME, R_PSZTREE] {
        wr(rec, off, name);
    }
    let cnr = sh().views[v].cnr;
    let arr = [rec];
    send(cnr, CM_INVALIDATERECORD, arr.as_ptr() as u32, mp(1, CMA_TEXTCHANGED));
    if ok {
        if let Obj::Group(h) = sh().views[v].items[i].obj {
            if let Some(g) = find_kind(&Kind::Group(h)) {
                let t = z(&new);
                unsafe { WinSetWindowText(sh().views[g].frame, t.as_ptr()) };
            }
        }
    }
}

/// A group gets a new title: a new group with the programs moved over
/// (the program list has no call to rename a group).
fn rename_group(h: u32, title: &[u8]) -> bool {
    if objects::titles(SGH_ROOT).iter().any(|g| g.3 == title) {
        return false;
    }
    let progs = objects::titles(h);
    let vis = objects::titles(SGH_ROOT).into_iter().find(|g| g.0 == h).map_or(SHE_VISIBLE, |g| g.2);
    let g2 = objects::create_group(title, vis);
    if g2 == 0 {
        return false;
    }
    for (p, ..) in progs {
        if let Some(d) = objects::definition(p) {
            objects::add_program(g2, &d);
        }
    }
    objects::destroy_group(h);
    shadows::group_renamed(h, g2);
    let gs = &mut sh().groups;
    for x in [&mut gs.prompts, &mut gs.programs, &mut gs.startup] {
        if *x == h {
            *x = g2;
        }
    }
    objects::write(objects::APP_DESKTOP, b"PromptsGroup", Some(&objects::hex(sh().groups.prompts)));
    objects::write(objects::APP_DESKTOP, b"ProgramsGroup", Some(&objects::hex(sh().groups.programs)));
    objects::write(objects::APP_DESKTOP, b"StartupGroup", Some(&objects::hex(sh().groups.startup)));
    // The icon keeps its place on the desktop.
    let fid = folder_id(&Kind::Desktop);
    let (o, n) = ([&b"G"[..], &objects::hex(h)].concat(), [&b"G"[..], &objects::hex(g2)].concat());
    if let Some(p) = icon_pos(&fid, &o) {
        set_icon_pos(&fid, &n, Some(p));
    }
    if let Some(v) = find_kind(&Kind::Group(h)) {
        sh().views[v].kind = Kind::Group(g2);
    }
    // The desktop refills after the edit ends.
    post(sh().views[0].client, WM_USER + 2, 0, 0);
    true
}

// ---- Deleting ----

fn delete(v: usize, list: Vec<usize>) {
    let kind = sh().views[v].kind.clone();
    let objs: Vec<(Obj, Vec<u8>, Vec<u8>)> = list.iter().map(|&i| (sh().views[v].items[i].obj, sh().views[v].items[i].name().to_vec(), sh().views[v].items[i].path.clone())).filter(|(o, _, _)| match o {
        Obj::Program(_) | Obj::File { .. } | Obj::Shadow(_) => true,
        Obj::Group(h) => *h != sh().groups.prompts && *h != sh().groups.programs && *h != sh().groups.startup,
        _ => false,
    }).collect();
    if objs.is_empty() {
        return;
    }
    let what = if objs.len() == 1 {
        let (o, n, _) = &objs[0];
        let kind_name: &[u8] = match o {
            Obj::Program(_) => b"the program object",
            Obj::Group(_) => b"the folder and its program objects",
            Obj::File { dir: true } => b"the folder and everything in it",
            Obj::Shadow(_) => b"the shadow (not its object)",
            _ => b"the file",
        };
        let mut t = b"Delete ".to_vec();
        t.extend_from_slice(kind_name);
        t.extend_from_slice(b" \"");
        t.extend(n.iter().map(|&c| if c == b'\n' { b' ' } else { c }));
        t.extend_from_slice(b"\"?");
        t
    } else {
        alloc::format!("Delete these {} objects?", objs.len()).into_bytes()
    };
    if message(sh().views[v].frame, &what, b"Confirm Delete", MB_YESNO | MB_WARNING) != MBID_YES {
        return;
    }
    let fid = folder_id(&kind);
    let mut shadows_changed = 0;
    for (o, name, path) in objs {
        match o {
            Obj::Program(h) => {
                objects::remove_program(h);
                set_icon_pos(&fid, &[&b"P"[..], &objects::hex(h)].concat(), None);
                shadows_changed += shadows::target_deleted(&Target::Program(h));
            }
            Obj::Group(h) => {
                if let Some(g) = find_kind(&Kind::Group(h)) {
                    close(g);
                }
                objects::destroy_group(h);
                set_icon_pos(&fid, &[&b"G"[..], &objects::hex(h)].concat(), None);
                shadows_changed += shadows::target_deleted(&Target::Group(h));
            }
            Obj::File { .. } => {
                let rc = fileops::delete_path(&path);
                if rc != 0 {
                    error(&alloc::format!("Cannot delete {} (error {rc}).", os2::path::from_bytes(&name)).into_bytes());
                    break;
                }
                shadows_changed += shadows::target_deleted(&Target::Path(path.clone()));
                refresh_dir(&fileops::parent(&path));
            }
            Obj::Shadow(id) => {
                shadows::remove(id);
                set_icon_pos(&fid, &shadows::key(id), None);
            }
            _ => {}
        }
    }
    if let Some(v) = find_kind(&kind) {
        fill(v);
    }
    if shadows_changed > 0 {
        refresh_shadows();
    }
}

// ---- Copy, Move, Create shadow ----

/// The target folder dialog for the selected objects, then the copy,
/// move or shadows.
fn copy_command(v: usize, cmd: u32, list: Vec<usize>) {
    let items: Vec<&Item> = list.iter().map(|&i| &sh().views[v].items[i]).filter(|it| !matches!(it.obj, Obj::Placeholder | Obj::Window(_))).collect();
    if items.is_empty() {
        return;
    }
    let files = items.iter().all(|it| matches!(it.obj, Obj::File { .. } | Obj::Shadow(_)));
    let progs = items.iter().all(|it| matches!(it.obj, Obj::Program(_) | Obj::Shadow(_)));
    let shadow = cmd == CMD_SHADOW;
    if !shadow && !files && !progs {
        error(b"Files and program objects cannot be copied or moved together.");
        return;
    }
    let (title, button): (&[u8], &[u8]) = match cmd {
        CMD_COPY => (b"Copy", b"~Copy"),
        CMD_MOVE => (b"Move", b"~Move"),
        _ => (b"Create Shadow", b"C~reate"),
    };
    let one = items.len() == 1 && !shadow && !matches!(items[0].obj, Obj::Shadow(_));
    let name = one.then(|| items[0].name().to_vec());
    let start = match &sh().views[v].kind {
        Kind::Dir(d) => Some(d.clone()),
        _ => None,
    };
    let all_shadows = items.iter().all(|it| matches!(it.obj, Obj::Shadow(_)));
    let (dirs, groups) = if shadow || all_shadows { (true, true) } else { (files, progs) };
    let Some((to, new_name)) = crate::copy::pick_folder(title, button, dirs, groups, name.as_deref(), start.as_deref()) else { return };
    let ops: Vec<(Obj, Vec<u8>, Vec<u8>, Option<Target>)> = items.iter().map(|it| (it.obj, it.name().to_vec(), it.path.clone(), target_of(it))).collect();
    let from = sh().views[v].kind.clone();
    let op = match cmd {
        CMD_COPY => DO_COPY,
        CMD_MOVE => DO_MOVE,
        _ => DO_LINK,
    };
    let rename = if one && !new_name.is_empty() { Some(new_name) } else { None };
    perform(&from, ops, &to, op, rename, None);
}

/// Copy, move or link (make shadows of) objects into a folder: files go
/// to the copy job, the rest happens now. `at` places new icons there
/// (the drop point, in the folder's container).
pub fn perform(from: &Kind, ops: Vec<(Obj, Vec<u8>, Vec<u8>, Option<Target>)>, to: &Kind, op: u32, rename: Option<Vec<u8>>, at: Option<(i32, i32)>) {
    let fid = folder_id(to);
    let mut files = Vec::new();
    let mut placed: Vec<Vec<u8>> = Vec::new();
    let mut groups = Vec::new();
    for (obj, name, path, target) in ops {
        if op == DO_LINK {
            if let Some(t) = target {
                let id = shadows::create(&fid, t);
                placed.push(shadows::key(id));
            }
            continue;
        }
        match (obj, to) {
            (Obj::File { .. }, Kind::Dir(d)) => {
                let n = rename.clone().unwrap_or_else(|| fileops::name_of(&path).to_vec());
                files.push((path, d.clone(), n));
            }
            (Obj::File { dir: false }, _) if op == DO_CREATEPROGRAMOBJECT => {
                // A program file on a group: a program object.
                let Some(g) = group_of(to) else { continue };
                let title = name[..name.iter().rposition(|&c| c == b'.').unwrap_or(name.len())].to_vec();
                let p = objects::Program { progc: PROG_DEFAULT, title, exe: path.clone(), dir: fileops::parent(&path), ..Default::default() };
                let h = objects::add_program(g, &p);
                if h != 0 {
                    placed.push([&b"P"[..], &objects::hex(h)].concat());
                }
                groups.push(g);
            }
            (Obj::Program(h), _) => {
                let (Some(g), Some(src)) = (group_of(to), group_of(from)) else { continue };
                let Some(mut p) = objects::definition(h) else { continue };
                if let Some(t) = &rename {
                    p.title = t.clone();
                }
                if op == DO_MOVE && src == g {
                    continue;
                }
                let n = objects::add_program(g, &p);
                if n != 0 {
                    placed.push([&b"P"[..], &objects::hex(n)].concat());
                    if op == DO_MOVE {
                        objects::remove_program(h);
                        shadows::program_moved(h, n);
                    }
                }
                groups.push(g);
                groups.push(src);
            }
            (Obj::Shadow(id), _) => {
                if op == DO_MOVE {
                    shadows::set_folder(id, &fid);
                    placed.push(shadows::key(id));
                } else if let Some(t) = target {
                    let n = shadows::create(&fid, t);
                    placed.push(shadows::key(n));
                }
            }
            _ => {}
        }
    }
    // New icons where they were dropped.
    if let Some((x, y)) = at.filter(|_| keeps_positions(to)) {
        for (k, id) in placed.iter().enumerate() {
            set_icon_pos(&fid, id, Some((x - 16 + 12 * k as i32, y - 16 - 12 * k as i32)));
        }
    }
    if !files.is_empty() {
        crate::copy::start(if op == DO_MOVE { crate::copy::Op::Move } else { crate::copy::Op::Copy }, files);
    }
    groups.sort();
    groups.dedup();
    for g in groups {
        refresh_group(g);
    }
    refresh_kind(to);
    refresh_kind(from);
    if matches!(to, Kind::Desktop) || matches!(from, Kind::Desktop) {
        refresh_kind(&Kind::Desktop);
    }
    refresh_shadows();
}

// ---- The minimized window viewer ----

fn minimized(frame: u32, on: bool) {
    let list = &mut sh().minimized;
    list.retain(|&f| f != frame);
    if on {
        list.push(frame);
    }
    refresh_kind(&Kind::Minimized);
}

/// Frames that went away while minimized leave the viewer.
fn check_minimized() {
    let hab = sh().hab;
    let before = sh().minimized.len();
    sh().minimized.retain(|&f| unsafe { WinIsWindow(hab, f) != 0 && WinQueryWindowULong(f, QWL_STYLE) & WS_MINIMIZED != 0 });
    if sh().minimized.len() != before {
        refresh_kind(&Kind::Minimized);
    }
}

// ---- Window procedures ----

static mut OLD_CNR: usize = 0;

/// The containers: Delete, F2 and F5 from the keyboard.
extern "C" fn cnr_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    if m == WM_CHAR && a & KC_VIRTUALKEY != 0 && a & KC_KEYUP == 0 {
        let vk = b >> 16;
        if let Some(v) = find_cnr_of(h).filter(|&v| sh().views[v].cnr == h) {
            let cur = send(h, CM_QUERYRECORDEMPHASIS, CMA_FIRST, CRA_CURSORED);
            let key = if cur != 0 && cur != 0xffff_ffff { rd(cur, R_KEY) } else { 0 };
            match vk {
                VK_DELETE => {
                    command(v, CMD_DELETE, key);
                    return 1;
                }
                VK_F2 if key != 0 => {
                    command(v, CMD_RENAME, key);
                    return 1;
                }
                VK_F5 => {
                    fill(v);
                    return 1;
                }
                _ => {}
            }
        }
    }
    let old: WndProc = unsafe { core::mem::transmute(OLD_CNR) };
    old(h, m, a, b)
}

/// WM_DRAWITEM from a container (CA_OWNERDRAW): shadows' names in the
/// shadow colour (blue, light blue on dark backgrounds; grey if broken),
/// as the Workplace Shell shows them. Other records keep the container's.
fn draw_item(v: usize, oi: u32) -> u32 {
    // OWNERITEM: hwnd, hps, fsState, fsAttribute, .., rclItem, idItem,
    // hItem (CNRDRAWITEMINFO: pRecord, pFieldInfo).
    let (hps, attr) = (rd(oi, 4), rd(oi, 12));
    let rect = RectL { l: rd(oi, 24) as i32, b: rd(oi, 28) as i32, r: rd(oi, 32) as i32, t: rd(oi, 36) as i32 };
    let info = rd(oi, 44);
    if info == 0 || rd(oi, 40) != CMA_TEXT || attr & CRA_SELECTED != 0 {
        return 0;
    }
    let rec = rd(info, 0);
    let Some(it) = sh().views[v].items.iter().find(|it| it.rec == rec) else { return 0 };
    if !matches!(it.obj, Obj::Shadow(_)) {
        return 0;
    }
    let bg = background(v);
    let (r, g, b) = ((bg >> 16) & 0xff, (bg >> 8) & 0xff, bg & 0xff);
    let dark = r * 3 + g * 6 + b < 1280;
    let fg: u32 = match (it.broken, dark) {
        (true, true) => 0xa0a0a0,
        (true, false) => 0x808080,
        (false, true) => 0x9cd8ff,
        (false, false) => 0x0000c0,
    };
    let lines: Vec<&[u8]> = it.name().split(|&c| c == b'\n').collect();
    let icon_view = sh().views[v].view == CV_ICON;
    unsafe {
        // LCOLF_RGB
        GpiCreateLogColorTable(hps, 0, 3, 0, 0, core::ptr::null());
        WinFillRect(hps, &rect, bg as i32);
        let lh = (rect.t - rect.b) / lines.len().max(1) as i32;
        for (k, ln) in lines.iter().enumerate() {
            let mut lr = RectL { l: rect.l, r: rect.r, t: rect.t - lh * k as i32, b: rect.t - lh * (k as i32 + 1) };
            if !icon_view {
                lr.l += 2;
            }
            // DT_VCENTER, DT_CENTER
            WinDrawText(hps, ln.len() as i32, ln.as_ptr(), &mut lr, fg as i32, bg as i32, 0x0400 | if icon_view { 0x0100 } else { 0 });
        }
    }
    1
}

/// A folder's background colour (RGB).
fn background(v: usize) -> u32 {
    let cnr = sh().views[v].cnr;
    let mut c = 0u32;
    let mut id = 0u32;
    if unsafe { WinQueryPresParam(cnr, 3, 0, &mut id, 4, &mut c, 0) } == 4 {
        return c;
    }
    0xffffff
}

pub extern "C" fn client_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        m if m != 0 && m == PROGRAM_LIST_CHANGED.load(core::sync::atomic::Ordering::Relaxed) => {
            refresh_programs();
            0
        }
        WM_SIZE => {
            if let Some(v) = find_client(h) {
                resize(v);
            }
            0
        }
        WM_SETFOCUS if b & 0xffff != 0 => {
            post(h, WM_SHELL_FOCUS, 0, 0);
            0
        }
        WM_SHELL_FOCUS => {
            // Only if the focus is still here (not taken by a program
            // started meanwhile).
            if let Some(v) = find_client(h).filter(|_| unsafe { WinQueryFocus(HWND_DESKTOP) } == h) {
                unsafe { WinSetFocus(HWND_DESKTOP, sh().views[v].cnr) };
            }
            0
        }
        WM_CONTROL if a & 0xffff == ID_CNR => {
            let Some(v) = find_client(h) else { return 0 };
            match a >> 16 {
                CN_ENTER => {
                    // NOTIFYRECORDENTER: hwndCnr, fKey, pRecord.
                    let rec = rd(b, 8);
                    if rec != 0 {
                        if let Some(i) = item_by_key(v, rd(rec, R_KEY)) {
                            open_item(v, i);
                        }
                    }
                    0
                }
                CN_CONTEXTMENU => {
                    context_menu(v, b);
                    0
                }
                CN_INITDRAG => crate::dnd::begin(v, b),
                CN_DRAGOVER => crate::dnd::drag_over(v, b),
                CN_DROP => {
                    crate::dnd::drop(v, b);
                    0
                }
                CN_DRAGLEAVE | CN_DROPHELP => 0,
                CN_EXPANDTREE => {
                    expand(v, b);
                    0
                }
                CN_BEGINEDIT => 0,
                CN_REALLOCPSZ => realloc_psz(b),
                CN_ENDEDIT => {
                    end_edit(v, b);
                    0
                }
                _ => 0,
            }
        }
        WM_COMMAND => {
            let Some(v) = find_client(h) else { return 0 };
            let (client, key) = sh().popup_on;
            let key = if client == h { key } else { 0 };
            sh().popup_on = (0, 0);
            command(v, a & 0xffff, key);
            0
        }
        WM_DRAWITEM if a & 0xffff == ID_CNR => find_client(h).map_or(0, |v| draw_item(v, b)),
        DM_RENDER => crate::dnd::render(h, a),
        DM_RENDERPREPARE => 1,
        DM_ENDCONVERSATION => {
            crate::dnd::end_conversation(h, a, b);
            0
        }
        DM_DRAGOVERNOTIFY => 0,
        WM_RS2_MINWINDOW => {
            minimized(a, b != 0);
            0
        }
        WM_TIMER if a == TID_MINWATCH => {
            check_minimized();
            0
        }
        WM_TIMER if a == crate::shutdown::TID_SHUTDOWN => {
            crate::shutdown::tick();
            0
        }
        m if m == WM_USER + 2 => {
            refresh_kind(&Kind::Desktop);
            0
        }
        m if m == WM_USER + 3 => {
            refresh_shadows();
            0
        }
        WM_CLOSE => {
            if let Some(v) = find_client(h) {
                if sh().views[v].kind == Kind::Desktop {
                    // The desktop is closed by shutting down.
                    crate::shutdown::ask();
                } else {
                    close(v);
                }
            }
            0
        }
        WM_SAVEAPPLICATION => {
            for v in 0..sh().views.len() {
                save_window(v);
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
        _ => unsafe { WinDefWindowProc(h, m, a, b) },
    }
}
