//! FTPD: the FTP server (RFC 959). `FTPD [-p port]` serves each client on
//! a thread of its own.
//!
//! Users are those of %ETC%\TRUSERS, as for IBM's FTPD:
//!
//! ```text
//! user: name password
//! rw: C:\HOME\NAME D:\SHARE
//! r: C:\PUB
//! ```
//!
//! A user's `rw:` directories may be read and written, `r:` ones only
//! read; a user with neither line may use the whole system. `anonymous`
//! (or `ftp`) is accepted with any password when TRUSERS lists it. Paths
//! are OS/2 paths (`C:/PUB/X` or `C:\PUB\X`; `/X` is on the current
//! drive), shown with forward slashes. Listings (LIST) are in the Unix
//! `ls -l` form clients parse; NLST gives the names.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use netapps::*;
use os2::net::*;
use os2::path::cstr;
os2::entry!(main);

struct User {
    name: String,
    password: String,
    rw: Vec<String>,
    r: Vec<String>,
}

fn trusers() -> Vec<User> {
    let mut v: Vec<User> = Vec::new();
    let Some(d) = read_file(&etc_path("TRUSERS")) else { return v };
    for l in utils::lines(&d) {
        let l = l.trim();
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        let Some((k, rest)) = l.split_once(':') else { continue };
        // A drive letter is not a key ("C:\..." on a line of its own).
        if k.len() == 1 && rest.starts_with('\\') {
            continue;
        }
        let mut w = rest.split_whitespace();
        match k.trim().to_ascii_lowercase().as_str() {
            "user" => v.push(User {
                name: String::from(w.next().unwrap_or("")),
                password: String::from(w.next().unwrap_or("")),
                rw: Vec::new(),
                r: Vec::new(),
            }),
            "rw" => {
                if let Some(u) = v.last_mut() {
                    u.rw.extend(w.map(norm_dir));
                }
            }
            "r" => {
                if let Some(u) = v.last_mut() {
                    u.r.extend(w.map(norm_dir));
                }
            }
            _ => {}
        }
    }
    v
}

fn norm_dir(s: &str) -> String {
    let mut d: String = s.chars().map(|c| if c == '/' { '\\' } else { c.to_ascii_uppercase() }).collect();
    while d.len() > 3 && d.ends_with('\\') {
        d.pop();
    }
    d
}

struct Session {
    ctrl: Sock,
    user: Option<User>,
    pending_user: Option<String>,
    cwd: String,
    binary: bool,
    port: Option<([u8; 4], u16)>,
    pasv: Option<Sock>,
    rnfr: Option<String>,
}

/// `C:\A\B` from a client path relative to `cwd` (forward or back
/// slashes, `.` and `..` resolved).
fn resolve_path(cwd: &str, p: &str) -> String {
    let p: String = p.chars().map(|c| if c == '/' { '\\' } else { c }).collect();
    let pb = p.as_bytes();
    let (drive, base, rest) = if pb.len() >= 2 && pb[1] == b':' {
        // On another drive: from its root.
        (String::from(&p[..2]), "", &p[2..])
    } else if p.starts_with('\\') {
        (String::from(&cwd[..2]), "", &p[..])
    } else {
        (String::from(&cwd[..2]), &cwd[2..], &p[..])
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in base.split('\\').chain(rest.split('\\')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            x => parts.push(x),
        }
    }
    let mut out = drive.to_ascii_uppercase();
    out.push('\\');
    out.push_str(&parts.join("\\"));
    out
}

fn shown(p: &str) -> String {
    p.chars().map(|c| if c == '\\' { '/' } else { c }).collect()
}

impl Session {
    fn reply(&self, s: &str) {
        let _ = self.ctrl.send_str(&alloc::format!("{s}\r\n"));
    }

