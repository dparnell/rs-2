//! APPINST.EXE: installs the OS/2 applications rs2 runs from the OS/2 Warp
//! 4.52 CD onto the boot drive.
//!
//! The applications are in a tree by category (apps.txt): checking a
//! category checks all its applications, and each can be checked by
//! itself; an application also brings what it needs (PM REXX: REXX).
//! Install unpacks their files from the CD's packed files to where OS/2
//! keeps them (`\OS2\APPS\KLONDIKE.EXE` ...), with their dates and
//! extended attributes, on a second thread with a progress bar; then it
//! makes their program objects in a desktop folder per category (program
//! groups, which rs2's desktop shows as folders; it is told to refresh
//! with the registered message `RS2_ProgramListChanged`), and adds
//! `C:\OS2\APPS` to CONFIG.SYS's PATH and LIBPATH when an application
//! there needs it (restart to use).
//!
//! `APPINST [X:]` takes the CD from drive X: instead of looking for it.

#![no_std]
#![no_main]

extern crate alloc;

mod catalog;
mod media;
mod pm;
mod tree;

use alloc::vec::Vec;
use catalog::Category;
use pm::*;
use tree::{Row, Tree};

os2::entry!(main);

const CLASS: &[u8] = b"AppInst\0";
pub const ID_TREE: u32 = 20;
const ID_INSTALL: u32 = 1;
const ID_CLOSE: u32 = 2;
const ID_REFRESH: u32 = 3;
const ID_LABEL: u32 = 10;
const ID_SOURCE: u32 = 11;
const ID_DESC: u32 = 30;
const ID_STATUS: u32 = 40;
const WM_PROGRESS: u32 = WM_USER + 1;
const WM_DONE: u32 = WM_USER + 2;
const SYSCLR_DIALOGBACKGROUND: i32 = -34;

pub struct App {
    hab: u32,
    frame: u32,
    client: u32,
    pub cats: Vec<Category>,
    pub tree: Tree,
    /// The CD's drive, and a drive given on the command line.
    cd: Option<u8>,
    cd_arg: Option<u8>,
    target: u8,
    busy: bool,
    progress: (u32, u32),
}

static mut APP: Option<App> = None;

pub fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

/// What the install thread is given and reports.
struct Job {
    client: u32,
    cd: u8,
    target: u8,
    /// (target path, CD file) of each file.
    files: Vec<(&'static [u8], &'static [u8])>,
    /// Whether `\OS2\APPS` must be on PATH and LIBPATH.
    apps_dir: bool,
    current: Vec<u8>,
    errors: Vec<Vec<u8>>,
    config_changed: bool,
}

static mut JOB: Option<Job> = None;

fn job() -> &'static mut Job {
    unsafe { (*core::ptr::addr_of_mut!(JOB)).as_mut().unwrap() }
}

