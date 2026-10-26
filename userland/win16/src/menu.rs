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
use thunk16::Pascal;

use crate::pm::*;
use crate::res::{self, Name};
use crate::user::{self, wnd};
use crate::{far_str, Global};

const MF_GRAYED: u16 = 0x0001;
const MF_DISABLED: u16 = 0x0002;
const MF_BITMAP: u16 = 0x0004;
const MF_CHECKED: u16 = 0x0008;
const MF_POPUP: u16 = 0x0010;
const MF_MENUBARBREAK: u16 = 0x0020;
const MF_MENUBREAK: u16 = 0x0040;
const MF_END: u16 = 0x0080;
const MF_OWNERDRAW: u16 = 0x0100;
const MF_BYPOSITION: u16 = 0x0400;
const MF_SEPARATOR: u16 = 0x0800;

const MIS_TEXT: u16 = 0x0001;
const MIS_SEPARATOR: u16 = 0x0004;
const MIS_SUBMENU: u16 = 0x0010;
const MIS_BREAKSEPARATOR: u16 = 0x0800;
const MIS_BREAK: u16 = 0x0400;
const MIA_CHECKED: u16 = 0x2000;
const MIA_DISABLED: u16 = 0x4000;
const MS_ACTIONBAR: u32 = 0x0001;

const MM_INSERTITEM: u32 = 0x0180;
const MM_DELETEITEM: u32 = 0x0181;
const MM_QUERYITEM: u32 = 0x0182;
const MM_QUERYITEMCOUNT: u32 = 0x0184;
const MM_REMOVEITEM: u32 = 0x0188;
const MM_QUERYITEMTEXT: u32 = 0x018b;
const MM_ITEMPOSITIONFROMID: u32 = 0x018f;
const MM_ITEMIDFROMPOSITION: u32 = 0x0190;
const MM_QUERYITEMATTR: u32 = 0x0191;
const MM_SETITEMATTR: u32 = 0x0192;
const MIT_NONE: u32 = 0xffff;

/// MENUITEM: iPosition, afStyle, afAttribute, id, hwndSubMenu, hItem.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    handle: u32,
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
    accels: Vec<Vec<(u8, u16, u16)>>,
}

static MENUS: Global<Menus> = Global::new(Menus { menus: Vec::new(), next: 0x6004, next_popup_id: 0xf800, accels: Vec::new() });

fn m() -> &'static mut Menus {
    MENUS.get()
}

/// The PM menu window of an HMENU.
fn pm_of(h: u16) -> u32 {
    m().menus.iter().find(|x| x.h == h).map_or(0, |x| x.pm)
}

/// The HMENU of a PM menu window (a new one if it has none).
fn handle_of(pm: u32) -> u16 {
    if pm == 0 {
        return 0;
    }
    if let Some(x) = m().menus.iter().find(|x| x.pm == pm) {
        return x.h;
    }
    let s = m();
    let h = s.next;
    s.next = s.next.wrapping_add(4).max(0x6004);
    s.menus.push(Menu { h, pm });
    h
}

fn known(pm: u32) -> bool {
    m().menus.iter().any(|x| x.pm == pm)
}

fn create(bar: bool) -> u16 {
    let pm = unsafe { WinCreateWindow(HWND_DESKTOP, b"#4\0".as_ptr(), b"\0".as_ptr(), if bar { MS_ACTIONBAR } else { 0 }, 0, 0, 0, 0, 0, HWND_TOP, 0, core::ptr::null(), core::ptr::null()) };
    handle_of(pm)
}

fn send(pm: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    unsafe { WinSendMsg(pm, msg, mp1, mp2) }
}

