//! DDEML monitors (APPCLASS_MONITOR), as IBM's PMDDEML has them (studied
//! with Ghidra: Win32DdeInitialize, DdeSendMsgHookProc,
//! DdePostMsgHookProc, AgentWmMonitor and the MonBroadcast* functions):
//!
//! - An instance made with APPCLASS_MONITOR sets two system hooks with
//!   this module: HK_SENDMSG and HK_INPUT, so that every PM process
//!   reports the DDE messages it sends (WM_DDE_INITIATE and
//!   WM_DDE_INITIATEACK) and the ones it takes off its queue
//!   (WM_DDE_FIRST to WM_DDE_LAST, PM_REMOVE). A count of monitors (IBM's
//!   cActiveMonitors, in rs2 a named shared block) tells the DDEML
//!   instances of every process whether to report at all.
//! - Each report is a MON*STRUCT, IBM's layout, sent to every frame on the
//!   desktop as AM_MONITOR (0x4B3; mp1 the MF_* kind, mp2 the structure).
//!   A monitor's agent with that MF_* flag in its afCmd gets XTYP_MONITOR
//!   with a data handle of the structure and the kind in dwData2.
//!   IBM passes a data handle of its shared handle table; rs2's handles
//!   are each process's own, so the structure travels in gettable shared
//!   memory and the monitor's process makes the handle.
//!
//! Kinds: string handles made, kept and freed (MF_HSZ_INFO), messages
//! sent and posted (MF_SENDMSGS, MF_POSTMSGS), callbacks (MF_CALLBACKS),
//! errors (MF_ERRORS), advise links started and stopped (MF_LINKS) and
//! conversations made and ended (MF_CONV).

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::DosFreeMem;

use crate::conv::{dispatch, Conv, ST_CLIENT};
use crate::handles::*;
use crate::pm::*;

pub const AM_MONITOR: u32 = 0x4b3;
pub const XTYP_MONITOR: u32 = 0x80f2;

pub const MF_HSZ_INFO: u32 = 0x0100_0000;
pub const MF_SENDMSGS: u32 = 0x0200_0000;
pub const MF_POSTMSGS: u32 = 0x0400_0000;
pub const MF_CALLBACKS: u32 = 0x0800_0000;
pub const MF_ERRORS: u32 = 0x1000_0000;
pub const MF_LINKS: u32 = 0x2000_0000;
pub const MF_CONV: u32 = 0x4000_0000;

// MONHSZSTRUCT fsAction.
pub const MH_CREATE: u16 = 1;
pub const MH_KEEP: u16 = 2;
pub const MH_DELETE: u16 = 3;

const HK_SENDMSG: u32 = 0;
const HK_INPUT: u32 = 1;

unsafe extern "C" {
    #[link_name = "PMWIN.928"]
    fn WinSetHook(hab: u32, hmq: u32, kind: u32, pfn: usize, hmod: u32) -> u32;
    #[link_name = "PMWIN.927"]
    fn WinReleaseHook(hab: u32, hmq: u32, kind: u32, pfn: usize, hmod: u32) -> u32;
    #[link_name = "PMWIN.901"]
    fn WinBroadcastMsg(h: u32, msg: u32, mp1: u32, mp2: u32, fl: u32) -> u32;
    #[link_name = "DOSCALLS.302"]
    fn DosGetSharedMem(p: *const u8, flags: u32) -> u32;
}

/// The monitors' count, shared by every process: `\SHAREMEM\RS2DDEML.MON`.
const COUNT: &[u8] = b"\\SHAREMEM\\RS2DDEML.MON\0";
static mut COUNT_P: *mut u32 = null_mut();

fn count_ptr(create: bool) -> *mut u32 {
    unsafe {
        if COUNT_P.is_null() {
            let mut p: *mut u8 = null_mut();
            if os2::DosGetNamedSharedMem(&mut p, COUNT.as_ptr(), 0x03) == 0 {
                COUNT_P = p as *mut u32;
            } else if create && os2::DosAllocSharedMem(&mut p, COUNT.as_ptr(), 4096, 0x13) == 0 {
                (p as *mut u32).write_volatile(0);
                COUNT_P = p as *mut u32;
            }
        }
        COUNT_P
    }
}

/// Whether a monitor runs anywhere.
pub fn active() -> bool {
    let p = count_ptr(false);
    !p.is_null() && unsafe { p.read_volatile() } != 0
}

fn hmod() -> u32 {
    let mut h = 0;
    unsafe { os2::DosQueryModuleHandle(b"PMDDEML\0".as_ptr(), &mut h) };
    h
}

