//! The switch list: the programs the window list (Ctrl+Esc) offers.
//! Programs add their main window with WinAddSwitchEntry (PMSHAPI on
//! OS/2; rs2's PMSHAPI and PMMERGE jump here, to rs2-private PMWIN
//! ordinals), and a frame created with FCF_TASKLIST gets an entry of its
//! own whose title follows the frame's, as IBM's FrameSetSwitchAndTitle
//! does. Entries go when their process ends; a frame's own entry also
//! when the frame is destroyed. The window list shows each entry's title
//! and leaves out SWL_INVISIBLE ones; frames without an entry are listed
//! by their title as before.

#![allow(non_snake_case)]

use alloc::vec::Vec;

use crate::misc::set_error;
use crate::state::{pid, st};

/// MAXNAMEL: the title's length without the NUL.
const MAXNAMEL: usize = 60;
pub const SWL_INVISIBLE: u32 = 0x01;
const SWL_VISIBLE: u32 = 0x04;

const PMERR_INVALID_SWITCH_HANDLE: u32 = 0x1202;
const PMERR_INVALID_PARM: u32 = 0x1303;

/// SWCNTRL (32-bit).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SwCntrl {
    pub hwnd: u32,
    pub hwnd_icon: u32,
    pub hprog: u32,
    pub pid: u32,
    pub sid: u32,
    pub visibility: u32,
    pub jump: u32,
    pub title: [u8; MAXNAMEL + 4],
    pub prog_type: u32,
}

pub struct Switch {
    pub handle: u32,
    pub ctl: SwCntrl,
    /// A frame's own entry (FCF_TASKLIST): its title is the frame's.
    pub frame: bool,
}

#[derive(Default)]
pub struct SwitchList {
    pub entries: Vec<Switch>,
    next: u32,
}

impl SwitchList {
    fn add(&mut self, ctl: SwCntrl, frame: bool) -> u32 {
        self.next = self.next.wrapping_add(1).max(1);
        let handle = self.next;
        self.entries.push(Switch { handle, ctl, frame });
        handle
    }

    pub fn get(&self, h: u32) -> Option<&Switch> {
        self.entries.iter().find(|e| e.handle == h)
    }

    /// The entry of a window, if it has one.
    pub fn of_window(&self, hwnd: u32) -> Option<&Switch> {
        self.entries.iter().find(|e| hwnd != 0 && e.ctl.hwnd == hwnd)
    }

    /// A process has ended.
    pub fn process_gone(&mut self, p: u32) {
        self.entries.retain(|e| e.ctl.pid != p);
    }

    /// A window was destroyed: a frame's own entry goes with it.
    pub fn window_gone(&mut self, hwnd: u32) {
        self.entries.retain(|e| !(e.frame && e.ctl.hwnd == hwnd));
    }
}

/// An entry as programs see it: a frame's own entry has the frame's
/// current title.
pub fn current(e: &Switch) -> SwCntrl {
    let mut c = e.ctl;
    if e.frame {
        let t = crate::frame::title_of(c.hwnd);
        set_title(&mut c, &t);
    }
    c
}

/// The title of an entry, without the NUL.
pub fn title(c: &SwCntrl) -> &[u8] {
    let n = c.title.iter().position(|&b| b == 0).unwrap_or(c.title.len());
    &c.title[..n.min(MAXNAMEL)]
}

fn set_title(c: &mut SwCntrl, t: &[u8]) {
    let n = t.len().min(MAXNAMEL);
    c.title = [0; MAXNAMEL + 4];
    c.title[..n].copy_from_slice(&t[..n]);
}

/// Check and complete an SWCNTRL as IBM's WinAddSwitchEntry does:
/// visibility one of SWL_VISIBLE, SWL_INVISIBLE, SWL_GRAYED (0 is
/// SWL_VISIBLE), a jump flag of at most SWL_JUMPABLE, the caller's
/// process when idProcess is 0, the title cut at MAXNAMEL.
fn checked(p: *const SwCntrl) -> Option<SwCntrl> {
    if p.is_null() {
        set_error(PMERR_INVALID_PARM);
        return None;
    }
    let mut c = unsafe { p.read_unaligned() };
    let v = c.visibility & 0xff;
    if v & 5 == 5 || v & 3 == 3 || v > 6 || c.jump & 0xff > 2 {
        set_error(PMERR_INVALID_PARM);
        return None;
    }
    c.visibility = if v == 0 { SWL_VISIBLE } else { v };
    c.jump &= 0xff;
    if c.pid == 0 {
        c.pid = pid();
    }
    c.title[MAXNAMEL..].fill(0);
    Some(c)
}

/// The entry for a frame created with FCF_TASKLIST.
pub fn add_frame(frame: u32) {
    let c = SwCntrl {
        hwnd: frame,
        hwnd_icon: 0,
        hprog: 0,
        pid: pid(),
        sid: 0,
        visibility: SWL_VISIBLE,
        jump: 2,
        title: [0; MAXNAMEL + 4],
        prog_type: 0,
    };
    st().switch.add(c, true);
}

