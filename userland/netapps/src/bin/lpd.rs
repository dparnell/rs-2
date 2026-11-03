//! LPD: the line printer daemon (RFC 1179), a print server for rs2's
//! spooler queues.
//!
//! `LPD [-b [banner]] [-c] [-f] [-s] [-cp codepage] [-p port]`, IBM's
//! options: jobs that LPR clients send to a queue name go into the PM
//! spooler queue of that name
//! through the queue manager calls (SplQmOpen, SplQmStartDoc, SplQmWrite,
//! SplQmEndDoc), as raw (PM_Q_RAW) jobs named after the job (the control
//! file's J line, else the file); the comment is `user@host`. Formatted
//! text (`f`, `p` lines, or every file with -f) gets CR LF line ends and,
//! with -cp, is translated from that code page to the process's. -b puts
//! a banner page in front of each job (host, user, job and class, or the
//! contents of the banner file given). -c is accepted (the control file
//! is never printed). -s takes requests only from reserved ports
//! (721-731). %ETC%\HOSTS.LPD, when it exists and lists anything, names
//! the hosts (names or addresses, one a line) that may print.
//!
//! The queue state commands list the queue's jobs (SplEnumJob); the
//! remove command deletes the jobs named by number, or the agent's own.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use netapps::*;
use os2::net::*;
use os2::*;
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "PMSPL.301"]
    fn SplQmOpen(tok: *const u8, n: i32, dop: *const u32) -> u32;
    #[link_name = "PMSPL.302"]
    fn SplQmStartDoc(h: u32, name: *const u8) -> u32;
    #[link_name = "PMSPL.304"]
    fn SplQmWrite(h: u32, n: i32, d: *const u8) -> u32;
    #[link_name = "PMSPL.303"]
    fn SplQmEndDoc(h: u32) -> u32;
    #[link_name = "PMSPL.306"]
    fn SplQmClose(h: u32) -> u32;
    #[link_name = "PMSPL.397"]
    fn SplQueryQueue(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, need: *mut u32) -> u32;
    #[link_name = "PMSPL.392"]
    fn SplEnumJob(comp: *const u8, q: *const u8, level: u32, buf: *mut u8, cb: u32, ret: *mut u32, total: *mut u32, need: *mut u32, r: u32) -> u32;
    #[link_name = "PMSPL.386"]
    fn SplDeleteJob(comp: *const u8, q: *const u8, id: u32) -> u32;
}

static BANNER: AtomicBool = AtomicBool::new(false);
static TEXT: AtomicBool = AtomicBool::new(false);
static SECURE: AtomicBool = AtomicBool::new(false);
static CP: AtomicU32 = AtomicU32::new(0);
static mut BANNER_FILE: Option<Vec<u8>> = None;

fn z(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().collect();
    v.push(0);
    v
}

fn pstr(p: u32) -> String {
    if p == 0 {
        return String::new();
    }
    let s = unsafe { core::ffi::CStr::from_ptr(p as *const core::ffi::c_char) };
    s.to_bytes().iter().map(|&b| b as char).collect()
}

/// The spooler queue of a queue name sent by a client.
fn find_queue(name: &str) -> Option<String> {
    let up = name.to_ascii_uppercase();
    let mut need = 0;
    let q = z(&up);
    let mut buf = alloc::vec![0u8; 4096];
    (unsafe { SplQueryQueue(core::ptr::null(), q.as_ptr(), 3, buf.as_mut_ptr(), buf.len() as u32, &mut need) } == 0).then_some(up)
}

/// Whether `peer` may print (HOSTS.LPD).
fn allowed(peer: [u8; 4]) -> bool {
    let Some(t) = read_file(&etc_path("HOSTS.LPD")) else { return true };
    let mut any = false;
    for l in t.split(|&b| b == b'\n') {
        let l: String = l.iter().map(|&b| b as char).collect();
        let l = l.split('#').next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        any = true;
        if l == "*" || l == ip(peer) || lookup(l).is_ok_and(|a| a == peer) {
            return true;
        }
    }
    !any
}

/// A line from the client (ending in LF).
fn line(s: &Sock) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match s.recv(&mut b) {
            Ok(1) => {
                if b[0] == b'\n' {
                    return Some(v);
                }
                v.push(b[0]);
                if v.len() > 4096 {
                    return None;
                }
            }
            _ => return if v.is_empty() { None } else { Some(v) },
        }
    }
}

fn text(v: &[u8]) -> String {
    v.iter().map(|&b| b as char).collect()
}

