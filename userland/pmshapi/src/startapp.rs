//! WinStartApp and WinTerminateApp: start a program from a PROGDETAILS
//! as a session (DosStartSession), as the Workplace Shell starts program
//! objects, and tell a window when it ends.
//!
//! An application started with SAF_STARTCHILDAPP is a related session:
//! the kernel writes its end to a termination queue of this process
//! (`\QUEUES\RS2\STARTAPP\pid`), which a thread reads, and the notify
//! window gets WM_APPTERMINATENOTIFY (HAPP, result code), as on OS/2.
//! The HAPP is the session's process ID. PMSHAPI does not import PMWIN
//! (text-mode programs load it): WinPostMsg is found when first needed.

use alloc::vec::Vec;

use os2::*;

const SAF_STARTCHILDAPP: u32 = 0x0002;
const SAF_MAXIMIZED: u32 = 0x0004;
const SAF_MINIMIZED: u32 = 0x0008;
const SAF_BACKGROUND: u32 = 0x0010;

const WM_APPTERMINATENOTIFY: u32 = 0x002e;

const PROG_FULLSCREEN: u32 = 1;
const PROG_WINDOWABLEVIO: u32 = 2;
const PROG_PM: u32 = 3;
const PROG_VDM: u32 = 4;
const PROG_WINDOWEDVDM: u32 = 7;

/// PROGDETAILS (pmshl.h), up to the strings.
#[repr(C)]
struct ProgDetails {
    length: u32,
    progc: u32,
    visible: u32,
    title: *const u8,
    executable: *const u8,
    parameters: *const u8,
    startup_dir: *const u8,
    icon: *const u8,
    environment: *const u8,
}

/// STARTDATA through InitYSize (the window's state and position).
#[repr(C)]
struct StartData {
    length: u16,
    related: u16,
    fgbg: u16,
    trace: u16,
    title: *const u8,
    name: *const u8,
    inputs: *const u8,
    termq: *const u8,
    env: *const u8,
    inherit: u16,
    session_type: u16,
    icon_file: *const u8,
    pgm_handle: u32,
    pgm_control: u16,
    x: u16,
    y: u16,
    cx: u16,
    cy: u16,
}

/// The notify windows of the related sessions started: (session, HAPP,
/// window).
static mut APPS: Vec<(u32, u32, u32)> = Vec::new();
static mut QUEUE: u32 = 0;
static mut LOCK: u32 = 0;

fn lock() {
    unsafe {
        if LOCK == 0 {
            DosCreateMutexSem(core::ptr::null(), &raw mut LOCK, 0, 0);
        }
        DosRequestMutexSem(LOCK, u32::MAX);
    }
}

fn unlock() {
    unsafe { DosReleaseMutexSem(LOCK) };
}

