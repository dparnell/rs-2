//! Sessions (screen groups).
//!
//! A session is a set of processes sharing a virtual text screen and a
//! keyboard queue. Session 1 is the boot session; DosStartSession creates
//! more. One session is in the foreground: it is shown on the framebuffer
//! and receives keystrokes. Ctrl+Esc opens the window list, Alt+Esc
//! switches to the next session.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kbd::KeyInfo;
use crate::sync::SpinLock;

pub struct Session {
    pub id: u32,
    pub title: String,
    /// Process that leads the session; the session ends with it.
    pub leader: u32,
    /// Session that started this one (focus returns there).
    pub parent_session: u32,
    /// For related (child) sessions: the starting process.
    pub parent_pid: Option<u32>,
    /// Termination queue name (related sessions).
    pub termq: Option<String>,
    /// A windowable VIO session (SSF_TYPE_WINDOWABLEVIO): shown in a PM
    /// window or, after Alt+Home, on the full screen.
    pub windowable: bool,
    /// Shown in its PM window now (not on the framebuffer).
    pub windowed: bool,
}

/// The Presentation Manager session, if one has been started.
static PM: AtomicU32 = AtomicU32::new(0);

pub fn pm_session() -> u32 {
    PM.load(Ordering::Relaxed)
}

/// The Presentation Manager session, created on first use: a session whose
/// screen is drawn by PM itself.
pub fn pm_session_or_create() -> u32 {
    let s = pm_session();
    if s != 0 {
        return s;
    }
    let sid = create(String::from("Presentation Manager"), foreground(), None, None);
    crate::screen::with_session(sid, |s| s.graphics = true);
    PM.store(sid, Ordering::Relaxed);
    sid
}

static SESSIONS: SpinLock<Vec<Session>> = SpinLock::new(Vec::new());
static FOREGROUND: AtomicU32 = AtomicU32::new(1);
static NEXT_ID: AtomicU32 = AtomicU32::new(2);

/// The session of the calling thread (1 for kernel threads).
pub fn current() -> u32 {
    crate::task::sched::current_opt().and_then(|t| t.process()).map_or(1, |p| p.session.load(Ordering::Relaxed))
}

pub fn foreground() -> u32 {
    FOREGROUND.load(Ordering::Relaxed)
}

// ---- Pop-ups (VioPopUp, hard errors) ----

/// Screen and keyboard queue id of the pop-up.
pub const POPUP_SID: u32 = 0xffff;

struct Popup {
    /// Process that owns it (its VIO and KBD calls go to the pop-up).
    owner: u32,
}

static POPUP: SpinLock<Option<Popup>> = SpinLock::new(None);
static POPUP_WAIT: crate::task::WaitQueue = crate::task::WaitQueue::new();

fn current_pid() -> u32 {
    crate::task::sched::current_opt().and_then(|t| t.process()).map_or(0, |p| p.pid)
}

/// The session whose screen and keyboard the calling thread uses: its own,
/// or the pop-up while its process owns one.
pub fn io_session() -> u32 {
    let pid = current_pid();
    if pid != 0 && POPUP.lock().as_ref().is_some_and(|p| p.owner == pid) {
        return POPUP_SID;
    }
    current()
}

/// The session that receives keystrokes: the pop-up while one is shown,
/// else the foreground session.
pub fn input_session() -> u32 {
    if POPUP.lock().is_some() { POPUP_SID } else { foreground() }
}

pub fn popup_active() -> bool {
    POPUP.lock().is_some()
}

/// Whether process `pid` owns the pop-up.
pub fn popup_owner(pid: u32) -> bool {
    POPUP.lock().as_ref().is_some_and(|p| p.owner == pid)
}

