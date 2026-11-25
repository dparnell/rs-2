//! SETUP.EXE of the developer disk: its installer under Presentation
//! Manager (docs/DEVDISK.md). It also makes a Developer folder on the
//! desktop.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use devinst::*;
use pmkit::*;

os2::entry!(main);

const TITLE: &[u8] = b"rs2 Developer Disk";
static CLASS: &[u8] = b"Rs2DevInstall\0";
const WC_BUTTON: u32 = 0xffff_0003;
const WC_STATIC: u32 = 0xffff_0005;
const WC_ENTRYFIELD: u32 = 0xffff_0006;
const WC_LISTBOX: u32 = 0xffff_0007;

const ID_TEXT: u32 = 10;
const ID_DRIVES: u32 = 11;
const ID_DIR: u32 = 12;
const ID_SOURCE: u32 = 13;
const ID_BAR: u32 = 14;
const ID_STATUS: u32 = 15;
const ID_INSTALL: u32 = 16;
const ID_CANCEL: u32 = 17;
const ID_DRIVES_LABEL: u32 = 18;
const ID_DIR_LABEL: u32 = 19;

/// mp1 the percent, mp2 a Box<String> (the file).
const WM_AT: u32 = WM_USER + 1;
/// mp1 a Box<Result<Vec<String>, String>>.
const WM_DONE: u32 = WM_USER + 2;

struct App {
    client: u32,
    disc: Result<Disc, String>,
    drives: Vec<Drive>,
    busy: bool,
}

static mut APP: Option<App> = None;
static STOP: AtomicBool = AtomicBool::new(false);

fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

