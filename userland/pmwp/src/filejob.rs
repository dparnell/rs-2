//! Copying and moving file system objects as the Workplace Shell does
//! it when the user asks (drag and drop, the Copy... and Move... menu
//! items): on a worker thread, with a progress window (shown when the
//! job takes longer than half a second; Cancel stops it after the
//! current file) and the name collision dialog when the target folder
//! has an object of that name (Rename, with a free name offered;
//! Replace; Skip; Cancel). Each file goes through DosCopy, so its EAs
//! come along (RS2SHELL's fileops.rs). The worker touches only files;
//! when it is done, the shell's thread moves the objects (paths,
//! handles, folders) and wakes the copies. Ported from RS2SHELL's
//! copy.rs, with the target folder dialog of the menu items.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::classes_gen::m;
use crate::fileops::{self, join};
use crate::pm::*;
use crate::wps;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Copy,
    Move,
}

/// One object to copy or move: the object and its path, the target
/// folder and its directory, and the name there.
pub struct JobItem {
    pub obj: usize,
    pub src: Vec<u8>,
    pub folder: usize,
    pub dir: Vec<u8>,
    pub name: Vec<u8>,
}

enum Question {
    /// `name` exists in `dir` already.
    Collision { dir: Vec<u8>, name: Vec<u8>, dir_obj: bool },
    Error(Vec<u8>),
}

enum Answer {
    Rename(Vec<u8>),
    Replace,
    Skip,
    Cancel,
}

/// A small lock for what both threads touch.
struct Lock<T> {
    busy: AtomicBool,
    v: core::cell::UnsafeCell<T>,
}

unsafe impl<T> Sync for Lock<T> {}

impl<T> Lock<T> {
    const fn new(v: T) -> Self {
        Lock { busy: AtomicBool::new(false), v: core::cell::UnsafeCell::new(v) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self.busy.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            unsafe { os2::DosSleep(0) };
        }
        let r = f(unsafe { &mut *self.v.get() });
        self.busy.store(false, Ordering::Release);
        r
    }
}

#[derive(Default)]
struct Status {
    file: Vec<u8>,
    to: Vec<u8>,
    files: u32,
    bytes: u64,
    total: fileops::Totals,
    question: Option<Question>,
    answer: Option<Answer>,
    /// The items done: (index, where it is now).
    done: Vec<(usize, Vec<u8>)>,
}

struct Job {
    op: Op,
    items: Vec<JobItem>,
    window: AtomicU32,
    sem: u32,
    cancel: AtomicBool,
    status: Lock<Status>,
}

const WM_JOB_PROGRESS: u32 = WM_USER + 20;
const WM_JOB_ASK: u32 = WM_USER + 21;
const WM_JOB_DONE: u32 = WM_USER + 22;
const TID_SHOW: u32 = 7;

const ID_ACTION: u16 = 501;
const ID_FILE: u16 = 502;
const ID_TO: u16 = 503;
const ID_COUNT: u16 = 504;
const ID_BARPOS: u16 = 505;
const BAR_CLASS: &[u8] = b"WPS_ProgressBar\0";

unsafe extern "C" {
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(h: u32) -> u32;
}

fn hab() -> u32 {
    unsafe { WinQueryAnchorBlock(HWND_DESKTOP) }
}

fn owner() -> u32 {
    crate::view::wins().iter().find(|w| w.desktop).map_or(HWND_DESKTOP, |w| w.frame)
}

