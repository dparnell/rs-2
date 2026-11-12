//! The QEMU guest agent's protocol (QGA): JSON commands from the host on
//! the virtio-serial port `org.qemu.guest_agent.0`, one JSON reply each
//! (`{"return": ...}` or `{"error": {"class", "desc"}}`), as QEMU's
//! qemu-ga answers them. Proxmox VE, libvirt and `virsh qemu-agent-command`
//! use it: to shut the guest down cleanly, show its addresses, freeze its
//! file systems around a backup, read and write files.
//!
//! A 0xFF byte from the host resets the reader (the host sends it before
//! `guest-sync-delimited`, whose reply starts with 0xFF too).
//! `guest-shutdown` gives no reply when it works, as qemu-ga.
//!
//! The OS's part is the [`Guest`] trait (VMAGENT's QEMUGA.EXE implements
//! it); [`Agent::feed`] takes bytes from the port and returns the bytes to
//! write back.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::json::{self, obj, s, Value};

pub const VERSION: &str = "rs2-1.0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shutdown {
    PowerDown,
    Halt,
    Reboot,
}

#[derive(Clone, Debug, Default)]
pub struct Iface {
    pub name: String,
    pub mac: Option<[u8; 6]>,
    /// IPv4 addresses and prefix lengths.
    pub ipv4: Vec<([u8; 4], u8)>,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub rx_dropped: u64,
}

/// What `guest-exec` captures (`capture-output`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    None,
    Stdout,
    Stderr,
    /// Standard output and error apart.
    Separated,
    /// Standard error with standard output.
    Merged,
}

/// A program to start (`guest-exec`).
#[derive(Clone, Debug)]
pub struct Exec {
    pub path: String,
    pub args: Vec<String>,
    /// `NAME=value` strings added to (or replacing) the agent's own.
    pub env: Vec<String>,
    /// Standard input (None: empty).
    pub input: Option<Vec<u8>>,
    pub capture: Capture,
}

/// A started program's state (`guest-exec-status`).
#[derive(Clone, Debug, Default)]
pub struct ExecStatus {
    pub exited: bool,
    pub exitcode: Option<i64>,
    /// How it ended when it did not exit (a POSIX signal number).
    pub signal: Option<i64>,
    /// What it wrote, and whether more was dropped (once exited).
    pub out: Option<(Vec<u8>, bool)>,
    pub err: Option<(Vec<u8>, bool)>,
}

pub struct OsInfo {
    pub version: String,
    pub machine: &'static str,
}

/// What the agent asks of the system.
pub trait Guest {
    fn hostname(&mut self) -> String;
    fn interfaces(&mut self) -> Vec<Iface>;
    fn os_info(&mut self) -> OsInfo;
    /// Nanoseconds since 1970 (UTC).
    fn time_ns(&mut self) -> i64;
    /// Set the clock (None: from the hardware clock).
    fn set_time_ns(&mut self, ns: Option<i64>) -> Result<(), String>;
    /// Seconds east of UTC and the zone's name, if known.
    fn timezone(&mut self) -> (i64, Option<String>);
    fn cpus(&mut self) -> u32;
    /// Shut down (may not return).
    fn shutdown(&mut self, how: Shutdown) -> Result<(), String>;
    /// Write the file systems' caches out (for a "freeze"); how many.
    fn flush(&mut self) -> u32;
    fn file_open(&mut self, path: &str, mode: &str) -> Result<u32, String>;
    /// Up to `count` bytes; and whether the end was reached.
    fn file_read(&mut self, h: u32, count: usize) -> Result<(Vec<u8>, bool), String>;
    fn file_write(&mut self, h: u32, data: &[u8]) -> Result<usize, String>;
    /// New position and whether it is at the end; whence 0 set, 1 cur, 2 end.
    fn file_seek(&mut self, h: u32, offset: i64, whence: u32) -> Result<(u64, bool), String>;
    fn file_flush(&mut self, h: u32) -> Result<(), String>;
    fn file_close(&mut self, h: u32) -> Result<(), String>;
    /// Start a program; its process ID.
    fn exec(&mut self, e: &Exec) -> Result<u32, String>;
    /// A program's state; an exited one is forgotten once reported.
    fn exec_status(&mut self, pid: u32) -> Result<ExecStatus, String>;
}

