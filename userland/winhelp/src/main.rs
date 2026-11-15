//! WINHELP.EXE: Windows Help for the Windows programs WIN16 runs.
//!
//! One runs at a time. It makes a hidden window of class `MS_WINHELP`
//! (as Windows' WinHelp does), a 4 KB named shared memory block for
//! requests and a named mutex for their writers; WIN16's WinHelp
//! (`userland/win16/src/winhelp.rs`) writes a [`Request`] into the block
//! and sends the window `WM_RS2_WINHELP`. The help file shows in the help
//! manager's window, which reads Windows `.HLP` files by converting them
//! to IPF (`crates/winhlp`); HELPMGR's `Rs2WinHelp` carries the command
//! out. As Windows' WinHelp, it shows one file at a time: a request for
//! another file replaces it. It ends when its help window is closed, or
//! when every program that asked for help has said HELP_QUIT.
//!
//! `WINHELP file.HLP` from a command line shows the file's contents.
//!
//! Request (little-endian): `u32 'RWH1'`, `u16` command, `u16` 0, `u32`
//! number (HELP_CONTEXT's id...), `u32` the caller's PM frame, `u16`
//! length of the file name, `u16` length of the string, the file name
//! (a full path), 0, the string (HELP_KEY's keyword, HELP_COMMAND's
//! macro, HELP_SETWINPOS's five words), 0.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

os2::entry!(main);

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct QMsg {
    hwnd: u32,
    msg: u32,
    mp1: u32,
    mp2: u32,
    time: u32,
    x: i32,
    y: i32,
    reserved: u32,
}

type WndProc = extern "system" fn(u32, u32, u32, u32) -> u32;

unsafe extern "C" {
    #[link_name = "PMWIN.763"]
    fn WinInitialize(fl: u32) -> u32;
    #[link_name = "PMWIN.716"]
    fn WinCreateMsgQueue(hab: u32, n: i32) -> u32;
    #[link_name = "PMWIN.915"]
    fn WinGetMsg(hab: u32, q: *mut QMsg, h: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.912"]
    fn WinDispatchMsg(hab: u32, q: *const QMsg) -> u32;
    #[link_name = "PMWIN.726"]
    fn WinDestroyMsgQueue(hmq: u32) -> u32;
    #[link_name = "PMWIN.888"]
    fn WinTerminate(hab: u32) -> u32;
    #[link_name = "PMWIN.926"]
    fn WinRegisterClass(hab: u32, name: *const u8, proc_: WndProc, style: u32, words: u32) -> u32;
    #[link_name = "PMWIN.909"]
    fn WinCreateWindow(parent: u32, class: *const u8, text: *const u8, style: u32, x: i32, y: i32, cx: i32, cy: i32, owner: u32, behind: u32, id: u32, ctl: *const u8, pres: *const u8) -> u32;
    #[link_name = "PMWIN.728"]
    fn WinDestroyWindow(h: u32) -> u32;
    #[link_name = "PMWIN.911"]
    fn WinDefWindowProc(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.902"]
    fn WinPostQueueMsg(hmq: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.919"]
    fn WinPostMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "PMWIN.789"]
    fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, id: u32, style: u32) -> u32;
    #[link_name = "HELPMGR.52"]
    fn WinDestroyHelpInstance(h: u32) -> u32;
    #[link_name = "HELPMGR.201"]
    fn Rs2WinHelp(hab: u32, inst: u32, file: *const u8, cmd: u32, num: u32, text: *const u8, ntext: u32, owner: u32, rc: *mut u32) -> u32;
}

const WM_QUIT: u32 = 0x002a;
const WM_RS2_WINHELP: u32 = 0x1157;
/// A copied request, to be carried out (mp1: a `Box<Vec<u8>>`).
const WM_RS2_DO: u32 = 0x1158;
const MAGIC: u32 = u32::from_le_bytes(*b"RWH1");
const HELP_QUIT: u32 = 0x0002;
const HELP_CONTENTS: u32 = 0x0003;

const SHMEM: &[u8] = b"\\SHAREMEM\\RS2\\WINHELP\0";
const MUTEX: &[u8] = b"\\SEM32\\RS2\\WINHELP\0";

struct State {
    hab: u32,
    hmq: u32,
    block: *mut u8,
    /// The file shown and its help instance.
    file: Vec<u8>,
    inst: u32,
    /// The programs' frames that asked for help (HELP_QUIT removes one).
    users: Vec<u32>,
}

static mut STATE: *mut State = core::ptr::null_mut();

fn st() -> &'static mut State {
    unsafe { &mut *STATE }
}