/// Start a job (returns at once; the objects change when it is done).
pub fn start(op: Op, items: Vec<JobItem>) {
    if items.is_empty() {
        return;
    }
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if !REGISTERED.swap(true, Ordering::Relaxed) {
        unsafe { WinRegisterClass(hab(), BAR_CLASS.as_ptr(), bar_proc, 0, 4) };
    }
    let mut sem = 0;
    unsafe { os2::DosCreateEventSem(core::ptr::null(), &mut sem, 0, 0) };
    let job = Box::into_raw(Box::new(Job { op, items, window: AtomicU32::new(0), sem, cancel: AtomicBool::new(false), status: Lock::new(Status::default()) }));
    let win = progress_window(unsafe { &*job });
    unsafe { (*job).window.store(win, Ordering::Release) };
    let mut tid = 0;
    let rc = unsafe { os2::DosCreateThread(&mut tid, worker, job as u32, 0, 64 * 1024) };
    if rc != 0 {
        message(owner(), alloc::format!("Cannot start the copy (error {rc}).").as_bytes(), b"Error", MB_OK | MB_ERROR);
        unsafe {
            WinDestroyWindow(win);
            drop(Box::from_raw(job));
        }
    }
}

// ---- The worker ----

extern "C" fn worker(param: u32) {
    let job = unsafe { &*(param as *const Job) };
    // A message queue, so that it may post to the shell's thread.
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let mut total = fileops::Totals::default();
    for it in &job.items {
        let t = fileops::measure(&it.src);
        total.files += t.files;
        total.bytes += t.bytes;
    }
    job.status.with(|s| s.total = total);
    let win = job.window.load(Ordering::Acquire);
    for (i, it) in job.items.iter().enumerate() {
        if job.cancel.load(Ordering::Relaxed) {
            break;
        }
        let (src, dir) = (&it.src, &it.dir);
        let mut name = it.name.clone();
        let mut replace = false;
        // The target has one of that name already (copying into the same
        // folder too): ask.
        loop {
            let dst = join(dir, &name);
            let same = fileops::same_path(src, &dst);
            if !fileops::exists(&dst) || (same && job.op == Op::Move) {
                break;
            }
            match ask(job, Question::Collision { dir: dir.clone(), name: name.clone(), dir_obj: fileops::is_dir(src) }) {
                Answer::Rename(n) => name = n,
                Answer::Replace if !same => {
                    replace = true;
                    break;
                }
                Answer::Replace => {}
                Answer::Skip => {
                    name.clear();
                    break;
                }
                Answer::Cancel => {
                    job.cancel.store(true, Ordering::Relaxed);
                    name.clear();
                    break;
                }
            }
        }
        if name.is_empty() {
            continue;
        }
        let dst = join(dir, &name);
        if job.op == Op::Move && fileops::same_path(src, &dst) {
            continue;
        }
        let mut progress = |f: &[u8], size: u32| {
            job.status.with(|s| {
                s.file = f.to_vec();
                s.to = dir.clone();
                s.files += 1;
                s.bytes += size as u64;
            });
            post(win, WM_JOB_PROGRESS, 0, 0);
            !job.cancel.load(Ordering::Relaxed)
        };
        let rc = match job.op {
            Op::Copy => fileops::copy_tree(src, &dst, replace, &mut progress),
            Op::Move => fileops::move_path(src, &dst, replace, &mut progress),
        };
        if rc == 0 {
            job.status.with(|s| s.done.push((i, dst.clone())));
        }
        if rc == fileops::ERROR_INTERRUPT {
            break;
        }
        if rc != 0 {
            let what = if job.op == Op::Copy { "copy" } else { "move" };
            let why = match rc {
                5 => "Access was denied",
                16 => "A folder cannot go into itself",
                32 | 33 => "A file is in use",
                39 | 112 => "The disk is full",
                19 => "The disk is write protected",
                206 => "A name is too long for the target's file system",
                _ => "An error occurred",
            };
            let text = alloc::format!("Cannot {what} {}. {why} (error {rc}). Go on with the rest?", os2::path::from_bytes(fileops::name_of(src)));
            if matches!(ask(job, Question::Error(text.into_bytes())), Answer::Cancel) {
                break;
            }
        }
    }
    post(win, WM_JOB_DONE, param, 0);
    unsafe {
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
}

/// Ask the shell's thread and wait for the answer.
fn ask(job: &Job, q: Question) -> Answer {
    let mut n = 0;
    unsafe { os2::DosResetEventSem(job.sem, &mut n) };
    job.status.with(|s| {
        s.question = Some(q);
        s.answer = None;
    });
    post(job.window.load(Ordering::Acquire), WM_JOB_ASK, 0, 0);
    unsafe { os2::DosWaitEventSem(job.sem, 0xffff_ffff) };
    job.status.with(|s| s.answer.take()).unwrap_or(Answer::Cancel)
}

/// The objects follow what happened to their files (the shell's thread).
fn finish(job: &Job) {
    let done = job.status.with(|s| core::mem::take(&mut s.done));
    for (i, dst) in done {
        let it = &job.items[i];
        match job.op {
            Op::Move => {
                if it.obj != 0 {
                    crate::fs::moved(it.obj, it.folder, &dst);
                }
            }
            Op::Copy => {
                let o = wps::object_from_path(&dst);
                if o != 0 && it.folder != 0 {
                    crate::obj::d(o).folder = it.folder;
                    wps::call("WPFolder", m::WPFolder::wpAddToContent, &[it.folder, o]);
                }
            }
        }
    }
}

// ---- The progress window ----

static JOBS: Lock<Vec<(u32, usize)>> = Lock::new(Vec::new());

fn job_of(win: u32) -> Option<&'static Job> {
    JOBS.with(|v| v.iter().find(|j| j.0 == win).map(|j| unsafe { &*(j.1 as *const Job) }))
}

