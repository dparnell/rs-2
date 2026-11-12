//! The network objects' own settings pages and dialogs, as IBM's PMWP
//! has them (studied with Ghidra: netg_wpAddNetworkPage and
//! NETNetworkDlgProc, ser_wpAddServerPage and SRVNetworkDlgProc,
//! sha_wpAddResourcePage and RDRVNetworkDlgProc, the Login and Logout
//! dialogs PMWPMRI's 2032-2042 hold, ser_wpMenuItemSelected's Access
//! another server; WPPRINT's RPRTNetworkDlgProc for network printers),
//! in rs2's own words, on rs2's LAN requester (lanreq, docs/LAN.md):
//!
//! - **Network page** of a network group (wpAddNetworkPage): the group,
//!   what it is, and whether this computer is logged on to it.
//! - **Server page** (wpAddServerPage): the group, the server, its
//!   description (the server's comment) and the status.
//! - **Resource page** of a shared directory (wpAddResourcePage): the
//!   group, server, resource, its description (the share's remark), the
//!   status and the drive it is assigned to.
//! - **Logon** (IBM's Login): a network group's logs this computer's
//!   requester on (NET LOGON: the user, password and domain every server
//!   connection uses); a server's or resource's logs on to that server
//!   only (a connection to its IPC$ with that user and password, which
//!   the server's other connections then use). **Logoff** undoes them.
//! - **Access another server / network directory / network printer**:
//!   network, server and resource chosen from what the requester sees
//!   (or typed), then opened (or, for a printer, made).
//! - **Access control** of a shared directory: what the user logged on
//!   may do there, tried on the server (list, then make, change and
//!   delete a scratch file). SMB servers keep their permissions to
//!   themselves; they are changed on the server.

#![allow(static_mut_refs)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::book;
use crate::obj;
use crate::pm::*;
use crate::setup::add_page;
use crate::util::z;

/// IBM's menu ids for Logon and Logoff (PMWP's netg_/ser_
/// wpMenuItemSelected) and Access another server.
pub const ID_LOGON: u32 = 0x2c4;
pub const ID_LOGOFF: u32 = 0x2c5;
pub const ID_ACCESS_ANOTHER: u32 = 0x2c8;
/// rs2's own: Access control.
pub const ID_ACCESS_CONTROL: u32 = 0x7a20;

const WC_COMBOBOX: u16 = 2;
const CBS_DROPDOWN: u32 = 0x0002;
const ES_UNREADABLE: u32 = 0x0080;
const ES_READONLY: u32 = 0x0020;
const CBN_LBSELECT: u32 = 4;
const CBN_ENTER: u32 = 7;

pub fn bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