fn main(s: &os2::Startup) -> u32 {
    // No hard-error pop-ups for empty drives while looking for the CD.
    unsafe { os2::dos::DosError(0) };
    let arg = s.args().iter().copied().find(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_uppercase());
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let cats = catalog::load();
    let sizes: Vec<usize> = cats.iter().map(|c| c.apps.len()).collect();
    unsafe {
        *core::ptr::addr_of_mut!(APP) = Some(App {
            hab,
            frame: 0,
            client: 0,
            cats,
            tree: Tree::new(&sizes),
            cd: None,
            cd_arg: arg,
            target: media::boot_drive(),
            busy: false,
            progress: (0, 0),
        });
        WinRegisterClass(hab, CLASS.as_ptr(), client_proc, WS_CLIPCHILDREN, 0);
    }
    tree::register(hab);
    let a = app();
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINBUTTON | FCF_MAXBUTTON | FCF_TASKLIST | FCF_NOBYTEALIGN;
    let mut client = 0u32;
    a.frame = unsafe {
        WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), b"Install OS/2 Applications\0".as_ptr(), WS_CLIPCHILDREN, 0, 0, &mut client)
    };
    let (sw, sh) = (sysval(SV_CXSCREEN), sysval(SV_CYSCREEN));
    let (cx, cy) = (600.min(sw), 520.min(sh));
    unsafe {
        WinSetWindowPos(a.frame, HWND_TOP, (sw - cx) / 2, (sh - cy) / 2, cx, cy, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
        WinSetFocus(HWND_DESKTOP, a.tree.hwnd);
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    unsafe {
        WinDestroyWindow(a.frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}

fn child(parent: u32, class: &[u8], text: &[u8], style: u32, id: u32) -> u32 {
    let t = z(text);
    unsafe { WinCreateWindow(parent, class.as_ptr(), t.as_ptr(), style, 0, 0, 0, 0, parent, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
}

fn item(id: u32) -> u32 {
    unsafe { WinWindowFromID(app().client, id) }
}

fn set_text(id: u32, t: &[u8]) {
    let t = z(t);
    unsafe { WinSetWindowText(item(id), t.as_ptr()) };
}

fn create(a: &mut App, h: u32) {
    a.client = h;
    let text = WS_VISIBLE | SS_TEXT | DT_LEFT | DT_VCENTER;
    child(h, b"#5\0", b"Source:", text, ID_LABEL);
    child(h, b"#5\0", b"", text, ID_SOURCE);
    child(h, b"#3\0", b"~Refresh", WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON, ID_REFRESH);
    a.tree.hwnd = child(h, tree::CLASS, b"", WS_VISIBLE | WS_TABSTOP, ID_TREE);
    child(h, b"#5\0", b"", WS_VISIBLE | SS_TEXT | DT_LEFT | DT_WORDBREAK, ID_DESC);
    child(h, b"#5\0", b"", text, ID_STATUS);
    child(h, b"#3\0", b"~Install", WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON, ID_INSTALL);
    child(h, b"#3\0", b"Close", WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON, ID_CLOSE);
    refresh(a);
}

/// Look for the CD and at what is installed already.
fn refresh(a: &mut App) {
    a.cd = match a.cd_arg {
        Some(d) if media::exists(&media::on(d, b"\\OS2IMAGE")) => Some(d),
        _ => media::find_cd(),
    };
    for (c, cat) in a.cats.iter().enumerate() {
        for (i, ap) in cat.apps.iter().enumerate() {
            a.tree.installed[c][i] = !ap.files.is_empty() && ap.files.iter().all(|f| media::exists(&media::on(a.target, f.target)));
        }
    }
    let src: Vec<u8> = match a.cd {
        Some(d) => {
            let mut s = media::on(d, b"\\");
            s.extend_from_slice(b"  OS/2 Warp CD, installing on drive ");
            s.push(a.target);
            s.push(b':');
            s
        }
        None => b"No OS/2 Warp CD found: put it in a drive and choose Refresh.".to_vec(),
    };
    set_text(ID_SOURCE, &src);
    update(a);
}

fn find_app(a: &App, id: &[u8]) -> Option<(usize, usize)> {
    a.cats.iter().enumerate().find_map(|(c, cat)| cat.apps.iter().position(|ap| ap.id == id).map(|i| (c, i)))
}

/// Check or uncheck a row; a checked application also checks what it
/// needs.
pub fn toggle(a: &mut App, r: Row) {
    if a.busy {
        return;
    }
    tree::set(&mut a.tree, r);
    for _ in 0..4 {
        let mut want = Vec::new();
        for (c, cat) in a.cats.iter().enumerate() {
            for (i, ap) in cat.apps.iter().enumerate() {
                if a.tree.checked[c][i] {
                    want.extend(ap.needs.iter().filter_map(|n| find_app(a, n)));
                }
            }
        }
        for (c, i) in want {
            a.tree.checked[c][i] = true;
        }
    }
}

/// The description, the status line and the Install button.
fn update(a: &mut App) {
    let desc: Vec<u8> = match a.tree.focused() {
        Some(Row::Cat(c)) => {
            let cat = &a.cats[c];
            let mut s = cat.title.to_vec();
            s.extend_from_slice(b": ");
            s.extend_from_slice(&tree::dec(cat.apps.len() as u32));
            s.extend_from_slice(b" applications. Check the box to install them all, or open the category to choose.");
            if let Some(f) = cat.folder {
                s.extend_from_slice(b" Their objects go in the ");
                s.extend_from_slice(f);
                s.extend_from_slice(b" folder on the desktop.");
            }
            s
        }
        Some(Row::App(c, i)) => {
            let ap = &a.cats[c].apps[i];
            let mut s = ap.description.to_vec();
            for n in &ap.needs {
                if let Some((nc, ni)) = find_app(a, n) {
                    s.extend_from_slice(b" Installed with ");
                    s.extend_from_slice(a.cats[nc].apps[ni].name);
                    s.push(b'.');
                }
            }
            s
        }
        None => Vec::new(),
    };
    set_text(ID_DESC, &desc);
    let n = a.tree.checked.iter().flatten().filter(|&&b| b).count();
    if !a.busy {
        let mut s = tree::dec(n as u32);
        s.extend_from_slice(if n == 1 { b" application selected." } else { b" applications selected." });
        set_text(ID_STATUS, &s);
    }
    unsafe {
        WinEnableWindow(item(ID_INSTALL), (n > 0 && a.cd.is_some() && !a.busy) as u32);
        WinEnableWindow(item(ID_REFRESH), (!a.busy) as u32);
    }
}

fn layout(a: &App) {
    let r = window_rect(a.client);
    let (w, h) = (r.r, r.t);
    let pos = |id: u32, x: i32, y: i32, cx: i32, cy: i32| unsafe {
        WinSetWindowPos(item(id), 0, x, y, cx.max(0), cy.max(0), SWP_MOVE | SWP_SIZE);
    };
    pos(ID_LABEL, 8, h - 32, 64, 24);
    pos(ID_SOURCE, 76, h - 32, w - 76 - 108, 24);
    pos(ID_REFRESH, w - 104, h - 34, 96, 28);
    pos(ID_INSTALL, w - 212, 8, 100, 28);
    pos(ID_CLOSE, w - 104, 8, 96, 28);
    pos(ID_STATUS, 8, 10, w - 228, 24);
    pos(ID_DESC, 8, 62, w - 16, 72);
    pos(ID_TREE, 8, 140, w - 16, h - 40 - 140);
}

/// The progress bar's rectangle.
fn bar(a: &App) -> RectL {
    let r = window_rect(a.client);
    RectL { l: 8, b: 42, r: r.r - 8, t: 54 }
}

fn paint(a: &App, h: u32) {
    let hps = unsafe { WinBeginPaint(h, 0, core::ptr::null_mut()) };
    let r = window_rect(h);
    unsafe { WinFillRect(hps, &r, SYSCLR_DIALOGBACKGROUND) };
    let (done, total) = a.progress;
    if total > 0 {
        let b = bar(a);
        let fill = b.l + ((b.r - b.l) as i64 * done as i64 / total as i64) as i32;
        unsafe {
            WinFillRect(hps, &RectL { l: b.l, b: b.b, r: fill, t: b.t }, SYSCLR_HILITEBACKGROUND);
            WinFillRect(hps, &RectL { l: fill, b: b.b, r: b.r, t: b.t }, SYSCLR_WINDOW);
            GpiSetColor(hps, SYSCLR_WINDOWTEXT);
            GpiMove(hps, &PointL { x: b.l, y: b.b });
            GpiBox(hps, DRO_OUTLINE, &PointL { x: b.r - 1, y: b.t - 1 }, 0, 0);
        }
    }
    unsafe { WinEndPaint(hps) };
}

// ---- Installing ----

fn start(a: &mut App) {
    let Some(cd) = a.cd else { return };
    if !media::writable(a.target) {
        let mut m = b"Drive ".to_vec();
        m.push(a.target);
        m.extend_from_slice(b": cannot be written: rs2 is running from a read-only medium. Install rs2 on a disk (or run it from a USB stick) to add applications.");
        message(&m, MB_ERROR);
        return;
    }
    let mut files: Vec<(&'static [u8], &'static [u8])> = Vec::new();
    let mut apps_dir = false;
    for (c, cat) in a.cats.iter().enumerate() {
        for (i, ap) in cat.apps.iter().enumerate() {
            if !a.tree.checked[c][i] {
                continue;
            }
            for f in &ap.files {
                if !files.iter().any(|(t, _)| t.eq_ignore_ascii_case(f.target)) {
                    files.push((f.target, f.source));
                }
                apps_dir |= f.target.len() > 10 && f.target[..10].eq_ignore_ascii_case(b"\\OS2\\APPS\\");
            }
        }
    }
    // One packed file after the other: each is read once.
    files.sort_by(|x, y| x.1.cmp(y.1));
    let total = files.len() as u32;
    unsafe {
        *core::ptr::addr_of_mut!(JOB) =
            Some(Job { client: a.client, cd, target: a.target, files, apps_dir, current: Vec::new(), errors: Vec::new(), config_changed: false });
    }
    a.busy = true;
    a.progress = (0, total);
    update(a);
    let mut tid = 0u32;
    unsafe { os2::dos::DosCreateThread(&mut tid, worker, 0, 0, 256 * 1024) };
}

unsafe extern "C" fn worker(_: u32) {
    let j = job();
    let mut cd = media::Cd::new(j.cd);
    let total = j.files.len() as u32;
    for i in 0..j.files.len() {
        let (target, source) = j.files[i];
        j.current = target.to_vec();
        post(j.client, WM_PROGRESS, i as u32, total);
        let res = cd.get(target, source).map_err(|e| e.as_bytes().to_vec()).and_then(|f| {
            media::write(&media::on(j.target, target), &f).map_err(|rc| {
                let mut s = b"cannot be written (error ".to_vec();
                s.extend_from_slice(&tree::dec(rc));
                s.push(b')');
                s
            })
        });
        if let Err(e) = res {
            let mut s = target.to_vec();
            s.push(b' ');
            s.extend_from_slice(&e);
            j.errors.push(s);
        }
    }
    if j.apps_dir {
        let mut dir = alloc::vec![j.target];
        dir.extend_from_slice(b":\\OS2\\APPS");
        j.config_changed = media::add_to_paths(j.target, &dir);
    }
    post(j.client, WM_DONE, total, 0);
}

/// The programs of a group (SGH_ROOT: the groups) as (handle, title).
fn titles(group: u32) -> Vec<(u32, Vec<u8>)> {
    let mut count = 0u32;
    let need = unsafe { PrfQueryProgramTitles(HINI_USERPROFILE, group, core::ptr::null_mut(), 0, &mut count) };
    if need == 0 {
        return Vec::new();
    }
    let mut buf = alloc::vec![0u8; need as usize + 16];
    if unsafe { PrfQueryProgramTitles(HINI_USERPROFILE, group, buf.as_mut_ptr(), buf.len() as u32, &mut count) } == 0 {
        return Vec::new();
    }
    // PROGTITLE: hprog, progc, fbVisible, pszTitle.
    let rd = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    (0..(count as usize).min(buf.len() / 16)).map(|k| (rd(k * 16), unsafe { cstr(rd(k * 16 + 12) as *const u8) }.to_vec())).collect()
}

/// The desktop folder (program group) titled `title`, made if missing.
fn group(title: &[u8]) -> u32 {
    if let Some((h, _)) = titles(SGH_ROOT).into_iter().find(|(_, t)| t.eq_ignore_ascii_case(title)) {
        return h;
    }
    let t = z(title);
    unsafe { PrfCreateGroup(HINI_USERPROFILE, t.as_ptr(), SHE_VISIBLE) }
}

/// Make (or update) a program object in `group`.
fn object(group: u32, o: &catalog::Object, d: u8) {
    let strs = [
        z(o.title),
        z(&media::on(d, o.program)),
        z(o.params),
        if o.dir.is_empty() { z(b"") } else { z(&media::on(d, o.dir)) },
    ];
    // PROGDETAILS: Length, progc, fbVisible, title, executable,
    // parameters, startup directory, icon, environment, SWP.
    let mut pd = [0u32; 18];
    pd[0] = 72;
    pd[1] = PROG_PM;
    pd[2] = SHE_VISIBLE;
    for (k, s) in strs.iter().enumerate() {
        pd[3 + k] = if s.len() > 1 || k == 0 { s.as_ptr() as u32 } else { 0 };
    }
    match titles(group).into_iter().find(|(_, t)| t.as_slice() == o.title) {
        Some((h, _)) => unsafe {
            PrfChangeProgram(HINI_USERPROFILE, h, pd.as_ptr() as *const u8);
        },
        None => unsafe {
            PrfAddProgram(HINI_USERPROFILE, pd.as_ptr() as *const u8, group);
        },
    }
}

fn finished(a: &mut App) {
    let j = job();
    let mut made = 0;
    for (c, cat) in a.cats.iter().enumerate() {
        let Some(folder) = cat.folder else { continue };
        let chosen: Vec<usize> = (0..cat.apps.len()).filter(|&i| a.tree.checked[c][i] && !cat.apps[i].objects.is_empty()).collect();
        if chosen.is_empty() {
            continue;
        }
        let g = group(folder);
        for i in chosen {
            for o in &cat.apps[i].objects {
                object(g, o, a.target);
                made += 1;
            }
        }
    }
    if made > 0 {
        // rs2's desktop shell refreshes its folders on this message.
        let atom = unsafe { WinAddAtom(WinQuerySystemAtomTable(), b"RS2_ProgramListChanged\0".as_ptr()) };
        unsafe { WinBroadcastMsg(HWND_DESKTOP, atom, 0, 0, BMSG_POST | BMSG_FRAMEONLY) };
    }
    a.busy = false;
    let n = a.tree.checked.iter().flatten().filter(|&&b| b).count();
    for row in a.tree.checked.iter_mut() {
        row.iter_mut().for_each(|b| *b = false);
    }
    refresh(a);
    let mut m: Vec<u8> = Vec::new();
    if j.errors.is_empty() {
        m.extend_from_slice(&tree::dec(n as u32));
        m.extend_from_slice(if n == 1 { b" application was installed." } else { b" applications were installed." });
        set_text(ID_STATUS, b"Installed.");
    } else {
        m.extend_from_slice(b"Some files could not be installed:\n");
        for e in j.errors.iter().take(6) {
            m.extend_from_slice(e);
            m.push(b'\n');
        }
        if j.errors.len() > 6 {
            m.extend_from_slice(b"...\n");
        }
        set_text(ID_STATUS, b"Installed with errors.");
    }
    if made > 0 {
        m.extend_from_slice(b"\n\nTheir objects are in folders on the desktop.");
    }
    if j.config_changed {
        m.extend_from_slice(b"\n\nCONFIG.SYS now has \\OS2\\APPS on its PATH and LIBPATH: restart rs2 before using Solitaire and Mahjongg.");
    }
    a.progress = (0, 0);
    unsafe { WinInvalidateRect(a.client, core::ptr::null(), 0) };
    message(&m, if j.errors.is_empty() { 0 } else { MB_ERROR });
}

fn message(text: &[u8], style: u32) {
    let t = z(text);
    unsafe { WinMessageBox(HWND_DESKTOP, app().frame, t.as_ptr(), b"Install OS/2 Applications\0".as_ptr(), 0, MB_OK | MB_MOVEABLE | style) };
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    let a = app();
    match msg {
        WM_CREATE => {
            create(a, h);
            0
        }
        WM_SIZE => {
            layout(a);
            0
        }
        WM_PAINT => {
            paint(a, h);
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_INSTALL => start(a),
                ID_REFRESH => refresh(a),
                ID_CLOSE => {
                    post(h, WM_CLOSE, 0, 0);
                }
                _ => {}
            }
            0
        }
        WM_CONTROL => {
            if lo(mp1) == ID_TREE {
                update(a);
            }
            0
        }
        WM_PROGRESS => {
            a.progress = (mp1, mp2);
            let mut s = b"Installing ".to_vec();
            s.extend_from_slice(&job().current);
            set_text(ID_STATUS, &s);
            let b = bar(a);
            unsafe { WinInvalidateRect(h, &b, 0) };
            0
        }
        WM_DONE => {
            finished(a);
            0
        }
        WM_CLOSE => {
            if a.busy {
                return 0;
            }
            post(h, WM_QUIT, 0, 0);
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}