fn control(h: u32, class: u32, text: &str, style: u32, id: u32, r: [i32; 4]) -> u32 {
    let t = z(text.as_bytes());
    unsafe { WinCreateWindow(h, class, t.as_ptr(), style | WS_VISIBLE, r[0], r[1], r[2], r[3], h, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
}

fn status(t: &str) {
    set_text(item(app().client, ID_STATUS), t.as_bytes());
}

fn source_wanted() -> bool {
    checked(app().client, ID_SOURCE)
}

fn build(h: u32) {
    let a = app();
    let label = SS_TEXT | DT_LEFT | DT_VCENTER;
    control(h, WC_STATIC, "Compilers for Rust, C and C++ that run on rs2 and make 64-bit programs.", label, ID_TEXT, [12, 298, 496, 20]);
    control(h, WC_STATIC, "Drive (HPFS or JFS):", label, ID_DRIVES_LABEL, [12, 270, 496, 20]);
    control(h, WC_LISTBOX, "", WS_TABSTOP, ID_DRIVES, [12, 186, 496, 82]);
    control(h, WC_STATIC, "Directory:", label, ID_DIR_LABEL, [12, 152, 80, 20]);
    control(h, WC_ENTRYFIELD, "", ES_MARGIN | ES_AUTOSCROLL | WS_TABSTOP, ID_DIR, [98, 154, 406, 18]);
    control(h, WC_BUTTON, "", BS_AUTOCHECKBOX | WS_TABSTOP, ID_SOURCE, [12, 118, 496, 22]);
    let bar = progress::create(h, ID_BAR);
    unsafe { WinSetWindowPos(bar, 0, 12, 82, 496, 22, SWP_MOVE | SWP_SIZE) };
    control(h, WC_STATIC, "", SS_TEXT | DT_LEFT | DT_WORDBREAK, ID_STATUS, [12, 44, 496, 34]);
    control(h, WC_BUTTON, "~Install", BS_PUSHBUTTON | WS_TABSTOP | WS_GROUP, ID_INSTALL, [304, 8, 98, 30]);
    control(h, WC_BUTTON, "Close", BS_PUSHBUTTON | WS_TABSTOP, ID_CANCEL, [410, 8, 98, 30]);
    let mut ok = false;
    match &a.disc {
        Ok(d) => {
            let more = d.needs_mb(true) - d.needs_mb(false);
            let t = format!("rs2's ~sources too, to build rs2 itself ({more} MB, and {BUILD_MB} MB for the build)");
            set_text(item(h, ID_SOURCE), if d.source.is_some() { t.as_bytes() } else { b"rs2's sources are not on this disk" });
            for x in &a.drives {
                lb_add(h, ID_DRIVES, format!("{}:  {}, {} MB free", x.letter, x.fs, x.free_mb).as_bytes());
            }
            ok = !a.drives.is_empty();
            if ok {
                let boot = devinst::boot_drive();
                let i = a.drives.iter().position(|x| x.letter == boot && x.free_mb > d.needs_mb(false) + 200);
                let i = i.unwrap_or_else(|| (0..a.drives.len()).max_by_key(|&i| a.drives[i].free_mb).unwrap_or(0));
                dlg_msg(h, ID_DRIVES, LM_SELECTITEM, i as u32, 1);
                set_text(item(h, ID_DIR), format!("{}:\\{DIR}", a.drives[i].letter).as_bytes());
                say_needs();
            } else {
                status(NO_DRIVE);
            }
        }
        Err(e) => status(e),
    }
    enable(h, ID_INSTALL, ok);
    enable(h, ID_SOURCE, ok && a.disc.as_ref().is_ok_and(|d| d.source.is_some()));
}

fn say_needs() {
    if let Ok(d) = &app().disc {
        status(&format!("{} MB are needed.", d.needs_mb(source_wanted())));
    }
}

struct Watch(u32, u64);

impl Progress for Watch {
    fn at(&mut self, file: &str, done: u64, total: u64) -> bool {
        let p = done * 1000 / total.max(1);
        if p != self.1 {
            self.1 = p;
            let b = Box::into_raw(Box::new(String::from(file)));
            post(self.0, WM_AT, (p / 10) as u32, b as u32);
        }
        !STOP.load(Ordering::Relaxed)
    }
}

struct Job {
    dir: String,
    source: bool,
}

unsafe extern "C" fn worker(p: u32) {
    let job = unsafe { Box::from_raw(p as *mut Job) };
    let a = app();
    // The Workplace Shell calls want an anchor block on this thread.
    let hab = unsafe { WinInitialize(0) };
    let r = match &a.disc {
        Ok(d) => install(d, &job.dir, job.source, &mut Watch(a.client, u64::MAX)),
        Err(e) => Err(e.clone()),
    };
    if r.is_ok() {
        objects(&job.dir, job.source);
    }
    unsafe { WinTerminate(hab) };
    post(a.client, WM_DONE, Box::into_raw(Box::new(r)) as u32, 0);
}

fn object(class: &str, title: &str, setup: &str, location: &str) {
    let [c, t, s, l] = [class, title, setup, location].map(|x| z(x.as_bytes()));
    // CO_UPDATEIFEXISTS
    unsafe { WinCreateObject(c.as_ptr(), t.as_ptr(), s.as_ptr(), l.as_ptr(), 2) };
}

fn objects(dir: &str, source: bool) {
    let f = "<RS2_DEVELOPER>";
    object("WPFolder", "Developer", "OBJECTID=<RS2_DEVELOPER>;", "<WP_DESKTOP>");
    let prompt = |title: &str, id: &str, at: &str| {
        object(
            "WPProgram",
            title,
            &format!("EXENAME={}:\\OS2\\CMD.EXE;PARAMETERS=/K {dir}\\DEVENV.CMD {at};PROGTYPE=WINDOWABLEVIO;OBJECTID=<{id}>;", devinst::boot_drive()),
            f,
        );
    };
    prompt("Developer^Command Prompt", "RS2_DEVPROMPT", "");
    if source {
        prompt("rs2's Sources", "RS2_DEVSOURCE", "src");
    }
    object("WPShadow", "Read Me", &format!("SHADOWID={dir}\\README.TXT;OBJECTID=<RS2_DEVREADME>;"), f);
    object("WPShadow", "Samples", &format!("SHADOWID={dir}\\samples;OBJECTID=<RS2_DEVSAMPLES>;"), f);
    object("WPShadow", "Documentation", &format!("SHADOWID={dir}\\docs;OBJECTID=<RS2_DEVDOCS>;"), f);
}

fn start() {
    let a = app();
    let t = window_text(item(a.client, ID_DIR));
    let dir = match check_dir(&String::from_utf8_lossy(&t)) {
        Ok(d) => d,
        Err(e) => {
            message(a.client, e.as_bytes(), TITLE, MB_OK | MB_ERROR);
            return;
        }
    };
    a.busy = true;
    STOP.store(false, Ordering::Relaxed);
    for id in [ID_INSTALL, ID_DRIVES, ID_DIR, ID_SOURCE] {
        enable(a.client, id, false);
    }
    set_text(item(a.client, ID_CANCEL), b"Stop");
    let job = Box::new(Job { dir, source: source_wanted() });
    let mut tid = 0;
    unsafe { os2::DosCreateThread(&mut tid, worker, Box::into_raw(job) as u32, 0, 256 * 1024) };
}

fn done(r: Result<Vec<String>, String>) {
    let a = app();
    a.busy = false;
    set_text(item(a.client, ID_CANCEL), b"Close");
    match r {
        Ok(notes) => {
            progress::set(item(a.client, ID_BAR), 100);
            status("The developer disk is installed.");
            message(a.client, format!("The developer disk is installed.\n\n{}", notes.join("\n")).as_bytes(), TITLE, MB_OK | MB_INFORMATION);
        }
        Err(e) => {
            progress::set(item(a.client, ID_BAR), 0);
            status(&e);
            message(a.client, e.as_bytes(), TITLE, MB_OK | MB_ERROR);
            for id in [ID_INSTALL, ID_DRIVES, ID_DIR] {
                enable(a.client, id, true);
            }
            enable(a.client, ID_SOURCE, a.disc.as_ref().is_ok_and(|d| d.source.is_some()));
        }
    }
}

extern "C" fn client_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    match msg {
        WM_PAINT => {
            let mut r = RectL::default();
            let hps = unsafe { WinBeginPaint(h, 0, &mut r) };
            unsafe {
                WinFillRect(hps, &r, SYSCLR_DIALOGBACKGROUND);
                WinEndPaint(hps);
            }
            0
        }
        WM_CONTROL => {
            let (id, code) = (lo(mp1), hi(mp1));
            if id == ID_DRIVES && code == LN_SELECT && !app().busy {
                let i = dlg_msg(h, ID_DRIVES, LM_QUERYSELECTION, LIT_FIRST, 0) as usize;
                if let Some(d) = app().drives.get(i) {
                    set_text(item(h, ID_DIR), format!("{}:\\{DIR}", d.letter).as_bytes());
                }
            } else if id == ID_SOURCE {
                say_needs();
            }
            0
        }
        WM_COMMAND => {
            match lo(mp1) {
                ID_INSTALL => start(),
                ID_CANCEL if app().busy => STOP.store(true, Ordering::Relaxed),
                ID_CANCEL => {
                    post(h, WM_QUIT, 0, 0);
                }
                _ => {}
            }
            0
        }
        WM_AT => {
            let f = unsafe { Box::from_raw(mp2 as *mut String) };
            if app().busy {
                progress::set(item(h, ID_BAR), mp1);
                status(&f);
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

fn main(s: &os2::Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    unsafe {
        *core::ptr::addr_of_mut!(APP) = Some(App { client: 0, disc: disc(s), drives: drives(), busy: false });
        WinRegisterClass(hab, CLASS.as_ptr(), client_proc, WS_CLIPCHILDREN, 0);
    }
    progress::register(hab);
    let mut fcf = FCF_TITLEBAR | FCF_SYSMENU | FCF_MINBUTTON | FCF_TASKLIST | 0x0200; // FCF_DLGBORDER
    let mut client = 0;
    let t = z(TITLE);
    let frame = unsafe { WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, CLASS.as_ptr(), t.as_ptr(), WS_CLIPCHILDREN, 0, 0, &mut client) };
    app().client = client;
    build(client);
    let mut r = RectL { l: 0, b: 0, r: 520, t: 326 };
    unsafe {
        WinCalcFrameRect(frame, &mut r, 0);
        let (w, h) = (r.r - r.l, r.t - r.b);
        let (sw, sh) = screen();
        WinSetWindowPos(frame, HWND_TOP, (sw - w) / 2, (sh - h) / 2, w, h, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
        WinSetFocus(HWND_DESKTOP, item(client, ID_INSTALL));
        let mut q = QMsg::default();
        while WinGetMsg(hab, &mut q, 0, 0, 0) != 0 {
            WinDispatchMsg(hab, &q);
        }
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}
