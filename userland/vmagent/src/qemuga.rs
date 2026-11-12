//! QEMUGA.EXE: the QEMU guest agent (docs/AGENT.md), for Proxmox VE,
//! libvirt and `virsh qemu-agent-command`: shutting down and restarting
//! cleanly, the network addresses, the OS, host name, time and processors,
//! file system "freezes" around backups (the caches written out), files
//! read and written, and programs run (`guest-exec`, their output kept). The protocol is guestagent::qga; this is the system's
//! side. CONFIG.SYS starts it (`RUN=C:\OS2\QEMUGA.EXE`); without the port
//! `org.qemu.guest_agent.0` (`\DEV\QEMUGA$`) it ends at once.
//!
//! A shutdown goes through the Workplace Shell when it runs (programs are
//! asked to close, as with the desktop's Shut down), else the file systems
//! are flushed and the machine turned off or restarted at once.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use core::sync::atomic::{AtomicBool, Ordering};

use guestagent::qga::{self, Agent, Capture, Exec, ExecStatus, Guest, Iface, OsInfo, Shutdown};

os2::entry!(main);

unsafe extern "C" {
    #[link_name = "DOSCALLS.415"]
    fn DosShutdown(reserved: u32) -> u32;
    #[link_name = "DOSCALLS.254"]
    fn DosResetBuffer(h: u32) -> u32;
    #[link_name = "RS2API.7"]
    fn Rs2PowerOff(action: u32) -> u32;
    #[link_name = "DOSCALLS.228"]
    fn DosSearchPath(flags: u32, path: *const u8, name: *const u8, buf: *mut u8, cb: u32) -> u32;
}

/// Output kept per stream (qemu-ga's limit).
const MAX_OUTPUT: usize = 16 * 1024 * 1024;

/// A child's output pipe, read by a thread of its own until it ends.
struct Sink {
    pipe: u32,
    data: Vec<u8>,
    truncated: bool,
    done: AtomicBool,
}

