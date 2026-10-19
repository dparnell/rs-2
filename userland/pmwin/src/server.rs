//! The PM server: a small process (`C:\OS2\PMSRV.EXE`, which only calls
//! `Rs2PmServer`) started with the first PM program's message queue. It
//! owns the desktop window and the input thread, and keeps the desktop
//! usable whatever the applications do:
//!
//! - The input thread (time-critical) reads raw input, moves the pointer
//!   and posts every event to the queue of the thread that owns the
//!   target window (`input.rs`). It never waits for an application.
//! - It notices queues that have had input waiting for longer than
//!   PM_HANGTIME milliseconds (default 2000) without their thread coming
//!   to get it: those are "not responding". Their frames are moved,
//!   sized, activated and hidden by the server, their title bars say so,
//!   and what needs painting in their windows gets a placeholder until
//!   the thread repaints.
//! - When a PM process ends (the kernel reports it as an input event), it
//!   removes the process's windows, queues, classes and GPI objects; the
//!   windows that come to light repaint.
//! - Ctrl+Esc opens the window list: switch to a window or end its
//!   program.
//! - When the last PM program has ended, the server ends too, and with it
//!   the PM session.

#![allow(non_snake_case)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use gpi::{Ps, Rect};
use pmwin::HWND_DESKTOP;

use crate::draw::Canvas;
use crate::frame::{self, FID_TITLEBAR};
use crate::input::{self, Input};
use crate::lock::wndproc;
use crate::msg::{self, post_queue, qmsg};
use crate::state::{self, now, pid, st};
use crate::win::{self, *};
use crate::{color, ctl, lock, pointer};

unsafe extern "C" {
    #[link_name = "RS2API.2"]
    fn Rs2ReadInput(ev: *mut Input, ms: u32) -> u32;
    #[link_name = "DOSCALLS.283"]
    fn DosExecPgm(fail: *mut u8, cbfail: u32, flags: u32, args: *const u8, env: *const u8, res: *mut [u32; 2], name: *const u8) -> u32;
    #[link_name = "DOSCALLS.320"]
    fn DosQueryModuleName(hmod: u32, cb: u32, buf: *mut u8) -> u32;
    #[link_name = "DOSCALLS.311"]
    fn DosCreateThread(ptid: *mut u32, pfn: extern "C" fn(u32) -> u32, param: u32, flags: u32, cb: u32) -> u32;
    #[link_name = "DOSCALLS.236"]
    fn DosSetPriority(scope: u32, class: u32, delta: i32, id: u32) -> u32;
    #[link_name = "DOSCALLS.227"]
    fn DosScanEnv(name: *const u8, value: *mut *const u8) -> u32;
    #[link_name = "DOSCALLS.235"]
    fn DosKillProcess(action: u32, pid: u32) -> u32;
    #[link_name = "SESMGR.37"]
    fn DosStartSession(sd: *const StartData, sid: *mut u32, pid: *mut u32) -> u32;
    #[link_name = "RS2API.11"]
    fn Rs2VioQuerySessions(entries: *mut VioSession, max: u32, found: *mut u32, all: u32) -> u32;
    #[link_name = "RS2API.16"]
    fn Rs2VioControl(sid: u32, cmd: u32, arg: u32) -> u32;
}

/// STARTDATA (the fields up to SessionType).
#[repr(C, packed)]
struct StartData {
    length: u16,
    related: u16,
    fgbg: u16,
    trace: u16,
    title: *const u8,
    name: *const u8,
    inputs: *const u8,
    termq: *const u8,
    env: *const u8,
    inherit: u16,
    session_type: u16,
}

/// RS2VIOSESSION (RS2API 11).
#[repr(C)]
#[derive(Clone, Copy)]
struct VioSession {
    sid: u32,
    flags: u32,
    changes: u32,
    leader: u32,
    title: [u8; 64],
}

const VS_WINDOWED: u32 = 0x01;
const VS_PM: u32 = 0x08;

/// Private messages to the server's window thread.
pub const WM_RS2_GHOSTS: u32 = 0x0ff0;
const WM_RS2_WINLIST: u32 = 0x0ff1;
const TID_CHECK: u32 = 0x7f01;

const EXEC_BACKGROUND: u32 = 4;

