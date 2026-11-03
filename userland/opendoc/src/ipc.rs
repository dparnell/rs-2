//! Apple events between processes (rs2's own transport; OS/2 OpenDoc
//! addresses a process by its PID, found by program name).
//!
//! A process that installs an Apple-event handler on a thread with a PM
//! message queue becomes a receiver:
//!
//! - It enters its PID and program name in a shared table
//!   (`\SHAREMEM\RS2AE\TABLE`, guarded by the mutex `\SEM32\RS2AE\TABLE`),
//!   which AEGetProcessID/AEGetProcessName (42, 71) read.
//! - It makes a 64 KB shared block `\SHAREMEM\RS2AE\P<pid>`, the event
//!   semaphores `\SEM32\RS2AE\REQ<pid>` and `...\DONE<pid>` and the
//!   mutex `\SEM32\RS2AE\MTX<pid>`.
//! - A thread of its own waits on REQ and posts a message to an object
//!   window of the handler's thread. There, in the program's own event
//!   loop, the event is given to its handler, and the reply goes back
//!   through the block before DONE is posted.
//!
//! A sender (any process: a REXX program run by CMD needs no PM) takes
//! the receiver's mutex, writes the event (`[length, send mode, event]`,
//! the descriptor encoding of ae.rs), posts REQ and waits for DONE
//! (30 seconds), then reads `[length, result code, reply]`.

use alloc::vec::Vec;

use os2::*;

use crate::ae;

unsafe extern "C" {
    #[link_name = "DOSCALLS.325"]
    fn DosOpenEventSem(name: *const u8, phev: *mut u32) -> u32;
    #[link_name = "DOSCALLS.332"]
    fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> u32;
    #[link_name = "DOSCALLS.335"]
    fn DosReleaseMutexSem(hmtx: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, class: *const u8, proc_: usize, style: u32, cb: u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: usize, name: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32,
                       owner: u32, behind: u32, id: u32, ctl: usize, pres: usize) -> u32;
    #[link_name = "PMWIN.919"]
    fn WinPostMsg(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize;
}

const BLOCK: u32 = 0x10000;
const WM_AE: u32 = 0x10ae;
const TIMEOUT: u32 = 30000;
const TABLE_SIZE: usize = 4096;
const ENTRY: usize = 32;

fn z(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// The shared table: count (4 bytes), then entries of PID (4) and
/// program name (28, NUL-terminated).
fn table() -> Option<(*mut u8, u32)> {
    let name = z("\\SHAREMEM\\RS2AE\\TABLE");
    let mut p: *mut u8 = core::ptr::null_mut();
    unsafe {
        if DosGetNamedSharedMem(&mut p, name.as_ptr(), 3) != 0 && DosAllocSharedMem(&mut p, name.as_ptr(), TABLE_SIZE as u32, 0x13) != 0 {
            return None;
        }
        let mname = z("\\SEM32\\RS2AE\\TABLE");
        let mut m = 0u32;
        if DosOpenMutexSem(mname.as_ptr(), &mut m) != 0 && DosCreateMutexSem(mname.as_ptr(), &mut m, 0, 0) != 0 {
            return None;
        }
        DosRequestMutexSem(m, TIMEOUT);
        Some((p, m))
    }
}

fn entries(p: *mut u8) -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(p, TABLE_SIZE) }
}

fn count(t: &[u8]) -> usize {
    (u32::from_le_bytes([t[0], t[1], t[2], t[3]]) as usize).min((TABLE_SIZE - 4) / ENTRY)
}

fn entry(t: &[u8], i: usize) -> (u32, Vec<u8>) {
    let e = &t[4 + i * ENTRY..4 + (i + 1) * ENTRY];
    let pid = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
    let name = e[4..].iter().copied().take_while(|&c| c != 0).collect();
    (pid, name)
}

fn stem(n: &[u8]) -> Vec<u8> {
    let u: Vec<u8> = n.iter().map(|c| c.to_ascii_uppercase()).collect();
    u.strip_suffix(b".EXE").map_or(u.clone(), |s| s.to_vec())
}

