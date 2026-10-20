//! Folder windows and the desktop: a container of objects each, filled
//! from the program list, the drives, a directory or the minimized
//! windows; opening, pop-up menus, renaming, deleting and dragging.

use alloc::vec::Vec;

use pmwin::shellicons as si;

use crate::objects::{self, icon_pos, set_icon_pos};
use crate::pm::*;
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
    /// A file or directory of a directory view (its name is the item's).
    File { dir: bool },
    /// A minimized frame.
    Window(u32),
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
}

/// An object being dragged with button 2.
pub struct Drag {
    pub client: u32,
    pub keys: Vec<u32>,
    pub start: PointL,
    pub pointer: u32,
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

fn object_id(it: &Item) -> Vec<u8> {
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
    }
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
fn keeps_positions(k: &Kind) -> bool {
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

fn item_by_key(v: usize, key: u32) -> Option<usize> {
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
    sh().views.push(View { kind: kind.clone(), frame, client, cnr, items: Vec::new(), view: view_mode, fields: false });
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
}

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
    sh().views[v].view = mode;
    let attr = match mode {
        CV_DETAIL => CV_DETAIL | CV_MINI | CA_DETAILSVIEWTITLES,
        CV_TREE => CV_TREE | CV_ICON | CA_TREELINE,
        _ => CV_ICON,
    };
    let mut ci = [0u32; 23];
    ci[0] = 92;
    ci[5] = attr;
    send(cnr, CM_SETCNRINFO, ci.as_ptr() as u32, CMA_FLWINDOWATTR);
    save_window(v);
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
    Item { obj, name: z(name), icon, file_icon, rec: 0, key, size: 0, date: 0, time: 0 }
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

pub fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(name);
    p
}

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
            let tb = unsafe { WinWindowFromID(f, FID_TITLEBAR) };
            let mut t = window_text(if tb != 0 { tb } else { f });
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
    let items = match &kind {
        Kind::Desktop => desktop_items(),
        Kind::Group(h) => group_items(*h),
        Kind::Drives => drive_items(),
        Kind::Dir(p) => dir_items(p),
        Kind::Minimized => minimized_items(),
    };
    sh().views[v].items = items;
    insert_records(v);
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
                wr(r, R_X, x as u32);
                wr(r, R_Y, y as u32);
                used.push((x, y));
            }
            None => unplaced.push(r),
        }
        r = rd(r, R_NEXT);
        if r == 0 {
            break;
        }
    }
    // RECORDINSERT: cb, pRecordOrder, pRecordParent, fInvalidateRecord,
    // zOrder (CMA_TOP), cRecordsInsert.
    let ri: [u32; 6] = [24, CMA_END, 0, 1, 1, n];
    send(cnr, CM_INSERTRECORD, first, ri.as_ptr() as u32);
    if !keep || used.is_empty() {
        arrange(v, false);
    } else {
        for r in unplaced {
            let p = free_slot(v, &used);
            wr(r, R_X, p.0 as u32);
            wr(r, R_Y, p.1 as u32);
            used.push(p);
        }
        unsafe { WinInvalidateRect(cnr, core::ptr::null(), 1) };
    }
}

/// Grid cells: the desktop fills columns from the top left, folders
/// rows.
fn cell(v: usize) -> (i32, i32) {
    if sh().views[v].kind == Kind::Desktop { (96, 86) } else { (88, 70) }
}

fn area(v: usize) -> (i32, i32) {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(sh().views[v].cnr, &mut r) };
    (r.r.max(100), r.t.max(100))
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
    let cnr = sh().views[v].cnr;
    let n = sh().views[v].items.len();
    trace(&alloc::format!("arrange: area {:?}", area(v)));
    for i in 0..n {
        let p = slot(v, i as i32);
        let r = sh().views[v].items[i].rec;
        wr(r, R_X, p.0 as u32);
        wr(r, R_Y, p.1 as u32);
    }
    unsafe { WinInvalidateRect(cnr, core::ptr::null(), 1) };
    if save {
        save_positions(v, None);
    }
}