/// This process is the PM server.
static IS_SERVER: AtomicBool = AtomicBool::new(false);
/// PMWIN's module handle (from _DLL_InitTerm).
pub static HMOD: AtomicU32 = AtomicU32::new(0);
/// Time of the last not-responding check.
static LAST_CHECK: AtomicU32 = AtomicU32::new(0);
/// The server is painting a placeholder (WinBeginPaint keeps the update
/// region).
static GHOST: AtomicBool = AtomicBool::new(false);

pub fn ghost_painting() -> bool {
    GHOST.load(Ordering::Relaxed)
}

/// Start the PM server unless it runs (called with the lock held when a
/// process creates its first message queue).
pub fn ensure_running() {
    let s = st();
    if IS_SERVER.load(Ordering::Relaxed) || s.server_pid != 0 {
        return;
    }
    // C:\OS2\DLL\PMWIN.DLL -> C:\OS2\PMSRV.EXE
    let mut path = [0u8; 260];
    if unsafe { DosQueryModuleName(HMOD.load(Ordering::Relaxed), 240, path.as_mut_ptr()) } != 0 {
        return;
    }
    let len = path.iter().position(|&b| b == 0).unwrap_or(0);
    let mut cut = len;
    for _ in 0..2 {
        cut = path[..cut].iter().rposition(|&b| b == b'\\').unwrap_or(0);
    }
    let name = b"\\PMSRV.EXE\0";
    path[cut..cut + name.len()].copy_from_slice(name);
    let mut fail = [0u8; 64];
    let mut res = [0u32; 2];
    let args = b"PMSRV\0\0\0";
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, EXEC_BACKGROUND, args.as_ptr(), core::ptr::null(), &mut res, path.as_ptr()) };
    if rc == 0 {
        st().server_pid = res[0];
    }
}

/// Entry of PMSRV.EXE: run the PM server until the last PM program ends.
#[unsafe(no_mangle)]
pub extern "C" fn Rs2PmServer() -> u32 {
    IS_SERVER.store(true, Ordering::Relaxed);
    if crate::misc::WinInitialize(0) == 0 {
        return 1;
    }
    let hmq = msg::WinCreateMsgQueue(1, 0);
    if hmq == 0 {
        return 1;
    }
    {
        let s = st();
        s.server_pid = pid();
        s.server_queue = hmq;
        s.hang_ms = hang_time();
        if let Some(d) = s.desk.get_mut(HWND_DESKTOP) {
            d.queue = hmq;
            d.proc_ = desktop_entry as *const () as u32;
        }
        let all = s.desk.screen_rect(HWND_DESKTOP);
        s.desk.invalidate_screen(all);
        pointer::init(s);
        win::WinRegisterClass(1, b"RS2WindowList\0".as_ptr(), wl_entry as *const () as u32, 0, 0);
    }
    let mut tid = 0;
    unsafe { DosCreateThread(&mut tid, input_thread_entry, 0, 2, 64 * 1024) };
    msg::WinStartTimer(1, HWND_DESKTOP, TID_CHECK, 250);
    let mut m = state::QMsg::default();
    while msg::WinGetMsg(1, &mut m, 0, 0, 0) != 0 {
        msg::WinDispatchMsg(1, &m);
    }
    // The last PM program has ended.
    let s = st();
    s.server_pid = 0;
    s.server_queue = 0;
    if let Some(d) = s.desk.get_mut(HWND_DESKTOP) {
        d.queue = 0;
        d.proc_ = 0;
    }
    msg::WinDestroyMsgQueue(hmq);
    0
}

/// PM_HANGTIME (milliseconds, from the environment: SET PM_HANGTIME=n
/// in CONFIG.SYS).
fn hang_time() -> u32 {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { DosScanEnv(b"PM_HANGTIME\0".as_ptr(), &mut v) } != 0 || v.is_null() {
        return 2000;
    }
    let mut n: u32 = 0;
    let mut i = 0;
    loop {
        let b = unsafe { *v.add(i) };
        if !b.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
        i += 1;
    }
    if i == 0 { 2000 } else { n.clamp(250, 600_000) }
}

/// The input thread starts on an OS/2 stack (4-byte aligned).
#[unsafe(naked)]
extern "C" fn input_thread_entry(_arg: u32) -> u32 {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16",
        "call {f}",
        "mov esp, ebp", "pop ebp", "ret",
        f = sym input_thread,
    );
}

