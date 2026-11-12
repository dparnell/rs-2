//! Arranging a folder's icon view, as Warp 4's PMWP does (studied with
//! Ghidra: ProcessCnrViewMenuItems, CnrViewSetupUndoData,
//! AddCnrViewMenuItems; the menus from PMWPMRI's resource 101).
//!
//! The View menu's Arrange is a submenu with IBM's directions and ids:
//! Standard (733, as Arrange itself, 309: CM_ARRANGE standard), From top,
//! From left, From right, From bottom (734-737: the grid filled from that
//! edge), Perimeter (739: round the edges, then inwards), Selected
//! horizontal and Selected vertical (740, 741: the selected icons in a row
//! or a column from the first of them). Before an arrange or a sort the
//! view keeps where every icon was (IBM's undo data, one set per view);
//! the menu then offers Undo arrange (310) or Undo sort (319), which put
//! the icons back.

use alloc::vec::Vec;

use crate::obj::OPEN_CONTENTS;
use crate::pm::*;
use crate::view::{self, wins};

pub const WPMENUID_UNDOARRANGE: u32 = 310;
pub const WPMENUID_UNDOSORT: u32 = 319;
pub const WPMENUID_STANDARD: u32 = 733;
pub const WPMENUID_ARRANGETOP: u32 = 734;
pub const WPMENUID_ARRANGELEFT: u32 = 735;
pub const WPMENUID_ARRANGERIGHT: u32 = 736;
pub const WPMENUID_ARRANGEBOTTOM: u32 = 737;
pub const WPMENUID_PERIMETER: u32 = 739;
pub const WPMENUID_SELECTEDHORZ: u32 = 740;
pub const WPMENUID_SELECTEDVERT: u32 = 741;

/// A view's undo data: its client window, whether a sort made it, and
/// where each object's icon was.
struct Undo {
    client: u32,
    sort: bool,
    at: Vec<(usize, i32, i32)>,
}

static mut UNDO: Vec<Undo> = Vec::new();
/// The client window of the view whose menu is being made.
static mut MENU_CLIENT: u32 = 0;

#[allow(static_mut_refs)]
fn undo() -> &'static mut Vec<Undo> {
    unsafe { &mut UNDO }
}

/// The view a menu is about to be made for (wpDisplayMenu, a pull-down).
pub fn set_menu_client(client: u32) {
    unsafe { MENU_CLIENT = client };
}

/// Is the view of the menu an icon view (or not known)?
pub fn menu_view_is_icon() -> bool {
    view::find_client(unsafe { MENU_CLIENT }).is_none_or(|w| wins()[w].view == OPEN_CONTENTS)
}

/// The undo the view of the menu offers: Some(true) Undo sort,
/// Some(false) Undo arrange.
fn pending(folder: usize) -> Option<bool> {
    let client = unsafe { MENU_CLIENT };
    let w = view::find_client(client).filter(|&w| wins()[w].obj == folder)?;
    undo().iter().find(|u| u.client == wins()[w].client).map(|u| u.sort)
}

/// The Arrange submenu and the undo item, for a folder's view menu
/// (`icon`: the view is an icon view, where they act).
pub fn menu_items(folder: usize, icon: bool) -> Vec<Menu<'static>> {
    let mut v = alloc::vec![Menu::SubId(
        crate::menu::WPMENUID_ARRANGE as u16,
        b"~Arrange",
        alloc::vec![
            Menu::Item(WPMENUID_STANDARD as u16, b"~Standard", icon),
            Menu::Item(WPMENUID_ARRANGETOP as u16, b"From ~top", icon),
            Menu::Item(WPMENUID_ARRANGELEFT as u16, b"From ~left", icon),
            Menu::Item(WPMENUID_ARRANGERIGHT as u16, b"From ~right", icon),
            Menu::Item(WPMENUID_ARRANGEBOTTOM as u16, b"From ~bottom", icon),
            Menu::Item(WPMENUID_PERIMETER as u16, b"~Perimeter", icon),
            Menu::Item(WPMENUID_SELECTEDHORZ as u16, b"Selected ~horizontally", icon),
            Menu::Item(WPMENUID_SELECTEDVERT as u16, b"Selected ~vertically", icon),
        ],
    )];
    match pending(folder) {
        Some(true) => v.push(Menu::Item(WPMENUID_UNDOSORT as u16, b"~Undo sort", true)),
        Some(false) => v.push(Menu::Item(WPMENUID_UNDOARRANGE as u16, b"~Undo arrange", true)),
        None => {}
    }
    v
}

