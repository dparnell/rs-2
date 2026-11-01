//! Menus the user adds to file system objects, and a folder's menu and
//! self-close settings (IBM's fs_wpAddFileMenuPage, MenuDlgProc,
//! fs_wpAddUserMenuItem, fs_wpAddUserItemsToPopupMenu,
//! fdr_wpAddFolderMenu2Page and fdr_wpAddFolderSelfClosePage, studied
//! with Ghidra):
//!
//! - **Menu** (WPFileSystem, wpAddFileMenuPage): the object's pop-up menu
//!   ("Primary pop-up menu") gets items the user adds, each a program
//!   started with the object's path as its parameter, and cascade menus
//!   (plain or conditional) holding more of them. IBM keeps 42-byte
//!   records (id, the menu it is in, its kind, its name); rs2 keeps the
//!   same with the program's path, in the object's state (WPFileSystem
//!   key 40). The items' ids start at WPMENUID_USER (0x6500), as IBM's.
//! - **Menu, page 2** (WPFolder, wpAddFolderMenu2Page): long or short
//!   menus (wpSetMenuStyle: short menus leave out Copy, Move, Create
//!   another, Create shadow and Rename) and whether the folder's windows
//!   have a menu bar (wpSetMenuBarVisibility: kept; rs2's folder windows
//!   have none yet).
//! - **Self-close** (WPFolder, wpAddFolderSelfClosePage): opening an
//!   object from the folder's window closes that window: never,
//!   subfolders only or all objects; by default as `PM_Workplace:
//!   FolderAutoClose` `Objects` says (NEVER, SUBFOLDERS, ALL), which the
//!   System object's Folder page would set.

#![allow(static_mut_refs)]

use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::setup::{add_page, check, checked, radio};
use crate::som;
use crate::util::{cstr, prf_read, z, HINI_USER};
use crate::wps;

pub const WPMENUID_USER: u32 = 0x6500;
const KIND_CASCADE: u16 = 1;
const KIND_CONDITIONAL: u16 = 2;
const KIND_ITEM: u16 = 3;

#[derive(Clone)]
pub struct UserItem {
    pub id: u16,
    /// The cascade menu it is in (0: the pop-up menu itself).
    pub parent: u16,
    pub kind: u16,
    pub name: Vec<u8>,
    /// The program an item starts (a path, or an object ID).
    pub program: Vec<u8>,
}

/// Each object's items (only objects that have some).
static mut MENUS: Vec<(usize, Vec<UserItem>)> = Vec::new();

pub fn items(o: usize) -> Vec<UserItem> {
    unsafe { MENUS.iter().find(|m| m.0 == o).map(|m| m.1.clone()).unwrap_or_default() }
}

fn set_items(o: usize, v: Vec<UserItem>) {
    unsafe {
        MENUS.retain(|m| m.0 != o);
        if !v.is_empty() {
            MENUS.push((o, v));
        }
    }
    obj::call(m::WPObject::wpSaveDeferred, &[o]);
}

/// The object goes to sleep or away.
pub fn forget(o: usize) {
    unsafe { MENUS.retain(|m| m.0 != o) };
}

fn encode(v: &[UserItem]) -> Vec<u8> {
    let mut b = Vec::new();
    for it in v {
        b.extend_from_slice(&it.id.to_le_bytes());
        b.extend_from_slice(&it.parent.to_le_bytes());
        b.extend_from_slice(&it.kind.to_le_bytes());
        b.push(it.name.len().min(255) as u8);
        b.extend_from_slice(&it.name[..it.name.len().min(255)]);
        b.push(it.program.len().min(255) as u8);
        b.extend_from_slice(&it.program[..it.program.len().min(255)]);
    }
    b
}

fn decode(b: &[u8]) -> Vec<UserItem> {
    let mut v = Vec::new();
    let mut i = 0;
    let r16 = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    while i + 7 <= b.len() {
        let (id, parent, kind) = (r16(i), r16(i + 2), r16(i + 4));
        let n = b[i + 6] as usize;
        i += 7;
        let Some(name) = b.get(i..i + n) else { break };
        i += n;
        let Some(&pn) = b.get(i) else { break };
        let Some(program) = b.get(i + 1..i + 1 + pn as usize) else { break };
        i += 1 + pn as usize;
        v.push(UserItem { id, parent, kind, name: name.to_vec(), program: program.to_vec() });
    }
    v
}