pub fn text(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

pub fn note(t: &str, title: &[u8]) {
    message(HWND_DESKTOP, &bytes(t), title, MB_OK | MB_INFORMATION | MB_MOVEABLE);
}

/// Where a network object is: the group (network), server and resource
/// (any may be empty).
#[derive(Clone, Default)]
pub struct Where {
    pub network: String,
    pub server: String,
    pub resource: String,
}

impl Where {
    /// From a UNC name (`\\SERVER` or `\\SERVER\SHARE`) and the group.
    pub fn of(unc: &str, network: &str) -> Where {
        let t = unc.trim_start_matches('\\');
        let (server, resource) = t.split_once('\\').unwrap_or((t, ""));
        Where { network: String::from(network), server: String::from(server), resource: String::from(resource) }
    }

    pub fn unc(&self) -> String {
        if self.resource.is_empty() { alloc::format!("\\\\{}", self.server) } else { alloc::format!("\\\\{}\\{}", self.server, self.resource) }
    }
}

/// The group a server belongs to: what the requester's server list says,
/// else the requester's own workgroup.
pub fn group_of(server: &str) -> String {
    lanreq::servers(None)
        .ok()
        .and_then(|v| v.into_iter().find(|s| s.name.eq_ignore_ascii_case(server)).map(|s| s.workgroup))
        .filter(|g| !g.is_empty())
        .unwrap_or_else(|| lanreq::status().workgroup)
}

/// A server's comment.
pub fn server_comment(server: &str) -> String {
    lanreq::servers(None).ok().and_then(|v| v.into_iter().find(|s| s.name.eq_ignore_ascii_case(server)).map(|s| s.comment)).unwrap_or_default()
}

/// A share's remark.
pub fn share_remark(server: &str, share: &str) -> Option<String> {
    lanreq::shares(server).ok()?.into_iter().find(|s| s.name.eq_ignore_ascii_case(share)).map(|s| s.remark)
}

/// The deviceless connection that logs on to one server.
fn ipc(server: &str) -> String {
    alloc::format!("\\\\{server}\\IPC$")
}

fn server_logged_on(server: &str) -> Option<String> {
    lanreq::uses().ok()?.into_iter().find(|u| u.local.is_empty() && u.remote.eq_ignore_ascii_case(&ipc(server))).map(|u| u.remote)
}

/// The status line of a page: running, logged on, reachable.
pub fn status(w: &Where) -> String {
    let st = lanreq::status();
    if !st.started {
        return String::from("The LAN requester is not running");
    }
    let user = if st.user.is_empty() { String::from("as a guest") } else { alloc::format!("as {}", st.user) };
    if w.server.is_empty() {
        return if st.user.is_empty() { String::from("Not logged on (guest connections)") } else { alloc::format!("Logged on {user} to {}", if st.domain.is_empty() { &st.workgroup } else { &st.domain }) };
    }
    let reach = match lanreq::shares(&w.server) {
        Ok(v) if w.resource.is_empty() || v.iter().any(|s| s.name.eq_ignore_ascii_case(&w.resource)) => true,
        Ok(_) => return String::from("The server does not share it"),
        Err(e) => return alloc::format!("Not available (error {e})"),
    };
    let _ = reach;
    if server_logged_on(&w.server).is_some() { alloc::format!("Available, logged on to the server") } else { alloc::format!("Available, connected {user}") }
}

/// The local name (drive or port) a UNC name is assigned to.
pub fn assigned(unc: &str) -> Option<String> {
    lanreq::uses().ok()?.into_iter().find(|u| u.remote.eq_ignore_ascii_case(unc) && !u.local.is_empty()).map(|u| u.local)
}

// ---- The pages ----

struct PageFields {
    labels: Vec<&'static [u8]>,
    values: Vec<String>,
}

static mut PAGES: Vec<(usize, PageFields)> = Vec::new();

extern "C" fn page_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe { WinSetWindowULong(h, QWL_USER, b) };
            if let Some(i) = unsafe { PAGES.iter().position(|p| p.0 == b as usize) } {
                let (_, f) = unsafe { PAGES.remove(i) };
                for (k, v) in f.values.iter().enumerate() {
                    set_item_text(h, 901 + k as u32, &bytes(v));
                }
            }
            0
        }
        WM_COMMAND => 0,
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// A page of read-only fields (label, value), IBM's layout: one line
/// each, from the top.
fn fields_page(o: usize, nb: u32, f: PageFields, tab: &'static [u8]) -> u32 {
    let mut items = Vec::new();
    let mut y = 102i16;
    for (k, l) in f.labels.iter().enumerate() {
        items.push(book::label(l, (6, y, 60)));
        // ES_AUTOSCROLL | ES_MARGIN | ES_READONLY
        items.push(item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | 0x0004 | 0x0008 | ES_READONLY, (70, y, 150, 8), 901 + k as u16));
        y -= 15;
    }
    unsafe { PAGES.push((o, f)) };
    let r = add_page(o, nb, page_proc, items, tab);
    unsafe { PAGES.retain(|p| p.0 != o) };
    r
}