/// A receiving process by program name (with or without .EXE): the
/// latest entered.
pub fn find_name(want: &[u8]) -> Option<u32> {
    let (p, m) = table()?;
    let t = entries(p);
    let w = stem(want);
    let r = (0..count(t)).rev().map(|i| entry(t, i)).find(|(pid, n)| *pid != ae::own_pid() && stem(n) == w).map(|x| x.0);
    unsafe { DosReleaseMutexSem(m) };
    r
}

pub fn name_of(pid: u32) -> Option<Vec<u8>> {
    let (p, m) = table()?;
    let t = entries(p);
    let r = (0..count(t)).map(|i| entry(t, i)).find(|x| x.0 == pid).map(|x| x.1);
    unsafe { DosReleaseMutexSem(m) };
    r
}

fn remove(pid: u32) {
    let Some((p, m)) = table() else { return };
    let t = entries(p);
    let n = count(t);
    if let Some(i) = (0..n).find(|&i| entry(t, i).0 == pid) {
        t.copy_within(4 + (i + 1) * ENTRY..4 + n * ENTRY, 4 + i * ENTRY);
        t[..4].copy_from_slice(&((n - 1) as u32).to_le_bytes());
    }
    unsafe { DosReleaseMutexSem(m) };
}

fn names(pid: u32) -> [Vec<u8>; 4] {
    [
        z(&alloc::format!("\\SHAREMEM\\RS2AE\\P{pid}")),
        z(&alloc::format!("\\SEM32\\RS2AE\\REQ{pid}")),
        z(&alloc::format!("\\SEM32\\RS2AE\\DONE{pid}")),
        z(&alloc::format!("\\SEM32\\RS2AE\\MTX{pid}")),
    ]
}

struct Receiver {
    block: *mut u8,
    req: u32,
    done: u32,
    hwnd: u32,
}

static mut RECEIVER: Option<Receiver> = None;

/// Become a receiver (once; on a thread with a message queue).
pub fn listen(name: &[u8]) {
    unsafe {
        if (*(&raw const RECEIVER)).is_some() {
            return;
        }
        let class = b"rs2AEReceiver\0";
        WinRegisterClass(0, class.as_ptr(), receiver_proc_t as usize, 0, 0);
        let hwnd = WinCreateWindow(2, class.as_ptr() as usize, b"\0".as_ptr(), 0, 0, 0, 0, 0, 0, 3, 0, 0, 0);
        if hwnd == 0 {
            return;
        }
        let pid = ae::own_pid();
        let [bn, rn, dn, mn] = names(pid);
        let mut block: *mut u8 = core::ptr::null_mut();
        let (mut req, mut done, mut mtx) = (0u32, 0u32, 0u32);
        if DosAllocSharedMem(&mut block, bn.as_ptr(), BLOCK, 0x13) != 0
            || DosCreateEventSem(rn.as_ptr(), &mut req, 0, 0) != 0
            || DosCreateEventSem(dn.as_ptr(), &mut done, 0, 0) != 0
            || DosCreateMutexSem(mn.as_ptr(), &mut mtx, 0, 0) != 0
        {
            return;
        }
        RECEIVER = Some(Receiver { block, req, done, hwnd });
        let mut tid = 0u32;
        DosCreateThread(&mut tid, waiter, 0, 0, 0x4000);
        // the table
        if let Some((p, m)) = table() {
            let t = entries(p);
            let n = count(t);
            if n < (TABLE_SIZE - 4) / ENTRY {
                let e = &mut t[4 + n * ENTRY..4 + (n + 1) * ENTRY];
                e[..4].copy_from_slice(&pid.to_le_bytes());
                let k = name.len().min(ENTRY - 5);
                e[4..4 + k].copy_from_slice(&name[..k]);
                e[4 + k] = 0;
                t[..4].copy_from_slice(&((n + 1) as u32).to_le_bytes());
            }
            DosReleaseMutexSem(m);
        }
    }
}