/// A monitor instance starts: the hooks, one more monitor.
pub fn start() -> bool {
    let m = hmod();
    unsafe {
        if WinSetHook(0, 0, HK_INPUT, post_hook as *const () as usize, m) == 0 {
            return false;
        }
        if WinSetHook(0, 0, HK_SENDMSG, send_hook as *const () as usize, m) == 0 {
            WinReleaseHook(0, 0, HK_INPUT, post_hook as *const () as usize, m);
            return false;
        }
    }
    let p = count_ptr(true);
    if !p.is_null() {
        unsafe { p.write_volatile(p.read_volatile() + 1) };
    }
    true
}

pub fn stop() {
    let m = hmod();
    unsafe {
        WinReleaseHook(0, 0, HK_INPUT, post_hook as *const () as usize, m);
        WinReleaseHook(0, 0, HK_SENDMSG, send_hook as *const () as usize, m);
    }
    let p = count_ptr(false);
    if !p.is_null() {
        unsafe { p.write_volatile(p.read_volatile().saturating_sub(1)) };
    }
}

fn now() -> u32 {
    unsafe { WinGetCurrentTime(0) }
}

/// The calling thread as IBM's hTask (PMMERGE's AAB32GetReg(0): the
/// process in the high word, the thread in the low).
fn task() -> u32 {
    let mut tib: *mut os2::Tib = null_mut();
    let mut pib: *mut os2::Pib = null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    let pid = unsafe { (*pib).pid };
    let tid = unsafe { (*(*tib).ptib2).tid };
    pid << 16 | (tid & 0xffff)
}

/// Report `data` (a MON*STRUCT) of kind `mf` to every monitor.
fn broadcast(mf: u32, data: &[u8]) {
    let mut p: *mut u8 = null_mut();
    // PAG_COMMIT | PAG_WRITE | PAG_READ | OBJ_GETTABLE
    if unsafe { os2::DosAllocSharedMem(&mut p, null(), data.len().max(4) as u32, 0x113) } != 0 {
        return;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        // BMSG_SEND | BMSG_FRAMEONLY
        WinBroadcastMsg(HWND_DESKTOP, AM_MONITOR, mf, p as u32, 0x0001 | 0x0008);
        DosFreeMem(p);
    }
}

struct W(Vec<u8>);

impl W {
    fn new() -> W {
        W(Vec::new())
    }
    fn u32(mut self, v: u32) -> W {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u16(mut self, v: u16) -> W {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn bytes(mut self, b: &[u8]) -> W {
        self.0.extend_from_slice(b);
        self
    }
}

/// The first 32 bytes of data, padded (the structures' Data[8]).
fn data32(d: *const u8, cb: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    if !d.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(d, out.as_mut_ptr(), cb.min(32) as usize) };
    }
    out
}

/// MF_HSZ_INFO (IBM's MonBroadcastHszInfo): cb (0x17), fsAction, dwTime,
/// hsz, hTask, a reserved ULONG, the string.
pub fn hsz(h: u32, action: u16) {
    if !active() {
        return;
    }
    let Some((s, _, _)) = string(h) else { return };
    let mut w = W::new().u32(0x17).u16(action).u32(now()).u32(h).u32(task()).u32(0).bytes(&s).0;
    w.push(0);
    w.resize(0x117, 0);
    broadcast(MF_HSZ_INFO, &w);
}

/// MF_ERRORS (MonBroadcastErrorInfo): cb (0x10), wLastError, dwTime, hTask.
pub fn error(e: u32) {
    if e == 0 || !active() {
        return;
    }
    broadcast(MF_ERRORS, &W::new().u32(0x10).u32(e).u32(now()).u32(task()).0);
}

/// The client's and the server's conversation handles and the service
/// and topic of a conversation (`h` is this side's).
fn conv_sides(h: u32, c: &Conv) -> (u32, u32, u32, u32) {
    let (client, server) = if c.status & ST_CLIENT != 0 { (h, c.partner) } else { (c.partner, h) };
    (c.svc, c.topic, client, server)
}

/// MF_CONV (MonBroadcastConvInfo): cb (0x20), fConnect, dwTime, hTask
/// (the partner's), hszSvc, hszTopic, hConvClient, hConvServer.
pub fn conv(h: u32, connect: bool) {
    if !active() {
        return;
    }
    let Some(c) = crate::conv::conv(h) else { return };
    let (svc, topic, client, server) = conv_sides(h, c);
    let (mut pid, mut tid) = (0, 0);
    let partner_task = if unsafe { WinQueryWindowProcess(c.partner, &mut pid, &mut tid) } != 0 { pid << 16 | (tid & 0xffff) } else { 0 };
    broadcast(MF_CONV, &W::new().u32(0x20).u32(connect as u32).u32(now()).u32(partner_task).u32(svc).u32(topic).u32(client).u32(server).0);
}

