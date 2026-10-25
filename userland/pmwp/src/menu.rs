//! Pop-up menus, as the Workplace Shell makes them: wpDisplayMenu builds
//! an object's menu from what wpFilterPopupMenu allows, lets the class
//! add its items (wpModifyPopupMenu, which calls wpInsertPopupMenuItems
//! with menus from its DLL) and shows it; the choice goes to
//! wpMenuItemSelected. The items have IBM's ids (WPMENUID_*), which
//! classes compiled for IBM's shell know.

use alloc::vec::Vec;

use crate::classes_gen::m;
use crate::methods;
use crate::obj::{self, OBJSTYLE_NOCOPY, OBJSTYLE_NODELETE, OBJSTYLE_NOLINK, OBJSTYLE_NOMOVE, OBJSTYLE_NOSETTINGS};
use crate::obj::{OPEN_CONTENTS, OPEN_DEFAULT, OPEN_DETAILS, OPEN_RUNNING, OPEN_SETTINGS, OPEN_TREE};
use crate::pm::*;
use crate::view;
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMWIN.778"]
    fn WinLoadMenu(owner: u32, hmod: u32, id: u32) -> u32;
}

pub const MENU_OBJECTPOPUP: u32 = 1;
pub const MENU_OPENVIEWPOPUP: u32 = 2;

pub const CTXT_NEW: u32 = 0x0001;
pub const CTXT_OPEN: u32 = 0x0002;
pub const CTXT_SETTINGS: u32 = 0x0010;
pub const CTXT_HELP: u32 = 0x0040;
pub const CTXT_DELETE: u32 = 0x0080;
pub const CTXT_COPY: u32 = 0x0100;
pub const CTXT_MOVE: u32 = 0x0200;
pub const CTXT_SHADOW: u32 = 0x0400;
pub const CTXT_PROGRAM: u32 = 0x0800;
pub const CTXT_ICON: u32 = 0x1000;
pub const CTXT_TREE: u32 = 0x2000;
pub const CTXT_DETAILS: u32 = 0x4000;
pub const CTXT_FIND: u32 = 0x008000;
pub const CTXT_ARRANGE: u32 = 0x020000;
pub const CTXT_SORT: u32 = 0x040000;
pub const CTXT_REFRESH: u32 = 0x400000;

pub const WPMENUID_OPEN: u32 = 1;
pub const WPMENUID_FIND: u32 = 8;
pub const WPMENUID_SORT: u32 = 5;
pub const WPMENUID_CREATEANOTHER: u32 = 101;
pub const WPMENUID_MOVE: u32 = 107;
pub const WPMENUID_COPY: u32 = 108;
pub const WPMENUID_DELETE: u32 = 109;
pub const WPMENUID_PROPERTIES: u32 = 112;
pub const WPMENUID_TREE: u32 = 123;
pub const WPMENUID_PROGRAM: u32 = 132;
pub const WPMENUID_ICON: u32 = 303;
pub const WPMENUID_DETAILS: u32 = 304;
pub const WPMENUID_ARRANGE: u32 = 309;
pub const WPMENUID_CREATESHADOW: u32 = 316;
pub const WPMENUID_REFRESH: u32 = 503;
pub const WPMENUID_SHUTDOWN: u32 = 704;
pub const WPMENUID_WINDOWLIST: u32 = 728;
/// rs2's own: rename in place (handled by the view).
pub const MENUID_RENAME: u32 = 0x6400;
const WPMENUID_PRINT: u32 = 3;
const CTXT_PRINT: u32 = 0x0020;

const MM_INSERTITEM: u32 = 0x0180;
const MM_QUERYITEM: u32 = 0x0182;
const MM_QUERYITEMCOUNT: u32 = 0x0184;
const MM_QUERYITEMTEXT: u32 = 0x018b;
const MM_QUERYITEMTEXTLENGTH: u32 = 0x018c;
const MM_ITEMIDFROMPOSITION: u32 = 0x0190;

/// wpFilterPopupMenu: what the menu offers (less what the object's
/// style forbids).
extern "C" fn wpFilterPopupMenu(o: usize, flags: u32, _cnr: u32, _multi: u32) -> u32 {
    let style = obj::call(m::WPObject::wpQueryStyle, &[o]) as u32;
    let mut f = flags;
    for (bit, ctxt) in [
        (OBJSTYLE_NODELETE, CTXT_DELETE),
        (OBJSTYLE_NOCOPY, CTXT_COPY),
        (OBJSTYLE_NOMOVE, CTXT_MOVE),
        (OBJSTYLE_NOLINK, CTXT_SHADOW),
        (OBJSTYLE_NOSETTINGS, CTXT_SETTINGS),
    ] {
        if style & bit != 0 {
            f &= !ctxt;
        }
    }
    f
}