/// wpAddNetworkPage of a network group.
pub fn group_page(o: usize, nb: u32, group: &str) -> u32 {
    let st = lanreq::status();
    let what = if group.eq_ignore_ascii_case(&st.workgroup) {
        String::from("This computer's workgroup")
    } else if st.other_domains.split(',').any(|d| d.eq_ignore_ascii_case(group)) {
        String::from("A domain this computer also uses")
    } else {
        String::from("A workgroup on the network")
    };
    let w = Where { network: String::from(group), ..Where::default() };
    fields_page(o, nb, PageFields { labels: alloc::vec![b"Network", b"Description", b"Status"], values: alloc::vec![String::from(group), what, status(&w)] }, b"~Network\0")
}

/// wpAddServerPage.
pub fn server_page(o: usize, nb: u32, w: &Where) -> u32 {
    fields_page(o, nb, PageFields { labels: alloc::vec![b"Network", b"Server", b"Description", b"Status"], values: alloc::vec![w.network.clone(), w.server.clone(), server_comment(&w.server), status(w)] }, b"~Server\0")
}

/// wpAddResourcePage (and a network printer's network page).
pub fn resource_page(o: usize, nb: u32, w: &Where, printer: bool) -> u32 {
    let local = assigned(&w.unc()).unwrap_or_else(|| String::from("None"));
    let labels: Vec<&'static [u8]> = alloc::vec![b"Network", b"Server", b"Resource", b"Description", b"Status", if printer { b"Assigned port" } else { b"Assigned drive" }];
    let values = alloc::vec![w.network.clone(), w.server.clone(), w.resource.clone(), share_remark(&w.server, &w.resource).unwrap_or_default(), status(w), local];
    fields_page(o, nb, PageFields { labels, values }, if printer { b"~Network\0" } else { b"~Resource\0" })
}

// ---- Logon and logoff ----

const ID_USER: u16 = 950;
const ID_PASSWORD: u16 = 951;
const ID_DOMAIN: u16 = 952;

static mut LOGON: (String, String, String) = (String::new(), String::new(), String::new());

extern "C" fn logon_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            unsafe {
                WinSendDlgItemMsg(h, ID_USER as u32, EM_SETTEXTLIMIT, 20, 0);
                WinSendDlgItemMsg(h, ID_PASSWORD as u32, EM_SETTEXTLIMIT, 127, 0);
                set_item_text(h, ID_USER as u32, &bytes(&LOGON.0));
                set_item_text(h, ID_DOMAIN as u32, &bytes(&LOGON.2));
            }
            0
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            unsafe {
                LOGON = (text(&item_text(h, ID_USER as u32)), text(&item_text(h, ID_PASSWORD as u32)), text(&item_text(h, ID_DOMAIN as u32)));
                WinDismissDlg(h, DID_OK);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// The Logon dialog: logs on to the group (the requester) or to the
/// server `w` names; true when logged on.
pub fn logon(w: &Where) -> bool {
    let st = lanreq::status();
    let whole = w.server.is_empty();
    let mut items = alloc::vec![book::label(b"Log on to:", (8, 116, 100))];
    let mut y = 102i16;
    let mut shown: Vec<(&[u8], String)> = alloc::vec![(b"Network", w.network.clone())];
    if !whole {
        shown.push((b"Server", w.server.clone()));
    }
    if !w.resource.is_empty() {
        shown.push((b"Resource", w.resource.clone()));
    }
    let mut texts = Vec::new();
    for (l, v) in &shown {
        items.push(book::label(l, (16, y, 60)));
        texts.push(bytes(v));
        items.push(item(WC_STATIC, texts.last().unwrap(), WS_VISIBLE | 0x0001, (80, y, 140, 8), 0xffff));
        y -= 12;
    }
    y -= 4;
    items.push(book::label(b"User ID", (8, y, 60)));
    items.push(item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (80, y, 120, 8), ID_USER));
    y -= 14;
    items.push(book::label(b"Password", (8, y, 60)));
    items.push(item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008 | ES_UNREADABLE, (80, y, 120, 8), ID_PASSWORD));
    y -= 14;
    if whole {
        items.push(book::label(b"Domain", (8, y, 60)));
        items.push(item(WC_ENTRYFIELD, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004 | 0x0008, (80, y, 120, 8), ID_DOMAIN));
        y -= 14;
    }
    items.push(item(WC_STATIC, b"The password is not shown as you type it.", WS_VISIBLE | 0x0001, (8, y, 220, 8), 0xffff));
    items.push(item(WC_BUTTON, b"~Log on", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (8, 4, 50, 14), DID_OK as u16));
    items.push(item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (64, 4, 50, 14), DID_CANCEL as u16));
    unsafe {
        LOGON = (st.user.clone(), String::new(), if whole { if w.network.is_empty() { st.workgroup.clone() } else { w.network.clone() } } else { String::new() });
    }
    let tpl = template(dialog_frame(b"Logon", 236, 136), items);
    if run(&tpl, logon_proc) != DID_OK {
        return false;
    }
    let (user, password, domain) = unsafe { LOGON.clone() };
    unsafe { LOGON.1.clear() };
    if user.is_empty() {
        note("Type a user ID to log on with.", b"Logon");
        return false;
    }
    if lanreq::ensure_started(&[]).is_err() {
        note("The LAN requester could not be started.", b"Logon");
        return false;
    }
    let r = if whole {
        lanreq::logon(&user, &password, &domain)
    } else {
        let _ = lanreq::use_del(&ipc(&w.server), true);
        lanreq::use_add("", &ipc(&w.server), Some(&password), Some(&user))
    };
    match r {
        Ok(()) => true,
        Err(e) => {
            note(&alloc::format!("{} could not log on to {} (error {e}): the user ID or password may be wrong.", user, if whole { &w.network } else { &w.server }), b"Logon");
            false
        }
    }
}