/// `n` bytes and the NUL after them.
fn read_exact(s: &Sock, n: usize) -> Option<Vec<u8>> {
    let mut v = Vec::with_capacity(n + 1);
    let mut buf = alloc::vec![0u8; 16384];
    while v.len() < n + 1 {
        let want = (n + 1 - v.len()).min(buf.len());
        match s.recv(&mut buf[..want]) {
            Ok(k) if k > 0 => v.extend_from_slice(&buf[..k]),
            _ => return None,
        }
    }
    v.truncate(n);
    Some(v)
}

struct Job {
    host: String,
    user: String,
    name: String,
    class: String,
    /// (data file name, format letter) in the order to print.
    prints: Vec<(String, u8)>,
}

fn parse_control(c: &[u8]) -> Job {
    let mut j = Job { host: String::new(), user: String::new(), name: String::new(), class: String::new(), prints: Vec::new() };
    let mut title = String::new();
    let mut n_name = String::new();
    for l in c.split(|&b| b == b'\n') {
        let Some((&k, rest)) = l.split_first() else { continue };
        let v = text(rest).trim_end_matches('\r').into();
        match k {
            b'H' => j.host = v,
            b'P' => j.user = v,
            b'J' => j.name = v,
            b'C' => j.class = v,
            b'T' => title = v,
            b'N' => n_name = v,
            b'f' | b'l' | b'o' | b'p' | b'r' | b'v' | b'c' | b'd' | b'g' | b'n' | b't' => j.prints.push((v, k)),
            _ => {}
        }
    }
    if j.name.is_empty() {
        j.name = if !title.is_empty() { title } else { n_name };
    }
    j
}

/// Text for the printer: CR LF line ends, translated from -cp.
fn as_text(d: &[u8]) -> Vec<u8> {
    let cp = CP.load(Ordering::Relaxed) as u16;
    let mut local = 0u32;
    let mut cpl = [0u32; 3];
    unsafe { DosQueryCp(12, cpl.as_mut_ptr(), &mut local) };
    let to = cpl[0] as u16;
    let map: Option<[u8; 256]> = (cp != 0 && cp != to).then(|| {
        let (a, b) = (uls::codepage(cp), uls::codepage(to));
        let mut m = [0u8; 256];
        for (i, v) in m.iter_mut().enumerate() {
            *v = i as u8;
            if i >= 0x80
                && let (Some(a), Some(b)) = (a, b)
            {
                *v = (0x80..256usize).find(|&j| b[j] == a[i]).map_or(b'?', |j| j as u8);
            }
        }
        m
    });
    let mut out = Vec::with_capacity(d.len() + d.len() / 32);
    let mut last = 0u8;
    for &c in d {
        let c = map.as_ref().map_or(c, |m| m[c as usize]);
        if c == b'\n' && last != b'\r' {
            out.push(b'\r');
        }
        out.push(c);
        last = c;
    }
    out
}

fn banner(j: &Job) -> Vec<u8> {
    #[allow(static_mut_refs)]
    if let Some(b) = unsafe { BANNER_FILE.as_ref() } {
        let mut v = b.clone();
        v.push(0x0c);
        return v;
    }
    let mut s = String::from("\r\n\r\n");
    s.push_str(&alloc::format!("    Host:  {}\r\n    User:  {}\r\n    Job:   {}\r\n    Class: {}\r\n", j.host, j.user, j.name, j.class));
    s.push('\x0c');
    s.into_bytes()
}

/// Spool one data file into `queue`; the job id.
fn spool(queue: &str, j: &Job, doc: &str, data: &[u8], fmt: u8, first: bool) -> Option<u32> {
    let qz = z(queue);
    let dt = b"PM_Q_RAW\0";
    let comment = z(&alloc::format!("{}@{}", j.user, j.host));
    let dop = [qz.as_ptr() as u32, 0, 0, dt.as_ptr() as u32, comment.as_ptr() as u32];
    let h = unsafe { SplQmOpen(b"*\0".as_ptr(), 5, dop.as_ptr()) };
    if h == 0 {
        return None;
    }
    let docz = z(doc);
    let mut body = Vec::new();
    if first && BANNER.load(Ordering::Relaxed) {
        body.extend_from_slice(&banner(j));
    }
    if matches!(fmt, b'f' | b'p') || TEXT.load(Ordering::Relaxed) && fmt != b'l' {
        body.extend_from_slice(&as_text(data));
    } else {
        body.extend_from_slice(data);
    }
    let ok = unsafe { SplQmStartDoc(h, docz.as_ptr()) } != 0 && body.chunks(32768).all(|c| unsafe { SplQmWrite(h, c.len() as i32, c.as_ptr()) } != 0);
    let id = if ok { unsafe { SplQmEndDoc(h) } } else { 0 };
    unsafe { SplQmClose(h) };
    (id != 0).then_some(id)
}

