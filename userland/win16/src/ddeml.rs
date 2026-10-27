//! DDEML: the DDE management library (DdeInitialize, DdeNameService,
//! DdeConnect, DdeClientTransaction, DdePostAdvise, string and data
//! handles...), built here on the WM_DDE_* message protocol (dde.rs
//! carries it between tasks), as Windows' DDEML.DLL is: so DDEML
//! programs talk to programs that use the messages themselves.
//!
//! Each instance has a hidden top-level window that answers
//! WM_DDE_INITIATE for its services (and a server conversation window
//! for each conversation it accepts); each client conversation has a
//! window of its own. Their window procedure is WIN16 entry 10
//! (`p_ddeml`). String handles (HSZ) are global atoms; data handles
//! (HDDEDATA) are global blocks of this task. Transactions wait in a
//! message loop of their own (synchronous) or complete later with
//! XTYP_XACT_COMPLETE (TIMEOUT_ASYNC).

use alloc::vec::Vec;
use thunk16::{flat, Pascal, PascalCall};

use crate::dde::*;
use crate::kernel::{global_alloc, global_free, global_size, GMEM_DDESHARE};
use crate::user::{self, post, send, wnd};
use crate::Global;

const XTYPF_NODATA: u16 = 0x0004;
const XTYPF_ACKREQ: u16 = 0x0008;
const XTYP_ERROR: u16 = 0x8002;
const XTYP_ADVDATA: u16 = 0x4010;
const XTYP_ADVREQ: u16 = 0x2022;
const XTYP_ADVSTART: u16 = 0x1030;
const XTYP_ADVSTOP: u16 = 0x8040;
const XTYP_EXECUTE: u16 = 0x4050;
const XTYP_CONNECT: u16 = 0x1062;
const XTYP_CONNECT_CONFIRM: u16 = 0x8072;
const XTYP_XACT_COMPLETE: u16 = 0x8080;
const XTYP_POKE: u16 = 0x4090;
const XTYP_REQUEST: u16 = 0x20b0;
const XTYP_DISCONNECT: u16 = 0x80c2;
const XTYP_WILDCONNECT: u16 = 0x20e2;
const XTYP_MASK: u16 = 0x00f0;

const DMLERR_DLL_NOT_INITIALIZED: u16 = 0x4003;
const DMLERR_EXECACKTIMEOUT: u16 = 0x4005;
const DMLERR_INVALIDPARAMETER: u16 = 0x4006;
const DMLERR_MEMORY_ERROR: u16 = 0x4008;
const DMLERR_NOTPROCESSED: u16 = 0x4009;
const DMLERR_NO_CONV_ESTABLISHED: u16 = 0x400a;
const DMLERR_POKEACKTIMEOUT: u16 = 0x400b;
const DMLERR_POSTMSG_FAILED: u16 = 0x400c;
const DMLERR_DATAACKTIMEOUT: u16 = 0x4002;
const DMLERR_ADVACKTIMEOUT: u16 = 0x4000;
const DMLERR_UNADVACKTIMEOUT: u16 = 0x4010;
const DMLERR_REENTRANCY: u16 = 0x400d;

const DDE_FACK: u16 = 0x8000;
const DDE_FBUSY: u16 = 0x4000;

const CBF_FAIL_CONNECTIONS: u32 = 0x2000;
const CBF_FAIL_ADVISES: u32 = 0x4000;
const CBF_FAIL_EXECUTES: u32 = 0x8000;
const CBF_FAIL_POKES: u32 = 0x10000;
const CBF_FAIL_REQUESTS: u32 = 0x20000;
const CBF_SKIP_CONNECT_CONFIRMS: u32 = 0x40000;
const CBF_SKIP_DISCONNECTS: u32 = 0x200000;
const APPCMD_CLIENTONLY: u32 = 0x10;

const DNS_REGISTER: u16 = 1;
const DNS_UNREGISTER: u16 = 2;

const ST_CONNECTED: u16 = 1;
const ST_ADVISE: u16 = 2;
const ST_ISLOCAL: u16 = 4;
const ST_BLOCKED: u16 = 8;
const ST_CLIENT: u16 = 16;
const ST_TERMINATED: u16 = 32;
const ST_INLIST: u16 = 64;

const TIMEOUT_ASYNC: u32 = 0xffff_ffff;
const QID_SYNC: u32 = 0xffff_ffff;

struct Inst {
    id: u32,
    callback: u32,
    flags: u32,
    error: u16,
    services: Vec<u16>,
    /// The window that answers WM_DDE_INITIATE.
    wnd: u16,
    /// Callbacks held (DdeEnableCallback EC_DISABLE).
    blocked: bool,
}

/// What a client transaction waits for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Wait {
    None,
    Data,
    Ack,
}

struct Advise {
    item: u16,
    fmt: u16,
    defer: bool,
    ack_req: bool,
}

struct Conv {
    h: u32,
    inst: u32,
    /// Our window and the partner's.
    wnd: u16,
    partner: u16,
    client: bool,
    service: u16,
    topic: u16,
    connected: bool,
    user: u32,
    list: u32,
    /// Server: the advise loops. Client: the ones started.
    advises: Vec<Advise>,
    /// The client's transaction in progress: type, item, format, its id.
    xact: Option<(u16, u16, u16, u32, bool)>,
    wait: Wait,
    /// The result: data handle, status.
    result: (u32, u16),
    /// Data or a message to free on a positive or negative ack.
    pending_mem: u16,
}

struct Ddeml {
    insts: Vec<Inst>,
    convs: Vec<Conv>,
    /// Data handles: (handle, global block, size, format, item, owned).
    data: Vec<(u32, u16, u32, u16, u16, bool)>,
    next: u32,
    /// A connection being made: (instance, window, list, single).
    connecting: Option<(u32, u16, u32, bool)>,
}

