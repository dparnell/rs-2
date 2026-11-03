//! FTP: the file transfer client (RFC 959).
//!
//! `FTP [-n] [-v] [-d] [-i] [-g] [-p] [host [port]]`, with the commands of
//! IBM's and BSD's ftp (`help` lists them). Data connections are active
//! (PORT: the server connects to a port FTP listens on), as IBM's FTP
//! makes them, or passive (PASV: FTP connects to the server) after
//! `passive` or with -p. Without -n FTP logs in at once, with the user
//! and password of the host's entry in %ETC%\NETRC when there is one
//! (`machine host login user password pw`), else asking for them.
//! Commands come from standard input, so `FTP -n host < script` runs a
//! script.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use netapps::*;
use os2::net::*;
os2::entry!(main);

enum Sink<'a> {
    /// To a file (handle), counting bytes.
    File(u32),
    /// To standard output (listings).
    Out,
    /// Collected (NLST for mget).
    Mem(&'a mut Vec<u8>),
}

struct Ftp {
    ctrl: Option<Sock>,
    rd: LineReader,
    host: String,
    verbose: bool,
    debug: bool,
    hash: bool,
    prompt: bool,
    glob: bool,
    passive: bool,
    binary: bool,
    sent_type: Option<bool>,
    /// -cp: ASCII transfers translated (inbound: network to local,
    /// outbound: local to network).
    xlate: Option<([u8; 256], [u8; 256])>,
    autologin: bool,
    code: u32,
    /// The last reply's text (all lines).
    text: String,
}

fn words(line: &str) -> Vec<String> {
    utils::split(line)
}

impl Ftp {
    fn connected(&self) -> bool {
        self.ctrl.is_some()
    }

    /// Read a reply (all its lines); returns its code (0 when the
    /// connection is gone).
    fn reply(&mut self) -> u32 {
        let Some(c) = &self.ctrl else { return 0 };
        self.text.clear();
        let Some(first) = self.rd.line(c) else {
            outln("421 Service not available, remote server has closed connection");
            self.ctrl = None;
            self.code = 421;
            return 421;
        };
        let code: u32 = first.get(..3).and_then(|s| s.parse().ok()).unwrap_or(0);
        let multi = first.as_bytes().get(3) == Some(&b'-');
        if self.verbose || code >= 400 {
            outln(&first);
        }
        self.text.push_str(&first);
        if multi {
            let end = alloc::format!("{code:03} ");
            while let Some(l) = self.rd.line(c) {
                if self.verbose || code >= 400 {
                    outln(&l);
                }
                self.text.push('\n');
                self.text.push_str(&l);
                if l.starts_with(&end) {
                    break;
                }
            }
        }
        self.code = code;
        code
    }

    /// Send a command and read its reply.
    fn cmd(&mut self, s: &str) -> u32 {
        let Some(c) = &self.ctrl else {
            outln("Not connected.");
            return 0;
        };
        if self.debug {
            if s.len() > 4 && s[..5].eq_ignore_ascii_case("PASS ") {
                outln("---> PASS XXXX");
            } else {
                outln(&alloc::format!("---> {s}"));
            }
        }
        if c.send_str(&alloc::format!("{s}\r\n")).is_err() {
            outln("Lost connection.");
            self.ctrl = None;
            return 0;
        }
        self.reply()
    }

    fn open(&mut self, host: &str, port: u16) {
        if self.connected() {
            outln(&alloc::format!("Already connected to {}, use close first.", self.host));
            return;
        }
        let (s, addr) = match Sock::connect(host, port) {
            Ok(v) => v,
            Err(e) => {
                outln(&alloc::format!("ftp: connect: {e}"));
                return;
            }
        };
        outln(&alloc::format!("Connected to {host} ({}).", ip(addr)));
        // A server that stops answering ends the session after two minutes.
        s.set_timeout(120_000);
        self.ctrl = Some(s);
        self.rd = LineReader::new();
        self.host = String::from(host);
        self.sent_type = None;
        let v = self.verbose;
        self.verbose = true;
        let code = self.reply();
        self.verbose = v;
        if code / 100 != 2 {
            self.ctrl = None;
            return;
        }
        if self.autologin {
            let (u, p) = netrc(host);
            self.login(u, p);
        }
    }

