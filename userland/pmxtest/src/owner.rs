//! Owner drawing: a list box (LS_OWNERDRAW), a pop-up menu item
//! (MIS_OWNERDRAW), a user button (BS_USERBUTTON), a value set
//! (VS_OWNERDRAW, VIA_OWNERDRAW), a slider (SLS_OWNERDRAW) and a notebook
//! tab without text: what the owner is asked and what it draws.

use alloc::vec::Vec;
use core::ptr::null;

use crate::pm::*;
use crate::{check, pump, pump_for};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct OwnerItem {
    hwnd: u32,
    hps: u32,
    fs_state: u32,
    fs_attribute: u32,
    fs_state_old: u32,
    fs_attribute_old: u32,
    rcl: RectL,
    id_item: i32,
    h_item: u32,
}

unsafe extern "C" {
    #[link_name = "PMWIN.743"]
    fn WinFillRect(hps: u32, r: *const RectL, c: i32) -> u32;
    #[link_name = "PMWIN.892"]
    fn WinUpdateWindow(h: u32) -> u32;
    #[link_name = "PMWIN.937"]
    fn WinPopupMenu(parent: u32, owner: u32, menu: u32, x: i32, y: i32, id: u32, fs: u32) -> u32;
    #[link_name = "PMGPI.476"]
    fn GpiQueryPel(hps: u32, p: *const i32) -> i32;
    #[link_name = "PMWIN.907"]
    fn WinCreateMenu(parent: u32, pmt: *const u8) -> u32;
    #[link_name = "PMWIN.834"]
    fn WinQueryWindow(h: u32, cmd: i32) -> u32;
    #[link_name = "PMWIN.775"]
    fn WinIsWindowVisible(h: u32) -> u32;
}

const WM_CONTROL: u32 = 0x0030;
const WM_DRAWITEM: u32 = 0x0036;
const WM_MEASUREITEM: u32 = 0x0037;
const CLR_RED: i32 = 2;
const CLR_BLUE: i32 = 1;

const ID_LIST: u32 = 100;
const ID_MENU: u32 = 200;
const ID_ITEM: u32 = 201;
const ID_BUTTON: u32 = 300;
const ID_VALUESET: u32 = 400;
const ID_SLIDER: u32 = 500;
const ID_NOTEBOOK: u32 = 600;

struct Seen {
    measured: Vec<(u32, u32)>,
    drawn: Vec<(u32, OwnerItem)>,
    buttons: Vec<(u32, u32)>,
}

static mut SEEN: Seen = Seen { measured: Vec::new(), drawn: Vec::new(), buttons: Vec::new() };

fn seen() -> &'static mut Seen {
    unsafe { &mut *core::ptr::addr_of_mut!(SEEN) }
}

/// The client's part.
pub fn client_msg(_h: u32, msg: u32, mp1: u32, mp2: u32) -> Option<u32> {
    match msg {
        WM_MEASUREITEM if mp1 & 0xffff == ID_LIST => {
            seen().measured.push((mp1 & 0xffff, mp2));
            Some(22)
        }
        WM_MEASUREITEM if mp1 & 0xffff == ID_MENU => {
            let oi = unsafe { &mut *(mp2 as *mut OwnerItem) };
            seen().measured.push((mp1 & 0xffff, oi.h_item));
            oi.rcl.right = 120;
            oi.rcl.top = 30;
            Some(0)
        }
        WM_DRAWITEM => {
            let oi = unsafe { &mut *(mp2 as *mut OwnerItem) };
            let id = mp1 & 0xffff;
            seen().drawn.push((id, *oi));
            // The list box's item 1 is left to the list box.
            if id == ID_LIST && oi.id_item == 1 {
                return Some(0);
            }
            unsafe { WinFillRect(oi.hps, &oi.rcl, if id == ID_LIST { CLR_RED } else { CLR_BLUE }) };
            // Highlighting done here too.
            oi.fs_state = 0;
            oi.fs_state_old = 0;
            oi.fs_attribute &= !0x8000;
            oi.fs_attribute_old &= !0x8000;
            Some(1)
        }
        WM_CONTROL if mp1 & 0xffff == ID_BUTTON && mp1 >> 16 == 3 => {
            // BN_PAINT: USERBUTTON hwnd, hps, fsState, fsStateOld.
            let ub = mp2 as *const u32;
            let (st, old) = unsafe { (*ub.add(2), *ub.add(3)) };
            seen().buttons.push((st, old));
            Some(0)
        }
        _ => None,
    }
}

fn create(parent: u32, class: u32, style: u32, (x, y, cx, cy): (i32, i32, i32, i32), id: u32, ctl: *const u8) -> u32 {
    unsafe { WinCreateWindow(parent, class as *const u8, b"\0".as_ptr(), 0x8000_0000 | style, x, y, cx, cy, parent, 3, id, ctl, null()) }
}

