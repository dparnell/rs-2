//! The Connections folder's objects:
//!
//! - **Network** (WPNetwork, `<WP_NETWORK>`): the network groups
//!   (workgroups and domains) the LAN requester sees, as IBM's File and
//!   Print Client fills it. Opening the folder starts the requester (from
//!   IBMLAN.INI) when it is not running.
//! - **Network groups** (WPNetgrp, real name `\\GROUP`): the servers of a
//!   workgroup.
//! - **Servers** (WPServer, `\\SERVER`): the shares of a server (its
//!   IPC$ and administrative `$` shares are not shown).
//! - **Shared directories** (WPSharedDir, `\\SERVER\SHARE`): a share is
//!   a folder on its UNC name, so opening it shows the files (the kernel's
//!   redirector resolves UNC paths). Its menu assigns a drive letter (NET
//!   USE: the drive appears in the Drives folder) or, for a print queue, a
//!   printer port (LPT1-LPT3; a printer object on that port then prints to
//!   the server's queue through rs2's spooler), and removes it again.
//! - **Printers** (WPPrinter, IBM's WPPRINT.DLL class): print
//!   destinations on rs2's spooler, in printer.rs. The Printers folder
//!   holds a template, as IBM's INI.RC puts one there
//!   (`<WP_NEWPRINTER>`).

#![allow(static_mut_refs)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::book;
use crate::classes_gen::m;
use crate::methods;
use crate::obj;
use crate::pm::*;
use crate::setup::add_page;
use crate::som;
use crate::util::{str_new, z};
use crate::wps::{self, Impl};

const QC_FIRST: u32 = 0;
const QC_NEXT: u32 = 1;

unsafe extern "C" {
    #[link_name = "PMWIN.789"]
    fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
}

fn message(text: &str) {
    let t = z(text.as_bytes());
    // MB_OK | MB_INFORMATION | MB_MOVEABLE
    unsafe { WinMessageBox(1, 1, t.as_ptr(), b"Network\0".as_ptr(), 0, 0x0000 | 0x0030 | 0x4000) };
}

fn bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

fn text(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// An object for a network path (`\\GROUP`, `\\SERVER`, `\\SERVER\SHARE`)
/// in `folder`: the awake one, or a new one of `class`.
fn net_object(class: &str, path: &str, title: &str, folder: usize) -> usize {
    let p = bytes(path);
    if let Some(o) = wps::by_path(&p) {
        return o;
    }
    let c = wps::class(class);
    if c == 0 {
        return 0;
    }
    let o = som::sc_call(som::sc::SOM_NEW, &[c]);
    if o == 0 {
        return 0;
    }
    wps::register(o, 0, &p);
    crate::fs::set_real_name_raw(o, &p);
    obj::wake(o, folder, Vec::new());
    obj::d(o).title = str_new(&bytes(title));
    o
}

/// Replace the network objects of `folder`'s content with `want` (other
/// content, abstract objects put there, stays).
fn set_net_content(folder: usize, class: &str, want: Vec<usize>) {
    let mut cur = Vec::new();
    let mut o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[folder, 0, QC_FIRST as usize]);
    while o != 0 {
        cur.push(o);
        o = wps::call("WPFolder", m::WPFolder::wpQueryContent, &[folder, o, QC_NEXT as usize]);
    }
    for &c in &cur {
        if wps::is(c, class) && !want.contains(&c) {
            wps::call("WPFolder", m::WPFolder::wpDeleteFromContent, &[folder, c]);
        }
    }
    for w in want {
        if !cur.contains(&w) {
            obj::d(w).folder = folder;
            wps::call("WPFolder", m::WPFolder::wpAddToContent, &[folder, w]);
        }
    }
}

fn path_text(o: usize) -> String {
    text(&crate::fs::path(o))
}

/// Share types (STYPE_*) of the shares seen, by UNC name.
static mut SHARE_TYPES: Vec<(String, u32)> = Vec::new();

fn share_type(unc: &str) -> u32 {
    unsafe { SHARE_TYPES.iter().find(|(u, _)| u.eq_ignore_ascii_case(unc)).map_or(0, |x| x.1) }
}

// ---- Network ----

extern "C" fn network_page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn network_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPNetwork"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    let st = lanreq::status();
    let state = if st.started {
        let user = if st.user.is_empty() { String::from("nobody is logged on") } else { alloc::format!("{} is logged on to {}", st.user, st.domain) };
        alloc::format!("The LAN requester is running on this computer, \\\\{}, in workgroup {}; {}.", st.computer, st.workgroup, user)
    } else {
        String::from("The LAN requester is not running. Opening this folder starts it with the settings in IBMLAN.INI (or NET START REQUESTER).")
    };
    // SS_TEXT | DT_WORDBREAK
    add_page(o, nb, network_page_proc, alloc::vec![
        book::label(b"Network status", (6, 106, 220)),
        item(WC_STATIC, &bytes(&state), WS_VISIBLE | 0x0001 | 0x4000, (12, 40, 214, 62), 0xffff),
    ], b"~Network\0");
    1
}