    fn login(&mut self, user: Option<String>, pass: Option<String>) {
        let user = match user {
            Some(u) => u,
            None => {
                let def = lpd_user();
                match read_line(&alloc::format!("Name ({}:{def}): ", self.host)) {
                    Some(u) if !u.trim().is_empty() => String::from(u.trim()),
                    Some(_) => def,
                    None => return,
                }
            }
        };
        let mut code = self.cmd(&alloc::format!("USER {user}"));
        if code == 331 {
            let p = match pass {
                Some(p) => p,
                None => read_password("Password: ").unwrap_or_default(),
            };
            code = self.cmd(&alloc::format!("PASS {p}"));
        }
        if code == 332 {
            let a = read_line("Account: ").unwrap_or_default();
            code = self.cmd(&alloc::format!("ACCT {a}"));
        }
        if code / 100 != 2 {
            outln("Login failed.");
        }
    }

    fn set_type(&mut self) -> bool {
        if self.sent_type == Some(self.binary) {
            return true;
        }
        let code = self.cmd(if self.binary { "TYPE I" } else { "TYPE A" });
        if code / 100 == 2 {
            self.sent_type = Some(self.binary);
            true
        } else {
            false
        }
    }

    /// Run a transfer command over a data connection. `put` is the data to
    /// send; otherwise what arrives goes to `sink`. Returns the bytes moved,
    /// or None if it failed.
    fn transfer(&mut self, command: &str, put: Option<&[u8]>, sink: Sink, typed: bool) -> Option<u64> {
        if !self.connected() {
            outln("Not connected.");
            return None;
        }
        if typed && !self.set_type() {
            return None;
        }
        let local = self.ctrl.as_ref().unwrap().local().addr;
        let mut listener = None;
        let mut data = None;
        if self.passive {
            if self.cmd("PASV") != 227 {
                outln("Passive mode refused.");
                return None;
            }
            let t = self.text.clone();
            let Some((a, p)) = parse_pasv(&t) else {
                outln("Cannot parse the PASV reply.");
                return None;
            };
            match Sock::connect_addr(a, p) {
                Ok(s) => data = Some(s),
                Err(e) => {
                    outln(&alloc::format!("ftp: data connection: {}", strerror(e)));
                    return None;
                }
            }
        } else {
            let l = Sock::tcp().ok()?;
            if l.bind(local, 0).is_err() || l.listen(1).is_err() {
                outln("ftp: cannot listen for the data connection");
                return None;
            }
            let p = l.local().port();
            let cmd = alloc::format!("PORT {},{},{},{},{},{}", local[0], local[1], local[2], local[3], p >> 8, p & 0xff);
            if self.cmd(&cmd) / 100 != 2 {
                return None;
            }
            listener = Some(l);
        }
        let code = self.cmd(command);
        if code / 100 != 1 {
            return None;
        }
        if let Some(l) = listener {
            if !l.readable(60000) {
                outln("ftp: the server did not open the data connection");
                return None;
            }
            match l.accept() {
                Ok((s, _)) => data = Some(s),
                Err(e) => {
                    outln(&alloc::format!("ftp: accept: {}", strerror(e)));
                    return None;
                }
            }
        }
        let d = data?;
        let start = ms();
        let mut total = 0u64;
        let mut marks = 0u64;
        let hash = self.hash;
        let mut tick = |total: u64| {
            if hash {
                while marks < total / 2048 {
                    out("#");
                    marks += 1;
                }
            }
        };
        if let Some(src) = put {
            let mut conv = Vec::new();
            let bytes: &[u8] = if self.binary {
                src
            } else {
                let mut last = 0;
                to_crlf(src, &mut conv, &mut last);
                // Ctrl+Z ends an OS/2 text file.
                if let Some(i) = conv.iter().position(|&b| b == 0x1a) {
                    conv.truncate(i);
                }
                if let Some((_, out)) = &self.xlate {
                    for b in conv.iter_mut() {
                        *b = out[*b as usize];
                    }
                }
                &conv
            };
            for chunk in bytes.chunks(8192) {
                if d.send(chunk).is_err() {
                    outln("ftp: data connection lost");
                    break;
                }
                total += chunk.len() as u64;
                tick(total);
            }
            d.shutdown_write();
        } else {
            let mut buf = alloc::vec![0u8; 16384];
            let mut sink = sink;
            let mut last = 0u8;
            loop {
                let n = match d.recv(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                total += n as u64;
                tick(total);
                let chunk = &buf[..n];
                let mut conv = Vec::new();
                let bytes: &[u8] = if self.binary && typed {
                    chunk
                } else {
                    to_crlf(chunk, &mut conv, &mut last);
                    if let Some((inb, _)) = &self.xlate {
                        for b in conv.iter_mut() {
                            *b = inb[*b as usize];
                        }
                    }
                    &conv
                };
                match &mut sink {
                    Sink::File(hf) => {
                        if write_file(*hf, bytes).is_err() {
                            outln("ftp: cannot write the local file");
                            break;
                        }
                    }
                    Sink::Out => os2::write(bytes),
                    Sink::Mem(v) => v.extend_from_slice(bytes),
                }
            }
        }
        drop(d);
        if hash && marks > 0 {
            outln("");
        }
        let code = self.reply();
        if code / 100 != 2 {
            return None;
        }
        if self.verbose && (put.is_some() || matches!(command.get(..4), Some("RETR"))) {
            outln(&rate(total, start));
        }
        Some(total)
    }

    fn get(&mut self, remote: &str, local: &str, append: bool) -> bool {
        if self.verbose {
            outln(&alloc::format!("local: {local} remote: {remote}"));
        }
        let hf = match create_file(local, append) {
            Ok(h) => h,
            Err(e) => {
                outln(&alloc::format!("ftp: {local}: cannot create (error {e})"));
                return false;
            }
        };
        let r = self.transfer(&alloc::format!("RETR {remote}"), None, Sink::File(hf), true);
        unsafe { os2::DosClose(hf) };
        if r.is_none() && !append {
            let c = os2::path::cstr(local);
            unsafe { os2::DosDelete(c.as_ptr()) };
        }
        r.is_some()
    }

    fn put(&mut self, local: &str, remote: &str, verb: &str) -> bool {
        let Some(data) = read_file(local) else {
            outln(&alloc::format!("ftp: {local}: cannot open"));
            return false;
        };
        if self.verbose {
            outln(&alloc::format!("local: {local} remote: {remote}"));
        }
        self.transfer(&alloc::format!("{verb} {remote}"), Some(&data), Sink::Out, true).is_some()
    }

    /// Names the server lists for a pattern (NLST).
    fn nlst(&mut self, pat: &str) -> Vec<String> {
        let mut v = Vec::new();
        let verbose = self.verbose;
        self.verbose = false;
        let cmd = if pat.is_empty() { String::from("NLST") } else { alloc::format!("NLST {pat}") };
        self.transfer(&cmd, None, Sink::Mem(&mut v), false);
        self.verbose = verbose;
        utils::lines(&v).into_iter().map(|l| String::from(l.trim())).filter(|l| !l.is_empty()).collect()
    }

    fn confirm(&self, what: &str, name: &str) -> bool {
        if !self.prompt || !stdin_is_console() {
            return true;
        }
        // Enter alone (or the end of input) means yes.
        !matches!(read_line(&alloc::format!("{what} {name}? ")).and_then(|a| a.chars().next()), Some('n' | 'N'))
    }

    fn command(&mut self, line: &str) -> bool {
        let w = words(line);
        let Some(c) = w.first() else { return true };
        let c = c.to_ascii_lowercase();
        let arg = |i: usize| w.get(i).cloned();
        let need = |s: &mut Ftp| {
            if !s.connected() {
                outln("Not connected.");
                false
            } else {
                true
            }
        };
        match c.as_str() {
            "open" | "o" => {
                let Some(h) = arg(1) else {
                    outln("usage: open host-name [port]");
                    return true;
                };
                let port = arg(2).map(|p| service_port(&p, "tcp", 21)).unwrap_or(21);
                self.open(&h, port);
            }
            "user" => {
                if need(self) {
                    match arg(1) {
                        Some(u) => self.login(Some(u), arg(2)),
                        None => outln("usage: user username [password] [account]"),
                    }
                }
            }
            "close" | "disconnect" => {
                if need(self) {
                    self.cmd("QUIT");
                    self.ctrl = None;
                }
            }
            "bye" | "quit" | "exit" => {
                if self.connected() {
                    self.cmd("QUIT");
                }
                return false;
            }
            "ascii" => {
                self.binary = false;
                outln("200 Type set to A.");
            }
            "binary" | "image" => {
                self.binary = true;
                outln("200 Type set to I.");
            }
            "type" => match arg(1).map(|t| t.to_ascii_lowercase()) {
                Some(t) if t == "binary" || t == "image" || t == "i" => self.binary = true,
                Some(t) if t == "ascii" || t == "a" => self.binary = false,
                _ => outln(&alloc::format!("Using {} mode to transfer files.", if self.binary { "binary" } else { "ascii" })),
            },
            "verbose" => {
                self.verbose = !self.verbose;
                outln(&alloc::format!("Verbose mode {}.", on(self.verbose)));
            }
            "debug" => {
                self.debug = !self.debug;
                outln(&alloc::format!("Debugging {}.", on(self.debug)));
            }
            "hash" => {
                self.hash = !self.hash;
                outln(&alloc::format!("Hash mark printing {} (2048 bytes/hash mark).", on(self.hash)));
            }
            "prompt" => {
                self.prompt = !self.prompt;
                outln(&alloc::format!("Interactive mode {}.", on(self.prompt)));
            }
            "glob" => {
                self.glob = !self.glob;
                outln(&alloc::format!("Globbing {}.", on(self.glob)));
            }
            "passive" | "pasv" => {
                self.passive = !self.passive;
                outln(&alloc::format!("Passive mode {}.", on(self.passive)));
            }
            "sendport" => {
                self.passive = !self.passive;
                outln(&alloc::format!("Use of PORT cmds {}.", on(!self.passive)));
            }
            "status" => {
                if self.connected() {
                    outln(&alloc::format!("Connected to {}.", self.host));
                } else {
                    outln("Not connected.");
                }
                outln(&alloc::format!("Mode: stream; Type: {}; Structure: file", if self.binary { "binary" } else { "ascii" }));
                outln(&alloc::format!(
                    "Verbose: {}; Bell: off; Prompting: {}; Globbing: {}; Hash: {}; Passive: {}",
                    on(self.verbose),
                    on(self.prompt),
                    on(self.glob),
                    on(self.hash),
                    on(self.passive)
                ));
            }
            "cd" | "cwd" => {
                if need(self) {
                    match arg(1) {
                        Some(d) => {
                            if d == ".." {
                                self.cmd("CDUP");
                            } else {
                                self.cmd(&alloc::format!("CWD {d}"));
                            }
                        }
                        None => outln("usage: cd remote-directory"),
                    }
                }
            }
            "cdup" => {
                if need(self) {
                    self.cmd("CDUP");
                }
            }
            "pwd" => {
                if need(self) {
                    self.cmd("PWD");
                }
            }
            "lcd" => {
                if let Some(d) = arg(1)
                    && let Err(e) = change_dir(&d)
                {
                    outln(&alloc::format!("lcd: {d}: error {e}"));
                }
                outln(&alloc::format!("Local directory now {}", current_dir()));
            }
            "mkdir" | "md" => {
                if need(self) {
                    match arg(1) {
                        Some(d) => {
                            self.cmd(&alloc::format!("MKD {d}"));
                        }
                        None => outln("usage: mkdir directory-name"),
                    }
                }
            }
            "rmdir" | "rd" => {
                if need(self) {
                    match arg(1) {
                        Some(d) => {
                            self.cmd(&alloc::format!("RMD {d}"));
                        }
                        None => outln("usage: rmdir directory-name"),
                    }
                }
            }
            "delete" | "del" => {
                if need(self) {
                    match arg(1) {
                        Some(f) => {
                            self.cmd(&alloc::format!("DELE {f}"));
                        }
                        None => outln("usage: delete remote-file"),
                    }
                }
            }
            "mdelete" | "mdel" => {
                if need(self) {
                    for p in w.iter().skip(1) {
                        for n in self.nlst(p) {
                            if self.confirm("mdelete", &n) {
                                self.cmd(&alloc::format!("DELE {n}"));
                            }
                        }
                    }
                }
            }
            "rename" | "ren" => {
                if need(self) {
                    match (arg(1), arg(2)) {
                        (Some(a), Some(b)) => {
                            if self.cmd(&alloc::format!("RNFR {a}")) == 350 {
                                self.cmd(&alloc::format!("RNTO {b}"));
                            }
                        }
                        _ => outln("usage: rename from-name to-name"),
                    }
                }
            }
            "ls" | "dir" | "nlist" | "mls" | "mdir" => {
                if need(self) {
                    let verb = if c == "dir" || c == "mdir" { "LIST" } else { "NLST" };
                    let cmd = match arg(1) {
                        Some(p) => alloc::format!("{verb} {p}"),
                        None => String::from(verb),
                    };
                    match arg(2) {
                        Some(file) => {
                            if let Ok(hf) = create_file(&file, false) {
                                self.transfer(&cmd, None, Sink::File(hf), false);
                                unsafe { os2::DosClose(hf) };
                            }
                        }
                        None => {
                            self.transfer(&cmd, None, Sink::Out, false);
                        }
                    }
                }
            }
            "get" | "recv" => {
                if need(self) {
                    match arg(1) {
                        Some(r) => {
                            let l = arg(2).unwrap_or_else(|| local_name(&r));
                            self.get(&r, &l, false);
                        }
                        None => outln("usage: get remote-file [ local-file ]"),
                    }
                }
            }
            "append" => {
                if need(self) {
                    match arg(1) {
                        Some(l) => {
                            let r = arg(2).unwrap_or_else(|| l.clone());
                            self.put(&l, &r, "APPE");
                        }
                        None => outln("usage: append local-file [ remote-file ]"),
                    }
                }
            }
            "put" | "send" => {
                if need(self) {
                    match arg(1) {
                        Some(l) => {
                            let r = arg(2).unwrap_or_else(|| String::from(l.rsplit(['\\', '/', ':']).next().unwrap_or(&l)));
                            self.put(&l, &r, "STOR");
                        }
                        None => outln("usage: put local-file [ remote-file ]"),
                    }
                }
            }
            "mget" => {
                if need(self) {
                    for p in w.iter().skip(1) {
                        let names = if self.glob { self.nlst(p) } else { alloc::vec![p.clone()] };
                        for n in names {
                            if self.confirm("mget", &n) {
                                let l = local_name(&n);
                                self.get(&n, &l, false);
                            }
                        }
                    }
                }
            }
            "mput" => {
                if need(self) {
                    for p in w.iter().skip(1) {
                        let dir = utils::dir_of(p);
                        let names: Vec<String> = if self.glob && (p.contains('*') || p.contains('?')) {
                            list_dir(p).into_iter().filter(|e| e.attr & 0x10 == 0).map(|e| alloc::format!("{dir}{}", e.name)).collect()
                        } else {
                            alloc::vec![p.clone()]
                        };
                        for n in names {
                            if self.confirm("mput", &n) {
                                let r = String::from(n.rsplit(['\\', '/', ':']).next().unwrap_or(&n));
                                self.put(&n, &r, "STOR");
                            }
                        }
                    }
                }
            }
            "size" => {
                if need(self) {
                    match arg(1) {
                        Some(f) => {
                            self.cmd(&alloc::format!("SIZE {f}"));
                        }
                        None => outln("usage: size remote-file"),
                    }
                }
            }
            "system" | "syst" => {
                if need(self) {
                    self.cmd("SYST");
                }
            }
            "quote" | "literal" => {
                if need(self) {
                    let rest = line.trim_start()[c.len()..].trim();
                    if rest.is_empty() {
                        outln("usage: quote line-to-send");
                    } else {
                        let v = self.verbose;
                        self.verbose = true;
                        self.cmd(rest);
                        self.verbose = v;
                    }
                }
            }
            "site" => {
                if need(self) {
                    let rest = line.trim_start()[c.len()..].trim();
                    let v = self.verbose;
                    self.verbose = true;
                    self.cmd(&alloc::format!("SITE {rest}"));
                    self.verbose = v;
                }
            }
            "rhelp" | "remotehelp" => {
                if need(self) {
                    let v = self.verbose;
                    self.verbose = true;
                    match arg(1) {
                        Some(t) => self.cmd(&alloc::format!("HELP {t}")),
                        None => self.cmd("HELP"),
                    };
                    self.verbose = v;
                }
            }
            "!" => shell(line.trim_start()[1..].trim()),
            "help" | "?" => help(arg(1)),
            _ => {
                if let Some(rest) = line.trim_start().strip_prefix('!') {
                    shell(rest.trim());
                } else {
                    outln("Invalid command.");
                }
            }
        }
        true
    }
}

fn on(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

const COMMANDS: &[(&str, &str)] = &[
    ("!", "run a local command"),
    ("append", "append a local file to a remote file"),
    ("ascii", "transfer files as text"),
    ("binary", "transfer files as they are"),
    ("bye", "end the session and FTP"),
    ("cd", "change the remote directory"),
    ("cdup", "go to the remote parent directory"),
    ("close", "end the session"),
    ("debug", "show the commands sent"),
    ("delete", "delete a remote file"),
    ("dir", "list a remote directory"),
    ("get", "get a file"),
    ("glob", "expand wildcards in mget and mput"),
    ("hash", "print # for each 2048 bytes"),
    ("lcd", "change the local directory"),
    ("ls", "list the names in a remote directory"),
    ("mdelete", "delete remote files"),
    ("mget", "get several files"),
    ("mkdir", "make a remote directory"),
    ("mput", "send several files"),
    ("open", "connect to a host"),
    ("passive", "use passive data connections (PASV)"),
    ("prompt", "ask for each file of mget, mput, mdelete"),
    ("put", "send a file"),
    ("pwd", "show the remote directory"),
    ("quit", "end the session and FTP"),
    ("quote", "send a command as it is"),
    ("rename", "rename a remote file"),
    ("rhelp", "the server's help"),
    ("rmdir", "remove a remote directory"),
    ("site", "send a SITE command"),
    ("size", "a remote file's size"),
    ("status", "show the settings"),
    ("system", "the server's system type"),
    ("type", "set the transfer type"),
    ("user", "log in as a user"),
    ("verbose", "show the server's replies"),
];

fn help(topic: Option<String>) {
    match topic {
        Some(t) => match COMMANDS.iter().find(|(c, _)| c.eq_ignore_ascii_case(&t)) {
            Some((c, d)) => outln(&alloc::format!("{c:<12}{d}")),
            None => outln(&alloc::format!("?Invalid help command {t}")),
        },
        None => {
            outln("Commands may be abbreviated.  Commands are:");
            outln("");
            for row in COMMANDS.chunks(5) {
                let mut l = String::new();
                for (c, _) in row {
                    l.push_str(&alloc::format!("{c:<15}"));
                }
                outln(l.trim_end());
            }
        }
    }
}

/// Run a command through %COMSPEC% /C.
fn shell(cmd: &str) {
    let comspec = env("COMSPEC").unwrap_or_else(|| String::from("C:\\OS2\\CMD.EXE"));
    let mut a: Vec<u8> = comspec.bytes().collect();
    a.push(0);
    if !cmd.is_empty() {
        a.extend_from_slice(b"/C ");
        a.extend_from_slice(cmd.as_bytes());
    }
    a.extend_from_slice(&[0, 0]);
    let mut prog: Vec<u8> = comspec.bytes().collect();
    prog.push(0);
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    unsafe { os2::DosExecPgm(fail.as_mut_ptr(), 64, 0, a.as_ptr(), core::ptr::null(), &mut res, prog.as_ptr()) };
}

/// A remote file name as a local one (its last part).
fn local_name(r: &str) -> String {
    String::from(r.rsplit(['/', '\\']).next().unwrap_or(r))
}

/// The address and port of a 227 reply: (h1,h2,h3,h4,p1,p2).
fn parse_pasv(t: &str) -> Option<([u8; 4], u16)> {
    let s = t.find('(').map(|i| &t[i + 1..]).or_else(|| t.get(4..))?;
    let nums: Vec<u32> = s.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).take(6).filter_map(|x| x.parse().ok()).collect();
    if nums.len() < 6 || nums.iter().any(|&n| n > 255) {
        return None;
    }
    Some(([nums[0] as u8, nums[1] as u8, nums[2] as u8, nums[3] as u8], (nums[4] << 8 | nums[5]) as u16))
}

/// The user and password %ETC%\NETRC gives for a host.
fn netrc(host: &str) -> (Option<String>, Option<String>) {
    let Some(d) = read_file(&etc_path("NETRC")) else { return (None, None) };
    let text: String = d.iter().map(|&b| b as char).collect();
    let toks: Vec<&str> = text.split_whitespace().collect();
    let (mut user, mut pass) = (None, None);
    let mut mine = false;
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            "machine" => {
                if mine && user.is_some() {
                    break;
                }
                mine = toks.get(i + 1).is_some_and(|h| h.eq_ignore_ascii_case(host));
                i += 1;
            }
            "default" => mine = user.is_none(),
            "login" if mine => {
                user = toks.get(i + 1).map(|s| s.to_string());
                i += 1;
            }
            "password" if mine => {
                pass = toks.get(i + 1).map(|s| s.to_string());
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    (user, pass)
}

fn lpd_user() -> String {
    netapps::lpd::user()
}

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let mut f = Ftp {
        ctrl: None,
        rd: LineReader::new(),
        host: String::new(),
        verbose: true,
        debug: false,
        hash: false,
        prompt: true,
        glob: true,
        passive: false,
        binary: false,
        sent_type: None,
        xlate: None,
        autologin: true,
        code: 0,
        text: String::new(),
    };
    let mut rest = Vec::new();
    let mut it = a.iter();
    while let Some(x) = it.next() {
        // -cp name: translate ASCII transfers to and from that code page.
        if rest.is_empty() && x.len() >= 3 && x[1..].to_ascii_lowercase().starts_with("cp") && (x.starts_with('-') || x.starts_with('/')) {
            let name = if x.len() > 3 { String::from(&x[3..]) } else { it.next().cloned().unwrap_or_default() };
            let mut cps = [0u32; 4];
            let mut n = 0;
            unsafe { os2::DosQueryCp(16, cps.as_mut_ptr(), &mut n) };
            let local = if cps[0] == 0 { 850 } else { cps[0] as u16 };
            match uls::codepage_by_name(&name).and_then(|net| Some((uls::translation(net, local, b'?')?, uls::translation(local, net, b'?')?))) {
                Some(t) => f.xlate = Some(t),
                None => {
                    eprint(&alloc::format!("ftp: translation {name} is not available\r\n"));
                    return 1;
                }
            }
            continue;
        }
        if x.len() >= 2 && (x.starts_with('-') || x.starts_with('/')) && rest.is_empty() {
            for ch in x[1..].chars() {
                match ch.to_ascii_lowercase() {
                    'n' => f.autologin = false,
                    'v' => f.verbose = true,
                    'd' => f.debug = true,
                    'i' => f.prompt = false,
                    'g' => f.glob = false,
                    'p' => f.passive = true,
                    _ => {
                        eprint("usage: ftp [-n] [-v] [-d] [-i] [-g] [-p] [-cp codepage] [host-name [port]]\r\n");
                        return 1;
                    }
                }
            }
        } else {
            rest.push(x.clone());
        }
    }
    if let Some(h) = rest.first() {
        let port = rest.get(1).map(|p| service_port(p, "tcp", 21)).unwrap_or(21);
        f.open(h, port);
    }
    while let Some(line) = read_line("ftp> ") {
        if !f.command(&line) {
            break;
        }
    }
    0
}
