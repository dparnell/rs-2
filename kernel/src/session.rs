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

/// Create a session with its own (hidden) screen and keyboard queue.
pub fn create(title: String, parent_session: u32, parent_pid: Option<u32>, termq: Option<String>) -> u32 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    crate::screen::create(id);
    SESSIONS.lock().push(Session { id, title, leader: 0, parent_session, parent_pid, termq });
    id
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

/// Bring session `sid` to the foreground.
pub fn select(sid: u32) -> bool {
    if !exists(sid) {
        return false;
    }
    close_list();
    FOREGROUND.store(sid, Ordering::Relaxed);
    crate::screen::show(sid);
    let leader = if sid == 1 { 0 } else { SESSIONS.lock().iter().find(|s| s.id == sid).map_or(0, |s| s.leader) };
    crate::infoseg::set_foreground(sid, leader);
    true
}

/// Alt+Esc: the next session in order.
pub fn select_next() {
    let ids: Vec<u32> = list().into_iter().map(|(i, _)| i).collect();
    let fg = foreground();
    let at = ids.iter().position(|&i| i == fg).unwrap_or(0);
    select(ids[(at + 1) % ids.len()]);
}

/// A process ended; if it led a session, the session ends.
pub fn process_ended(pid: u32, code: u32) {
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