    /// May the user read (or write) `path`?
    fn allowed(&self, path: &str, write: bool) -> bool {
        let Some(u) = &self.user else { return false };
        if u.rw.is_empty() && u.r.is_empty() {
            return !(write && is_anonymous(&u.name));
        }
        let up = path.to_ascii_uppercase();
        let under = |d: &String| up == *d || up.starts_with(&alloc::format!("{}\\", d.trim_end_matches('\\')));
        if u.rw.iter().any(under) {
            return true;
        }
        !write && u.r.iter().any(under)
    }

    fn data_conn(&mut self) -> Option<Sock> {
        if let Some(l) = self.pasv.take() {
            if !l.readable(60000) {
                return None;
            }
            return l.accept().ok().map(|(s, _)| s);
        }
        let (a, p) = self.port.take()?;
        Sock::connect_addr(a, p).ok()
    }

    fn send_data(&mut self, data: &[u8], what: &str) {
        self.reply(&alloc::format!("150 Opening {} mode data connection for {what} ({} bytes).", if self.binary { "BINARY" } else { "ASCII" }, data.len()));
        let Some(d) = self.data_conn() else {
            self.reply("425 Can't open data connection.");
            return;
        };
        let ok = data.chunks(16384).all(|c| d.send(c).is_ok());
        drop(d);
        if ok {
            self.reply("226 Transfer complete.");
        } else {
            self.reply("426 Connection closed; transfer aborted.");
        }
    }

    fn list(&mut self, arg: &str, long: bool) {
        let arg = arg.split_whitespace().filter(|w| !w.starts_with('-')).next().unwrap_or("");
        let path = resolve_path(&self.cwd, if arg.is_empty() { "." } else { arg });
        if !self.allowed(&path, false) {
            self.reply("550 Permission denied.");
            return;
        }
        let spec = if is_dir(&path) { alloc::format!("{}\\*", path.trim_end_matches('\\')) } else { path.clone() };
        let mut entries = list_dir(&spec);
        entries.sort_by(|a, b| a.name.to_ascii_uppercase().cmp(&b.name.to_ascii_uppercase()));
        let mut text = String::new();
        for e in &entries {
            if long {
                let t = &e.time;
                let mode = if e.attr & 0x10 != 0 { "drwxr-xr-x" } else if e.attr & 1 != 0 { "-r--r--r--" } else { "-rw-r--r--" };
                text.push_str(&alloc::format!(
                    "{mode}   1 owner    group    {:>10} {} {:>2} {:04} {}\r\n",
                    e.size,
                    MONTHS[(t.month.clamp(1, 12) - 1) as usize],
                    t.day,
                    t.year,
                    e.name
                ));
            } else {
                text.push_str(&e.name);
                text.push_str("\r\n");
            }
        }
        let bytes: Vec<u8> = text.chars().map(|c| c as u8).collect();
        self.reply("150 Opening ASCII mode data connection for file list.");
        let Some(d) = self.data_conn() else {
            self.reply("425 Can't open data connection.");
            return;
        };
        let _ = d.send(&bytes);
        drop(d);
        self.reply("226 Transfer complete.");
    }

    fn retr(&mut self, arg: &str) {
        let path = resolve_path(&self.cwd, arg);
        if !self.allowed(&path, false) {
            self.reply("550 Permission denied.");
            return;
        }
        let Some(data) = read_file(&path).filter(|_| !is_dir(&path)) else {
            self.reply(&alloc::format!("550 {arg}: No such file."));
            return;
        };
        let data = if self.binary {
            data
        } else {
            let mut o = Vec::new();
            let mut last = 0;
            to_crlf(&data, &mut o, &mut last);
            o
        };
        self.send_data(&data, arg);
    }

