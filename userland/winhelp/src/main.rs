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
const HELP_COMMAND: u32 = 0x0102;
const HELP_PARTIALKEY: u32 = 0x0105;
/// Borland's MULTIHLP: find a keyword in the help file's .HDX index.
const BORLAND_SEARCH: u32 = 0x1004;

const SHMEM: &[u8] = b"\\SHAREMEM\\RS2\\WINHELP\0";
const MUTEX: &[u8] = b"\\SEM32\\RS2\\WINHELP\0";

struct State {
    /// The main thread's queue, and the help thread's (its anchor block,
    /// queue and object window), which opens the files and owns the help
    /// window: converting a large file takes a while, and the programs
    /// asking for help must not wait for it.
    hmq: u32,
    hab: u32,
    work_hmq: u32,
    work: u32,
    ready: u32,
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
    unsafe { STATE = Box::into_raw(Box::new(State { hmq, hab: 0, work_hmq: 0, work: 0, ready: 0, block, file: Vec::new(), inst: 0, users: Vec::new() })) };
    unsafe { WinRegisterClass(hab, b"MS_WINHELP\0".as_ptr(), proc_, 0, 0) };
    // A hidden top-level window: WinHelp finds it among the desktop's.
    let w = unsafe { WinCreateWindow(1, b"MS_WINHELP\0".as_ptr(), b"Windows Help\0".as_ptr(), 0, 0, 0, 0, 0, 0, 3, 0, core::ptr::null(), core::ptr::null()) };
    // The help thread.
    unsafe { os2::DosCreateEventSem(core::ptr::null(), &mut st().ready, 0, 0) };
    let mut tid = 0u32;
    unsafe { os2::DosCreateThread(&mut tid, help_thread, 0, 0, 256 * 1024) };
    unsafe { os2::DosWaitEventSem(st().ready, 10_000) };
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
        let s = st();
        unsafe { WinPostMsg(s.work, WM_RS2_DO, Box::into_raw(Box::new(req)) as u32, 1) };
    }
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(hab, &q) };
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
            unsafe { WinPostMsg(s.work, WM_RS2_DO, p as u32, 0) };
            1
        }
        _ => unsafe { WinDefWindowProc(h, m, a, b) },
    }
}

/// The help thread: carries out the requests the main thread copied
/// (WM_RS2_DO; mp2 1: from the command line); ends the program when its
/// help window is closed (the help manager posts WM_QUIT for a book's) or
/// the last program said HELP_QUIT.
unsafe extern "C" fn help_thread(_: u32) {
    let s = st();
    s.hab = unsafe { WinInitialize(0) };
    s.work_hmq = unsafe { WinCreateMsgQueue(s.hab, 0) };
    unsafe { WinRegisterClass(s.hab, b"RS2WINHELPWORK\0".as_ptr(), work_proc, 0, 0) };
    // HWND_OBJECT
    s.work = unsafe { WinCreateWindow(2, b"RS2WINHELPWORK\0".as_ptr(), b"\0".as_ptr(), 0, 0, 0, 0, 0, 0, 3, 0, core::ptr::null(), core::ptr::null()) };
    unsafe { os2::DosPostEventSem(s.ready) };
    let mut q = QMsg::default();
    while unsafe { WinGetMsg(s.hab, &mut q, 0, 0, 0) } != 0 {
        unsafe { WinDispatchMsg(s.hab, &q) };
    }
    let s = st();
    if s.inst != 0 {
        unsafe { WinDestroyHelpInstance(s.inst) };
        s.inst = 0;
    }
    unsafe { WinPostQueueMsg(s.hmq, WM_QUIT, 0, 0) };
}