static D: Global<Ddeml> = Global::new(Ddeml { insts: Vec::new(), convs: Vec::new(), data: Vec::new(), next: 1, connecting: None });

fn d() -> &'static mut Ddeml {
    D.get()
}

fn next() -> u32 {
    let n = d().next;
    d().next += 1;
    n
}

fn inst(id: u32) -> Option<&'static mut Inst> {
    d().insts.iter_mut().find(|i| i.id == id)
}

fn conv(h: u32) -> Option<&'static mut Conv> {
    d().convs.iter_mut().find(|c| c.h == h)
}

fn set_error(id: u32, e: u16) {
    if let Some(i) = inst(id) {
        i.error = e;
    }
}

// ---- Data handles ----

fn new_data(bytes: &[u8], fmt: u16, item: u16, owned: bool) -> u32 {
    let g = global_alloc(0x0002 | GMEM_DDESHARE, bytes.len().max(1) as u32);
    if g == 0 {
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), flat(((g | 7) as u32) << 16), bytes.len()) };
    let h = 0x0d00_0000 | next();
    d().data.push((h, g | 7, bytes.len() as u32, fmt, item, owned));
    h
}

fn data_of(h: u32) -> Option<&'static mut (u32, u16, u32, u16, u16, bool)> {
    d().data.iter_mut().find(|x| x.0 == h)
}

fn data_bytes(h: u32) -> Option<Vec<u8>> {
    let &mut (_, g, n, ..) = data_of(h)?;
    Some(unsafe { core::slice::from_raw_parts(flat((g as u32) << 16), n as usize) }.to_vec())
}

fn free_data(h: u32) -> bool {
    let Some(i) = d().data.iter().position(|x| x.0 == h) else { return false };
    let (_, g, ..) = d().data.remove(i);
    global_free(g);
    true
}

/// Free a data handle DDEML made for a callback (not the program's own).
fn free_unowned(h: u32) {
    if data_of(h).is_some_and(|x| !x.5) {
        free_data(h);
    }
}

/// A WM_DDE_DATA or WM_DDE_POKE block (flags, format, value) of a data
/// handle: a GMEM_DDESHARE global.
fn dde_block(flags: u16, fmt: u16, bytes: &[u8]) -> u16 {
    let g = global_alloc(0x0002 | GMEM_DDESHARE, bytes.len() as u32 + 4);
    if g == 0 {
        return 0;
    }
    let p = flat(((g | 7) as u32) << 16);
    unsafe {
        (p as *mut u16).write_unaligned(flags);
        (p.add(2) as *mut u16).write_unaligned(fmt);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(4), bytes.len());
    }
    g
}

/// A global block's bytes.
fn block(g: u16) -> Vec<u8> {
    let n = global_size(g) as usize;
    let n = if n == 0 { 0 } else { n };
    unsafe { core::slice::from_raw_parts(flat(((g | 7) as u32) << 16), n) }.to_vec()
}

// ---- The callback ----

#[allow(clippy::too_many_arguments)]
fn callback(id: u32, ty: u16, fmt: u16, hconv: u32, hsz1: u32, hsz2: u32, data: u32, d1: u32, d2: u32) -> u32 {
    let Some(i) = inst(id) else { return 0 };
    let fl = i.flags;
    let fail = match ty {
        XTYP_CONNECT | XTYP_WILDCONNECT => fl & CBF_FAIL_CONNECTIONS != 0,
        XTYP_ADVSTART | XTYP_ADVSTOP => fl & CBF_FAIL_ADVISES != 0,
        XTYP_EXECUTE => fl & CBF_FAIL_EXECUTES != 0,
        XTYP_POKE => fl & CBF_FAIL_POKES != 0,
        XTYP_REQUEST | XTYP_ADVREQ => fl & CBF_FAIL_REQUESTS != 0,
        XTYP_CONNECT_CONFIRM => fl & CBF_SKIP_CONNECT_CONFIRMS != 0,
        XTYP_DISCONNECT => fl & CBF_SKIP_DISCONNECTS != 0,
        _ => false,
    };
    if fail || i.callback == 0 {
        return 0;
    }
    PascalCall::new().w(ty).w(fmt).l(hconv).l(hsz1).l(hsz2).l(data).l(d1).l(d2).call(i.callback)
}

// ---- Windows ----

/// The class of DDEML's windows (registered on first use).
fn create_window(hinst: u16) -> u16 {
    static CLASS: Global<bool> = Global::new(false);
    if !*CLASS.get() {
        unsafe extern "C" {
            fn p_ddeml();
        }
        let proc_ = thunk16::far(p_ddeml as *const u8);
        let name = b"DDEMLWnd";
        let ok = crate::mem16::with_str(name, |fname| {
            crate::mem16::with(26, |far, p| {
                let mut b = [0u8; 26];
                b[2..6].copy_from_slice(&proc_.to_le_bytes());
                b[10..12].copy_from_slice(&hinst.max(1).to_le_bytes());
                b[22..26].copy_from_slice(&fname.to_le_bytes());
                unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, 26) };
                user::call(57, &mut crate::commdlg::args(&[crate::commdlg::A::L(far)]))
            })
        });
        *CLASS.get() = ok != 0;
    }
    let Some(ci) = user::find_class_name(b"DDEMLWnd") else { return 0 };
    user::create(&user::Create { exstyle: 0, class: ci, title: b"", title_far: 0, class_far: 0, style: user::WS_POPUP, pos: [0, 0, 0, 0], parent: 0, menu: 0, hinst, param: 0 })
}

