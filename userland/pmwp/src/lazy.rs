//! Pickup and Drop (lazy drag), as Warp 4's PMWP offers them (studied
//! with Ghidra: ModifyLazyDragMenuItems, CnrOwner_InitPickup,
//! QueryDropOps, CnrOwner_DropNotify, CancelLazyDragIfSourceClosing; the
//! items of PMWPMRI's menus 101, 719 and 924).
//!
//! An object's pop-up menu has Pickup (188): the object (with the others
//! selected, when it is one of them) is picked up with PMDRAG's
//! DrgLazyDrag. Its records show halftoned (CRA_PICKED) and the user goes
//! on working. While objects are picked up, menus of objects and folders
//! that take them have Drop (189), a submenu with the operations the
//! target allows (Copy 190, Move 191, Create shadow 192; the default one
//! marked), each asked of the target's wpDragOver as IBM's QueryDropOps
//! does, and Cancel drag (193); a target that takes none has Cancel drag
//! alone. Choosing an operation calls DrgLazyDrop: the view gets DM_DROP
//! and the objects go through the target's wpDragOver and wpDrop as in a
//! drag with the mouse. Either way the source view gets DM_DROPNOTIFY and
//! puts the objects down. The folder menu bar's Folder pull-down has
//! Pickup and Drop for the folder itself. A view that closes with objects
//! picked up from it cancels the drag.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::obj;
use crate::pm::*;
use crate::view::{self, wins};

unsafe extern "C" {
    #[link_name = "PMDRAG.67"]
    fn DrgLazyDrag(src: u32, pdinfo: u32, pdimg: *const u8, cdimg: u32, rsvd: u32) -> u32;
    #[link_name = "PMDRAG.68"]
    fn DrgCancelLazyDrag() -> u32;
    #[link_name = "PMDRAG.69"]
    fn DrgLazyDrop(target: u32, op: u32, ptl: *const PointL) -> u32;
    #[link_name = "PMDRAG.70"]
    fn DrgQueryDraginfoPtr(rsvd: u32) -> u32;
    #[link_name = "PMDRAG.73"]
    fn DrgQueryDragStatus() -> u32;
}

pub const WPMENUID_PICKUP: u32 = 188;
pub const WPMENUID_PUTDOWN: u32 = 189;
pub const WPMENUID_PUTDOWN_COPY: u32 = 190;
pub const WPMENUID_PUTDOWN_MOVE: u32 = 191;
pub const WPMENUID_PUTDOWN_LINK: u32 = 192;
pub const WPMENUID_PUTDOWN_CANCEL: u32 = 193;

const DGS_LAZYDRAGINPROGRESS: u32 = 0x0002;
const CRA_PICKED: u32 = 0x0200;
const OBJSTYLE_NODRAG: u32 = 0x0100;
const DO_DEFAULT: u32 = 0xbffe;
pub const DM_DROPNOTIFY: u32 = 0x031e;
/// DRAGINFO: usOperation, hwndSource.
const DI_OP: usize = 6;
const DI_SRC: usize = 8;

/// The objects this process has picked up: the view they came from and
/// the objects.
static mut PICKED: Option<(u32, Vec<usize>)> = None;
/// What a Drop chosen from a menu drops on (the object, else the view's
/// folder), until the view's DM_DROP.
static mut DROP_ON: usize = 0;

#[allow(static_mut_refs)]
fn picked() -> &'static mut Option<(u32, Vec<usize>)> {
    unsafe { &mut PICKED }
}

/// The DRAGINFO of the lazy drag in progress (in any process).
fn pending() -> u32 {
    if unsafe { DrgQueryDragStatus() } & DGS_LAZYDRAGINPROGRESS == 0 {
        return 0;
    }
    unsafe { DrgQueryDraginfoPtr(0) }
}

fn set_op(pdinfo: u32, op: u32) {
    unsafe { core::ptr::write_unaligned((pdinfo as usize + DI_OP) as *mut u16, op as u16) };
}

fn op_of(pdinfo: u32) -> u32 {
    unsafe { core::ptr::read_unaligned((pdinfo as usize + DI_OP) as *const u16) as u32 }
}

