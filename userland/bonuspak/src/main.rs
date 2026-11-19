//! BONUSPAK.EXE: installs the parts of the OS/2 Warp 4 BonusPak (IBM
//! Works, HyperACCESS Lite, FaxWorks, ...) from the Warp 4 CD.
//!
//! On OS/2 Warp 4 the Feature Installer installs them from the CD's
//! `OS2IMAGE\FI`; rs2 has no Feature Installer, so this program reads the
//! same package descriptions (`FEATURE_\OS2BONUS\*\INSTDATA.INI`, see
//! `fi`) and does what they say: copies the files with their dates,
//! attributes and extended attributes, edits CONFIG.SYS and the other text
//! files (`edit`), writes OS2.INI and OS2SYS.INI, registers the Workplace
//! Shell classes, makes the objects and runs the packages' own programs
//! (IBM Works' FPWINI).
//!
//! The window shows the parts in groups with check boxes (`catalog`: rs2's
//! names and descriptions); a part that needs another (HP JetAdmin's
//! common files) brings it. Install runs on a second thread with a
//! progress bar.
//!
//! `BONUSPAK [X: | path]` takes the CD from drive X: (or a copy of its
//! `OS2IMAGE\FI` directory) instead of looking for it.

#![no_std]
#![no_main]

extern crate alloc;

mod catalog;
mod edit;
mod fi;
#[path = "../../appinst/src/media.rs"]
#[allow(dead_code)]
mod media;
#[path = "../../appinst/src/pm.rs"]
mod pm;
#[path = "../../appinst/src/tree.rs"]
mod tree;

use alloc::vec::Vec;
use fi::Package;
use pm::*;
use tree::{Row, Tree};

os2::entry!(main);

const CLASS: &[u8] = b"BonusPak\0";
const TITLE: &[u8] = b"Install BonusPak\0";
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
const MB_OKCANCEL: u32 = 0x0001;
const MB_WARNING: u32 = 0x0020;
const MBID_OK: u32 = 1;
const HINI_SYSTEMPROFILE: u32 = 0xffff_fffe;
const CO_UPDATEIFEXISTS: u32 = 2;

unsafe extern "C" {
    #[link_name = "PMWP.252"]
    fn WinQueryObject(id: *const u8) -> u32;
    #[link_name = "PMWP.200"]
    fn WinRegisterObjectClass(class: *const u8, module: *const u8) -> u32;
    #[link_name = "PMSHAPI.102"]
    fn PrfOpenProfile(hab: u32, name: *const u8) -> u32;
    #[link_name = "PMSHAPI.103"]
    fn PrfCloseProfile(hini: u32) -> u32;
    #[link_name = "PMSHAPI.116"]
    fn PrfWriteProfileString(hini: u32, app: *const u8, key: *const u8, data: *const u8) -> u32;
}

/// A part in the tree.
pub struct Part {
    pub name: &'static [u8],
    pub description: &'static [u8],
    /// Its package's place in `catalog::LOAD_ORDER`.
    pub pkg: usize,
}

pub struct Category {
    pub title: &'static [u8],
    pub apps: Vec<Part>,
}

pub struct App {
    frame: u32,
    client: u32,
    pub cats: Vec<Category>,
    pub tree: Tree,
    /// The packages read from the CD (by `catalog::LOAD_ORDER`).
    pkgs: Vec<Option<Package>>,
    /// The CD's `OS2IMAGE\FI` directory.
    source: Option<Vec<u8>>,
    source_arg: Option<Vec<u8>>,
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
    source: Vec<u8>,
    /// The packages to install, in order.
    pkgs: Vec<usize>,
    current: Vec<u8>,
    errors: Vec<Vec<u8>>,
    config_changed: bool,
    /// Objects made, and whether any had to go into a program group
    /// (no Workplace Shell).
    objects: u32,
    groups: bool,
}

static mut JOB: Option<Job> = None;

fn job() -> &'static mut Job {
    unsafe { (*core::ptr::addr_of_mut!(JOB)).as_mut().unwrap() }
}