/// Show a pop-up screen for the calling process over whatever is in the
/// foreground. Opaque pop-ups are a cleared 80x25 screen; transparent ones
/// start as a copy of the screen. Fails with ERROR_VIO_EXISTING_POPUP if
/// another pop-up is up and `wait` is false.
pub fn popup_begin(wait: bool, transparent: bool) -> Result<(), u32> {
    let pid = current_pid();
    loop {
        {
            let mut g = POPUP.lock();
            match g.as_ref() {
                None => {
                    *g = Some(Popup { owner: pid });
                    break;
                }
                Some(p) if p.owner == pid => return Err(406),
                Some(_) if !wait => return Err(406),
                Some(_) => {}
            }
        }
        if !POPUP_WAIT.wait_until(None, || POPUP.lock().is_none()) {
            return Err(crate::api::err::INTERRUPT);
        }
    }
    close_list();
    let fg = foreground();
    crate::api::vio::notify_popup(fg, true);
    crate::screen::create(POPUP_SID);
    if transparent {
        let mut g = crate::screen::SCREEN.lock();
        let src: Option<*const crate::screen::Screen> = g.get(fg).map(|s| s as *const _);
        if let (Some(src), Some(dst)) = (src, g.get(POPUP_SID)) {
            // Distinct screens: the pop-up was just created.
            dst.copy_from(unsafe { &*src });
        }
    }
    crate::kbd::drop_queue(POPUP_SID);
    crate::screen::show(POPUP_SID);
    crate::screen::with_session(POPUP_SID, |s| {
        let pos = (s.cur_row, s.cur_col);
        s.set_cursor(Some(pos));
    });
    Ok(())
}

/// End the calling process's pop-up (ERROR_VIO_NO_POPUP if it has none).
pub fn popup_end() -> Result<(), u32> {
    end_popup_of(current_pid())
}

fn end_popup_of(pid: u32) -> Result<(), u32> {
    {
        let mut g = POPUP.lock();
        if !g.as_ref().is_some_and(|p| p.owner == pid) {
            return Err(405);
        }
        *g = None;
    }
    let fg = foreground();
    crate::screen::show(fg);
    if fg == pm_session() && fg != 0 {
        crate::display::post_repaint();
    }
    crate::screen::destroy(POPUP_SID);
    crate::kbd::drop_queue(POPUP_SID);
    crate::console::drop_session(POPUP_SID);
    crate::api::vio::notify_popup(fg, false);
    POPUP_WAIT.wake_all();
    Ok(())
}

/// Create a session with its own (hidden) screen and keyboard queue.
pub fn create(title: String, parent_session: u32, parent_pid: Option<u32>, termq: Option<String>) -> u32 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    crate::screen::create(id);
    SESSIONS.lock().push(Session { id, title, leader: 0, parent_session, parent_pid, termq, windowable: false, windowed: false });
    id
}

/// Make `sid` a windowable VIO session, shown in its window.
pub fn make_windowed(sid: u32) {
    if let Some(s) = SESSIONS.lock().iter_mut().find(|s| s.id == sid) {
        s.windowable = true;
        s.windowed = true;
    }
    crate::screen::with_session(sid, |s| s.set_windowed(true, true));
    crate::viowin::changed();
}

/// Whether `sid` is shown in a PM window now.
pub fn is_windowed(sid: u32) -> bool {
    sid != 1 && SESSIONS.lock().iter().any(|s| s.id == sid && s.windowed)
}

pub fn is_windowable(sid: u32) -> bool {
    sid != 1 && SESSIONS.lock().iter().any(|s| s.id == sid && s.windowable)
}

/// Switch a windowable session between its window (`on`) and the full
/// screen (Alt+Home). False if `sid` is not windowable.
pub fn set_windowed(sid: u32, on: bool) -> bool {
    {
        let mut g = SESSIONS.lock();
        let Some(s) = g.iter_mut().find(|s| s.id == sid && s.windowable) else { return false };
        if s.windowed == on {
            drop(g);
            if on {
                crate::viowin::activate(sid);
            }
            return true;
        }
        s.windowed = on;
    }
    crate::screen::with_session(sid, |s| s.set_windowed(true, on));
    crate::viowin::changed();
    if on {
        // Back into the window: PM comes to the front with it.
        crate::viowin::activate(sid);
    } else {
        select(sid);
    }
    true
}

/// Leader and title of a session.
pub fn info(sid: u32) -> Option<(u32, String)> {
    SESSIONS.lock().iter().find(|s| s.id == sid).map(|s| (s.leader, s.title.clone()))
}

/// (id, title, windowable, windowed) of the sessions other than the boot
/// session.
pub fn list_full() -> Vec<(u32, String, bool, bool)> {
    SESSIONS.lock().iter().map(|s| (s.id, s.title.clone(), s.windowable, s.windowed)).collect()
}

