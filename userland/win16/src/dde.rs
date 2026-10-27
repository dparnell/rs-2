//! DDE between Windows tasks, and the atoms it names things with.
//!
//! Each Windows task is a process of its own (docs/WIN16.md), so the
//! WM_DDE_* conversation between two of them crosses processes:
//!
//! - **Windows of other tasks** are known here by proxy HWNDs (0x1004
//!   and up, 4 apart: below the task's own), each standing for another
//!   process's PM client window. SendMessage and PostMessage to a proxy
//!   go to that window as the PM message `W16X | msg` with the sender's
//!   PM window in mp1 (`FLAG_HWND`, when wParam is a window: the DDE
//!   messages) and lParam in mp2; the receiving WIN16 turns it back into
//!   the Windows message, with a proxy of the sender as wParam.
//! - **SendMessage(0xFFFF, ...)** (WM_DDE_INITIATE) goes to the task's
//!   top-level windows and to every other task's.
//! - **Global atoms** are PM's system atom table (atom.rs), shared by
//!   every PM process, PM programs included.
//! - **GMEM_DDESHARE blocks** are giveable shared memory (kernel.rs),
//!   tiled at the same address in every process, so the handle in
//!   lParam (WM_DDE_DATA's hData, WM_DDE_POKE's, WM_DDE_ADVISE's
//!   hOptions, WM_DDE_EXECUTE's hCommands) is the same selector in the
//!   receiver once the block is given to it (`FLAG_GAVE_LO`/`_HI`); the
//!   receiver adopts it as one of its global blocks.
//! - **PM programs' DDE**: a Windows task's WM_DDE_INITIATE also goes to
//!   PM's DDE (WinDdeInitiate with the atoms' names). A PM server's
//!   WM_DDE_INITIATEACK and its DDESTRUCT messages (WM_DDE_DATA, ACK,
//!   TERMINATE) come back as Windows' messages with GMEM_DDESHARE blocks
//!   and atoms; the Windows client's messages to it become DDESTRUCTs
//!   (WinDdePostMsg). (A PM client of a Windows server is not bridged.)

use alloc::vec::Vec;

use crate::pm::*;
use crate::user::{self, wnd, WS_CHILD};
use crate::Global;