/// Commands and whether they reply when they work.
const COMMANDS: &[(&str, bool)] = &[
    ("guest-sync-delimited", true),
    ("guest-sync", true),
    ("guest-ping", true),
    ("guest-info", true),
    ("guest-shutdown", false),
    ("guest-get-osinfo", true),
    ("guest-get-host-name", true),
    ("guest-network-get-interfaces", true),
    ("guest-get-time", true),
    ("guest-set-time", true),
    ("guest-get-timezone", true),
    ("guest-get-vcpus", true),
    ("guest-get-users", true),
    ("guest-fsfreeze-status", true),
    ("guest-fsfreeze-freeze", true),
    ("guest-fsfreeze-freeze-list", true),
    ("guest-fsfreeze-thaw", true),
    ("guest-fstrim", true),
    ("guest-file-open", true),
    ("guest-file-close", true),
    ("guest-file-read", true),
    ("guest-file-write", true),
    ("guest-file-seek", true),
    ("guest-file-flush", true),
    ("guest-exec", true),
    ("guest-exec-status", true),
];

/// Largest file read in one command (qemu-ga's limit is 48 MB; the port
/// is slow, so less).
const MAX_READ: usize = 4 * 1024 * 1024;
const MAX_INPUT: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub struct Agent {
    buf: Vec<u8>,
    frozen: bool,
}

fn err(class: &str, desc: &str) -> Value {
    obj([("error", obj([("class", s(class)), ("desc", s(desc))]))])
}

fn generic(desc: &str) -> Value {
    err("GenericError", desc)
}

fn ret(v: Value) -> Value {
    obj([("return", v)])
}

fn empty() -> Value {
    Value::Obj(Vec::new())
}