fn mp(lo: u32, hi: u32) -> u32 {
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

fn win_text(s: &[u8]) -> Vec<u8> {
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
fn insert(menu: u16, pos: i16, flags: u16, id: u16, text: &[u8]) -> bool {
    let pm = pm_of(menu);
    if pm == 0 {
        return false;
    }
    let mut mi = MenuItem { pos, ..Default::default() };
    if flags & MF_SEPARATOR != 0 || (flags & (MF_POPUP | MF_BITMAP | MF_OWNERDRAW) == 0 && text.is_empty() && id == 0) {
        mi.style = MIS_SEPARATOR;
        mi.id = 0xffff;
    } else {
        mi.style = MIS_TEXT;
        if flags & MF_POPUP != 0 {
            let sub = pm_of(id);
            if sub == 0 {
                return false;
            }
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
    let t = pm_text(text);
    send(pm, MM_INSERTITEM, &mi as *const MenuItem as u32, t.as_ptr() as u32) != MIT_NONE
}

/// The PM ID of an item given by ID or (MF_BYPOSITION) position.
fn item_id(pm: u32, which: u16, flags: u16) -> Option<u16> {
    if flags & MF_BYPOSITION != 0 {
        let id = send(pm, MM_ITEMIDFROMPOSITION, which as u32, 0);
        (id != MIT_NONE).then_some(id as u16)
    } else {
        // IDs are looked for in the submenus too.
        let pos = send(pm, MM_ITEMPOSITIONFROMID, mp(which as u32, 1), 0);
        (pos != MIT_NONE).then_some(which)
    }
}

fn query(pm: u32, id: u16) -> Option<MenuItem> {
    let mut mi = MenuItem::default();
    (send(pm, MM_QUERYITEM, mp(id as u32, 1), &mut mi as *mut MenuItem as u32) != 0).then_some(mi)
}

/// Windows' state flags of an item (GetMenuState).
fn state(pm: u32, id: u16) -> u32 {
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
fn set_attr(menu: u16, which: u16, flags: u16, mask: u16, on: bool) -> u32 {
    let pm = pm_of(menu);
    let Some(id) = item_id(pm, which, flags) else { return 0xffff_ffff };
    let old = send(pm, MM_QUERYITEMATTR, mp(id as u32, 1), mask as u32);
    send(pm, MM_SETITEMATTR, mp(id as u32, 1), mp(mask as u32, if on { mask as u32 } else { 0 }));
    match mask {
        MIA_DISABLED => if old != 0 { (MF_GRAYED | MF_DISABLED) as u32 } else { 0 },
        _ => if old != 0 { MF_CHECKED as u32 } else { 0 },
    }
}

fn remove(menu: u16, which: u16, flags: u16, destroy: bool) -> bool {
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
fn forget_tree(pm: u32) {
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

/// A menu from a Windows menu template (MENUITEMTEMPLATEHEADER: version,
/// offset; items: flags, ID (not for pop-ups), text; MF_END ends a level).
fn from_template(t: &[u8]) -> u16 {
    let off = res::w16(t, 2) as usize;
    let bar = create(true);
    let mut p = 4 + off;
    items(t, &mut p, bar);
    bar
}

fn items(t: &[u8], p: &mut usize, menu: u16) {
    while *p + 2 <= t.len() {
        let flags = res::w16(t, *p);
        *p += 2;
        if flags & MF_POPUP != 0 {
            let text = res::sz(t, p).to_vec();
            let sub = create(false);
            items(t, p, sub);
            insert(menu, -1, flags & !MF_END, sub, &text);
        } else {
            let id = res::w16(t, *p);
            *p += 2;
            let text = res::sz(t, p).to_vec();
            insert(menu, -1, flags & !MF_END, id, &text);
        }
        if flags & MF_END != 0 {
            return;
        }
    }
}

/// LoadMenu from the program's resources.
pub fn load(_hinst: u16, name: &Name) -> u16 {
    match res::find(res::RT_MENU, name) {
        Some(t) => from_template(t),
        None => 0,
    }
}

/// SetMenu: the menu bar of a top-level window (0 removes it).
pub fn set_menu(hwnd: u16, menu: u16) -> bool {
    let Some((f, old, framed)) = wnd(hwnd).map(|w| (w.frame, w.menu, w.framed)) else { return false };
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
            WinSetParent(pm, f, 0);
            WinSetOwner(pm, f);
            WinSetWindowUShort(pm, QWS_ID, FID_MENU as u16);
            WinShowWindow(pm, 1);
        }
        WinSendMsg(f, WM_UPDATEFRAME, 0, 0);
    }
    if let Some(w) = wnd(hwnd) {
        w.menu = menu;
    }
    true
}

/// PM's menu messages to a client (from its frame) as Windows'.
pub fn pm_message(h: u16, msg: u32, mp1: u32, mp2: u32) -> u32 {
    use crate::user::*;
    let bar = wnd(h).map_or(0, |w| pm_of(w.menu));
    match msg {
        WM_COMMAND => {
            // CMDSRC_MENU 2, CMDSRC_ACCELERATOR 3.
            let src = mp2 & 0xffff;
            if src == 2 || src == 3 {
                send_cmd(h, mp1 as u16, if src == 3 { 1 << 16 } else { 0 });
            }
            0
        }
        WM_INITMENU => {
            let pm = mp2;
            if pm == bar && bar != 0 {
                user::send(h, W_INITMENU, handle_of(pm), 0);
            } else if known(pm) || pm != 0 && is_below(bar, pm) {
                let index = parent_index(bar, pm).unwrap_or(0);
                user::send(h, W_INITMENUPOPUP, handle_of(pm), index as u32);
            }
            0
        }
        WM_MENUSELECT => {
            if mp1 >> 16 == 0 {
                let pm = mp2;
                let id = mp1 as u16;
                let mut flags = if known(pm) { state(pm, id) & 0xff } else { 0 };
                if id >= 0xf800 {
                    flags |= MF_POPUP as u32;
                }
                let item = if id >= 0xf800 { query(pm, id).map_or(0, |mi| handle_of(mi.sub)) } else { id };
                user::send(h, W_MENUSELECT, item, (handle_of(pm) as u32) << 16 | flags);
            }
            1
        }
        WM_MENUEND => {
            if mp2 == bar {
                user::send(h, W_MENUSELECT, 0, 0xffff);
            }
            0
        }
        _ => 0,
    }
}

fn send_cmd(h: u16, id: u16, lp: u32) {
    user::send(h, user::W_COMMAND, id, lp);
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
    let bar = wnd(h).map_or(0, |w| pm_of(w.menu));
    bar != 0 && query(bar, cmd).is_some_and(|mi| mi.attr & MIA_DISABLED != 0)
}

/// TranslateAccelerator(hwnd, haccel, lpMsg)
fn translate_accel(h: u16, acc: u16, p: *const u8) -> bool {
    use crate::user::*;
    let Some((_mh, msg, wp, _lp)) = read_msg(p) else { return false };
    let Some(table) = m().accels.get((acc as usize).wrapping_sub(0x7004) / 4) else { return false };
    let down = |vk| key_state(vk) & 0x8000 != 0;
    let (shift, ctrl, alt) = (down(0x10), down(0x11), down(0x12));
    let hit = table.iter().find(|&&(f, key, _)| {
        if f & 1 != 0 {
            (msg == W_KEYDOWN || msg == W_SYSKEYDOWN) && key == wp && (f & 4 != 0) == shift && (f & 8 != 0) == ctrl && (f & 0x10 != 0) == alt
        } else {
            (msg == W_CHAR || msg == W_SYSCHAR) && key == wp && (f & 0x10 != 0) == (msg == W_SYSCHAR)
        }
    });
    let Some(&(_, _, cmd)) = hit else { return false };
    let t = top_level(h);
    let menu = wnd(t).map_or(0, |w| w.menu);
    if menu != 0 {
        user::send(t, W_INITMENU, menu, 0);
    }
    if !disabled(t, cmd) {
        send_cmd(h, cmd, 1 << 16);
    }
    true
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // LoadMenu(hinst, name)
        150 => {
            let (hi, n) = (a.w(), a.l());
            Name::of(n).map_or(0, |n| load(hi, &n)) as u32
        }
        // CreateMenu, CreatePopupMenu
        151 => create(true) as u32,
        415 => create(false) as u32,
        // DestroyMenu(hmenu)
        152 => {
            let h = a.w();
            let pm = pm_of(h);
            if pm == 0 {
                return Some(0);
            }
            forget_tree(pm);
            unsafe { WinDestroyWindow(pm) };
            1
        }
        // ChangeMenu: the old way; as AppendMenu/ModifyMenu.
        153 => {
            let (h, _cmd, text, id, fl) = (a.w(), a.w(), a.l(), a.w(), a.w());
            let t = far_str(text).unwrap_or(b"").to_vec();
            insert(h, -1, fl, id, &t) as u32
        }
        // CheckMenuItem(hmenu, id, flags)
        154 => {
            let (h, id, fl) = (a.w(), a.w(), a.w());
            set_attr(h, id, fl, MIA_CHECKED, fl & MF_CHECKED != 0)
        }
        // EnableMenuItem(hmenu, id, flags)
        155 => {
            let (h, id, fl) = (a.w(), a.w(), a.w());
            set_attr(h, id, fl, MIA_DISABLED, fl & (MF_GRAYED | MF_DISABLED) != 0)
        }
        // GetSystemMenu(hwnd, revert): not WIN16's.
        156 => {
            a.w();
            a.w();
            0
        }
        // GetMenu(hwnd)
        157 => wnd(a.w()).map_or(0, |w| w.menu as u32),
        // SetMenu(hwnd, hmenu)
        158 => {
            let (h, mm) = (a.w(), a.w());
            set_menu(h, mm) as u32
        }
        // GetSubMenu(hmenu, pos)
        159 => {
            let (h, pos) = (a.w(), a.w());
            let pm = pm_of(h);
            let id = send(pm, MM_ITEMIDFROMPOSITION, pos as u32, 0);
            if pm == 0 || id == MIT_NONE {
                return Some(0);
            }
            let mut mi = MenuItem::default();
            send(pm, MM_QUERYITEM, mp(id, 0), &mut mi as *mut MenuItem as u32);
            handle_of(mi.sub) as u32
        }
        // DrawMenuBar(hwnd)
        160 => {
            let h = a.w();
            let pm = wnd(h).map_or(0, |w| pm_of(w.menu));
            if pm != 0 {
                unsafe { WinInvalidateRect(pm, core::ptr::null(), 0) };
            }
            0
        }
        // GetMenuString(hmenu, id, buf, max, flags)
        161 => {
            let (h, which, buf, max, fl) = (a.w(), a.w(), a.p(), a.i(), a.w());
            let pm = pm_of(h);
            let Some(id) = item_id(pm, which, fl) else { return Some(0) };
            let mut b = [0u8; 256];
            send(pm, MM_QUERYITEMTEXT, mp(id as u32, 255), b.as_mut_ptr() as u32);
            let t = win_text(unsafe { crate::cstr(b.as_ptr()) });
            user::copy_out(&t, buf, max) as u32
        }
        // HiliteMenuItem(hwnd, hmenu, id, flags)
        162 => {
            a.w();
            a.w();
            a.w();
            a.w();
            1
        }
        // LoadMenuIndirect(lpTemplate)
        220 => {
            let p = a.p();
            if p.is_null() {
                return Some(0);
            }
            let t = unsafe { core::slice::from_raw_parts(p, 0x10000 - (p as usize & 0xffff)) };
            from_template(t) as u32
        }
        // GetMenuState(hmenu, id, flags)
        250 => {
            let (h, which, fl) = (a.w(), a.w(), a.w());
            let pm = pm_of(h);
            match item_id(pm, which, fl) {
                Some(id) => state(pm, id),
                None => 0xffff,
            }
        }
        // GetMenuItemCount(hmenu)
        263 => {
            let pm = pm_of(a.w());
            if pm == 0 { 0xffff } else { send(pm, MM_QUERYITEMCOUNT, 0, 0) }
        }
        // GetMenuItemID(hmenu, pos): -1 for a pop-up.
        264 => {
            let (h, pos) = (a.w(), a.w());
            let pm = pm_of(h);
            let id = send(pm, MM_ITEMIDFROMPOSITION, pos as u32, 0);
            if pm == 0 || id == MIT_NONE || id >= 0xf800 { 0xffff } else { id }
        }
        // IsMenu(hmenu)
        358 => (pm_of(a.w()) != 0) as u32,
        // InsertMenu(hmenu, pos, flags, id, text)
        410 => {
            let (h, pos, fl, id, text) = (a.w(), a.w(), a.w(), a.w(), a.l());
            let pm = pm_of(h);
            let at = if fl & MF_BYPOSITION != 0 {
                pos as i16
            } else {
                match send(pm, MM_ITEMPOSITIONFROMID, mp(pos as u32, 0), 0) {
                    MIT_NONE => -1,
                    p => p as i16,
                }
            };
            insert(h, at, fl, id, &item_text(fl, text)) as u32
        }
        // AppendMenu(hmenu, flags, id, text)
        411 => {
            let (h, fl, id, text) = (a.w(), a.w(), a.w(), a.l());
            insert(h, -1, fl, id, &item_text(fl, text)) as u32
        }
        // RemoveMenu, DeleteMenu(hmenu, pos, flags)
        412 | 413 => {
            let (h, pos, fl) = (a.w(), a.w(), a.w());
            remove(h, pos, fl, ord == 413) as u32
        }
        // ModifyMenu(hmenu, pos, flags, id, text)
        414 => {
            let (h, pos, fl, id, text) = (a.w(), a.w(), a.w(), a.w(), a.l());
            let pm = pm_of(h);
            let Some(old) = item_id(pm, pos, fl) else { return Some(0) };
            let at = send(pm, MM_ITEMPOSITIONFROMID, mp(old as u32, 0), 0) as i16;
            remove(h, old, 0, false);
            insert(h, at, fl & !MF_BYPOSITION, id, &item_text(fl, text)) as u32
        }
        // TrackPopupMenu(hmenu, flags, x, y, reserved, hwnd, lpRect)
        416 => {
            let (h, _fl, x, y, _r, hwnd, _rc) = (a.w(), a.w(), a.i(), a.i(), a.w(), a.w(), a.l());
            track_popup(h, x, y, hwnd) as u32
        }
        // GetMenuCheckMarkDimensions
        417 => 0x000e_000e,
        // SetMenuItemBitmaps
        418 => {
            a.w();
            a.w();
            a.w();
            a.w();
            a.w();
            1
        }
        // LoadAccelerators(hinst, name)
        177 => {
            let (_hi, n) = (a.w(), a.l());
            let Some(t) = Name::of(n).and_then(|n| res::find(res::RT_ACCELERATOR, &n)) else { return Some(0) };
            let mut v = Vec::new();
            for e in t.chunks(5) {
                if e.len() < 5 {
                    break;
                }
                v.push((e[0], res::w16(e, 1), res::w16(e, 3)));
                if e[0] & 0x80 != 0 {
                    break;
                }
            }
            let s = m();
            s.accels.push(v);
            0x7004 + 4 * (s.accels.len() as u32 - 1)
        }
        // TranslateAccelerator(hwnd, haccel, lpMsg)
        178 => {
            let (h, acc, p) = (a.w(), a.w(), a.p());
            translate_accel(h, acc, p) as u32
        }
        _ => return None,
    })
}

/// The text of a new item (nothing for separators, bitmaps and owner
/// drawn items).
fn item_text(fl: u16, text: u32) -> Vec<u8> {
    if fl & (MF_SEPARATOR | MF_BITMAP | MF_OWNERDRAW) != 0 {
        return Vec::new();
    }
    far_str(text).unwrap_or(b"").to_vec()
}

/// TrackPopupMenu: show the menu at (x, y) on the screen; its commands go
/// to the window; returns when the menu closes.
fn track_popup(menu: u16, x: i32, y: i32, hwnd: u16) -> bool {
    let pm = pm_of(menu);
    let Some(c) = wnd(hwnd).map(|w| w.client) else { return false };
    if pm == 0 {
        return false;
    }
    unsafe extern "C" {
        #[link_name = "PMWIN.937"]
        fn WinPopupMenu(parent: u32, owner: u32, menu: u32, x: i32, y: i32, id: u32, fs: u32) -> u32;
    }
    let sy = user::screen_size().1;
    // PU_KEYBOARD | PU_MOUSEBUTTON1
    if unsafe { WinPopupMenu(HWND_DESKTOP, c, pm, x, sy - y, 0, 0x0200 | 0x0400) } == 0 {
        return false;
    }
    let hab = crate::kernel::TASK.get().hab;
    while unsafe { WinIsWindowVisible(pm) } != 0 {
        let mut q = QMsg::default();
        if unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } == 0 {
            unsafe { WinPostQueueMsg(crate::kernel::TASK.get().hmq, WM_QUIT, q.mp1, q.mp2) };
            break;
        }
        unsafe { WinDispatchMsg(hab, &q) };
    }
    true
}
