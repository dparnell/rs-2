//! Menus and accelerators.
//!
//! An HMENU is a PM menu window (WC_MENU): a menu bar (CreateMenu, the
//! top of a menu template) becomes the frame's FID_MENU with SetMenu; a
//! pop-up menu (CreatePopupMenu, MF_POPUP items) is a hidden menu window
//! hung on its item (MIS_SUBMENU, with an ID of its own: Windows' pop-up
//! items have none). PM's menus draw and track themselves, keyboard
//! included (Alt, F10, mnemonics); their notifications, which the frame
//! passes to the client, become Windows' WM_INITMENU, WM_INITMENUPOPUP,
//! WM_MENUSELECT and WM_COMMAND (pm_message). Windows' "&" mnemonics are
//! PM's "~".
//!
//! Accelerator tables (RT_ACCELERATOR: flags, key, command) are WIN16's:
//! TranslateAccelerator matches WM_KEYDOWN (FVIRTKEY entries, with the
//! shift states) or WM_CHAR and sends WM_COMMAND with lParam 1 << 16.

use alloc::vec::Vec;

use crate::pm::*;
use crate::res::{self, Name};
use crate::wnd::{self as user, wnd};
use crate::Global;

pub const MF_GRAYED: u16 = 0x0001;
pub const MF_DISABLED: u16 = 0x0002;
pub const MF_BITMAP: u16 = 0x0004;
pub const MF_CHECKED: u16 = 0x0008;
pub const MF_POPUP: u16 = 0x0010;
pub const MF_MENUBARBREAK: u16 = 0x0020;
pub const MF_MENUBREAK: u16 = 0x0040;
pub const MF_END: u16 = 0x0080;
pub const MF_OWNERDRAW: u16 = 0x0100;
pub const MF_BYPOSITION: u16 = 0x0400;
pub const MF_SEPARATOR: u16 = 0x0800;

pub const MIS_TEXT: u16 = 0x0001;
pub const MIS_OWNERDRAW: u16 = 0x0008;
pub const MIS_SEPARATOR: u16 = 0x0004;
pub const MIS_SUBMENU: u16 = 0x0010;
const MIS_BREAKSEPARATOR: u16 = 0x0800;
const MIS_BREAK: u16 = 0x0400;
const MIS_SYSCOMMAND: u16 = 0x0040;
const FID_SYSMENU: u32 = 0x8002;
const SC_SYSMENU: u16 = 0x8007;
pub const MIA_CHECKED: u16 = 0x2000;
pub const MIA_DISABLED: u16 = 0x4000;
const MS_ACTIONBAR: u32 = 0x0001;

const MM_INSERTITEM: u32 = 0x0180;
const MM_DELETEITEM: u32 = 0x0181;
pub const MM_QUERYITEM: u32 = 0x0182;
pub const MM_QUERYITEMCOUNT: u32 = 0x0184;
const MM_REMOVEITEM: u32 = 0x0188;
pub const MM_QUERYITEMTEXT: u32 = 0x018b;
pub const MM_ITEMPOSITIONFROMID: u32 = 0x018f;
pub const MM_ITEMIDFROMPOSITION: u32 = 0x0190;
pub const MM_QUERYITEMATTR: u32 = 0x0191;
pub const MM_SETITEMATTR: u32 = 0x0192;
pub const MIT_NONE: u32 = 0xffff;

/// MENUITEM: iPosition, afStyle, afAttribute, id, hwndSubMenu, hItem.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct MenuItem {
    pub pos: i16,
    pub style: u16,
    pub attr: u16,
    pub id: u16,
    pub sub: u32,
    pub handle: u32,
}

struct Menu {
    h: u16,
    pm: u32,
}

struct Menus {
    menus: Vec<Menu>,
    next: u16,
    /// IDs of pop-up items (PM wants one for every item).
    next_popup_id: u16,
    accels: Vec<Option<Vec<(u8, u16, u16)>>>,
    /// System menus (PM menu windows) GetSystemMenu gave out.
    system: Vec<u32>,
}

static MENUS: Global<Menus> = Global::new(Menus { menus: Vec::new(), next: 0xa004, next_popup_id: 0xf800, accels: Vec::new(), system: Vec::new() });

fn m() -> &'static mut Menus {
    MENUS.get()
}

/// The PM menu window of an HMENU.
/// A menu window becomes a menu bar (horizontal) or a pop-up.
fn set_bar(pm: u32, bar: bool) {
    if pm != 0 {
        unsafe { WinSetWindowBits(pm, QWL_STYLE, if bar { MS_ACTIONBAR } else { 0 }, MS_ACTIONBAR) };
    }
}

pub fn pm_of(h: u16) -> u32 {
    m().menus.iter().find(|x| x.h == h).map_or(0, |x| x.pm)
}

/// The HMENU of a PM menu window (a new one if it has none).
pub fn handle_of(pm: u32) -> u16 {
    if pm == 0 {
        return 0;
    }
    if let Some(x) = m().menus.iter().find(|x| x.pm == pm) {
        return x.h;
    }
    let s = m();
    let h = s.next;
    s.next = s.next.wrapping_add(4).max(0xa004);
    s.menus.push(Menu { h, pm });
    h
}

