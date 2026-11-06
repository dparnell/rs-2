//! PKGMGR.EXE: rs2's Package Manager, a window over yum and RPM.
//!
//! The tools do the work, on pipes (`child`): `rpm -qa` for what is
//! installed, `yum list available`, `yum check-update`, `yum info`. Each yum
//! command starts Python, some ten seconds under emulation, so the lists of
//! what can be installed and updated are kept in `\VAR\CACHE\PKGMGR` and only
//! Refresh asks again.
//!
//! Before anything changes, the same yum command runs with "N" as its
//! answer: that shows what else would be installed or removed, in the lower
//! pane, and asks. Without yum (no UNIXROOT with `\USR\BIN\BASH.EXE`) the
//! window offers PKGSETUP.
//!
//! `PKGMGR /TEST:cmd name`: a button pressed without a person, for tests
//! (cmd is INSTALL or REMOVE; the question is answered Yes).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../pmchkdsk/src/child.rs"]
mod child;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use child::Child;
use pmkit::tpl::Menu;
use pmkit::*;

os2::entry!(main);

const ID_INSTALLED: u32 = 10;
const ID_AVAILABLE: u32 = 11;
const ID_UPDATES: u32 = 12;
const ID_SEARCH_LABEL: u32 = 13;
const ID_SEARCH: u32 = 14;
const ID_LIST: u32 = 15;
const ID_TEXT: u32 = 16;
const ID_STATUS: u32 = 17;
const ID_ACTION: u32 = 18;
const ID_DETAILS: u32 = 19;
const ID_UPDATE_ALL: u32 = 20;
const ID_REFRESH: u32 = 21;
const ID_SETUP: u32 = 22;

const IDM_FILE: u16 = 100;
const IDM_REFRESH: u16 = 101;
const IDM_EXIT: u16 = 102;

const WM_LINE: u32 = WM_USER + 1;
const WM_ENDED: u32 = WM_USER + 2;

/// Lines kept in the lower pane.
const KEEP: usize = 300;
const CLASS: &[u8] = b"Rs2PackageManager";

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Installed,
    Available,
    Updates,
}

#[derive(Clone)]
struct Pkg {
    name: String,
    version: String,
    text: String,
}

#[derive(Clone, PartialEq)]
enum Job {
    Installed,
    Available,
    Updates,
    Info(String),
    /// What `yum verb names` would do.
    Preview(String, String),
    Run(String, String),
    Setup,
}

struct App {
    client: u32,
    root: Option<String>,
    tab: Tab,
    installed: Vec<Pkg>,
    available: Vec<Pkg>,
    updates: Vec<Pkg>,
    /// What the list box shows, as indexes into the tab's list.
    shown: Vec<usize>,
    queue: Vec<Job>,
    job: Option<Job>,
    child: Option<Child>,
    /// The output of the running job.
    lines: Vec<String>,
    /// The test's yes.
    auto: bool,
}

static mut APP: Option<App> = None;

fn app() -> &'static mut App {
    unsafe { (*core::ptr::addr_of_mut!(APP)).as_mut().unwrap() }
}

fn ctl(id: u32) -> u32 {
    item(app().client, id)
}

fn say(t: &str) {
    set_text(ctl(ID_STATUS), t.as_bytes());
}

fn pane(t: &str) {
    set_text(ctl(ID_TEXT), t.as_bytes());
}

fn env(name: &str) -> Option<String> {
    let n = z(name.as_bytes());
    let mut p: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(n.as_ptr(), &mut p) } != 0 || p.is_null() {
        return None;
    }
    let mut v = Vec::new();
    let mut q = p;
    while unsafe { *q } != 0 {
        v.push(unsafe { *q });
        q = unsafe { q.add(1) };
    }
    String::from_utf8(v).ok()
}

/// The UNIXROOT, if yum is there.
fn find_root() -> Option<String> {
    let r = env("UNIXROOT")?;
    read_file(format!("{r}\\USR\\BIN\\YUM").as_bytes()).map(|_| r)
}

fn cache(file: &str) -> String {
    format!("{}\\VAR\\CACHE\\PKGMGR\\{file}", app().root.as_deref().unwrap_or("C:"))
}

// ---- Lists ----