fn label(text: &[u8], (x, y, cx): (i16, i16, i16), id: u16) -> Item {
    // SS_TEXT | DT_LEFT
    item(WC_STATIC, text, WS_VISIBLE | 0x0001, (x, y, cx, 8), id)
}

fn progress_window(job: &Job) -> u32 {
    let title: &[u8] = if job.op == Op::Copy { b"Copy" } else { b"Move" };
    let tpl = template(dialog_frame(title, 240, 92), alloc::vec![
        label(if job.op == Op::Copy { b"Copying:" } else { b"Moving:" }, (6, 78, 228), ID_ACTION),
        label(b"", (12, 66, 222), ID_FILE),
        label(b"", (6, 52, 228), ID_TO),
        label(b"", (6, 40, 228), ID_COUNT),
        label(b"", (6, 26, 228), ID_BARPOS),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP | WS_GROUP, (95, 4, 50, 14), DID_CANCEL as u16),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, HWND_DESKTOP, progress_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return 0;
    }
    JOBS.with(|v| v.push((dlg, job as *const Job as usize)));
    // The bar where the placeholder is.
    let ph = unsafe { WinWindowFromID(dlg, ID_BARPOS as u32) };
    let s = window_pos(ph);
    unsafe {
        WinShowWindow(ph, 0);
        WinCreateWindow(dlg, BAR_CLASS.as_ptr() as u32, b"\0".as_ptr(), WS_VISIBLE, s.x, s.y, s.cx, s.cy.max(10), dlg, HWND_TOP, 600, core::ptr::null(), core::ptr::null());
        WinStartTimer(hab(), dlg, TID_SHOW, 500);
    }
    center(dlg);
    dlg
}

fn mb(n: u64) -> alloc::string::String {
    let tenths = n * 10 / (1024 * 1024);
    alloc::format!("{}.{} MB", tenths / 10, tenths % 10)
}

fn update(dlg: u32, job: &Job) {
    let (file, to, files, bytes, total) = job.status.with(|s| (s.file.clone(), s.to.clone(), s.files, s.bytes, s.total));
    set_item_text(dlg, ID_FILE as u32, &file);
    let mut t = b"To: ".to_vec();
    t.extend_from_slice(&to);
    set_item_text(dlg, ID_TO as u32, &t);
    let c = alloc::format!("File {} of {} ({} of {})", files, total.files.max(files), mb(bytes), mb(total.bytes.max(bytes)));
    set_item_text(dlg, ID_COUNT as u32, c.as_bytes());
    let permille = if total.bytes > 0 { (bytes * 1000 / total.bytes).min(1000) as u32 } else if total.files > 0 { files * 1000 / total.files } else { 0 };
    let bar = unsafe { WinWindowFromID(dlg, 600) };
    unsafe {
        WinSetWindowULong(bar, QWL_USER, permille);
        WinInvalidateRect(bar, core::ptr::null(), 0);
    }
}