fn known(pm: u32) -> bool {
    m().menus.iter().any(|x| x.pm == pm)
}

pub fn create(bar: bool) -> u16 {
    let pm = unsafe { WinCreateWindow(HWND_DESKTOP, b"#4\0".as_ptr(), b"\0".as_ptr(), if bar { MS_ACTIONBAR } else { 0 }, 0, 0, 0, 0, 0, HWND_TOP, 0, core::ptr::null(), core::ptr::null()) };
    handle_of(pm)
}

pub fn send(pm: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe { WinSendMsg(pm, msg, mp1, mp2) }
}

pub fn mp(lo: u32, hi: u32) -> u32 {
    (hi & 0xffff) << 16 | lo & 0xffff
}

/// Windows' "&" mnemonics as PM's "~".
fn pm_text(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'&' if s.get(i + 1) == Some(&b'&') => {
                v.push(b'&');
                i += 1;
            }
            b'&' => v.push(b'~'),
            b'~' => v.extend_from_slice(b"~~"),
            c => v.push(c),
        }
        i += 1;
    }
    v.push(0);
    v
}

pub fn win_text(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'~' if s.get(i + 1) == Some(&b'~') => {
                v.push(b'~');
                i += 1;
            }
            b'~' => v.push(b'&'),
            b'&' => v.extend_from_slice(b"&&"),
            c => v.push(c),
        }
        i += 1;
    }
    v
}

/// Insert an item at `pos` (-1: the end): Windows' flags, ID or pop-up
/// HMENU, and text.
pub fn insert(menu: u16, pos: i16, flags: u16, id: u16, text: &[u8]) -> bool {
    let pm = pm_of(menu);
    if pm == 0 {
        return false;
    }
    let mut mi = MenuItem { pos, ..Default::default() };
    if flags & MF_SEPARATOR != 0 || (flags & (MF_POPUP | MF_BITMAP | MF_OWNERDRAW) == 0 && text.is_empty() && id == 0) {
        mi.style = MIS_SEPARATOR;
        // Separators get IDs of their own (PM's 0xFFFF is "no item", and
        // programs remove separators by position); WIN16's own (0xF000
        // and up) keep theirs.
        mi.id = if id >= 0xf000 {
            id
        } else {
            let s = m();
            let i = s.next_popup_id;
            s.next_popup_id = s.next_popup_id.wrapping_add(1).max(0xf800);
            i
        };
    } else {
        mi.style = MIS_TEXT;
        // An owner-drawn item: PM asks the owner to measure and draw it
        // (owner_item), with the program's item data as its hItem.
        if flags & MF_OWNERDRAW != 0 {
            mi.style = MIS_OWNERDRAW;
            mi.handle = core::mem::take(OD_DATA.get());
        }
        if flags & MF_POPUP != 0 {
            let sub = pm_of(id);
            if sub == 0 {
                return false;
            }
            // Windows' CreateMenu and CreatePopupMenu make the same menu
            // (Delphi's drop-downs come from CreateMenu): it is a pop-up
            // now.
            set_bar(sub, false);
            mi.style |= MIS_SUBMENU;
            mi.sub = sub;
            let s = m();
            mi.id = s.next_popup_id;
            s.next_popup_id = s.next_popup_id.wrapping_add(1).max(0xf800);
        } else {
            mi.id = id;
        }
    }
    if flags & MF_MENUBARBREAK != 0 {
        mi.style |= MIS_BREAKSEPARATOR;
    } else if flags & MF_MENUBREAK != 0 {
        mi.style |= MIS_BREAK;
    }
    if flags & (MF_GRAYED | MF_DISABLED) != 0 {
        mi.attr |= MIA_DISABLED;
    }
    if flags & MF_CHECKED != 0 {
        mi.attr |= MIA_CHECKED;
    }
    // A program's items in a system menu send WM_SYSCOMMAND.
    if m().system.contains(&pm) {
        mi.style |= MIS_SYSCOMMAND;
    }
    let t = pm_text(text);
    send(pm, MM_INSERTITEM, &mi as *const MenuItem as u32, t.as_ptr() as u32) != MIT_NONE
}

/// A new pop-up menu (CreatePopupMenu).
pub fn create_popup() -> u16 {
    create(false)
}

/// An owner-drawn item WIN16 draws itself (the MDI items of a frame's
/// menu bar while a child is maximized): `id`, with the pop-up `sub`
/// (0: none), at `pos` (-1: the end).
pub fn insert_owner(menu: u16, pos: i16, id: u16, sub: u16) -> bool {
    let pm = pm_of(menu);
    if pm == 0 {
        return false;
    }
    let mut mi = MenuItem { pos, style: 0x0008, id, ..Default::default() };
    if sub != 0 {
        let sp = pm_of(sub);
        if sp == 0 {
            return false;
        }
        set_bar(sp, false);
        mi.style |= MIS_SUBMENU;
        mi.sub = sp;
    }
    send(pm, MM_INSERTITEM, &mi as *const MenuItem as u32, b"\0".as_ptr() as u32) != MIT_NONE
}