fn bytes(p: *const u8) -> &'static [u8] {
    if p.is_null() {
        return b"";
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

fn z(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn pid() -> u32 {
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut Pib = core::ptr::null_mut();
    unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        (*pib).pid
    }
}

fn queue_name() -> Vec<u8> {
    let mut n = b"\\QUEUES\\RS2\\STARTAPP\\".to_vec();
    n.extend_from_slice(alloc::format!("{}", pid()).as_bytes());
    n.push(0);
    n
}

/// WinPostMsg, found in PMWIN.
fn post(hwnd: u32, msg: u32, mp1: u32, mp2: u32) {
    static mut POST: usize = 0;
    unsafe {
        if POST == 0 {
            let mut hmod = 0;
            if DosLoadModule(core::ptr::null_mut(), 0, b"PMWIN\0".as_ptr(), &mut hmod) != 0
                || DosQueryProcAddr(hmod, 919, core::ptr::null(), &raw mut POST) != 0
            {
                POST = 0;
                return;
            }
        }
        let f: extern "C" fn(u32, u32, u32, u32) -> u32 = core::mem::transmute(POST);
        f(hwnd, msg, mp1, mp2);
    }
}

/// The termination queue's reader: WM_APPTERMINATENOTIFY for each
/// related session that ends.
extern "C" fn reader(_: u32) {
    loop {
        let mut req = RequestData { pid: 0, data: 0 };
        let (mut cb, mut pb, mut prio) = (0u32, core::ptr::null::<u8>(), 0u8);
        if unsafe { DosReadQueue(QUEUE, &mut req, &mut cb, &mut pb, 0, 0, &mut prio, 0) } != 0 {
            return;
        }
        if pb.is_null() || cb < 4 {
            continue;
        }
        let (sid, rc) = unsafe { (u16::from_le_bytes([*pb, *pb.add(1)]) as u32, u16::from_le_bytes([*pb.add(2), *pb.add(3)]) as u32) };
        lock();
        let found = unsafe {
            let apps = &mut *(&raw mut APPS);
            apps.iter().position(|a| a.0 == sid).map(|i| apps.remove(i))
        };
        unlock();
        if let Some((_, happ, hwnd)) = found {
            if hwnd != 0 {
                post(hwnd, WM_APPTERMINATENOTIFY, happ, rc);
            }
        }
    }
}

fn termq() -> Option<Vec<u8>> {
    let name = queue_name();
    unsafe {
        if QUEUE == 0 {
            // QUE_FIFO
            if DosCreateQueue(&raw mut QUEUE, 0, name.as_ptr()) != 0 {
                return None;
            }
            let mut tid = 0;
            DosCreateThread(&mut tid, reader, 0, 0, 0x4000);
        }
    }
    Some(name)
}

/// The environment of the new session: this process's, then the
/// details' variables (replacing those of the same names). The
/// Workplace Shell's own WORKPLACE_PROCESS=YES becomes
/// WORKPLACE__PROCESS=NO, as IBM's shell passes it on.
fn environment(extra: *const u8) -> Vec<u8> {
    let mut extra_list: Vec<&[u8]> = Vec::new();
    let mut p = extra;
    while !p.is_null() && unsafe { *p } != 0 {
        let s = bytes(p);
        extra_list.push(s);
        p = unsafe { p.add(s.len() + 1) };
    }
    let name = |s: &[u8]| s.split(|&c| c == b'=').next().unwrap_or(b"").to_ascii_uppercase();
    let names: Vec<Vec<u8>> = extra_list.iter().map(|s| name(s)).collect();
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut Pib = core::ptr::null_mut();
    let mut own = unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        (*pib).pchenv as *const u8
    };
    let mut b = Vec::new();
    while !own.is_null() && unsafe { *own } != 0 {
        let s = bytes(own);
        own = unsafe { own.add(s.len() + 1) };
        if names.contains(&name(s)) {
            continue;
        }
        if s.eq_ignore_ascii_case(b"WORKPLACE_PROCESS=YES") {
            b.extend_from_slice(b"WORKPLACE__PROCESS=NO");
        } else {
            b.extend_from_slice(s);
        }
        b.push(0);
    }
    for s in extra_list {
        b.extend_from_slice(s);
        b.push(0);
    }
    b.push(0);
    b
}