fn main(s: &os2::Startup) -> u32 {
    // No hard-error pop-ups for empty drives while looking for the CD.
    unsafe { os2::dos::DosError(0) };
    let arg = s.args().trim_ascii();
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let mut cats = Vec::new();
    for g in catalog::GROUPS {
        let apps = g
            .parts
            .iter()
            .filter_map(|p| catalog::LOAD_ORDER.iter().position(|(id, _)| *id == p.id).map(|pkg| Part { name: p.name, description: p.description, pkg }))
            .collect();
        cats.push(Category { title: g.title, apps });
    }
    let sizes: Vec<usize> = cats.iter().map(|c: &Category| c.apps.len()).collect();
    unsafe {
        *core::ptr::addr_of_mut!(APP) = Some(App {
            frame: 0,
            client: 0,
            cats,
            tree: Tree::new(&sizes),
            pkgs: Vec::new(),
            source: None,
            source_arg: (!arg.is_empty()).then(|| arg.to_vec()),
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
    a.frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), TITLE.as_ptr(), WS_CLIPCHILDREN, 0, 0, &mut client) };
    let (sw, sh) = (sysval(SV_CXSCREEN), sysval(SV_CYSCREEN));
    let (cx, cy) = (600.min(sw), 500.min(sh));
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

fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p.extend_from_slice(name);
    p
}

const ROOT_INI: &[u8] = b"FEATURE_\\OS2BONUS\\INSTDATA.INI";

/// The CD's `OS2IMAGE\FI` directory: from the command line (a drive, the
/// FI directory or a directory holding OS2IMAGE), else the first drive
/// that has it.
fn find_source(arg: Option<&[u8]>) -> Option<Vec<u8>> {
    let candidates = |base: &[u8]| -> Vec<Vec<u8>> {
        let mut b = base.to_vec();
        if b.len() == 2 && b[1] == b':' {
            b.push(b'\\');
        }
        alloc::vec![b.clone(), join(&b, b"OS2IMAGE\\FI"), join(&b, b"FI")]
    };
    if let Some(arg) = arg {
        return candidates(arg).into_iter().find(|c| media::exists(&join(c, ROOT_INI)));
    }
    let map = media::drives();
    (2..26u8).filter(|d| map & (1 << d) != 0).map(|d| media::on(b'A' + d, b"\\OS2IMAGE\\FI")).find(|c| media::exists(&join(c, ROOT_INI)))
}

/// Read the packages from the CD.
fn load(source: &[u8], target: u8) -> Vec<Option<Package>> {
    let drive = [target, b':'];
    let mut vars = fi::Vars::default();
    vars.set(b"BootDrive", &drive);
    vars.set(b"TargetDrive", &drive);
    vars.set(b"SourceDrive", &source[..2]);
    vars.set(b"SourcePath", &source[2..]);
    vars.set(b"MediaPath", b"FI");
    if let Some(root) = media::read_file(&join(source, ROOT_INI)).map(|t| fi::parse(&t)) {
        if let Some(b) = root.first() {
            vars.add_block(b);
        }
    }
    let mut ea_next = Vec::new();
    catalog::LOAD_ORDER
        .iter()
        .map(|(id, dir)| {
            let mut path = join(source, b"FEATURE_\\OS2BONUS\\");
            path.extend_from_slice(dir);
            path.extend_from_slice(b"\\INSTDATA.INI");
            let blocks = fi::parse(&media::read_file(&path)?);
            let b = blocks.iter().find(|b| b.kind.eq_ignore_ascii_case(id))?;
            Some(fi::package(b, &vars, &mut ea_next))
        })
        .collect()
}

/// Whether a package's files are all on the target drive; for one
/// without files of its own (JetAdmin's client, MarkVision: their files
/// are the common parts'), whether its objects are there.
fn installed(p: &Package) -> bool {
    if !p.files.is_empty() {
        return p.files.iter().all(|f| media::exists(&f.target));
    }
    let ids: Vec<&[u8]> = p.objects.iter().map(|o| setup_value(&o.setup, b"OBJECTID")).filter(|id| !id.is_empty()).collect();
    !ids.is_empty()
        && ids.iter().all(|id| {
            let i = z(id);
            unsafe { WinQueryObject(i.as_ptr()) != 0 }
        })
}