    fn stor(&mut self, arg: &str, append: bool) {
        let path = resolve_path(&self.cwd, arg);
        if !self.allowed(&path, true) {
            self.reply("550 Permission denied.");
            return;
        }
        let hf = match create_file(&path, append) {
            Ok(h) => h,
            Err(_) => {
                self.reply(&alloc::format!("553 {arg}: cannot create the file."));
                return;
            }
        };
        self.reply(&alloc::format!("150 Opening {} mode data connection for {arg}.", if self.binary { "BINARY" } else { "ASCII" }));
        let Some(d) = self.data_conn() else {
            unsafe { os2::DosClose(hf) };
            self.reply("425 Can't open data connection.");
            return;
        };
        let mut buf = alloc::vec![0u8; 16384];
        let mut ok = true;
        let mut last = 0u8;
        loop {
            match d.recv(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let w = if self.binary {
                        write_file(hf, &buf[..n])
                    } else {
                        let mut o = Vec::new();
                        to_crlf(&buf[..n], &mut o, &mut last);
                        write_file(hf, &o)
                    };
                    if w.is_err() {
                        ok = false;
                        break;
                    }
                }
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        unsafe { os2::DosClose(hf) };
        if ok {
            self.reply("226 Transfer complete.");
        } else {
            self.reply("451 Transfer aborted: local error.");
        }
    }

    fn command(&mut self, line: &str) -> bool {
        let (verb, arg) = match line.split_once(' ') {
            Some((v, a)) => (v.to_ascii_uppercase(), a.trim()),
            None => (line.trim().to_ascii_uppercase(), ""),
        };
        let logged = self.user.is_some();
        let needs_login = !matches!(verb.as_str(), "USER" | "PASS" | "QUIT" | "NOOP" | "SYST" | "HELP" | "FEAT");
        if needs_login && !logged {
            self.reply("530 Please login with USER and PASS.");
            return true;
        }
        match verb.as_str() {
            "USER" => {
                self.user = None;
                self.pending_user = Some(String::from(arg));
                if is_anonymous(arg) {
                    self.reply("331 Guest login ok, send your e-mail address as password.");
                } else {
                    self.reply(&alloc::format!("331 Password required for {arg}."));
                }
            }
            "PASS" => {
                let Some(name) = self.pending_user.take() else {
                    self.reply("503 Login with USER first.");
                    return true;
                };
                let users = trusers();
                let found = users.into_iter().find(|u| {
                    if is_anonymous(&name) {
                        is_anonymous(&u.name)
                    } else {
                        u.name.eq_ignore_ascii_case(&name) && u.password == arg
                    }
                });
                match found {
                    Some(u) => {
                        if let Some(d) = u.rw.first().or(u.r.first()) {
                            self.cwd = d.clone();
                        }
                        self.reply(&alloc::format!("230 User {} logged in.", u.name));
                        self.user = Some(u);
                    }
                    None => self.reply("530 Login incorrect."),
                }
            }
            "QUIT" => {
                self.reply("221 Goodbye.");
                return false;
            }
            "NOOP" => self.reply("200 NOOP command successful."),
            "SYST" => self.reply("215 OS/2 operating system"),
            "FEAT" => {
                self.reply("211-Extensions supported:");
                self.reply(" SIZE");
                self.reply(" MDTM");
                self.reply("211 End");
            }
            "HELP" => {
                self.reply("214-The following commands are recognized:");
                self.reply("   USER PASS QUIT NOOP SYST TYPE MODE STRU PORT PASV LIST NLST");
                self.reply("   RETR STOR APPE DELE RNFR RNTO CWD  CDUP PWD  MKD  RMD  SIZE");
                self.reply("   MDTM XCWD XPWD XMKD XRMD XCUP FEAT HELP ALLO");
                self.reply("214 Direct comments to the system administrator.");
            }
            "TYPE" => {
                let t = arg.to_ascii_uppercase();
                if t.starts_with('I') || t.starts_with('L') {
                    self.binary = true;
                    self.reply("200 Type set to I.");
                } else if t.starts_with('A') {
                    self.binary = false;
                    self.reply("200 Type set to A.");
                } else {
                    self.reply("504 Type not implemented.");
                }
            }
            "MODE" => self.reply(if arg.eq_ignore_ascii_case("S") { "200 Mode S ok." } else { "504 Only stream mode." }),
            "STRU" => self.reply(if arg.eq_ignore_ascii_case("F") { "200 Structure F ok." } else { "504 Only file structure." }),
            "ALLO" => self.reply("202 ALLO command ignored."),
            "PORT" => {
                let n: Vec<u32> = arg.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if n.len() != 6 || n.iter().any(|&x| x > 255) {
                    self.reply("501 Invalid PORT command.");
                } else {
                    self.pasv = None;
                    self.port = Some(([n[0] as u8, n[1] as u8, n[2] as u8, n[3] as u8], (n[4] << 8 | n[5]) as u16));
                    self.reply("200 PORT command successful.");
                }
            }
            "PASV" => {
                let a = self.ctrl.local().addr;
                let Ok(l) = Sock::tcp() else {
                    self.reply("425 Can't open passive connection.");
                    return true;
                };
                if l.bind(a, 0).is_err() || l.listen(1).is_err() {
                    self.reply("425 Can't open passive connection.");
                    return true;
                }
                let p = l.local().port();
                self.pasv = Some(l);
                self.port = None;
                self.reply(&alloc::format!("227 Entering Passive Mode ({},{},{},{},{},{})", a[0], a[1], a[2], a[3], p >> 8, p & 0xff));
            }
            "LIST" | "NLST" => self.list(arg, verb == "LIST"),
            "RETR" => self.retr(arg),
            "STOR" => self.stor(arg, false),
            "APPE" => self.stor(arg, true),
            "SIZE" | "MDTM" => {
                let path = resolve_path(&self.cwd, arg);
                let e = list_dir(&path);
                match e.first().filter(|e| e.attr & 0x10 == 0 && self.allowed(&path, false)) {
                    Some(e) if verb == "SIZE" => self.reply(&alloc::format!("213 {}", e.size)),
                    Some(e) => {
                        let t = &e.time;
                        self.reply(&alloc::format!("213 {:04}{:02}{:02}{:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second));
                    }
                    None => self.reply(&alloc::format!("550 {arg}: No such file.")),
                }
            }
            "DELE" => {
                let path = resolve_path(&self.cwd, arg);
                let c = cstr(&path);
                if !self.allowed(&path, true) {
                    self.reply("550 Permission denied.");
                } else if unsafe { os2::DosDelete(c.as_ptr()) } == 0 {
                    self.reply("250 DELE command successful.");
                } else {
                    self.reply(&alloc::format!("550 {arg}: cannot delete."));
                }
            }
            "RNFR" => {
                let path = resolve_path(&self.cwd, arg);
                if self.allowed(&path, true) && (file_size(&path).is_some() || is_dir(&path)) {
                    self.rnfr = Some(path);
                    self.reply("350 File exists, ready for destination name.");
                } else {
                    self.reply(&alloc::format!("550 {arg}: No such file or directory."));
                }
            }
            "RNTO" => {
                let Some(from) = self.rnfr.take() else {
                    self.reply("503 Bad sequence of commands.");
                    return true;
                };
                let to = resolve_path(&self.cwd, arg);
                let (a, b) = (cstr(&from), cstr(&to));
                if self.allowed(&to, true) && unsafe { os2::DosMove(a.as_ptr(), b.as_ptr()) } == 0 {
                    self.reply("250 RNTO command successful.");
                } else {
                    self.reply("550 Rename failed.");
                }
            }
            "CWD" | "XCWD" | "CDUP" | "XCUP" => {
                let target = if verb.ends_with("UP") { ".." } else { arg };
                let path = resolve_path(&self.cwd, target);
                if is_dir(&path) && self.allowed(&path, false) {
                    self.cwd = path;
                    self.reply("250 CWD command successful.");
                } else {
                    self.reply(&alloc::format!("550 {target}: No such directory."));
                }
            }
            "PWD" | "XPWD" => self.reply(&alloc::format!("257 \"{}\" is current directory.", shown(&self.cwd))),
            "MKD" | "XMKD" => {
                let path = resolve_path(&self.cwd, arg);
                let c = cstr(&path);
                if self.allowed(&path, true) && unsafe { os2::DosCreateDir(c.as_ptr(), core::ptr::null_mut()) } == 0 {
                    self.reply(&alloc::format!("257 \"{}\" directory created.", shown(&path)));
                } else {
                    self.reply(&alloc::format!("550 {arg}: cannot create the directory."));
                }
            }
            "RMD" | "XRMD" => {
                let path = resolve_path(&self.cwd, arg);
                let c = cstr(&path);
                if self.allowed(&path, true) && unsafe { os2::DosDeleteDir(c.as_ptr()) } == 0 {
                    self.reply("250 RMD command successful.");
                } else {
                    self.reply(&alloc::format!("550 {arg}: cannot remove the directory."));
                }
            }
            "" => {}
            _ => self.reply(&alloc::format!("500 '{verb}': command not understood.")),
        }
        true
    }
}

fn is_anonymous(n: &str) -> bool {
    n.eq_ignore_ascii_case("anonymous") || n.eq_ignore_ascii_case("ftp")
}

unsafe extern "C" fn serve(s: u32) {
    let ctrl = Sock(s);
    let peer = ctrl.peer();
    outln(&alloc::format!("FTPD: connection from {}", ip(peer.addr)));
    let mut sess = Session { ctrl, user: None, pending_user: None, cwd: current_dir(), binary: false, port: None, pasv: None, rnfr: None };
    sess.reply(&alloc::format!("220 {} FTP server (rs2 FTPD) ready.", hostname()));
    let mut rd = LineReader::new();
    while let Some(line) = rd.line(&sess.ctrl) {
        if !sess.command(&line) {
            break;
        }
    }
    outln(&alloc::format!("FTPD: {} disconnected", ip(peer.addr)));
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut port = service_port("ftp", "tcp", 21);
    let mut i = 0;
    while i < a.len() {
        let x = a[i].to_ascii_lowercase();
        if let Some(v) = x.strip_prefix("-p") {
            let v = if v.is_empty() {
                i += 1;
                a.get(i).cloned().unwrap_or_default()
            } else {
                String::from(v)
            };
            match v.parse() {
                Ok(p) => port = p,
                Err(_) => {
                    eprint("Usage: FTPD [-p<port>]\r\n");
                    return 1;
                }
            }
        } else if x != "-e" {
            eprint("Usage: FTPD [-p<port>]\r\n");
            return 1;
        }
        i += 1;
    }
    let l = match Sock::tcp() {
        Ok(l) => l,
        Err(e) => {
            eprint(&alloc::format!("FTPD: socket: {}\r\n", strerror(e)));
            return 2;
        }
    };
    l.reuse();
    if let Err(e) = l.bind([0; 4], port).and_then(|_| l.listen(5)) {
        eprint(&alloc::format!("FTPD: port {port}: {}\r\n", strerror(e)));
        return 2;
    }
    if read_file(&etc_path("TRUSERS")).is_none() {
        outln(&alloc::format!("FTPD: {} not found: nobody can log in", etc_path("TRUSERS")));
    }
    outln(&alloc::format!("FTPD: listening on port {port}"));
    loop {
        match l.accept() {
            Ok((c, _)) => {
                let raw = c.into_raw();
                if thread(serve, raw).is_err() {
                    drop(Sock(raw));
                }
            }
            Err(e) => {
                eprint(&alloc::format!("FTPD: accept: {}\r\n", strerror(e)));
                unsafe { os2::DosSleep(1000) };
            }
        }
    }
}