/// WinStartApp(hwndNotify, pDetails, pszParams, Reserved, fbOptions):
/// the program as a session (a PM, windowed or full-screen OS/2, or DOS
/// session as the details' progc says; the command processor for no
/// executable or "*", and for .CMD/.BAT files), in its startup
/// directory, with its environment. pszParams, when given, replaces the
/// details' parameters. Returns the HAPP, 0 if it did not start.
#[unsafe(no_mangle)]
pub extern "C" fn WinStartApp(hwnd: u32, details: *const u8, params: *const u8, _reserved: u32, options: u32) -> u32 {
    if details.is_null() {
        return 0;
    }
    let d = unsafe { &*(details as *const ProgDetails) };
    let exe = bytes(d.executable);
    let args = if params.is_null() { bytes(d.parameters) } else { bytes(params) };
    let up = exe.to_ascii_uppercase();
    let dos = d.progc == PROG_VDM || d.progc == PROG_WINDOWEDVDM;
    let batch = !dos && (up.ends_with(b".CMD") || up.ends_with(b".BAT"));
    let shell = exe.is_empty() || exe == b"*";
    let (name, inputs): (Option<Vec<u8>>, Vec<u8>) = if batch {
        let mut i = b"/C ".to_vec();
        i.extend_from_slice(exe);
        if !args.is_empty() {
            i.push(b' ');
            i.extend_from_slice(args);
        }
        (None, i)
    } else if shell {
        (None, args.to_vec())
    } else {
        (Some(z(exe)), args.to_vec())
    };
    let session_type = match d.progc {
        PROG_FULLSCREEN => 1,
        PROG_WINDOWABLEVIO => 2,
        PROG_PM => 3,
        PROG_VDM => 4,
        PROG_WINDOWEDVDM => 7,
        13..=20 => d.progc as u16,
        _ if batch => 2,
        _ => 0,
    };
    // The session's current directory is this process's when it starts.
    let dir = bytes(d.startup_dir);
    let (mut disk, mut map) = (0u32, 0u32);
    let mut old = [0u8; 260];
    let mut cb = old.len() as u32;
    unsafe {
        DosQueryCurrentDisk(&mut disk, &mut map);
        if DosQueryCurrentDir(0, old.as_mut_ptr(), &mut cb) != 0 {
            old[0] = 0;
        }
    }
    if !dir.is_empty() {
        if dir.len() >= 2 && dir[1] == b':' && dir[0].is_ascii_alphabetic() {
            unsafe { DosSetDefaultDisk((dir[0].to_ascii_uppercase() - b'A' + 1) as u32) };
        }
        let c = z(dir);
        unsafe { DosSetCurrentDir(c.as_ptr()) };
    }
    let title = bytes(d.title);
    let title = if title.is_empty() { exe.rsplit(|&c| c == b'\\').next().unwrap_or(b"") } else { title };
    let (t, i) = (z(title), z(&inputs));
    let env = environment(d.environment);
    let child = options & SAF_STARTCHILDAPP != 0;
    let q = if child { termq() } else { None };
    // SSF_CONTROL_MAXIMIZE 2, SSF_CONTROL_MINIMIZE 4.
    let control = if options & SAF_MAXIMIZED != 0 { 2 } else if options & SAF_MINIMIZED != 0 { 4 } else { 0 };
    let sd = StartData {
        length: core::mem::size_of::<StartData>() as u16,
        related: child as u16,
        fgbg: (options & SAF_BACKGROUND != 0) as u16,
        trace: 0,
        title: t.as_ptr(),
        name: name.as_ref().map_or(core::ptr::null(), |n| n.as_ptr()),
        inputs: i.as_ptr(),
        termq: q.as_ref().map_or(core::ptr::null(), |n| n.as_ptr()),
        env: env.as_ptr(),
        inherit: 0,
        session_type,
        icon_file: core::ptr::null(),
        pgm_handle: 0,
        pgm_control: control,
        x: 0,
        y: 0,
        cx: 0,
        cy: 0,
    };
    let (mut sid, mut spid) = (0u32, 0u32);
    if child {
        lock();
    }
    let rc = unsafe { DosStartSession(&sd as *const StartData as *const os2::StartData, &mut sid, &mut spid) };
    // ERROR_SMG_START_IN_BACKGROUND is not an error.
    let ok = rc == 0 || rc == 457;
    let happ = if ok { if spid != 0 { spid } else { sid | 0x8000_0000 } } else { 0 };
    if child {
        if ok {
            unsafe { (*(&raw mut APPS)).push((sid, happ, hwnd)) };
        }
        unlock();
    }
    unsafe {
        DosSetDefaultDisk(disk);
        if old[0] != 0 {
            let mut p = b"\\".to_vec();
            p.extend_from_slice(bytes(old.as_ptr()));
            p.push(0);
            DosSetCurrentDir(p.as_ptr());
        }
    }
    happ
}

/// WinTerminateApp(happ): end an application this process started as a
/// related session.
#[unsafe(no_mangle)]
pub extern "C" fn WinTerminateApp(happ: u32) -> u32 {
    lock();
    let sid = unsafe { (*(&raw const APPS)).iter().find(|a| a.1 == happ).map(|a| a.0) };
    unlock();
    match sid {
        // STOP_SESSION_SPECIFIED
        Some(s) => (unsafe { DosStopSession(0, s) } == 0) as u32,
        None => 0,
    }
}