/// The length of the first complete JSON object or array in `b` (which
/// starts with `{` or `[`), if it is all there.
fn complete(b: &[u8]) -> Option<usize> {
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, &c) in b.iter().enumerate() {
        if in_str {
            match c {
                _ if esc => esc = false,
                b'\\' => esc = true,
                b'"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

impl Agent {
    pub fn new() -> Agent {
        Agent::default()
    }

    /// Bytes from the host; the bytes to send back.
    pub fn feed(&mut self, g: &mut dyn Guest, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &b in data {
            if b == 0xff {
                // The host resets the stream.
                self.buf.clear();
            } else {
                self.buf.push(b);
            }
        }
        if self.buf.len() > MAX_INPUT {
            self.buf.clear();
        }
        loop {
            let start = self.buf.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(self.buf.len());
            self.buf.drain(..start);
            let Some(&first) = self.buf.first() else { break };
            if first != b'{' && first != b'[' {
                // Not a command: up to the next line or object.
                let skip = self.buf.iter().position(|&c| c == b'{' || c == b'\n').unwrap_or(self.buf.len()).max(1);
                self.buf.drain(..skip);
                out.extend(line(&generic("Invalid JSON syntax")));
                continue;
            }
            let Some(n) = complete(&self.buf) else { break };
            let text: Vec<u8> = self.buf.drain(..n).collect();
            let reply = match json::parse(&text) {
                Some((v, _)) => self.command(g, &v),
                None => Some((false, generic("Invalid JSON syntax"))),
            };
            if let Some((delimited, v)) = reply {
                if delimited {
                    out.push(0xff);
                }
                out.extend(line(&v));
            }
        }
        out
    }

    /// A command's reply (and whether it is preceded by 0xFF); None for
    /// none (a shutdown).
    fn command(&mut self, g: &mut dyn Guest, v: &Value) -> Option<(bool, Value)> {
        let Some(name) = v.get("execute").and_then(Value::as_str) else {
            return Some((false, err("GenericError", "Expected 'execute' in QMP input")));
        };
        let args = v.get("arguments").cloned().unwrap_or(Value::Null);
        let arg = |k: &str| args.get(k);
        let id = v.get("id").cloned();
        let with_id = |mut r: Value| {
            if let (Some(id), Value::Obj(m)) = (&id, &mut r) {
                m.push((String::from("id"), id.clone()));
            }
            r
        };
        let r = match name {
            "guest-sync-delimited" | "guest-sync" => match arg("id").and_then(Value::as_int) {
                Some(n) => return Some((name == "guest-sync-delimited", with_id(ret(Value::Int(n))))),
                None => generic("Parameter 'id' is missing"),
            },
            "guest-ping" => ret(empty()),
            "guest-info" => ret(obj([
                ("version", s(VERSION)),
                (
                    "supported_commands",
                    Value::Arr(
                        COMMANDS
                            .iter()
                            .map(|&(n, ok)| obj([("enabled", Value::Bool(true)), ("name", s(n)), ("success-response", Value::Bool(ok))]))
                            .collect(),
                    ),
                ),
            ])),
            "guest-shutdown" => {
                let how = match arg("mode").and_then(Value::as_str).unwrap_or("powerdown") {
                    "powerdown" => Shutdown::PowerDown,
                    "halt" => Shutdown::Halt,
                    "reboot" => Shutdown::Reboot,
                    m => return Some((false, with_id(generic(&alloc::format!("Invalid parameter 'mode': {m}"))))),
                };
                match g.shutdown(how) {
                    Ok(()) => return None,
                    Err(e) => generic(&e),
                }
            }
            "guest-get-osinfo" => {
                let o = g.os_info();
                ret(obj([
                    ("id", s("rs2")),
                    ("name", s("rs2")),
                    ("pretty-name", s(&alloc::format!("rs2 {} (OS/2 Warp 4.52 compatible)", o.version))),
                    ("version", s(&o.version)),
                    ("version-id", s(&o.version)),
                    ("kernel-release", s(&o.version)),
                    ("kernel-version", s("rs2")),
                    ("machine", s(o.machine)),
                ]))
            }
            "guest-get-host-name" => ret(obj([("host-name", Value::Str(g.hostname()))])),
            "guest-network-get-interfaces" => ret(Value::Arr(g.interfaces().iter().map(iface).collect())),
            "guest-get-time" => ret(Value::Int(g.time_ns())),
            "guest-set-time" => match g.set_time_ns(arg("time").and_then(Value::as_int)) {
                Ok(()) => ret(empty()),
                Err(e) => generic(&e),
            },
            "guest-get-timezone" => {
                let (off, zone) = g.timezone();
                let mut m = vec![(String::from("offset"), Value::Int(off))];
                if let Some(z) = zone {
                    m.push((String::from("zone"), Value::Str(z)));
                }
                ret(Value::Obj(m))
            }
            "guest-get-vcpus" => ret(Value::Arr(
                (0..g.cpus()).map(|i| obj([("logical-id", Value::Int(i as i64)), ("online", Value::Bool(true)), ("can-offline", Value::Bool(false))])).collect(),
            )),
            "guest-get-users" => ret(Value::Arr(Vec::new())),
            "guest-fsfreeze-status" => ret(s(if self.frozen { "frozen" } else { "thawed" })),
            "guest-fsfreeze-freeze" | "guest-fsfreeze-freeze-list" => {
                if self.frozen {
                    generic("The command guest-fsfreeze-freeze has been disabled for this instance")
                } else {
                    self.frozen = true;
                    ret(Value::Int(g.flush() as i64))
                }
            }
            "guest-fsfreeze-thaw" => {
                let n = if self.frozen { g.flush() } else { 0 };
                self.frozen = false;
                ret(Value::Int(n as i64))
            }
            "guest-fstrim" => ret(obj([("paths", Value::Arr(Vec::new()))])),
            "guest-file-open" => match arg("path").and_then(Value::as_str) {
                Some(p) => match g.file_open(p, arg("mode").and_then(Value::as_str).unwrap_or("r")) {
                    Ok(h) => ret(Value::Int(h as i64)),
                    Err(e) => generic(&e),
                },
                None => generic("Parameter 'path' is missing"),
            },
            "guest-file-close" | "guest-file-flush" | "guest-file-read" | "guest-file-write" | "guest-file-seek" => {
                match arg("handle").and_then(Value::as_int) {
                    Some(h) => self.file(g, name, h as u32, &args),
                    None => generic("Parameter 'handle' is missing"),
                }
            }
            "guest-exec" => match exec_request(&args) {
                Ok(e) => match g.exec(&e) {
                    Ok(pid) => ret(obj([("pid", Value::Int(pid as i64))])),
                    Err(e) => generic(&e),
                },
                Err(e) => generic(&e),
            },
            "guest-exec-status" => match arg("pid").and_then(Value::as_int) {
                Some(pid) => match g.exec_status(pid as u32) {
                    Ok(st) => ret(exec_status(&st)),
                    Err(e) => generic(&e),
                },
                None => generic("Parameter 'pid' is missing"),
            },
            _ => err("CommandNotFound", &alloc::format!("The command {name} has not been found")),
        };
        Some((false, with_id(r)))
    }

    fn file(&mut self, g: &mut dyn Guest, name: &str, h: u32, args: &Value) -> Value {
        let r = match name {
            "guest-file-close" => g.file_close(h).map(|_| empty()),
            "guest-file-flush" => g.file_flush(h).map(|_| empty()),
            "guest-file-read" => {
                let count = args.get("count").and_then(Value::as_int).unwrap_or(4096).clamp(0, MAX_READ as i64) as usize;
                g.file_read(h, count).map(|(d, eof)| {
                    obj([("count", Value::Int(d.len() as i64)), ("buf-b64", Value::Str(base64(&d))), ("eof", Value::Bool(eof))])
                })
            }
            "guest-file-write" => match args.get("buf-b64").and_then(Value::as_str).and_then(unbase64) {
                Some(d) => {
                    let n = args.get("count").and_then(Value::as_int).map_or(d.len(), |c| (c.max(0) as usize).min(d.len()));
                    g.file_write(h, &d[..n]).map(|w| obj([("count", Value::Int(w as i64)), ("eof", Value::Bool(false))]))
                }
                None => Err(String::from("Invalid parameter 'buf-b64'")),
            },
            _ => {
                let offset = args.get("offset").and_then(Value::as_int).unwrap_or(0);
                // "set"/"cur"/"end" or 0/1/2.
                let whence = match args.get("whence") {
                    Some(Value::Int(n)) => *n as u32,
                    Some(Value::Str(w)) => match w.as_str() {
                        "cur" => 1,
                        "end" => 2,
                        _ => 0,
                    },
                    _ => 0,
                };
                g.file_seek(h, offset, whence).map(|(pos, eof)| obj([("position", Value::Int(pos as i64)), ("eof", Value::Bool(eof))]))
            }
        };
        match r {
            Ok(v) => ret(v),
            Err(e) => generic(&e),
        }
    }
}

fn strings(v: Option<&Value>, what: &str) -> Result<Vec<String>, String> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Arr(a)) => a.iter().map(|x| x.as_str().map(String::from).ok_or_else(|| alloc::format!("Invalid parameter '{what}'"))).collect(),
        _ => Err(alloc::format!("Invalid parameter '{what}'")),
    }
}

