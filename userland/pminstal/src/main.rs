//! PMINSTAL.EXE: rs2's installer under Presentation Manager. The work is
//! INSTALL.EXE's (`engine`, shared source): the window chooses the disk,
//! whether to update the rs2 on it or erase it, and for a new installation
//! the layout (one FAT32 partition, or a FAT32 boot partition with the
//! system on HPFS). The installation runs on a thread of its own and
//! reports here.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../install/src/engine/mod.rs"]
mod engine;
#[path = "../../appinst/src/pm.rs"]
mod pm;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use engine::{DiskInfo, Fs};
use pm::*;

os2::entry!(main);

const WC_BUTTON: &[u8] = b"#3\0";
const WC_STATIC: &[u8] = b"#5\0";
const WC_LISTBOX: &[u8] = b"#7\0";
const BS_AUTORADIOBUTTON: u32 = 4;
const BM_QUERYCHECK: u32 = 0x0124;
const BM_SETCHECK: u32 = 0x0125;
const LN_SELECT: u32 = 1;
const MB_YESNO: u32 = 0x0004;
const MB_QUERY: u32 = 0x0010;
const MB_WARNING: u32 = 0x0020;
const MB_INFORMATION: u32 = 0x0030;
const MBID_YES: u32 = 6;

const ID_VERSION: u32 = 10;
const ID_DISKS_LABEL: u32 = 11;
const ID_DISKS: u32 = 12;
const ID_UPDATE: u32 = 13;
const ID_ERASE: u32 = 14;
const ID_LAYOUT_LABEL: u32 = 15;
const ID_FAT: u32 = 16;
const ID_HPFS: u32 = 17;
const ID_JFS: u32 = 18;
const ID_STATUS: u32 = 19;
const ID_INSTALL: u32 = 20;
const ID_CLOSE: u32 = 21;

/// From the installing thread: mp1 a Box<String> (a step), or 0 for a
/// file (mp2 the count).
const WM_PROGRESS: u32 = WM_USER + 1;
/// The installation ended: mp1 a Box<Result<Vec<String>, String>>.
const WM_DONE: u32 = WM_USER + 2;

static CLASS: &[u8] = b"Rs2Install\0";

struct App {
    client: u32,
    disks: Vec<DiskInfo>,
    busy: bool,
    step: String,
}

static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

fn z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
    v.push(0);
    v
}

fn item(id: u32) -> u32 {
    unsafe { WinWindowFromID(app().client, id) }
}

fn set_text(id: u32, s: &str) {
    let t = z(s);
    unsafe { WinSetWindowText(item(id), t.as_ptr()) };
}

fn checked(id: u32) -> bool {
    unsafe { WinSendMsg(item(id), BM_QUERYCHECK, 0, 0) != 0 }
}

fn check(id: u32, on: bool) {
    unsafe { WinSendMsg(item(id), BM_SETCHECK, on as u32, 0) };
}

fn enable(id: u32, on: bool) {
    unsafe { WinEnableWindow(item(id), on as u32) };
}

fn layout_text(fs: Fs) -> &'static str {
    match fs {
        Fs::Fat => "FAT32",
        Fs::Hpfs => "FAT32 boot partition, HPFS system",
        Fs::Jfs => "FAT32 boot partition, JFS system",
    }
}