extern "C" fn input_thread() -> u32 {
    // PRTYS_THREAD, PRTYC_TIMECRITICAL.
    unsafe { DosSetPriority(2, 3, 31, 0) };
    loop {
        let mut ev = Input::default();
        let rc = unsafe { Rs2ReadInput(&mut ev, 100) };
        lock::enter_quiet();
        if rc == 0 {
            input::event(&ev);
            // Whatever else has arrived, without waiting.
            let mut n = 0;
            while n < 64 && unsafe { Rs2ReadInput(&mut ev, 0) } == 0 {
                input::event(&ev);
                n += 1;
            }
        }
        check_hung();
        let me = pid();
        let quit = st().procs.iter().all(|&p| p == me);
        if quit {
            let q = st().server_queue;
            post_queue(q, qmsg(0, WM_QUIT, 0, 0));
        }
        lock::leave();
        if quit {
            return 0;
        }
    }
}

// ---- Not responding ----

/// Mark the queues whose threads have not come for their input within
/// the hang time (and unmark those that came back).
fn check_hung() {
    let t = now();
    if t.wrapping_sub(LAST_CHECK.load(Ordering::Relaxed)) < 200 {
        return;
    }
    LAST_CHECK.store(t, Ordering::Relaxed);
    let s = st();
    let me = pid();
    let limit = s.hang_ms;
    let mut changed = Vec::new();
    for (&h, q) in s.queues.iter_mut() {
        if q.pid == me {
            continue;
        }
        let hung = q.msgs.hung(t, limit);
        if hung != q.hung {
            q.hung = hung;
            changed.push(h);
        }
    }
    let mut any = false;
    for q in changed {
        // Their title bars change; while hung, the server paints them.
        for h in win::top_windows_of(q) {
            if st().frames.contains_key(&h) {
                let tb = WinWindowFromID(h, FID_TITLEBAR);
                if tb != 0 {
                    st().desk.invalidate(tb, None, false);
                }
            }
        }
        any |= st().queues.get(&q).is_some_and(|x| x.hung);
    }
    if any {
        // A program that hid the pointer and stopped does not keep it
        // hidden for everyone.
        if st().ptr_level < 0 {
            st().ptr_level = -1;
            pointer::WinShowPointer(HWND_DESKTOP, 1);
        }
        let q = st().server_queue;
        post_queue(q, qmsg(HWND_DESKTOP, WM_RS2_GHOSTS, 0, 0));
    }
}

/// Paint placeholders where windows of threads that do not respond need
/// painting: frames and title bars as the server knows them (the title
/// says "not responding"), the rest plain. Their update regions stay as
/// they are, so the threads repaint when they come back.
pub fn paint_ghosts() {
    let s = st();
    let hung: Vec<u32> = s.queues.iter().filter(|(_, q)| q.hung).map(|(&h, _)| h).collect();
    if hung.is_empty() {
        return;
    }
    // Parents before children, bottom of the z-order first.
    let mut order = Vec::new();
    let mut stack = alloc::vec![HWND_DESKTOP];
    while let Some(h) = stack.pop() {
        let Some(w) = s.desk.get(h) else { continue };
        if !w.visible_style() {
            continue;
        }
        if hung.contains(&w.queue) {
            order.push(h);
        }
        stack.extend(w.children.iter().rev().copied());
    }
    if order.is_empty() {
        return;
    }
    pointer::hide(s);
    for h in order {
        ghost(h);
    }
    pointer::show(st());
}

fn ghost(h: u32) {
    let s = st();
    let Some(w) = s.desk.get(h) else { return };
    // What needs painting and has not been painted yet.
    let mut need = w.update.clone();
    for g in &w.ghost.rects {
        need.subtract(g);
    }
    if need.is_empty() {
        return;
    }
    let (cx, cy) = (w.cx, w.cy);
    let o = s.desk.origin(h);
    let vis = s.desk.visible_region(h, true);
    let mut dev = need.clone();
    dev.offset(o.x, o.y);
    let clip: Vec<Rect> = vis.rects.iter().flat_map(|v| dev.rects.iter().map(move |u| v.intersect(u))).filter(|r| !r.is_empty()).collect();
    let proc_ = w.proc_;
    // Built-in controls (title bar, menus, scroll bars, buttons...) are
    // PMWIN code that only draws: they paint themselves, here.
    let builtin = s.classes.get(w.class as usize).is_some_and(|c| c.pid == 0 && c.proc_ == proc_);
    // A container's records are its program's memory: a plain
    // placeholder for it.
    if builtin && !s.frames.contains_key(&h) && !crate::container::is_container(h) {
        GHOST.store(true, Ordering::Relaxed);
        msg::call(proc_, h, WM_PAINT, 0, 0);
        GHOST.store(false, Ordering::Relaxed);
        if let Some(w) = st().desk.get_mut(h) {
            w.ghost = w.update.clone();
        }
        return;
    }
    let mut ps = Ps::new(o, clip);
    let mut c = Canvas::new(&mut ps, s.surface, cx, cy);
    if frame::is_minimized(h) {
        let (icon, title) = (frame::icon(h), frame::title_of(h));
        frame::draw_minimized(&mut c, icon, &title, s.active == h);
    } else if s.frames.contains_key(&h) {
        let f = frame::fcf(h);
        frame::draw_frame(&mut c, f, s.active == h);
    } else {
        let r = c.rect();
        c.fill(r, color::sys(color::SYSCLR_DIALOGBACKGROUND));
    }
    if let Some(w) = st().desk.get_mut(h) {
        w.ghost = w.update.clone();
    }
}

