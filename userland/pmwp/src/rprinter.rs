//! Network printers (WPRPrinter, IBM's WPPRINT.DLL class; studied with
//! Ghidra: rprt_wpSetup, rprt_wpParseNetId, rprt_wpQueryNetworkId,
//! rprt_wpQueryLocalAlias, rprt_wpNetworkConnect, RPRTNetworkDlgProc,
//! RefreshAlias, AddRemotePrintDest, pdv_pdvNewRemote): a print queue on
//! a LAN server.
//!
//! As in IBM's shell, a network printer prints through a **local alias**:
//! a queue of this computer's spooler whose PRQINFO6 names the server and
//! its queue (pszRemoteComputerName, pszRemoteQueueName). rs2's PMSPL
//! gives such a queue a printer of its own whose port is the server's
//! queue (`\\SERVER\QUEUE`); the spooler prints the jobs with the local
//! printer driver (rs2's PostScript driver) and sends them there through
//! the LAN requester (a print file on the share), which SMB servers
//! (Windows, Samba, macOS) print. Programs choose the alias as they
//! choose any printer.
//!
//! - A server's folder (connections.rs) shows its print queues as network
//!   printers. They are not kept: dragging one out (to the desktop or the
//!   Printers folder) makes a copy that is, and opening one or dropping a
//!   file on it makes its alias.
//! - Its pages: Network (the network, server, resource, its remark, the
//!   status and the assigned port), then the printer's own.
//! - Its menu: Assign port (IBM's Assign a port: NET USE LPTn of the queue,
//!   for programs that print to a port) or Unassign port, Logon and
//!   Logoff of the server, Access another network printer.
//! - Setup strings: NETID=network\\server\queue (IBM's), the printer's
//!   QUEUENAME= for the alias; the Printers folder has a "Network printer"
//!   template that asks which printer with Access another network
//!   printer.
//! - wpQueryPrinterName names the alias (rs2's printer objects name their
//!   local queue), wpQueryComputerName the server, wpQueryLocalAlias the
//!   alias, wpQueryNetworkId `network\\server\queue`.

#![allow(static_mut_refs)]

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::null;

use crate::classes_gen::m;
use crate::methods;
use crate::netpages::{self, bytes, text, Kind, Where};
use crate::obj::{self, OPEN_SETTINGS};
use crate::pm::*;
use crate::som;
use crate::util::{cstr, str_bytes, str_new, z, Str};
use crate::wps::{self, Impl};

unsafe extern "C" {
    #[link_name = "PMSPL.399"]
    fn SplEnumQueue(comp: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.393"]
    fn SplCreateQueue(comp: *const u8, level: u32, buf: *const u8, cb: u32) -> u32;
}

/// IBM's menu ids (wprprint.idl).
const WPPOIDM_RPRTACCESSANOTHER: u32 = 9018;
const WPPOIDM_RPRTCONNECT: u32 = 9019;
const WPPOIDM_RPRTDISCONNECT: u32 = 9020;
const WPPOIDM_RPRTLOGON: u32 = 9021;
const WPPOIDM_RPRTLOGOFF: u32 = 9022;

#[repr(C)]
pub struct RPrinterData {
    network: Str,
    server: Str,
    resource: Str,
    /// wpSetResNetIdentity's string.
    netid: Str,
    /// Shown in a server's folder, not kept.
    browse: u32,
    /// The server could not be reached last time (the icon says so).
    unavailable: u32,
}

fn rd(o: usize) -> &'static mut RPrinterData {
    som::data(wps::ccd("WPRPrinter"), o)
}

pub fn where_of(o: usize) -> Where {
    let d = rd(o);
    Where { network: text(str_bytes(&d.network)), server: text(str_bytes(&d.server)), resource: text(str_bytes(&d.resource)) }
}

fn set_where(o: usize, w: &Where) {
    let d = rd(o);
    d.network = str_new(&bytes(&w.network));
    d.server = str_new(&bytes(&w.server.to_ascii_uppercase()));
    d.resource = str_new(&bytes(&w.resource.to_ascii_uppercase()));
}

/// IBM's network id: `network\\server\resource`.
fn net_id(w: &Where) -> String {
    alloc::format!("{}\\\\{}\\{}", w.network, w.server, w.resource)
}