/// Take the item `id` out of a menu, keeping its pop-up (it goes back
/// to the desktop).
pub fn remove_keep(menu: u16, id: u16) -> bool {
    remove(menu, id, 0, false)
}

/// DrawMenuBar: the window's menu bar is drawn again.
pub fn draw_bar(h: u16) {
    let pm = wnd(h).map_or(0, |w| pm_of(w.menu as u16));
    if pm != 0 {
        // Its items' sizes may have changed: PM lays it out again.
        let mut r = RectL::default();
        unsafe {
            WinQueryWindowRect(pm, &mut r);
            WinInvalidateRect(pm, core::ptr::null(), 0);
        }
    }
}

/// A window's system menu (the pop-up of its frame's FID_SYSMENU).
pub fn system_menu(h: u16) -> u16 {
    let Some(f) = wnd(user::top_level(h)).filter(|w| w.framed).map(|w| w.frame) else { return 0 };
    let sm = unsafe { WinWindowFromID(f, FID_SYSMENU) };
    let Some(mi) = query(sm, SC_SYSMENU) else { return 0 };
    let sub = mi.sub;
    if sub == 0 {
        return 0;
    }
    if !m().system.contains(&sub) {
        m().system.push(sub);
    }
    handle_of(sub)
}

/// The menu or submenu that has the item `id` itself.
pub fn holder(pm: u32, id: u16) -> Option<u32> {
    if pm == 0 {
        return None;
    }
    if send(pm, MM_ITEMPOSITIONFROMID, mp(id as u32, 0), 0) != MIT_NONE {
        return Some(pm);
    }
    let n = send(pm, MM_QUERYITEMCOUNT, 0, 0);
    for i in 0..n {
        let sid = send(pm, MM_ITEMIDFROMPOSITION, i, 0);
        if let Some(mi) = query(pm, sid as u16) {
            if mi.sub != 0 {
                if let Some(m) = holder(mi.sub, id) {
                    return Some(m);
                }
            }
        }
    }
    None
}

/// The PM ID of an item given by ID or (MF_BYPOSITION) position.
pub fn item_id(pm: u32, which: u16, flags: u16) -> Option<u16> {
    if flags & MF_BYPOSITION != 0 {
        let id = send(pm, MM_ITEMIDFROMPOSITION, which as u32, 0);
        (id != MIT_NONE).then_some(id as u16)
    } else {
        // IDs are looked for in the submenus too.
        let pos = send(pm, MM_ITEMPOSITIONFROMID, mp(which as u32, 1), 0);
        (pos != MIT_NONE).then_some(which)
    }
}

pub fn query(pm: u32, id: u16) -> Option<MenuItem> {
    let mut mi = MenuItem::default();
    (send(pm, MM_QUERYITEM, mp(id as u32, 1), &mut mi as *mut MenuItem as u32) != 0).then_some(mi)
}

/// Windows' state flags of an item (GetMenuState).
pub fn state(pm: u32, id: u16) -> u32 {
    let Some(mi) = query(pm, id) else { return 0xffff };
    let mut f = 0u32;
    if mi.attr & MIA_DISABLED != 0 {
        f |= (MF_GRAYED | MF_DISABLED) as u32;
    }
    if mi.attr & MIA_CHECKED != 0 {
        f |= MF_CHECKED as u32;
    }
    if mi.style & MIS_SEPARATOR != 0 {
        f |= MF_SEPARATOR as u32;
    }
    if mi.style & MIS_SUBMENU != 0 {
        let n = send(mi.sub, MM_QUERYITEMCOUNT, 0, 0);
        f |= MF_POPUP as u32 | (n & 0xff) << 8;
    }
    f
}

/// Change an attribute (MIA_DISABLED or MIA_CHECKED): the previous
/// state in Windows' flags, or -1.
pub fn set_attr(menu: u16, which: u16, flags: u16, mask: u16, on: bool) -> u32 {
    let pm = pm_of(menu);
    let Some(id) = item_id(pm, which, flags) else { return 0xffff_ffff };
    let old = send(pm, MM_QUERYITEMATTR, mp(id as u32, 1), mask as u32);
    send(pm, MM_SETITEMATTR, mp(id as u32, 1), mp(mask as u32, if on { mask as u32 } else { 0 }));
    match mask {
        MIA_DISABLED => if old != 0 { (MF_GRAYED | MF_DISABLED) as u32 } else { 0 },
        _ => if old != 0 { MF_CHECKED as u32 } else { 0 },
    }
}

pub fn remove(menu: u16, which: u16, flags: u16, destroy: bool) -> bool {
    let pm = pm_of(menu);
    let Some(id) = item_id(pm, which, flags) else { return false };
    if let Some(mi) = query(pm, id) {
        if mi.sub != 0 {
            if destroy {
                forget_tree(mi.sub);
            } else {
                // A removed pop-up lives on: back to the desktop.
                unsafe { WinSetParent(mi.sub, HWND_DESKTOP, 0) };
            }
        }
        // MM_REMOVEITEM keeps the submenu window; MM_DELETEITEM
        // destroys it.
        send(pm, if destroy { MM_DELETEITEM } else { MM_REMOVEITEM }, mp(id as u32, 1), 0);
        if !destroy && mi.sub != 0 {
            return true;
        }
    }
    true
}

