//! PM's own view of itself for the rescue report (Ctrl+Alt+W, S): the
//! message queues (what each thread is doing: waiting for messages, for
//! a reply, in a modal loop or a menu), the capture, focus and active
//! windows, and the top-level windows.
//!
//! RESCUE.EXE posts `\SEM32\RS2\PMSTATE`; the PM server's input thread
//! (which takes PM's lock at least every 100 ms) writes the report to
//! RS2PM.TXT on the boot drive and posts `\SEM32\RS2\PMSTATED`. If a
//! thread keeps PM's lock, no report comes, which RESCUE says.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};

use pmwin::{ws, HWND_DESKTOP};

use crate::state::{now, st, State};

unsafe extern "C" {
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> u32;
    #[link_name = "DOSCALLS.327"]
    fn DosResetEventSem(hev: u32, count: *mut u32) -> u32;
    #[link_name = "DOSCALLS.328"]
    fn DosPostEventSem(hev: u32) -> u32;
}

const REQUEST: &[u8] = b"\\SEM32\\RS2\\PMSTATE\0";
const DONE: &[u8] = b"\\SEM32\\RS2\\PMSTATED\0";

/// The semaphores (RESCUE creates them), and when opening them last
/// failed.
static REQ: AtomicU32 = AtomicU32::new(0);
static ACK: AtomicU32 = AtomicU32::new(0);
static TRIED: AtomicU32 = AtomicU32::new(0);

/// The input thread, with the lock: write the report if RESCUE asked.
pub fn check_request() {
    if REQ.load(Ordering::Relaxed) == 0 {
        let t = now();
        if TRIED.load(Ordering::Relaxed) != 0 && t.wrapping_sub(TRIED.load(Ordering::Relaxed)) < 2000 {
            return;
        }
        TRIED.store(t.max(1), Ordering::Relaxed);
        let (mut r, mut a) = (0, 0);
        if unsafe { DosOpenEventSem(REQUEST.as_ptr(), &mut r) } != 0 || unsafe { DosOpenEventSem(DONE.as_ptr(), &mut a) } != 0 {
            return;
        }
        ACK.store(a, Ordering::Relaxed);
        REQ.store(r, Ordering::Relaxed);
    }
    let mut n = 0;
    if unsafe { DosResetEventSem(REQ.load(Ordering::Relaxed), &mut n) } != 0 || n == 0 {
        return;
    }
    let text = report(st());
    let mut boot = [0u32; 1];
    unsafe { os2::DosQuerySysInfo(5, 5, boot.as_mut_ptr(), 4) };
    let drive = if (1..=26).contains(&boot[0]) { b'A' + boot[0] as u8 - 1 } else { b'C' };
    let mut path = alloc::vec![drive];
    path.extend_from_slice(b":\\RS2PM.TXT");
    crate::gpiseg::write_file(&path, text.as_bytes());
    unsafe { DosPostEventSem(ACK.load(Ordering::Relaxed)) };
}

/// "hwnd Class "title"".
fn name(s: &State, h: u32) -> String {
    if h == 0 {
        return String::from("none");
    }
    if h == HWND_DESKTOP {
        return alloc::format!("{h:x} (the desktop)");
    }
    match s.desk.get(h) {
        Some(w) => {
            let class = s.classes.get(w.class as usize).map_or(String::new(), |c| class_name(&c.name));
            let title: String = w.text.iter().take(40).map(|&c| if c < 0x20 { ' ' } else { c as char }).collect();
            alloc::format!("{h:x} {class} \"{title}\"")
        }
        None => alloc::format!("{h:x} (gone)"),
    }
}