/// The DDEML windows' procedure (entry 10).
pub fn wnd_proc(h: u16, m: u16, wp: u16, lp: u32) -> u32 {
    let (lo, hi) = (lp as u16, (lp >> 16) as u16);
    match m {
        WM_DDE_INITIATE => {
            if let Some(id) = d().insts.iter().find(|i| i.wnd == h).map(|i| i.id) {
                initiate(id, wp, lo, hi);
            }
            0
        }
        WM_DDE_ACK if d().connecting.is_some_and(|c| c.1 == h) && !d().convs.iter().any(|c| c.wnd == h && c.partner == wp) => {
            // A server answers our WM_DDE_INITIATE (sent).
            let (id, _, list, single) = d().connecting.unwrap();
            let already = d().convs.iter().any(|c| c.wnd == h && c.connected);
            if single && already {
                post(wp, WM_DDE_TERMINATE, h, 0);
                return 0;
            }
            let hc = 0x0c00_0000 | next();
            d().convs.push(Conv { h: hc, inst: id, wnd: h, partner: wp, client: true, service: lo, topic: hi, connected: true, user: 0, list, advises: Vec::new(), xact: None, wait: Wait::None, result: (0, 0), pending_mem: 0 });
            0
        }
        WM_DDE_TERMINATE => {
            if let Some(c) = d().convs.iter_mut().find(|c| c.wnd == h && c.partner == wp) {
                let hc = c.h;
                terminated(hc, true);
            }
            0
        }
        WM_DDE_ACK | WM_DDE_DATA | WM_DDE_REQUEST | WM_DDE_POKE | WM_DDE_EXECUTE | WM_DDE_ADVISE | WM_DDE_UNADVISE => {
            let Some(hc) = d().convs.iter().find(|c| c.wnd == h && c.partner == wp && c.connected).map(|c| c.h) else {
                // Not a conversation of ours: free what it brought.
                if m == WM_DDE_DATA || m == WM_DDE_POKE || m == WM_DDE_ADVISE {
                    global_free(lo);
                }
                return 0;
            };
            if conv(hc).is_some_and(|c| c.client) { client_msg(hc, m, lo, hi) } else { server_msg(hc, m, lo, hi) }
            0
        }
        _ => user::def_window_proc(h, m, wp, lp),
    }
}

// ---- Server side ----

/// A WM_DDE_INITIATE to an instance's window.
fn initiate(id: u32, client: u16, app: u16, topic: u16) {
    let Some(i) = inst(id) else { return };
    if i.flags & APPCMD_CLIENTONLY != 0 || i.blocked {
        return;
    }
    let services = i.services.clone();
    let hinst = crate::kernel::TASK.get().hinst;
    let accept = |svc: u16| {
        if callback(id, XTYP_CONNECT, 0, 0, topic as u32, svc as u32, 0, 0, (client == 0) as u32) & 0xffff == 0 {
            return;
        }
        let w = create_window(hinst);
        if w == 0 {
            return;
        }
        let hc = 0x0c00_0000 | next();
        d().convs.push(Conv { h: hc, inst: id, wnd: w, partner: client, client: false, service: svc, topic, connected: true, user: 0, list: 0, advises: Vec::new(), xact: None, wait: Wait::None, result: (0, 0), pending_mem: 0 });
        // The ACK carries atoms of our own (the client deletes them).
        let (a, t) = (add_atom_ref(svc), add_atom_ref(topic));
        send(client, WM_DDE_ACK, w, (t as u32) << 16 | a as u32);
        callback(id, XTYP_CONNECT_CONFIRM, 0, hc, topic as u32, svc as u32, 0, 0, 0);
    };
    if app != 0 {
        if services.iter().any(|&s| same_atom(s, app)) {
            accept(app);
        }
    } else {
        // Any service: each of ours whose topic the callback takes.
        for s in services {
            accept(s);
        }
    }
}

fn same_atom(a: u16, b: u16) -> bool {
    a == b || global_atom_name(a).eq_ignore_ascii_case(&global_atom_name(b))
}

fn add_atom_ref(a: u16) -> u16 {
    if a == 0 {
        return 0;
    }
    let n = global_atom_name(a);
    if n.is_empty() { a } else { add_global_atom(&n) }
}

fn ack(c: &Conv, status: u16, hi: u16) {
    post(c.partner, WM_DDE_ACK, c.wnd, (hi as u32) << 16 | status as u32);
}