/// Remember where a view's icons are, before an arrange or a sort.
fn keep(w: usize, sort: bool) {
    if wins()[w].view != OPEN_CONTENTS {
        return;
    }
    let client = wins()[w].client;
    let at = wins()[w].recs.iter().filter(|r| r.obj != 0).map(|r| (r.obj, rd(r.rec, R_X) as i32, rd(r.rec, R_Y) as i32)).collect();
    undo().retain(|u| u.client != client);
    undo().push(Undo { client, sort, at });
}

/// A sort is about to change a folder's icon views.
pub fn before_sort(folder: usize) {
    for w in 0..wins().len() {
        if wins()[w].obj == folder {
            keep(w, true);
        }
    }
}

/// A view closes: its undo data goes.
pub fn forget(client: u32) {
    undo().retain(|u| u.client != client);
}

/// The grid of a view: the first column's x, the top row's y, the cell
/// size and how many columns and rows fit (as view.rs's slots).
fn grid(w: usize) -> (i32, i32, i32, i32, i32, i32) {
    let mut r = RectL::default();
    unsafe { WinQueryWindowRect(wins()[w].child, &mut r) };
    let (aw, ah) = (r.r.max(100), (r.t - 2).max(100));
    let (cw, ch) = view::cell(w);
    if wins()[w].desktop {
        ((cw - 32) / 2, ah - 12 - 32, cw, ch, (aw / cw).max(1), ((ah - 16) / ch).max(1))
    } else {
        (4 + (cw - 32) / 2, ah - 8 - 32, cw, ch, ((aw - 8) / cw).max(1), ((ah - 8) / ch).max(1))
    }
}

/// The grid cells (column, row; row 0 at the top) in the order a
/// direction fills them, for `n` icons. A grid too small grows (more
/// rows, or for the left and right more columns).
fn cells(dir: u32, n: i32, cols: i32, rows: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    match dir {
        WPMENUID_ARRANGELEFT | WPMENUID_ARRANGERIGHT => {
            let cols = cols.max((n + rows - 1) / rows);
            for k in 0..cols {
                let c = if dir == WPMENUID_ARRANGELEFT { k } else { cols - 1 - k };
                for r in 0..rows {
                    out.push((c, r));
                }
            }
        }
        WPMENUID_ARRANGEBOTTOM => {
            let rows = rows.max((n + cols - 1) / cols);
            for r in (0..rows).rev() {
                for c in 0..cols {
                    out.push((c, r));
                }
            }
        }
        WPMENUID_PERIMETER => {
            let rows = rows.max((n + cols - 1) / cols);
            let (mut l, mut t, mut rr, mut b) = (0, 0, cols - 1, rows - 1);
            while l <= rr && t <= b {
                for c in l..=rr {
                    out.push((c, t));
                }
                for r in t + 1..=b {
                    out.push((rr, r));
                }
                if t < b {
                    for c in (l..rr).rev() {
                        out.push((c, b));
                    }
                }
                if l < rr {
                    for r in (t + 1..b).rev() {
                        out.push((l, r));
                    }
                }
                l += 1;
                t += 1;
                rr -= 1;
                b -= 1;
            }
        }
        _ => {
            let rows = rows.max((n + cols - 1) / cols);
            for r in 0..rows {
                for c in 0..cols {
                    out.push((c, r));
                }
            }
        }
    }
    out
}