// ---- Processes ----

/// A PM process ended (or let go of PM): remove what it left behind.
pub fn process_gone(p: u32) {
    let s = st();
    s.procs.retain(|&x| x != p);
    let queues: Vec<u32> = s.queues.iter().filter(|(_, q)| q.pid == p).map(|(&h, _)| h).collect();
    for q in &queues {
        for h in win::top_windows_of(*q) {
            win::remove(h);
        }
        if let Some(mut x) = st().queues.remove(q) {
            msg::abandon_sent(&mut x);
        }
    }
    let s = st();
    s.timers.retain(|t| !queues.contains(&t.queue));
    // Windows refer to their class by index: an ended process's classes
    // stay as unnamed tombstones.
    for c in s.classes.iter_mut().filter(|c| c.pid == p) {
        c.name.clear();
        c.proc_ = 0;
        c.pid = u32::MAX;
    }
    for slot in s.pss.iter_mut() {
        if slot.as_ref().is_some_and(|e| e.pid == p) {
            *slot = None;
        }
    }
    s.objects.remove(&p);
    pointer::process_gone(p);
    let s = st();
    for slot in s.accels.iter_mut() {
        if slot.as_ref().is_some_and(|a| a.pid == p) {
            *slot = None;
        }
    }
    s.hooks.retain(|h| h.pid != p);
    for slot in s.atoms.iter_mut().skip(1) {
        if slot.as_ref().is_some_and(|t| t.pid == p) {
            *slot = None;
        }
    }
    crate::clip::process_gone(p, &queues);
    crate::viowin::process_gone(p);
    let s = st();
    if s.server_pid == p {
        s.server_pid = 0;
        s.server_queue = 0;
    }
    if s.active == 0 {
        win::activate_next();
    }
}

// ---- The desktop ----

wndproc! {
    desktop_entry => desktop_proc;
    wl_entry => wl_proc;
}

extern "C" fn desktop_proc(h: u32, m: u32, mp1: u32, _mp2: u32) -> u32 {
    match m {
        WM_PAINT => {
            win::paint_desktop();
            paint_ghosts();
            0
        }
        WM_TIMER if mp1 == TID_CHECK => {
            paint_ghosts();
            0
        }
        WM_RS2_GHOSTS => {
            paint_ghosts();
            0
        }
        WM_RS2_WINLIST => {
            open_window_list();
            0
        }
        _ => {
            let _ = h;
            0
        }
    }
}

// ---- The window list ----

/// What a line of the window list stands for.
#[derive(Clone, Copy)]
enum Entry {
    Frame(u32),
    /// A text session on the full screen.
    Session(u32),
    /// Start a command prompt in a window (true) or full screen.
    Prompt(bool),
}

/// The window list's lines (server process only).
static mut LISTED: Vec<Entry> = Vec::new();
static LIST_FRAME: AtomicU32 = AtomicU32::new(0);

const ID_LIST: u32 = 1;
const ID_SWITCH: u32 = 2;
const ID_END: u32 = 3;
const ID_CANCEL: u32 = 4;

/// Ctrl+Esc (from the input thread): open the window list, or close it.
pub fn window_list() {
    let q = st().server_queue;
    post_queue(q, qmsg(HWND_DESKTOP, WM_RS2_WINLIST, 0, 0));
}

fn listed() -> &'static mut Vec<Entry> {
    unsafe { &mut *core::ptr::addr_of_mut!(LISTED) }
}