/// 02: receive a job.
fn receive(s: &Sock, queue: &str, peer: [u8; 4]) {
    let _ = s.send(&[0]);
    let mut control: Option<Vec<u8>> = None;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    while let Some(l) = line(s) {
        let Some((&k, rest)) = l.split_first() else { break };
        match k {
            // Abort the job.
            1 => {
                files.clear();
                control = None;
                let _ = s.send(&[0]);
            }
            2 | 3 => {
                let t = text(rest);
                let mut it = t.trim().splitn(2, ' ');
                let n: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let name = String::from(it.next().unwrap_or(""));
                if n > 64 << 20 {
                    let _ = s.send(&[1]);
                    return;
                }
                let _ = s.send(&[0]);
                let Some(d) = read_exact(s, n) else { return };
                let _ = s.send(&[0]);
                if k == 2 {
                    control = Some(d);
                } else {
                    files.push((name, d));
                }
            }
            _ => {
                let _ = s.send(&[1]);
            }
        }
    }
    let Some(c) = control else {
        outln(&alloc::format!("LPD: {}: job without a control file dropped", ip(peer)));
        return;
    };
    let mut j = parse_control(&c);
    if j.host.is_empty() {
        j.host = ip(peer);
    }
    if j.prints.is_empty() {
        j.prints = files.iter().map(|(n, _)| (n.clone(), b'l')).collect();
    }
    let mut first = true;
    for (fname, fmt) in &j.prints {
        let Some((_, data)) = files.iter().find(|(n, _)| n == fname) else { continue };
        let doc = if j.name.is_empty() { fname.clone() } else { j.name.clone() };
        match spool(queue, &j, &doc, data, *fmt, first) {
            Some(id) => outln(&alloc::format!("LPD: job {id} on {queue} from {}@{}: {} ({} bytes)", j.user, j.host, doc, data.len())),
            None => outln(&alloc::format!("LPD: cannot spool {} to {queue}", doc)),
        }
        first = false;
    }
}

/// (id, owner, document, size, status) of a queue's jobs.
fn jobs(queue: &str) -> Vec<(u32, String, String, u32, u16)> {
    let qz = z(queue);
    let mut buf = alloc::vec![0u8; 65536];
    let (mut ret, mut total, mut need) = (0, 0, 0);
    let mut v = Vec::new();
    if unsafe { SplEnumJob(core::ptr::null(), qz.as_ptr(), 2, buf.as_mut_ptr(), buf.len() as u32, &mut ret, &mut total, &mut need, 0) } != 0 {
        return v;
    }
    let rd16 = |o: usize| u16::from_le_bytes([buf[o], buf[o + 1]]);
    let rd32 = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    // PRJINFO2: id, priority, user, position, status, submitted, size,
    // comment, document.
    for i in 0..ret as usize {
        let o = i * 28;
        let comment = pstr(rd32(o + 20));
        let owner = comment.split('@').next().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| pstr(rd32(o + 4)));
        v.push((rd16(o) as u32, owner, pstr(rd32(o + 24)), rd32(o + 16), rd16(o + 10)));
    }
    v
}

/// 03 and 04: the queue's state.
fn state(s: &Sock, queue: &str, long: bool) {
    let js = jobs(queue);
    let mut o = alloc::format!("{}: {}\n", hostname(), queue);
    if js.is_empty() {
        o.push_str("no entries\n");
    } else if !long {
        o.push_str("Rank   Owner      Job  Files                 Total Size\n");
    }
    for (rank, (id, owner, doc, size, st)) in js.iter().enumerate() {
        let r = if st & 3 == 2 { String::from("active") } else { alloc::format!("{}", rank + 1) };
        if long {
            o.push_str(&alloc::format!("\n{}: {}  [job {:03}]\n        {}  {} bytes\n", owner, r, id, doc, size));
        } else {
            o.push_str(&alloc::format!("{:<6} {:<10} {:<4} {:<21} {} bytes\n", r, owner, id, doc, size));
        }
    }
    let _ = s.send(o.as_bytes());
}

/// 05: remove jobs.
fn remove(s: &Sock, queue: &str, agent: &str, which: &[&str]) {
    let qz = z(queue);
    let mut o = String::new();
    for (id, owner, doc, _, _) in jobs(queue) {
        let named = which.iter().any(|w| w.parse::<u32>().ok() == Some(id) || (w.parse::<u32>().is_err() && w.eq_ignore_ascii_case(&owner)));
        let mine = which.is_empty() && owner.eq_ignore_ascii_case(agent);
        let allowed = agent == "root" || owner.eq_ignore_ascii_case(agent) || which.iter().any(|w| w.parse::<u32>().ok() == Some(id));
        if (named || mine) && allowed && unsafe { SplDeleteJob(core::ptr::null(), qz.as_ptr(), id) } == 0 {
            o.push_str(&alloc::format!("job {id} ({doc}) dequeued\n"));
        }
    }
    let _ = s.send(o.as_bytes());
}