/// The network groups: the workgroups of the servers seen, the
/// requester's own and its other domains.
extern "C" fn network_populate(obj: usize, r: u32, p: *const u8, folders_only: u32) -> u32 {
    som::call_parent(wps::ccd("WPNetwork"), wps::cd("WPFolder"), m::WPFolder::wpPopulate, &[obj, r as usize, p as usize, folders_only as usize]);
    if lanreq::ensure_started(&[]).is_err() {
        return 1;
    }
    let st = lanreq::status();
    let mut groups: Vec<String> = lanreq::servers(None).unwrap_or_default().into_iter().map(|s| s.workgroup).collect();
    groups.push(st.workgroup.clone());
    groups.extend(st.other_domains.split(',').map(String::from).filter(|s| !s.is_empty()));
    groups.sort();
    groups.dedup();
    let want: Vec<usize> = groups.iter().map(|g| net_object("WPNetgrp", &alloc::format!("\\\\{g}"), g, obj)).filter(|&o| o != 0).collect();
    set_net_content(obj, "WPNetgrp", want);
    1
}

// ---- Network groups ----

extern "C" fn netgrp_populate(obj: usize, r: u32, p: *const u8, folders_only: u32) -> u32 {
    fo_reset(obj, r, p, folders_only, "WPNetgrp");
    let group = path_text(obj).trim_start_matches('\\').to_ascii_uppercase();
    let want: Vec<usize> = lanreq::servers(Some(&group))
        .unwrap_or_default()
        .into_iter()
        .map(|s| net_object("WPServer", &alloc::format!("\\\\{}", s.name), &s.name, obj))
        .filter(|&o| o != 0)
        .collect();
    set_net_content(obj, "WPServer", want);
    1
}

/// The folder's own content (abstract objects only: a network path has
/// no directory to list).
fn fo_reset(obj: usize, r: u32, p: *const u8, folders_only: u32, class: &str) {
    som::call_parent(wps::ccd(class), wps::cd("WPFolder"), m::WPFolder::wpPopulate, &[obj, r as usize, p as usize, folders_only as usize]);
}

extern "C" fn netgrp_query_identity(obj: usize, buf: *mut u8, cb: u32) -> u32 {
    copy_out(&bytes(path_text(obj).trim_start_matches('\\')), buf, cb)
}

fn copy_out(s: &[u8], buf: *mut u8, cb: u32) -> u32 {
    if buf.is_null() || (cb as usize) <= s.len() {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
    }
    1
}

// ---- Servers ----

extern "C" fn server_populate(obj: usize, r: u32, p: *const u8, folders_only: u32) -> u32 {
    fo_reset(obj, r, p, folders_only, "WPServer");
    let server = path_text(obj).trim_start_matches('\\').to_ascii_uppercase();
    let mut want = Vec::new();
    match lanreq::shares(&server) {
        Ok(v) => {
            for s in v.into_iter().filter(|s| !s.name.ends_with('$') && (s.typ & 0xFFFF) <= 1) {
                let unc = alloc::format!("\\\\{}\\{}", server, s.name.to_ascii_uppercase());
                unsafe {
                    SHARE_TYPES.retain(|(u, _)| !u.eq_ignore_ascii_case(&unc));
                    SHARE_TYPES.push((unc.clone(), s.typ & 0xFFFF));
                }
                let title = if s.typ & 0xFFFF == 1 { alloc::format!("{} (printer)", s.name) } else { s.name.clone() };
                let o = net_object("WPSharedDir", &unc, &title, obj);
                if o != 0 {
                    want.push(o);
                }
            }
        }
        Err(e) => message(&alloc::format!("The shares of \\\\{server} cannot be listed (error {e}).")),
    }
    set_net_content(obj, "WPSharedDir", want);
    1
}

extern "C" fn server_query_identity(obj: usize, buf: *mut u8, cb: u32) -> u32 {
    copy_out(&crate::fs::path(obj), buf, cb)
}

// ---- Shared directories ----

const ID_ASSIGN: u32 = 0x7A10;
const ID_UNASSIGN: u32 = 0x7A11;

#[repr(C)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    item: u32,
}

fn add_menu_item(menu: u32, id: u32, t: &str) {
    let mi = MenuItem { pos: -1, style: 0x0001, attr: 0, id: id as u16, sub: 0, item: 0 };
    let t = z(t.as_bytes());
    crate::pm::send(menu, 0x0180, &mi as *const MenuItem as u32, t.as_ptr() as u32);
}

/// The local name a share is connected to, if any.
fn assigned(unc: &str) -> Option<String> {
    lanreq::uses().ok()?.into_iter().find(|u| u.remote.eq_ignore_ascii_case(unc) && !u.local.is_empty()).map(|u| u.local)
}