fn main(s: &os2::Startup) -> u32 {
    let hab = unsafe { WinInitialize(0) };
    let hmq = unsafe { WinCreateMsgQueue(hab, 0) };
    let mut block: *mut u8 = core::ptr::null_mut();
    // PAG_COMMIT | PAG_READ | PAG_WRITE. Another WINHELP.EXE has it:
    // that one serves.
    if unsafe { os2::DosAllocSharedMem(&mut block, SHMEM.as_ptr(), 4096, 0x13) } != 0 {
        let args = s.args();
        if !args.is_empty() {
            message(b"Windows Help is already running.");
        }
        return 1;
    }
    let mut mtx = 0u32;
    unsafe { os2::DosCreateMutexSem(MUTEX.as_ptr(), &mut mtx, 0, 0) };
    unsafe { STATE = Box::into_raw(Box::new(State { hab, hmq, block, file: Vec::new(), inst: 0, users: Vec::new() })) };
    unsafe { WinRegisterClass(hab, b"MS_WINHELP\0".as_ptr(), proc_, 0, 0) };
    // A hidden top-level window: WinHelp finds it among the desktop's.
    let w = unsafe { WinCreateWindow(1, b"MS_WINHELP\0".as_ptr(), b"Windows Help\0".as_ptr(), 0, 0, 0, 0, 0, 0, 3, 0, core::ptr::null(), core::ptr::null()) };
    // WINHELP file: the file's contents.
    let args = s.args();
    let file: Vec<u8> = args.split(|&c| c == b' ').find(|a| !a.is_empty() && a[0] != b'/' && a[0] != b'-').unwrap_or(b"").to_vec();
    if !file.is_empty() {
        let mut full = file.clone();
        full.push(0);
        let mut buf = [0u8; 260];
        if unsafe { os2::DosQueryPathInfo(full.as_ptr(), 5, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
            let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
            full = buf[..=n].to_vec();
        }
        let mut req = Vec::new();
        req.extend_from_slice(&MAGIC.to_le_bytes());
        req.extend_from_slice(&(HELP_CONTENTS as u16).to_le_bytes());
        req.extend_from_slice(&[0; 2]);
        req.extend_from_slice(&0u32.to_le_bytes());
        req.extend_from_slice(&0u32.to_le_bytes());
        req.extend_from_slice(&((full.len() - 1) as u16).to_le_bytes());
        req.extend_from_slice(&0u16.to_le_bytes());
        req.extend_from_slice(&full);
        req.push(0);
        if !carry_out(&req) {
            message(b"Cannot open the help file.");
            return 1;
        }
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
    }
    let s = st();
    if s.inst != 0 {
        unsafe { WinDestroyHelpInstance(s.inst) };
    }
    unsafe {
        WinDestroyWindow(w);
        os2::DosCloseMutexSem(mtx);
        os2::DosFreeMem(block);
        WinDestroyMsgQueue(hmq);
        WinTerminate(hab);
    }
    0
}

fn message(text: &[u8]) {
    let mut t = text.to_vec();
    t.push(0);
    // MB_OK | MB_ERROR | MB_MOVEABLE
    unsafe { WinMessageBox(1, 1, t.as_ptr(), b"Windows Help\0".as_ptr(), 0, 0x4040) };
}

extern "system" fn proc_(h: u32, m: u32, a: u32, b: u32) -> u32 {
    match m {
        WM_RS2_WINHELP => {
            // Copy the request out of the block before answering: the
            // caller writes the next one once the send returns.
            let s = st();
            let len = request_len(s.block);
            if len == 0 {
                return 0;
            }
            let req = unsafe { core::slice::from_raw_parts(s.block, len) }.to_vec();
            let p = Box::into_raw(Box::new(req));
            unsafe { WinPostMsg(h, WM_RS2_DO, p as u32, 0) };
            1
        }
        WM_RS2_DO => {
            let req = unsafe { Box::from_raw(a as *mut Vec<u8>) };
            carry_out(&req);
            0
        }
        _ => unsafe { WinDefWindowProc(h, m, a, b) },
    }
}

fn rd16(p: *const u8, o: usize) -> u16 {
    unsafe { (p.add(o) as *const u16).read_unaligned() }
}

fn rd32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}

/// The length of the request in `p`, 0 when it is not one.
fn request_len(p: *const u8) -> usize {
    if rd32(p, 0) != MAGIC {
        return 0;
    }
    let n = 20 + rd16(p, 16) as usize + 1 + rd16(p, 18) as usize + 1;
    if n > 4096 { 0 } else { n }
}

/// Carry out request `r`; false when the file could not be shown.
fn carry_out(r: &[u8]) -> bool {
    let p = r.as_ptr();
    let cmd = rd16(p, 4) as u32;
    let num = rd32(p, 8);
    let owner = rd32(p, 12);
    let nf = rd16(p, 16) as usize;
    let nt = rd16(p, 18) as usize;
    let file = &r[20..20 + nf];
    let text = &r[20 + nf + 1..20 + nf + 1 + nt];
    let s = st();
    if cmd == HELP_QUIT {
        s.users.retain(|&u| u != owner);
        if s.users.is_empty() {
            unsafe { WinPostQueueMsg(s.hmq, WM_QUIT, 0, 0) };
        }
        return true;
    }
    if owner != 0 && !s.users.contains(&owner) {
        s.users.push(owner);
    }
    // Another file replaces the one shown.
    if !file.is_empty() && !file.eq_ignore_ascii_case(&s.file) && s.inst != 0 {
        unsafe { WinDestroyHelpInstance(s.inst) };
        s.inst = 0;
    }
    if !file.is_empty() {
        s.file = file.to_vec();
    }
    let mut f = s.file.clone();
    f.push(0);
    let mut t = text.to_vec();
    t.push(0);
    let mut rc = 0u32;
    let inst = unsafe { Rs2WinHelp(s.hab, s.inst, f.as_ptr(), cmd, num, t.as_ptr(), nt as u32, owner, &mut rc) };
    if inst == 0 {
        return false;
    }
    s.inst = inst;
    true
}