fn drawn(id: u32) -> Vec<OwnerItem> {
    seen().drawn.iter().filter(|d| d.0 == id).map(|d| d.1).collect()
}

pub fn owner_checks(hab: u32, client: u32) {
    let s = seen();
    // The list box.
    let lb = create(client, 0xffff_0007, 0x0002, (10, 10, 180, 100), ID_LIST, null());
    check("LS_OWNERDRAW: WM_MEASUREITEM when the list box is made", s.measured.contains(&(ID_LIST, 0)));
    for (i, t) in [b"one\0", b"two\0", b"six\0"].iter().enumerate() {
        unsafe {
            WinSendMsg(lb, 0x0161, 0xffff, t.as_ptr() as u32);
            WinSendMsg(lb, 0x0169, i as u32, 0x100 + i as u32);
        }
    }
    unsafe { WinSendMsg(lb, 0x0164, 1, 1) };
    s.drawn.clear();
    unsafe { WinUpdateWindow(lb) };
    let d = drawn(ID_LIST);
    let item = |i: i32| d.iter().find(|o| o.id_item == i).copied();
    check(
        "WM_DRAWITEM for each item (the height measured, the handle, the selection)",
        d.len() == 3 && d.iter().all(|o| o.rcl.top - o.rcl.bottom == 22 && o.hwnd == lb) && item(1).is_some_and(|o| o.fs_state == 1 && o.fs_state_old == 1 && o.h_item == 0x101) && item(0).is_some_and(|o| o.fs_state == 0),
    );
    let hps = unsafe { WinGetPS(lb) };
    let at = |o: Option<OwnerItem>| {
        o.map_or(-2, |o| {
            let p = [(o.rcl.left + o.rcl.right) / 2, (o.rcl.bottom + o.rcl.top) / 2];
            unsafe { GpiQueryPel(hps, p.as_ptr()) }
        })
    };
    let (p0, p1) = (at(item(0)), at(item(1)));
    unsafe { WinReleasePS(hps) };
    check("what the owner draws stays, an item it leaves is drawn by the list box", p0 == CLR_RED && p1 != CLR_RED);
    unsafe { WinDestroyWindow(lb) };

    // A pop-up menu with an owner-drawn item.
    let menu = create(HWND_DESKTOP, 0xffff_0004, 0, (0, 0, 0, 0), ID_MENU, null());
    unsafe { crate::pm::WinSetOwner(menu, client) };
    // MENUITEM: iPosition, afStyle (MIS_OWNERDRAW), afAttribute, id,
    // hwndSubMenu, hItem.
    let mut mi = [0u8; 16];
    mi[0..2].copy_from_slice(&(-1i16).to_le_bytes());
    mi[2..4].copy_from_slice(&8u16.to_le_bytes());
    mi[6..8].copy_from_slice(&(ID_ITEM as u16).to_le_bytes());
    mi[12..16].copy_from_slice(&0x5555u32.to_le_bytes());
    unsafe { WinSendMsg(menu, 0x0180, mi.as_ptr() as u32, 0) };
    s.measured.clear();
    s.drawn.clear();
    let ok = unsafe { WinPopupMenu(client, client, menu, 20, 20, ID_ITEM, 0x0200 | 0x0001) } != 0;
    pump_for(hab, 200);
    let m = drawn(ID_MENU);
    check("MIS_OWNERDRAW: WM_MEASUREITEM (hItem) when the menu pops up", ok && s.measured.contains(&(ID_MENU, 0x5555)));
    check("MIS_OWNERDRAW: WM_DRAWITEM at the measured size", m.iter().any(|o| o.id_item == ID_ITEM as i32 && o.h_item == 0x5555 && o.rcl.top - o.rcl.bottom == 30 && o.rcl.right - o.rcl.left >= 120 && o.fs_state & 8 != 0));
    unsafe {
        WinSendMsg(menu, 0x0186, 1, 0);
        WinDestroyWindow(menu);
    }
    pump(hab);

    // WinCreateMenu without a template: an empty menu to fill
    // (VisualAge Smalltalk builds its menus so).
    let m = unsafe { WinCreateMenu(client, null()) };
    let n0 = unsafe { WinSendMsg(m, 0x0184, 0, 0) }; // MM_QUERYITEMCOUNT
    let mut mi = [0u8; 16];
    mi[0..2].copy_from_slice(&(-1i16).to_le_bytes());
    mi[2..4].copy_from_slice(&1u16.to_le_bytes()); // MIS_TEXT
    mi[6..8].copy_from_slice(&(ID_ITEM as u16).to_le_bytes());
    unsafe { WinSendMsg(m, 0x0180, mi.as_ptr() as u32, b"~File\0".as_ptr() as u32) };
    let n1 = unsafe { WinSendMsg(m, 0x0184, 0, 0) };
    check("WinCreateMenu without a template", m != 0 && n0 == 0 && n1 == 1);
    // A pull-down made with the menu as its parent (VisualAge Smalltalk)
    // drops down on the desktop.
    let sub = unsafe { WinCreateMenu(m, null()) };
    unsafe { WinSendMsg(sub, 0x0180, mi.as_ptr() as u32, b"~Open\0".as_ptr() as u32) };
    let mut si = [0u8; 16];
    si[0..2].copy_from_slice(&0i16.to_le_bytes());
    si[2..4].copy_from_slice(&(1u16 | 0x10).to_le_bytes()); // MIS_TEXT | MIS_SUBMENU
    si[6..8].copy_from_slice(&(ID_ITEM as u16 + 1).to_le_bytes());
    si[8..12].copy_from_slice(&sub.to_le_bytes());
    unsafe { WinSendMsg(m, 0x0180, si.as_ptr() as u32, b"~Tools\0".as_ptr() as u32) };
    unsafe { WinSendMsg(m, 0x0185, 1, 0) }; // MM_STARTMENUMODE, the submenu shown
    let on_desktop = unsafe { WinQueryWindow(sub, 5) == 1 && WinIsWindowVisible(sub) != 0 };
    unsafe { WinSendMsg(m, 0x0186, 1, 0) }; // MM_ENDMENUMODE
    pump(hab);
    check("a submenu made as a child of its menu drops down on the desktop", sub != 0 && on_desktop);
    unsafe { WinDestroyWindow(m) };

    // A user button.
    s.buttons.clear();
    let b = create(client, 0xffff_0003, 7, (200, 10, 80, 30), ID_BUTTON, null());
    unsafe { WinUpdateWindow(b) };
    let first = s.buttons.first().copied();
    unsafe {
        WinSendMsg(b, 0x0123, 1, 0);
        WinUpdateWindow(b);
    }
    let last = s.buttons.last().copied();
    check("BS_USERBUTTON: WM_CONTROL BN_PAINT with the state (BDS_HILITED, the old state)", first == Some((0, 0)) && last == Some((0x100, 0)));
    unsafe { WinDestroyWindow(b) };

    // A value set: VS_OWNERDRAW background, a VIA_OWNERDRAW item.
    let vscd: [u16; 4] = [8, 0, 1, 2];
    s.drawn.clear();
    let vs = create(client, 0xffff_0027, 0x0200 | 0x0004, (10, 120, 160, 60), ID_VALUESET, vscd.as_ptr() as *const u8);
    unsafe {
        WinSendMsg(vs, 0x037b, 1 | 2 << 16, 0x0020 | 1 << 16);
        WinUpdateWindow(vs);
    }
    let v = drawn(ID_VALUESET);
    check("VS_OWNERDRAW: the background (VDA_BACKGROUND)", v.iter().any(|o| o.id_item == 4 && o.h_item == 0));
    check("VIA_OWNERDRAW: the item (VDA_ITEM, its row and column)", v.iter().any(|o| o.id_item == 1 && o.h_item == (1 | 2 << 16) && o.fs_attribute & 0x20 != 0));
    unsafe { WinDestroyWindow(vs) };

    // A slider.
    let sld: [u16; 6] = [12, 0, 5, 0, 0, 0];
    s.drawn.clear();
    let sl = create(client, 0xffff_0026, 0x0040, (10, 190, 200, 50), ID_SLIDER, sld.as_ptr() as *const u8);
    unsafe { WinUpdateWindow(sl) };
    let parts: Vec<i32> = drawn(ID_SLIDER).iter().map(|o| o.id_item).collect();
    check("SLS_OWNERDRAW: background, shaft and arm (SDA_*)", [3, 2, 4].iter().all(|p| parts.contains(p)));
    unsafe { WinDestroyWindow(sl) };

    // A notebook tab without text.
    s.drawn.clear();
    let nb = create(client, 0xffff_0028, 0, (220, 60, 170, 180), ID_NOTEBOOK, null());
    let page = unsafe { WinSendMsg(nb, 0x0355, 0, 0x0040 | 0x0100 | 2 << 16) };
    unsafe { WinUpdateWindow(nb) };
    check("a notebook tab without text is the owner's (BKA_TAB, the page ID)", page != 0 && drawn(ID_NOTEBOOK).iter().any(|o| o.id_item == 4 && o.h_item == page && o.fs_attribute & 0x40 != 0));
    unsafe { WinDestroyWindow(nb) };
}