fn server_msg(hc: u32, m: u16, lo: u16, hi: u16) {
    let Some(c) = conv(hc) else { return };
    let (id, topic) = (c.inst, c.topic as u32);
    match m {
        WM_DDE_REQUEST => {
            let (fmt, item) = (lo, hi);
            let r = callback(id, XTYP_REQUEST, fmt, hc, topic, item as u32, 0, 0, 0);
            let Some(c) = conv(hc) else { return };
            match r {
                0 => ack(c, 0, item),
                data => {
                    let bytes = data_bytes(data).unwrap_or_default();
                    // fResponse | fRelease
                    let g = dde_block(0x1000 | 0x2000, fmt, &bytes);
                    free_unowned(data);
                    if g == 0 || !post(c.partner, WM_DDE_DATA, c.wnd, (add_atom_ref(item) as u32) << 16 | g as u32) {
                        global_free(g);
                    }
                }
            }
        }
        WM_DDE_POKE => {
            let (g, item) = (lo, hi);
            let b = block(g);
            if b.len() < 4 {
                ack(c, 0, item);
                return;
            }
            let (flags, fmt) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
            let data = new_data(&b[4..], fmt, item, false);
            let r = callback(id, XTYP_POKE, fmt, hc, topic, item as u32, data, 0, 0) as u16;
            free_data(data);
            let Some(c) = conv(hc) else { return };
            let status = if r & DDE_FACK != 0 { DDE_FACK | (r & 0xff) } else { r & (DDE_FBUSY | 0xff) };
            ack(c, status, item);
            if status & DDE_FACK != 0 && flags & 0x2000 != 0 {
                global_free(g);
            }
        }
        WM_DDE_EXECUTE => {
            let g = hi;
            let b = block(g);
            let n = b.iter().position(|&x| x == 0).unwrap_or(b.len());
            let data = new_data(&b[..n + (n < b.len()) as usize], 1, 0, false);
            let r = callback(id, XTYP_EXECUTE, 0, hc, topic, 0, data, 0, 0) as u16;
            free_data(data);
            let Some(c) = conv(hc) else { return };
            let status = if r & DDE_FACK != 0 { DDE_FACK | (r & 0xff) } else { r & (DDE_FBUSY | 0xff) };
            ack(c, status, g);
        }
        WM_DDE_ADVISE => {
            let (g, item) = (lo, hi);
            let b = block(g);
            if b.len() < 4 {
                ack(c, 0, item);
                return;
            }
            let (flags, fmt) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
            let ok = callback(id, XTYP_ADVSTART, fmt, hc, topic, item as u32, 0, 0, 0) & 0xffff != 0;
            let Some(c) = conv(hc) else { return };
            if ok {
                c.advises.retain(|a| !(same_atom(a.item, item) && a.fmt == fmt));
                c.advises.push(Advise { item: add_atom_ref(item), fmt, defer: flags & 0x4000 != 0, ack_req: flags & 0x8000 != 0 });
                ack(c, DDE_FACK, item);
                global_free(g);
            } else {
                ack(c, 0, item);
            }
        }
        WM_DDE_UNADVISE => {
            let (fmt, item) = (lo, hi);
            let advs: Vec<(u16, u16)> = c.advises.iter().filter(|a| (item == 0 || same_atom(a.item, item)) && (fmt == 0 || a.fmt == fmt)).map(|a| (a.item, a.fmt)).collect();
            for (it, f) in &advs {
                callback(id, XTYP_ADVSTOP, *f, hc, topic, *it as u32, 0, 0, 0);
            }
            let Some(c) = conv(hc) else { return };
            c.advises.retain(|a| !advs.iter().any(|x| x.0 == a.item && x.1 == a.fmt));
            ack(c, if advs.is_empty() { 0 } else { DDE_FACK }, item);
        }
        _ => {}
    }
}

/// The partner ended the conversation (or we did, `theirs` false).
fn terminated(hc: u32, theirs: bool) {
    let Some(c) = conv(hc) else { return };
    if !c.connected {
        return;
    }
    c.connected = false;
    let (id, w, partner, client) = (c.inst, c.wnd, c.partner, c.client);
    if theirs {
        post(partner, WM_DDE_TERMINATE, w, 0);
    }
    c.wait = Wait::None;
    callback(id, XTYP_DISCONNECT, 0, hc, 0, 0, 0, 0, 0);
    // A server conversation's window goes with it (a client's when it
    // has no other conversation).
    if !d().convs.iter().any(|x| x.wnd == w && x.connected) && (!client || d().connecting.is_none_or(|k| k.1 != w)) {
        user::destroy_window(w);
    }
}

// ---- Client side ----

fn client_msg(hc: u32, m: u16, lo: u16, hi: u16) {
    let Some(c) = conv(hc) else { return };
    let id = c.inst;
    match m {
        WM_DDE_ACK => {
            let status = lo;
            if c.wait == Wait::Ack || c.wait == Wait::Data {
                c.result.1 = status;
                c.wait = Wait::None;
                if c.pending_mem != 0 {
                    // The data (or commands) of a refused poke, advise or
                    // execute is ours to free; an execute's in any case.
                    let ty = c.xact.map_or(0, |x| x.0);
                    if status & DDE_FACK == 0 || ty == XTYP_EXECUTE {
                        global_free(c.pending_mem);
                    }
                    c.pending_mem = 0;
                }
                let _ = hi;
                complete(hc);
            }
        }
        WM_DDE_DATA => {
            let (g, item) = (lo, hi);
            let b = block(g);
            if b.len() < 4 {
                return;
            }
            let (flags, fmt) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
            let release = flags & 0x2000 != 0;
            let ack_req = flags & 0x8000 != 0;
            if flags & 0x1000 != 0 && c.wait == Wait::Data {
                // The answer to our request.
                let data = new_data(&b[4..], fmt, item, false);
                c.result = (data, DDE_FACK);
                c.wait = Wait::None;
                if ack_req {
                    ack(c, DDE_FACK, item);
                }
                if release {
                    global_free(g);
                }
                complete(hc);
                return;
            }
            // An advise loop's data.
            let topic = c.topic as u32;
            let data = if b.len() > 4 || flags & 0x4000 == 0 { new_data(&b[4..], fmt, item, false) } else { 0 };
            let r = callback(id, XTYP_ADVDATA, fmt, hc, topic, item as u32, data, 0, 0) as u16;
            free_data(data);
            let Some(c) = conv(hc) else { return };
            if ack_req {
                ack(c, if r & DDE_FACK != 0 { DDE_FACK } else { r & DDE_FBUSY }, item);
            }
            if release && (r & DDE_FACK != 0 || !ack_req) {
                global_free(g);
            }
        }
        _ => {}
    }
}