/// Look for the CD and at what is installed already.
fn refresh(a: &mut App) {
    a.source = find_source(a.source_arg.as_deref());
    a.pkgs = match &a.source {
        Some(s) => load(s, a.target),
        None => Vec::new(),
    };
    for (c, cat) in a.cats.iter().enumerate() {
        for (i, part) in cat.apps.iter().enumerate() {
            a.tree.installed[c][i] = a.pkgs.get(part.pkg).and_then(|p| p.as_ref()).is_some_and(installed);
            if a.pkgs.get(part.pkg).is_none_or(|p| p.is_none()) {
                a.tree.checked[c][i] = false;
            }
        }
    }
    let src: Vec<u8> = match &a.source {
        Some(s) => {
            let mut t = s.clone();
            t.extend_from_slice(b"  (OS/2 Warp 4 CD), installing on drive ");
            t.push(a.target);
            t.push(b':');
            t
        }
        None => match &a.source_arg {
            Some(arg) => {
                let mut t = b"No BonusPak in ".to_vec();
                t.extend_from_slice(arg);
                t.extend_from_slice(b": give the CD's drive or its OS2IMAGE\\FI directory.");
                t
            }
            None => b"No OS/2 Warp 4 CD found: put it in a drive and choose Refresh.".to_vec(),
        },
    };
    set_text(ID_SOURCE, &src);
    update(a);
}

fn find_part(a: &App, pkg: usize) -> Option<(usize, usize)> {
    a.cats.iter().enumerate().find_map(|(c, cat)| cat.apps.iter().position(|p| p.pkg == pkg).map(|i| (c, i)))
}

fn needs(a: &App, pkg: usize) -> Vec<usize> {
    let Some(Some(p)) = a.pkgs.get(pkg) else { return Vec::new() };
    p.needs.iter().filter_map(|n| catalog::LOAD_ORDER.iter().position(|(id, _)| id.eq_ignore_ascii_case(n))).collect()
}

/// Check or uncheck a row; a checked part also checks what it needs.
pub fn toggle(a: &mut App, r: Row) {
    if a.busy {
        return;
    }
    tree::set(&mut a.tree, r);
    // Parts the CD does not have stay unchecked.
    for (c, cat) in a.cats.iter().enumerate() {
        for (i, part) in cat.apps.iter().enumerate() {
            if a.pkgs.get(part.pkg).is_none_or(|p| p.is_none()) {
                a.tree.checked[c][i] = false;
            }
        }
    }
    for _ in 0..4 {
        let mut want = Vec::new();
        for (c, cat) in a.cats.iter().enumerate() {
            for (i, part) in cat.apps.iter().enumerate() {
                if a.tree.checked[c][i] {
                    want.extend(needs(a, part.pkg).into_iter().filter_map(|n| find_part(a, n)));
                }
            }
        }
        for (c, i) in want {
            a.tree.checked[c][i] = true;
        }
    }
}

fn dec(n: u64) -> Vec<u8> {
    tree::dec(n.min(u32::MAX as u64) as u32)
}

/// A size as "12.3 MB" or "640 KB".
fn size_text(bytes: u64) -> Vec<u8> {
    if bytes < 1024 * 1024 {
        let mut s = dec(bytes.div_ceil(1024));
        s.extend_from_slice(b" KB");
        return s;
    }
    let tenths = bytes * 10 / (1024 * 1024);
    let mut s = dec(tenths / 10);
    s.push(b'.');
    s.push(b'0' + (tenths % 10) as u8);
    s.extend_from_slice(b" MB");
    s
}