/// `guest-exec`'s arguments.
fn exec_request(args: &Value) -> Result<Exec, String> {
    let path = args.get("path").and_then(Value::as_str).ok_or_else(|| String::from("Parameter 'path' is missing"))?;
    let input = match args.get("input-data") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().and_then(unbase64).ok_or_else(|| String::from("Invalid parameter 'input-data'"))?),
    };
    // A boolean (older hosts) or a mode.
    let capture = match args.get("capture-output") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Capture::None,
        Some(Value::Bool(true)) => Capture::Separated,
        Some(Value::Str(m)) => match m.as_str() {
            "none" => Capture::None,
            "stdout" => Capture::Stdout,
            "stderr" => Capture::Stderr,
            "separated" => Capture::Separated,
            "merged" => Capture::Merged,
            _ => return Err(alloc::format!("Invalid parameter 'capture-output': {m}")),
        },
        _ => return Err(String::from("Invalid parameter 'capture-output'")),
    };
    Ok(Exec { path: String::from(path), args: strings(args.get("arg"), "arg")?, env: strings(args.get("env"), "env")?, input, capture })
}

fn exec_status(st: &ExecStatus) -> Value {
    let mut m = vec![(String::from("exited"), Value::Bool(st.exited))];
    if let Some(c) = st.exitcode {
        m.push((String::from("exitcode"), Value::Int(c)));
    }
    if let Some(sig) = st.signal {
        m.push((String::from("signal"), Value::Int(sig)));
    }
    for (name, d) in [("out", &st.out), ("err", &st.err)] {
        if let Some((data, truncated)) = d {
            if !data.is_empty() {
                m.push((alloc::format!("{name}-data"), Value::Str(base64(data))));
            }
            if *truncated {
                m.push((alloc::format!("{name}-truncated"), Value::Bool(true)));
            }
        }
    }
    Value::Obj(m)
}