/// A transaction is answered: an asynchronous one's callback.
fn complete(hc: u32) {
    let Some(c) = conv(hc) else { return };
    let Some((ty, item, fmt, xid, sync)) = c.xact else { return };
    if sync {
        return;
    }
    c.xact = None;
    let (id, topic, (data, status)) = (c.inst, c.topic as u32, c.result);
    let data = if ty == XTYP_REQUEST { data } else { 0 };
    callback(id, XTYP_XACT_COMPLETE, fmt, hc, topic, item as u32, data, xid, status as u32);
    free_unowned(data);
}

/// Wait in a message loop until the conversation's transaction is
/// answered or `ms` have passed: answered?
fn wait(hc: u32, ms: u32) -> bool {
    let hab = crate::kernel::TASK.get().hab;
    let start = unsafe { crate::pm::WinGetCurrentTime(hab) };
    let mut msg = [0u8; 18];
    loop {
        match conv(hc) {
            Some(c) if c.wait != Wait::None && c.connected => {}
            Some(c) => return c.wait == Wait::None,
            None => return false,
        }
        if unsafe { crate::pm::WinGetCurrentTime(hab) }.wrapping_sub(start) >= ms {
            return false;
        }
        if user::get_message(msg.as_mut_ptr(), 0, 0, 0, Some(1)) == 0 {
            unsafe { os2::DosSleep(5) };
            continue;
        }
        if user::read_msg(msg.as_ptr()).is_some_and(|m| m.1 == user::W_QUIT) {
            post(0, user::W_QUIT, user::read_msg(msg.as_ptr()).map_or(0, |m| m.2), 0);
            return false;
        }
        user::translate(msg.as_ptr() as *const u16);
        user::dispatch(msg.as_mut_ptr());
    }
}

#[allow(clippy::too_many_arguments)]
fn client_transaction(pdata: u32, cb: u32, hc: u32, item: u16, fmt: u16, ty: u16, timeout: u32, presult: *mut u32) -> u32 {
    let Some(c) = conv(hc) else { return 0 };
    let id = c.inst;
    if !c.connected || !c.client {
        set_error(id, DMLERR_NO_CONV_ESTABLISHED);
        return 0;
    }
    if c.wait != Wait::None {
        set_error(id, DMLERR_REENTRANCY);
        return 0;
    }
    // The data: bytes, or a data handle (cb -1).
    let bytes = || -> Vec<u8> {
        if cb == 0xffff_ffff {
            let b = data_bytes(pdata).unwrap_or_default();
            free_unowned(pdata);
            b
        } else if pdata == 0 {
            Vec::new()
        } else {
            unsafe { core::slice::from_raw_parts(flat(pdata), cb as usize) }.to_vec()
        }
    };
    let (w, partner) = (c.wnd, c.partner);
    let base = ty & !(XTYPF_NODATA | XTYPF_ACKREQ);
    let (msg, lp, waits, mem, err) = match base {
        XTYP_REQUEST => (WM_DDE_REQUEST, (add_atom_ref(item) as u32) << 16 | fmt as u32, Wait::Data, 0, DMLERR_DATAACKTIMEOUT),
        XTYP_POKE => {
            // fRelease
            let g = dde_block(0x2000, fmt, &bytes());
            if g == 0 {
                set_error(id, DMLERR_MEMORY_ERROR);
                return 0;
            }
            (WM_DDE_POKE, (add_atom_ref(item) as u32) << 16 | g as u32, Wait::Ack, g, DMLERR_POKEACKTIMEOUT)
        }
        XTYP_EXECUTE => {
            let mut b = bytes();
            if b.last() != Some(&0) {
                b.push(0);
            }
            let g = global_alloc(0x0002 | GMEM_DDESHARE, b.len() as u32);
            if g == 0 {
                set_error(id, DMLERR_MEMORY_ERROR);
                return 0;
            }
            unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), flat(((g | 7) as u32) << 16), b.len()) };
            (WM_DDE_EXECUTE, (g as u32) << 16, Wait::Ack, g, DMLERR_EXECACKTIMEOUT)
        }
        XTYP_ADVSTART => {
            let flags = if ty & XTYPF_NODATA != 0 { 0x4000 } else { 0 } | if ty & XTYPF_ACKREQ != 0 { 0x8000 } else { 0 };
            let g = global_alloc(0x0002 | GMEM_DDESHARE, 4);
            if g == 0 {
                set_error(id, DMLERR_MEMORY_ERROR);
                return 0;
            }
            let p = flat(((g | 7) as u32) << 16);
            unsafe {
                (p as *mut u16).write_unaligned(flags);
                (p.add(2) as *mut u16).write_unaligned(fmt);
            }
            (WM_DDE_ADVISE, (add_atom_ref(item) as u32) << 16 | g as u32, Wait::Ack, g, DMLERR_ADVACKTIMEOUT)
        }
        XTYP_ADVSTOP => (WM_DDE_UNADVISE, (add_atom_ref(item) as u32) << 16 | fmt as u32, Wait::Ack, 0, DMLERR_UNADVACKTIMEOUT),
        _ => {
            set_error(id, DMLERR_INVALIDPARAMETER);
            return 0;
        }
    };
    let sync = timeout != TIMEOUT_ASYNC;
    let xid = next();
    if let Some(c) = conv(hc) {
        c.wait = waits;
        c.result = (0, 0);
        c.pending_mem = mem;
        c.xact = Some((base, item, fmt, xid, sync));
    }
    if !post(partner, msg, w, lp) {
        if mem != 0 {
            global_free(mem);
        }
        if let Some(c) = conv(hc) {
            c.wait = Wait::None;
            c.xact = None;
        }
        set_error(id, DMLERR_POSTMSG_FAILED);
        return 0;
    }
    if !sync {
        if !presult.is_null() {
            unsafe { presult.write_unaligned(xid) };
        }
        return 1;
    }
    let answered = wait(hc, timeout.max(1));
    let Some(c) = conv(hc) else { return 0 };
    c.xact = None;
    if !answered {
        c.wait = Wait::None;
        set_error(id, if c.connected { err } else { DMLERR_NO_CONV_ESTABLISHED });
        return 0;
    }
    let (data, status) = c.result;
    if !presult.is_null() {
        unsafe { presult.write_unaligned(status as u32) };
    }
    match base {
        XTYP_REQUEST => {
            if data == 0 {
                set_error(id, DMLERR_NOTPROCESSED);
            }
            data
        }
        _ if status & DDE_FACK != 0 => {
            if base == XTYP_ADVSTART {
                c.advises.push(Advise { item: add_atom_ref(item), fmt, defer: ty & XTYPF_NODATA != 0, ack_req: ty & XTYPF_ACKREQ != 0 });
            }
            if base == XTYP_ADVSTOP {
                c.advises.retain(|a| !(same_atom(a.item, item) && a.fmt == fmt));
            }
            1
        }
        _ => {
            set_error(id, DMLERR_NOTPROCESSED);
            0
        }
    }
}