/// wpParseNetId's reading of a network id: the network before the first
/// `\` (a `:` ends it too), then `\` or `\\`, the server, `\`, the
/// resource. A UNC name alone (`\\SERVER\QUEUE`) has the server's group.
fn parse_net_id(s: &str) -> Option<Where> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("\\\\") {
        let (server, resource) = rest.split_once('\\')?;
        return Some(Where { network: netpages::group_of(server), server: server.into(), resource: resource.into() });
    }
    let (net, rest) = s.split_once('\\')?;
    let net = net.split(':').next().unwrap_or("");
    let rest = rest.strip_prefix('\\').unwrap_or(rest);
    let (server, resource) = rest.split_once('\\')?;
    (!server.is_empty() && !resource.is_empty()).then(|| Where { network: net.into(), server: server.into(), resource: resource.into() })
}

// ---- The local alias ----

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// The spooler's network printer queues: (queue, server, queue there).
pub fn remote_queues() -> Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let (mut ret, mut total, mut need) = (0, 0, 0);
    unsafe { SplEnumQueue(null(), 6, core::ptr::null_mut(), 0, &mut ret, &mut total, &mut need, 0) };
    let mut buf = alloc::vec![0u8; need as usize + 64];
    if unsafe { SplEnumQueue(null(), 6, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) } != 0 {
        return Vec::new();
    }
    let s = |p: u32| cstr(p as *const u8).to_vec();
    (0..ret as usize)
        .map(|i| i * 52)
        .filter(|&o| o + 52 <= buf.len())
        .map(|o| (s(rd32(&buf, o)), s(rd32(&buf, o + 44)), s(rd32(&buf, o + 48))))
        .filter(|q| !q.1.is_empty())
        .collect()
}

/// The alias of `\\SERVER\QUEUE`, if the spooler has one.
fn find_alias(w: &Where) -> Option<Vec<u8>> {
    let server = w.server.trim_start_matches('\\');
    remote_queues()
        .into_iter()
        .find(|(_, c, q)| text(c).trim_start_matches('\\').eq_ignore_ascii_case(server) && text(q).eq_ignore_ascii_case(&w.resource))
        .map(|q| q.0)
}

/// A new alias for `\\SERVER\QUEUE` (SplCreateQueue level 6, as IBM's
/// AddRemotePrintDest; rs2's PMSPL adds its printer): named after the
/// queue, with rs2's PostScript driver.
fn create_alias(w: &Where, descr: &[u8]) -> Option<Vec<u8>> {
    let taken: Vec<Vec<u8>> = crate::printer::queues().into_iter().map(|q| q.name).collect();
    let mut base: Vec<u8> = w.resource.bytes().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).take(8).collect();
    if base.is_empty() || base[0].is_ascii_digit() {
        base = b"NETPRN".to_vec();
    }
    let mut name = base.clone();
    let mut n = 1u32;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        let d = alloc::format!("{n}").into_bytes();
        name = [&base[..base.len().min(8 - d.len().min(7))], &d[..]].concat();
        n += 1;
    }
    let (qz, dz) = (z(&name), z(descr));
    let comp = z(&bytes(&alloc::format!("\\\\{}", w.server)));
    let rq = z(&bytes(&w.resource));
    let drv = b"PSCRIPT.Generic PostScript Printer\0";
    let mut b = Vec::new();
    b.extend_from_slice(&(qz.as_ptr() as u32).to_le_bytes());
    for v in [5u16, 0, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for p in [0u32, b"PMPRINT\0".as_ptr() as u32, 0, dz.as_ptr() as u32] {
        b.extend_from_slice(&p.to_le_bytes());
    }
    b.extend_from_slice(&[0, 0, 0, 0]);
    for p in [b"\0".as_ptr() as u32, drv.as_ptr() as u32, 0, comp.as_ptr() as u32, rq.as_ptr() as u32] {
        b.extend_from_slice(&p.to_le_bytes());
    }
    (unsafe { SplCreateQueue(null(), 6, b.as_ptr(), b.len() as u32) } == 0).then_some(name)
}