/// Forget the handles of a menu and its submenus (being destroyed).
pub fn forget_tree(pm: u32) {
    let n = send(pm, MM_QUERYITEMCOUNT, 0, 0);
    for i in 0..n {
        let id = send(pm, MM_ITEMIDFROMPOSITION, i, 0) as u16;
        if let Some(mi) = query(pm, id) {
            if mi.sub != 0 {
                forget_tree(mi.sub);
            }
        }
    }
    m().menus.retain(|x| x.pm != pm);
}

/// LoadMenu from a module's resources.
pub fn load(hinst: u32, name: &Name) -> u16 {
    match res::find(hinst, res::RT_MENU, name) {
        Some(t) => from_template(t),
        None => 0,
    }
}

/// SetMenu: the menu bar of a top-level window (0 removes it).
pub fn set_menu(hwnd: u16, menu: u32) -> bool {
    let menu = menu as u16;
    let Some((f, old, framed)) = wnd(hwnd).map(|w| (w.frame, w.menu as u16, w.framed)) else { return false };
    if !framed {
        return false;
    }
    let pm = pm_of(menu);
    unsafe {
        let oldpm = pm_of(old);
        if oldpm != 0 && oldpm != pm {
            WinShowWindow(oldpm, 0);
            WinSetWindowUShort(oldpm, QWS_ID, 0);
            WinSetParent(oldpm, HWND_DESKTOP, 0);
        }
        if pm != 0 {
            set_bar(pm, true);
            WinSetParent(pm, f, 0);
            WinSetOwner(pm, f);
            WinSetWindowUShort(pm, QWS_ID, FID_MENU as u16);
            WinShowWindow(pm, 1);
        }
        WinSendMsg(f, WM_UPDATEFRAME, 0, 0);
    }
    if let Some(w) = wnd(hwnd) {
        w.menu = menu as u32;
    }
    true
}

/// The menu bar that has had Windows' WM_INITMENU in this menu mode.
static BAR_INIT: crate::Global<u32> = crate::Global::new(0);

/// PM's menu messages to a client (from its frame) as Windows'.
pub fn pm_message(h: u16, msg: u32, mp1: u32, mp2: u32) -> u32 {
    use crate::wnd::*;
    let bar = wnd(h).map_or(0, |w| pm_of(w.menu as u16));
    match msg {
        WM_COMMAND => {
            // CMDSRC_MENU 2, CMDSRC_ACCELERATOR 3; a system menu's own
            // items are WM_SYSCOMMAND (the frame's).
            let src = mp2 & 0xffff;
            if src == 2 || src == 3 {
                match RETURNED.get() {
                    Some(c) => *c = mp1 & 0xffff,
                    None => send_cmd(h, mp1 & 0xffff, src == 3),
                }
            }
            0
        }
        WM_INITMENU => {
            let pm = mp2;
            if pm == bar && bar != 0 {
                *BAR_INIT.get() = bar;
                user::send(h, W_INITMENU, handle_of(pm) as u32, 0);
            } else if known(pm) || pm != 0 && is_below(bar, pm) {
                let index = parent_index(bar, pm).unwrap_or(0);
                let sys = m().system.contains(&pm) as u32;
                user::send(h, W_INITMENUPOPUP, handle_of(pm) as u32, index as u32 | sys << 16);
            }
            0
        }
        WM_MENUSELECT => {
            if mp2 == bar && bar != 0 && *BAR_INIT.get() != bar {
                *BAR_INIT.get() = bar;
                user::send(h, W_INITMENU, handle_of(bar) as u32, 0);
            }
            if mp1 >> 16 == 0 {
                let pm = mp2;
                let id = mp1 as u16;
                let mut flags = if known(pm) { state(pm, id) & 0xff } else { 0 };
                // A pop-up item: its position.
                let item = if id >= 0xf800 {
                    flags |= MF_POPUP as u32;
                    self::send(pm, MM_ITEMPOSITIONFROMID, mp(id as u32, 0), 0) & 0xffff
                } else {
                    id as u32
                };
                if m().system.contains(&pm) {
                    flags |= 0x2000; // MF_SYSMENU
                }
                user::send(h, W_MENUSELECT, item | flags << 16, handle_of(pm) as u32);
            }
            1
        }
        WM_MENUEND => {
            if mp2 == bar {
                *BAR_INIT.get() = 0;
                user::send(h, W_MENUSELECT, 0xffff_0000, 0);
            }
            0
        }
        _ => 0,
    }
}

/// WM_COMMAND for a menu item (HIWORD 0) or accelerator (1).
fn send_cmd(h: u16, id: u32, accel: bool) {
    user::send(h, user::W_COMMAND, id | (accel as u32) << 16, 0);
}

/// Is `pm` a submenu (at any depth) of `root`?
fn is_below(root: u32, pm: u32) -> bool {
    parent_index(root, pm).is_some()
}

