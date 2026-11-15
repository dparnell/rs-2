//! WinHelp(hwnd, file, command, data): Windows Help.
//!
//! As on Windows, help is a program of its own: rs2's WINHELP.EXE (an
//! OS/2 program, `userland/winhelp`), which shows Windows `.HLP` files in
//! the help manager's window (HELPMGR converts them to IPF as it loads
//! them). WinHelp finds its window by class, `MS_WINHELP` as Windows'
//! own, starting WINHELP.EXE when there is none, and hands it the request
//! through the named shared memory WINHELP.EXE made: the file (as a full
//! path: WINHELP.EXE has its own current directory), the command, its
//! number and its string (a keyword, a macro), and the PM window of the
//! caller (the owner of the help window, and who HELP_QUIT is from).
//!
//! The request's layout is `userland/winhelp/src/main.rs`'s `Request`.

use alloc::vec::Vec;
use thunk16::flat;

use crate::far_str;

/// The class of WINHELP.EXE's window.
const CLASS: &[u8] = b"MS_WINHELP";
/// The request block and the semaphore that serializes its writers.
const SHMEM: &[u8] = b"\\SHAREMEM\\RS2\\WINHELP\0";
const MUTEX: &[u8] = b"\\SEM32\\RS2\\WINHELP\0";
/// The message WINHELP.EXE takes a request with (WM_USER + 0x157).
const WM_RS2_WINHELP: u32 = 0x1157;
const MAGIC: u32 = u32::from_le_bytes(*b"RWH1");
/// Room for the request (WINHELP.EXE's block is 4 KB).
const MAX: usize = 4096;

const HELP_CONTEXT: u16 = 0x0001;
const HELP_QUIT: u16 = 0x0002;
const HELP_SETCONTENTS: u16 = 0x0005;
const HELP_CONTEXTPOPUP: u16 = 0x0008;
const HELP_KEY: u16 = 0x0101;
const HELP_COMMAND: u16 = 0x0102;
const HELP_PARTIALKEY: u16 = 0x0105;
const HELP_MULTIKEY: u16 = 0x0201;
const HELP_SETWINPOS: u16 = 0x0203;

unsafe extern "C" {
    #[link_name = "PMWIN.702"]
    fn WinBeginEnumWindows(parent: u32) -> u32;
    #[link_name = "PMWIN.756"]
    fn WinGetNextWindow(henum: u32) -> u32;
    #[link_name = "PMWIN.737"]
    fn WinEndEnumWindows(henum: u32) -> u32;
    #[link_name = "PMWIN.805"]
    fn WinQueryClassName(h: u32, n: i32, buf: *mut u8) -> i32;
    #[link_name = "PMWIN.920"]
    fn WinSendMsg(h: u32, m: u32, a: u32, b: u32) -> u32;
    #[link_name = "DOSCALLS.332"]
    fn DosOpenMutexSem(name: *const u8, phmtx: *mut u32) -> u32;
}

/// WINHELP.EXE's window: a top-level window (a client of a desktop
/// frame) of class MS_WINHELP.
fn find_window() -> u32 {
    let mut found = 0;
    // HWND_DESKTOP's children: the frames; the client is one of theirs.
    let e = unsafe { WinBeginEnumWindows(1) };
    loop {
        let f = unsafe { WinGetNextWindow(e) };
        if f == 0 {
            break;
        }
        if is_class(f) {
            found = f;
            break;
        }
        let ce = unsafe { WinBeginEnumWindows(f) };
        loop {
            let c = unsafe { WinGetNextWindow(ce) };
            if c == 0 {
                break;
            }
            if is_class(c) {
                found = c;
                break;
            }
        }
        unsafe { WinEndEnumWindows(ce) };
        if found != 0 {
            break;
        }
    }
    unsafe { WinEndEnumWindows(e) };
    found
}

fn is_class(h: u32) -> bool {
    let mut b = [0u8; 32];
    let n = unsafe { WinQueryClassName(h, b.len() as i32, b.as_mut_ptr()) };
    n > 0 && b[..n as usize].eq_ignore_ascii_case(CLASS)
}

/// WINHELP.EXE's window, started if need be (it makes its window within
/// a few seconds).
fn server(start: bool) -> u32 {
    let h = find_window();
    if h != 0 || !start {
        return h;
    }
    let Some(prog) = winhelp_exe() else {
        crate::trace(format_args!("W16 WinHelp: no WINHELP.EXE"));
        return 0;
    };
    let mut a = prog.clone();
    a.push(0);
    a.extend_from_slice(&[0, 0]);
    let mut z = prog;
    z.push(0);
    let mut fail = [0u8; 64];
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    // EXEC_ASYNC: it runs on, and outlives the caller.
    if unsafe { os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 1, a.as_ptr(), core::ptr::null(), &mut res, z.as_ptr()) } != 0 {
        return 0;
    }
    for _ in 0..100 {
        unsafe { os2::DosSleep(50) };
        let h = find_window();
        if h != 0 {
            return h;
        }
    }
    0
}

/// WINHELP.EXE: in the Windows directory (where Windows has it), else
/// rs2's in \OS2, else along the PATH.
fn winhelp_exe() -> Option<Vec<u8>> {
    if let Some(p) = crate::compat::find_program(b"WINHELP.EXE") {
        return Some(full_path(&p));
    }
    let boot = [boot_drive(), b':'];
    let p = [&boot[..], b"\\OS2\\WINHELP.EXE"].concat();
    exists(&p).then_some(p)
}