pub fn set_leader(sid: u32, pid: u32) {
    if let Some(s) = SESSIONS.lock().iter_mut().find(|s| s.id == sid) {
        s.leader = pid;
    }
}

/// Undo `create` when the program could not be started.
pub fn discard(sid: u32) {
    SESSIONS.lock().retain(|s| s.id != sid);
    crate::screen::destroy(sid);
    crate::kbd::drop_queue(sid);
}

pub fn is_leader(pid: u32) -> bool {
    SESSIONS.lock().iter().any(|s| s.leader == pid)
}

pub fn exists(sid: u32) -> bool {
    sid == 1 || SESSIONS.lock().iter().any(|s| s.id == sid)
}

/// (id, title) of every session, the boot session first.
pub fn list() -> Vec<(u32, String)> {
    let mut v = alloc::vec![(1, boot_title())];
    v.extend(SESSIONS.lock().iter().map(|s| (s.id, s.title.clone())));
    v
}

fn boot_title() -> String {
    String::from("OS/2 Window")
}

/// Sessions started as children of `pid`.
pub fn children_of(pid: u32) -> Vec<u32> {
    SESSIONS.lock().iter().filter(|s| s.parent_pid == Some(pid)).map(|s| s.id).collect()
}

pub fn parent_pid(sid: u32) -> Option<u32> {
    SESSIONS.lock().iter().find(|s| s.id == sid).and_then(|s| s.parent_pid)
}

/// Bring session `sid` to the foreground. A windowed session's window
/// comes to the front in PM.
pub fn select(sid: u32) -> bool {
    if !exists(sid) {
        return false;
    }
    if is_windowed(sid) {
        close_list();
        crate::viowin::activate(sid);
        return true;
    }
    close_list();
    let old = FOREGROUND.swap(sid, Ordering::Relaxed);
    if !popup_active() {
        crate::screen::show(sid);
    }
    if old != sid {
        crate::api::vio::notify_switch(old, sid);
    }
    let leader = if sid == 1 { 0 } else { SESSIONS.lock().iter().find(|s| s.id == sid).map_or(0, |s| s.leader) };
    crate::infoseg::set_foreground(sid, leader);
    if sid == pm_session() {
        // PM repaints the screen when it comes back.
        crate::display::post_repaint();
    }
    true
}

/// Alt+Esc: the next session in order.
pub fn select_next() {
    if popup_active() {
        return;
    }
    // Windowed sessions belong to PM's screen.
    let ids: Vec<u32> = list().into_iter().map(|(i, _)| i).filter(|&i| !is_windowed(i)).collect();
    let fg = foreground();
    let at = ids.iter().position(|&i| i == fg).unwrap_or(0);
    select(ids[(at + 1) % ids.len()]);
}

/// A process ended; if it led a session, the session ends.
pub fn process_ended(pid: u32, code: u32) {
    let _ = end_popup_of(pid);
    crate::display::process_ended(pid);
    crate::api::vio::process_ended(pid);
    crate::api::subsys::process_ended(pid);
    crate::api::harderr::process_ended(pid);
    crate::monitor::process_ended(pid);
    crate::viowin::process_ended(pid);
    // The PM session ends with its last process.
    let pm = pm_session();
    if pm != 0 && !crate::task::process::all().iter().any(|p| p.pid != pid && p.exit_info().is_none() && p.session.load(Ordering::Relaxed) == pm) {
        PM.store(0, Ordering::Relaxed);
        let back = SESSIONS.lock().iter().find(|s| s.id == pm).map_or(1, |s| s.parent_session);
        if foreground() == pm {
            select(if exists(back) && back != pm { back } else { 1 });
        }
        discard(pm);
        crate::display::reset_input();
        crate::display::reset_cursor();
    }
    let s = {
        let mut ss = SESSIONS.lock();
        let Some(i) = ss.iter().position(|s| s.leader == pid) else { return };
        ss.remove(i)
    };
    if let Some(q) = &s.termq {
        // Termination notification: session ID and result code.
        let mut data = Vec::new();
        data.extend_from_slice(&(s.id as u16).to_le_bytes());
        data.extend_from_slice(&(code as u16).to_le_bytes());
        crate::api::queue::post_kernel(q, 0, &data);
    }
    if foreground() == s.id {
        let back = if exists(s.parent_session) { s.parent_session } else { 1 };
        select(back);
    }
    if LIST.lock().is_some() {
        close_list();
    }
    crate::screen::destroy(s.id);
    crate::kbd::drop_queue(s.id);
    crate::console::drop_session(s.id);
    crate::mouse::drop_session(s.id);
    crate::conkbd::drop_session(s.id);
    crate::api::vio::drop_session(s.id);
    if s.windowable {
        crate::viowin::session_ended(s.id);
    }
}