unsafe extern "C" {
    #[link_name = "PMWIN.830"]
    fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.700"]
    fn WinAddAtom(t: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.721"]
    fn WinDeleteAtom(t: u32, atom: u32) -> u32;
    #[link_name = "PMWIN.802"]
    fn WinQueryAtomName(t: u32, atom: u32, buf: *mut u8, cch: u32) -> u32;
    #[link_name = "PMWIN.838"]
    fn WinQueryWindowProcess(h: u32, pid: *mut u32, tid: *mut u32) -> u32;
    #[link_name = "PMWIN.899"]
    fn WinWindowFromID(h: u32, id: u32) -> u32;
    #[link_name = "PMWIN.702"]
    fn WinBeginEnumWindows(h: u32) -> u32;
    #[link_name = "PMWIN.756"]
    fn WinGetNextWindow(henum: u32) -> u32;
    #[link_name = "PMWIN.737"]
    fn WinEndEnumWindows(henum: u32) -> u32;
    #[link_name = "PMWIN.805"]
    fn WinQueryClassName(h: u32, cch: i32, buf: *mut u8) -> i32;
    #[link_name = "DOSCALLS.303"]
    fn DosGiveSharedMem(p: *const u8, pid: u32, flags: u32) -> u32;
    #[link_name = "PMWIN.718"]
    fn WinDdeInitiate(client: u32, app: *const u8, topic: *const u8, ctx: *const u32) -> u32;
    #[link_name = "PMWIN.719"]
    fn WinDdePostMsg(to: u32, from: u32, wm: u32, dde: *mut u8, fl: u32) -> u32;
}

/// A Windows message between tasks: W16X | the message.
pub const W16X: u32 = 0x0002_0000;
/// mp1 is the sender's PM window (wParam an HWND).
const FLAG_HWND: u32 = 0x0004_0000;
/// lParam's low (high) word is a GMEM_DDESHARE block given to the
/// receiver.
const FLAG_GAVE_LO: u32 = 0x0008_0000;
const FLAG_GAVE_HI: u32 = 0x0010_0000;
const FLAGS: u32 = FLAG_HWND | FLAG_GAVE_LO | FLAG_GAVE_HI;

pub const WM_DDE_INITIATE: u16 = 0x03e0;
pub const WM_DDE_TERMINATE: u16 = 0x03e1;
pub const WM_DDE_ADVISE: u16 = 0x03e2;
pub const WM_DDE_UNADVISE: u16 = 0x03e3;
pub const WM_DDE_ACK: u16 = 0x03e4;
pub const WM_DDE_DATA: u16 = 0x03e5;
pub const WM_DDE_REQUEST: u16 = 0x03e6;
pub const WM_DDE_POKE: u16 = 0x03e7;
pub const WM_DDE_EXECUTE: u16 = 0x03e8;

const PROXY_BASE: u16 = 0x1004;

// PM's DDE messages and DDESTRUCT status bits.
const PM_DDE_INITIATE: u32 = 0xa0;
const PM_DDE_REQUEST: u32 = 0xa1;
const PM_DDE_ACK: u32 = 0xa2;
const PM_DDE_DATA: u32 = 0xa3;
const PM_DDE_ADVISE: u32 = 0xa4;
const PM_DDE_UNADVISE: u32 = 0xa5;
const PM_DDE_POKE: u32 = 0xa6;
const PM_DDE_EXECUTE: u32 = 0xa7;
const PM_DDE_TERMINATE: u32 = 0xa8;
const PM_DDE_INITIATEACK: u32 = 0xa9;
const DDE_FACK: u16 = 0x0001;
const DDE_FBUSY: u16 = 0x0002;
const DDE_FNODATA: u16 = 0x0004;
const DDE_FACKREQ: u16 = 0x0008;
const DDE_FRESPONSE: u16 = 0x0010;

/// What a Windows client's message to a PM server left to do when the
/// server acknowledges it: (proxy, the block to free on a positive ACK,
/// WM_DDE_EXECUTE's commands to hand back).
static PM_PENDING: Global<Vec<(u16, u16, u16)>> = Global::new(Vec::new());

/// The proxies: (HWND, the other process's PM window).
static PROXIES: Global<Vec<(u16, u32)>> = Global::new(Vec::new());

fn pid() -> u32 {
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut os2::Pib = core::ptr::null_mut();
    unsafe { os2::DosGetInfoBlocks(&mut tib, &mut pib) };
    if pib.is_null() { 0 } else { unsafe { (*pib).pid } }
}

/// The process of a PM window.
fn window_pid(pm: u32) -> u32 {
    let (mut p, mut t) = (0u32, 0u32);
    unsafe { WinQueryWindowProcess(pm, &mut p, &mut t) };
    p
}

/// The proxy HWND of another process's PM window (made on first use).
pub fn proxy(pm: u32) -> u16 {
    if pm == 0 {
        return 0;
    }
    // One of the task's own windows?
    let own = user::hwnd_of(pm);
    if own != 0 {
        return own;
    }
    let list = PROXIES.get();
    if let Some(&(h, _)) = list.iter().find(|p| p.1 == pm) {
        return h;
    }
    // Reuse the slot of a window that is gone.
    let hab = crate::kernel::TASK.get().hab;
    if let Some(slot) = list.iter_mut().find(|p| unsafe { WinIsWindow(hab, p.1) } == 0) {
        slot.1 = pm;
        return slot.0;
    }
    let h = PROXY_BASE + 4 * list.len() as u16;
    list.push((h, pm));
    h
}

/// The PM window behind a proxy.
pub fn foreign(h: u16) -> Option<u32> {
    PROXIES.get().iter().find(|p| p.0 == h).map(|p| p.1)
}

/// IsWindow of a proxy.
pub fn is_foreign_window(h: u16) -> bool {
    foreign(h).is_some_and(|pm| unsafe { WinIsWindow(crate::kernel::TASK.get().hab, pm) } != 0)
}

/// The PM window standing for HWND `h` in another process (its client;
/// a proxy's own window).
fn pm_of(h: u16) -> u32 {
    wnd(h).map(|w| w.client).or_else(|| foreign(h)).unwrap_or(0)
}

/// Is `m` a message whose wParam is a window (the DDE messages)?
fn has_hwnd(m: u16) -> bool {
    (WM_DDE_INITIATE..=WM_DDE_EXECUTE).contains(&m)
}

/// The PM message and parameters of a Windows message for another
/// process's window `to`; GMEM_DDESHARE blocks in lParam are given to it.
fn outgoing(to: u32, m: u16, wp: u16, lp: u32) -> (u32, u32, u32) {
    let mut msg = W16X | m as u32;
    let mut mp1 = wp as u32;
    if has_hwnd(m) {
        msg |= FLAG_HWND;
        mp1 = pm_of(wp);
    }
    let (lo, hi) = (lp as u16, (lp >> 16) as u16);
    let give = |h: u16| {
        if h == 0 || !crate::kernel::is_shared(h) {
            return false;
        }
        let p = thunk16::flat(((h | 7) as u32) << 16);
        // PAG_READ | PAG_WRITE (already given: fine).
        let rc = unsafe { DosGiveSharedMem(p, window_pid(to), 3) };
        rc == 0 || rc == 5
    };
    match m {
        WM_DDE_DATA | WM_DDE_POKE | WM_DDE_ADVISE if give(lo) => msg |= FLAG_GAVE_LO,
        WM_DDE_EXECUTE if give(hi) => msg |= FLAG_GAVE_HI,
        _ => {}
    }
    (msg, mp1, lp)
}

/// A W16X message from another process as the Windows message.
pub fn incoming(msg: u32, mp1: u32, mp2: u32) -> (u16, u16, u32) {
    let m = msg as u16;
    let wp = if msg & FLAG_HWND != 0 { proxy(mp1) } else { mp1 as u16 };
    for (flag, h) in [(FLAG_GAVE_LO, mp2 as u16), (FLAG_GAVE_HI, (mp2 >> 16) as u16)] {
        if msg & flag != 0 && h != 0 {
            let (mut cb, mut fl) = (0x10000u32, 0u32);
            let p = thunk16::flat(((h | 7) as u32) << 16);
            unsafe extern "C" {
                #[link_name = "DOSCALLS.306"]
                fn DosQueryMem(p: *const u8, cb: *mut u32, fl: *mut u32) -> u32;
            }
            if unsafe { DosQueryMem(p, &mut cb, &mut fl) } == 0 {
                crate::kernel::adopt(h, cb);
            }
        }
    }
    (m, wp, mp2)
}

pub fn is_w16x(msg: u32) -> bool {
    msg & W16X != 0 && msg & !(W16X | FLAGS | 0xffff) == 0
}

/// SendMessage to a proxy: the receiver's result.
pub fn send_foreign(h: u16, m: u16, wp: u16, lp: u32) -> Option<u32> {
    let to = foreign(h)?;
    if !is_w16(to) {
        return Some(to_pm(h, to, m, wp, lp) as u32);
    }
    let (msg, mp1, mp2) = outgoing(to, m, wp, lp);
    Some(unsafe { WinSendMsg(to, msg, mp1, mp2) })
}

/// PostMessage to a proxy.
pub fn post_foreign(h: u16, m: u16, wp: u16, lp: u32) -> Option<bool> {
    let to = foreign(h)?;
    if !is_w16(to) {
        return Some(to_pm(h, to, m, wp, lp));
    }
    let (msg, mp1, mp2) = outgoing(to, m, wp, lp);
    Some(unsafe { WinPostMsg(to, msg, mp1, mp2) } != 0)
}

/// Is a PM window a Windows task's (`Win16Wnd`)?
fn is_w16(h: u32) -> bool {
    let mut b = [0u8; 16];
    let n = unsafe { WinQueryClassName(h, 16, b.as_mut_ptr()) };
    n > 0 && &b[..n as usize] == b"Win16Wnd"
}

// ---- PM's DDE ----

/// A DDESTRUCT (cbData, fsStatus, usFormat, offszItemName, offabData,
/// the item's name and the data) in giveable shared memory.
fn ddestruct(status: u16, fmt: u16, item: &[u8], data: &[u8]) -> *mut u8 {
    let size = 12 + item.len() + 1 + data.len();
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), size as u32, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | os2::OBJ_GIVEABLE) } != 0 {
        return core::ptr::null_mut();
    }
    unsafe {
        (p as *mut u32).write_unaligned(data.len() as u32);
        (p.add(4) as *mut u16).write_unaligned(status);
        (p.add(6) as *mut u16).write_unaligned(fmt);
        (p.add(8) as *mut u16).write_unaligned(12);
        (p.add(10) as *mut u16).write_unaligned((13 + item.len()) as u16);
        core::ptr::copy_nonoverlapping(item.as_ptr(), p.add(12), item.len());
        *p.add(12 + item.len()) = 0;
        core::ptr::copy_nonoverlapping(data.as_ptr(), p.add(13 + item.len()), data.len());
    }
    p
}