fn open_window_list() {
    let old = LIST_FRAME.swap(0, Ordering::Relaxed);
    if old != 0 && st().desk.exists(old) {
        win::destroy(old);
        return;
    }
    let s = st();
    let me = pid();
    let (sw, sh) = (s.surface.width, s.surface.height);
    let frames: Vec<u32> = s
        .desk
        .get(HWND_DESKTOP)
        .map_or(Vec::new(), |w| w.children.clone())
        .into_iter()
        .rev()
        .filter(|&f| s.frames.contains_key(&f) && s.queues.get(&s.queue_of(f)).is_some_and(|q| q.pid != me))
        .filter(|&f| s.desk.get(f).is_some_and(|w| !w.text.is_empty() || WinWindowFromID(f, FID_TITLEBAR) != 0))
        .collect();
    // A VIO session on the full screen is listed as a session, not as its
    // hidden window.
    let fullscreen = crate::viowin::fullscreen_frames();
    let frames: Vec<u32> = frames.into_iter().filter(|f| !fullscreen.contains(f)).collect();
    let mut sessions = [VioSession { sid: 0, flags: 0, changes: 0, leader: 0, title: [0; 64] }; 32];
    let mut found = 0;
    unsafe { Rs2VioQuerySessions(sessions.as_mut_ptr(), 32, &mut found, 1) };
    let sessions: Vec<VioSession> = sessions.into_iter().take(found.min(32) as usize).filter(|v| v.flags & (VS_WINDOWED | VS_PM) == 0).collect();
    // FCF_TITLEBAR | FCF_SYSMENU | FCF_DLGBORDER.
    let mut fcf: u32 = 0x0001 | 0x0002 | 0x0100;
    let mut client = 0;
    let frame = win::WinCreateStdWindow(HWND_DESKTOP, 0, &mut fcf, b"RS2WindowList\0".as_ptr(), b"Window List\0".as_ptr(), 0, 0, 0, &mut client);
    if frame == 0 {
        return;
    }
    let (w, h) = (360, 260);
    WinSetWindowPos(frame, 0, (sw - w) / 2, (sh - h) / 2, w, h, 0x0003);
    let (cw, ch) = st().desk.get(client).map_or((0, 0), |c| (c.cx, c.cy));
    let lb = win::WinCreateWindow(client, 0xffff_0007 as *const u8, b"\0".as_ptr(), 0x8000_0000, 8, 40, cw - 16, ch - 48, client, 0, ID_LIST, 0, 0);
    let bw = (cw - 32) / 3;
    for (i, (id, t)) in [(ID_SWITCH, &b"~Switch to\0"[..]), (ID_END, &b"~End task\0"[..]), (ID_CANCEL, &b"Cancel\0"[..])].into_iter().enumerate() {
        let style = 0x8000_0000 | 0x0002_0000 | if id == ID_SWITCH { 0x0400 } else { 0 };
        win::WinCreateWindow(client, 0xffff_0003 as *const u8, t.as_ptr(), style, 8 + i as i32 * (bw + 8), 8, bw, 24, client, 0, id, 0, 0);
    }
    let l = listed();
    l.clear();
    for f in frames {
        let hung = st().is_hung(f);
        let mut t = frame_title(f);
        if hung {
            t.extend_from_slice(b" (not responding)");
        }
        t.push(0);
        msg::send(lb, 0x0161, (-1i32) as u32, t.as_ptr() as u32);
        l.push(Entry::Frame(f));
    }
    for v in sessions {
        let n = v.title.iter().position(|&b| b == 0).unwrap_or(64);
        let mut t = v.title[..n].to_vec();
        if !t.windows(11).any(|w| w.eq_ignore_ascii_case(b"full screen")) {
            t.extend_from_slice(b" (full screen)");
        }
        t.push(0);
        msg::send(lb, 0x0161, (-1i32) as u32, t.as_ptr() as u32);
        l.push(Entry::Session(v.sid));
    }
    for (t, win) in [(&b"Command prompt (window)\0"[..], true), (&b"Command prompt (full screen)\0"[..], false)] {
        msg::send(lb, 0x0161, (-1i32) as u32, t.as_ptr() as u32);
        l.push(Entry::Prompt(win));
    }
    if !l.is_empty() {
        msg::send(lb, 0x0164, 0, 1);
    }
    win::show(frame, true);
    win::activate(frame);
    win::WinSetFocus(HWND_DESKTOP, lb);
    LIST_FRAME.store(frame, Ordering::Relaxed);
}