fn new_id(v: &[UserItem]) -> u16 {
    (WPMENUID_USER as u16..0x7fff).find(|id| !v.iter().any(|it| it.id == *id)).unwrap_or(WPMENUID_USER as u16)
}

// ---- WPFileSystem: saving, the pop-up menu ----

extern "C" fn fs_save_state(o: usize) -> u32 {
    som::call_parent(wps::ccd("WPFileSystem"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[o]);
    let v = items(o);
    if !v.is_empty() {
        let b = encode(&v);
        obj::wpSaveData(o, b"WPFileSystem\0".as_ptr(), 40, b.as_ptr(), b.len() as u32);
    }
    1
}

extern "C" fn fs_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPFileSystem"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    if let Some(b) = obj::restored_data(o, b"WPFileSystem", 40) {
        let v = decode(&b);
        unsafe {
            MENUS.retain(|m| m.0 != o);
            if !v.is_empty() {
                MENUS.push((o, v));
            }
        }
    }
    1
}

const MM_INSERTITEM: u32 = 0x0180;
const MM_QUERYITEM: u32 = 0x0182;
const MM_SETDEFAULTITEMID: u32 = 0x0432;
const MIS_TEXT: u16 = 0x0001;
const MIS_SUBMENU: u16 = 0x0010;
const MS_CONDITIONALCASCADE: u32 = 0x0040;
const QWL_STYLE: i32 = -2;

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

/// wpAddUserItemsToPopupMenu(hwndMenu, hwndCnr, iPosition): the user's
/// cascade menus first (each a submenu), then the items into the menu or
/// their cascade menu, as IBM's.
extern "C" fn wpAddUserItemsToPopupMenu(o: usize, menu: u32, _cnr: u32, pos: u32) -> u32 {
    let v = items(o);
    for c in v.iter().filter(|c| c.kind == KIND_CASCADE || c.kind == KIND_CONDITIONAL) {
        let sub = popup_menu(&[]);
        let mi = MenuItem { pos: pos as i16, style: MIS_TEXT | MIS_SUBMENU, attr: 0, id: c.id, sub, item: 0 };
        let t = z(&c.name);
        send(menu, MM_INSERTITEM, &mi as *const MenuItem as u32, t.as_ptr() as u32);
        if c.kind == KIND_CONDITIONAL {
            unsafe { WinSetWindowULong(sub, QWL_STYLE, WinQueryWindowULong(sub, QWL_STYLE) | MS_CONDITIONALCASCADE) };
        }
    }
    let mut first_in: Vec<u16> = Vec::new();
    for it in v.iter().filter(|i| i.kind == KIND_ITEM) {
        let mi = MenuItem { pos: -1, style: MIS_TEXT, attr: 0, id: it.id, sub: 0, item: 0 };
        let t = z(&it.name);
        let mut target = menu;
        if it.parent != 0 {
            let mut c = MenuItem::default();
            if send(menu, MM_QUERYITEM, mp(it.parent as u32, 1), &mut c as *mut MenuItem as u32) == 0 || c.sub == 0 {
                continue;
            }
            target = c.sub;
        }
        send(target, MM_INSERTITEM, &mi as *const MenuItem as u32, t.as_ptr() as u32);
        // A conditional cascade menu's first item is its default.
        if it.parent != 0 && !first_in.contains(&it.parent) {
            first_in.push(it.parent);
            if v.iter().any(|c| c.id == it.parent && c.kind == KIND_CONDITIONAL) {
                send(target, MM_SETDEFAULTITEMID, it.id as u32, 0);
            }
        }
    }
    1
}