/// A DDESTRUCT's status, format, item and data.
fn dde_parts(p: *const u8) -> (u16, u16, Vec<u8>, Vec<u8>) {
    if p.is_null() {
        return (0, 0, Vec::new(), Vec::new());
    }
    unsafe {
        let cb = (p as *const u32).read_unaligned() as usize;
        let (st, fmt) = ((p.add(4) as *const u16).read_unaligned(), (p.add(6) as *const u16).read_unaligned());
        let (oi, od) = ((p.add(8) as *const u16).read_unaligned() as usize, (p.add(10) as *const u16).read_unaligned() as usize);
        (st, fmt, crate::cstr(p.add(oi)).to_vec(), core::slice::from_raw_parts(p.add(od), cb).to_vec())
    }
}

/// A global block's bytes.
fn block_bytes(h: u16) -> Vec<u8> {
    let n = crate::kernel::global_size(h) as usize;
    unsafe { core::slice::from_raw_parts(thunk16::flat(((h | 7) as u32) << 16), n) }.to_vec()
}

/// A Windows message to a PM program's DDE window: its DDESTRUCT.
fn to_pm(h: u16, to: u32, m: u16, wp: u16, lp: u32) -> bool {
    let (lo, hi) = (lp as u16, (lp >> 16) as u16);
    let from = pm_of(wp);
    let name = |a: u16| global_atom_name(a);
    let post = |wm: u32, d: *mut u8| unsafe { WinDdePostMsg(to, from, wm, d, 0) } != 0;
    match m {
        WM_DDE_REQUEST => {
            let ok = post(PM_DDE_REQUEST, ddestruct(0, lo, &name(hi), &[]));
            delete_global_atom(hi);
            ok
        }
        WM_DDE_UNADVISE => {
            let ok = post(PM_DDE_UNADVISE, ddestruct(0, lo, &name(hi), &[]));
            delete_global_atom(hi);
            ok
        }
        WM_DDE_ADVISE | WM_DDE_POKE => {
            // DDEADVISE or DDEPOKE: flags, format (and the data).
            let b = block_bytes(lo);
            if b.len() < 4 {
                return false;
            }
            let (fl, fmt) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
            let (wm, st, data) = if m == WM_DDE_ADVISE {
                (PM_DDE_ADVISE, if fl & 0x4000 != 0 { DDE_FNODATA } else { 0 } | if fl & 0x8000 != 0 { DDE_FACKREQ } else { 0 }, &b[..0])
            } else {
                (PM_DDE_POKE, 0, &b[4..])
            };
            // The block is freed when the server takes it (a poke's only
            // with fRelease).
            let free = m == WM_DDE_ADVISE || fl & 0x2000 != 0;
            PM_PENDING.get().push((h, if free { lo } else { 0 }, 0));
            let ok = post(wm, ddestruct(st, fmt, &name(hi), data));
            delete_global_atom(hi);
            ok
        }
        WM_DDE_EXECUTE => {
            let b = block_bytes(hi);
            let n = b.iter().position(|&c| c == 0).map_or(b.len(), |i| i + 1);
            PM_PENDING.get().push((h, 0, hi));
            post(PM_DDE_EXECUTE, ddestruct(0, 1, b"", &b[..n]))
        }
        WM_DDE_ACK => {
            // The client acknowledges data.
            let st = if lo & 0x8000 != 0 { DDE_FACK } else { 0 } | if lo & 0x4000 != 0 { DDE_FBUSY } else { 0 } | (lo & 0xff) << 8;
            let ok = post(PM_DDE_ACK, ddestruct(st, 0, &name(hi), &[]));
            delete_global_atom(hi);
            ok
        }
        WM_DDE_TERMINATE => post(PM_DDE_TERMINATE, core::ptr::null_mut()),
        _ => false,
    }
}