/// The position of the item that has the submenu `pm`, in its menu.
fn parent_index(root: u32, pm: u32) -> Option<u16> {
    if root == 0 {
        return None;
    }
    let n = send(root, MM_QUERYITEMCOUNT, 0, 0);
    for i in 0..n {
        let id = send(root, MM_ITEMIDFROMPOSITION, i, 0) as u16;
        if let Some(mi) = query(root, id) {
            if mi.sub == pm {
                return Some(i as u16);
            }
            if mi.sub != 0 {
                if let Some(j) = parent_index(mi.sub, pm) {
                    return Some(j);
                }
            }
        }
    }
    None
}

/// Is a command a disabled item of the window's menu?
fn disabled(h: u16, cmd: u16) -> bool {
    let bar = wnd(h).map_or(0, |w| pm_of(w.menu as u16));
    bar != 0 && query(bar, cmd).is_some_and(|mi| mi.attr & MIA_DISABLED != 0)
}

/// An owner-drawn item's data (the "text" argument), for insert.
static OD_DATA: Global<u32> = Global::new(0);

/// TrackPopupMenu: show the menu at (x, y) on the screen; its commands go
/// to the window; returns when the menu closes.
/// The pop-up menu TrackPopupMenu shows (EndMenu closes it).
static TRACKING: crate::Global<u32> = crate::Global::new(0);

/// EndMenu: the pop-up being tracked, or the active window's menu bar,
/// leaves menu mode.
pub fn end_menu() {
    let pm = core::mem::replace(TRACKING.get(), 0);
    let _ = pm;
    // MM_ENDMENUMODE
    if pm != 0 && unsafe { WinIsWindowVisible(pm) } != 0 {
        unsafe { WinSendMsg(pm, 0x0186, 1, 0) };
        return;
    }
    let active = unsafe { crate::pm::WinQueryActiveWindow(HWND_DESKTOP) };
    let bar = unsafe { WinWindowFromID(active, FID_MENU) };
    if bar != 0 {
        unsafe { WinSendMsg(bar, 0x0186, 1, 0) };
    }
}

pub fn track_popup(menu: u16, x: i32, y: i32, hwnd: u16, flags: u32) -> u32 {
    let pm = pm_of(menu);
    set_bar(pm, false);
    let Some(c) = wnd(hwnd).map(|w| w.client) else { return 0 };
    if pm == 0 {
        return 0;
    }
    unsafe extern "C" {
        #[link_name = "PMWIN.937"]
        fn WinPopupMenu(parent: u32, owner: u32, menu: u32, x: i32, y: i32, id: u32, fs: u32) -> u32;
    }
    let sy = user::screen_size().1;
    // PU_KEYBOARD | PU_MOUSEBUTTON1; TPM_RETURNCMD (0x100): the command
    // comes back instead of going to the window.
    if unsafe { WinPopupMenu(HWND_DESKTOP, c, pm, x, sy - y, 0, 0x0200 | 0x0400) } == 0 {
        return 0;
    }
    *RETURNED.get() = if flags & 0x100 != 0 { Some(0) } else { None };
    let hab = crate::hab();
    *TRACKING.get() = pm;
    while unsafe { WinIsWindowVisible(pm) } != 0 {
        let mut q = QMsg::default();
        if unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } == 0 {
            unsafe { WinPostQueueMsg(crate::queue().hmq, WM_QUIT, q.mp1, q.mp2) };
            break;
        }
        unsafe { WinDispatchMsg(hab, &q) };
    }
    match RETURNED.get().take() {
        Some(cmd) => cmd,
        None => 1,
    }
}

/// TrackPopupMenu with TPM_RETURNCMD: the command chosen.
pub static RETURNED: Global<Option<u32>> = Global::new(None);

/// Is a PM window one of WIN16's menus?
fn is_menu_window(pm: u32) -> bool {
    m().menus.iter().any(|x| x.pm == pm)
}

/// The text of a new item (nothing for separators, bitmaps and owner
/// drawn items: an owner-drawn item's argument is its data).
pub fn item_text(fl: u32, text: u32, wide: bool) -> Vec<u8> {
    if fl & MF_OWNERDRAW as u32 != 0 {
        *OD_DATA.get() = text;
    }
    if fl & (MF_SEPARATOR | MF_BITMAP | MF_OWNERDRAW) as u32 != 0 || text >> 16 == 0 {
        return Vec::new();
    }
    if wide { crate::wide_str(text).unwrap_or_default() } else { crate::far_str(text).unwrap_or(b"").to_vec() }
}

/// InsertMenu(hmenu, pos, flags, id, text): by command, before the item,
/// in the (sub)menu that has it.
pub fn insert_menu(h: u16, pos: u32, fl: u32, id: u32, text: &[u8]) -> bool {
    let (h, at) = if fl & MF_BYPOSITION as u32 != 0 {
        (h, if pos == 0xffff_ffff { -1 } else { pos as i16 })
    } else {
        match holder(pm_of(h), pos as u16) {
            Some(m) => (handle_of(m), send(m, MM_ITEMPOSITIONFROMID, mp(pos, 0), 0) as i16),
            None => (h, -1),
        }
    };
    insert(h, at, fl as u16, id as u16, text)
}