/// Logoff: the requester's logon (a group) or the server's connection.
pub fn logoff(w: &Where) {
    let st = lanreq::status();
    if w.server.is_empty() {
        if st.user.is_empty() {
            note("Nobody is logged on to the network.", b"Logoff");
            return;
        }
        if message(HWND_DESKTOP, &bytes(&alloc::format!("Log {} off the network {}?", st.user, w.network)), b"Logoff", MB_YESNO | MB_ICONQUESTION | MB_MOVEABLE) == MBID_YES {
            if let Err(e) = lanreq::logon("", "", "") {
                note(&alloc::format!("Logging off failed (error {e})."), b"Logoff");
            }
        }
        return;
    }
    if server_logged_on(&w.server).is_none() {
        note(&alloc::format!("There is no logon of its own to \\\\{}: it uses the network's.", w.server), b"Logoff");
        return;
    }
    if message(HWND_DESKTOP, &bytes(&alloc::format!("Log off the server \\\\{}?", w.server)), b"Logoff", MB_YESNO | MB_ICONQUESTION | MB_MOVEABLE) == MBID_YES {
        if let Err(e) = lanreq::use_del(&ipc(&w.server), true) {
            note(&alloc::format!("Logging off failed (error {e})."), b"Logoff");
        }
    }
}

// ---- Access another server, network directory or network printer ----

const ID_NET: u16 = 960;
const ID_SRV: u16 = 961;
const ID_RES: u16 = 962;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Server,
    Directory,
    Printer,
}

static mut ACCESS: (Kind, Where) = (Kind::Server, Where { network: String::new(), server: String::new(), resource: String::new() });

fn fill(h: u32, id: u16, items: &[String], sel: &str) {
    unsafe { WinSendDlgItemMsg(h, id as u32, LM_DELETEALL, 0, 0) };
    for (i, s) in items.iter().enumerate() {
        let t = z(&bytes(s));
        unsafe { WinSendDlgItemMsg(h, id as u32, LM_INSERTITEM, 0xffff, t.as_ptr() as u32) };
        if s.eq_ignore_ascii_case(sel) {
            unsafe { WinSendDlgItemMsg(h, id as u32, LM_SELECTITEM, i as u32, 1) };
        }
    }
    if !sel.is_empty() {
        set_item_text(h, id as u32, &bytes(sel));
    }
}