unsafe extern "C" fn drain(p: u32) {
    let s = unsafe { &mut *(p as *mut Sink) };
    let mut buf = alloc::vec![0u8; 4096];
    loop {
        let mut n = 0;
        if unsafe { os2::DosRead(s.pipe, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        let room = MAX_OUTPUT - s.data.len();
        s.data.extend_from_slice(&buf[..(n as usize).min(room)]);
        s.truncated |= n as usize > room;
    }
    unsafe { os2::DosClose(s.pipe) };
    s.done.store(true, Ordering::Release);
}

struct Feed {
    pipe: u32,
    data: Vec<u8>,
}

/// A child's standard input, written by a thread and then closed.
unsafe extern "C" fn feed(p: u32) {
    let f = unsafe { alloc::boxed::Box::from_raw(p as *mut Feed) };
    let mut at = 0;
    while at < f.data.len() {
        let mut w = 0;
        if unsafe { os2::DosWrite(f.pipe, f.data[at..].as_ptr(), (f.data.len() - at) as u32, &mut w) } != 0 || w == 0 {
            break;
        }
        at += w as usize;
    }
    unsafe { os2::DosClose(f.pipe) };
}

fn thread(f: unsafe extern "C" fn(u32), param: u32) -> bool {
    let mut tid = 0;
    unsafe { os2::DosCreateThread(&mut tid, f, param, 2, 65536) == 0 }
}

/// A program started by guest-exec. The sinks belong to their threads
/// until `done`.
struct Child {
    pid: u32,
    out: *mut Sink,
    err: Option<*mut Sink>,
    capture: Capture,
    ended: Option<os2::ResultCodes>,
}

struct Os2 {
    files: Vec<u32>,
    children: Vec<Child>,
}

fn pipe() -> Result<(u32, u32), String> {
    let (mut r, mut w) = (0, 0);
    match unsafe { os2::DosCreatePipe(&mut r, &mut w, 4096) } {
        0 => Ok((r, w)),
        rc => Err(rc_text("DosCreatePipe", rc)),
    }
}

fn cstr(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = os2::text_bytes(s).into_owned();
    v.push(0);
    v
}

/// Code page bytes as a `str`, one character per byte (as os2::text_bytes
/// takes them back).
fn text_string(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn env(name: &str) -> Option<String> {
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(cstr(name).as_ptr(), &mut v) } != 0 || v.is_null() {
        return None;
    }
    let mut n = 0;
    while unsafe { *v.add(n) } != 0 {
        n += 1;
    }
    Some(text_string(unsafe { core::slice::from_raw_parts(v, n) }))
}

/// The agent's environment block as strings.
fn environment() -> Vec<Vec<u8>> {
    let (mut tib, mut pib): (*mut os2::Tib, *mut os2::Pib) = (core::ptr::null_mut(), core::ptr::null_mut());
    let mut v = Vec::new();
    unsafe {
        os2::DosGetInfoBlocks(&mut tib, &mut pib);
        if pib.is_null() {
            return v;
        }
        let mut p = (*pib).pchenv;
        while !p.is_null() && *p != 0 {
            let mut e = Vec::new();
            while *p != 0 {
                e.push(*p);
                p = p.add(1);
            }
            p = p.add(1);
            v.push(e);
        }
    }
    v
}

/// The program's file: as given, or found along PATH (and the current
/// directory) with .EXE, .COM, .CMD or .BAT when it has no extension.
fn find_program(path: &str) -> Option<String> {
    let path = path.replace('/', "\\");
    let file = path.rsplit(['\\', ':']).next().unwrap_or("");
    let names: Vec<String> = if file.contains('.') {
        alloc::vec![path.clone()]
    } else {
        [".EXE", ".COM", ".CMD", ".BAT"].iter().map(|e| alloc::format!("{path}{e}")).collect()
    };
    let bare = !path.contains(['\\', ':']);
    for n in names {
        if bare {
            let mut buf = [0u8; 260];
            // SEARCH_CUR_DIRECTORY | SEARCH_ENVIRONMENT
            if unsafe { DosSearchPath(3, b"PATH\0".as_ptr(), cstr(&n).as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0 {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
                return Some(text_string(&buf[..len]));
            }
        } else {
            let mut st = [0u8; 64];
            if unsafe { os2::DosQueryPathInfo(cstr(&n).as_ptr(), 1, st.as_mut_ptr(), st.len() as u32) } == 0 {
                return Some(n);
            }
        }
    }
    None
}

fn is_command_processor(prog: &str) -> bool {
    let f = prog.rsplit(['\\', ':']).next().unwrap_or("").to_ascii_uppercase();
    matches!(f.as_str(), "CMD.EXE" | "4OS2.EXE")
}

fn rc_text(what: &str, rc: u32) -> String {
    let why = match rc {
        2 => "The system cannot find the file specified.",
        3 => "The system cannot find the path specified.",
        5 => "Access is denied.",
        6 => "The handle is invalid.",
        32 => "The file is in use by another process.",
        _ => "",
    };
    alloc::format!("{what}: error {rc}. {why}")
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// Minutes west of UTC of the system clock (0 when not set).
fn tz_west(dt: &os2::DateTime) -> i64 {
    if dt.timezone == -1 { 0 } else { dt.timezone as i64 }
}

fn sysinfo(i: u32) -> u32 {
    let mut v = 0;
    unsafe { os2::DosQuerySysInfo(i, i, &mut v, 4) };
    v
}

impl Os2 {
    fn handle(&self, h: u32) -> Result<u32, String> {
        if self.files.contains(&h) { Ok(h) } else { Err(String::from("Invalid parameter 'handle'")) }
    }
}

impl Guest for Os2 {
    fn hostname(&mut self) -> String {
        os2::net::hostname()
    }

    fn interfaces(&mut self) -> Vec<Iface> {
        os2::net::os2_interfaces()
            .into_iter()
            .map(|(name, i)| Iface {
                mac: (i.flags & os2::net::IF_LOOPBACK == 0).then_some(i.mac),
                ipv4: if i.addr != [0; 4] { alloc::vec![(i.addr, qga::prefix(i.mask))] } else { Vec::new() },
                rx_bytes: i.rx_bytes as u64,
                rx_packets: i.rx_packets as u64,
                tx_bytes: i.tx_bytes as u64,
                tx_packets: i.tx_packets as u64,
                rx_dropped: i.rx_dropped as u64,
                name,
            })
            .collect()
    }

    fn os_info(&mut self) -> OsInfo {
        OsInfo { version: String::from(env!("CARGO_PKG_VERSION")), machine: "x86_64" }
    }

    fn time_ns(&mut self) -> i64 {
        let mut dt: os2::DateTime = unsafe { core::mem::zeroed() };
        unsafe { os2::DosGetDateTime(&mut dt) };
        let days = days_from_civil(dt.year as i64, dt.month as i64, dt.day as i64);
        let secs = days * 86400 + dt.hours as i64 * 3600 + dt.minutes as i64 * 60 + dt.seconds as i64 + tz_west(&dt) * 60;
        secs * 1_000_000_000 + dt.hundredths as i64 * 10_000_000
    }

    fn set_time_ns(&mut self, ns: Option<i64>) -> Result<(), String> {
        let Some(ns) = ns else { return Ok(()) };
        let mut dt: os2::DateTime = unsafe { core::mem::zeroed() };
        unsafe { os2::DosGetDateTime(&mut dt) };
        let local = ns.div_euclid(1_000_000_000) - tz_west(&dt) * 60;
        let (y, m, d) = civil_from_days(local.div_euclid(86400));
        let t = local.rem_euclid(86400);
        dt.year = y as u16;
        dt.month = m as u8;
        dt.day = d as u8;
        dt.hours = (t / 3600) as u8;
        dt.minutes = (t / 60 % 60) as u8;
        dt.seconds = (t % 60) as u8;
        dt.hundredths = (ns.rem_euclid(1_000_000_000) / 10_000_000) as u8;
        dt.weekday = ((local.div_euclid(86400) + 4).rem_euclid(7)) as u8;
        match unsafe { os2::DosSetDateTime(&dt) } {
            0 => Ok(()),
            rc => Err(rc_text("DosSetDateTime", rc)),
        }
    }

    fn timezone(&mut self) -> (i64, Option<String>) {
        let mut dt: os2::DateTime = unsafe { core::mem::zeroed() };
        unsafe { os2::DosGetDateTime(&mut dt) };
        (-tz_west(&dt) * 60, None)
    }

    fn cpus(&mut self) -> u32 {
        // QSV_NUMPROCESSORS
        sysinfo(26).max(1)
    }

    fn shutdown(&mut self, how: Shutdown) -> Result<(), String> {
        let restart = (how == Shutdown::Reboot) as u32;
        os2::println!("QEMUGA: {}", if restart == 1 { "restarting" } else { "shutting down" });
        // Through the Workplace Shell, if it runs (PMWP's Rs2ShutdownSystem).
        let mut hmod = 0;
        let mut fail = [0u8; 16];
        if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), 16, b"PMWP\0".as_ptr(), &mut hmod) } == 0 {
            let mut pfn = 0usize;
            if unsafe { os2::DosQueryProcAddr(hmod, 9101, core::ptr::null(), &mut pfn) } == 0 && pfn != 0 {
                let f: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(pfn) };
                if f(restart) != 0 {
                    os2::println!("QEMUGA: the Workplace Shell closes the programs");
                    return Ok(());
                }
            }
        }
        unsafe {
            DosShutdown(0);
            Rs2PowerOff(restart);
        }
        Err(String::from("The machine could not be turned off"))
    }

    fn flush(&mut self) -> u32 {
        unsafe { DosResetBuffer(0xffff_ffff) };
        1
    }

    fn file_open(&mut self, path: &str, mode: &str) -> Result<u32, String> {
        // OPEN_ACTION_*: fail/open/replace if exists, create if new.
        let (action, write, append) = match mode.trim_end_matches('b').trim_end_matches("b+") {
            "r" => (0x01, false, false),
            "r+" => (0x01, true, false),
            "w" | "w+" => (0x12, true, false),
            "a" | "a+" => (0x11, true, true),
            m if m.starts_with('r') && m.ends_with('+') => (0x01, true, false),
            _ => return Err(alloc::format!("Invalid parameter 'mode': {mode}")),
        };
        let mut name: Vec<u8> = os2::text_bytes(path).into_owned();
        name.push(0);
        let (mut h, mut act) = (0u32, 0u32);
        // Deny none; read-only or read/write.
        let open_mode = 0x40 | if write { 2 } else { 0 };
        let rc = unsafe { os2::DosOpen(name.as_ptr(), &mut h, &mut act, 0, 0, action, open_mode, core::ptr::null_mut()) };
        if rc != 0 {
            return Err(rc_text(path, rc));
        }
        if append {
            let mut pos = 0;
            unsafe { os2::DosSetFilePtr(h, 0, 2, &mut pos) };
        }
        self.files.push(h);
        Ok(h)
    }

    fn file_read(&mut self, h: u32, count: usize) -> Result<(Vec<u8>, bool), String> {
        let h = self.handle(h)?;
        let mut buf = alloc::vec![0u8; count];
        let mut got = 0;
        let rc = unsafe { os2::DosRead(h, buf.as_mut_ptr(), count as u32, &mut got) };
        if rc != 0 {
            return Err(rc_text("DosRead", rc));
        }
        buf.truncate(got as usize);
        Ok((buf, (got as usize) < count))
    }

    fn file_write(&mut self, h: u32, data: &[u8]) -> Result<usize, String> {
        let h = self.handle(h)?;
        let mut n = 0;
        match unsafe { os2::DosWrite(h, data.as_ptr(), data.len() as u32, &mut n) } {
            0 => Ok(n as usize),
            rc => Err(rc_text("DosWrite", rc)),
        }
    }

    fn file_seek(&mut self, h: u32, offset: i64, whence: u32) -> Result<(u64, bool), String> {
        let h = self.handle(h)?;
        if whence > 2 {
            return Err(String::from("Invalid parameter 'whence'"));
        }
        let (mut pos, mut end) = (0u32, 0u32);
        let rc = unsafe { os2::DosSetFilePtr(h, offset as i32, whence, &mut pos) };
        if rc != 0 {
            return Err(rc_text("DosSetFilePtr", rc));
        }
        // The size, then back.
        unsafe {
            os2::DosSetFilePtr(h, 0, 2, &mut end);
            let mut p = 0;
            os2::DosSetFilePtr(h, pos as i32, 0, &mut p);
        }
        Ok((pos as u64, pos >= end))
    }

    fn file_flush(&mut self, h: u32) -> Result<(), String> {
        let h = self.handle(h)?;
        match unsafe { DosResetBuffer(h) } {
            0 => Ok(()),
            rc => Err(rc_text("DosResetBuffer", rc)),
        }
    }

    fn file_close(&mut self, h: u32) -> Result<(), String> {
        let h = self.handle(h)?;
        self.files.retain(|&f| f != h);
        match unsafe { os2::DosClose(h) } {
            0 => Ok(()),
            rc => Err(rc_text("DosClose", rc)),
        }
    }

    fn exec(&mut self, e: &Exec) -> Result<u32, String> {
        let found = find_program(&e.path).ok_or_else(|| alloc::format!("Failed to execute child process \u{201c}{}\u{201d} (No such file or directory)", e.path))?;
        let ext = found.rsplit('.').next().unwrap_or("").to_ascii_uppercase();
        // Batch files through the command processor.
        let (prog, mut tail) = if ext == "CMD" || ext == "BAT" {
            let comspec = env("COMSPEC").filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("C:\\OS2\\CMD.EXE"));
            (comspec, alloc::format!("/c {found} "))
        } else {
            (found, String::new())
        };
        // The command processor takes its command line as typed; other
        // programs split it as the C runtime does.
        if is_command_processor(&prog) {
            tail.push_str(&e.args.join(" "));
        } else {
            tail.push_str(&qga::command_line(&e.args));
        }
        let progz = cstr(&prog);
        let mut argv = progz.clone();
        argv.extend_from_slice(&os2::text_bytes(tail.trim_end()));
        argv.extend_from_slice(&[0, 0]);
        // Our environment with the host's variables over it.
        let key = |v: &[u8]| -> Vec<u8> { v.split(|&c| c == b'=').next().unwrap_or(&[]).to_ascii_uppercase() };
        let extra: Vec<Vec<u8>> = e.env.iter().map(|v| os2::text_bytes(v).into_owned()).collect();
        let mut envb = Vec::new();
        for v in environment().into_iter().filter(|v| !extra.iter().any(|x| key(x) == key(v))).chain(extra.iter().cloned()) {
            envb.extend_from_slice(&v);
            envb.push(0);
        }
        envb.push(0);

        let (in_r, in_w) = pipe()?;
        let (out_r, out_w) = pipe()?;
        let err = if e.capture == Capture::Merged { None } else { Some(pipe()?) };
        let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
        let mut fail = [0u8; 64];
        let rc = unsafe {
            for h in [in_w, out_r].into_iter().chain(err.map(|p| p.0)) {
                os2::DosSetFHState(h, os2::OPEN_FLAGS_NOINHERIT);
            }
            // The child's standard handles, then ours back.
            let mut saved = [0xffff_ffffu32; 3];
            for (h, s) in saved.iter_mut().enumerate() {
                if os2::DosDupHandle(h as u32, s) == 0 {
                    os2::DosSetFHState(*s, os2::OPEN_FLAGS_NOINHERIT);
                }
            }
            for (h, from) in [(0u32, in_r), (1, out_w), (2, err.map_or(out_w, |p| p.1))] {
                let mut t = h;
                os2::DosDupHandle(from, &mut t);
            }
            // EXEC_ASYNCRESULT
            let rc = os2::DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 2, argv.as_ptr(), envb.as_ptr(), &mut res, progz.as_ptr());
            for (h, s) in saved.iter().enumerate() {
                let mut t = h as u32;
                if *s != 0xffff_ffff {
                    os2::DosDupHandle(*s, &mut t);
                    os2::DosClose(*s);
                } else {
                    os2::DosClose(t);
                }
            }
            os2::DosClose(in_r);
            os2::DosClose(out_w);
            if let Some((_, w)) = err {
                os2::DosClose(w);
            }
            rc
        };
        if rc != 0 {
            unsafe {
                for h in [in_w, out_r].into_iter().chain(err.map(|p| p.0)) {
                    os2::DosClose(h);
                }
            }
            return Err(rc_text(&alloc::format!("Failed to execute child process \u{201c}{}\u{201d}", e.path), rc));
        }
        let pid = res.code_terminate;
        os2::println!("QEMUGA: started {prog} (process {pid})");
        // Standard input: the host's data, or none.
        match &e.input {
            Some(d) if !d.is_empty() => {
                let f = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Feed { pipe: in_w, data: d.clone() }));
                if !thread(feed, f as u32) {
                    unsafe {
                        drop(alloc::boxed::Box::from_raw(f));
                        os2::DosClose(in_w);
                    }
                }
            }
            _ => unsafe {
                os2::DosClose(in_w);
            },
        }
        let sink = |pipe: u32| {
            let s = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Sink { pipe, data: Vec::new(), truncated: false, done: AtomicBool::new(false) }));
            if !thread(drain, s as u32) {
                unsafe {
                    os2::DosClose(pipe);
                    (*s).done.store(true, Ordering::Release);
                }
            }
            s
        };
        let out = sink(out_r);
        let err = err.map(|p| sink(p.0));
        self.children.push(Child { pid, out, err, capture: e.capture, ended: None });
        Ok(pid)
    }

    fn exec_status(&mut self, pid: u32) -> Result<ExecStatus, String> {
        let i = self.children.iter().position(|c| c.pid == pid).ok_or_else(|| String::from("Invalid parameter 'pid'"))?;
        let c = &mut self.children[i];
        if c.ended.is_none() {
            let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
            let mut who = 0;
            // DCWA_PROCESS, DCWW_NOWAIT
            match unsafe { os2::DosWaitChild(0, os2::DCWW_NOWAIT, &mut res, &mut who, pid) } {
                0 => c.ended = Some(res),
                // ERROR_CHILD_NOT_COMPLETE
                129 => {}
                // Gone without a result (already collected).
                _ => c.ended = Some(os2::ResultCodes { code_terminate: 0, code_result: 0 }),
            }
        }
        let done = |s: *mut Sink| unsafe { (*s).done.load(Ordering::Acquire) };
        let Some((term, code)) = c.ended.as_ref().map(|r| (r.code_terminate, r.code_result)) else { return Ok(ExecStatus::default()) };
        // Exited once its output has ended too.
        if !done(c.out) || c.err.is_some_and(|s| !done(s)) {
            return Ok(ExecStatus::default());
        }
        let mut st = ExecStatus { exited: true, ..Default::default() };
        match term {
            // TC_EXIT
            0 => st.exitcode = Some(code as i32 as i64),
            // TC_HARDERROR: SIGABRT
            1 => st.signal = Some(6),
            // TC_KILLPROCESS: SIGKILL
            3 => st.signal = Some(9),
            // TC_TRAP, TC_EXCEPTION: SIGSEGV
            _ => st.signal = Some(11),
        }
        let c = self.children.remove(i);
        let out = unsafe { alloc::boxed::Box::from_raw(c.out) };
        let err = c.err.map(|s| unsafe { alloc::boxed::Box::from_raw(s) });
        let take = |s: alloc::boxed::Box<Sink>| (s.data, s.truncated);
        match c.capture {
            Capture::None => {}
            Capture::Stdout => st.out = Some(take(out)),
            Capture::Stderr => st.err = err.map(take),
            Capture::Separated => {
                st.out = Some(take(out));
                st.err = err.map(take);
            }
            Capture::Merged => st.out = Some(take(out)),
        }
        os2::println!("QEMUGA: process {pid} ended ({term}, {code})");
        Ok(st)
    }
}

fn main(_s: &os2::Startup) -> u32 {
    let (mut h, mut action) = (0u32, 0u32);
    if unsafe { os2::DosOpen(b"\\DEV\\QEMUGA$\0".as_ptr(), &mut h, &mut action, 0, 0, 0x01, 0x0042, core::ptr::null_mut()) } != 0 {
        return 1;
    }
    // One agent.
    let mut mtx = 0;
    if unsafe { os2::DosCreateMutexSem(b"\\SEM32\\RS2\\QEMUGA\0".as_ptr(), &mut mtx, 0, 0) } != 0 {
        return 0;
    }
    let mut g = Os2 { files: Vec::new(), children: Vec::new() };
    let mut a = Agent::new();
    let mut buf = alloc::vec![0u8; 4096];
    loop {
        let mut n = 0;
        let rc = unsafe { os2::DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if rc != 0 || n == 0 {
            unsafe { os2::DosSleep(500) };
            continue;
        }
        let out = a.feed(&mut g, &buf[..n as usize]);
        if !out.is_empty() {
            let mut w = 0;
            unsafe { os2::DosWrite(h, out.as_ptr(), out.len() as u32, &mut w) };
        }
    }
}