unsafe extern "C" fn serve(raw: u32) {
    let s = Sock(raw);
    let peer = s.peer();
    s.set_timeout(120_000);
    if !allowed(peer.addr) {
        outln(&alloc::format!("LPD: {} is not in HOSTS.LPD", ip(peer.addr)));
        let _ = s.send(b"\x01");
        return;
    }
    if SECURE.load(Ordering::Relaxed) && !(721..=731).contains(&peer.port()) {
        outln(&alloc::format!("LPD: {}: port {} is not reserved", ip(peer.addr), peer.port()));
        let _ = s.send(b"\x01");
        return;
    }
    let Some(l) = line(&s) else { return };
    let Some((&cmd, rest)) = l.split_first() else { return };
    let t = text(rest);
    let mut words = t.split_ascii_whitespace();
    let qname = words.next().unwrap_or("");
    let Some(queue) = find_queue(qname) else {
        outln(&alloc::format!("LPD: {}: invalid printer name {qname}", ip(peer.addr)));
        if cmd == 2 {
            let _ = s.send(&[1]);
        } else {
            let _ = s.send(alloc::format!("{qname}: invalid printer name\n").as_bytes());
        }
        return;
    };
    match cmd {
        // Print waiting jobs: the spooler does.
        1 => {}
        2 => receive(&s, &queue, peer.addr),
        3 | 4 => state(&s, &queue, cmd == 4),
        5 => {
            let agent = words.next().unwrap_or("");
            let rest: Vec<&str> = words.collect();
            remove(&s, &queue, agent, &rest);
        }
        _ => {}
    }
}

fn usage() -> u32 {
    eprint("Usage: lpd -? | {[-c] [-b [banner]] [-s] [-f] [-cp codepage] [-p port]}\r\n");
    eprint("  -c         the control file is not printed\r\n");
    eprint("  -b [file]  a banner page in front of each job (from the file, if given)\r\n");
    eprint("  -s         secure: only clients on reserved ports (721-731)\r\n");
    eprint("  -f         print every file as text\r\n");
    eprint("  -cp n      text arrives in code page n\r\n");
    1
}

fn main(st: &os2::Startup) -> u32 {
    let a = args(st);
    let mut port = service_port("printer", "tcp", 515);
    let mut i = 0;
    while i < a.len() {
        let x = a[i].to_ascii_lowercase();
        match x.as_str() {
            "-c" => {}
            "-s" => SECURE.store(true, Ordering::Relaxed),
            "-f" => TEXT.store(true, Ordering::Relaxed),
            "-b" => {
                BANNER.store(true, Ordering::Relaxed);
                if let Some(f) = a.get(i + 1).filter(|f| !f.starts_with('-')) {
                    match read_file(f) {
                        Some(d) => unsafe { BANNER_FILE = Some(d) },
                        None => {
                            eprint(&alloc::format!("LPD: cannot read {f}\r\n"));
                            return 1;
                        }
                    }
                    i += 1;
                }
            }
            "-p" => {
                i += 1;
                match a.get(i).and_then(|v| v.parse().ok()) {
                    Some(p) => port = p,
                    None => return usage(),
                }
            }
            _ if x.starts_with("-cp") => {
                let v = if x.len() > 3 {
                    String::from(&x[3..])
                } else {
                    i += 1;
                    a.get(i).cloned().unwrap_or_default()
                };
                match v.parse::<u16>() {
                    Ok(n) if uls::codepage(n).is_some() => CP.store(n as u32, Ordering::Relaxed),
                    _ => {
                        eprint(&alloc::format!("LPD: translation {v} is not available\r\n"));
                        return 1;
                    }
                }
            }
            _ => return usage(),
        }
        i += 1;
    }
    let l = match Sock::tcp() {
        Ok(l) => l,
        Err(_) => return 2,
    };
    l.reuse();
    if let Err(e) = l.bind([0; 4], port).and_then(|_| l.listen(5)) {
        eprint(&alloc::format!("LPD: port {port}: {}\r\n", strerror(e)));
        return 2;
    }
    outln(&alloc::format!("LPD: listening on port {port}"));
    loop {
        match l.accept() {
            Ok((c, _)) => {
                let raw = c.into_raw();
                if thread(serve, raw).is_err() {
                    drop(Sock(raw));
                }
            }
            Err(_) => unsafe {
                DosSleep(1000);
            },
        }
    }
}