/// ModifyMenu(hmenu, pos, flags, id, text): the item stays in the
/// (sub)menu that has it, where it was.
pub fn modify_menu(h: u16, pos: u32, fl: u32, id: u32, text: &[u8]) -> bool {
    let Some(old) = item_id(pm_of(h), pos as u16, fl as u16) else { return false };
    let pm = holder(pm_of(h), old).unwrap_or(pm_of(h));
    let h = handle_of(pm);
    let at = send(pm, MM_ITEMPOSITIONFROMID, mp(old as u32, 0), 0) as i16;
    remove(h, old, 0, false);
    insert(h, at, (fl as u16) & !MF_BYPOSITION, id as u16, text)
}

/// A menu from a Win32 menu template: MENUITEMTEMPLATEHEADER (version 0,
/// offset), items (flags, ID unless a pop-up, UTF-16 text; MF_END ends a
/// level); or an extended one (version 1, offset): MENUEX_TEMPLATE_ITEMs
/// (type, state, ID, resInfo, text, DWORD-aligned; a pop-up's help ID).
pub fn from_template(t: &[u8]) -> u16 {
    let bar = create(true);
    if res::w16(t, 0) == 1 {
        let mut p = 4 + res::w16(t, 2) as usize;
        items_ex(t, &mut p, bar);
    } else {
        let mut p = 4 + res::w16(t, 2) as usize;
        items(t, &mut p, bar);
    }
    bar
}

fn items(t: &[u8], p: &mut usize, menu: u16) {
    while *p + 2 <= t.len() {
        let flags = res::w16(t, *p);
        *p += 2;
        if flags & MF_POPUP != 0 {
            let text = crate::narrow(&res::wsz(t, p));
            let sub = create(false);
            items(t, p, sub);
            insert(menu, -1, flags & !MF_END, sub, &text);
        } else {
            let id = res::w16(t, *p);
            *p += 2;
            let text = crate::narrow(&res::wsz(t, p));
            insert(menu, -1, flags & !MF_END, id, &text);
        }
        if flags & MF_END != 0 {
            return;
        }
    }
}

fn items_ex(t: &[u8], p: &mut usize, menu: u16) {
    while *p + 14 <= t.len() {
        let (ty, st, id) = (res::l32(t, *p), res::l32(t, *p + 4), res::l32(t, *p + 8));
        let info = res::w16(t, *p + 12);
        *p += 14;
        let text = crate::narrow(&res::wsz(t, p));
        *p = (*p + 3) & !3;
        // MFT_SEPARATOR 0x800, MFT_MENUBARBREAK 0x20, MFT_MENUBREAK 0x40;
        // MFS_GRAYED 3, MFS_CHECKED 8.
        let flags = (ty & 0x0860) as u16 | (st & 0x000b) as u16;
        if info & 1 != 0 {
            *p += 4;
            let sub = create(false);
            items_ex(t, p, sub);
            insert(menu, -1, flags | MF_POPUP, sub, &text);
        } else {
            insert(menu, -1, flags, id as u16, &text);
        }
        if info & 0x80 != 0 {
            return;
        }
    }
}

// ---- Accelerators ----

/// Accelerator tables: 0x000A0000 + index (programs only hand them back).
const ACCEL_BASE: u32 = 0x000a_0000;

fn new_accel(v: Vec<(u8, u16, u16)>) -> u32 {
    let s = m();
    s.accels.push(Some(v));
    ACCEL_BASE + s.accels.len() as u32 - 1
}

pub fn accel(h: u32) -> Option<&'static Vec<(u8, u16, u16)>> {
    m().accels.get(h.wrapping_sub(ACCEL_BASE) as usize)?.as_ref()
}

/// LoadAccelerators: RT_ACCELERATOR's entries (fFlags, key, command, a
/// pad word; 0x80 in the flags of the last).
pub fn load_accel(hinst: u32, name: &Name) -> u32 {
    let Some(t) = res::find(hinst, res::RT_ACCELERATOR, name) else { return 0 };
    let mut v = Vec::new();
    for e in t.chunks(8) {
        if e.len() < 6 {
            break;
        }
        v.push((e[0], res::w16(e, 2), res::w16(e, 4)));
        if e[0] & 0x80 != 0 {
            break;
        }
    }
    new_accel(v)
}

/// CreateAcceleratorTable(ACCEL[n]): fVirt, key, cmd (6 bytes each).
pub fn create_accel(p: u32, n: i32) -> u32 {
    if p == 0 || n <= 0 {
        return 0;
    }
    let v = (0..n as u32).map(|i| (crate::rd16(p, 6 * i) as u8, crate::rd16(p, 6 * i + 2), crate::rd16(p, 6 * i + 4))).collect();
    new_accel(v)
}

/// CopyAcceleratorTable(h, ACCEL[n]): the entries (all of them with no
/// buffer).
pub fn copy_accel(h: u32, p: u32, n: i32) -> i32 {
    let Some(t) = accel(h) else { return 0 };
    if p == 0 {
        return t.len() as i32;
    }
    let k = t.len().min(n.max(0) as usize);
    for (i, &(f, key, cmd)) in t[..k].iter().enumerate() {
        let o = p + 6 * i as u32;
        unsafe {
            ((o) as *mut u16).write_unaligned(f as u16 & 0x7f);
            ((o + 2) as *mut u16).write_unaligned(key);
            ((o + 4) as *mut u16).write_unaligned(cmd);
        }
    }
    k as i32
}

