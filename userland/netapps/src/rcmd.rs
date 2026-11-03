//! The server side of rexec and rsh (REXECD, RSHD): reading the request,
//! the second (standard error) connection, checking the user, and running
//! the command under the command processor with its standard handles on
//! pipes that go to and from the connections.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use os2::net::*;
use os2::*;

use crate::*;

/// A NUL-terminated string from the client (at most `max` bytes).
pub fn read_z(s: &Sock, max: usize) -> Option<String> {
    let mut v = String::new();
    let mut b = [0u8; 1];
    loop {
        match s.recv(&mut b) {
            Ok(1) if b[0] == 0 => return Some(v),
            Ok(1) => {
                if v.len() >= max {
                    return None;
                }
                v.push(b[0] as char);
            }
            _ => return None,
        }
    }
}

/// Connect back to the client's port for standard error (`reserved`:
/// from a port below 1024, as rshd must).
pub fn stderr_conn(peer: [u8; 4], port: u16, reserved: bool) -> Option<Sock> {
    if !reserved {
        return Sock::connect_addr(peer, port).ok();
    }
    let first = (ms() / 100 % 256) as u16;
    connect_from(peer, port, (0..256u16).map(|k| 1023 - (first + k) % 256)).ok()
}

/// The password of `user` in %ETC%\TRUSERS (`user: name password`, as
/// FTPD reads it); None if the file names no such user.
pub fn trusers_password(user: &str) -> Option<String> {
    let d = read_file(&etc_path("TRUSERS"))?;
    for l in utils::lines(&d) {
        let l = l.trim();
        let Some((k, rest)) = l.split_once(':') else { continue };
        if !k.trim().eq_ignore_ascii_case("user") {
            continue;
        }
        let mut w = rest.split_whitespace();
        if w.next().is_some_and(|n| n.eq_ignore_ascii_case(user)) {
            return Some(String::from(w.next().unwrap_or("")));
        }
    }
    None
}

/// The process's environment strings.
pub fn environment() -> Vec<String> {
    let mut tib = core::ptr::null_mut();
    let mut pib: *mut Pib = core::ptr::null_mut();
    let mut v = Vec::new();
    unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        if pib.is_null() {
            return v;
        }
        let mut p = (*pib).pchenv;
        while !p.is_null() && *p != 0 {
            let mut e = String::new();
            while *p != 0 {
                e.push(*p as char);
                p = p.add(1);
            }
            p = p.add(1);
            v.push(e);
        }
    }
    v
}

/// Held while standard handles are switched to a child's pipes.
static EXEC_LOCK: AtomicBool = AtomicBool::new(false);

pub struct Child {
    pub pid: u32,
    pub stdin: u32,
    pub stdout: u32,
    /// None: standard error goes with standard output.
    pub stderr: Option<u32>,
}

fn pipe() -> Option<(u32, u32)> {
    let (mut r, mut w) = (0, 0);
    (unsafe { DosCreatePipe(&mut r, &mut w, 4096) } == 0).then_some((r, w))
}