/// Arrange a view's icons (a View menu item; `id` the item).
pub fn arrange(w: usize, id: u32) {
    if wins()[w].view != OPEN_CONTENTS {
        return;
    }
    keep(w, false);
    if matches!(id, WPMENUID_STANDARD | crate::menu::WPMENUID_ARRANGE) {
        view::arrange(w);
        return;
    }
    let recs: Vec<u32> = wins()[w].recs.iter().filter(|r| r.obj != 0).map(|r| r.rec).collect();
    let (x0, y0, cw, ch, cols, rows) = grid(w);
    if matches!(id, WPMENUID_SELECTEDHORZ | WPMENUID_SELECTEDVERT) {
        // The selected icons, in the view's order, from where the first
        // of them is.
        let sel: Vec<u32> = recs.iter().copied().filter(|&r| rd(r, R_ATTR) & CRA_SELECTED != 0).collect();
        let Some(&first) = sel.first() else { return };
        let (x, y) = (rd(first, R_X) as i32, rd(first, R_Y) as i32);
        for (i, &r) in sel.iter().enumerate() {
            let i = i as i32;
            let (nx, ny) = if id == WPMENUID_SELECTEDHORZ { (x + i * cw, y) } else { (x, y - i * ch) };
            wr(r, R_X, nx.max(0) as u32);
            wr(r, R_Y, ny as u32);
        }
    } else {
        let order = cells(id, recs.len() as i32, cols, rows);
        for (&r, &(c, row)) in recs.iter().zip(order.iter()) {
            wr(r, R_X, (x0 + c * cw) as u32);
            wr(r, R_Y, (y0 - row * ch) as u32);
        }
    }
    send(wins()[w].child, CM_INVALIDATERECORD, 0, mp(0, CMA_REPOSITION));
    view::save_positions(w);
}

/// Undo arrange, Undo sort: the icons go back where they were.
pub fn undo_last(w: usize) {
    let client = wins()[w].client;
    let Some(i) = undo().iter().position(|u| u.client == client) else { return };
    let u = undo().remove(i);
    for r in wins()[w].recs.iter() {
        if let Some(&(_, x, y)) = u.at.iter().find(|a| a.0 == r.obj) {
            wr(r.rec, R_X, x as u32);
            wr(r.rec, R_Y, y as u32);
        }
    }
    send(wins()[w].child, CM_INVALIDATERECORD, 0, mp(0, CMA_REPOSITION));
    view::save_positions(w);
}

/// A View menu item of the arrange family, chosen in the view whose
/// frame is `frame` (else every icon view of the folder).
pub fn selected(folder: usize, frame: u32, id: u32) -> bool {
    let arrange_ids = [
        crate::menu::WPMENUID_ARRANGE,
        WPMENUID_STANDARD,
        WPMENUID_ARRANGETOP,
        WPMENUID_ARRANGELEFT,
        WPMENUID_ARRANGERIGHT,
        WPMENUID_ARRANGEBOTTOM,
        WPMENUID_PERIMETER,
        WPMENUID_SELECTEDHORZ,
        WPMENUID_SELECTEDVERT,
    ];
    let undo_ids = [WPMENUID_UNDOARRANGE, WPMENUID_UNDOSORT];
    if !arrange_ids.contains(&id) && !undo_ids.contains(&id) {
        return false;
    }
    let views: Vec<usize> = match wins().iter().position(|w| w.frame == frame && w.obj == folder) {
        Some(w) => alloc::vec![w],
        None => (0..wins().len()).filter(|&w| wins()[w].obj == folder && wins()[w].view == OPEN_CONTENTS).collect(),
    };
    for w in views {
        if undo_ids.contains(&id) {
            undo_last(w);
        } else {
            arrange(w, id);
        }
    }
    true
}