extern "C" fn progress_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_TIMER if a == TID_SHOW => {
            unsafe {
                WinStopTimer(hab(), h, TID_SHOW);
                WinSetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER | SWP_ACTIVATE);
            }
            0
        }
        WM_JOB_PROGRESS => {
            if let Some(job) = job_of(h) {
                update(h, job);
            }
            0
        }
        WM_JOB_ASK => {
            let Some(job) = job_of(h) else { return 0 };
            // The progress window shows now, below the question.
            unsafe {
                WinStopTimer(hab(), h, TID_SHOW);
                WinSetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_SHOW | SWP_ZORDER);
            }
            let q = job.status.with(|s| s.question.take());
            let ans = match q {
                Some(Question::Collision { dir, name, dir_obj }) => collision(h, &dir, &name, dir_obj),
                Some(Question::Error(t)) => {
                    if message(h, &t, b"Error", MB_OKCANCEL | MB_ERROR) == MBID_OK { Answer::Skip } else { Answer::Cancel }
                }
                None => Answer::Cancel,
            };
            job.status.with(|s| s.answer = Some(ans));
            unsafe { os2::DosPostEventSem(job.sem) };
            0
        }
        WM_JOB_DONE => {
            let job = unsafe { Box::from_raw(a as *mut Job) };
            JOBS.with(|v| v.retain(|j| j.0 != h));
            finish(&job);
            unsafe {
                os2::DosCloseEventSem(job.sem);
                WinStopTimer(hab(), h, TID_SHOW);
                WinDestroyWindow(h);
            }
            0
        }
        WM_COMMAND if a & 0xffff == DID_CANCEL => {
            if let Some(job) = job_of(h) {
                job.cancel.store(true, Ordering::Relaxed);
                set_item_text(h, ID_ACTION as u32, b"Stopping after this file...");
            }
            0
        }
        // Closing it is cancelling.
        WM_CLOSE => {
            if let Some(job) = job_of(h) {
                job.cancel.store(true, Ordering::Relaxed);
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

extern "C" fn bar_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    if m != WM_PAINT {
        return unsafe { WinDefWindowProc(h, m, a, b) };
    }
    let mut r = RectL::default();
    unsafe {
        let hps = WinBeginPaint(h, 0, &mut r);
        let mut all = RectL::default();
        WinQueryWindowRect(h, &mut all);
        let permille = WinQueryWindowULong(h, QWL_USER).min(1000) as i32;
        // CLR_WHITE, CLR_DARKBLUE, CLR_DARKGRAY
        WinFillRect(hps, &all, -2);
        let w = (all.r - all.l - 2) * permille / 1000;
        let done = RectL { l: all.l + 1, b: all.b + 1, r: all.l + 1 + w, t: all.t - 1 };
        WinFillRect(hps, &done, 9);
        for e in [RectL { l: all.l, b: all.b, r: all.r, t: all.b + 1 }, RectL { l: all.l, b: all.t - 1, r: all.r, t: all.t }, RectL { l: all.l, b: all.b, r: all.l + 1, t: all.t }, RectL { l: all.r - 1, b: all.b, r: all.r, t: all.t }] {
            WinFillRect(hps, &e, 10);
        }
        WinEndPaint(hps);
    }
    0
}

// ---- The name collision dialog ----

const ID_CTEXT: u16 = 520;
const ID_CNAME: u16 = 521;
const ID_REPLACE: u16 = 522;
const ID_SKIP: u16 = 523;

static mut COLLISION_NAME: Vec<u8> = Vec::new();

extern "C" fn collision_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            center(h);
            let n = unsafe { (*core::ptr::addr_of!(COLLISION_NAME)).clone() };
            set_item_text(h, ID_CNAME as u32, &n);
            unsafe { WinSendDlgItemMsg(h, ID_CNAME as u32, EM_SETTEXTLIMIT, 254, 0) };
            0
        }
        WM_COMMAND => {
            let id = a & 0xffff;
            if id == DID_OK {
                unsafe { *core::ptr::addr_of_mut!(COLLISION_NAME) = item_text(h, ID_CNAME as u32) };
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// The name collision dialog: an object of that name is in the target
/// folder already.
fn collision(owner: u32, dir: &[u8], name: &[u8], dir_obj: bool) -> Answer {
    let long = fileops::long_names(dir);
    let free = pmwin::drag::unique_name(name, long, |n| fileops::exists(&join(dir, n)));
    let mut text = b"An object named \"".to_vec();
    text.extend_from_slice(name);
    text.extend_from_slice(b"\" is already in ");
    text.extend_from_slice(fileops::strip(dir));
    text.extend_from_slice(if dir_obj { b". Rename the folder you are copying, replace the one there (and everything in it), or skip it." } else { b". Rename the object you are copying, replace the one there, or skip it." });
    let tpl = template(dialog_frame(b"Name Collision", 250, 100), alloc::vec![
        // SS_TEXT | DT_WORDBREAK
        item(WC_STATIC, &text, WS_VISIBLE | 0x0001 | 0x4000, (6, 50, 238, 44), ID_CTEXT),
        label(b"New name:", (6, 38, 60), 0xffff),
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 26, 234, 8), ID_CNAME),
        item(WC_BUTTON, b"~Rename", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 4, 54, 14), DID_OK as u16),
        item(WC_BUTTON, b"Re~place", WS_VISIBLE | WS_TABSTOP, (64, 4, 54, 14), ID_REPLACE),
        item(WC_BUTTON, b"~Skip", WS_VISIBLE | WS_TABSTOP, (122, 4, 54, 14), ID_SKIP),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (180, 4, 54, 14), DID_CANCEL as u16),
    ]);
    unsafe { *core::ptr::addr_of_mut!(COLLISION_NAME) = free };
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, collision_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return Answer::Cancel;
    }
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    match r {
        DID_OK => {
            let n: Vec<u8> = unsafe { (*core::ptr::addr_of!(COLLISION_NAME)).clone() };
            let n: Vec<u8> = n.into_iter().filter(|&c| c != b'\\' && c != b'/').collect();
            if n.is_empty() { Answer::Skip } else { Answer::Rename(n) }
        }
        x if x == ID_REPLACE as u32 => Answer::Replace,
        x if x == ID_SKIP as u32 => Answer::Skip,
        _ => Answer::Cancel,
    }
}