/// The object's alias, made when `make` and there is none.
fn alias(o: usize, make: bool) -> Option<Vec<u8>> {
    let w = where_of(o);
    if w.server.is_empty() || w.resource.is_empty() {
        return None;
    }
    let cur = crate::printer::queue_name(o);
    if !cur.is_empty() && crate::printer::queue_info(&cur).is_some() {
        return Some(cur);
    }
    let a = match find_alias(&w) {
        Some(a) => Some(a),
        None if make => {
            let descr = bytes(&alloc::format!("{} on {}", w.resource, w.server));
            create_alias(&w, &descr)
        }
        None => None,
    }?;
    crate::printer::set_queue_name(o, &a);
    if rd(o).browse == 0 {
        obj::call(m::WPObject::wpSaveDeferred, &[o]);
    }
    Some(a)
}

// ---- Objects in a server's folder ----

static mut BROWSE: Vec<(String, usize)> = Vec::new();

/// A server folder's network printer for `\\SERVER\QUEUE` (the awake one,
/// or a new one, not kept).
pub fn browse_object(folder: usize, w: &Where, title: &str) -> usize {
    let unc = w.unc();
    if let Some(&(_, o)) = unsafe { BROWSE.iter().find(|b| b.0.eq_ignore_ascii_case(&unc)) } {
        return o;
    }
    let c = wps::class("WPRPrinter");
    if c == 0 {
        return 0;
    }
    let o = som::sc_call(som::sc::SOM_NEW, &[c]);
    if o == 0 {
        return 0;
    }
    rd(o).browse = 1;
    obj::wake(o, folder, Vec::new());
    set_where(o, w);
    rd(o).netid = str_new(&bytes(&net_id(w)));
    obj::d(o).title = str_new(&bytes(title));
    obj::d(o).style |= obj::OBJSTYLE_NOMOVE | obj::OBJSTYLE_NODELETE | obj::OBJSTYLE_NORENAME;
    let _ = alias(o, false);
    unsafe { BROWSE.push((unc, o)) };
    o
}

// ---- Methods ----

fn copy_out(s: &[u8], buf: *mut u8, pcb: *mut u32) -> u32 {
    if pcb.is_null() {
        return 0;
    }
    unsafe {
        let need = s.len() as u32 + 1;
        if s.is_empty() {
            *pcb = 0;
            return 0;
        }
        if buf.is_null() || *pcb < need {
            *pcb = need;
            return 0;
        }
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
        *buf.add(s.len()) = 0;
        *pcb = need;
    }
    1
}

extern "C" fn wpParseNetId(o: usize, id: *const u8) -> u32 {
    match parse_net_id(&text(cstr(id))) {
        Some(w) => {
            set_where(o, &w);
            1
        }
        None => 0,
    }
}

extern "C" fn wpSetResNetIdentity(o: usize, id: *const u8) -> u32 {
    rd(o).netid = str_new(cstr(id));
    1
}

extern "C" fn wpQueryResNetIdentity(o: usize) -> usize {
    match &rd(o).netid {
        Some(b) => b.as_ptr() as usize,
        None => 0,
    }
}

fn str_ptr(s: &Str) -> usize {
    s.as_ref().map_or(0, |b| b.as_ptr() as usize)
}

extern "C" fn wpQueryNetworkName(o: usize) -> usize {
    str_ptr(&rd(o).network)
}

extern "C" fn wpQueryServerName(o: usize) -> usize {
    str_ptr(&rd(o).server)
}

extern "C" fn wpQueryResourceName(o: usize) -> usize {
    str_ptr(&rd(o).resource)
}

extern "C" fn wpQueryNetworkId(o: usize, buf: *mut u8, pcb: *mut u32) -> u32 {
    let w = where_of(o);
    if w.server.is_empty() {
        return copy_out(b"", buf, pcb);
    }
    copy_out(&bytes(&net_id(&w)), buf, pcb)
}

extern "C" fn wpQueryLocalAlias(o: usize, buf: *mut u8, pcb: *mut u32) -> u32 {
    copy_out(&crate::printer::queue_name(o), buf, pcb)
}

/// wpQueryComputerName: the server (`\\SERVER`).
extern "C" fn wpQueryComputerName(o: usize, buf: *mut u8) -> u32 {
    let s = bytes(&alloc::format!("\\\\{}", where_of(o).server));
    if !buf.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
            *buf.add(s.len()) = 0;
        }
    }
    s.len() as u32
}