extern "system" fn work_proc(h: u32, m: u32, a: u32, b: u32) -> u32 {
    if m == WM_RS2_DO {
        let req = unsafe { Box::from_raw(a as *mut Vec<u8>) };
        if !carry_out(&req) && b == 1 {
            message(b"Cannot open the help file.");
            unsafe { WinPostQueueMsg(st().work_hmq, WM_QUIT, 0, 0) };
        }
        return 0;
    }
    unsafe { WinDefWindowProc(h, m, a, b) }
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
    // Borland's MULTIHLP commands (Delphi's F1): 0x1004 looks the
    // keyword up in the help file's .HDX index; 0x1005 and 0x1006 carry
    // window data that is not needed here.
    if cmd == BORLAND_SEARCH {
        let (file, macro_) = match hdx_lookup(file, text) {
            Some((f, ctx)) => (f, [&b"JI(`',`"[..], &ctx, b"')"].concat()),
            None => (file.to_vec(), text.rsplit(|&c| c == b'_').next().unwrap_or(text).to_vec()),
        };
        let found = macro_.starts_with(b"JI(");
        let mut req = Vec::new();
        req.extend_from_slice(&r[..4]);
        req.extend_from_slice(&(if found { HELP_COMMAND } else { HELP_PARTIALKEY } as u16).to_le_bytes());
        req.extend_from_slice(&r[6..16]);
        req.extend_from_slice(&(file.len() as u16).to_le_bytes());
        req.extend_from_slice(&(macro_.len() as u16).to_le_bytes());
        req.extend_from_slice(&file);
        req.push(0);
        req.extend_from_slice(&macro_);
        req.push(0);
        return carry_out(&req);
    }
    if cmd == 0x1005 || cmd == 0x1006 {
        return true;
    }
    let s = st();
    if cmd == HELP_QUIT {
        s.users.retain(|&u| u != owner);
        if s.users.is_empty() {
            unsafe { WinPostQueueMsg(s.work_hmq, WM_QUIT, 0, 0) };
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

/// A file read whole.
fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let mut z = path.to_vec();
    z.push(0);
    let (mut hf, mut action) = (0u32, 0u32);
    // OPEN_ACTION_OPEN_IF_EXISTS; read-only, deny write.
    if unsafe { os2::DosOpen(z.as_ptr(), &mut hf, &mut action, 0, 0, 0x0001, 0x0020, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut size = 0u32;
    unsafe { os2::DosSetFilePtr(hf, 0, 2, &mut size) };
    unsafe { os2::DosSetFilePtr(hf, 0, 0, &mut 0) };
    let mut buf = alloc::vec![0u8; size as usize];
    let mut got = 0u32;
    let rc = unsafe { os2::DosRead(hf, buf.as_mut_ptr(), size, &mut got) };
    unsafe { os2::DosClose(hf) };
    (rc == 0 && got == size).then_some(buf)
}

/// Borland's help index (`.HDX`, made by HELPINST from the `.KWF` keyword
/// files, next to the help file of the same name): the help file and
/// context of `keyword`'s first topic.
///
/// The file: `Borland Help Data File`, 1Ah, the number of help files and
/// their names (`u8` length, the name), two `u16`s and a table of
/// keywords; then the records, one topic each: `u8` record length (all
/// of it), `u8` flags, the keyword, the topic's context string and its
/// title (each `u8` length, text) and `u8` the help file's number.
fn hdx_lookup(help: &[u8], keyword: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if keyword.is_empty() || keyword.len() > 255 {
        return None;
    }
    let dot = help.iter().rposition(|&c| c == b'.')?;
    let hdx = [&help[..dot], b".HDX"].concat();
    let d = read_file(&hdx)?;
    const SIG: &[u8] = b"Borland Help Data File\x1a";
    if !d.starts_with(SIG) {
        return None;
    }
    let mut p = SIG.len();
    let n = *d.get(p)? as usize;
    p += 1;
    let mut files: Vec<&[u8]> = Vec::new();
    for _ in 0..n {
        let l = *d.get(p)? as usize;
        files.push(d.get(p + 1..p + 1 + l)?);
        p += 1 + l;
    }
    let k = keyword.len();
    let mut i = p + 2;
    while i + 1 + k < d.len() {
        if d[i] as usize == k && d[i + 1..i + 1 + k].eq_ignore_ascii_case(keyword) {
            let len = d[i - 2] as usize;
            let mut q = i + 1 + k;
            let ctx_len = d[q] as usize;
            let ctx = d.get(q + 1..q + 1 + ctx_len);
            q += 1 + ctx_len;
            let title_len = d.get(q).copied().unwrap_or(0) as usize;
            q += 1 + title_len;
            if let (Some(ctx), Some(&f)) = (ctx, d.get(q)) {
                if q + 1 == i - 2 + len && (f as usize) < files.len() {
                    let dir = &help[..help.iter().rposition(|&c| c == b'\\').map_or(0, |x| x + 1)];
                    return Some(([dir, files[f as usize]].concat(), ctx.to_vec()));
                }
            }
        }
        i += 1;
    }
    None
}