/// Is `m` one of PM's DDE messages a Windows window may get?
pub fn is_pm_dde(m: u32) -> bool {
    matches!(m, PM_DDE_INITIATEACK | PM_DDE_DATA | PM_DDE_ACK | PM_DDE_TERMINATE)
}

/// One of PM's DDE messages (from a PM server) as Windows' message. A
/// message (its time tells it from another with the same parameters) is
/// translated once, as PeekMessage may look at it before GetMessage
/// takes it: the DDESTRUCT is freed then.
pub fn from_pm(m: u32, mp1: u32, mp2: u32, time: u32) -> (u16, u16, u32) {
    static LAST: Global<Option<((u32, u32, u32, u32), (u16, u16, u32))>> = Global::new(None);
    let key = (m, mp1, mp2, time);
    if let Some((k, r)) = *LAST.get() {
        if k == key && mp2 != 0 && time != 0 {
            return r;
        }
    }
    let r = translate_pm_dde(m, mp1, mp2);
    *LAST.get() = Some((key, r));
    r
}

fn translate_pm_dde(m: u32, mp1: u32, mp2: u32) -> (u16, u16, u32) {
    let server = proxy(mp1);
    let p = mp2 as *mut u8;
    let r = match m {
        PM_DDE_INITIATEACK => {
            // DDEINIT: cb, pszAppName, pszTopic.
            let (app, topic) = if p.is_null() {
                (Vec::new(), Vec::new())
            } else {
                unsafe { (crate::cstr((p as *const u32).add(1).read_unaligned() as *const u8).to_vec(), crate::cstr((p as *const u32).add(2).read_unaligned() as *const u8).to_vec()) }
            };
            (WM_DDE_ACK, server, (add_global_atom(&topic) as u32) << 16 | add_global_atom(&app) as u32)
        }
        PM_DDE_DATA => {
            let (st, fmt, item, data) = dde_parts(p);
            // DDEDATA: fResponse, fRelease (ours: the client frees it),
            // fAckReq; the format, the data.
            let fl = 0x2000 | if st & DDE_FRESPONSE != 0 { 0x1000 } else { 0 } | if st & DDE_FACKREQ != 0 { 0x8000 } else { 0 };
            let g = crate::kernel::global_alloc(0x0002 | crate::kernel::GMEM_DDESHARE, data.len() as u32 + 4);
            if g != 0 {
                let q = thunk16::flat(((g | 7) as u32) << 16);
                unsafe {
                    (q as *mut u16).write_unaligned(fl);
                    (q.add(2) as *mut u16).write_unaligned(fmt);
                    core::ptr::copy_nonoverlapping(data.as_ptr(), q.add(4), data.len());
                }
            }
            (WM_DDE_DATA, server, (add_global_atom(&item) as u32) << 16 | g as u32)
        }
        PM_DDE_ACK => {
            let (st, _, item, _) = dde_parts(p);
            let w = if st & DDE_FACK != 0 { 0x8000 } else { 0 } | if st & DDE_FBUSY != 0 { 0x4000 } else { 0 } | st >> 8;
            // What the acknowledged message left: its block, an execute's
            // commands (the ACK's high word).
            let mut hi = 0;
            let pend = PM_PENDING.get();
            if let Some(i) = pend.iter().position(|x| x.0 == server) {
                let (_, free, exec) = pend.remove(i);
                if free != 0 && st & DDE_FACK != 0 {
                    crate::kernel::global_free(free);
                }
                hi = exec;
            }
            if hi == 0 {
                hi = add_global_atom(&item);
            }
            (WM_DDE_ACK, server, (hi as u32) << 16 | w as u32)
        }
        _ => (WM_DDE_TERMINATE, server, 0),
    };
    if !p.is_null() {
        unsafe { os2::DosFreeMem(p) };
    }
    r
}