extern "C" fn wpSetComputerName(o: usize, name: *const u8) -> u32 {
    let mut w = where_of(o);
    w.server = text(cstr(name)).trim_start_matches('\\').to_ascii_uppercase();
    set_where(o, &w);
    1
}

extern "C" fn wpAddNetworkPage(o: usize, nb: u32) -> u32 {
    netpages::resource_page(o, nb, &where_of(o), true)
}

/// IBM's Assign a port: the free printer ports, LPT1-LPT9.
const ID_PORT: u16 = 22201;
static mut PORT_PICK: String = String::new();

extern "C" fn connect_proc(h: u32, msg: u32, a: u32, b: u32) -> u32 {
    match msg {
        WM_INITDLG => {
            let used: Vec<String> = lanreq::uses().unwrap_or_default().into_iter().map(|u| u.local).collect();
            let mut first = true;
            for n in 1..=9 {
                let p = alloc::format!("LPT{n}");
                if used.iter().any(|u| u.eq_ignore_ascii_case(&p)) {
                    continue;
                }
                let t = z(&bytes(&p));
                let i = unsafe { WinSendDlgItemMsg(h, ID_PORT as u32, LM_INSERTITEM, 0xffff, t.as_ptr() as u32) };
                if first {
                    unsafe { WinSendDlgItemMsg(h, ID_PORT as u32, LM_SELECTITEM, i, 1) };
                    first = false;
                }
            }
            0
        }
        WM_COMMAND if a & 0xffff == DID_OK => {
            unsafe {
                PORT_PICK = text(&item_text(h, ID_PORT as u32));
                WinDismissDlg(h, DID_OK);
            }
            0
        }
        _ => unsafe { WinDefDlgProc(h, msg, a, b) },
    }
}

extern "C" fn wpNetworkConnect(o: usize) -> u32 {
    let w = where_of(o);
    let tpl = template(dialog_frame(b"Assign a port", 160, 60), alloc::vec![
        crate::book::label(b"Assign port", (8, 44, 60)),
        // CBS_DROPDOWNLIST
        item(2, b"", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0004, (70, 8, 70, 46), ID_PORT),
        item(WC_BUTTON, b"OK", WS_VISIBLE | WS_TABSTOP | WS_GROUP | 0x0400, (8, 4, 40, 14), DID_OK as u16),
        item(WC_BUTTON, b"Cancel", WS_VISIBLE | WS_TABSTOP, (54, 4, 40, 14), DID_CANCEL as u16),
    ]);
    let dlg = unsafe { WinCreateDlg(HWND_DESKTOP, HWND_DESKTOP, connect_proc as *const () as u32, tpl.as_ptr(), 0) };
    if dlg == 0 {
        return 0;
    }
    center(dlg);
    let r = unsafe { WinProcessDlg(dlg) };
    unsafe { WinDestroyWindow(dlg) };
    let port = unsafe { core::mem::take(&mut PORT_PICK) };
    if r != DID_OK || port.is_empty() {
        return 0;
    }
    let _ = lanreq::ensure_started(&[]);
    match lanreq::use_add(&port, &w.unc(), None, None) {
        Ok(()) => {
            netpages::note(&alloc::format!("{} now prints to {} as well: programs that print to the port print there.", port, w.unc()), b"Assign a port");
            1
        }
        Err(e) => {
            netpages::note(&alloc::format!("{} could not be assigned to {port} (error {e}).", w.unc()), b"Assign a port");
            0
        }
    }
}

extern "C" fn wpNetworkDisconnect(o: usize) -> u32 {
    let w = where_of(o);
    let Some(l) = netpages::assigned(&w.unc()) else { return 0 };
    match lanreq::use_del(&l, false) {
        Ok(()) => 1,
        Err(e) => {
            netpages::note(&alloc::format!("{l} could not be unassigned (error {e})."), b"Unassign port");
            0
        }
    }
}

extern "C" fn wpNetworkLogon(o: usize) -> u32 {
    netpages::logon(&where_of(o)) as u32
}

extern "C" fn wpNetworkLogoff(o: usize) -> u32 {
    netpages::logoff(&where_of(o));
    1
}