/// Joins a program's arguments into an OS/2 command line (after the
/// program's name): quoted where they hold spaces, tabs or quotes, with
/// the C runtime's rules for backslashes before a quote.
pub fn command_line(args: &[String]) -> String {
    let mut o = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            o.push(' ');
        }
        if !a.is_empty() && !a.contains([' ', '\t', '"']) {
            o.push_str(a);
            continue;
        }
        o.push('"');
        let mut slashes = 0;
        for c in a.chars() {
            match c {
                '\\' => slashes += 1,
                '"' => {
                    o.extend(core::iter::repeat_n('\\', slashes * 2 + 1));
                    slashes = 0;
                }
                _ => {
                    o.extend(core::iter::repeat_n('\\', slashes));
                    slashes = 0;
                }
            }
            if c != '\\' {
                o.push(c);
            }
        }
        o.extend(core::iter::repeat_n('\\', slashes * 2));
        o.push('"');
    }
    o
}

fn line(v: &Value) -> Vec<u8> {
    let mut t = v.to_string();
    t.push('\n');
    t.into_bytes()
}

fn iface(i: &Iface) -> Value {
    let mut m = vec![(String::from("name"), Value::Str(i.name.clone()))];
    if let Some(mac) = i.mac {
        let t = alloc::format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
        m.push((String::from("hardware-address"), Value::Str(t)));
    }
    let ips = i
        .ipv4
        .iter()
        .map(|(a, p)| {
            obj([
                ("ip-address-type", s("ipv4")),
                ("ip-address", Value::Str(alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]))),
                ("prefix", Value::Int(*p as i64)),
            ])
        })
        .collect();
    m.push((String::from("ip-addresses"), Value::Arr(ips)));
    let n = |v: u64| Value::Int(v as i64);
    m.push((
        String::from("statistics"),
        obj([
            ("rx-bytes", n(i.rx_bytes)),
            ("rx-packets", n(i.rx_packets)),
            ("rx-errs", n(0)),
            ("rx-dropped", n(i.rx_dropped)),
            ("tx-bytes", n(i.tx_bytes)),
            ("tx-packets", n(i.tx_packets)),
            ("tx-errs", n(0)),
            ("tx-dropped", n(0)),
        ]),
    ));
    Value::Obj(m)
}

/// The prefix length of a netmask.
pub fn prefix(mask: [u8; 4]) -> u8 {
    u32::from_be_bytes(mask).leading_ones() as u8
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(d: &[u8]) -> String {
    let mut o = String::with_capacity(d.len().div_ceil(3) * 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                o.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                o.push('=');
            }
        }
    }
    o
}