/// A class name, the public classes ("#1") by their WC_ names.
fn class_name(n: &[u8]) -> String {
    const PUBLIC: &[(&str, &str)] = &[
        ("#1", "WC_FRAME"), ("#2", "WC_COMBOBOX"), ("#3", "WC_BUTTON"), ("#4", "WC_MENU"), ("#5", "WC_STATIC"),
        ("#6", "WC_ENTRYFIELD"), ("#7", "WC_LISTBOX"), ("#8", "WC_SCROLLBAR"), ("#9", "WC_TITLEBAR"), ("#10", "WC_MLE"),
        ("#32", "WC_SPINBUTTON"), ("#37", "WC_CONTAINER"), ("#38", "WC_SLIDER"), ("#39", "WC_VALUESET"), ("#40", "WC_NOTEBOOK"),
    ];
    let n = String::from_utf8_lossy(n).into_owned();
    PUBLIC.iter().find(|(k, _)| *k == n).map_or(n, |(_, v)| String::from(*v))
}

fn report(s: &State) -> String {
    let mut r = String::new();
    let _ = write!(r, "---- PM ----\r\n");
    let shown = s.surface.pixels() as usize == s.screen;
    let _ = write!(
        r,
        "capture {}\r\nfocus {}\r\nactive {}\r\npointer {},{} buttons {:x}; drawing {} (held since {:?}, threads drawing {:?}); lock taken over from dead threads {} times\r\n",
        name(s, s.capture),
        name(s, s.focus),
        name(s, s.active),
        s.pointer.x,
        s.pointer.y,
        s.buttons,
        if shown { "on the screen" } else { "in the shadow buffer (PM in the background)" },
        s.damage_held,
        s.drawing,
        crate::lock::RECOVERED.load(Ordering::Relaxed)
    );
    let _ = write!(r, "QUEUE PID TID: STATE\r\n");
    for (&h, q) in &s.queues {
        let mut state: Vec<String> = Vec::new();
        if h == s.server_queue {
            state.push(String::from("PM server"));
        }
        if q.hung {
            state.push(String::from("NOT RESPONDING"));
        }
        if q.msgs.waiting() {
            state.push(String::from("waiting for messages"));
        }
        if let Some((w, m)) = q.awaiting {
            let to = s.queue_of(w);
            let who = s.queues.get(&to).map_or(String::from("?"), |x| alloc::format!("{} {}", x.pid, x.tid));
            state.push(alloc::format!("WAITING FOR THE REPLY to message {m:x} sent to {} (queue {to:x}, pid/tid {who})", name(s, w)));
        }
        for &(what, w) in &q.modal {
            state.push(alloc::format!("in {what} on {}", name(s, w)));
        }
        if q.menu_mode.is_some() {
            state.push(String::from("tracking a menu"));
        }
        let _ = write!(r, "{h:x} {} {}: {} posted, {} sent waiting; {}\r\n", q.pid, q.tid, q.msgs.len(), q.sent.len(), state.join("; "));
    }
    let _ = write!(r, "TOP-LEVEL WINDOWS (top first): HWND CLASS \"TITLE\" X,Y CXxCY FLAGS QUEUE OWNER\r\n");
    let tops: Vec<u32> = s.desk.get(HWND_DESKTOP).map_or(Vec::new(), |d| d.children.iter().rev().copied().collect());
    for h in tops {
        let Some(w) = s.desk.get(h) else { continue };
        let mut fl = String::new();
        fl.push(if w.style & ws::VISIBLE != 0 { 'V' } else { '-' });
        fl.push(if w.style & ws::DISABLED != 0 { 'D' } else { '-' });
        fl.push(if w.style & ws::MINIMIZED != 0 { 'm' } else { '-' });
        fl.push(if w.style & ws::MAXIMIZED != 0 { 'M' } else { '-' });
        fl.push(if w.update.is_empty() { '-' } else { 'U' });
        let _ = write!(r, "{} {},{} {}x{} {fl} {:x} {}\r\n", name(s, h), w.x, w.y, w.cx, w.cy, w.queue, name(s, w.owner));
    }
    let _ = write!(r, "(FLAGS: V visible, D disabled, m minimized, M maximized, U needs painting)\r\n");
    r
}