extern "C" fn shared_modify_menu(o: usize, menu: u32, cnr: u32, pos: u32) -> u32 {
    som::call_parent(wps::ccd("WPSharedDir"), wps::cd("WPObject"), m::WPObject::wpModifyPopupMenu, &[o, menu as usize, cnr as usize, pos as usize]);
    let unc = path_text(o);
    let printer = share_type(&unc) == 1;
    match assigned(&unc) {
        Some(l) => add_menu_item(menu, ID_UNASSIGN, &alloc::format!("Unassign {l}")),
        None => add_menu_item(menu, ID_ASSIGN, if printer { "Assign port" } else { "Assign drive" }),
    }
    1
}

/// A free drive letter (from Z: down) or printer port (LPT3 down).
fn free_local(printer: bool) -> Option<String> {
    let used: Vec<String> = lanreq::uses().unwrap_or_default().into_iter().map(|u| u.local).collect();
    if printer {
        return ["LPT3", "LPT2", "LPT1"].iter().find(|p| !used.iter().any(|u| u == *p)).map(|p| String::from(*p));
    }
    let (mut cur, mut map) = (0u32, 0u32);
    unsafe { os2::DosQueryCurrentDisk(&mut cur, &mut map) };
    (b'D'..=b'Z').rev().find(|&l| map & (1 << (l - b'A')) == 0).map(|l| alloc::format!("{}", l as char))
}

extern "C" fn shared_menu_selected(o: usize, frame: u32, id: u32) -> u32 {
    let unc = path_text(o);
    match id {
        ID_ASSIGN => {
            let printer = share_type(&unc) == 1;
            let Some(local) = free_local(printer) else {
                message("No drive letter or printer port is free.");
                return 1;
            };
            match lanreq::use_add(&local, &unc, None, None) {
                Ok(()) => {
                    if printer {
                        message(&alloc::format!("{unc} is now on {local}. A printer whose port is {local} prints to it."));
                    } else {
                        crate::fs::refresh_drives();
                    }
                }
                Err(e) => message(&alloc::format!("{unc} could not be assigned (error {e}).")),
            }
            1
        }
        ID_UNASSIGN => {
            if let Some(l) = assigned(&unc) {
                match lanreq::use_del(l.trim_end_matches(':'), false) {
                    Ok(()) => crate::fs::refresh_drives(),
                    Err(e) => message(&alloc::format!("{l} could not be unassigned (error {e}): files may be open on it.")),
                }
            }
            1
        }
        _ => som::call_parent(wps::ccd("WPSharedDir"), wps::cd("WPObject"), m::WPObject::wpMenuItemSelected, &[o, frame as usize, id as usize]) as u32,
    }
}

/// A print queue has no files to show: opening it says how to print to it.
extern "C" fn shared_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    let unc = path_text(o);
    if share_type(&unc) == 1 && view != obj::OPEN_SETTINGS {
        match assigned(&unc) {
            Some(l) => message(&alloc::format!("{unc} is a printer on the server, on port {l} here.")),
            None => message(&alloc::format!("{unc} is a printer on the server. Assign it a port (in its menu) to print to it.")),
        }
        return 1;
    }
    som::call_parent(wps::ccd("WPSharedDir"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32
}

methods! {
    __m_network_add_pages => network_add_pages, 2;
    __m_network_populate => network_populate, 4;
    __m_netgrp_populate => netgrp_populate, 4;
    __m_netgrp_query_identity => netgrp_query_identity, 3;
    __m_server_populate => server_populate, 4;
    __m_server_query_identity => server_query_identity, 3;
    __m_shared_modify_menu => shared_modify_menu, 4;
    __m_shared_menu_selected => shared_menu_selected, 3;
    __m_shared_open => shared_open, 4;
}

pub fn impl_network() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![("wpAddSettingsPages", __m_network_add_pages as usize), ("wpPopulate", __m_network_populate as usize)],
    }
}

pub fn impl_netgrp() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![("wpQueryNetIdentity", __m_netgrp_query_identity as usize)],
        overrides: alloc::vec![("wpPopulate", __m_netgrp_populate as usize)],
    }
}

pub fn impl_server() -> Impl {
    Impl {
        data: 0,
        methods: alloc::vec![("wpQuerySrvrIdentity", __m_server_query_identity as usize)],
        overrides: alloc::vec![("wpPopulate", __m_server_populate as usize)],
    }
}

pub fn impl_shared_dir() -> Impl {
    Impl {
        data: 0,
        methods: Vec::new(),
        overrides: alloc::vec![
            ("wpModifyPopupMenu", __m_shared_modify_menu as usize),
            ("wpMenuItemSelected", __m_shared_menu_selected as usize),
            ("wpOpen", __m_shared_open as usize),
        ],
    }
}