pub fn destroy_accel(h: u32) -> bool {
    match m().accels.get_mut(h.wrapping_sub(ACCEL_BASE) as usize) {
        Some(e) if e.is_some() => {
            *e = None;
            true
        }
        _ => false,
    }
}

/// TranslateAccelerator(hwnd, haccel, msg): FVIRTKEY (1) entries match
/// WM_KEYDOWN with the shift states (FSHIFT 4, FCONTROL 8, FALT 0x10),
/// the others WM_CHAR.
pub fn translate_accel(h: u16, acc: u32, msg: &crate::wnd::Msg) -> bool {
    use crate::wnd::*;
    let (m_, wp) = (msg.msg as u16, msg.wp);
    let Some(table) = accel(acc) else { return false };
    let down = |vk| key_state(vk) & 0x8000 != 0;
    let (shift, ctrl, alt) = (down(0x10), down(0x11), down(0x12));
    let hit = table.iter().find(|&&(f, key, _)| {
        if f & 1 != 0 {
            (m_ == W_KEYDOWN || m_ == W_SYSKEYDOWN) && key as u32 == wp && (f & 4 != 0) == shift && (f & 8 != 0) == ctrl && (f & 0x10 != 0) == alt
        } else {
            (m_ == W_CHAR || m_ == W_SYSCHAR) && key as u32 == wp && (f & 0x10 != 0) == (m_ == W_SYSCHAR)
        }
    });
    let Some(&(_, _, cmd)) = hit else { return false };
    let t = top_level(h);
    let menu = wnd(t).map_or(0, |w| w.menu);
    if menu != 0 {
        user::send(t, W_INITMENU, menu, 0);
    }
    if !disabled(t, cmd) {
        send_cmd(h, cmd as u32, true);
    }
    true
}

/// PM's WM_MEASUREITEM and WM_DRAWITEM (`oi` an OWNERITEM: hwnd, hps,
/// fsState, fsAttribute, fsStateOld, fsAttributeOld, rclItem, idItem,
/// hItem) for an owner-drawn item of a window's menu: Windows'
/// WM_MEASUREITEM and WM_DRAWITEM (ODT_MENU) to the window.
pub fn owner_item(h: u16, msg: u32, oi: *mut u8) -> Option<u32> {
    if oi.is_null() {
        return None;
    }
    let rd = |o: usize| unsafe { (oi.add(o) as *const i32).read_unaligned() };
    let wr = |o: usize, v: i32| unsafe { (oi.add(o) as *mut i32).write_unaligned(v) };
    let (menu, hps, style, attr, attr_old) = (rd(0) as u32, rd(4) as u32, rd(8) as u16, rd(12) as u16, rd(20) as u16);
    let (id, data) = (rd(40) as u32 & 0xffff, rd(44) as u32);
    if style & MIS_OWNERDRAW == 0 || !is_menu_window(menu) {
        return None;
    }
    const ODT_MENU: u32 = 1;
    if msg == 0x0037 {
        // MEASUREITEMSTRUCT: CtlType, CtlID, itemID, itemWidth,
        // itemHeight, itemData.
        let mut mi: [u32; 6] = [ODT_MENU, 0, id, 0, 0, data];
        user::send(h, user::W_MEASUREITEM, 0, mi.as_mut_ptr() as u32);
        let (l, bt) = (rd(24), rd(28));
        wr(32, l + (mi[3] as i32).max(1));
        wr(36, bt + (mi[4] as i32).max(1));
        return Some(mi[4]);
    }
    let mut rc = RectL::default();
    unsafe { WinQueryWindowRect(menu, &mut rc) };
    let height = rc.t - rc.b;
    let hdc = crate::gdi::new_dc(hps, menu, height, crate::gdi::DcKind::Borrowed);
    const MIA_HILITED: u16 = 0x8000;
    let action: u32 = if attr ^ attr_old == MIA_HILITED { 2 } else { 1 };
    let state: u32 = if attr & MIA_HILITED != 0 { 1 } else { 0 } | if attr & MIA_DISABLED != 0 { 2 } else { 0 } | if attr & MIA_CHECKED != 0 { 8 } else { 0 };
    // DRAWITEMSTRUCT: CtlType, CtlID, itemID, itemAction, itemState,
    // hwndItem (the HMENU), hDC, rcItem, itemData.
    let di: [u32; 12] = [
        ODT_MENU,
        0,
        id,
        action,
        state,
        handle_of(menu) as u32,
        hdc as u32,
        rd(24) as u32,
        (height - rd(36)) as u32,
        rd(32) as u32,
        (height - rd(28)) as u32,
        data,
    ];
    user::send(h, user::W_DRAWITEM, 0, di.as_ptr() as u32);
    crate::gdi::release_dc(hdc);
    wr(12, (attr & !MIA_HILITED) as i32);
    wr(20, (attr_old & !MIA_HILITED) as i32);
    Some(1)
}