/// wpAuthenticate: the server can be reached (the requester started),
/// else the user is asked to log on.
extern "C" fn wpAuthenticate(o: usize) -> u32 {
    let w = where_of(o);
    if lanreq::ensure_started(&[]).is_err() {
        return 0;
    }
    match lanreq::shares(&w.server) {
        Ok(_) => {
            rd(o).unavailable = 0;
            1
        }
        Err(lanreq::NERR_BAD_PASSWORD) | Err(lanreq::NERR_BAD_USERNAME) | Err(5) => netpages::logon(&w) as u32,
        Err(_) => {
            rd(o).unavailable = 1;
            0
        }
    }
}

/// wpCreateAnotherPrinter: IBM's Access another network printer; the new
/// printer goes where this one is (the Printers folder for a server's
/// own).
extern "C" fn wpCreateAnotherPrinter(o: usize) -> u32 {
    let start = where_of(o);
    let Some(w) = netpages::access_another(Kind::Printer, &start) else { return 0 };
    let folder_id: &[u8] = if rd(o).browse != 0 { b"<WP_PRINTERSFOLDER>" } else { b"" };
    (make_printer(&w, folder_id, obj::d(o).folder) != 0) as u32
}

/// A kept network printer for `w` in a folder (by object ID, else the
/// folder object).
fn make_printer(w: &Where, folder_id: &[u8], folder: usize) -> usize {
    let title = bytes(&alloc::format!("{} on {}", w.resource, w.server));
    let setup = bytes(&alloc::format!("NETID={};", net_id(w)));
    let o = if !folder_id.is_empty() {
        crate::api::create_object(b"WPRPrinter", &title, &setup, folder_id, 0) as usize
    } else {
        let c = wps::class("WPRPrinter");
        let t = z(&title);
        let s = z(&setup);
        wps::call("M_WPObject", m::M_WPObject::wpclsNew, &[c, t.as_ptr() as usize, s.as_ptr() as usize, folder, 0])
    };
    if o == 0 {
        netpages::note(&alloc::format!("A printer for {} could not be made.", w.unc()), b"Network printer");
    }
    o
}

// ---- Overrides ----

extern "C" fn rprt_setup(o: usize, setup: *const u8) -> u32 {
    let r = som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpSetup, &[o, setup as usize]);
    for (k, v) in obj::setup_pairs(cstr(setup)) {
        if k == b"NETID" {
            if let Some(w) = parse_net_id(&text(&v)) {
                set_where(o, &w);
                rd(o).netid = str_new(&v);
                let template = obj::call(m::WPObject::wpQueryStyle, &[o]) as u32 & obj::OBJSTYLE_TEMPLATE != 0;
                if !template {
                    alias(o, true);
                }
                obj::call(m::WPObject::wpSaveDeferred, &[o]);
            }
        }
    }
    r as u32
}

extern "C" fn rprt_save_state(o: usize) -> u32 {
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpSaveState, &[o]);
    let w = where_of(o);
    if !w.server.is_empty() {
        let id = z(&bytes(&net_id(&w)));
        obj::wpSaveString(o, b"WPRPrinter\0".as_ptr(), 1, id.as_ptr());
    }
    1
}

extern "C" fn rprt_restore_state(o: usize, r: u32) -> u32 {
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpRestoreState, &[o, r as usize]);
    if let Some(id) = obj::restored_string(o, b"WPRPrinter", 1) {
        let id = id.strip_suffix(&[0]).unwrap_or(&id).to_vec();
        if let Some(w) = parse_net_id(&text(&id)) {
            set_where(o, &w);
        }
        rd(o).netid = str_new(&id);
    }
    1
}

/// A server folder's network printer is not kept: it has a handle as a
/// transient object has (IBM's trn_wpQueryHandle form, with numbers
/// from 0x8000 up), not an abstract object's number.
static mut BROWSE_SEQ: u32 = 0x8000;

extern "C" fn rprt_query_handle(o: usize) -> u32 {
    if rd(o).browse != 0 && obj::d(o).handle == 0 {
        let n = unsafe {
            BROWSE_SEQ += 1;
            BROWSE_SEQ
        };
        let h = (n & 0xff_0000) << 8 | 0x4_0000 | (n & 0xffff);
        obj::set_handle(o, h);
        wps::register(o, h, b"");
        return h;
    }
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpQueryHandle, &[o]) as u32
}