/// Keep the icon positions of a folder (all items, or some keys).
fn save_positions(v: usize, keys: Option<&[u32]>) {
    let kind = sh().views[v].kind.clone();
    if !keeps_positions(&kind) {
        return;
    }
    let fid = folder_id(&kind);
    for it in sh().views[v].items.iter() {
        if keys.is_some_and(|k| !k.contains(&it.key)) {
            continue;
        }
        let p = (rd(it.rec, R_X) as i32, rd(it.rec, R_Y) as i32);
        set_icon_pos(&fid, &object_id(it), Some(p));
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
            let Kind::Dir(d) = sh().views[v].kind.clone() else { return };
            let path = join(&d, &name);
            if dir {
                let mut p = path;
                p.push(b'\\');
                open(Kind::Dir(p));
            } else {
                crate::launch::open_file(&path);
            }
        }
        Obj::Window(f) => restore_window(f),
    }
}

fn restore_window(f: u32) {
    unsafe { WinSetWindowPos(f, HWND_TOP, 0, 0, 0, 0, SWP_RESTORE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER) };
}

/// The selected items of a view (or the one the menu was opened on, if
/// it is not selected).
fn selected(v: usize, on: u32) -> Vec<usize> {
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
            match obj {
                Obj::Program(_) => {
                    items.push(Menu::Item(CMD_SETTINGS as u16, b"~Settings...", true));
                    items.push(Menu::Item(CMD_RENAME as u16, b"Re~name\tF2", true));
                    items.push(Menu::Sep);
                    items.push(Menu::Item(CMD_DELETE as u16, b"~Delete...\tDel", true));
                }
                Obj::Group(_) => {
                    items.push(Menu::Item(CMD_RENAME as u16, b"Re~name\tF2", true));
                    items.push(Menu::Sep);
                    items.push(Menu::Item(CMD_DELETE as u16, b"~Delete...\tDel", !builtin_group));
                }
                Obj::File { dir } => {
                    if !dir {
                        items.push(Menu::Item(CMD_EDIT as u16, b"Open in ~editor", crate::launch::editor().is_some()));
                    }
                    items.push(Menu::Item(CMD_SETTINGS as u16, b"~Properties...", true));
                    items.push(Menu::Item(CMD_RENAME as u16, b"Re~name\tF2", true));
                    items.push(Menu::Sep);
                    items.push(Menu::Item(CMD_DELETE as u16, b"~Delete...\tDel", true));
                }
                Obj::Window(_) => {
                    items.push(Menu::Item(CMD_CLOSEWIN as u16, b"~Close", true));
                }
                _ => {}
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
                        if let Kind::Dir(d) = &kind {
                            let p = join(d, sh().views[v].items[i].name());
                            crate::dialogs::properties(&p);
                        }
                    }
                    _ => {}
                }
            }
        }
        CMD_DELETE => delete(v, selected(v, key)),
        CMD_RENAME => {
            if let Some(i) = item_by_key(v, key) {
                rename(v, i);
            }
        }
        CMD_EDIT => {
            if let (Some(i), Kind::Dir(d)) = (item_by_key(v, key), &kind) {
                let p = join(d, sh().views[v].items[i].name());
                if let Some(e) = crate::launch::editor() {
                    crate::launch::start(&e, &p, d, b"", PROG_PM);
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
    let mut n = 1;
    let name = loop {
        let t = if n == 1 { b"New Folder".to_vec() } else { alloc::format!("New Folder {n}").into_bytes() };
        if !objects::exists(&join(dir, &t)) {
            break t;
        }
        n += 1;
    };
    let p = z(&join(dir, &name));
    let rc = unsafe { os2::DosCreateDir(p.as_ptr(), core::ptr::null_mut()) };
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
                let Kind::Dir(d) = sh().views[v].kind.clone() else { return };
                let (a, b) = (z(&join(&d, &old)), z(&join(&d, &new)));
                let rc = unsafe { os2::DosMove(a.as_ptr(), b.as_ptr()) };
                if rc != 0 {
                    error(&alloc::format!("Cannot rename the file (error {rc}).").into_bytes());
                }
                rc == 0
            }
            _ => false,
        }
    };
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
    let gs = &mut sh().groups;
    for x in [&mut gs.prompts, &mut gs.programs] {
        if *x == h {
            *x = g2;
        }
    }
    objects::write(objects::APP_DESKTOP, b"PromptsGroup", Some(&objects::hex(sh().groups.prompts)));
    objects::write(objects::APP_DESKTOP, b"ProgramsGroup", Some(&objects::hex(sh().groups.programs)));
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
    let objs: Vec<(Obj, Vec<u8>)> = list.iter().map(|&i| (sh().views[v].items[i].obj, sh().views[v].items[i].name().to_vec())).filter(|(o, _)| match o {
        Obj::Program(_) | Obj::File { .. } => true,
        Obj::Group(h) => *h != sh().groups.prompts && *h != sh().groups.programs,
        _ => false,
    }).collect();
    if objs.is_empty() {
        return;
    }
    let what = if objs.len() == 1 {
        let (o, n) = &objs[0];
        let kind_name: &[u8] = match o {
            Obj::Program(_) => b"the program object",
            Obj::Group(_) => b"the folder and its program objects",
            Obj::File { dir: true } => b"the folder and everything in it",
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
    for (o, name) in objs {
        match o {
            Obj::Program(h) => {
                objects::remove_program(h);
                set_icon_pos(&fid, &[&b"P"[..], &objects::hex(h)].concat(), None);
            }
            Obj::Group(h) => {
                if let Some(g) = find_kind(&Kind::Group(h)) {
                    close(g);
                }
                objects::destroy_group(h);
                set_icon_pos(&fid, &[&b"G"[..], &objects::hex(h)].concat(), None);
            }
            Obj::File { dir } => {
                if let Kind::Dir(d) = &kind {
                    let p = join(d, &name);
                    let rc = if dir { delete_tree(&p) } else { delete_file(&p) };
                    if rc != 0 {
                        error(&alloc::format!("Cannot delete {} (error {rc}).", os2::path::from_bytes(&name)).into_bytes());
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(v) = find_kind(&kind) {
        fill(v);
    }
}

fn delete_file(p: &[u8]) -> u32 {
    let c = z(p);
    unsafe { os2::DosDelete(c.as_ptr()) }
}

/// Delete a directory with everything in it.
pub fn delete_tree(dir: &[u8]) -> u32 {
    for it in dir_listing(dir) {
        let p = join(dir, &it.0);
        let rc = if it.1 { delete_tree(&p) } else { delete_file(&p) };
        if rc != 0 {
            return rc;
        }
    }
    let c = z(dir);
    unsafe { os2::DosDeleteDir(c.as_ptr()) }
}

/// (name, is a directory) of a directory's entries.
pub fn dir_listing(dir: &[u8]) -> Vec<(Vec<u8>, bool)> {
    let spec = z(&join(dir, b"*"));
    let mut buf = alloc::vec![0u8; 4096];
    let mut hdir = 0xffff_ffffu32;
    let mut count = 32u32;
    let mut out = Vec::new();
    let mut rc = unsafe { os2::DosFindFirst(spec.as_ptr(), &mut hdir, 0x37, buf.as_mut_ptr(), buf.len() as u32, &mut count, 1) };
    while rc == 0 && count > 0 {
        let mut at = 0usize;
        for _ in 0..count {
            let next = u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]);
            let attr = u32::from_le_bytes([buf[at + 24], buf[at + 25], buf[at + 26], buf[at + 27]]);
            let n = buf[at + 28] as usize;
            let name = buf[at + 29..at + 29 + n].to_vec();
            if name != b"." && name != b".." {
                out.push((name, attr & 0x10 != 0));
            }
            if next == 0 {
                break;
            }
            at += next as usize;
        }
        count = 32;
        rc = unsafe { os2::DosFindNext(hdir, buf.as_mut_ptr(), buf.len() as u32, &mut count) };
    }
    unsafe { os2::DosFindClose(hdir) };
    out
}

// ---- Dragging ----

/// CN_INITDRAG: the shell drags the object (and the other selected
/// ones) itself.
fn init_drag(v: usize, di: u32) -> u32 {
    let rec = rd(di, 4);
    if rec == 0 {
        return 0;
    }
    let key = rd(rec, R_KEY);
    let list: Vec<u32> = selected(v, key).into_iter().map(|i| sh().views[v].items[i].key).collect();
    if list.is_empty() {
        return 0;
    }
    let pointer = if list.len() > 1 { sys_icon(SPTR_MULTFILE) } else { item_by_key(v, key).map_or(sys_icon(SPTR_FILE), |i| sh().views[v].items[i].icon) };
    let client = sh().views[v].client;
    sh().drag = Some(Drag { client, keys: list, start: PointL { x: rd(di, 8) as i32, y: rd(di, 12) as i32 }, pointer });
    unsafe {
        WinSetCapture(HWND_DESKTOP, client);
        WinSetPointer(HWND_DESKTOP, pointer);
    }
    1
}

fn end_drag(drop: bool) {
    let Some(d) = sh().drag.take() else { return };
    unsafe {
        WinSetCapture(HWND_DESKTOP, 0);
        WinSetPointer(HWND_DESKTOP, sys_icon(SPTR_ARROW));
    }
    if !drop {
        return;
    }
    let Some(src) = find_client(d.client) else { return };
    let end = pointer_pos();
    let under = unsafe { WinWindowFromPoint(HWND_DESKTOP, &end, 1) };
    let Some(dst) = find_cnr_of(under) else { return };
    // An object under the pointer in the target folder.
    let target = record_at(dst, end).filter(|t| !(dst == src && d.keys.contains(&sh().views[dst].items[*t].key)));
    let into: Option<Kind> = match target.map(|t| (sh().views[dst].items[t].obj, t)) {
        Some((Obj::Group(h), _)) => Some(Kind::Group(h)),
        Some((Obj::Drive(c), _)) => Some(Kind::Dir(alloc::vec![c, b':', b'\\'])),
        Some((Obj::File { dir: true }, t)) => match &sh().views[dst].kind {
            Kind::Dir(p) => {
                let mut q = join(p, sh().views[dst].items[t].name());
                q.push(b'\\');
                Some(Kind::Dir(q))
            }
            _ => None,
        },
        Some(_) => None,
        None if dst != src => Some(sh().views[dst].kind.clone()),
        None => None,
    };
    match into {
        Some(k) if k != sh().views[src].kind => drop_into(src, &d.keys, &k),
        Some(_) => {}
        None if dst == src && target.is_none() && sh().views[src].view == CV_ICON => {
            let (dx, dy) = (end.x - d.start.x, end.y - d.start.y);
            for &k in &d.keys {
                if let Some(i) = item_by_key(src, k) {
                    let r = sh().views[src].items[i].rec;
                    wr(r, R_X, (rd(r, R_X) as i32 + dx).max(0) as u32);
                    wr(r, R_Y, (rd(r, R_Y) as i32 + dy).max(0) as u32);
                }
            }
            unsafe { WinInvalidateRect(sh().views[src].cnr, core::ptr::null(), 1) };
            save_positions(src, Some(&d.keys));
        }
        None => {}
    }
}

/// The item whose icon or text is under a desktop point.
fn record_at(v: usize, p: PointL) -> Option<usize> {
    let cnr = sh().views[v].cnr;
    let mut pt = [p];
    unsafe { WinMapWindowPoints(HWND_DESKTOP, cnr, pt.as_mut_ptr(), 1) };
    let p = pt[0];
    for (i, it) in sh().views[v].items.iter().enumerate() {
        let mut r = RectL::default();
        // QUERYRECORDRECT: cb, pRecord, fRightSplitWindow, fsExtent
        // (CMA_ICON | CMA_TEXT).
        let q: [u32; 4] = [16, it.rec, 0, 0x0800 | 0x1000];
        if send(cnr, CM_QUERYRECORDRECT, &mut r as *mut RectL as u32, q.as_ptr() as u32) != 0 && p.x >= r.l && p.x < r.r && p.y >= r.b && p.y < r.t {
            return Some(i);
        }
    }
    None
}

/// Objects dropped on another folder: files move, program objects move
/// to the other group, programs dropped on a group get a program object.
fn drop_into(src: usize, keys: &[u32], to: &Kind) {
    let src_kind = sh().views[src].kind.clone();
    let mut changed_groups = Vec::new();
    let mut changed_dirs = Vec::new();
    for &k in keys {
        let Some(i) = item_by_key(src, k) else { continue };
        let (obj, name) = (sh().views[src].items[i].obj, sh().views[src].items[i].name().to_vec());
        match (obj, &src_kind, to) {
            (Obj::File { .. }, Kind::Dir(from), Kind::Dir(dest)) => {
                let (a, b) = (join(from, &name), join(dest, &name));
                let rc = move_path(&a, &b);
                if rc != 0 {
                    error(&alloc::format!("Cannot move {} (error {rc}).", os2::path::from_bytes(&name)).into_bytes());
                    break;
                }
                changed_dirs.push(from.clone());
                changed_dirs.push(dest.clone());
            }
            (Obj::File { dir: false }, Kind::Dir(from), _) => {
                // A program file dropped on a group: a program object.
                let Some(g) = group_of(to) else { continue };
                let up = name.to_ascii_uppercase();
                if [&b".EXE"[..], b".COM", b".CMD", b".BAT"].iter().any(|e| up.ends_with(e)) {
                    let title = name[..name.iter().rposition(|&c| c == b'.').unwrap_or(name.len())].to_vec();
                    let p = objects::Program { progc: PROG_DEFAULT, title, exe: join(from, &name), dir: from.clone(), ..Default::default() };
                    objects::add_program(g, &p);
                    changed_groups.push(g);
                }
            }
            (Obj::Program(h), _, _) => {
                let (Some(from), Some(g)) = (group_of(&src_kind), group_of(to)) else { continue };
                if from == g {
                    continue;
                }
                if let Some(p) = objects::definition(h) {
                    if objects::add_program(g, &p) != 0 {
                        objects::remove_program(h);
                    }
                }
                changed_groups.push(from);
                changed_groups.push(g);
            }
            _ => {}
        }
    }
    changed_groups.dedup();
    for g in changed_groups {
        refresh_group(g);
    }
    changed_dirs.dedup();
    for d in changed_dirs {
        refresh_dir(&d);
    }
}

/// Move a file or directory; across drives copy, then delete.
fn move_path(a: &[u8], b: &[u8]) -> u32 {
    let (ca, cb) = (z(a), z(b));
    let rc = unsafe { os2::DosMove(ca.as_ptr(), cb.as_ptr()) };
    if rc != 17 {
        return rc;
    }
    // ERROR_NOT_SAME_DEVICE
    let rc = unsafe { os2::DosCopy(ca.as_ptr(), cb.as_ptr(), 0) };
    if rc != 0 {
        return rc;
    }
    let isdir = {
        let mut st = os2::FileStatus3::default();
        unsafe { os2::DosQueryPathInfo(ca.as_ptr(), 1, &mut st as *mut _ as *mut u8, core::mem::size_of::<os2::FileStatus3>() as u32) };
        st.attr & 0x10 != 0
    };
    if isdir { delete_tree(a) } else { delete_file(a) }
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

pub extern "C" fn client_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
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
            if let Some(v) = find_client(h) {
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
                CN_INITDRAG => init_drag(v, b),
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
        WM_MOUSEMOVE if sh().drag.is_some() => {
            let p = sh().drag.as_ref().map_or(0, |d| d.pointer);
            unsafe { WinSetPointer(HWND_DESKTOP, p) };
            1
        }
        WM_BUTTON2UP if sh().drag.is_some() => {
            end_drag(true);
            1
        }
        WM_BUTTON1DOWN if sh().drag.is_some() => {
            end_drag(false);
            1
        }
        WM_CHAR if sh().drag.is_some() => {
            end_drag(false);
            1
        }
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