fn boot_drive() -> u8 {
    let mut d = 3u32;
    // QSV_BOOT_DRIVE
    unsafe { os2::DosQuerySysInfo(5, 5, &mut d, 4) };
    b'A' + d as u8 - 1
}

fn exists(p: &[u8]) -> bool {
    let mut z = p.to_vec();
    z.push(0);
    let mut buf = [0u8; 64];
    unsafe { os2::DosQueryPathInfo(z.as_ptr(), 1, buf.as_mut_ptr(), buf.len() as u32) == 0 }
}

/// `p` as a full path (FIL_QUERYFULLNAME).
fn full_path(p: &[u8]) -> Vec<u8> {
    let mut z = p.to_vec();
    z.push(0);
    let mut buf = [0u8; 260];
    if unsafe { os2::DosQueryPathInfo(z.as_ptr(), 5, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
        return unsafe { crate::cstr(buf.as_ptr()) }.to_vec();
    }
    p.to_vec()
}

/// The help file as Windows finds it: as given with a path, else in the
/// current directory, the Windows directories, the program's directory
/// and along the PATH. A `>window` suffix (a secondary window) is kept
/// apart.
fn help_file(name: &[u8]) -> Vec<u8> {
    let name = match name.iter().position(|&c| c == b'>') {
        Some(i) => &name[..i],
        None => name,
    };
    match crate::compat::find_program(name) {
        Some(p) => full_path(&p),
        None => full_path(name),
    }
}

/// WinHelp(hwnd, file, command, data): TRUE when the request was given to
/// WINHELP.EXE.
pub fn win_help(hwnd: u16, file: u32, cmd: u16, data: u32) -> u32 {
    let file = far_str(file).unwrap_or(b"").to_vec();
    if crate::tracing() {
        crate::trace(format_args!("W16 WinHelp {} cmd {cmd:#x} data {data:#x}", alloc::string::String::from_utf8_lossy(&file)));
    }
    // Windows does not start WinHelp to quit it.
    let srv = server(cmd != HELP_QUIT);
    if srv == 0 {
        return (cmd == HELP_QUIT) as u32;
    }
    let path = if file.is_empty() { Vec::new() } else { help_file(&file) };
    // The command's string, or the number.
    let (num, text): (u32, Vec<u8>) = match cmd {
        HELP_KEY | HELP_PARTIALKEY | HELP_COMMAND => (0, far_str(data).unwrap_or(b"").to_vec()),
        // MULTIKEYHELP: mkSize, mkKeylist (the table's letter), the
        // phrase.
        HELP_MULTIKEY => {
            let p = flat(data);
            let list = unsafe { *p.add(2) };
            (list as u32, far_str(data + 3).unwrap_or(b"").to_vec())
        }
        // HELPWININFO: wStructSize, x, y, dx, dy, wMax, rgchMember; the
        // numbers go as they are (in Windows' 1024-unit coordinates).
        HELP_SETWINPOS => {
            let p = flat(data);
            let w = |i: usize| unsafe { (p.add(2 + 2 * i) as *const u16).read_unaligned() };
            let v: Vec<u8> = (0..5).flat_map(|i| w(i).to_le_bytes()).collect();
            (0, v)
        }
        HELP_CONTEXT | HELP_CONTEXTPOPUP | HELP_SETCONTENTS => (data, Vec::new()),
        _ => (data, Vec::new()),
    };
    let owner = crate::user::wnd(crate::user::top_level(hwnd)).map_or(0, |w| w.frame);
    let mut req: Vec<u8> = Vec::with_capacity(64 + path.len() + text.len());
    req.extend_from_slice(&MAGIC.to_le_bytes());
    req.extend_from_slice(&cmd.to_le_bytes());
    req.extend_from_slice(&0u16.to_le_bytes());
    req.extend_from_slice(&num.to_le_bytes());
    req.extend_from_slice(&owner.to_le_bytes());
    req.extend_from_slice(&(path.len() as u16).to_le_bytes());
    req.extend_from_slice(&(text.len() as u16).to_le_bytes());
    req.extend_from_slice(&path);
    req.push(0);
    req.extend_from_slice(&text);
    req.push(0);
    if req.len() > MAX {
        return 0;
    }
    let mut block: *mut u8 = core::ptr::null_mut();
    // PAG_READ | PAG_WRITE
    if unsafe { os2::DosGetNamedSharedMem(&mut block, SHMEM.as_ptr(), 0x3) } != 0 {
        return 0;
    }
    let mut mtx = 0u32;
    let locked = unsafe { DosOpenMutexSem(MUTEX.as_ptr(), &mut mtx) } == 0 && unsafe { os2::DosRequestMutexSem(mtx, 5000) } == 0;
    unsafe { core::ptr::copy_nonoverlapping(req.as_ptr(), block, req.len()) };
    // WINHELP.EXE copies the request before it answers.
    let r = unsafe { WinSendMsg(srv, WM_RS2_WINHELP, 0, 0) };
    if locked {
        unsafe { os2::DosReleaseMutexSem(mtx) };
    }
    if mtx != 0 {
        unsafe { os2::DosCloseMutexSem(mtx) };
    }
    unsafe { os2::DosFreeMem(block) };
    r
}