/// WinAddSwitchEntry(pswctl): the new entry's handle, 0 on errors.
#[unsafe(no_mangle)]
pub extern "C" fn WinAddSwitchEntry(p: *const SwCntrl) -> u32 {
    let Some(c) = checked(p) else { return 0 };
    // The program's entry for its frame replaces the frame's own.
    let l = &mut st().switch;
    l.entries.retain(|e| !(e.frame && c.hwnd != 0 && e.ctl.hwnd == c.hwnd));
    l.add(c, false)
}

/// WinChangeSwitchEntry(hsw, pswctl): 0, or an error code.
#[unsafe(no_mangle)]
pub extern "C" fn WinChangeSwitchEntry(h: u32, p: *const SwCntrl) -> u32 {
    let Some(c) = checked(p) else { return PMERR_INVALID_PARM };
    match st().switch.entries.iter_mut().find(|e| e.handle == h) {
        Some(e) => {
            e.ctl = c;
            // A program that sets the title takes it over from the frame.
            e.frame = false;
            0
        }
        None => {
            set_error(PMERR_INVALID_SWITCH_HANDLE);
            PMERR_INVALID_SWITCH_HANDLE
        }
    }
}

/// WinQuerySwitchEntry(hsw, pswctl): 0, or an error code.
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySwitchEntry(h: u32, p: *mut SwCntrl) -> u32 {
    let Some(e) = st().switch.get(h) else {
        set_error(PMERR_INVALID_SWITCH_HANDLE);
        return PMERR_INVALID_SWITCH_HANDLE;
    };
    if p.is_null() {
        return PMERR_INVALID_PARM;
    }
    unsafe { p.write_unaligned(current(e)) };
    0
}

/// WinRemoveSwitchEntry(hsw): 0, or an error code.
#[unsafe(no_mangle)]
pub extern "C" fn WinRemoveSwitchEntry(h: u32) -> u32 {
    let l = &mut st().switch.entries;
    let n = l.len();
    l.retain(|e| e.handle != h);
    if l.len() == n {
        set_error(PMERR_INVALID_SWITCH_HANDLE);
        return PMERR_INVALID_SWITCH_HANDLE;
    }
    0
}

/// WinQuerySwitchHandle(hwnd, idProcess): the entry of the window (or of
/// the window it belongs to: its parents and owners are tried), or with
/// hwnd 0 the first entry of the process (the caller's for 0).
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySwitchHandle(hwnd: u32, p: u32) -> u32 {
    let s = st();
    if hwnd == 0 {
        let p = if p == 0 { pid() } else { p };
        return s.switch.entries.iter().find(|e| e.ctl.pid == p).map_or(0, |e| e.handle);
    }
    let mut w = hwnd;
    for _ in 0..64 {
        if let Some(e) = s.switch.of_window(w) {
            return e.handle;
        }
        let Some(x) = s.desk.get(w) else { break };
        w = if x.parent != pmwin::HWND_DESKTOP && x.parent != 0 { x.parent } else { x.owner };
        if w == 0 || w == pmwin::HWND_DESKTOP {
            break;
        }
    }
    set_error(PMERR_INVALID_SWITCH_HANDLE);
    0
}

/// WinQuerySwitchList(hab, pswblk, cbBuf): the number of entries; with a
/// buffer, SWBLOCK (count, then SWENTRY: handle and SWCNTRL) with as many
/// as fit, returning how many it holds.
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySwitchList(_hab: u32, buf: *mut u8, cb: u32) -> u32 {
    let l = &st().switch.entries;
    if buf.is_null() {
        return l.len() as u32;
    }
    if cb < 4 {
        return 0;
    }
    let size = 4 + core::mem::size_of::<SwCntrl>();
    let n = l.len().min((cb as usize - 4) / size);
    unsafe {
        (buf as *mut u32).write_unaligned(n as u32);
        for (i, e) in l.iter().take(n).enumerate() {
            let at = buf.add(4 + i * size);
            (at as *mut u32).write_unaligned(e.handle);
            (at.add(4) as *mut SwCntrl).write_unaligned(current(e));
        }
    }
    n as u32
}

/// WinSwitchToProgram(hsw): bring the entry's window to the front; 0, or
/// an error code.
#[unsafe(no_mangle)]
pub extern "C" fn WinSwitchToProgram(h: u32) -> u32 {
    let Some(hwnd) = st().switch.get(h).map(|e| e.ctl.hwnd) else {
        set_error(PMERR_INVALID_SWITCH_HANDLE);
        return PMERR_INVALID_SWITCH_HANDLE;
    };
    crate::server::switch_to_window(hwnd);
    0
}