/// MF_LINKS (MonBroadcastLinkInfo): cb (0x2c), dwTime, hTask, hszSvc,
/// hszTopic, hszItem, hConvClient, hConvServer, fEstablished, fNoData,
/// wFmt, fServer.
pub fn link(h: u32, item: u32, established: bool, nodata: bool, fmt: u16) {
    if !active() {
        return;
    }
    let Some(c) = crate::conv::conv(h) else { return };
    let (svc, topic, client, server) = conv_sides(h, c);
    let srv = c.status & ST_CLIENT == 0;
    broadcast(
        MF_LINKS,
        &W::new().u32(0x2c).u32(now()).u32(task()).u32(svc).u32(topic).u32(item).u32(client).u32(server).u32(established as u32).u32(nodata as u32).u16(fmt).u16(srv as u16).0,
    );
}

/// MF_CALLBACKS (MonBroadcastCBInfo): cb (0x68), dwTime, hTask, dwRet,
/// wType, wFmt, hConv, hsz1, hsz2, hData, dwData1, dwData2, the
/// conversation's CONVCONTEXT, cbData and the first 32 bytes of the data
/// (for a transaction that returns data).
#[allow(clippy::too_many_arguments)]
pub fn callback(ret: u32, typ: u32, fmt: u32, hconv: u32, hsz1: u32, hsz2: u32, hdata: u32, ul1: u32, ul2: u32) {
    if typ == XTYP_MONITOR || !active() {
        return;
    }
    let cc = crate::conv::conv(hconv).map_or([0u32; 6], |c| c.cc);
    let mut w = W::new().u32(0x68).u32(now()).u32(task()).u32(ret).u16(typ as u16).u16(fmt as u16).u32(hconv).u32(hsz1).u32(hsz2).u32(hdata).u32(ul1).u32(ul2);
    for v in cc {
        w = w.u32(v);
    }
    // XCLASS_DATA: the data the callback returned.
    let (mut cb, mut d32) = (0u32, [0u8; 32]);
    if typ & 0x2000 != 0 && ret != 0 && ret != 0xffff_ffff {
        if let Some((p, _)) = data(ret) {
            let dd = parse(p);
            cb = dd.cb;
            d32 = data32(dd.data, dd.cb);
        }
    }
    let w = w.u32(cb).bytes(&d32).0;
    broadcast(MF_CALLBACKS, &w);
}

/// MONMSGSTRUCT as IBM's MonBroadcastPost/SendMsgInfo build it: cb
/// (0x48), hwndTo, dwTime, hTask, wMsg, mp1, mp2, then the message's
/// data: two USHORTs (a DDESTRUCT's status and format), the offsets of
/// two strings after the structure (a DDESTRUCT's item name; a DDEINIT's
/// application and topic), cbData and the first 32 bytes of the data.
fn msg_struct(hwnd: u32, msg: u32, mp1: u32, mp2: u32, lo: u16, hi: u16, s1: &[u8], s2: &[u8], cb: u32, d32: [u8; 32]) -> Vec<u8> {
    let o1 = if s1.is_empty() && s2.is_empty() { 0 } else { 0x48 };
    let o2 = if s2.is_empty() { 0 } else { 0x48 + s1.len() as u16 + 1 };
    let mut w = W::new().u32(0x48).u32(hwnd).u32(now()).u32(task()).u32(msg).u32(mp1).u32(mp2).u16(lo).u16(hi).u16(o1).u16(o2).u32(cb).bytes(&d32).0;
    if o1 != 0 {
        w.extend_from_slice(s1);
        w.push(0);
    }
    if o2 != 0 {
        w.extend_from_slice(s2);
        w.push(0);
    }
    w
}

fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    let mut v = Vec::new();
    let mut i = 0;
    while unsafe { *p.add(i) } != 0 && i < 256 {
        v.push(unsafe { *p.add(i) });
        i += 1;
    }
    v
}

static mut LAST_SEND: [u32; 5] = [0; 5];
static mut LAST_POST: u32 = 0;