pub fn unbase64(t: &str) -> Option<Vec<u8>> {
    let mut o = Vec::with_capacity(t.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in t.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            o.push((acc >> bits) as u8);
        }
    }
    Some(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[derive(Default)]
    struct G {
        shut: Option<Shutdown>,
        flushes: u32,
        file: Vec<u8>,
        pos: usize,
        exec: Option<Exec>,
    }

    impl Guest for G {
        fn hostname(&mut self) -> String {
            "rs2".to_string()
        }
        fn interfaces(&mut self) -> Vec<Iface> {
            vec![Iface { name: "lan0".to_string(), mac: Some([0x52, 0x54, 0, 0x12, 0x34, 0x56]), ipv4: vec![([10, 0, 2, 15], 24)], ..Default::default() }]
        }
        fn os_info(&mut self) -> OsInfo {
            OsInfo { version: "0.1".to_string(), machine: "x86_64" }
        }
        fn time_ns(&mut self) -> i64 {
            1_700_000_000_000_000_000
        }
        fn set_time_ns(&mut self, _ns: Option<i64>) -> Result<(), String> {
            Ok(())
        }
        fn timezone(&mut self) -> (i64, Option<String>) {
            (3600, None)
        }
        fn cpus(&mut self) -> u32 {
            2
        }
        fn shutdown(&mut self, how: Shutdown) -> Result<(), String> {
            self.shut = Some(how);
            Ok(())
        }
        fn flush(&mut self) -> u32 {
            self.flushes += 1;
            1
        }
        fn file_open(&mut self, path: &str, _mode: &str) -> Result<u32, String> {
            if path == "C:\\CONFIG.SYS" { Ok(1000) } else { Err("The system cannot find the file specified.".to_string()) }
        }
        fn file_read(&mut self, _h: u32, count: usize) -> Result<(Vec<u8>, bool), String> {
            let end = (self.pos + count).min(self.file.len());
            let d = self.file[self.pos..end].to_vec();
            self.pos = end;
            Ok((d, end == self.file.len()))
        }
        fn file_write(&mut self, _h: u32, data: &[u8]) -> Result<usize, String> {
            self.file.extend_from_slice(data);
            Ok(data.len())
        }
        fn file_seek(&mut self, _h: u32, offset: i64, _whence: u32) -> Result<(u64, bool), String> {
            self.pos = offset as usize;
            Ok((self.pos as u64, self.pos >= self.file.len()))
        }
        fn file_flush(&mut self, _h: u32) -> Result<(), String> {
            Ok(())
        }
        fn file_close(&mut self, _h: u32) -> Result<(), String> {
            Ok(())
        }
        fn exec(&mut self, e: &Exec) -> Result<u32, String> {
            self.exec = Some(e.clone());
            Ok(42)
        }
        fn exec_status(&mut self, pid: u32) -> Result<ExecStatus, String> {
            match pid {
                42 => Ok(ExecStatus { exited: true, exitcode: Some(3), out: Some((b"hi\r\n".to_vec(), false)), err: Some((Vec::new(), true)), ..Default::default() }),
                43 => Ok(ExecStatus::default()),
                _ => Err("Invalid parameter 'pid'".to_string()),
            }
        }
    }

    fn talk(a: &mut Agent, g: &mut G, input: &str) -> String {
        String::from_utf8_lossy(&a.feed(g, input.as_bytes())).into_owned()
    }

    #[test]
    fn sync_and_errors() {
        let (mut a, mut g) = (Agent::new(), G::default());
        // In two pieces, the host's 0xFF reset first.
        let mut out = a.feed(&mut g, b"\xff{\"execute\":\"guest-sync-delimited\",");
        assert!(out.is_empty());
        out = a.feed(&mut g, b"\"arguments\":{\"id\":123456}}\n");
        assert_eq!(out, b"\xff{\"return\": 123456}\n");
        assert_eq!(talk(&mut a, &mut g, r#"{"execute":"guest-ping"} {"execute":"nope","id":"x"}"#), "{\"return\": {}}\n{\"error\": {\"class\": \"CommandNotFound\", \"desc\": \"The command nope has not been found\"}, \"id\": \"x\"}\n");
        assert!(talk(&mut a, &mut g, "garbage\n").contains("Invalid JSON"));
        assert!(talk(&mut a, &mut g, r#"{"execute":"guest-info"}"#).contains("\"name\": \"guest-shutdown\", \"success-response\": false"));
    }

    #[test]
    fn info_commands() {
        let (mut a, mut g) = (Agent::new(), G::default());
        let r = talk(&mut a, &mut g, r#"{"execute":"guest-network-get-interfaces"}"#);
        assert!(r.contains("\"hardware-address\": \"52:54:00:12:34:56\""), "{r}");
        assert!(r.contains("\"ip-address\": \"10.0.2.15\", \"prefix\": 24"), "{r}");
        assert!(talk(&mut a, &mut g, r#"{"execute":"guest-get-osinfo"}"#).contains("\"id\": \"rs2\""));
        assert_eq!(talk(&mut a, &mut g, r#"{"execute":"guest-get-host-name"}"#), "{\"return\": {\"host-name\": \"rs2\"}}\n");
        assert_eq!(prefix([255, 255, 240, 0]), 20);
    }

    #[test]
    fn shutdown_and_freeze() {
        let (mut a, mut g) = (Agent::new(), G::default());
        assert_eq!(talk(&mut a, &mut g, r#"{"execute":"guest-shutdown","arguments":{"mode":"reboot"}}"#), "");
        assert_eq!(g.shut, Some(Shutdown::Reboot));
        assert!(talk(&mut a, &mut g, r#"{"execute":"guest-shutdown","arguments":{"mode":"x"}}"#).contains("GenericError"));
        assert_eq!(talk(&mut a, &mut g, r#"{"execute":"guest-fsfreeze-freeze"}"#), "{\"return\": 1}\n");
        assert_eq!(talk(&mut a, &mut g, r#"{"execute":"guest-fsfreeze-status"}"#), "{\"return\": \"frozen\"}\n");
        assert_eq!(talk(&mut a, &mut g, r#"{"execute":"guest-fsfreeze-thaw"}"#), "{\"return\": 1}\n");
        assert_eq!(g.flushes, 2);
    }

    #[test]
    fn files() {
        let (mut a, mut g) = (Agent::new(), G::default());
        assert_eq!(talk(&mut a, &mut g, r#"{"execute":"guest-file-open","arguments":{"path":"C:\\CONFIG.SYS","mode":"w+"}}"#), "{\"return\": 1000}\n");
        let w = alloc::format!(r#"{{"execute":"guest-file-write","arguments":{{"handle":1000,"buf-b64":"{}"}}}}"#, base64(b"hello, world"));
        assert_eq!(talk(&mut a, &mut g, &w), "{\"return\": {\"count\": 12, \"eof\": false}}\n");
        talk(&mut a, &mut g, r#"{"execute":"guest-file-seek","arguments":{"handle":1000,"offset":7,"whence":"set"}}"#);
        assert_eq!(
            talk(&mut a, &mut g, r#"{"execute":"guest-file-read","arguments":{"handle":1000,"count":100}}"#),
            "{\"return\": {\"count\": 5, \"buf-b64\": \"d29ybGQ=\", \"eof\": true}}\n"
        );
        assert!(talk(&mut a, &mut g, r#"{"execute":"guest-file-open","arguments":{"path":"C:\\NO"}}"#).contains("cannot find"));
        assert_eq!(unbase64(&base64(b"ab")), Some(b"ab".to_vec()));
    }

    #[test]
    fn exec() {
        let (mut a, mut g) = (Agent::new(), G::default());
        let r = talk(&mut a, &mut g, r#"{"execute":"guest-exec","arguments":{"path":"cmd","arg":["/c","dir C:\\"],"env":["X=1"],"input-data":"aGk=","capture-output":true}}"#);
        assert_eq!(r, "{\"return\": {\"pid\": 42}}\n");
        let e = g.exec.take().unwrap();
        assert_eq!((e.path.as_str(), e.args.len(), e.env[0].as_str(), e.input.as_deref(), e.capture), ("cmd", 2, "X=1", Some(&b"hi"[..]), Capture::Separated));
        talk(&mut a, &mut g, r#"{"execute":"guest-exec","arguments":{"path":"x","capture-output":"merged"}}"#);
        assert_eq!(g.exec.take().unwrap().capture, Capture::Merged);
        assert!(talk(&mut a, &mut g, r#"{"execute":"guest-exec","arguments":{"path":"x","capture-output":"odd"}}"#).contains("GenericError"));
        assert!(talk(&mut a, &mut g, r#"{"execute":"guest-exec","arguments":{"arg":[]}}"#).contains("'path' is missing"));
        assert_eq!(
            talk(&mut a, &mut g, r#"{"execute":"guest-exec-status","arguments":{"pid":42}}"#),
            "{\"return\": {\"exited\": true, \"exitcode\": 3, \"out-data\": \"aGkNCg==\", \"err-truncated\": true}}\n"
        );
        assert_eq!(talk(&mut a, &mut g, r#"{"execute":"guest-exec-status","arguments":{"pid":43}}"#), "{\"return\": {\"exited\": false}}\n");
        assert!(talk(&mut a, &mut g, r#"{"execute":"guest-exec-status","arguments":{"pid":7}}"#).contains("'pid'"));
        let cl = |a: &[&str]| command_line(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(cl(&["/c", "dir C:\\"]), "/c \"dir C:\\\\\"");
        assert_eq!(cl(&["a b", "", "x\"y", "C:\\OS2"]), "\"a b\" \"\" \"x\\\"y\" C:\\OS2");
    }
}