extern "C" fn wpModifyPopupMenu(_o: usize, _menu: u32, _cnr: u32, _pos: u32) -> u32 {
    1
}

/// MENUITEM: iPosition, afStyle, afAttribute, id, hwndSubMenu, hItem.
#[repr(C)]
#[derive(Default)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    item: u32,
}

/// wpInsertPopupMenuItems: the items of a menu in a module, put in the
/// menu at a position (or in the submenu `sub_id` of the menu).
extern "C" fn wpInsertPopupMenuItems(_o: usize, menu: u32, pos: u32, hmod: u32, menu_id: u32, sub_id: u32) -> u32 {
    let from = unsafe { WinLoadMenu(HWND_DESKTOP, hmod, menu_id) };
    if from == 0 {
        return 0;
    }
    let mut target = menu;
    if sub_id != 0 {
        let mut mi = MenuItem::default();
        if send(menu, MM_QUERYITEM, mp(sub_id, 1), &mut mi as *mut MenuItem as u32) != 0 && mi.sub != 0 {
            target = mi.sub;
        }
    }
    let n = send(from, MM_QUERYITEMCOUNT, 0, 0);
    let mut at = pos as i16;
    for i in 0..n {
        let id = send(from, MM_ITEMIDFROMPOSITION, i, 0) & 0xffff;
        let mut mi = MenuItem::default();
        send(from, MM_QUERYITEM, mp(id, 0), &mut mi as *mut MenuItem as u32);
        let len = send(from, MM_QUERYITEMTEXTLENGTH, id, 0) & 0xffff;
        let mut text = alloc::vec![0u8; len as usize + 1];
        send(from, MM_QUERYITEMTEXT, mp(id, len + 1), text.as_mut_ptr() as u32);
        mi.pos = at;
        if at >= 0 {
            at += 1;
        }
        send(target, MM_INSERTITEM, &mi as *const MenuItem as u32, text.as_ptr() as u32);
    }
    1
}

fn is(o: usize, c: &str) -> bool {
    wps::is(o, c)
}