/// HK_SENDMSG (IBM's DdeSendMsgHookProc): WM_DDE_INITIATE and
/// WM_DDE_INITIATEACK as they are sent (SMHSTRUCT: mp2, mp1, msg, hwnd,
/// model), once each (a second monitor's hook sees the same message).
extern "C" fn send_hook(_hab: u32, smh: *const u32, _inter: u32) -> u32 {
    if smh.is_null() || !active() {
        return 0;
    }
    let m: [u32; 5] = unsafe { core::array::from_fn(|i| smh.add(i).read_unaligned()) };
    let (mp2, mp1, msg, hwnd) = (m[0], m[1], m[2], m[3]);
    if msg != WM_DDE_INITIATE && msg != WM_DDE_INITIATEACK {
        return 0;
    }
    unsafe {
        if LAST_SEND == m {
            return 0;
        }
        LAST_SEND = m;
    }
    // DDEINIT: cb, pszAppName, pszTopic, offConvContext.
    let p = mp2 as *const u32;
    let (app, topic, cc) = if p.is_null() {
        (Vec::new(), Vec::new(), [0u8; 32])
    } else {
        let (a, t, off) = unsafe { (p.add(1).read_unaligned() as *const u8, p.add(2).read_unaligned() as *const u8, p.add(3).read_unaligned()) };
        (cstr(a), cstr(t), if off != 0 { data32((p as *const u8).wrapping_add(off as usize), 24) } else { [0u8; 32] })
    };
    let cb = if cc != [0u8; 32] { 24 } else { 0 };
    let w = msg_struct(hwnd, msg, mp1, mp2, 0, 0, &app, &topic, cb, cc);
    broadcast(MF_SENDMSGS, &w);
    0
}

/// HK_INPUT (IBM's DdePostMsgHookProc): DDE messages as they are taken
/// off a queue (PM_REMOVE), once each.
extern "C" fn post_hook(_hab: u32, q: *const QMsg, remove: u32) -> u32 {
    if q.is_null() || remove & 1 == 0 || !active() {
        return 0;
    }
    let qm = unsafe { q.read_unaligned() };
    if !(0xa0..0xb0).contains(&qm.msg) {
        return 0;
    }
    unsafe {
        if LAST_POST == qm.time && qm.time != 0 {
            return 0;
        }
        LAST_POST = qm.time;
    }
    let p = qm.mp2 as *const u8;
    let w = if p.is_null() || qm.msg == WM_DDE_INITIATE || qm.msg == WM_DDE_INITIATEACK {
        msg_struct(qm.hwnd, qm.msg, qm.mp1, qm.mp2, 0, 0, b"", b"", 0, [0; 32])
    } else {
        let d = parse(p);
        msg_struct(qm.hwnd, qm.msg, qm.mp1, qm.mp2, d.status, d.fmt, &d.item, b"", d.cb, data32(d.data, d.cb))
    };
    broadcast(MF_POSTMSGS, &w);
    0
}

/// AM_MONITOR at an agent (IBM's AgentWmMonitor): a monitor that asked
/// for this kind gets XTYP_MONITOR with a data handle of the structure
/// and the kind in dwData2.
pub fn agent_monitor(h: u32, mf: u32, p: *mut u8) -> u32 {
    let Some(i) = crate::instance(h) else { return 0 };
    if i.flags & crate::APPCLASS_MONITOR == 0 || i.flags & mf == 0 || p.is_null() {
        return 0;
    }
    let (mut pid, mut tid) = (0, 0);
    let mine = unsafe { WinQueryWindowProcess(h, &mut pid, &mut tid) } != 0 && pid == crate::my_pid();
    // The sender's block: readable here once got (it is the sender's to free).
    let other = unsafe { DosGetSharedMem(p, 0x01) } == 0;
    let cb = unsafe { (p as *const u32).read_unaligned() } as usize;
    let len = match mf {
        MF_HSZ_INFO => 0x117,
        MF_SENDMSGS | MF_POSTMSGS => {
            // The structure and its strings.
            let mut n = 0x48;
            let o2 = unsafe { (p.add(34) as *const u16).read_unaligned() } as usize;
            let o1 = unsafe { (p.add(32) as *const u16).read_unaligned() } as usize;
            for o in [o1, o2] {
                if o != 0 {
                    n = n.max(o + cstr(unsafe { p.add(o) }).len() + 1);
                }
            }
            n
        }
        _ => cb,
    };
    let s = make_struct(0, p, len as u32, len as u32, 0, 0);
    if other && !mine {
        unsafe { DosFreeMem(p) };
    }
    let hd = data_handle(s, 0);
    let r = dispatch(h, XTYP_MONITOR, 0, 0, 0, 0, hd, 0, mf);
    free_data(hd);
    (r == 0) as u32
}