/// One of the user's items was chosen: its program starts with the
/// object (as a file dropped on it would).
pub fn selected(o: usize, id: u32) -> bool {
    if id < WPMENUID_USER || id > 0x7fff {
        return false;
    }
    let v = items(o);
    let Some(it) = v.iter().find(|i| i.id as u32 == id && i.kind == KIND_ITEM) else { return false };
    let Some(path) = wps::path_of(o) else { return true };
    let prog = &it.program;
    // An object ID names a program object.
    if prog.first() == Some(&b'<') {
        let p = wps::object_from_location(prog);
        if p != 0 {
            crate::dnd::start_program_with(p, &path);
        }
        return true;
    }
    let mut params = Vec::new();
    if path.contains(&b' ') {
        params.push(b'"');
        params.extend_from_slice(&path);
        params.push(b'"');
    } else {
        params.extend_from_slice(&path);
    }
    let dir = crate::fileops::parent(&path);
    crate::launch::start(prog, &params, &dir, &it.name, 0);
    true
}

// ---- The Menu page ----

const ID_MENUS: u16 = 568;
const ID_MENU_NEW: u16 = 492;
const ID_MENU_PROPS: u16 = 570;
const ID_MENU_DEL: u16 = 195;
const ID_ACTIONS_OF: u16 = 572;
const ID_ITEMS: u16 = 3570;
const ID_ITEM_NEW: u16 = 573;
const ID_ITEM_PROPS: u16 = 574;
const ID_ITEM_DEL: u16 = 575;
const LIT_NONE: u32 = 0xffff;

fn page_obj(h: u32) -> usize {
    unsafe { WinQueryWindowULong(h, QWL_USER) as usize }
}

fn sel(h: u32, id: u16) -> Option<usize> {
    let i = unsafe { WinSendDlgItemMsg(h, id as u32, LM_QUERYSELECTION, LIT_NONE, 0) } as i32 as i16;
    (i >= 0).then_some(i as usize)
}

/// The menus listed: the pop-up menu (id 0) and the cascade menus.
fn menus_of(o: usize) -> Vec<(u16, Vec<u8>)> {
    core::iter::once((0u16, b"~Primary pop-up menu".to_vec()))
        .chain(items(o).into_iter().filter(|i| i.kind != KIND_ITEM).map(|i| (i.id, i.name)))
        .collect()
}

fn current_menu(h: u32) -> u16 {
    let o = page_obj(h);
    sel(h, ID_MENUS).and_then(|i| menus_of(o).get(i).map(|m| m.0)).unwrap_or(0)
}

fn items_of(o: usize, menu: u16) -> Vec<UserItem> {
    items(o).into_iter().filter(|i| i.kind == KIND_ITEM && i.parent == menu).collect()
}

fn fill_menus(h: u32, select: usize) {
    let o = page_obj(h);
    unsafe { WinSendDlgItemMsg(h, ID_MENUS as u32, LM_DELETEALL, 0, 0) };
    for (_, n) in menus_of(o) {
        let t: Vec<u8> = n.into_iter().filter(|&c| c != b'~').chain(core::iter::once(0)).collect();
        unsafe { WinSendDlgItemMsg(h, ID_MENUS as u32, LM_INSERTITEM, LIT_END, t.as_ptr() as u32) };
    }
    unsafe { WinSendDlgItemMsg(h, ID_MENUS as u32, LM_SELECTITEM, select as u32, 1) };
    fill_items(h);
}

fn fill_items(h: u32) {
    let o = page_obj(h);
    let menu = current_menu(h);
    let name = menus_of(o).into_iter().find(|m| m.0 == menu).map(|m| m.1).unwrap_or_default();
    set_item_text(h, ID_ACTIONS_OF as u32, &name.into_iter().filter(|&c| c != b'~').collect::<Vec<u8>>());
    unsafe { WinSendDlgItemMsg(h, ID_ITEMS as u32, LM_DELETEALL, 0, 0) };
    for it in items_of(o, menu) {
        let t = z(&it.name.iter().copied().filter(|&c| c != b'~').collect::<Vec<u8>>());
        unsafe { WinSendDlgItemMsg(h, ID_ITEMS as u32, LM_INSERTITEM, LIT_END, t.as_ptr() as u32) };
    }
}