/// wpDisplayMenu(hwndOwner, hwndClient, pptl, ulMenuType, ulReserved).
extern "C" fn wpDisplayMenu(o: usize, _owner: u32, client: u32, ptl: *const PointL, kind: u32, _res: u32) -> u32 {
    let all = CTXT_OPEN | CTXT_SETTINGS | CTXT_HELP | CTXT_DELETE | CTXT_COPY | CTXT_MOVE | CTXT_SHADOW | CTXT_PROGRAM | CTXT_ICON | CTXT_TREE
        | CTXT_DETAILS | CTXT_ARRANGE | CTXT_REFRESH | CTXT_SORT | CTXT_NEW | CTXT_FIND | CTXT_PRINT;
    let flags = obj::call(m::WPObject::wpFilterPopupMenu, &[o, all as usize, 0, 0]) as u32;
    let folder = is(o, "WPFolder");
    // A drive's views are its root folder's.
    let views = folder || is(o, "WPDisk");
    let program = is(o, "WPProgram") || is(o, "WPProgramFile");
    let mut open: Vec<Menu> = Vec::new();
    if flags & CTXT_SETTINGS != 0 {
        open.push(Menu::Item(WPMENUID_PROPERTIES as u16, b"~Settings", true));
    }
    if views {
        if flags & CTXT_ICON != 0 {
            open.push(Menu::Item(WPMENUID_ICON as u16, b"~Icon view", true));
        }
        if flags & CTXT_TREE != 0 {
            open.push(Menu::Item(WPMENUID_TREE as u16, b"~Tree view", true));
        }
        if flags & CTXT_DETAILS != 0 {
            open.push(Menu::Item(WPMENUID_DETAILS as u16, b"~Details view", true));
        }
    }
    if program && flags & CTXT_PROGRAM != 0 {
        open.push(Menu::Item(WPMENUID_PROGRAM as u16, b"~Program", true));
    }
    // Objects whose opening acts (rs2obj.rs, minwin.rs).
    let action: Option<&[u8]> = if is(o, "RS2Shutdown") {
        Some(b"~Shut down")
    } else if is(o, "RS2WindowList") {
        Some(b"~Window list")
    } else if is(o, "WPMinWindow") {
        Some(b"~Restore")
    } else if is(o, "Mindex") {
        Some(b"~Index")
    } else if is(o, "WPUrl") {
        Some(b"~Browser")
    } else if is(o, "WPLaunchPad") {
        Some(b"~LaunchPad")
    } else {
        None
    };
    if let Some(t) = action {
        open.push(Menu::Item(WPMENUID_PROGRAM as u16, t, true));
    }
    let mut items: Vec<Menu> = Vec::new();
    if flags & CTXT_OPEN != 0 && !open.is_empty() {
        items.push(Menu::SubId(WPMENUID_OPEN as u16, b"~Open", open));
    }
    if views && flags & CTXT_FIND != 0 && !is(o, "WPMinWinViewer") {
        items.push(Menu::Item(WPMENUID_FIND as u16, b"~Find...", true));
    }
    if folder && flags & CTXT_REFRESH != 0 {
        items.push(Menu::Item(WPMENUID_REFRESH as u16, b"Refresh ~now", true));
    }
    if kind == MENU_OPENVIEWPOPUP {
        if folder && flags & CTXT_SORT != 0 {
            items.push(crate::sortinc::menu(o));
        }
        if flags & CTXT_ARRANGE != 0 {
            items.push(Menu::Item(WPMENUID_ARRANGE as u16, b"~Arrange", true));
        }
        if is(o, "WPDesktop") {
            items.push(Menu::Sep);
            items.push(Menu::Item(WPMENUID_WINDOWLIST as u16, b"~Window list", true));
            items.push(Menu::Item(WPMENUID_SHUTDOWN as u16, b"Shut ~down...", true));
        }
    } else {
        items.push(Menu::Sep);
        let style = obj::call(m::WPObject::wpQueryStyle, &[o]) as u32;
        items.push(Menu::Item(MENUID_RENAME as u16, b"Re~name", style & obj::OBJSTYLE_NORENAME == 0));
        if flags & CTXT_COPY != 0 {
            items.push(Menu::Item(WPMENUID_COPY as u16, b"~Copy...", style & obj::OBJSTYLE_NOCOPY == 0));
        }
        if flags & CTXT_MOVE != 0 {
            items.push(Menu::Item(WPMENUID_MOVE as u16, b"~Move...", style & obj::OBJSTYLE_NOMOVE == 0));
        }
        if flags & CTXT_NEW != 0 && !is(o, "WPDisk") && !is(o, "WPMinWindow") {
            items.push(Menu::Item(WPMENUID_CREATEANOTHER as u16, b"Create ~another...", style & obj::OBJSTYLE_NOCOPY == 0));
        }
        if flags & CTXT_SHADOW != 0 {
            items.push(Menu::Item(WPMENUID_CREATESHADOW as u16, b"Create s~hadow...", style & obj::OBJSTYLE_NOLINK == 0));
        }
        if flags & CTXT_DELETE != 0 {
            items.push(Menu::Item(WPMENUID_DELETE as u16, b"~Delete...", true));
        }
        // Data files print on the default printer (printer.rs).
        if flags & CTXT_PRINT != 0 && is(o, "WPDataFile") && !folder {
            items.push(Menu::Item(WPMENUID_PRINT as u16, b"~Print", true));
        }
    }
    let menu = popup_menu(&items);
    if menu == 0 {
        return 0;
    }
    obj::call(m::WPObject::wpModifyPopupMenu, &[o, menu as usize, 0, 0xffff]);
    let old = unsafe { view::POPUP.0 };
    if old != 0 {
        unsafe { WinDestroyWindow(old) };
    }
    unsafe { view::POPUP = (menu, o) };
    let p = if ptl.is_null() { pointer_pos() } else { unsafe { *ptl } };
    unsafe { WinPopupMenu(HWND_DESKTOP, client, menu, p.x, p.y, 0, PU_HCONSTRAIN | PU_VCONSTRAIN | PU_KEYBOARD | PU_MOUSEBUTTON1 | PU_MOUSEBUTTON2) };
    menu
}

fn view_of(o: usize, view: u32) {
    obj::call(m::WPObject::wpViewObject, &[o, 0, view as usize, 0]);
}