// ---- The target folder dialog ----

/// What the target folder dialog lists.
#[derive(Clone)]
enum Choice {
    Up,
    Desktop,
    Dir(Vec<u8>),
}

struct Picker {
    /// The directory listed (None: the top: the desktop and the drives).
    at: Option<Vec<u8>>,
    list: Vec<Choice>,
    name: Option<Vec<u8>>,
    result: Option<(usize, Vec<u8>)>,
}

static mut PICKER: Option<Picker> = None;

fn picker() -> &'static mut Picker {
    unsafe { (*core::ptr::addr_of_mut!(PICKER)).as_mut().unwrap() }
}

const ID_TPATH: u16 = 530;
const ID_TLIST: u16 = 531;
const ID_TNAME: u16 = 532;

fn desktop_path() -> Vec<u8> {
    let d = crate::fs::desktop();
    let mut p = if d != 0 { wps::path_of(d).unwrap_or_default() } else { Vec::new() };
    if !p.is_empty() && !p.ends_with(b"\\") {
        p.push(b'\\');
    }
    p
}

fn choice_text(c: &Choice) -> Vec<u8> {
    match c {
        Choice::Up => b"[..] (up)".to_vec(),
        Choice::Desktop => b"Desktop".to_vec(),
        Choice::Dir(p) => {
            if p.len() <= 3 {
                p.clone()
            } else {
                let o = wps::by_path(&crate::util::normalize(p));
                let mut t = o.map(crate::obj::title).filter(|t| !t.is_empty()).unwrap_or_else(|| fileops::name_of(fileops::strip(p)).to_vec());
                t.retain(|&c| c != b'\n');
                t.push(b'\\');
                t
            }
        }
    }
}

