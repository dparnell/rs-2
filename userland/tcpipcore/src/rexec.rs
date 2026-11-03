//! `rexec()`: run a command on a host's rexec server (TCPIP32 and
//! TCP32DLL export it).
//!
//! `int rexec(char **ahost, int rport, char *user, char *passwd, char
//! *cmd, int *fd2p)`: `*ahost` is looked up and replaced by the host's
//! official name; `rport` is the server's port in network byte order (as
//! getservbyname gives it). A NULL user or password is taken from the
//! host's entry in %ETC%\NETRC (`machine host login user password pw`).
//! With `fd2p`, a second connection for the command's standard error is
//! made (the server connects back to a port this call listens on) and
//! its socket stored there. Returns the socket of the connection, or -1
//! (the server's message, if it refused, goes to standard error).

use alloc::vec::Vec;

use os2::net::*;

use crate::errno::{fail, EFAULT};
use crate::{cstr, files, netdb};

fn eprint(s: &[u8]) {
    let mut w = 0;
    unsafe { os2::DosWrite(2, s.as_ptr(), s.len() as u32, &mut w) };
}

/// (login, password) of `host` in NETRC.
fn netrc(host: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let Some(t) = files::read_etc(b"NETRC") else { return (None, None) };
    let words: Vec<&[u8]> = t.split(|c| c.is_ascii_whitespace()).filter(|w| !w.is_empty()).collect();
    let (mut user, mut pass) = (None, None);
    let mut on = false;
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let next = words.get(i + 1).copied();
        if w.eq_ignore_ascii_case(b"machine") {
            if on {
                break;
            }
            on = next.is_some_and(|n| n.eq_ignore_ascii_case(host));
            i += 2;
        } else if w.eq_ignore_ascii_case(b"default") {
            if on {
                break;
            }
            on = true;
            i += 1;
        } else if on && w.eq_ignore_ascii_case(b"login") {
            user = next.map(|n| n.to_vec());
            i += 2;
        } else if on && w.eq_ignore_ascii_case(b"password") {
            pass = next.map(|n| n.to_vec());
            i += 2;
        } else {
            i += 1;
        }
    }
    (user, pass)
}

fn close(s: u32) {
    unsafe { Rs2SockClose(s) };
}

fn send_all(s: u32, b: &[u8]) -> Result<(), u32> {
    let mut at = 0;
    while at < b.len() {
        let mut n = 0;
        match unsafe { Rs2SockSend(s, b[at..].as_ptr(), (b.len() - at) as u32, 0, &mut n) } {
            0 => at += n as usize,
            e => return Err(e),
        }
    }
    Ok(())
}

pub unsafe fn rexec(ahost: *mut *mut u8, rport: i32, user: *const u8, pass: *const u8, cmd: *const u8, fd2p: *mut i32) -> i32 {
    if ahost.is_null() || unsafe { (*ahost).is_null() } || cmd.is_null() {
        return fail(EFAULT);
    }
    let he = unsafe { netdb::gethostbyname(*ahost) };
    if he.is_null() {
        let mut m = unsafe { cstr::bytes(*ahost) }.to_vec();
        m.extend_from_slice(b": unknown host\r\n");
        eprint(&m);
        return -1;
    }
    let (name, addr) = unsafe {
        let h = &*he;
        let a = *h.h_addr_list;
        (h.h_name, [*a, *a.add(1), *a.add(2), *a.add(3)])
    };
    unsafe { *ahost = name };
    let host = unsafe { cstr::bytes(name) }.to_vec();
    let (nu, np) = if user.is_null() || pass.is_null() { netrc(&host) } else { (None, None) };
    let user = if user.is_null() { nu.unwrap_or_default() } else { unsafe { cstr::bytes(user) }.to_vec() };
    let pass = if pass.is_null() { np.unwrap_or_default() } else { unsafe { cstr::bytes(pass) }.to_vec() };
    let mut s = 0;
    if let e @ 1.. = unsafe { Rs2SockCreate(AF_INET, SOCK_STREAM, 0, &mut s) } {
        return fail(e);
    }
    // rport is in network byte order already.
    let a = SockAddrIn { family: AF_INET as u16, port: rport as u16, addr, zero: [0; 8] };
    if let e @ 1.. = unsafe { Rs2SockConnect(s, &a, 16) } {
        let mut m = host.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(crate::errno::strerror(e as i32).as_bytes());
        m.extend_from_slice(b"\r\n");
        eprint(&m);
        close(s);
        return fail(e);
    }
    let mut req: Vec<u8> = Vec::new();
    let mut listener = 0;
    if fd2p.is_null() {
        req.push(0);
    } else {
        // A port for the server to connect back to (standard error).
        if let e @ 1.. = unsafe { Rs2SockCreate(AF_INET, SOCK_STREAM, 0, &mut listener) } {
            close(s);
            return fail(e);
        }
        let any = SockAddrIn::new([0; 4], 0);
        let mut me = SockAddrIn::default();
        if unsafe { Rs2SockBind(listener, &any, 16) } != 0 || unsafe { Rs2SockListen(listener, 1) } != 0 || unsafe { Rs2SockGetName(listener, 0, &mut me) } != 0 {
            close(listener);
            close(s);
            return -1;
        }
        req.extend_from_slice(alloc::format!("{}", me.port()).as_bytes());
        req.push(0);
    }
    for part in [&user[..], &pass[..], unsafe { cstr::bytes(cmd) }] {
        req.extend_from_slice(part);
        req.push(0);
    }
    if let Err(e) = send_all(s, &req) {
        close(s);
        if listener != 0 {
            close(listener);
        }
        return fail(e);
    }
    if listener != 0 {
        let mut peer = SockAddrIn::default();
        let mut ns = 0;
        let rc = unsafe { Rs2SockAccept(listener, &mut peer, &mut ns) };
        close(listener);
        if rc != 0 {
            close(s);
            return fail(rc);
        }
        unsafe { *fd2p = ns as i32 };
    }
    let mut b = [0u8; 1];
    let mut n = 0;
    if unsafe { Rs2SockRecv(s, b.as_mut_ptr(), 1, 0, &mut n) } != 0 || n != 1 {
        close(s);
        return -1;
    }
    if b[0] != 0 {
        // The server's message, to the end of its line.
        let mut m = Vec::new();
        let mut c = [0u8; 1];
        while unsafe { Rs2SockRecv(s, c.as_mut_ptr(), 1, 0, &mut n) } == 0 && n == 1 {
            m.push(c[0]);
            if c[0] == b'\n' {
                break;
            }
        }
        eprint(&m);
        close(s);
        if !fd2p.is_null() {
            close(unsafe { *fd2p } as u32);
        }
        return -1;
    }
    s as i32
}