/// Start `%COMSPEC% /c cmd` with its standard handles on pipes.
pub fn spawn(cmd: &str, separate_stderr: bool, extra_env: &[String]) -> Option<Child> {
    let (in_r, in_w) = pipe()?;
    let (out_r, out_w) = pipe()?;
    let err = if separate_stderr { Some(pipe()?) } else { None };
    unsafe {
        DosSetFHState(in_w, OPEN_FLAGS_NOINHERIT);
        DosSetFHState(out_r, OPEN_FLAGS_NOINHERIT);
        if let Some((r, _)) = err {
            DosSetFHState(r, OPEN_FLAGS_NOINHERIT);
        }
    }
    let comspec = env("COMSPEC").filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("C:\\OS2\\CMD.EXE"));
    let mut prog: Vec<u8> = comspec.bytes().collect();
    prog.push(0);
    let mut argv = prog.clone();
    argv.extend_from_slice(b"/c ");
    argv.extend_from_slice(cmd.as_bytes());
    argv.extend_from_slice(&[0, 0]);
    // The environment: ours and the extra variables.
    let mut envb: Vec<u8> = Vec::new();
    for e in environment() {
        let k = e.split('=').next().unwrap_or("");
        if extra_env.iter().any(|x| x.split('=').next().is_some_and(|xk| xk.eq_ignore_ascii_case(k))) {
            continue;
        }
        envb.extend_from_slice(e.as_bytes());
        envb.push(0);
    }
    for e in extra_env {
        envb.extend_from_slice(e.as_bytes());
        envb.push(0);
    }
    envb.push(0);
    while EXEC_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        unsafe { DosSleep(1) };
    }
    let mut saved = [0xffff_ffffu32; 3];
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe {
        for (h, s) in saved.iter_mut().enumerate() {
            DosDupHandle(h as u32, s);
            DosSetFHState(*s, OPEN_FLAGS_NOINHERIT);
        }
        let mut t = 0;
        DosDupHandle(in_r, &mut t);
        let mut t = 1;
        DosDupHandle(out_w, &mut t);
        let mut t = 2;
        DosDupHandle(err.map_or(out_w, |e| e.1), &mut t);
        let mut fail = [0u8; 64];
        let rc = DosExecPgm(fail.as_mut_ptr(), 64, 2, argv.as_ptr(), envb.as_ptr(), &mut res, prog.as_ptr());
        for (h, s) in saved.iter().enumerate() {
            let mut t = h as u32;
            DosDupHandle(*s, &mut t);
            DosClose(*s);
        }
        rc
    };
    EXEC_LOCK.store(false, Ordering::Release);
    unsafe {
        DosClose(in_r);
        DosClose(out_w);
        if let Some((_, w)) = err {
            DosClose(w);
        }
    }
    if rc != 0 {
        unsafe {
            DosClose(in_w);
            DosClose(out_r);
            if let Some((r, _)) = err {
                DosClose(r);
            }
        }
        return None;
    }
    Some(Child { pid: res.code_terminate, stdin: in_w, stdout: out_r, stderr: err.map(|e| e.0) })
}

struct Copy {
    from_pipe: u32,
    to_sock: u32,
}

/// A pipe to a socket until the pipe ends.
unsafe extern "C" fn pipe_to_sock(p: u32) {
    let c = unsafe { alloc::boxed::Box::from_raw(p as *mut Copy) };
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        if unsafe { DosRead(c.from_pipe, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        let mut sent = 0;
        if unsafe { Rs2SockSend(c.to_sock, buf.as_ptr(), n, 0, &mut sent) } != 0 {
            break;
        }
    }
    unsafe { DosClose(c.from_pipe) };
}

struct Feed {
    sock: u32,
    pipe: u32,
}

/// The client's data to the command's standard input until the client
/// shuts its side.
unsafe extern "C" fn sock_to_pipe(p: u32) {
    let f = unsafe { alloc::boxed::Box::from_raw(p as *mut Feed) };
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        if unsafe { Rs2SockRecv(f.sock, buf.as_mut_ptr(), buf.len() as u32, 0, &mut n) } != 0 || n == 0 {
            break;
        }
        let mut w = 0;
        if unsafe { DosWrite(f.pipe, buf.as_ptr(), n, &mut w) } != 0 {
            break;
        }
    }
    unsafe { DosClose(f.pipe) };
}

/// End the command when its time is up.
unsafe extern "C" fn watchdog(p: u32) {
    let (pid, secs) = *unsafe { alloc::boxed::Box::from_raw(p as *mut (u32, u32)) };
    unsafe {
        DosSleep(secs * 1000);
        DosKillProcess(0, pid);
    }
}

/// Run the command for the client: standard input from `s`, output to
/// `s`, standard error to `err` (or `s`). Returns when the command's
/// output ends; the command is ended after `timeout` seconds (0: never).
pub fn run(s: &Sock, err: Option<&Sock>, cmd: &str, extra_env: &[String], timeout: u32) -> bool {
    let Some(child) = spawn(cmd, err.is_some(), extra_env) else { return false };
    let _ = s.send(&[0]);
    let feed = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Feed { sock: s.0, pipe: child.stdin }));
    if thread(sock_to_pipe, feed as u32).is_err() {
        unsafe { DosClose(child.stdin) };
    }
    if let (Some(e), Some(h)) = (err, child.stderr) {
        let c = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Copy { from_pipe: h, to_sock: e.0 }));
        let _ = thread(pipe_to_sock, c as u32);
    }
    if timeout != 0 {
        let w = alloc::boxed::Box::into_raw(alloc::boxed::Box::new((child.pid, timeout)));
        let _ = thread(watchdog, w as u32);
    }
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0;
        if unsafe { DosRead(child.stdout, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        if s.send(&buf[..n as usize]).is_err() {
            unsafe { DosKillProcess(0, child.pid) };
            break;
        }
    }
    unsafe { DosClose(child.stdout) };
    true
}