// ---- The window list (Ctrl+Esc) ----

struct WindowList {
    sel: usize,
    entries: Vec<(u32, String)>,
    /// Session the list is drawn on, and the cells it covers.
    on: u32,
    top: usize,
    left: usize,
    saved: Vec<Vec<u16>>,
}

static LIST: SpinLock<Option<WindowList>> = SpinLock::new(None);

const LIST_WIDTH: usize = 40;

/// Ctrl+Esc: open the window list, or close it if open.
pub fn toggle_list() {
    if popup_active() {
        return;
    }
    if LIST.lock().is_some() {
        close_list();
        return;
    }
    let entries = list();
    let on = foreground();
    let sel = entries.iter().position(|(i, _)| *i == on).unwrap_or(0);
    let h = entries.len() + 3;
    let Some((rows, cols)) = crate::screen::with_session(on, |s| (s.rows, s.cols)) else { return };
    let top = rows.saturating_sub(h) / 2;
    let left = cols.saturating_sub(LIST_WIDTH) / 2;
    let saved = crate::screen::with_session(on, |s| {
        (top..(top + h).min(rows)).map(|r| (left..(left + LIST_WIDTH).min(cols)).map(|c| s.cell(r, c)).collect()).collect()
    })
    .unwrap_or_default();
    *LIST.lock() = Some(WindowList { sel, entries, on, top, left, saved });
    draw_list();
}

fn close_list() {
    let Some(l) = LIST.lock().take() else { return };
    crate::screen::with_session(l.on, |s| {
        for (dr, row) in l.saved.iter().enumerate() {
            for (dc, &cell) in row.iter().enumerate() {
                s.put(l.top + dr, l.left + dc, cell);
            }
        }
    });
}

fn draw_list() {
    let g = LIST.lock();
    let Some(l) = g.as_ref() else { return };
    const FRAME: u16 = 0x1f00;
    const SEL: u16 = 0x7000;
    let line = |s: &mut crate::screen::Screen, r: usize, text: &str, attr: u16| {
        let bytes = text.as_bytes();
        for c in 0..LIST_WIDTH {
            let ch = if c == 0 || c == LIST_WIDTH - 1 { b' ' } else { bytes.get(c - 1).copied().unwrap_or(b' ') };
            s.put(l.top + r, l.left + c, attr | ch as u16);
        }
    };
    crate::screen::with_session(l.on, |s| {
        line(s, 0, "            Window List", FRAME);
        for (i, (id, title)) in l.entries.iter().enumerate() {
            let text = alloc::format!(" {:2}  {}", id, title);
            let text: String = text.chars().take(LIST_WIDTH - 3).collect();
            line(s, i + 1, &text, if i == l.sel { SEL } else { FRAME });
        }
        line(s, l.entries.len() + 1, "", FRAME);
        line(s, l.entries.len() + 2, " Enter=switch  Esc=cancel", FRAME);
    });
}

/// Keys while the window list is open. Returns true if consumed.
pub fn list_key(k: &KeyInfo) -> bool {
    let choice = {
        let mut g = LIST.lock();
        let Some(l) = g.as_mut() else { return false };
        match (k.ch, k.scan) {
            (0x1b, _) => Some(None),
            (b'\r', _) => Some(Some(l.entries[l.sel].0)),
            (0 | 0xe0, 0x48) => {
                l.sel = l.sel.checked_sub(1).unwrap_or(l.entries.len() - 1);
                None
            }
            (0 | 0xe0, 0x50) => {
                l.sel = (l.sel + 1) % l.entries.len();
                None
            }
            _ => None,
        }
    };
    match choice {
        Some(Some(sid)) => {
            close_list();
            select(sid);
        }
        Some(None) => close_list(),
        None => draw_list(),
    }
    true
}