fn path_text(c: &Choice) -> Vec<u8> {
    match c {
        Choice::Dir(p) => p.clone(),
        Choice::Up => picker().at.clone().unwrap_or_default(),
        Choice::Desktop => desktop_path(),
    }
}

fn drives() -> Vec<u8> {
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut cur, &mut map) };
    (0..26u8).filter(|d| map & (1 << d) != 0).map(|d| b'A' + d).collect()
}

fn fill_picker(h: u32) {
    let pk = picker();
    let mut list = Vec::new();
    match &pk.at {
        None => {
            list.push(Choice::Desktop);
            for c in drives() {
                list.push(Choice::Dir(alloc::vec![c, b':', b'\\']));
            }
        }
        Some(d) => {
            list.push(Choice::Up);
            let mut subs: Vec<Vec<u8>> = fileops::listing(d).into_iter().filter(|e| e.1).map(|e| e.0).collect();
            subs.sort_by_key(|a| a.to_ascii_uppercase());
            for s in subs {
                let mut p = join(d, &s);
                p.push(b'\\');
                list.push(Choice::Dir(p));
            }
        }
    }
    unsafe { WinSendDlgItemMsg(h, ID_TLIST as u32, LM_DELETEALL, 0, 0) };
    for c in &list {
        let t = z(&choice_text(c));
        unsafe { WinSendDlgItemMsg(h, ID_TLIST as u32, LM_INSERTITEM, LIT_END, t.as_ptr() as u32) };
    }
    if let Some(d) = &pk.at {
        set_item_text(h, ID_TPATH as u32, d);
    }
    picker().list = list;
}

fn selection(h: u32) -> Option<Choice> {
    let i = unsafe { WinSendDlgItemMsg(h, ID_TLIST as u32, LM_QUERYSELECTION, LIT_NONE, 0) } as i32 as i16;
    if i < 0 {
        return None;
    }
    picker().list.get(i as usize).cloned()
}

/// The folder typed or picked.
fn target_of(text: &[u8]) -> usize {
    let t: Vec<u8> = text.iter().copied().filter(|&c| c != b'\r' && c != b'\n').collect();
    if t.eq_ignore_ascii_case(b"Desktop") {
        return crate::fs::desktop();
    }
    if t.len() >= 3 && t[1] == b':' && fileops::is_dir(&t) {
        let o = wps::object_from_path(&t);
        if o != 0 && wps::is(o, "WPFolder") {
            return o;
        }
    }
    0
}