fn groups() -> Vec<String> {
    let st = lanreq::status();
    let mut g: Vec<String> = lanreq::servers(None).unwrap_or_default().into_iter().map(|s| s.workgroup).filter(|g| !g.is_empty()).collect();
    g.push(st.workgroup);
    g.extend(st.other_domains.split(',').map(String::from).filter(|s| !s.is_empty()));
    g.sort();
    g.dedup();
    g
}

fn servers_of(group: &str) -> Vec<String> {
    let mut v: Vec<String> = lanreq::servers(if group.is_empty() { None } else { Some(group) }).unwrap_or_default().into_iter().map(|s| s.name).collect();
    v.sort();
    v
}

fn resources_of(server: &str, kind: Kind) -> Vec<String> {
    let want = if kind == Kind::Printer { 1 } else { 0 };
    lanreq::shares(server).unwrap_or_default().into_iter().filter(|s| !s.name.ends_with('$') && (s.typ & 0xffff) == want).map(|s| s.name).collect()
}

extern "C" fn access_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    let kind = unsafe { ACCESS.0 };
    match msg {
        WM_INITDLG => {
            let w = unsafe { ACCESS.1.clone() };
            fill(h, ID_NET, &groups(), &w.network);
            fill(h, ID_SRV, &servers_of(&w.network), &w.server);
            if kind != Kind::Server {
                fill(h, ID_RES, &resources_of(&w.server, kind), &w.resource);
            }
            0
        }
        WM_CONTROL => {
            let (id, code) = ((a & 0xffff) as u16, a >> 16);
            if code == CBN_LBSELECT || code == CBN_ENTER {
                if id == ID_NET {
                    let g = text(&item_text(h, ID_NET as u32));
                    fill(h, ID_SRV, &servers_of(&g), "");
                } else if id == ID_SRV && kind != Kind::Server {
                    let s = text(&item_text(h, ID_SRV as u32));
                    fill(h, ID_RES, &resources_of(&s, kind), "");
                }
            }
            0
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            let w = Where {
                network: text(&item_text(h, ID_NET as u32)).trim().into(),
                server: String::from(text(&item_text(h, ID_SRV as u32)).trim().trim_start_matches('\\')).to_ascii_uppercase(),
                resource: if kind == Kind::Server { String::new() } else { String::from(text(&item_text(h, ID_RES as u32)).trim()).to_ascii_uppercase() },
            };
            if w.server.is_empty() || (kind != Kind::Server && w.resource.is_empty()) {
                note("Choose or type the server and resource.", b"Network");
                return 0;
            }
            unsafe {
                ACCESS.1 = w;
                WinDismissDlg(h, DID_OK);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

/// IBM's Access another server / network directory / network printer:
/// what the user chose.
pub fn access_another(kind: Kind, start: &Where) -> Option<Where> {
    let _ = lanreq::ensure_started(&[]);
    let title: &[u8] = match kind {
        Kind::Server => b"Access another server",
        Kind::Directory => b"Access another network directory",
        Kind::Printer => b"Access another network printer",
    };
    let combo = |id: u16, y: i16| item(WC_COMBOBOX, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | CBS_DROPDOWN, (60, y - 50, 150, 58), id);
    let mut items = alloc::vec![book::label(b"Network:", (8, 92, 50)), combo(ID_NET, 92), book::label(b"Server:", (8, 74, 50)), combo(ID_SRV, 74)];
    if kind != Kind::Server {
        items.push(book::label(b"Resource:", (8, 56, 50)));
        items.push(combo(ID_RES, 56));
    }
    items.push(item(WC_BUTTON, b"~OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (8, 4, 50, 14), DID_OK as u16));
    items.push(item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (64, 4, 50, 14), DID_CANCEL as u16));
    unsafe { ACCESS = (kind, start.clone()) };
    let tpl = template(dialog_frame(title, 222, 110), items);
    (run(&tpl, access_proc) == DID_OK).then(|| unsafe { ACCESS.1.clone() })
}

// ---- Access control ----

/// What the user may do on a shared directory, tried there: list, make a
/// file, change its attributes, delete it.
pub fn access_control(w: &Where) {
    let unc = w.unc();
    let st = lanreq::status();
    let who = if st.user.is_empty() { String::from("a guest") } else { st.user.clone() };
    let mut find = 0xffff_ffffu32;
    let mut buf = [0u8; 300];
    let mut n = 1u32;
    let pat = z(&bytes(&alloc::format!("{unc}\\*")));
    // FIL_STANDARD, all files and directories.
    let read = unsafe { os2::DosFindFirst(pat.as_ptr(), &mut find, 0x37, buf.as_mut_ptr(), buf.len() as u32, &mut n, 1) } == 0;
    if read {
        unsafe { os2::DosFindClose(find) };
    }
    let probe = z(&bytes(&alloc::format!("{unc}\\RS2ACCES.TMP")));
    let (mut hf, mut act) = (0u32, 0u32);
    let create = unsafe { os2::DosOpen(probe.as_ptr(), &mut hf, &mut act, 0, 0, os2::OPEN_ACTION_CREATE_IF_NEW | os2::OPEN_ACTION_REPLACE_IF_EXISTS, os2::OPEN_ACCESS_READWRITE | 0x0010, core::ptr::null_mut()) } == 0;
    let mut write = false;
    let mut attrib = false;
    let mut delete = false;
    if create {
        let mut w = 0u32;
        write = unsafe { os2::DosWrite(hf, b"rs2".as_ptr(), 3, &mut w) } == 0 && w == 3;
        unsafe { os2::DosClose(hf) };
        attrib = set_readonly(&probe, true) && set_readonly(&probe, false);
        delete = unsafe { os2::DosDelete(probe.as_ptr()) } == 0;
    }
    let yn = |b: bool| if b { "yes" } else { "no" };
    note(
        &alloc::format!(
            "What {who} may do on {unc}, as tried just now:\n\nSee its files: {}\nCreate files: {}\nWrite to files: {}\nChange attributes: {}\nDelete files: {}\n\nThe server keeps the permissions; its administrator changes them there.",
            yn(read),
            yn(create),
            yn(write),
            yn(attrib),
            yn(delete)
        ),
        b"Access control",
    );
}

fn set_readonly(path: &[u8], on: bool) -> bool {
    // FIL_STANDARD: FILESTATUS3, attributes at offset 20.
    let mut fs3 = [0u8; 24];
    if unsafe { os2::DosQueryPathInfo(path.as_ptr(), 1, fs3.as_mut_ptr(), 24) } != 0 {
        return false;
    }
    let mut attr = u32::from_le_bytes([fs3[20], fs3[21], fs3[22], fs3[23]]);
    attr = if on { attr | 1 } else { attr & !1 };
    fs3[20..24].copy_from_slice(&attr.to_le_bytes());
    unsafe { os2::DosSetPathInfo(path.as_ptr(), 1, fs3.as_mut_ptr(), 24, 0) == 0 }
}

/// A modal dialog in the middle of the screen.
fn run(tpl: &[u8], proc_: extern "C" fn(u32, u32, u32, u32) -> u32) -> u32 {
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, HWND_DESKTOP, proc_ as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return DID_CANCEL;
    }
    center(dlg);
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    r
}

/// The network group a folder of the network is in (`\\GROUP`), or the
/// requester's workgroup.
pub fn network_of(o: usize) -> String {
    let mut f = obj::d(o).folder;
    for _ in 0..4 {
        if f == 0 {
            break;
        }
        if crate::wps::is(f, "WPNetgrp") {
            return text(&crate::fs::path(f)).trim_start_matches('\\').into();
        }
        f = obj::d(f).folder;
    }
    lanreq::status().workgroup
}
