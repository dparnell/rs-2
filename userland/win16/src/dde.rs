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
//! - **Global atoms** are PM's system atom table (shared by every PM
//!   process, PM programs included); local atoms a PM atom table of the
//!   task's own.
//! - **GMEM_DDESHARE blocks** are giveable shared memory (kernel.rs),
//!   tiled at the same address in every process, so the handle in
//!   lParam (WM_DDE_DATA's hData, WM_DDE_POKE's, WM_DDE_ADVISE's
//!   hOptions, WM_DDE_EXECUTE's hCommands) is the same selector in the
//!   receiver once the block is given to it (`FLAG_GAVE_LO`/`_HI`); the
//!   receiver adopts it as one of its global blocks.

use alloc::vec::Vec;

use crate::pm::*;
use crate::user::{self, wnd, WS_CHILD};
use crate::{far_str, Global};

unsafe extern "C" {
    #[link_name = "PMWIN.830"]
    fn WinQuerySystemAtomTable() -> u32;
    #[link_name = "PMWIN.714"]
    fn WinCreateAtomTable(initial: u32, buckets: u32) -> u32;
    #[link_name = "PMWIN.700"]
    fn WinAddAtom(t: u32, name: *const u8) -> u32;
    #[link_name = "PMWIN.744"]
    fn WinFindAtom(t: u32, name: *const u8) -> u32;
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
    let (msg, mp1, mp2) = outgoing(to, m, wp, lp);
    Some(unsafe { WinSendMsg(to, msg, mp1, mp2) })
}

/// PostMessage to a proxy.
pub fn post_foreign(h: u16, m: u16, wp: u16, lp: u32) -> Option<bool> {
    let to = foreign(h)?;
    let (msg, mp1, mp2) = outgoing(to, m, wp, lp);
    Some(unsafe { WinPostMsg(to, msg, mp1, mp2) } != 0)
}

/// The client windows of the other Windows tasks' top-level windows.
fn other_tasks_windows() -> Vec<u32> {
    let me = pid();
    let mut out = Vec::new();
    let is_w16 = |h: u32| {
        let mut b = [0u8; 16];
        let n = unsafe { WinQueryClassName(h, 16, b.as_mut_ptr()) };
        n > 0 && &b[..n as usize] == b"Win16Wnd"
    };
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
    0
}

// ---- Atoms ----

/// An atom name argument for PM: a string, or MAKEINTATOM.
fn atom_name(far: u32) -> Vec<u8> {
    match far_str(far) {
        Some(s) => user::z(s),
        // "#n"
        None => user::z(alloc::format!("#{}", far as u16).as_bytes()),
    }
}

fn local_table() -> u32 {
    static T: Global<u32> = Global::new(0);
    let t = T.get();
    if *t == 0 {
        *t = unsafe { WinCreateAtomTable(0, 0) };
    }
    *t
}

/// The atom calls: KERNEL's local ones (68-73) and USER's global ones
/// (268-271, as 1268-1271), None for others.
pub fn atoms(ord: u32, a: &mut thunk16::Pascal) -> Option<u32> {
    let table = || if ord >= 1000 { unsafe { WinQuerySystemAtomTable() } } else { local_table() };
    Some(match ord {
        // InitAtomTable(size)
        68 => {
            a.w();
            (local_table() != 0) as u32
        }
        // FindAtom(name), GlobalFindAtom
        69 | 1270 => {
            let n = atom_name(a.l());
            unsafe { WinFindAtom(table(), n.as_ptr()) & 0xffff }
        }
        // AddAtom(name), GlobalAddAtom
        70 | 1268 => {
            let n = atom_name(a.l());
            unsafe { WinAddAtom(table(), n.as_ptr()) & 0xffff }
        }
        // DeleteAtom(atom), GlobalDeleteAtom: 0 when deleted.
        71 | 1269 => {
            let atom = a.w();
            unsafe { WinDeleteAtom(table(), atom as u32) & 0xffff }
        }
        // GetAtomName(atom, buf, n), GlobalGetAtomName
        72 | 1271 => {
            let (atom, buf, n) = (a.w(), a.p(), a.i());
            if buf.is_null() || n <= 0 {
                return Some(0);
            }
            if atom < 0xc000 {
                let s = alloc::format!("#{atom}");
                return Some(user::copy_out(s.as_bytes(), buf, n) as u32);
            }
            unsafe { WinQueryAtomName(table(), atom as u32, buf, n as u32) }
        }
        // GetAtomHandle(atom)
        73 => a.w() as u32,
        _ => return None,
    })
}

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
