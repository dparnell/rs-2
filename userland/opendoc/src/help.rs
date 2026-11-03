//! ODHelp (OS/2) and the status line extension.
//!
//! IBM's parts (MMODPTS's DisplayHelp) ask the session for its ODHelp
//! (ODSession::GetHelp) and call DisplayHelp(file, panel) for a panel of
//! their help library (MMPARTS.HLP) or DisplayHelpIndex(file) for its
//! index (Help menu 0x12: 0x5022 Help index, 0x5023 General help). rs2's
//! ODHelp keeps one help manager instance per library (HELPMGR's
//! WinCreateHelpInstance, the library found on HELP), associated with
//! the active window, and sends it HM_DISPLAY_HELP / HM_HELP_INDEX.
//!
//! The status line: a part asks the session for its
//! "OpenDoc:Extension:StatusLine" (HasExtension/AcquireExtension, as MMODPTS does when it
//! initializes) and calls SetStatusLineText(text, frame), e.g. with the
//! hint for the menu item under the pointer (WM_MENUSELECT), and with an
//! empty text when the "Statusline" focus leaves it. The text shows in a
//! line at the bottom of the frame's document window, hidden while empty.

use alloc::vec::Vec;

use crate::rt::*;
use crate::services::{sess, win, wstate};
use crate::*;

unsafe extern "C" {
    #[link_name = "HELPMGR.51"]
    fn WinCreateHelpInstance(hab: u32, init: *mut u32) -> u32;
    #[link_name = "HELPMGR.54"]
    fn WinAssociateHelpInstance(help: u32, app: u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: usize, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32,
                       owner: u32, behind: u32, id: u32, ctl: usize, pres: usize) -> u32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize;
    #[link_name = "PMWIN.875"]
    fn WinSetWindowPos(hwnd: u32, behind: u32, x: i32, y: i32, cx: i32, cy: i32, fl: u32) -> u32;
    #[link_name = "PMWIN.840"]
    fn WinQueryWindowRect(hwnd: u32, rect: *mut [i32; 4]) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(hwnd: u32, id: u32) -> u32;
    #[link_name = "PMWIN.877"]
    fn WinSetWindowText(hwnd: u32, text: *const u8) -> u32;
    #[link_name = "PMWIN.883"]
    fn WinShowWindow(hwnd: u32, show: u32) -> u32;
    #[link_name = "PMWIN.799"]
    fn WinQueryActiveWindow(desktop: u32) -> u32;
    #[link_name = "PMWIN.800"]
    fn WinQueryAnchorBlock(hwnd: u32) -> u32;
    #[link_name = "PMWIN.841"]
    fn WinQueryWindowText(hwnd: u32, cb: i32, buf: *mut u8) -> i32;
}

const WC_STATIC: usize = 0xffff_0005;
const WS_VISIBLE: u32 = 0x8000_0000;
const SS_TEXT: u32 = 0x0001;
const DT_VCENTER: u32 = 0x0400;
const HM_DISPLAY_HELP: u32 = 0x222;
const HM_HELP_INDEX: u32 = 0x22a;
const HM_RESOURCEID: usize = 0;
const SWP_SIZE: u32 = 0x1;
const SWP_MOVE: u32 = 0x2;
const SWP_ZORDER: u32 = 0x4;
const HWND_TOP: u32 = 3;

/// The status line's window ID in a document window's client.
pub const STATUS_ID: u32 = 0x8101;
const STATUS_CY: i32 = 20;

/// IBM's name for it (MMODPTS's); the spelling with blanks is taken too.
pub const NAME_STATUS_LINE: &[u8] = b"OpenDoc:Extension:StatusLine";

// ---- ODHelp ----

static mut INSTANCES: Vec<(Vec<u8>, u32)> = Vec::new();

fn upper(s: &[u8]) -> Vec<u8> {
    s.iter().map(|b| b.to_ascii_uppercase()).collect()
}

/// The help instance for a library, created the first time.
fn instance(file: &[u8]) -> u32 {
    let key = upper(file);
    let list = unsafe { &mut *(&raw mut INSTANCES) };
    if let Some(&(_, h)) = list.iter().find(|(f, _)| *f == key) {
        return h;
    }
    let lib = z(file);
    let title = b"Help\0";
    // HELPINIT: cb, rc, tutorial, help table, its module, accelerators,
    // their ID, action bar, title, show panel IDs, library names.
    let mut init: [u32; 11] = [44, 0, 0, 0, 0, 0, 0, 0, title.as_ptr() as u32, 0, lib.as_ptr() as u32];
    let active = unsafe { WinQueryActiveWindow(1) };
    let hab = unsafe { WinQueryAnchorBlock(active) };
    let h = unsafe { WinCreateHelpInstance(hab, init.as_mut_ptr()) };
    if h != 0 && init[1] == 0 {
        list.push((key, h));
        h
    } else {
        0
    }
}