/// Objects in a server's folder are not kept.
extern "C" fn rprt_save_immediate(o: usize) -> u32 {
    if rd(o).browse != 0 {
        return 1;
    }
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpSaveImmediate, &[o]) as u32
}

extern "C" fn rprt_uninit(o: usize) {
    let d = rd(o);
    for s in [&mut d.network, &mut d.server, &mut d.resource, &mut d.netid] {
        *s = None;
    }
    unsafe { BROWSE.retain(|b| b.1 != o) };
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpUnInitData, &[o]);
}

/// A copy is kept, can move, and shares the alias.
extern "C" fn rprt_copy(o: usize, folder: usize, lock: u32) -> usize {
    let n = som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpCopyObject, &[o, folder, lock as usize]);
    if n != 0 && wps::is(n, "WPRPrinter") {
        rd(n).browse = 0;
        set_where(n, &where_of(o));
        rd(n).netid = str_new(&bytes(&net_id(&where_of(o))));
        obj::d(n).style &= !(obj::OBJSTYLE_NOMOVE | obj::OBJSTYLE_NODELETE | obj::OBJSTYLE_NORENAME);
        alias(n, true);
        obj::call(m::WPObject::wpSaveImmediate, &[n]);
    }
    n
}

extern "C" fn rprt_open(o: usize, cnr: u32, view: u32, param: u32) -> u32 {
    if view != OPEN_SETTINGS && wps::is(o, "WPRPrinter") && !where_of(o).server.is_empty() && alias(o, true).is_none() {
        netpages::note(&alloc::format!("{} could not be set up on this computer.", where_of(o).unc()), b"Network printer");
        return 0;
    }
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpOpen, &[o, cnr as usize, view as usize, param as usize]) as u32
}

extern "C" fn rprt_drag_over(o: usize, cnr: u32, pdinfo: u32) -> u32 {
    if crate::printer::queue_name(o).is_empty() && !where_of(o).server.is_empty() {
        return match crate::dnd::dropped_files(pdinfo) {
            Some(v) if !v.is_empty() => mp(DOR_DROP, DO_COPY),
            _ => mp(DOR_NODROPOP, 0),
        };
    }
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpDragOver, &[o, cnr as usize, pdinfo as usize]) as u32
}

extern "C" fn rprt_drop(o: usize, cnr: u32, pdinfo: u32, it: u32) -> u32 {
    alias(o, true);
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpDrop, &[o, cnr as usize, pdinfo as usize, it as usize]) as u32
}

extern "C" fn rprt_add_pages(o: usize, nb: u32) -> u32 {
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpAddSettingsPages, &[o, nb as usize]);
    if !where_of(o).server.is_empty() {
        wps::call("WPRPrinter", m::WPRPrinter::wpAddNetworkPage, &[o, nb as usize]);
    }
    1
}

#[repr(C)]
struct MenuItem {
    pos: i16,
    style: u16,
    attr: u16,
    id: u16,
    sub: u32,
    item: u32,
}

fn add_item(menu: u32, id: u32, t: &str) {
    let mi = MenuItem { pos: -1, style: 0x0001, attr: 0, id: id as u16, sub: 0, item: 0 };
    let t = z(&bytes(t));
    send(menu, 0x0180, &mi as *const MenuItem as u32, t.as_ptr() as u32);
}

extern "C" fn rprt_modify_menu(o: usize, menu: u32, cnr: u32, pos: u32) -> u32 {
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpModifyPopupMenu, &[o, menu as usize, cnr as usize, pos as usize]);
    let w = where_of(o);
    if w.server.is_empty() {
        return 1;
    }
    match netpages::assigned(&w.unc()) {
        Some(l) => add_item(menu, WPPOIDM_RPRTDISCONNECT, &alloc::format!("Unassign port {l}")),
        None => add_item(menu, WPPOIDM_RPRTCONNECT, "Assi~gn port..."),
    }
    add_item(menu, WPPOIDM_RPRTLOGON, "Log~on...");
    add_item(menu, WPPOIDM_RPRTLOGOFF, "Log~off");
    add_item(menu, WPPOIDM_RPRTACCESSANOTHER, "Access a~nother network printer...");
    1
}