/// MENUITEMINFO (A or W): cbSize, fMask, fType, fState, wID, hSubMenu,
/// hbmpChecked, hbmpUnchecked, dwItemData, dwTypeData, cch, hbmpItem.
/// GetMenuItemInfo.
pub fn get_item_info(h: u16, item: u32, by_pos: bool, p: u32, wide: bool) -> bool {
    let pm = pm_of(h);
    let Some(id) = item_id(pm, item as u16, if by_pos { MF_BYPOSITION } else { 0 }) else { return false };
    let Some(owner) = holder(pm, id) else { return false };
    let Some(mi) = query(owner, id) else { return false };
    let mask = crate::rd32(p, 4);
    let st = state(owner, id);
    // MIIM_FTYPE 0x100 / MIIM_TYPE 0x10: MFT_SEPARATOR, MFT_OWNERDRAW.
    if mask & 0x110 != 0 {
        let ty = if mi.style & MIS_SEPARATOR != 0 { 0x800 } else { 0 } | if mi.style & MIS_OWNERDRAW != 0 { 0x100 } else { 0 };
        crate::wr32(p, 8, ty);
    }
    if mask & 1 != 0 {
        crate::wr32(p, 12, st & 0x0b);
    }
    if mask & 2 != 0 {
        crate::wr32(p, 16, if mi.sub != 0 { 0 } else { id as u32 });
    }
    if mask & 4 != 0 {
        crate::wr32(p, 20, if mi.sub != 0 { handle_of(mi.sub) as u32 } else { 0 });
    }
    if mask & 0x20 != 0 {
        crate::wr32(p, 32, mi.handle);
    }
    if mask & 0x50 != 0 {
        let mut b = [0u8; 256];
        send(owner, MM_QUERYITEMTEXT, mp(id as u32, 255), b.as_mut_ptr() as u32);
        let t = win_text(unsafe { crate::cstr(b.as_ptr()) });
        let (buf, cch) = (crate::rd32(p, 36), crate::rd32(p, 40));
        let n = if buf == 0 {
            if wide { crate::to_wide(&t).len() } else { t.len() }
        } else if wide {
            crate::copy_out_w(&crate::to_wide(&t), buf as *mut u16, cch as i32)
        } else {
            crate::copy_out(&t, buf as *mut u8, cch as i32)
        };
        crate::wr32(p, 40, n as u32);
    }
    true
}

/// SetMenuItemInfo: the state, ID, submenu, data and text asked for.
pub fn set_item_info(h: u16, item: u32, by_pos: bool, p: u32, wide: bool) -> bool {
    let pm = pm_of(h);
    let Some(id) = item_id(pm, item as u16, if by_pos { MF_BYPOSITION } else { 0 }) else { return false };
    let Some(owner) = holder(pm, id) else { return false };
    let mask = crate::rd32(p, 4);
    if mask & 1 != 0 {
        let st = crate::rd32(p, 12);
        let oh = handle_of(owner);
        set_attr(oh, id, 0, MIA_DISABLED, st & 3 != 0);
        set_attr(oh, id, 0, MIA_CHECKED, st & 8 != 0);
    }
    if mask & 0x50 != 0 || (mask & 0x10 != 0 && crate::rd32(p, 8) & 0x900 == 0) {
        let t = crate::rd32(p, 36);
        let text = if wide { crate::wide_str(t).unwrap_or_default() } else { crate::far_str(t).unwrap_or(b"").to_vec() };
        let pt = pm_text(&text);
        // MM_SETITEMTEXT
        send(owner, 0x018e, mp(id as u32, 0), pt.as_ptr() as u32);
    }
    if mask & 0x20 != 0 {
        // MM_SETITEMHANDLE
        send(owner, 0x01ac, mp(id as u32, 0), crate::rd32(p, 32));
    }
    true
}

/// InsertMenuItem: an item from a MENUITEMINFO.
pub fn insert_item_info(h: u16, item: u32, by_pos: bool, p: u32, wide: bool) -> bool {
    let mask = crate::rd32(p, 4);
    let ty = if mask & 0x110 != 0 { crate::rd32(p, 8) } else { 0 };
    let st = if mask & 1 != 0 { crate::rd32(p, 12) } else { 0 };
    let id = if mask & 2 != 0 { crate::rd32(p, 16) } else { 0 };
    let sub = if mask & 4 != 0 { crate::rd32(p, 20) } else { 0 };
    if mask & 0x20 != 0 {
        *OD_DATA.get() = crate::rd32(p, 32);
    }
    let t = crate::rd32(p, 36);
    let text = if ty & 0x904 != 0 || mask & 0x50 == 0 && mask & 0x10 == 0 {
        Vec::new()
    } else if wide {
        crate::wide_str(t).unwrap_or_default()
    } else {
        crate::far_str(t).unwrap_or(b"").to_vec()
    };
    let mut fl = (ty & 0x0960) | (st & 0x0b);
    let mut idv = id;
    if sub != 0 {
        fl |= MF_POPUP as u32;
        idv = sub;
    }
    if ty & 0x800 != 0 {
        fl |= MF_SEPARATOR as u32;
    }
    let fl = fl | if by_pos { MF_BYPOSITION as u32 } else { 0 };
    insert_menu(h, item, fl, idv, &text)
}