/// Connect to the servers of a service and topic (0: any): the
/// conversations made.
fn connect(id: u32, service: u16, topic: u16, list: u32, single: bool) -> Vec<u32> {
    let hinst = crate::kernel::TASK.get().hinst;
    let w = create_window(hinst);
    if w == 0 {
        return Vec::new();
    }
    d().connecting = Some((id, w, list, single));
    let (a, t) = (add_atom_ref(service), add_atom_ref(topic));
    send(0xffff, WM_DDE_INITIATE, w, (t as u32) << 16 | a as u32);
    d().connecting = None;
    let made: Vec<u32> = d().convs.iter().filter(|c| c.wnd == w && c.connected).map(|c| c.h).collect();
    if made.is_empty() {
        user::destroy_window(w);
    }
    made
}

/// The handle of a string: its atom.
fn hsz(atom: u16) -> u32 {
    atom as u32
}

pub fn call(ord: u32, a: &mut Pascal) -> u32 {
    match ord {
        // DdeInitialize(lpidInst, pfnCallback, afCmd, ulRes)
        2 => {
            let (pid, cb, fl, _) = (a.p() as *mut u32, a.l(), a.l(), a.l());
            if pid.is_null() {
                return DMLERR_INVALIDPARAMETER as u32;
            }
            let cur = unsafe { pid.read_unaligned() };
            if cur != 0 {
                return match inst(cur) {
                    Some(i) => {
                        i.flags = fl;
                        if cb != 0 {
                            i.callback = cb;
                        }
                        0
                    }
                    None => DMLERR_INVALIDPARAMETER as u32,
                };
            }
            let id = 0x0001_0000 | next();
            let w = create_window(crate::kernel::TASK.get().hinst);
            d().insts.push(Inst { id, callback: cb, flags: fl, error: 0, services: Vec::new(), wnd: w, blocked: false });
            unsafe { pid.write_unaligned(id) };
            0
        }
        // DdeUninitialize(idInst)
        3 => {
            let id = a.l();
            let convs: Vec<u32> = d().convs.iter().filter(|c| c.inst == id && c.connected).map(|c| c.h).collect();
            for hc in convs {
                disconnect(hc);
            }
            let Some(i) = d().insts.iter().position(|i| i.id == id) else { return 0 };
            let ins = d().insts.remove(i);
            for s in ins.services {
                delete_global_atom(s);
            }
            user::destroy_window(ins.wnd);
            d().convs.retain(|c| c.inst != id);
            1
        }
        // DdeConnectList(idInst, hszService, hszTopic, hConvList, pCC)
        4 => {
            let (id, svc, topic, old, _cc) = (a.l(), a.l(), a.l(), a.l(), a.l());
            if inst(id).is_none() {
                return 0;
            }
            let list = if old != 0 { old } else { 0x0e00_0000 | next() };
            let made = connect(id, svc as u16, topic as u16, list, false);
            if made.is_empty() && old == 0 {
                set_error(id, DMLERR_NO_CONV_ESTABLISHED);
                return 0;
            }
            list
        }
        // DdeQueryNextServer(hConvList, hConvPrev)
        5 => {
            let (list, prev) = (a.l(), a.l());
            let convs: Vec<u32> = d().convs.iter().filter(|c| c.list == list && list != 0).map(|c| c.h).collect();
            match prev {
                0 => convs.first().copied().unwrap_or(0),
                p => convs.iter().position(|&c| c == p).and_then(|i| convs.get(i + 1)).copied().unwrap_or(0),
            }
        }
        // DdeDisconnectList(hConvList)
        6 => {
            let list = a.l();
            let convs: Vec<u32> = d().convs.iter().filter(|c| c.list == list && list != 0).map(|c| c.h).collect();
            for hc in convs {
                disconnect(hc);
            }
            1
        }
        // DdeConnect(idInst, hszService, hszTopic, pCC)
        7 => {
            let (id, svc, topic, _cc) = (a.l(), a.l(), a.l(), a.l());
            if inst(id).is_none() {
                return 0;
            }
            match connect(id, svc as u16, topic as u16, 0, true).first() {
                Some(&hc) => hc,
                None => {
                    set_error(id, DMLERR_NO_CONV_ESTABLISHED);
                    0
                }
            }
        }
        // DdeDisconnect(hConv)
        8 => {
            let hc = a.l();
            disconnect(hc) as u32
        }
        // DdeQueryConvInfo(hConv, idTransaction, pConvInfo)
        9 => {
            let (hc, _xid, p) = (a.l(), a.l(), a.p());
            let Some(c) = conv(hc) else { return 0 };
            if p.is_null() {
                return 0;
            }
            let cb = (unsafe { (p as *const u32).read_unaligned() } as usize).clamp(4, 58);
            let mut b = [0u8; 58];
            b[0..4].copy_from_slice(&(cb as u32).to_le_bytes());
            b[4..8].copy_from_slice(&c.user.to_le_bytes());
            b[12..16].copy_from_slice(&hsz(c.service).to_le_bytes());
            b[16..20].copy_from_slice(&hsz(c.service).to_le_bytes());
            b[20..24].copy_from_slice(&hsz(c.topic).to_le_bytes());
            if let Some((ty, item, fmt, _, _)) = c.xact {
                b[24..28].copy_from_slice(&hsz(item).to_le_bytes());
                b[28..30].copy_from_slice(&fmt.to_le_bytes());
                b[30..32].copy_from_slice(&ty.to_le_bytes());
            }
            let mut st = if c.connected { ST_CONNECTED } else { ST_TERMINATED };
            if c.client {
                st |= ST_CLIENT;
            }
            if !c.advises.is_empty() {
                st |= ST_ADVISE;
            }
            if c.list != 0 {
                st |= ST_INLIST;
            }
            if user::wnd(c.partner).is_some() {
                st |= ST_ISLOCAL;
            }
            if inst(c.inst).is_some_and(|i| i.blocked) {
                st |= ST_BLOCKED;
            }
            b[32..34].copy_from_slice(&st.to_le_bytes());
            // XST_CONNECTED
            b[34..36].copy_from_slice(&(if c.connected { 2u16 } else { 0 }).to_le_bytes());
            b[38..42].copy_from_slice(&c.list.to_le_bytes());
            unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), p, cb) };
            cb as u32
        }
        // DdeSetUserHandle(hConv, id, hUser)
        10 => {
            let (hc, _id, hu) = (a.l(), a.l(), a.l());
            match conv(hc) {
                Some(c) => {
                    c.user = hu;
                    1
                }
                None => 0,
            }
        }
        // DdeClientTransaction(pData, cbData, hConv, hszItem, wFmt, wType,
        // dwTimeout, pdwResult)
        11 => {
            let (pdata, cb, hc, item, fmt, ty, timeout, pres) = (a.l(), a.l(), a.l(), a.l(), a.w(), a.w(), a.l(), a.p() as *mut u32);
            client_transaction(pdata, cb, hc, item as u16, fmt, ty, timeout, pres)
        }
        // DdeAbandonTransaction(idInst, hConv, idTransaction)
        12 => {
            let (_id, hc, _xid) = (a.l(), a.l(), a.l());
            if let Some(c) = conv(hc) {
                c.xact = None;
                c.wait = Wait::None;
            }
            1
        }
        // DdePostAdvise(idInst, hszTopic, hszItem)
        13 => {
            let (id, topic, item) = (a.l(), a.l(), a.l());
            post_advise(id, topic as u16, item as u16) as u32
        }
        // DdeCreateDataHandle(idInst, pSrc, cb, cbOff, hszItem, wFmt, afCmd)
        14 => {
            let (_id, src, cb, off, item, fmt, fl) = (a.l(), a.l(), a.l(), a.l(), a.l(), a.w(), a.w());
            let mut bytes = alloc::vec![0u8; (cb + off) as usize];
            if src != 0 {
                unsafe { core::ptr::copy_nonoverlapping(flat(src), bytes.as_mut_ptr().add(off as usize), cb as usize) };
            }
            // HDATA_APPOWNED
            new_data(&bytes, fmt, item as u16, fl & 1 != 0)
        }
        // DdeAddData(hData, pSrc, cb, cbOff)
        15 => {
            let (h, src, cb, off) = (a.l(), a.l(), a.l(), a.l());
            let Some(mut b) = data_bytes(h) else { return 0 };
            let end = (off + cb) as usize;
            if b.len() < end {
                b.resize(end, 0);
            }
            if src != 0 {
                unsafe { core::ptr::copy_nonoverlapping(flat(src), b.as_mut_ptr().add(off as usize), cb as usize) };
            }
            let Some(&mut (_, g, _, fmt, item, owned)) = data_of(h) else { return 0 };
            let ng = crate::kernel::global_realloc(g, b.len() as u32, 0);
            if ng == 0 {
                return 0;
            }
            unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), flat(((ng | 7) as u32) << 16), b.len()) };
            if let Some(x) = data_of(h) {
                *x = (h, ng | 7, b.len() as u32, fmt, item, owned);
            }
            h
        }
        // DdeGetData(hData, pDst, cbMax, cbOff): the bytes (that would be)
        // copied.
        16 => {
            let (h, dst, max, off) = (a.l(), a.l(), a.l(), a.l());
            let Some(b) = data_bytes(h) else { return 0 };
            let avail = b.len().saturating_sub(off as usize);
            if dst == 0 {
                return avail as u32;
            }
            let n = avail.min(max as usize);
            unsafe { core::ptr::copy_nonoverlapping(b.as_ptr().add(off as usize), flat(dst), n) };
            n as u32
        }
        // DdeAccessData(hData, pcbDataSize): a far pointer to the data.
        17 => {
            let (h, pcb) = (a.l(), a.p() as *mut u32);
            let Some(&mut (_, g, n, ..)) = data_of(h) else { return 0 };
            if !pcb.is_null() {
                unsafe { pcb.write_unaligned(n) };
            }
            (g as u32) << 16
        }
        // DdeUnaccessData(hData)
        18 => {
            let h = a.l();
            data_of(h).is_some() as u32
        }
        // DdeFreeDataHandle(hData)
        19 => free_data(a.l()) as u32,
        // DdeGetLastError(idInst)
        20 => {
            let id = a.l();
            match inst(id) {
                Some(i) => core::mem::replace(&mut i.error, 0) as u32,
                None => DMLERR_DLL_NOT_INITIALIZED as u32,
            }
        }
        // DdeCreateStringHandle(idInst, psz, iCodePage)
        21 => {
            let (_id, s, _cp) = (a.l(), a.l(), a.w());
            let Some(name) = crate::far_str(s).filter(|n| !n.is_empty()) else { return 0 };
            hsz(add_global_atom(name))
        }
        // DdeFreeStringHandle(idInst, hsz)
        22 => {
            let (_id, h) = (a.l(), a.l());
            if h != 0 {
                delete_global_atom(h as u16);
            }
            1
        }
        // DdeQueryString(idInst, hsz, psz, cchMax, iCodePage)
        23 => {
            let (_id, h, buf, max, _cp) = (a.l(), a.l(), a.p(), a.l(), a.w());
            let name = global_atom_name(h as u16);
            if buf.is_null() {
                return name.len() as u32;
            }
            user::copy_out(&name, buf, max.min(0x7fff) as i32) as u32
        }
        // DdeKeepStringHandle(idInst, hsz)
        24 => {
            let (_id, h) = (a.l(), a.l());
            (add_atom_ref(h as u16) != 0) as u32
        }
        // DdeEnableCallback(idInst, hConv, wCmd)
        26 => {
            let (id, _hc, cmd) = (a.l(), a.l(), a.w());
            if let Some(i) = inst(id) {
                // EC_ENABLEALL 0, EC_DISABLE ST_BLOCKED, EC_QUERYWAITING 2
                match cmd {
                    0 => i.blocked = false,
                    8 => i.blocked = true,
                    2 => return 0,
                    _ => {}
                }
            }
            1
        }
        // DdeNameService(idInst, hsz1, hsz2, afCmd)
        27 => {
            let (id, h1, _h2, cmd) = (a.l(), a.l(), a.l(), a.w());
            let Some(i) = inst(id) else { return 0 };
            let h1 = h1 as u16;
            if cmd & DNS_REGISTER != 0 && h1 != 0 {
                if !i.services.iter().any(|&s| same_atom(s, h1)) {
                    i.services.push(add_atom_ref(h1));
                }
            }
            if cmd & DNS_UNREGISTER != 0 {
                let gone: Vec<u16> = i.services.iter().copied().filter(|&s| h1 == 0 || same_atom(s, h1)).collect();
                i.services.retain(|s| !gone.contains(s));
                for s in gone {
                    delete_global_atom(s);
                }
            }
            1
        }
        // DdeCmpStringHandles(hsz1, hsz2)
        36 => {
            let (x, y) = (a.l(), a.l());
            let (s, t) = (global_atom_name(x as u16).to_ascii_lowercase(), global_atom_name(y as u16).to_ascii_lowercase());
            match s.cmp(&t) {
                core::cmp::Ordering::Less => 0xffff,
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 1,
            }
        }
        // DdeReconnect(hConv)
        37 => {
            let hc = a.l();
            let Some(c) = conv(hc) else { return 0 };
            if c.connected {
                return hc;
            }
            let (id, s, t) = (c.inst, c.service, c.topic);
            connect(id, s, t, 0, true).first().copied().unwrap_or(0)
        }
        _ => 0,
    }
}