/// `name.arch  version  repository` lines of yum: the architecture of this
/// system's packages only (pentium4 and noarch come before i686).
fn parse_yum(text: &[String]) -> Vec<Pkg> {
    let mut v: Vec<Pkg> = Vec::new();
    for l in text {
        let w: Vec<&str> = l.split_whitespace().collect();
        if w.len() != 3 || !w[0].contains('.') || !w[1].contains(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let (name, arch) = w[0].rsplit_once('.').unwrap_or((w[0], ""));
        if arch == "i686" || arch == "i386" || arch == "src" {
            continue;
        }
        if v.last().is_none_or(|p| p.name != name) {
            v.push(Pkg { name: name.to_string(), version: w[1].to_string(), text: w[2].to_string() });
        }
    }
    v
}

/// `name|version|size|summary` lines of rpm.
fn parse_rpm(text: &[String]) -> Vec<Pkg> {
    let mut v: Vec<Pkg> = text
        .iter()
        .filter_map(|l| {
            let mut p = l.splitn(3, '|');
            Some(Pkg { name: p.next()?.to_string(), version: p.next()?.to_string(), text: p.next()?.to_string() })
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn list() -> &'static Vec<Pkg> {
    let a = app();
    match a.tab {
        Tab::Installed => &a.installed,
        Tab::Available => &a.available,
        Tab::Updates => &a.updates,
    }
}

fn fit(s: &str, w: usize) -> String {
    let mut t: String = s.chars().take(w).collect();
    while t.len() < w {
        t.push(' ');
    }
    t
}

/// Fill the list box from the tab and the search text.
fn show_list() {
    let want = String::from_utf8_lossy(&window_text(ctl(ID_SEARCH))).to_ascii_lowercase();
    let want = want.trim().to_string();
    let lb = ctl(ID_LIST);
    send(lb, LM_DELETEALL, 0, 0);
    let installed: Vec<&str> = app().installed.iter().map(|p| p.name.as_str()).collect();
    let mut shown = Vec::new();
    unsafe { WinEnableWindowUpdate(lb, 0) };
    for (i, p) in list().iter().enumerate() {
        if !want.is_empty() && !p.name.to_ascii_lowercase().contains(&want) && !p.text.to_ascii_lowercase().contains(&want) {
            continue;
        }
        if app().tab == Tab::Available && installed.binary_search(&p.name.as_str()).is_ok() {
            continue;
        }
        let line = z(format!("{} {} {}", fit(&p.name, 30), fit(&p.version, 20), p.text).as_bytes());
        send(lb, LM_INSERTITEM, LIT_END, line.as_ptr() as u32);
        shown.push(i);
    }
    unsafe { WinEnableWindowUpdate(lb, 1) };
    app().shown = shown;
    update_controls();
}

fn selected() -> Option<&'static Pkg> {
    let i = send(ctl(ID_LIST), LM_QUERYSELECTION, LIT_FIRST, 0) as i32;
    let n = *app().shown.get(usize::try_from(i).ok()?)?;
    list().get(n)
}

fn update_controls() {
    let a = app();
    let idle = a.job.is_none() && a.queue.is_empty();
    let have = a.root.is_some();
    let c = a.client;
    let sel = selected().is_some();
    let verb = match a.tab {
        Tab::Installed => "~Remove",
        Tab::Available => "~Install",
        Tab::Updates => "~Update",
    };
    set_text(ctl(ID_ACTION), verb.as_bytes());
    enable(c, ID_ACTION, idle && have && sel);
    enable(c, ID_DETAILS, idle && have && sel);
    enable(c, ID_REFRESH, idle && have);
    enable(c, ID_UPDATE_ALL, idle && have && !a.updates.is_empty());
    enable(c, ID_SETUP, idle && !have);
    let shown = a.shown.len();
    if idle && have {
        say(&format!("{shown} packages. {} installed, {} updates.", a.installed.len(), a.updates.len()));
    }
}

// ---- Jobs ----

fn script(job: &Job) -> String {
    match job {
        Job::Installed => String::from("rpm -qa --qf '%{NAME}|%{VERSION}-%{RELEASE}|%{SUMMARY}\\n'"),
        Job::Available => String::from("yum -q list available"),
        Job::Updates => String::from("yum -q check-update; true"),
        Job::Info(n) => format!("yum -q info {n}"),
        Job::Preview(v, n) => format!("yum {v} {n}"),
        Job::Run(v, n) => format!("yum -y {v} {n}"),
        Job::Setup => String::new(),
    }
}

fn describe(job: &Job) -> String {
    match job {
        Job::Installed => String::from("Reading what is installed..."),
        Job::Available => String::from("Asking the repository what is available..."),
        Job::Updates => String::from("Looking for updates..."),
        Job::Info(n) => format!("Reading about {n}..."),
        Job::Preview(_, n) => format!("Working out what {n} needs..."),
        Job::Run(v, n) => format!("yum {v} {n}..."),
        Job::Setup => String::from("Downloading RPM and yum..."),
    }
}

/// Start the next job, if nothing is running.
fn next() {
    let a = app();
    if a.job.is_some() || a.queue.is_empty() {
        update_controls();
        return;
    }
    let job = a.queue.remove(0);
    a.lines.clear();
    say(&describe(&job));
    let (prog, args) = if job == Job::Setup {
        (format!("{}:\\OS2\\PKGSETUP.EXE", boot_drive() as char), String::new())
    } else {
        let root = a.root.clone().unwrap_or_default();
        (format!("{root}\\USR\\BIN\\BASH.EXE"), format!("-c \"{} 2>&1\"", script(&job)))
    };
    match child::start_lines(prog.as_bytes(), args.as_bytes(), a.client, WM_LINE, WM_ENDED, true) {
        Ok(mut c) => {
            // A preview's answer; for anything else, the end of the input.
            if matches!(job, Job::Preview(..)) {
                c.answer(b"N");
            }
            c.close();
            a.child = Some(c);
            a.job = Some(job);
        }
        Err(rc) => say(&format!("{prog} cannot be started (error {rc}).")),
    }
    update_controls();
}

fn add(job: Job) {
    app().queue.push(job);
    next();
}

fn save_list(file: &str, lines: &[String]) {
    let text = lines.join("\r\n");
    let dir = cache("");
    let mut at = String::new();
    for part in dir.trim_end_matches('\\').split('\\') {
        at.push_str(part);
        let d = z(at.as_bytes());
        unsafe { os2::DosCreateDir(d.as_ptr(), core::ptr::null_mut()) };
        at.push('\\');
    }
    let _ = write_file(cache(file).as_bytes(), text.as_bytes());
}

fn load_list(file: &str) -> Option<Vec<String>> {
    let d = read_file(cache(file).as_bytes())?;
    Some(String::from_utf8_lossy(&d).lines().map(|l| l.to_string()).collect())
}

fn append(line: String) {
    let a = app();
    let live = matches!(a.job, Some(Job::Run(..) | Job::Setup));
    if live && line.is_empty() {
        return;
    }
    a.lines.push(line);
    if live {
        let from = a.lines.len().saturating_sub(KEEP);
        if a.lines.len() % 4 == 0 || a.lines.len() < 8 {
            pane(&a.lines[from..].join("\r\n"));
        }
        if let Some(l) = a.lines.last() {
            say(l);
        }
    }
}

/// The lines that say how much a transaction is.
fn summary(lines: &[String]) -> String {
    let mut s = String::new();
    for l in lines {
        let t = l.trim();
        let counted = ["Install ", "Upgrade ", "Update ", "Remove ", "Reinstall "].iter().any(|k| t.starts_with(k)) && t.contains("Package");
        if counted || t.starts_with("Total download size") || t.starts_with("Installed size") {
            s.push_str(t);
            s.push('\n');
        }
    }
    s
}

fn ended(code: u32) {
    let a = app();
    let Some(job) = a.job.take() else { return };
    if let Some(mut c) = a.child.take() {
        c.close();
    }
    let lines = core::mem::take(&mut a.lines);
    match job {
        Job::Installed => {
            a.installed = parse_rpm(&lines);
            show_list();
        }
        Job::Available => {
            a.available = parse_yum(&lines);
            if !a.available.is_empty() {
                save_list("AVAIL.TXT", &lines);
            }
            show_list();
        }
        Job::Updates => {
            a.updates = parse_yum(&lines);
            save_list("UPDATES.TXT", &lines);
            show_list();
        }
        Job::Info(_) => pane(&lines.join("\r\n")),
        Job::Preview(verb, names) => {
            pane(&lines[lines.len().saturating_sub(KEEP)..].join("\r\n"));
            let s = summary(&lines);
            if s.is_empty() {
                say("Nothing to do: see below.");
            } else {
                let q = z(format!("{s}\nGo ahead? The list is in the window.").as_bytes());
                let yes = a.auto || message(a.client, &q, b"Package Manager", MB_YESNO | MB_ICONQUESTION | MB_MOVEABLE) == MBID_YES;
                if yes {
                    a.queue.push(Job::Run(verb, names));
                }
            }
        }
        Job::Run(verb, names) => {
            pane(&lines[lines.len().saturating_sub(KEEP)..].join("\r\n"));
            trace("pkgmgr: ", format!("yum {verb} {names} ended {code}").as_bytes());
            a.queue.push(Job::Installed);
            if verb != "install" {
                a.queue.push(Job::Updates);
            }
            if a.auto {
                post(a.client, WM_CLOSE, 0, 0);
            }
        }
        Job::Setup => {
            pane(&lines[lines.len().saturating_sub(KEEP)..].join("\r\n"));
            if code == 0 {
                say("Restart the computer, then open the Package Manager again.");
            } else {
                say("RPM and yum could not be installed: see below.");
            }
        }
    }
    next();
}

fn act() {
    let Some(p) = selected() else { return };
    let verb = match app().tab {
        Tab::Installed => "remove",
        Tab::Available => "install",
        Tab::Updates => "update",
    };
    add(Job::Preview(verb.to_string(), p.name.clone()));
}

fn details() {
    let Some(p) = selected() else { return };
    add(Job::Info(p.name.clone()));
}

fn refresh() {
    let a = app();
    a.queue.extend([Job::Installed, Job::Available, Job::Updates]);
    next();
}

fn set_tab(t: Tab) {
    app().tab = t;
    show_list();
}

// ---- The window ----

fn control(class: u16, text: &str, style: u32, id: u32) -> u32 {
    let t = z(text.as_bytes());
    let p = app().client;
    unsafe { WinCreateWindow(p, wc(class), t.as_ptr(), style | WS_VISIBLE, 0, 0, 10, 10, p, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
}

fn build() {
    control(WC_BUTTON, "I~nstalled", BS_AUTORADIOBUTTON | WS_GROUP | WS_TABSTOP, ID_INSTALLED);
    control(WC_BUTTON, "~Available", BS_AUTORADIOBUTTON, ID_AVAILABLE);
    control(WC_BUTTON, "U~pdates", BS_AUTORADIOBUTTON, ID_UPDATES);
    control(WC_STATIC, "~Search:", SS_TEXT | DT_RIGHT | DT_VCENTER | DT_MNEMONIC, ID_SEARCH_LABEL);
    control(WC_ENTRYFIELD, "", ES_MARGIN | ES_AUTOSCROLL | WS_TABSTOP | WS_GROUP, ID_SEARCH);
    let lb = control(WC_LISTBOX, "", LS_NOADJUSTPOS | LS_HORZSCROLL | WS_TABSTOP, ID_LIST);
    set_font(lb, b"10.System VIO");
    let mle = control(WC_MLE, "", MLS_BORDER | MLS_VSCROLL | MLS_READONLY | MLS_WORDWRAP, ID_TEXT);
    set_font(mle, b"10.System VIO");
    control(WC_STATIC, "", SS_TEXT | DT_LEFT | DT_VCENTER, ID_STATUS);
    control(WC_BUTTON, "~Install", BS_PUSHBUTTON | WS_TABSTOP | WS_GROUP, ID_ACTION);
    control(WC_BUTTON, "~Details", BS_PUSHBUTTON | WS_TABSTOP, ID_DETAILS);
    control(WC_BUTTON, "Update a~ll", BS_PUSHBUTTON | WS_TABSTOP, ID_UPDATE_ALL);
    control(WC_BUTTON, "Re~fresh", BS_PUSHBUTTON | WS_TABSTOP, ID_REFRESH);
    control(WC_BUTTON, "Set up ~yum", BS_PUSHBUTTON | WS_TABSTOP, ID_SETUP);
    send(ctl(ID_SEARCH), EM_SETTEXTLIMIT, 64, 0);
    send(ctl(ID_INSTALLED), BM_SETCHECK, 1, 0);
}

fn place(id: u32, x: i32, y: i32, cx: i32, cy: i32) {
    unsafe { WinSetWindowPos(ctl(id), 0, x, y, cx, cy, SWP_MOVE | SWP_SIZE) };
}

fn layout(cx: i32, cy: i32) {
    let m = 8;
    let top = cy - 32;
    place(ID_INSTALLED, m, top, 100, 24);
    place(ID_AVAILABLE, m + 104, top, 100, 24);
    place(ID_UPDATES, m + 208, top, 100, 24);
    place(ID_SEARCH_LABEL, m + 316, top, 64, 24);
    place(ID_SEARCH, m + 386, top + 2, (cx - m * 2 - 386).max(40), 20);
    let (bh, sh) = (30, 22);
    let mut x = m;
    for (id, w) in [(ID_ACTION, 96), (ID_DETAILS, 96), (ID_UPDATE_ALL, 104), (ID_REFRESH, 96), (ID_SETUP, 112)] {
        place(id, x, m, w, bh);
        x += w + 6;
    }
    let status = m + bh + 4;
    place(ID_STATUS, m, status, cx - m * 2, sh);
    let body = status + sh + 4;
    let rest = (top - 6 - body).max(40);
    let low = rest * 3 / 10;
    place(ID_TEXT, m, body, cx - m * 2, low);
    place(ID_LIST, m, body + low + 6, cx - m * 2, rest - low - 6);
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
        WM_SIZE => {
            layout((mp2 & 0xffff) as i32, (mp2 >> 16) as i32);
            0
        }
        WM_LINE => {
            let l = child::take_line(mp2);
            append(String::from_utf8_lossy(&l).to_string());
            0
        }
        WM_ENDED => {
            ended(mp1);
            0
        }
        WM_CONTROL => {
            let (id, code) = (mp1 & 0xffff, mp1 >> 16);
            match (id, code) {
                (ID_INSTALLED, BN_CLICKED) => set_tab(Tab::Installed),
                (ID_AVAILABLE, BN_CLICKED) => set_tab(Tab::Available),
                (ID_UPDATES, BN_CLICKED) => set_tab(Tab::Updates),
                (ID_SEARCH, EN_CHANGE) => show_list(),
                (ID_LIST, LN_SELECT) => update_controls(),
                (ID_LIST, LN_ENTER) => details(),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match mp1 & 0xffff {
                ID_ACTION => act(),
                ID_DETAILS => details(),
                ID_UPDATE_ALL => add(Job::Preview(String::from("update"), String::new())),
                ID_REFRESH => refresh(),
                ID_SETUP => add(Job::Setup),
                c if c == IDM_REFRESH as u32 => refresh(),
                c if c == IDM_EXIT as u32 => {
                    post(h, WM_CLOSE, 0, 0);
                }
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefWindowProc(h, msg, mp1, mp2) },
    }
}

fn main(s: &os2::Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    unsafe {
        *core::ptr::addr_of_mut!(APP) = Some(App {
            client: 0,
            root: None,
            tab: Tab::Installed,
            installed: Vec::new(),
            available: Vec::new(),
            updates: Vec::new(),
            shown: Vec::new(),
            queue: Vec::new(),
            job: None,
            child: None,
            lines: Vec::new(),
            auto: false,
        });
    }
    let (frame, client) = std_window(hab, CLASS, client_proc, 0, 0, FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST, b"Package Manager", 0);
    app().client = client;
    app().root = find_root();
    build();
    set_menu(frame, &[Menu::Sub(IDM_FILE, b"~File", alloc::vec![Menu::Item(IDM_REFRESH, b"~Refresh"), Menu::Sep, Menu::Item(IDM_EXIT, b"E~xit")])]);
    show_at_saved(hab, frame, b"PKGMGR", b"Position", Some((760, 560)));
    if app().root.is_some() {
        if let Some(l) = load_list("AVAIL.TXT") {
            app().available = parse_yum(&l);
        } else {
            app().queue.push(Job::Available);
        }
        match load_list("UPDATES.TXT") {
            Some(l) => app().updates = parse_yum(&l),
            None => app().queue.push(Job::Updates),
        }
        app().queue.insert(0, Job::Installed);
        // /TEST:INSTALL name
        let w = words(arg(s));
        if let [cmd, name] = &w[..] {
            let verb = if cmd.eq_ignore_ascii_case(b"/TEST:REMOVE") { "remove" } else { "install" };
            app().auto = true;
            app().queue.push(Job::Preview(verb.to_string(), String::from_utf8_lossy(name).to_string()));
        }
        next();
    } else {
        pane("RPM and yum are not installed.\r\n\r\nSet up yum downloads about 80 MB from rpm.netlabs.org: they are not part of rs2. It needs an HPFS or JFS system drive, and the computer has to restart afterwards.");
        say("RPM and yum are not installed.");
        update_controls();
    }
    message_loop(hab);
    save_position(frame, b"PKGMGR", b"Position");
    unsafe {
        WinDestroyWindow(frame);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}