fn show(file: usize, msg: u32, mp1: usize, mp2: usize) -> usize {
    let f = cstr(file);
    if f.is_empty() {
        return 0;
    }
    let h = instance(f);
    if h == 0 {
        return 0;
    }
    let active = unsafe { WinQueryActiveWindow(1) };
    if active != 0 {
        unsafe { WinAssociateHelpInstance(h, active) };
    }
    (unsafe { WinSendMsg(h, msg, mp1, mp2) } == 0) as usize
}

methods! {
    // DisplayHelp(file, panel) (index 1): the panel (a resource ID).
    fn DisplayHelp(this: usize, ev: usize, file: usize, panel: usize) -> usize {
        show(file, HM_DISPLAY_HELP, panel & 0xffff, HM_RESOURCEID)
    }
    // DisplayHelpIndex(file) (index 2).
    fn DisplayHelpIndex(this: usize, ev: usize, file: usize) -> usize {
        show(file, HM_HELP_INDEX, 0, 0)
    }
    // DisplayHelpUsingHelp(file) (index 3): the help manager's own help.
    fn DisplayHelpUsingHelp(this: usize, ev: usize, file: usize) -> usize {
        show(file, HM_DISPLAY_HELP, 0, 0)
    }
}

// ---- The status line ----

/// The document client a frame shows in (the active document window's
/// when the frame is 0).
fn client_of(session: usize, frame: usize) -> u32 {
    let w = crate::dnd::window_of_frame(frame);
    if w != 0 {
        return win(w).client;
    }
    if session == 0 {
        return 0;
    }
    let ws = sess(session).window_state;
    if ws == 0 {
        return 0;
    }
    let active = unsafe { WinQueryActiveWindow(1) };
    let wins = &wstate(ws).windows;
    wins.iter().find(|&&w| win(w).hwnd == active).or(wins.first()).map_or(0, |&w| win(w).client)
}

/// Show `text` in the status line of a document client (hidden when empty).
pub fn set_status(client: u32, text: &[u8]) {
    if client == 0 {
        return;
    }
    let mut line = unsafe { WinWindowFromID(client, STATUS_ID) };
    if line == 0 {
        if text.is_empty() {
            return;
        }
        line = unsafe {
            WinCreateWindow(client, WC_STATIC, b"\0".as_ptr(), SS_TEXT | DT_VCENTER, 0, 0, 10, STATUS_CY,
                            client, HWND_TOP, STATUS_ID, 0, 0)
        };
        if line == 0 {
            return;
        }
        resized(client);
    }
    let mut t = Vec::with_capacity(text.len() + 2);
    t.push(b' ');
    t.extend_from_slice(text);
    t.push(0);
    unsafe {
        WinSetWindowText(line, t.as_ptr());
        WinShowWindow(line, !text.is_empty() as u32);
    }
}

/// The status line's text (without the leading blank), for tests.
pub fn status_text(client: u32) -> Vec<u8> {
    let line = unsafe { WinWindowFromID(client, STATUS_ID) };
    if line == 0 {
        return Vec::new();
    }
    let mut b = [0u8; 256];
    let n = unsafe { WinQueryWindowText(line, b.len() as i32, b.as_mut_ptr()) }.max(0) as usize;
    let s = &b[..n.min(b.len())];
    s.strip_prefix(b" ").unwrap_or(s).to_vec()
}

/// Keep the status line across the bottom of its client.
pub fn resized(client: u32) {
    let line = unsafe { WinWindowFromID(client, STATUS_ID) };
    if line == 0 {
        return;
    }
    let mut r = [0i32; 4];
    unsafe {
        WinQueryWindowRect(client, &mut r);
        WinSetWindowPos(line, HWND_TOP, 0, 0, r[2], STATUS_CY, SWP_MOVE | SWP_SIZE | SWP_ZORDER);
    }
}

fn ext_base(ext: usize) -> usize {
    *somsup::data::<usize>(cd!(ODExtensionCClassData), ext)
}

methods! {
    // SetStatusLineText(text, frame) (index 0).
    fn SetStatusLineText(this: usize, ev: usize, text: usize, frame: usize) -> usize {
        set_status(client_of(ext_base(this), frame), cstr(text));
        0
    }
    // ClearStatusLineText(frame) (index 1).
    fn ClearStatusLineText(this: usize, ev: usize, frame: usize) -> usize {
        set_status(client_of(ext_base(this), frame), b"");
        0
    }
}

/// The session's extensions: the status line.
pub fn session_has_extension(name: usize) -> bool {
    let n = cstr(name);
    n == NAME_STATUS_LINE || n == b"OpenDoc Extension StatusLine"
}

pub fn session_acquire_extension(session: usize, ev: usize, name: usize) -> usize {
    if !session_has_extension(name) {
        return 0;
    }
    let s = sess(session);
    if s.status_line == 0 {
        let e = crate::storage::new_of(cd!(ODStatusLineExtensionClassData), "ODStatusLineExtension");
        if e != 0 {
            call(cd!(ODStatusLineExtensionClassData), 2, &[e, ev, session]);
        }
        s.status_line = e;
    }
    crate::obj::acquire(s.status_line, ev)
}