extern "C" fn wpMenuItemSelected(o: usize, frame: u32, id: u32) -> u32 {
    match id {
        0 | WPMENUID_OPEN => view_of(o, OPEN_DEFAULT),
        WPMENUID_PROPERTIES => view_of(o, OPEN_SETTINGS),
        WPMENUID_ICON => view_of(o, OPEN_CONTENTS),
        WPMENUID_TREE => view_of(o, OPEN_TREE),
        WPMENUID_DETAILS => view_of(o, OPEN_DETAILS),
        WPMENUID_PROGRAM => view_of(o, OPEN_RUNNING),
        WPMENUID_REFRESH => view::refresh(o),
        WPMENUID_PRINT => {
            crate::printer::print_object(o);
        }
        WPMENUID_SHUTDOWN => crate::shutdown::ask(),
        WPMENUID_WINDOWLIST => {
            post(HWND_DESKTOP, WM_RS2_WINLIST, 0, 0);
        }
        WPMENUID_ARRANGE => {
            for w in 0..view::wins().len() {
                if view::wins()[w].obj == o {
                    view::arrange(w);
                }
            }
        }
        WPMENUID_FIND => {
            // A drive searches its root folder.
            let f = if is(o, "WPDisk") { crate::abs::root_of(o) } else { o };
            if f != 0 {
                crate::find::ask(f);
            }
        }
        WPMENUID_CREATEANOTHER => {
            let start = wps::path_of(obj::folder_of(o)).map(|mut p| {
                if !p.ends_with(b"\\") {
                    p.push(b'\\');
                }
                p
            });
            let t = obj::title(o);
            if let Some((f, n)) = crate::filejob::pick_folder(b"Create Another", b"C~reate", Some(&t), start.as_deref()) {
                crate::templates::create_from(o, f, Some(&n));
            }
        }
        WPMENUID_COPY | WPMENUID_MOVE | WPMENUID_CREATESHADOW => {
            // The selected objects of the view the menu came from (or
            // the object alone).
            let objs = view::wins().iter().position(|w| w.frame == frame).map_or(alloc::vec![o], |w| view::selected(w, o));
            let start = wps::path_of(obj::folder_of(o)).map(|mut p| {
                if !p.ends_with(b"\\") {
                    p.push(b'\\');
                }
                p
            });
            let (title, button): (&[u8], &[u8]) = match id {
                WPMENUID_COPY => (b"Copy", b"~Copy"),
                WPMENUID_MOVE => (b"Move", b"~Move"),
                _ => (b"Create Shadow", b"C~reate"),
            };
            // One object copied can get a new name on the way.
            let name = (id == WPMENUID_COPY && objs.len() == 1).then(|| wps::path_of(o).map(|p| crate::fileops::name_of(&p).to_vec()).unwrap_or_else(|| obj::title(o)));
            if let Some((f, n)) = crate::filejob::pick_folder(title, button, name.as_deref(), start.as_deref()) {
                match id {
                    WPMENUID_COPY => crate::filejob::transfer(crate::filejob::Op::Copy, &objs, f, Some(&n)),
                    WPMENUID_MOVE => crate::filejob::transfer(crate::filejob::Op::Move, &objs, f, None),
                    _ => {
                        for &x in &objs {
                            obj::call(m::WPObject::wpCreateShadowObject, &[x, f, 0]);
                        }
                    }
                }
            }
        }
        WPMENUID_DELETE => {
            // The selected objects of the view the menu came from.
            let objs = view::wins().iter().position(|w| w.frame == frame).map_or(alloc::vec![o], |w| view::selected(w, o));
            crate::delete::delete_objects(frame, &objs);
        }
        _ if crate::sortinc::menu_selected(o, id) => {}
        _ => return 0,
    }
    1
}

extern "C" fn wpMenuItemHelpSelected(_o: usize, _main: u32, _sub: u32) -> u32 {
    0
}

methods! {
    __m_wpFilterPopupMenu => wpFilterPopupMenu, 4;
    __m_wpModifyPopupMenu => wpModifyPopupMenu, 4;
    __m_wpInsertPopupMenuItems => wpInsertPopupMenuItems, 6;
    __m_wpDisplayMenu => wpDisplayMenu, 6;
    __m_wpMenuItemSelected => wpMenuItemSelected, 3;
    __m_wpMenuItemHelpSelected => wpMenuItemHelpSelected, 3;
}

pub fn methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
        ("wpFilterPopupMenu", __m_wpFilterPopupMenu as usize),
        ("wpModifyPopupMenu", __m_wpModifyPopupMenu as usize),
        ("wpInsertPopupMenuItems", __m_wpInsertPopupMenuItems as usize),
        ("wpDisplayMenu", __m_wpDisplayMenu as usize),
        ("wpMenuItemSelected", __m_wpMenuItemSelected as usize),
        ("wpMenuItemHelpSelected", __m_wpMenuItemHelpSelected as usize),
    ]
}

pub fn _unused(_: Impl) {}