fn frame_title(f: u32) -> Vec<u8> {
    let tb = WinWindowFromID(f, FID_TITLEBAR);
    let t = ctl::text(if tb != 0 { tb } else { f });
    if t.is_empty() { b"(untitled)".to_vec() } else { t }
}

fn close_window_list() {
    let f = LIST_FRAME.swap(0, Ordering::Relaxed);
    if f != 0 {
        win::destroy(f);
    }
}

/// The line chosen in the list.
fn chosen(client: u32) -> Option<Entry> {
    let lb = WinWindowFromID(client, ID_LIST);
    let i = msg::send(lb, 0x0165, 0xffff_ffff, 0);
    listed().get(i as usize).copied().filter(|e| !matches!(e, Entry::Frame(f) if !st().desk.exists(*f)))
}

fn switch_to(e: Option<Entry>) {
    match e {
        Some(Entry::Frame(f)) => {
            win::show(f, true);
            if frame::is_minimized(f) {
                frame::restore(f);
            }
            win::activate(f);
        }
        Some(Entry::Session(sid)) => {
            lock::unlocked(|| unsafe { Rs2VioControl(sid, 4, 0) });
        }
        Some(Entry::Prompt(window)) => start_prompt(window),
        None => {}
    }
}

/// A command prompt (%COMSPEC%) in a new session: in a window or full
/// screen.
fn start_prompt(window: bool) {
    let title: &[u8] = if window { b"OS/2 Window\0" } else { b"OS/2 Full Screen\0" };
    let sd = StartData {
        length: core::mem::size_of::<StartData>() as u16,
        related: 0,
        fgbg: 0,
        trace: 0,
        title: title.as_ptr(),
        name: core::ptr::null(),
        inputs: b"\0".as_ptr(),
        termq: core::ptr::null(),
        env: core::ptr::null(),
        inherit: 0,
        session_type: if window { 2 } else { 1 },
    };
    let (mut sid, mut pid) = (0, 0);
    lock::unlocked(|| unsafe { DosStartSession(&sd, &mut sid, &mut pid) });
}

/// End the program of a line: a close request to a frame that responds,
/// the end of the process for one that does not; a full-screen session's
/// processes end.
fn end_task(e: Option<Entry>) {
    let f = match e {
        Some(Entry::Frame(f)) => f,
        Some(Entry::Session(sid)) => {
            lock::unlocked(|| unsafe { Rs2VioControl(sid, 3, 0) });
            return;
        }
        _ => return,
    };
    let s = st();
    let q = s.queue_of(f);
    let Some(x) = s.queues.get(&q) else { return };
    if x.hung {
        let p = x.pid;
        lock::unlocked(|| unsafe { DosKillProcess(0, p) });
    } else {
        msg::post(f, WM_SYSCOMMAND, frame::SC_CLOSE, 0);
    }
}

extern "C" fn wl_proc(h: u32, m: u32, mp1: u32, mp2: u32) -> u32 {
    match m {
        WM_PAINT => {
            crate::draw::paint(h, |c| {
                let r = c.rect();
                c.fill(r, crate::draw::dialog_bg());
            });
            0
        }
        ctl::WM_COMMAND => {
            let f = chosen(h);
            close_window_list();
            match ctl::lo(mp1) {
                ID_SWITCH => switch_to(f),
                ID_END => end_task(f),
                _ => {}
            }
            0
        }
        ctl::WM_CONTROL if ctl::lo(mp1) == ID_LIST && ctl::hi(mp1) == 5 => {
            // LN_ENTER
            let f = chosen(h);
            close_window_list();
            switch_to(f);
            0
        }
        WM_CHAR => {
            let k = ctl::Key::from(mp1, mp2);
            if k.up() {
                return 0;
            }
            match k.vk {
                ctl::VK_ESC => close_window_list(),
                ctl::VK_DELETE => {
                    let f = chosen(h);
                    close_window_list();
                    end_task(f);
                }
                ctl::VK_NEWLINE | ctl::VK_ENTER => {
                    let f = chosen(h);
                    close_window_list();
                    switch_to(f);
                }
                _ => {}
            }
            1
        }
        WM_CLOSE => {
            close_window_list();
            0
        }
        _ => msg::WinDefWindowProc(h, m, mp1, mp2),
    }
}