/// The view a menu acts in: the one whose frame is `frame`, else one
/// showing `o` (or its folder).
fn view_for(o: usize, frame: u32) -> Option<usize> {
    if let Some(w) = wins().iter().position(|w| w.frame == frame && w.view != obj::OPEN_SETTINGS) {
        return Some(w);
    }
    let f = obj::folder_of(o);
    wins().iter().position(|w| w.view != obj::OPEN_SETTINGS && (w.recs.iter().any(|r| r.obj == o) || w.obj == f || w.obj == o))
}

/// The operations a target takes of the objects picked up (IBM's
/// QueryDropOps): each asked of its wpDragOver; the default first.
fn drop_ops(target: usize, cnr: u32) -> (Vec<u32>, u32) {
    let pdinfo = pending();
    if pdinfo == 0 || target == 0 || unsafe { DrgAccessDraginfo(pdinfo) } == 0 {
        return (Vec::new(), 0);
    }
    let saved = op_of(pdinfo);
    let ask = |op: u32| {
        set_op(pdinfo, op);
        let r = obj::call(m::WPObject::wpDragOver, &[target, cnr as usize, pdinfo as usize]) as u32;
        (r & 0xffff == DOR_DROP, r >> 16)
    };
    let (_, default) = ask(DO_DEFAULT);
    let ops = [DO_COPY, DO_MOVE, DO_LINK].into_iter().filter(|&op| ask(op).0).collect();
    set_op(pdinfo, saved);
    unsafe { DrgFreeDraginfo(pdinfo) };
    (ops, default)
}

/// The lazy drag items of a menu: for an object (`pickup`: it can be
/// picked up from here) or a folder's open view (`pickup` false).
pub fn menu_items(target: usize, pickup: bool, frame: u32) -> Vec<Menu<'static>> {
    let mut v = Vec::new();
    let pdinfo = pending();
    let style = obj::call(m::WPObject::wpQueryStyle, &[target]) as u32;
    let is_picked = picked().as_ref().is_some_and(|p| p.1.contains(&target));
    if pickup && style & OBJSTYLE_NODRAG == 0 && !is_picked {
        // Objects picked up elsewhere (another view, another program)
        // must be put down first.
        let ours = picked().as_ref().map(|p| p.0);
        let here = view_for(target, frame).map(|w| wins()[w].client);
        let ok = pdinfo == 0 || (ours.is_some() && ours == here);
        v.push(Menu::Item(WPMENUID_PICKUP as u16, b"P~ick up", ok));
    }
    if pdinfo != 0 {
        let cnr = view_for(target, frame).map_or(0, |w| wins()[w].child);
        let (ops, default) = if is_picked { (Vec::new(), 0) } else { drop_ops(target, cnr) };
        if ops.is_empty() {
            v.push(Menu::Item(WPMENUID_PUTDOWN_CANCEL as u16, b"Cancel dra~g", true));
        } else {
            let mut sub = Vec::new();
            for op in ops {
                let (id, t): (u32, &'static [u8]) = match op {
                    DO_COPY => (WPMENUID_PUTDOWN_COPY, b"~Copy"),
                    DO_MOVE => (WPMENUID_PUTDOWN_MOVE, b"~Move"),
                    _ => (WPMENUID_PUTDOWN_LINK, b"Create ~shadow"),
                };
                // The default operation is checked.
                sub.push(Menu::Check(id as u16, t, op == default));
            }
            sub.push(Menu::Item(WPMENUID_PUTDOWN_CANCEL as u16, b"Cancel dra~g", true));
            v.push(Menu::SubId(WPMENUID_PUTDOWN as u16, b"D~rop", sub));
        }
    }
    v
}