extern "C" fn picker_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_INITDLG => {
            center(h);
            if let Some(n) = picker().name.clone() {
                set_item_text(h, ID_TNAME as u32, &n);
            }
            fill_picker(h);
            0
        }
        WM_CONTROL if a & 0xffff == ID_TLIST as u32 => {
            let Some(c) = selection(h) else { return 0 };
            match a >> 16 {
                LN_SELECT => set_item_text(h, ID_TPATH as u32, &path_text(&c)),
                LN_ENTER => {
                    let pk = picker();
                    match c {
                        Choice::Up => {
                            let at = pk.at.clone().unwrap_or_default();
                            pk.at = if at.len() <= 3 { None } else { Some(fileops::parent(&at)) };
                            fill_picker(h);
                        }
                        Choice::Dir(p) => {
                            pk.at = Some(p);
                            fill_picker(h);
                        }
                        Choice::Desktop => {
                            let p = desktop_path();
                            if !p.is_empty() {
                                pk.at = Some(p);
                                fill_picker(h);
                            }
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            let id = a & 0xffff;
            if id == DID_OK {
                let t = item_text(h, ID_TPATH as u32);
                let f = target_of(&t);
                if f == 0 {
                    message(h, b"Pick a folder from the list, or type the path of a directory.", b"Target folder", MB_OK | MB_WARNING);
                    return 0;
                }
                let name = if picker().name.is_some() { item_text(h, ID_TNAME as u32) } else { Vec::new() };
                picker().result = Some((f, name));
            }
            unsafe { WinDismissDlg(h, id) };
            0
        }
        _ => unsafe { WinDefDlgProc(h, m, a, b) },
    }
}

/// The target folder dialog of Copy..., Move..., Create shadow... and
/// Create another...: the folder (and, with `name`, the name typed).
pub fn pick_folder(title: &[u8], button: &[u8], name: Option<&[u8]>, start: Option<&[u8]>) -> Option<(usize, Vec<u8>)> {
    unsafe {
        PICKER = Some(Picker { at: start.map(|s| s.to_vec()), list: Vec::new(), name: name.map(|n| n.to_vec()), result: None });
    }
    let mut v = alloc::vec![
        label(b"Target folder:", (6, 150, 200), 0xffff),
        item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 138, 218, 8), ID_TPATH),
        // LS_NOADJUSTPOS
        item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004, (6, if name.is_some() { 50 } else { 26 }, 222, if name.is_some() { 82 } else { 106 }), ID_TLIST),
    ];
    if name.is_some() {
        v.push(label(b"New name:", (6, 38, 200), 0xffff));
        v.push(item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (8, 26, 218, 8), ID_TNAME));
    }
    v.push(item(WC_BUTTON, button, WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 4, 54, 14), DID_OK as u16));
    v.push(item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (64, 4, 54, 14), DID_CANCEL as u16));
    let tpl = template(dialog_frame(title, 234, 166), v);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner(), picker_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return None;
    }
    unsafe {
        WinProcessDlg(dlg);
        WinDestroyWindow(dlg);
    }
    let r = picker().result.take();
    unsafe { PICKER = None };
    r
}

// ---- Copying and moving objects ----

/// Copy or move objects into a folder as the user asked: file system
/// objects through a job (progress, name collisions), abstract ones at
/// once. `name` renames a single object on the way.
pub fn transfer(op: Op, objs: &[usize], folder: usize, name: Option<&[u8]>) {
    if folder == 0 {
        return;
    }
    let dir = wps::path_of(folder);
    let mut items = Vec::new();
    for &o in objs {
        let style = crate::obj::d(o).style;
        let barred = match op {
            Op::Copy => style & crate::obj::OBJSTYLE_NOCOPY != 0,
            Op::Move => style & crate::obj::OBJSTYLE_NOMOVE != 0,
        };
        if barred {
            continue;
        }
        let name = name.filter(|n| !n.is_empty() && objs.len() == 1);
        match (wps::path_of(o), &dir) {
            (Some(src), Some(dir)) if src.len() > 3 => {
                let n = name.map(|n| n.to_vec()).unwrap_or_else(|| fileops::name_of(&src).to_vec());
                items.push(JobItem { obj: o, src, folder, dir: dir.clone(), name: n });
            }
            _ => {
                let made = match op {
                    Op::Copy => crate::obj::call(m::WPObject::wpCopyObject, &[o, folder, 0]),
                    Op::Move => {
                        if crate::obj::call(m::WPObject::wpMoveObject, &[o, folder]) != 0 { o } else { 0 }
                    }
                };
                if let (Some(n), true) = (name, made != 0) {
                    let t = z(n);
                    crate::obj::call(m::WPObject::wpSetTitle, &[made, t.as_ptr() as usize]);
                }
            }
        }
    }
    start(op, items);
}