fn disconnect(hc: u32) -> bool {
    let Some(c) = conv(hc) else { return false };
    if !c.connected {
        return false;
    }
    c.connected = false;
    let (w, partner, client) = (c.wnd, c.partner, c.client);
    post(partner, WM_DDE_TERMINATE, w, 0);
    if !d().convs.iter().any(|x| x.wnd == w && x.connected) && (client || wnd(w).is_some()) {
        user::destroy_window(w);
    }
    true
}

/// DdePostAdvise: the data of each advise loop on the topic and item.
fn post_advise(id: u32, topic: u16, item: u16) -> bool {
    let convs: Vec<u32> = d().convs.iter().filter(|c| c.inst == id && !c.client && c.connected && (topic == 0 || same_atom(c.topic, topic))).map(|c| c.h).collect();
    for hc in convs {
        let Some(c) = conv(hc) else { continue };
        let loops: Vec<(u16, u16, bool, bool)> = c.advises.iter().filter(|a| item == 0 || same_atom(a.item, item)).map(|a| (a.item, a.fmt, a.defer, a.ack_req)).collect();
        let ctopic = c.topic;
        let n = loops.len();
        for (k, (it, fmt, defer, ack_req)) in loops.into_iter().enumerate() {
            let data = callback(id, XTYP_ADVREQ, fmt, hc, ctopic as u32, it as u32, 0, (n - k - 1) as u32, 0);
            if data == 0 {
                continue;
            }
            let bytes = if defer { Vec::new() } else { data_bytes(data).unwrap_or_default() };
            free_unowned(data);
            let flags = 0x2000 | if ack_req { 0x8000 } else { 0 };
            let g = dde_block(flags, fmt, &bytes);
            let Some(c) = conv(hc) else { break };
            if g == 0 || !post(c.partner, WM_DDE_DATA, c.wnd, (add_atom_ref(it) as u32) << 16 | g as u32) {
                global_free(g);
            }
        }
    }
    let _ = (XTYP_ERROR, DMLERR_DLL_NOT_INITIALIZED, ST_BLOCKED, QID_SYNC, XTYP_MASK, send as fn(u16, u16, u16, u32) -> u32);
    true
}
