//! Help requests, routed as IBM's PMWIN does: WM_HELP reaching a frame
//! and F1 in a menu call the help hook (HK_HELP), which the help manager
//! (HELPMGR.DLL) installs for each help instance's queue; the help system
//! commands (SC_HELPINDEX and so on) go straight to the help instance the
//! frame, or a frame above or owning it, is associated with
//! (WM_SETHELPINFO, which WinAssociateHelpInstance sends).

use crate::ctl;
use crate::gpiapi::{PointL, RectL};
use crate::lock;
use crate::msg::send;
use crate::state::st;
use crate::win::{WinMapWindowPoints, WM_SYSCOMMAND};
use pmwin::HWND_DESKTOP;

pub const HK_HELP: u32 = 5;
pub const HLPM_FRAME: u32 = -1i32 as u32;
pub const HLPM_WINDOW: u32 = -2i32 as u32;
pub const HLPM_MENU: u32 = -3i32 as u32;

pub const WM_QUERYHELPINFO: u32 = 0x005b;
pub const WM_SETHELPINFO: u32 = 0x005c;

pub const SC_HELPKEYS: u32 = 0x8012;
pub const SC_HELPINDEX: u32 = 0x8013;
pub const SC_HELPEXTENDED: u32 = 0x8014;
pub const SC_SWITCHPANELIDS: u32 = 0x8015;

const HM_EXT_HELP: u32 = 0x0223;
const HM_SET_SHOW_PANEL_ID: u32 = 0x0228;
const HM_HELP_INDEX: u32 = 0x022a;
const HM_KEYS_HELP: u32 = 0x022c;
const CMIC_TOGGLE: u32 = 2;

const FID_CLIENT: u32 = 0x8008;
const CBID_LIST: u32 = 0x029a;
const CBID_EDIT: u32 = 0x029b;

type HelpHook = extern "C" fn(hab: u32, mode: u32, topic: u32, sub: u32, rect: *const RectL) -> u32;

/// Call the help hooks of the calling thread's queue (then the system
/// ones), newest first, until one returns TRUE. The chain is read again
/// after each hook: a hook may set or release hooks (OS/2's E installs its
/// own, which creates the help instance on the first request, releases
/// itself and leaves the request to the help manager's hook).
pub fn call_hook(mode: u32, topic: u32, sub: u32, rect: RectL) -> bool {
    let me = crate::shared::pid();
    let mut called: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
    loop {
        let s = st();
        let q = s.my_queue();
        let next = s
            .hooks
            .iter()
            .find(|h| h.kind == HK_HELP && h.pid == me && (h.queue == q || h.queue == 0) && !called.contains(&h.pfn))
            .map(|h| h.pfn);
        let Some(pfn) = next else { return false };
        called.push(pfn);
        let f: HelpHook = unsafe { core::mem::transmute(pfn as usize) };
        if lock::unlocked(|| f(0, mode, topic, sub, &rect)) != 0 {
            return true;
        }
    }
}

/// A window's rectangle in screen coordinates.
pub fn screen_rect(h: u32) -> RectL {
    let (cx, cy) = ctl::size(h);
    let mut p = [PointL { x: 0, y: 0 }, PointL { x: cx, y: cy }];
    WinMapWindowPoints(h, HWND_DESKTOP, p.as_mut_ptr(), 2);
    RectL { left: p[0].x, bottom: p[0].y, right: p[1].x, top: p[1].y }
}

/// WM_HELP at frame `frame` (IBM's FrameProcessHelp): help on the window
/// with the focus.
pub fn frame_help(frame: u32) {
    let focus = st().focus;
    let mut w = if focus != 0 && st().desk.exists(focus) { focus } else { frame };
    // A combination box's entry field or list stands for the box.
    let id = ctl::id(w);
    if id == CBID_EDIT || id == CBID_LIST {
        let p = ctl::parent(w);
        if crate::combo::is_combo(p) {
            w = p;
        }
    }
    let rect = screen_rect(w);
    // Controls of a dialog (owner other than a client) are help on that
    // window; the rest on the frame.
    let owner = ctl::owner(w);
    if owner != 0 && ctl::id(owner) != FID_CLIENT {
        call_hook(HLPM_WINDOW, ctl::id(owner), ctl::id(w), rect);
    } else {
        call_hook(HLPM_FRAME, ctl::id(frame), ctl::id(w), rect);
    }
}

/// F1 on menu item `item` (an id, 0xffff: none) of pull-down `menu`
/// (IBM's MenuHelp): menu mode has ended.
pub fn menu_help(menu: u32, item: u32, rect: RectL) {
    call_hook(HLPM_MENU, ctl::id(menu), item, rect);
}

/// The help instance for `h`: the first frame from `h` up through
/// parents (owners at the desktop) that has one (HELPMGR's
/// HAPIFindWndWithHelp), if that window still exists.
pub fn instance_for(h: u32) -> u32 {
    let mut w = h;
    while w != 0 && w != HWND_DESKTOP {
        let f = frame_at(w);
        if f == 0 {
            return 0;
        }
        let inst = send(f, WM_QUERYHELPINFO, 0, 0);
        if inst != 0 {
            return if st().desk.exists(inst) { inst } else { 0 };
        }
        let p = ctl::parent(f);
        w = if p == 0 || p == HWND_DESKTOP { ctl::owner(f) } else { p };
    }
    0
}

/// `h` or the nearest parent that is a frame.
fn frame_at(mut h: u32) -> u32 {
    while h != 0 && h != HWND_DESKTOP {
        if crate::frame::is_frame(h) {
            return h;
        }
        h = ctl::parent(h);
    }
    0
}

/// The help system commands at frame `h` (IBM's FrameHelpSysCmd): the
/// help instance gets the matching HM_ message; the client gets the
/// command too when there is no help instance (and always for
/// SC_SWITCHPANELIDS). Returns false for other commands.
pub fn syscommand(h: u32, sc: u32, mp1: u32, mp2: u32) -> bool {
    let (hm, hm_mp1, always) = match sc {
        SC_HELPEXTENDED => (HM_EXT_HELP, 0, false),
        SC_HELPINDEX => (HM_HELP_INDEX, 0, false),
        SC_HELPKEYS => (HM_KEYS_HELP, 0, false),
        SC_SWITCHPANELIDS => (HM_SET_SHOW_PANEL_ID, CMIC_TOGGLE, true),
        _ => return false,
    };
    let inst = instance_for(h);
    if inst != 0 {
        send(inst, hm, hm_mp1, 0);
    }
    if always || inst == 0 {
        let client = st().frames.get(&h).map_or(0, |f| f.client);
        if client != 0 {
            send(client, WM_SYSCOMMAND, mp1, mp2);
        }
    }
    true
}