fn main(_s: &os2::Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    unsafe {
        *core::ptr::addr_of_mut!(APP) = Some(App { client: 0, disks: engine::disks(), busy: false, step: String::new() });
        WinRegisterClass(hab, CLASS.as_ptr(), client_proc, WS_CLIPCHILDREN, 0);
    }
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON | FCF_TASKLIST | 0x0200; // FCF_DLGBORDER
    let mut client = 0u32;
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), b"Install rs2\0".as_ptr(), WS_CLIPCHILDREN, 0, 0, &mut client) };
    app().client = client;
    build(client);
    let (cx, cy) = (560, 420);
    let mut r = RectL { l: 0, b: 0, r: cx, t: cy };
    unsafe { WinCalcFrameRect(frame, &mut r, 0) };
    let (fw, fh) = (r.r - r.l, r.t - r.b);
    let (sw, sh) = (unsafe { WinQuerySysValue(HWND_DESKTOP, SV_CXSCREEN) }, unsafe { WinQuerySysValue(HWND_DESKTOP, SV_CYSCREEN) });
    unsafe {
        WinSetWindowPos(frame, HWND_TOP, (sw - fw) / 2, (sh - fh) / 2, fw, fh, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
        WinSetFocus(HWND_DESKTOP, item(ID_DISKS));
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}

unsafe extern "C" {
    #[link_name = "PMWIN.704"]
    fn WinCalcFrameRect(h: u32, r: *mut RectL, client: u32) -> u32;
}

fn control(parent: u32, class: &[u8], text: &str, style: u32, id: u32, x: i32, y: i32, cx: i32, cy: i32) -> u32 {
    let t = z(text);
    unsafe { WinCreateWindow(parent, class.as_ptr(), t.as_ptr(), style | WS_VISIBLE, x, y, cx, cy, parent, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
}

/// The controls (client coordinates, from the bottom left).
fn build(h: u32) {
    let label = SS_TEXT | DT_LEFT | DT_VCENTER;
    control(h, WC_STATIC, &alloc::format!("This disc: {}", engine::this_version()), label, ID_VERSION, 12, 390, 536, 20);
    control(h, WC_STATIC, "Install on:", label, ID_DISKS_LABEL, 12, 364, 536, 20);
    control(h, WC_LISTBOX, "", WS_TABSTOP, ID_DISKS, 12, 270, 536, 92);
    control(h, WC_BUTTON, "~Update the rs2 on the disk (settings, desktop and files are kept)", BS_AUTORADIOBUTTON | WS_GROUP | WS_TABSTOP, ID_UPDATE, 12, 236, 536, 24);
    control(h, WC_BUTTON, "~Erase the disk and install anew", BS_AUTORADIOBUTTON, ID_ERASE, 12, 210, 536, 24);
    control(h, WC_STATIC, "New installation:", label, ID_LAYOUT_LABEL, 12, 178, 536, 20);
    control(h, WC_BUTTON, "One ~FAT32 partition for everything", BS_AUTORADIOBUTTON | WS_GROUP | WS_TABSTOP, ID_FAT, 28, 152, 520, 24);
    control(h, WC_BUTTON, "FAT32 boot partition, system on ~HPFS (at most 64 GB)", BS_AUTORADIOBUTTON, ID_HPFS, 28, 126, 520, 24);
    control(h, WC_BUTTON, "FAT32 boot partition, system on ~JFS", BS_AUTORADIOBUTTON, ID_JFS, 28, 100, 520, 24);
    control(h, WC_STATIC, "", SS_TEXT | DT_LEFT | DT_WORDBREAK, ID_STATUS, 12, 48, 536, 44);
    control(h, WC_BUTTON, "~Install", BS_PUSHBUTTON | WS_TABSTOP | WS_GROUP, ID_INSTALL, 344, 10, 98, 30);
    control(h, WC_BUTTON, "Close", BS_PUSHBUTTON | WS_TABSTOP, ID_CLOSE, 450, 10, 98, 30);
    for d in &app().disks {
        let what = match &d.found {
            Some(f) => alloc::format!("  -  {} installed ({})", f.version, layout_text(f.layout())),
            None => String::new(),
        };
        let t = z(&alloc::format!("Disk {}: {} MB{what}", d.number, d.disk.mb()));
        unsafe { WinSendMsg(item(ID_DISKS), LM_INSERTITEM, LIT_END as u32, t.as_ptr() as u32) };
    }
    check(ID_FAT, true);
    if app().disks.is_empty() {
        set_text(ID_STATUS, "No hard disks were found.");
    } else {
        unsafe { WinSendMsg(item(ID_DISKS), LM_SELECTITEM, 0, 1) };
    }
    update_controls();
}

fn selected() -> Option<usize> {
    let i = unsafe { WinSendMsg(item(ID_DISKS), LM_QUERYSELECTION, LIT_FIRST as u32, 0) } as i32;
    (i >= 0 && (i as usize) < app().disks.len()).then_some(i as usize)
}

/// Enable what applies to the selected disk and the chosen action.
fn update_controls() {
    let a = app();
    let disk = selected().map(|i| &a.disks[i]);
    let installed = disk.is_some_and(|d| d.found.is_some());
    enable(ID_UPDATE, installed && !a.busy);
    if !installed && checked(ID_UPDATE) || !checked(ID_UPDATE) && !checked(ID_ERASE) {
        check(ID_UPDATE, installed);
        check(ID_ERASE, !installed);
    }
    let erase = checked(ID_ERASE);
    let available = engine::layouts();
    for (id, fs) in [(ID_FAT, Fs::Fat), (ID_HPFS, Fs::Hpfs), (ID_JFS, Fs::Jfs)] {
        enable(id, erase && !a.busy && available.contains(&fs));
    }
    enable(ID_ERASE, !a.busy && disk.is_some());
    enable(ID_DISKS, !a.busy);
    enable(ID_INSTALL, !a.busy && disk.is_some());
    enable(ID_CLOSE, !a.busy);
    if !a.busy {
        let text = match disk {
            Some(d) if checked(ID_UPDATE) => alloc::format!("The system files on disk {} will be replaced with this disc's.", d.number),
            Some(d) if d.disk.mb() < engine::MIN_MB => alloc::format!("Disk {} is too small ({} MB or more is needed).", d.number, engine::MIN_MB),
            Some(d) => alloc::format!("Everything on disk {} will be erased.", d.number),
            None => String::from("Choose the disk to install on."),
        };
        set_text(ID_STATUS, &text);
    }
}

fn chosen_fs() -> Fs {
    if checked(ID_HPFS) {
        Fs::Hpfs
    } else if checked(ID_JFS) {
        Fs::Jfs
    } else {
        Fs::Fat
    }
}

/// What the installing thread works on.
struct Job {
    client: u32,
    disk: usize,
    update: bool,
    fs: Fs,
}

fn start() {
    let Some(i) = selected() else { return };
    let update = checked(ID_UPDATE);
    let fs = chosen_fs();
    let n = app().disks[i].number;
    let (text, style) = if update {
        (alloc::format!("Update the rs2 on disk {n}?"), MB_YESNO | MB_QUERY | MB_MOVEABLE)
    } else {
        (alloc::format!("All data on disk {n} will be erased, and rs2 installed: {}.\n\nContinue?", match fs {
            Fs::Fat => "one FAT32 partition for everything",
            Fs::Hpfs => "a FAT32 boot partition and the system on HPFS",
            Fs::Jfs => "a FAT32 boot partition and the system on JFS",
        }), MB_YESNO | MB_WARNING | MB_MOVEABLE)
    };
    let t = z(&text);
    if unsafe { WinMessageBox(HWND_DESKTOP, app().client, t.as_ptr(), b"Install rs2\0".as_ptr(), 0, style) } != MBID_YES {
        return;
    }
    app().busy = true;
    update_controls();
    let job = Box::new(Job { client: app().client, disk: i, update, fs });
    let mut tid = 0;
    unsafe { os2::DosCreateThread(&mut tid, worker, Box::into_raw(job) as u32, 0, 256 * 1024) };
}

struct Report {
    client: u32,
    notes: Vec<String>,
}

impl engine::Progress for Report {
    fn step(&mut self, what: &str) {
        let b = Box::new(String::from(what));
        unsafe { WinPostMsg(self.client, WM_PROGRESS, Box::into_raw(b) as u32, 0) };
    }
    fn file(&mut self, count: u32, _name: &str) {
        if count % 5 == 0 {
            unsafe { WinPostMsg(self.client, WM_PROGRESS, 0, count) };
        }
    }
    fn note(&mut self, what: &str) {
        self.notes.push(String::from(what));
    }
}

unsafe extern "C" fn worker(p: u32) {
    let job = unsafe { Box::from_raw(p as *mut Job) };
    let mut r = Report { client: job.client, notes: Vec::new() };
    let d = &app().disks[job.disk];
    let res = if job.update {
        match &d.found {
            Some(f) => engine::update(f, &mut r),
            None => Err(String::from("There is no rs2 to update on that disk.")),
        }
    } else {
        engine::install_fresh(&d.disk, job.fs, &mut r)
    };
    let out: Box<Result<Vec<String>, String>> = Box::new(res.map(|_| core::mem::take(&mut r.notes)));
    unsafe { WinPostMsg(job.client, WM_DONE, Box::into_raw(out) as u32, 0) };
}

fn done(res: Result<Vec<String>, String>) {
    app().busy = false;
    match res {
        Ok(notes) => {
            let mut text = String::from("rs2 is installed.");
            for n in &notes {
                text.push_str("\n");
                text.push_str(n);
            }
            text.push_str("\n\nRemove the installation disc and restart the computer now?");
            set_text(ID_STATUS, "Installation is complete.");
            let t = z(&text);
            let a = unsafe { WinMessageBox(HWND_DESKTOP, app().client, t.as_ptr(), b"Install rs2\0".as_ptr(), 0, MB_YESNO | MB_INFORMATION | MB_MOVEABLE) };
            if a == MBID_YES {
                engine::disk::restart();
            }
            // The disks changed: list them again.
            app().disks = engine::disks();
            unsafe { WinSendMsg(item(ID_DISKS), LM_DELETEALL, 0, 0) };
            let h = app().client;
            for id in [ID_VERSION, ID_DISKS_LABEL, ID_DISKS, ID_UPDATE, ID_ERASE, ID_LAYOUT_LABEL, ID_FAT, ID_HPFS, ID_JFS, ID_STATUS, ID_INSTALL, ID_CLOSE] {
                unsafe { WinDestroyWindow(WinWindowFromID(h, id)) };
            }
            build(h);
        }
        Err(e) => {
            set_text(ID_STATUS, &alloc::format!("Installation failed: {e}"));
            let t = z(&e);
            unsafe { WinMessageBox(HWND_DESKTOP, app().client, t.as_ptr(), b"Install rs2\0".as_ptr(), 0, MB_OK | MB_ERROR | MB_MOVEABLE) };
            update_controls();
        }
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            unsafe {
                WinFillRect(hps, &r, CLR_PALEGRAY);
                WinEndPaint(hps);
            }
            0
        }
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            if (id == ID_DISKS && code == LN_SELECT) || (matches!(id, ID_UPDATE | ID_ERASE) && code == BN_CLICKED) {
                update_controls();
            }
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                ID_INSTALL => start(),
                ID_CLOSE => unsafe {
                    WinPostMsg(h, WM_QUIT, 0, 0);
                },
                _ => {}
            }
            0
        }
        WM_PROGRESS => {
            if mp1 != 0 {
                let s = unsafe { Box::from_raw(mp1 as *mut String) };
                app().step = *s;
                set_text(ID_STATUS, &alloc::format!("{}...", app().step));
            } else {
                set_text(ID_STATUS, &alloc::format!("{}... {} files", app().step, mp2));
            }
            0
        }
        WM_DONE => {
            let r = unsafe { Box::from_raw(mp1 as *mut Result<Vec<String>, String>) };
            done(*r);
            0
        }
        WM_CLOSE if app().busy => 0,
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}