extern "C" fn rprt_menu_selected(o: usize, frame: u32, id: u32) -> u32 {
    let c = |i: usize| wps::call("WPRPrinter", i, &[o]) as u32;
    match id {
        WPPOIDM_RPRTCONNECT => c(m::WPRPrinter::wpNetworkConnect),
        WPPOIDM_RPRTDISCONNECT => c(m::WPRPrinter::wpNetworkDisconnect),
        WPPOIDM_RPRTLOGON => c(m::WPRPrinter::wpNetworkLogon),
        WPPOIDM_RPRTLOGOFF => c(m::WPRPrinter::wpNetworkLogoff),
        WPPOIDM_RPRTACCESSANOTHER => c(m::WPRPrinter::wpCreateAnotherPrinter),
        _ => som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpMenuItemSelected, &[o, frame as usize, id as usize]) as u32,
    }
}

extern "C" fn rprt_icon(o: usize) -> u32 {
    if obj::d(o).icon == 0 {
        return net_icon(if rd(o).unavailable != 0 { pmwin::neticons::NET_PRINTER_OFF } else { pmwin::neticons::NET_PRINTER });
    }
    som::call_parent(wps::ccd("WPRPrinter"), wps::cd("WPObject"), m::WPObject::wpQueryIcon, &[o]) as u32
}

extern "C" fn rprtM_icon(_cls: usize) -> u32 {
    net_icon(pmwin::neticons::NET_PRINTER)
}

extern "C" fn rprtM_title(_cls: usize) -> usize {
    b"Network printer\0".as_ptr() as usize
}

static mut ICONS: [u32; pmwin::neticons::LAST as usize + 1] = [0; pmwin::neticons::LAST as usize + 1];

/// One of rs2's network icons (made once).
pub fn net_icon(id: u32) -> u32 {
    if id == 0 || id > pmwin::neticons::LAST {
        return 0;
    }
    let c = unsafe { &mut ICONS };
    if c[id as usize] == 0 {
        c[id as usize] = pmwin::neticons::icon(id).map_or(0, |(big, mini)| crate::pm::make_icon(&big, &mini));
    }
    c[id as usize]
}

/// A network printer made from the template: Access another network
/// printer says which (cancelled, the new object goes again).
pub fn copied_from_template(o: usize) {
    if !where_of(o).server.is_empty() {
        alias(o, true);
        return;
    }
    match netpages::access_another(Kind::Printer, &Where { network: lanreq::status().workgroup, ..Where::default() }) {
        Some(w) => {
            set_where(o, &w);
            rd(o).netid = str_new(&bytes(&net_id(&w)));
            let t = z(&bytes(&alloc::format!("{} on {}", w.resource, w.server)));
            obj::call(m::WPObject::wpSetTitle, &[o, t.as_ptr() as usize]);
            if alias(o, true).is_none() {
                netpages::note(&alloc::format!("{} could not be set up on this computer.", w.unc()), b"Network printer");
            }
            obj::call(m::WPObject::wpSaveImmediate, &[o]);
        }
        None => {
            obj::call(m::WPObject::wpDelete, &[o, 0]);
        }
    }
}

/// The Printers folder's "Network printer" template (once: deleted, it
/// stays deleted).
pub fn ensure_template() {
    const FLAG: &[u8] = b"Rs2NetPrinterTemplate";
    if crate::util::prf_read(crate::util::HINI_USER, b"PM_Workplace", FLAG).is_some() {
        return;
    }
    if wps::handle_of_id(b"<WP_PRINTERSFOLDER>").is_none() {
        return;
    }
    if wps::handle_of_id(b"<RS2_NEWNETPRINTER>").is_none() {
        crate::api::create_object(b"WPRPrinter", b"Network printer", b"TEMPLATE=YES;OBJECTID=<RS2_NEWNETPRINTER>;", b"<WP_PRINTERSFOLDER>", 0);
    }
    crate::util::prf_write(crate::util::HINI_USER, b"PM_Workplace", FLAG, Some(b"1\0"));
}