/// The receiver's thread: a request becomes a message to the handler's
/// thread.
unsafe extern "C" fn waiter(_: u32) {
    loop {
        let Some(r) = (unsafe { &*(&raw const RECEIVER) }) else { return };
        unsafe {
            DosWaitEventSem(r.req, 0xffff_ffff);
            let mut n = 0u32;
            DosResetEventSem(r.req, &mut n);
            WinPostMsg(r.hwnd, WM_AE, 0, 0);
        }
    }
}

fn rd(p: *const u8, off: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(p.add(off) as *const u32) }
}

fn wr(p: *mut u8, off: usize, v: u32) {
    unsafe { core::ptr::write_unaligned(p.add(off) as *mut u32, v) };
}

extern "C" fn receiver_proc(hwnd: u32, msg: u32, mp1: usize, mp2: usize) -> usize {
    if msg != WM_AE {
        return unsafe { WinDefWindowProc(hwnd, msg, mp1, mp2) };
    }
    let Some(r) = (unsafe { &*(&raw const RECEIVER) }) else { return 0 };
    let b = r.block;
    let len = (rd(b, 0) as usize).min(BLOCK as usize - 8);
    let mode = rd(b, 4);
    let req = unsafe { core::slice::from_raw_parts(b.add(8), len) }.to_vec();
    let mut p = 0;
    let (rc, out) = match ae::decode(&req, &mut p) {
        Some(mut e) => {
            let mut reply = ae::Desc::default();
            let rc = ae::deliver(&e as *const ae::Desc as usize, &mut reply as *mut ae::Desc as usize, mode);
            let mut out = Vec::new();
            ae::encode(&reply, &mut out);
            ae::AEDisposeDesc(&mut reply as *mut ae::Desc as usize);
            ae::AEDisposeDesc(&mut e as *mut ae::Desc as usize);
            (rc, out)
        }
        None => (ae::ERR_NOT_EVENT, Vec::new()),
    };
    let n = out.len().min(BLOCK as usize - 8);
    unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), b.add(8), n) };
    wr(b, 0, n as u32);
    wr(b, 4, rc as u32);
    unsafe { DosPostEventSem(r.done) };
    0
}

crate::tramp!(receiver_proc_t => receiver_proc, 4);

/// Send an encoded event to another process; its result code and the
/// encoded reply.
pub fn send(pid: u32, mode: u32, event: &[u8]) -> Result<(i32, Vec<u8>), i32> {
    if pid == 0 || event.len() > BLOCK as usize - 8 {
        return Err(ae::ERR_PROC_NOT_FOUND);
    }
    let [bn, rn, dn, mn] = names(pid);
    let mut b: *mut u8 = core::ptr::null_mut();
    let (mut req, mut done, mut mtx) = (0u32, 0u32, 0u32);
    unsafe {
        if DosGetNamedSharedMem(&mut b, bn.as_ptr(), 3) != 0
            || DosOpenEventSem(rn.as_ptr(), &mut req) != 0
            || DosOpenEventSem(dn.as_ptr(), &mut done) != 0
            || DosOpenMutexSem(mn.as_ptr(), &mut mtx) != 0
        {
            remove(pid);
            return Err(ae::ERR_PROC_NOT_FOUND);
        }
        if DosRequestMutexSem(mtx, TIMEOUT) != 0 {
            return Err(-1712); // errAETimeout
        }
        wr(b, 0, event.len() as u32);
        wr(b, 4, mode);
        core::ptr::copy_nonoverlapping(event.as_ptr(), b.add(8), event.len());
        let mut n = 0u32;
        DosResetEventSem(done, &mut n);
        DosPostEventSem(req);
        let w = DosWaitEventSem(done, TIMEOUT);
        let r = if w != 0 {
            Err(-1712)
        } else {
            let len = (rd(b, 0) as usize).min(BLOCK as usize - 8);
            Ok((rd(b, 4) as i32, core::slice::from_raw_parts(b.add(8), len).to_vec()))
        };
        DosReleaseMutexSem(mtx);
        DosCloseEventSem(req);
        DosCloseEventSem(done);
        DosCloseMutexSem(mtx);
        r
    }
}