extern "C" fn menu_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            fill_menus(h, 0);
            0
        }
        WM_CONTROL if a & 0xffff == ID_MENUS as u32 && a >> 16 == LN_SELECT => {
            fill_items(h);
            0
        }
        WM_COMMAND => {
            let o = page_obj(h);
            let mut v = items(o);
            match (a & 0xffff) as u16 {
                ID_MENU_NEW => {
                    if let Some((name, kind)) = menu_dialog(h, b"", KIND_CASCADE) {
                        let id = new_id(&v);
                        v.push(UserItem { id, parent: 0, kind, name, program: Vec::new() });
                        set_items(o, v);
                        let n = menus_of(o).len() - 1;
                        fill_menus(h, n);
                    }
                }
                ID_MENU_PROPS => {
                    let menu = current_menu(h);
                    if let Some(i) = v.iter().position(|i| i.id == menu && menu != 0) {
                        if let Some((name, kind)) = menu_dialog(h, &v[i].name.clone(), v[i].kind) {
                            v[i].name = name;
                            v[i].kind = kind;
                            set_items(o, v);
                            fill_menus(h, sel(h, ID_MENUS).unwrap_or(0));
                        }
                    }
                }
                ID_MENU_DEL => {
                    let menu = current_menu(h);
                    if menu != 0 {
                        v.retain(|i| i.id != menu && i.parent != menu);
                        set_items(o, v);
                        fill_menus(h, 0);
                    }
                }
                ID_ITEM_NEW => {
                    if let Some((name, program)) = item_dialog(h, b"", b"") {
                        let id = new_id(&v);
                        v.push(UserItem { id, parent: current_menu(h), kind: KIND_ITEM, name, program });
                        set_items(o, v);
                        fill_items(h);
                    }
                }
                ID_ITEM_PROPS | ID_ITEM_DEL => {
                    let menu = current_menu(h);
                    let Some(k) = sel(h, ID_ITEMS) else { return 0 };
                    let Some(id) = items_of(o, menu).get(k).map(|i| i.id) else { return 0 };
                    let Some(i) = v.iter().position(|x| x.id == id) else { return 0 };
                    if a & 0xffff == ID_ITEM_DEL as u32 {
                        v.remove(i);
                    } else if let Some((name, program)) = item_dialog(h, &v[i].name.clone(), &v[i].program.clone()) {
                        v[i].name = name;
                        v[i].program = program;
                    } else {
                        return 0;
                    }
                    set_items(o, v);
                    fill_items(h);
                }
                _ => {}
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn group(t: &[u8], (x, y, cx, cy): (i16, i16, i16, i16)) -> Item {
    // SS_GROUPBOX
    item(WC_STATIC, t, WS_VISIBLE | 0x0002, (x, y, cx, cy), 0xffff)
}

extern "C" fn wpAddFileMenuPage(o: usize, nb: u32) -> u32 {
    let b = |t: &[u8], (x, y): (i16, i16), id: u16| item(WC_BUTTON, t, WS_VISIBLE | WS_TABSTOP | WS_GROUP, (x, y, 70, 13), id);
    // LS_NOADJUSTPOS
    let list = |(x, y, cx, cy): (i16, i16, i16, i16), id: u16| item(WC_LISTBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0008, (x, y, cx, cy), id);
    add_page(o, nb, menu_page_proc, alloc::vec![
        book::label(b"Available menus", (4, 106, 120)),
        list((4, 62, 140, 42), ID_MENUS),
        b(b"~Create another...", (152, 90), ID_MENU_NEW),
        b(b"~Properties...", (152, 76), ID_MENU_PROPS),
        b(b"~Delete", (152, 62), ID_MENU_DEL),
        book::label(b"Actions on menu:", (4, 50, 64)),
        item(WC_STATIC, b"", WS_VISIBLE | 0x0001, (70, 50, 150, 8), ID_ACTIONS_OF),
        list((4, 4, 140, 44), ID_ITEMS),
        b(b"C~reate another...", (152, 34), ID_ITEM_NEW),
        b(b"Pr~operties...", (152, 20), ID_ITEM_PROPS),
        b(b"De~lete", (152, 6), ID_ITEM_DEL),
    ], b"~Menu\0")
}

// The Menu Properties and Menu Item Properties dialogs (IBM's 1066 and
// 1067).

const ID_NAME: u16 = 576;
const ID_CASCADE: u16 = 577;
const ID_CONDITIONAL: u16 = 578;
const ID_PROGRAM: u16 = 581;

static mut DLG_IN: (Vec<u8>, Vec<u8>, u16) = (Vec::new(), Vec::new(), 0);
static mut DLG_OUT: Option<(Vec<u8>, Vec<u8>, u16)> = None;

extern "C" fn props_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let (n, p, k) = unsafe { DLG_IN.clone() };
            set_item_text(h, ID_NAME as u32, &n);
            set_item_text(h, ID_PROGRAM as u32, &p);
            check(h, if k == KIND_CONDITIONAL { ID_CONDITIONAL } else { ID_CASCADE }, true);
            unsafe { WinSetFocus(HWND_DESKTOP, WinWindowFromID(h, ID_NAME as u32)) };
            1
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            let n = item_text(h, ID_NAME as u32);
            if !n.is_empty() {
                let kind = if checked(h, ID_CONDITIONAL) { KIND_CONDITIONAL } else { KIND_CASCADE };
                unsafe { DLG_OUT = Some((n, item_text(h, ID_PROGRAM as u32), kind)) };
                unsafe { WinDismissDlg(h, DID_OK) };
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

fn run_props(owner: u32, title: &[u8], items: Vec<Item>, input: (Vec<u8>, Vec<u8>, u16)) -> Option<(Vec<u8>, Vec<u8>, u16)> {
    unsafe {
        DLG_IN = input;
        DLG_OUT = None;
    }
    let mut all = items;
    all.push(item(WC_BUTTON, b"~OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (6, 4, 50, 14), DID_OK as u16));
    all.push(item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (62, 4, 50, 14), DID_CANCEL as u16));
    let tpl = template(dialog_frame(title, 200, 96), all);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, owner, props_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return None;
    }
    unsafe {
        WinProcessDlg(dlg);
        WinDestroyWindow(dlg);
        DLG_OUT.take()
    }
}

fn entry(y: i16, id: u16) -> Item {
    // ES_AUTOSCROLL | ES_MARGIN
    item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (70, y, 120, 8), id)
}

fn menu_dialog(owner: u32, name: &[u8], kind: u16) -> Option<(Vec<u8>, u16)> {
    let items = alloc::vec![
        book::label(b"Menu name:", (6, 78, 60)),
        entry(78, ID_NAME),
        group(b"Menu type", (6, 26, 186, 44)),
        radio(b"~Cascade menu", (12, 50, 170), ID_CASCADE, true),
        radio(b"Con~ditional cascade", (12, 36, 170), ID_CONDITIONAL, false),
    ];
    run_props(owner, b"Menu Properties", items, (name.to_vec(), Vec::new(), kind)).map(|(n, _, k)| (n, k))
}

fn item_dialog(owner: u32, name: &[u8], program: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let items = alloc::vec![
        book::label(b"Menu item name:", (6, 78, 62)),
        entry(78, ID_NAME),
        group(b"Program", (6, 26, 186, 44)),
        book::label(b"Name:", (12, 48, 50)),
        entry(48, ID_PROGRAM),
        book::label(b"(a program's path, or a program object's ID)", (12, 34, 176)),
    ];
    run_props(owner, b"Menu Item Properties", items, (name.to_vec(), program.to_vec(), KIND_ITEM)).map(|(n, p, _)| (n, p))
}

// ---- WPFolder: menu style and menu bar (Menu page 2) ----

pub const MENUS_SHORT: u32 = 0;
pub const MENUS_LONG: u32 = 1;

fn system_default(key: &[u8], default: u32) -> u32 {
    prf_read(HINI_USER, b"PM_Workplace", key).and_then(|v| v.first().copied()).map_or(default, |c| (c == b'1') as u32)
}

/// A folder's menus: its own, else the system's (`PM_Workplace`
/// `FolderMenus`, long unless set).
pub fn menu_style(o: usize) -> u32 {
    match crate::fs::folder_menu_flags(o) & 3 {
        1 => MENUS_SHORT,
        2 => MENUS_LONG,
        _ => system_default(b"FolderMenus", MENUS_LONG),
    }
}

pub fn menu_bar(o: usize) -> bool {
    match crate::fs::folder_menu_flags(o) >> 2 & 3 {
        1 => false,
        2 => true,
        _ => system_default(b"FolderMenuBar", 1) != 0,
    }
}

extern "C" fn wpSetMenuStyle(o: usize, style: u32) -> u32 {
    if !wps::is(o, "WPFolder") {
        return 0;
    }
    let f = crate::fs::folder_menu_flags(o) & !3;
    crate::fs::set_folder_menu_flags(o, f | match style {
        MENUS_SHORT => 1,
        MENUS_LONG => 2,
        _ => 0,
    });
    1
}

extern "C" fn wpQueryMenuStyle(o: usize) -> u32 {
    if wps::is(o, "WPFolder") { menu_style(o) } else { MENUS_LONG }
}

extern "C" fn wpSetMenuBarVisibility(o: usize, on: u32) -> u32 {
    let f = crate::fs::folder_menu_flags(o) & !0xc;
    crate::fs::set_folder_menu_flags(o, f | match on {
        0 => 1 << 2,
        1 => 2 << 2,
        _ => 0,
    });
    1
}

extern "C" fn wpQueryMenuBarVisibility(o: usize) -> u32 {
    menu_bar(o) as u32
}

const ID_BAR: u16 = 1;
const ID_LONG: u16 = 2;
const ID_SHORT: u16 = 3;

extern "C" fn menu2_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            let o = b as usize;
            check(h, if menu_style(o) == MENUS_SHORT { ID_SHORT } else { ID_LONG }, true);
            check(h, ID_BAR, menu_bar(o));
            0
        }
        WM_CONTROL if a >> 16 == 1 => {
            let o = page_obj(h);
            match (a & 0xffff) as u16 {
                ID_LONG => {
                    wpSetMenuStyle(o, MENUS_LONG);
                }
                ID_SHORT => {
                    wpSetMenuStyle(o, MENUS_SHORT);
                }
                ID_BAR => {
                    wpSetMenuBarVisibility(o, checked(h, ID_BAR) as u32);
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderMenu2Page(o: usize, nb: u32) -> u32 {
    let page = {
        let tpl = template(book::page_frame(), alloc::vec![
            group(b"Menu style", (4, 60, 160, 56)),
            radio(b"Display ~long menus", (10, 92, 150), ID_LONG, true),
            radio(b"Display ~short menus", (10, 78, 150), ID_SHORT, false),
            // BS_AUTOCHECKBOX
            item(WC_BUTTON, b"Display ~Folder Menu Bars", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0001, (8, 44, 160, 10), ID_BAR),
        ]);
        book::make_page(nb, menu2_proc, &tpl, o)
    };
    if page == 0 {
        return 0;
    }
    book::insert_more(o, nb, page, b"Menu - Page 2 of 2\0")
}

// ---- WPFolder: self-close ----

pub const SELFCLOSE_NEVER: u32 = 1;
pub const SELFCLOSE_SUBFOLDERS: u32 = 2;
pub const SELFCLOSE_ALL: u32 = 3;

/// The folder's self-close setting (its own, else the system's).
pub fn self_close(o: usize) -> u32 {
    match crate::fs::folder_menu_flags(o) >> 4 & 3 {
        0 => match prf_read(HINI_USER, b"PM_Workplace:FolderAutoClose", b"Objects").map(|v| v.into_iter().take_while(|&c| c != 0).collect::<Vec<u8>>()) {
            Some(v) if v.eq_ignore_ascii_case(b"SUBFOLDERS") => SELFCLOSE_SUBFOLDERS,
            Some(v) if v.eq_ignore_ascii_case(b"ALL") => SELFCLOSE_ALL,
            _ => SELFCLOSE_NEVER,
        },
        v => v,
    }
}

fn set_self_close(o: usize, v: u32) {
    let f = crate::fs::folder_menu_flags(o) & !0x30;
    crate::fs::set_folder_menu_flags(o, f | (v & 3) << 4);
}

/// An object was opened from a view of `folder` (its frame): the view
/// closes if the folder's self-close setting says so. The desktop never
/// closes.
pub fn opened_from(folder: usize, frame: u32, opened: usize) {
    if folder == 0 || wps::is(folder, "WPDesktop") || frame == 0 {
        return;
    }
    let close = match self_close(folder) {
        SELFCLOSE_ALL => true,
        SELFCLOSE_SUBFOLDERS => wps::is(opened, "WPFolder") || wps::is(opened, "WPDisk"),
        _ => false,
    };
    if close {
        crate::view::close_frame(frame);
    }
}

const ID_NEVER: u16 = 2102;
const ID_SUBFOLDERS: u16 = 2101;
const ID_ALL: u16 = 2103;

extern "C" fn self_close_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            check(h, match self_close(b as usize) {
                SELFCLOSE_SUBFOLDERS => ID_SUBFOLDERS,
                SELFCLOSE_ALL => ID_ALL,
                _ => ID_NEVER,
            }, true);
            0
        }
        WM_CONTROL if a >> 16 == 1 => {
            let o = page_obj(h);
            match (a & 0xffff) as u16 {
                ID_NEVER => set_self_close(o, SELFCLOSE_NEVER),
                ID_SUBFOLDERS => set_self_close(o, SELFCLOSE_SUBFOLDERS),
                ID_ALL => set_self_close(o, SELFCLOSE_ALL),
                _ => {}
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpAddFolderSelfClosePage(o: usize, nb: u32) -> u32 {
    let page = {
        let tpl = template(book::page_frame(), alloc::vec![
            group(b"Folder automatic close", (4, 40, 200, 76)),
            book::label(b"Automatically close folder view when", (8, 98, 190)),
            book::label(b"opening objects contained within", (8, 88, 190)),
            radio(b"~Never", (12, 72, 150), ID_NEVER, true),
            radio(b"~Subfolders only", (12, 60, 150), ID_SUBFOLDERS, false),
            radio(b"~All objects", (12, 48, 150), ID_ALL, false),
        ]);
        book::make_page(nb, self_close_proc, &tpl, o)
    };
    if page == 0 {
        return 0;
    }
    book::insert_after(o, nb, page, b"Window - Page 2 of 2\0", b"~Window")
}

methods! {
    __m_wpAddFileMenuPage => wpAddFileMenuPage, 2;
    __m_wpAddUserItemsToPopupMenu => wpAddUserItemsToPopupMenu, 4;
    __m_fs_save_state => fs_save_state, 1;
    __m_fs_restore_state => fs_restore_state, 2;
    __m_wpAddFolderMenu2Page => wpAddFolderMenu2Page, 2;
    __m_wpAddFolderSelfClosePage => wpAddFolderSelfClosePage, 2;
    __m_wpSetMenuStyle => wpSetMenuStyle, 2;
    __m_wpQueryMenuStyle => wpQueryMenuStyle, 1;
    __m_wpSetMenuBarVisibility => wpSetMenuBarVisibility, 2;
    __m_wpQueryMenuBarVisibility => wpQueryMenuBarVisibility, 1;
}

pub fn file_system_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpAddFileMenuPage", __m_wpAddFileMenuPage as usize), ("wpAddUserItemsToPopupMenu", __m_wpAddUserItemsToPopupMenu as usize)]
}

pub fn file_system_overrides() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpSaveState", __m_fs_save_state as usize), ("wpRestoreState", __m_fs_restore_state as usize)]
}

pub fn object_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![("wpSetMenuStyle", __m_wpSetMenuStyle as usize), ("wpQueryMenuStyle", __m_wpQueryMenuStyle as usize)]
}

pub fn folder_methods() -> Vec<(&'static str, usize)> {
    alloc::vec![
        ("wpAddFolderMenu2Page", __m_wpAddFolderMenu2Page as usize),
        ("wpAddFolderSelfClosePage", __m_wpAddFolderSelfClosePage as usize),
        ("wpSetMenuBarVisibility", __m_wpSetMenuBarVisibility as usize),
        ("wpQueryMenuBarVisibility", __m_wpQueryMenuBarVisibility as usize),
    ]
}

pub fn _unused() -> &'static [u8] {
    cstr(b"\0".as_ptr())
}