methods! {
    __m_rprt_parse => wpParseNetId, 2;
    __m_rprt_setid => wpSetResNetIdentity, 2;
    __m_rprt_queryid => wpQueryResNetIdentity, 1;
    __m_rprt_netname => wpQueryNetworkName, 1;
    __m_rprt_srvname => wpQueryServerName, 1;
    __m_rprt_resname => wpQueryResourceName, 1;
    __m_rprt_netid => wpQueryNetworkId, 3;
    __m_rprt_alias => wpQueryLocalAlias, 3;
    __m_rprt_compname => wpQueryComputerName, 2;
    __m_rprt_setcomp => wpSetComputerName, 2;
    __m_rprt_netpage => wpAddNetworkPage, 2;
    __m_rprt_connect => wpNetworkConnect, 1;
    __m_rprt_disconnect => wpNetworkDisconnect, 1;
    __m_rprt_logon => wpNetworkLogon, 1;
    __m_rprt_logoff => wpNetworkLogoff, 1;
    __m_rprt_auth => wpAuthenticate, 1;
    __m_rprt_another => wpCreateAnotherPrinter, 1;
    __m_rprt_setup => rprt_setup, 2;
    __m_rprt_save => rprt_save_state, 1;
    __m_rprt_restore => rprt_restore_state, 2;
    __m_rprt_saveimm => rprt_save_immediate, 1;
    __m_rprt_handle => rprt_query_handle, 1;
    __m_rprt_uninit => rprt_uninit, 1;
    __m_rprt_copy => rprt_copy, 3;
    __m_rprt_open => rprt_open, 4;
    __m_rprt_dragover => rprt_drag_over, 3;
    __m_rprt_drop => rprt_drop, 4;
    __m_rprt_pages => rprt_add_pages, 2;
    __m_rprt_menu => rprt_modify_menu, 4;
    __m_rprt_selected => rprt_menu_selected, 3;
    __m_rprt_icon => rprt_icon, 1;
    __m_rprtM_icon => rprtM_icon, 1;
    __m_rprtM_title => rprtM_title, 1;
}

pub fn impl_rprinter() -> Impl {
    Impl {
        data: core::mem::size_of::<RPrinterData>() as u32,
        methods: alloc::vec![
            ("wpParseNetId", __m_rprt_parse as usize),
            ("wpSetResNetIdentity", __m_rprt_setid as usize),
            ("wpQueryResNetIdentity", __m_rprt_queryid as usize),
            ("wpQueryNetworkName", __m_rprt_netname as usize),
            ("wpQueryServerName", __m_rprt_srvname as usize),
            ("wpQueryResourceName", __m_rprt_resname as usize),
            ("wpQueryNetworkId", __m_rprt_netid as usize),
            ("wpQueryLocalAlias", __m_rprt_alias as usize),
            ("wpAddNetworkPage", __m_rprt_netpage as usize),
            ("wpNetworkConnect", __m_rprt_connect as usize),
            ("wpNetworkDisconnect", __m_rprt_disconnect as usize),
            ("wpNetworkLogon", __m_rprt_logon as usize),
            ("wpNetworkLogoff", __m_rprt_logoff as usize),
            ("wpAuthenticate", __m_rprt_auth as usize),
            ("wpCreateAnotherPrinter", __m_rprt_another as usize),
        ],
        overrides: alloc::vec![
            ("wpQueryComputerName", __m_rprt_compname as usize),
            ("wpSetComputerName", __m_rprt_setcomp as usize),
            ("wpSetup", __m_rprt_setup as usize),
            ("wpSaveState", __m_rprt_save as usize),
            ("wpRestoreState", __m_rprt_restore as usize),
            ("wpSaveImmediate", __m_rprt_saveimm as usize),
            ("wpQueryHandle", __m_rprt_handle as usize),
            ("wpUnInitData", __m_rprt_uninit as usize),
            ("wpCopyObject", __m_rprt_copy as usize),
            ("wpOpen", __m_rprt_open as usize),
            ("wpDragOver", __m_rprt_dragover as usize),
            ("wpDrop", __m_rprt_drop as usize),
            ("wpAddSettingsPages", __m_rprt_pages as usize),
            ("wpModifyPopupMenu", __m_rprt_menu as usize),
            ("wpMenuItemSelected", __m_rprt_selected as usize),
            ("wpQueryIcon", __m_rprt_icon as usize),
        ],
    }
}

pub fn impl_m_rprinter() -> Impl {
    Impl { data: 0, methods: Vec::new(), overrides: alloc::vec![("wpclsQueryIcon", __m_rprtM_icon as usize), ("wpclsQueryTitle", __m_rprtM_title as usize)] }
}