/// The objects to pick up from a view: `o` and, when it is selected,
/// the others selected.
fn pickup(w: usize, o: usize) {
    let mut list = if view::record_of(w, o) != 0 { view::selected(w, o) } else { alloc::vec![o] };
    list.retain(|&x| obj::call(m::WPObject::wpQueryStyle, &[x]) as u32 & OBJSTYLE_NODRAG == 0);
    let client = wins()[w].client;
    // More objects picked up in the same view join those there.
    if let Some((c, old)) = picked().take() {
        if c == client && pending() != 0 {
            for x in old {
                if !list.contains(&x) {
                    list.push(x);
                }
            }
            unsafe { DrgCancelLazyDrag() };
        } else {
            *picked() = Some((c, old));
        }
    }
    if list.is_empty() || pending() != 0 {
        return;
    }
    let pdinfo = crate::dnd::make_draginfo(w, &list);
    if pdinfo == 0 {
        return;
    }
    let icon = if list.len() > 1 { unsafe { WinQuerySysPointer(HWND_DESKTOP, SPTR_MULTFILE, 0) } } else { crate::templates::view_icon(list[0]) };
    // DRAGIMAGE: cb, cptl, hImage, sizlStretch, fl, cxOffset, cyOffset.
    let mut img = [0u8; 24];
    img[0..2].copy_from_slice(&24u16.to_le_bytes());
    img[4..8].copy_from_slice(&icon.to_le_bytes());
    img[16..20].copy_from_slice(&DRG_ICON.to_le_bytes());
    if unsafe { DrgLazyDrag(client, pdinfo, img.as_ptr(), 1, 0) } == 0 {
        unsafe {
            DrgDeleteDraginfoStrHandles(pdinfo);
            DrgFreeDraginfo(pdinfo);
        }
        return;
    }
    emphasis(w, &list, true);
    *picked() = Some((client, list));
}

fn emphasis(w: usize, list: &[usize], on: bool) {
    let cnr = wins()[w].child;
    for &o in list {
        let r = view::record_of(w, o);
        if r != 0 {
            send(cnr, CM_SETRECORDEMPHASIS, r, mp(on as u32, CRA_PICKED));
        }
    }
}

/// A lazy drag menu item chosen for `o` (an object, or the folder of the
/// view whose frame is `frame`).
pub fn selected(o: usize, frame: u32, id: u32) -> bool {
    match id {
        WPMENUID_PICKUP => {
            if let Some(w) = view_for(o, frame) {
                pickup(w, o);
            }
        }
        WPMENUID_PUTDOWN_CANCEL => {
            if pending() != 0 {
                unsafe { DrgCancelLazyDrag() };
            }
        }
        WPMENUID_PUTDOWN_COPY | WPMENUID_PUTDOWN_MOVE | WPMENUID_PUTDOWN_LINK => {
            let op = match id {
                WPMENUID_PUTDOWN_COPY => DO_COPY,
                WPMENUID_PUTDOWN_MOVE => DO_MOVE,
                _ => DO_LINK,
            };
            let Some(w) = view_for(o, frame) else { return true };
            if pending() == 0 {
                return true;
            }
            unsafe { DROP_ON = o };
            let p = pointer_pos();
            unsafe { DrgLazyDrop(wins()[w].client, op, &p) };
            unsafe { DROP_ON = 0 };
        }
        _ => return false,
    }
    true
}

/// DM_DROP to a view's client window: a Drop chosen from a menu.
pub fn client_drop(client: u32, pdinfo: u32) -> u32 {
    let Some(w) = view::find_client(client) else { return 0 };
    if pdinfo == 0 || unsafe { DrgAccessDraginfo(pdinfo) } == 0 {
        return 0;
    }
    let on = unsafe { DROP_ON };
    let to = if on != 0 { on } else { wins()[w].obj };
    crate::dnd::drop_to(w, to, pdinfo);
    0
}

/// DM_DROPNOTIFY to a view's client: the objects it had picked up are
/// down (dropped on `target`, or the drag was cancelled).
pub fn drop_notify(client: u32, pdinfo: u32, target: u32) -> u32 {
    if let Some((c, list)) = picked().take() {
        if c == client {
            if let Some(w) = view::find_client(client) {
                emphasis(w, &list, false);
            }
        } else {
            *picked() = Some((c, list));
        }
    }
    if pdinfo != 0 && unsafe { core::ptr::read_unaligned((pdinfo as usize + DI_SRC) as *const u32) } == client {
        unsafe {
            if target == 0 {
                DrgDeleteDraginfoStrHandles(pdinfo);
            }
            DrgFreeDraginfo(pdinfo);
        }
    }
    0
}

/// A view is closing: objects picked up from it are put down.
pub fn view_closing(client: u32) {
    if picked().as_ref().is_some_and(|p| p.0 == client) && pending() != 0 {
        unsafe { DrgCancelLazyDrag() };
    }
}