/// Free bytes on drive `d` (DosQueryFSInfo level 1, FSALLOCATE).
fn free_space(d: u8) -> Option<u64> {
    let mut buf = [0u8; 18];
    let rc = unsafe { os2::dos::DosQueryFSInfo((d - b'A' + 1) as u32, 1, buf.as_mut_ptr(), 18) };
    if rc != 0 {
        return None;
    }
    let rd = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]) as u64;
    let sector = u16::from_le_bytes([buf[16], buf[17]]) as u64;
    Some(rd(4) * rd(12) * sector)
}

/// The packages the checked parts install, in load order.
fn chosen(a: &App) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    for (c, cat) in a.cats.iter().enumerate() {
        for (i, part) in cat.apps.iter().enumerate() {
            if a.tree.checked[c][i] && a.pkgs.get(part.pkg).is_some_and(|p| p.is_some()) {
                v.push(part.pkg);
            }
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// The description, the status line and the Install button.
fn update(a: &mut App) {
    let desc: Vec<u8> = match a.tree.focused() {
        Some(Row::Cat(c)) => {
            let cat = &a.cats[c];
            let mut s = cat.title.to_vec();
            s.extend_from_slice(b": ");
            s.extend_from_slice(&dec(cat.apps.len() as u64));
            s.extend_from_slice(if cat.apps.len() == 1 { b" part" } else { b" parts" });
            s.extend_from_slice(b". Check the box to install them all, or open the group to choose.");
            s
        }
        Some(Row::App(c, i)) => {
            let part = &a.cats[c].apps[i];
            let mut s = part.description.to_vec();
            match a.pkgs.get(part.pkg).and_then(|p| p.as_ref()) {
                Some(p) if p.files.is_empty() => {
                    s.extend_from_slice(b"\nNo files of its own: its objects and settings.");
                    for n in needs(a, part.pkg) {
                        if let Some((nc, ni)) = find_part(a, n) {
                            s.extend_from_slice(b" Installed with ");
                            s.extend_from_slice(a.cats[nc].apps[ni].name);
                            s.push(b'.');
                        }
                    }
                }
                Some(p) => {
                    s.extend_from_slice(b"\n");
                    s.extend_from_slice(&size_text(p.size()));
                    s.extend_from_slice(b" in ");
                    s.extend_from_slice(&dec(p.files.len() as u64));
                    s.extend_from_slice(if p.files.len() == 1 { b" file" } else { b" files" });
                    if let Some(f) = p.files.first() {
                        if let Some(end) = f.target.iter().rposition(|&c| c == b'\\') {
                            s.extend_from_slice(b", in ");
                            s.extend_from_slice(&f.target[..end]);
                        }
                    }
                    s.push(b'.');
                    for n in needs(a, part.pkg) {
                        if let Some((nc, ni)) = find_part(a, n) {
                            s.extend_from_slice(b" Installed with ");
                            s.extend_from_slice(a.cats[nc].apps[ni].name);
                            s.push(b'.');
                        }
                    }
                }
                None if a.source.is_some() => s.extend_from_slice(b"\nThis CD does not have it."),
                None => {}
            }
            s
        }
        None => Vec::new(),
    };
    set_text(ID_DESC, &desc);
    let pkgs = chosen(a);
    if !a.busy {
        let mut s = dec(pkgs.len() as u64);
        s.extend_from_slice(if pkgs.len() == 1 { b" part selected" } else { b" parts selected" });
        if !pkgs.is_empty() {
            let total: u64 = pkgs.iter().filter_map(|&i| a.pkgs[i].as_ref()).map(|p| p.size()).sum();
            s.extend_from_slice(b", ");
            s.extend_from_slice(&size_text(total));
        }
        if let Some(free) = free_space(a.target) {
            s.extend_from_slice(b"; ");
            s.extend_from_slice(&size_text(free));
            s.extend_from_slice(b" free on ");
            s.push(a.target);
            s.push(b':');
        }
        s.push(b'.');
        set_text(ID_STATUS, &s);
    }
    unsafe {
        WinEnableWindow(item(ID_INSTALL), (!pkgs.is_empty() && a.source.is_some() && !a.busy) as u32);
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
    pos(ID_DESC, 8, 62, w - 16, 88);
    pos(ID_TREE, 8, 156, w - 16, h - 40 - 156);
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
    let Some(source) = a.source.clone() else { return };
    if !media::writable(a.target) {
        let mut m = b"Drive ".to_vec();
        m.push(a.target);
        m.extend_from_slice(b": cannot be written: rs2 is running from a read-only medium. Install rs2 on a disk (or run it from a USB stick) to add the BonusPak.");
        message(&m, MB_ERROR);
        return;
    }
    let pkgs = chosen(a);
    let total: u64 = pkgs.iter().filter_map(|&i| a.pkgs[i].as_ref()).map(|p| p.size()).sum();
    if free_space(a.target).is_some_and(|f| f < total) {
        let mut m = b"The parts chosen need ".to_vec();
        m.extend_from_slice(&size_text(total));
        m.extend_from_slice(b", more than drive ");
        m.push(a.target);
        m.extend_from_slice(b": has free. Install anyway?");
        if confirm(&m) != MBID_OK {
            return;
        }
    }
    // What changes at startup: the CONFIG.SYS lines that start programs
    // or load drivers.
    let mut boot: Vec<Vec<u8>> = Vec::new();
    for &i in &pkgs {
        for e in &a.pkgs[i].as_ref().unwrap().edits {
            let upper: Vec<u8> = e.new_line.iter().map(|c| c.to_ascii_uppercase()).collect();
            if e.file.to_ascii_uppercase().ends_with(b"\\CONFIG.SYS") && ["DEVICE=", "CALL=", "RUN=", "BASEDEV="].iter().any(|k| upper.starts_with(k.as_bytes())) {
                boot.push(e.new_line.clone());
            }
        }
    }
    if !boot.is_empty() {
        let mut m = b"These lines will be added to CONFIG.SYS, which rs2 reads at startup:\n\n".to_vec();
        for l in &boot {
            m.extend_from_slice(l);
            m.push(b'\n');
        }
        m.extend_from_slice(b"\nrs2 does not load IBM's device drivers (it starts without them). Install anyway?");
        if confirm(&m) != MBID_OK {
            return;
        }
    }
    unsafe {
        *core::ptr::addr_of_mut!(JOB) = Some(Job {
            client: a.client,
            source,
            pkgs,
            current: Vec::new(),
            errors: Vec::new(),
            config_changed: false,
            objects: 0,
            groups: false,
        });
    }
    a.busy = true;
    a.progress = (0, 1);
    update(a);
    let mut tid = 0u32;
    unsafe { os2::dos::DosCreateThread(&mut tid, worker, 0, 0, 256 * 1024) };
}

fn error(j: &mut Job, what: &[u8], why: &[u8]) {
    let mut s = what.to_vec();
    s.push(b' ');
    s.extend_from_slice(why);
    j.errors.push(s);
}

fn errno(rc: u32) -> Vec<u8> {
    let mut s = b"(error ".to_vec();
    s.extend_from_slice(&tree::dec(rc));
    s.push(b')');
    s
}

/// Copy a package's file from the CD.
fn copy(source: &[u8], f: &fi::File) -> Result<(), Vec<u8>> {
    let dir = join(source, &f.media_dir);
    let data = media::read_file(&join(&dir, &f.media_name)).ok_or_else(|| b"is not on the CD".to_vec())?;
    let eas = f.ea.and_then(|n| {
        let mut name = Vec::new();
        for k in (0..8).rev() {
            name.push(b"0123456789ABCDEF"[((n >> (k * 4)) & 0xf) as usize]);
        }
        name.extend_from_slice(b".EAS");
        media::read_file(&join(&dir, &name)).and_then(|old| fi::fea2list(&old))
    });
    let u = media::Unpacked { data, date: f.date, time: f.time, attributes: f.attributes, eas };
    media::write(&f.target, &u).map_err(|rc| {
        let mut s = b"cannot be written ".to_vec();
        s.extend_from_slice(&errno(rc));
        s
    })
}

/// Run a package's program and wait for it.
fn run(program: &[u8], args: &[u8]) -> Result<(), Vec<u8>> {
    if !media::exists(program) {
        return Err(b"is not there".to_vec());
    }
    let mut a = program.to_vec();
    a.push(0);
    a.extend_from_slice(args);
    a.extend_from_slice(&[0, 0]);
    let name = z(program);
    let mut fail = [0u8; 64];
    let mut res = os2::dos::ResultCodes { code_terminate: 0, code_result: 0 };
    // EXEC_SYNC.
    let rc = unsafe { os2::dos::DosExecPgm(fail.as_mut_ptr(), 64, 0, a.as_ptr(), core::ptr::null(), &mut res, name.as_ptr()) };
    if rc != 0 {
        let mut s = b"could not be started ".to_vec();
        s.extend_from_slice(&errno(rc));
        return Err(s);
    }
    Ok(())
}

/// Apply a package's text file edits to one file.
fn edit_file(j: &mut Job, file: &[u8], edits: &[&fi::Edit]) {
    // A file that is not there (no Win-OS/2: no WIN.INI) is left alone.
    let Some(text) = media::read_file(file) else { return };
    let mut lines = edit::split(&text);
    let mut changed = false;
    for e in edits {
        changed |= edit::apply(e, &mut lines);
    }
    if !changed {
        return;
    }
    let u = media::Unpacked { data: edit::join(&lines), date: 0, time: 0, attributes: 0x20, eas: None };
    match media::write(file, &u) {
        Ok(()) => j.config_changed |= file.to_ascii_uppercase().ends_with(b"\\CONFIG.SYS"),
        Err(rc) => error(j, file, &[&b"could not be updated "[..], &errno(rc)].concat()),
    }
}

fn prf(p: &fi::Prf) -> bool {
    let upper = p.file.to_ascii_uppercase();
    let (hini, opened) = if upper.ends_with(b"\\OS2\\OS2.INI") {
        (HINI_USERPROFILE, false)
    } else if upper.ends_with(b"\\OS2\\OS2SYS.INI") {
        (HINI_SYSTEMPROFILE, false)
    } else {
        let f = z(&p.file);
        (unsafe { PrfOpenProfile(0, f.as_ptr()) }, true)
    };
    if hini == 0 {
        return false;
    }
    let (app, key, value) = (z(&p.app), z(&p.key), z(&p.value));
    let ok = unsafe { PrfWriteProfileString(hini, app.as_ptr(), key.as_ptr(), value.as_ptr()) } != 0;
    if opened {
        unsafe { PrfCloseProfile(hini) };
    }
    ok
}

fn create_object(class: &[u8], title: &[u8], setup: &[u8], location: &[u8]) -> u32 {
    let (c, t, s, l) = (z(class), z(title), z(setup), z(location));
    unsafe { WinCreateObject(c.as_ptr(), t.as_ptr(), s.as_ptr(), l.as_ptr(), CO_UPDATEIFEXISTS) }
}

/// A setup string's value for `key` (`EXENAME=...;`).
fn setup_value<'a>(setup: &'a [u8], key: &[u8]) -> &'a [u8] {
    for part in setup.split(|&c| c == b';') {
        if let Some(eq) = part.iter().position(|&c| c == b'=') {
            if part[..eq].trim_ascii().eq_ignore_ascii_case(key) {
                return &part[eq + 1..];
            }
        }
    }
    b""
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

/// Without a Workplace Shell (RS2SHELL), a program object becomes a
/// program in the desktop's "BonusPak" group.
fn group_program(o: &fi::Object) -> bool {
    let exe = setup_value(&o.setup, b"EXENAME");
    if exe.is_empty() {
        return false;
    }
    let group = match titles(SGH_ROOT).into_iter().find(|(_, t)| t.eq_ignore_ascii_case(b"BonusPak")) {
        Some((h, _)) => h,
        None => unsafe { PrfCreateGroup(HINI_USERPROFILE, b"BonusPak\0".as_ptr(), SHE_VISIBLE) },
    };
    let title: Vec<u8> = o.title.iter().map(|&c| if c == b'\n' { b' ' } else { c }).collect();
    let strs = [z(&title), z(exe), z(setup_value(&o.setup, b"PARAMETERS")), z(setup_value(&o.setup, b"STARTUPDIR"))];
    // PROGDETAILS: Length, progc, fbVisible, title, executable,
    // parameters, startup directory, icon, environment, SWP.
    let mut pd = [0u32; 18];
    pd[0] = 72;
    pd[1] = PROG_PM;
    pd[2] = SHE_VISIBLE;
    for (k, s) in strs.iter().enumerate() {
        pd[3 + k] = if s.len() > 1 || k == 0 { s.as_ptr() as u32 } else { 0 };
    }
    match titles(group).into_iter().find(|(_, t)| *t == title) {
        Some((h, _)) => unsafe { PrfChangeProgram(HINI_USERPROFILE, h, pd.as_ptr() as *const u8) != 0 },
        None => unsafe { PrfAddProgram(HINI_USERPROFILE, pd.as_ptr() as *const u8, group) != 0 },
    }
}

/// Make an object where the package puts it, else in Programs or on the
/// desktop (IBM's folders rs2's desktop lacks: <WP_APPSFOLDER>).
fn object(j: &mut Job, o: &fi::Object) {
    for location in [&o.location[..], b"<WP_PROGRAMSFOLDER>", b"<WP_DESKTOP>"] {
        if !location.is_empty() && create_object(&o.class, &o.title, &o.setup, location) != 0 {
            j.objects += 1;
            return;
        }
    }
    if o.class.eq_ignore_ascii_case(b"WPProgram") && group_program(o) {
        j.objects += 1;
        j.groups = true;
        return;
    }
    let title: Vec<u8> = o.title.iter().map(|&c| if c == b'\n' { b' ' } else { c }).collect();
    error(j, &[&b"The object "[..], &title, b" ("].concat(), &[&o.class[..], b") could not be made"].concat());
}

unsafe extern "C" fn worker(_: u32) {
    let j = job();
    let a = app();
    // The Workplace Shell calls want an anchor block on this thread.
    let hab = unsafe { WinInitialize(0) };
    let pkgs: Vec<&Package> = j.pkgs.iter().filter_map(|&i| a.pkgs[i].as_ref()).collect();
    let steps = |p: &Package| p.files.len() + p.edits.len() + p.prfs.len() + p.classes.len() + p.objects.len() + p.exits.len();
    let total = pkgs.iter().map(|p| steps(p)).sum::<usize>().max(1) as u32;
    let mut done = 0u32;
    let tick = |j: &mut Job, done: &mut u32, what: &[u8]| {
        j.current = what.to_vec();
        post(j.client, WM_PROGRESS, *done, total);
        *done += 1;
    };
    // Files first, from all the packages.
    for p in &pkgs {
        for f in &p.files {
            tick(j, &mut done, &f.target);
            if let Err(e) = copy(&j.source, f) {
                error(j, &f.target, &e);
            }
        }
    }
    for p in &pkgs {
        for x in p.exits.iter().filter(|x| !x.at_end) {
            tick(j, &mut done, &x.program);
            if let Err(e) = run(&x.program, &x.args) {
                error(j, &x.program, &e);
            }
        }
    }
    // Text files, one file at a time with each package's edits in order.
    for p in &pkgs {
        let mut files: Vec<&[u8]> = Vec::new();
        for e in &p.edits {
            if !files.iter().any(|f| f.eq_ignore_ascii_case(&e.file)) {
                files.push(&e.file);
            }
        }
        for f in files {
            let edits: Vec<&fi::Edit> = p.edits.iter().filter(|e| e.file.eq_ignore_ascii_case(f)).collect();
            for _ in &edits {
                tick(j, &mut done, f);
            }
            edit_file(j, f, &edits);
        }
    }
    for p in &pkgs {
        for r in &p.prfs {
            tick(j, &mut done, &r.file);
            if !prf(r) {
                error(j, &r.file, &[&b"could not be written ("[..], &r.app, b", ", &r.key, b")"].concat());
            }
        }
    }
    for p in &pkgs {
        for (class, dll) in &p.classes {
            tick(j, &mut done, class);
            let (c, d) = (z(class), z(dll));
            if unsafe { WinRegisterObjectClass(c.as_ptr(), d.as_ptr()) } == 0 {
                error(j, &[&b"The class "[..], class].concat(), &[&b"("[..], dll, b") could not be registered"].concat());
            }
        }
    }
    for p in &pkgs {
        for o in &p.objects {
            tick(j, &mut done, &o.title);
            object(j, o);
        }
    }
    for p in &pkgs {
        for x in p.exits.iter().filter(|x| x.at_end) {
            tick(j, &mut done, &x.program);
            if let Err(e) = run(&x.program, &x.args) {
                error(j, &x.program, &e);
            }
        }
    }
    unsafe { WinTerminate(hab) };
    post(j.client, WM_DONE, total, 0);
}

fn finished(a: &mut App) {
    let j = job();
    if j.groups {
        // rs2's desktop shell refreshes its folders on this message.
        let atom = unsafe { WinAddAtom(WinQuerySystemAtomTable(), b"RS2_ProgramListChanged\0".as_ptr()) };
        unsafe { WinBroadcastMsg(HWND_DESKTOP, atom, 0, 0, BMSG_POST | BMSG_FRAMEONLY) };
    }
    a.busy = false;
    let names: Vec<&[u8]> = j.pkgs.iter().filter_map(|&p| find_part(a, p)).map(|(c, i)| a.cats[c].apps[i].name).collect();
    for row in a.tree.checked.iter_mut() {
        row.iter_mut().for_each(|b| *b = false);
    }
    refresh(a);
    let mut m: Vec<u8> = Vec::new();
    if j.errors.is_empty() {
        m.extend_from_slice(b"Installed: ");
        set_text(ID_STATUS, b"Installed.");
    } else {
        m.extend_from_slice(b"Installed with errors: ");
        set_text(ID_STATUS, b"Installed with errors.");
    }
    for (k, n) in names.iter().enumerate() {
        if k > 0 {
            m.extend_from_slice(if k + 1 == names.len() { b" and " } else { b", " });
        }
        m.extend_from_slice(n);
    }
    m.push(b'.');
    if !j.errors.is_empty() {
        m.extend_from_slice(b"\n\n");
        for e in j.errors.iter().take(6) {
            m.extend_from_slice(e);
            m.push(b'\n');
        }
        if j.errors.len() > 6 {
            m.extend_from_slice(b"...\n");
        }
    }
    if j.groups {
        m.extend_from_slice(b"\n\nWithout the Workplace Shell their programs are in the BonusPak folder on the desktop.");
    }
    if j.config_changed {
        m.extend_from_slice(b"\n\nCONFIG.SYS has changed: restart rs2 before using them.");
    }
    a.progress = (0, 0);
    unsafe { WinInvalidateRect(a.client, core::ptr::null(), 0) };
    message(&m, if j.errors.is_empty() { 0 } else { MB_ERROR });
}

fn message(text: &[u8], style: u32) {
    let t = z(text);
    unsafe { WinMessageBox(HWND_DESKTOP, app().frame, t.as_ptr(), TITLE.as_ptr(), 0, MB_OK | MB_MOVEABLE | style) };
}

fn confirm(text: &[u8]) -> u32 {
    let t = z(text);
    unsafe { WinMessageBox(HWND_DESKTOP, app().frame, t.as_ptr(), TITLE.as_ptr(), 0, MB_OKCANCEL | MB_MOVEABLE | MB_WARNING) }
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
            s.extend(job().current.iter().map(|&c| if c == b'\n' { b' ' } else { c }));
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