/// A Windows client's WM_DDE_INITIATE to PM's DDE servers.
fn pm_initiate(client: u16, app: u16, topic: u16) {
    let c = pm_of(client);
    if c == 0 {
        return;
    }
    let (a, t) = (user::z(&global_atom_name(app)), user::z(&global_atom_name(topic)));
    unsafe { WinDdeInitiate(c, a.as_ptr(), t.as_ptr(), core::ptr::null()) };
}

/// The client windows of the other Windows tasks' top-level windows.
fn other_tasks_windows() -> Vec<u32> {
    let me = pid();
    let mut out = Vec::new();
    let e = unsafe { WinBeginEnumWindows(HWND_DESKTOP) };
    loop {
        let h = unsafe { WinGetNextWindow(e) };
        if h == 0 {
            break;
        }
        if window_pid(h) == me {
            continue;
        }
        let c = if is_w16(h) { h } else { unsafe { WinWindowFromID(h, FID_CLIENT) } };
        if c != 0 && is_w16(c) {
            out.push(c);
        }
    }
    unsafe { WinEndEnumWindows(e) };
    out
}

/// SendMessage(0xFFFF, ...): every top-level window, the task's own and
/// the other tasks'.
pub fn broadcast(m: u16, wp: u16, lp: u32) -> u32 {
    let own: Vec<u16> = user::all().into_iter().filter(|&h| wnd(h).is_some_and(|w| w.style & WS_CHILD == 0)).collect();
    for h in own {
        if wnd(h).is_some() {
            user::send(h, m, wp, lp);
        }
    }
    for pm in other_tasks_windows() {
        let (msg, mp1, mp2) = outgoing(pm, m, wp, lp);
        unsafe { WinSendMsg(pm, msg, mp1, mp2) };
    }
    if m == WM_DDE_INITIATE {
        pm_initiate(wp, lp as u16, (lp >> 16) as u16);
    }
    0
}

// ---- Atoms ----

/// A global atom's name (for DDEML).
pub fn global_atom_name(atom: u16) -> Vec<u8> {
    let mut b = [0u8; 256];
    let n = unsafe { WinQueryAtomName(WinQuerySystemAtomTable(), atom as u32, b.as_mut_ptr(), 256) } as usize;
    b[..n.min(255)].to_vec()
}

/// Add (or count once more) a global atom.
pub fn add_global_atom(name: &[u8]) -> u16 {
    let z = user::z(name);
    unsafe { WinAddAtom(WinQuerySystemAtomTable(), z.as_ptr()) as u16 }
}

pub fn delete_global_atom(atom: u16) {
    unsafe { WinDeleteAtom(WinQuerySystemAtomTable(), atom as u32) };
}
