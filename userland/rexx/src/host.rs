//! The interpreter's host on OS/2: standard handles and the session I/O
//! exit, commands through registered subcommand handlers, the external
//! data queue, function packages and REXX programs on disk, the macro
//! space, the environment, files, messages from REX.MSG.

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{copy_nonoverlapping, null, null_mut};

use os2::*;
use rexx::{CmdResult, External, FileInfo, Host, Now, OpenMode, State};

use crate::RxString;
use crate::api::*;

/// Exit numbers (RXSYSEXIT codes).
pub const RXFNC: usize = 2;
pub const RXCMD: usize = 3;
pub const RXMSQ: usize = 4;
pub const RXSIO: usize = 5;
pub const RXHLT: usize = 7;
pub const RXTRC: usize = 8;
pub const RXINI: usize = 9;
pub const RXTER: usize = 10;

const RXEXIT_HANDLED: i32 = 0;
const RXEXIT_RAISE_ERROR: i32 = -1;

pub struct OsHost {
    /// Exit names by exit number (from RexxStart's exit list).
    pub exits: [Option<Vec<u8>>; 11],
    /// SETLOCAL saves: (environment block, drive, directory).
    pub locals: Vec<(Vec<u8>, u32, Vec<u8>)>,
    pub tid: u32,
}

/// The process's current REXX queue.
static mut QUEUE: Option<Vec<u8>> = None;

#[allow(static_mut_refs)]
pub fn queue_name() -> Vec<u8> {
    unsafe {
        if QUEUE.is_none() {
            let q = env_get(b"RXQUEUE").filter(|v| !v.is_empty()).unwrap_or_else(|| b"SESSION".to_vec());
            QUEUE = Some(q.to_ascii_uppercase());
        }
        QUEUE.clone().unwrap()
    }
}

fn set_queue_name(n: Vec<u8>) {
    unsafe { QUEUE = Some(n) };
}

fn cz(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn cstr(p: *const u8) -> &'static [u8] {
    if p.is_null() {
        return b"";
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

fn rx_bytes(r: &RxString) -> Vec<u8> {
    if r.ptr.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(r.ptr, r.len as usize).to_vec() } }
}

pub fn write_handle(h: u32, data: &[u8]) {
    let mut n = 0u32;
    unsafe { DosWrite(h, data.as_ptr(), data.len() as u32, &mut n) };
}

/// Read a line from a handle (CR LF or LF ends it); None at the end.
pub fn read_line(h: u32) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    let mut any = false;
    loop {
        let mut b = [0u8; 1];
        let mut n = 0u32;
        if unsafe { DosRead(h, b.as_mut_ptr(), 1, &mut n) } != 0 || n == 0 {
            return if any { Some(line) } else { None };
        }
        any = true;
        match b[0] {
            b'\n' => return Some(line),
            b'\r' => {}
            0x1a if line.is_empty() => return None,
            c => line.push(c),
        }
    }
}

// ---- the environment block ----

fn pib() -> *mut Pib {
    let mut tib: *mut Tib = null_mut();
    let mut pib: *mut Pib = null_mut();
    unsafe { DosGetInfoBlocks(&mut tib, &mut pib) };
    pib
}

/// The environment block of the process (var=value\0 ... \0).
pub fn env_block() -> Vec<u8> {
    let p = pib();
    if p.is_null() {
        return vec![0];
    }
    let e = unsafe { (*p).pchenv };
    if e.is_null() {
        return vec![0];
    }
    let mut n = 0;
    unsafe {
        while !(*e.add(n) == 0 && (n == 0 || *e.add(n - 1) == 0)) {
            n += 1;
        }
        // e[n] is the final zero
        core::slice::from_raw_parts(e, n + 1).to_vec()
    }
}

fn env_entries(b: &[u8]) -> Vec<&[u8]> {
    b.split(|&c| c == 0).filter(|e| !e.is_empty()).collect()
}

pub fn env_get(name: &[u8]) -> Option<Vec<u8>> {
    let b = env_block();
    for e in env_entries(&b) {
        if let Some(p) = e.iter().position(|&c| c == b'=') {
            if e[..p].eq_ignore_ascii_case(name) {
                return Some(e[p + 1..].to_vec());
            }
        }
    }
    None
}

/// Replace the process's environment strings (the program's path and
/// arguments after them are kept): in place when they fit, else in a new
/// block that the PIB then points to.
pub fn env_put(entries: &[Vec<u8>]) {
    let p = pib();
    if p.is_null() {
        return;
    }
    let old = env_block();
    let e = unsafe { (*p).pchenv } as *mut u8;
    // what follows the block: program path and arguments (as OS/2 lays it out)
    let tail_start = old.len();
    let mut tail_len = 0;
    unsafe {
        // path\0 then arguments "prog\0args\0\0" may follow
        let mut i = tail_start;
        let mut zeros = 0;
        while tail_len < 4096 {
            let c = *e.add(i);
            i += 1;
            tail_len += 1;
            if c == 0 {
                zeros += 1;
                if zeros == 4 {
                    break;
                }
            }
        }
    }
    let mut nb = Vec::new();
    for x in entries {
        nb.extend_from_slice(x);
        nb.push(0);
    }
    if nb.is_empty() {
        nb.push(0);
    }
    nb.push(0);
    let mut size = 0u32;
    let mut flags = 0u32;
    unsafe { DosQueryMem(e, &mut size, &mut flags) };
    if (nb.len() + tail_len) as u32 <= size {
        unsafe {
            let tail: Vec<u8> = core::slice::from_raw_parts(e.add(tail_start), tail_len).to_vec();
            copy_nonoverlapping(nb.as_ptr(), e, nb.len());
            copy_nonoverlapping(tail.as_ptr(), e.add(nb.len()), tail.len());
        }
    } else {
        let mut np: *mut u8 = null_mut();
        let total = (nb.len() + tail_len + 4096) as u32;
        if unsafe { DosAllocMem(&mut np, total, PAG_COMMIT | PAG_READ | PAG_WRITE | OBJ_TILE) } != 0 {
            return;
        }
        unsafe {
            copy_nonoverlapping(nb.as_ptr(), np, nb.len());
            copy_nonoverlapping(e.add(tail_start), np.add(nb.len()), tail_len);
            (*p).pchenv = np;
        }
    }
}

pub fn env_set(name: &[u8], val: Option<&[u8]>) {
    let b = env_block();
    let mut entries: Vec<Vec<u8>> = Vec::new();
    let mut done = false;
    for e in env_entries(&b) {
        let same = e.iter().position(|&c| c == b'=').is_some_and(|p| e[..p].eq_ignore_ascii_case(name));
        if same {
            if let Some(v) = val {
                if !done {
                    let mut x = e[..e.iter().position(|&c| c == b'=').unwrap()].to_vec();
                    x.push(b'=');
                    x.extend_from_slice(v);
                    entries.push(x);
                    done = true;
                }
            }
        } else {
            entries.push(e.to_vec());
        }
    }
    if !done {
        if let Some(v) = val {
            let mut x = name.to_ascii_uppercase();
            x.push(b'=');
            x.extend_from_slice(v);
            entries.push(x);
        }
    }
    env_put(&entries);
}

fn current_dir() -> Vec<u8> {
    let (mut disk, mut map) = (0u32, 0u32);
    unsafe { DosQueryCurrentDisk(&mut disk, &mut map) };
    let mut buf = [0u8; 260];
    let mut cb = buf.len() as u32;
    unsafe { DosQueryCurrentDir(0, buf.as_mut_ptr(), &mut cb) };
    let mut d = vec![b'@' + disk as u8, b':', b'\\'];
    d.extend_from_slice(cstr(buf.as_ptr()));
    d
}

// ---- messages ----

static FALLBACK: &str = include_str!("../msg/REX.TXT");

/// A message's text from REX.MSG (or the built-in copy), without its
/// prefix, %1/%2 removed.
pub fn message_text(num: u32) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 512];
    let mut n = 0u32;
    let empty: [*const u8; 2] = [b"\0".as_ptr(), b"\0".as_ptr()];
    let rc = unsafe { os2::msg::DosGetMessage(empty.as_ptr(), 2, buf.as_mut_ptr(), buf.len() as u32, num, b"REX.MSG\0".as_ptr(), &mut n) };
    if rc == 0 {
        let mut t = buf[..n as usize].to_vec();
        // drop "REXnnnn: " and the line end
        if t.starts_with(b"REX") && t.len() > 9 && &t[7..9] == b": " {
            t.drain(..9);
        }
        while t.last().is_some_and(|&c| c == b'\n' || c == b'\r') {
            t.pop();
        }
        return Some(t);
    }
    let key = alloc::format!("REX{num:04}");
    for l in FALLBACK.lines() {
        if l.starts_with(&key) && l.len() > 9 {
            let t = l[9..].trim_start_matches(": ").trim_end();
            let t = t.replace("%1", "").replace("%2", "");
            return Some(t.into_bytes());
        }
    }
    None
}

/// The full error report line: REXnnnn: Error n running FILE, line L: text
fn error_report(code: u32, file: &[u8], line: u32) -> Vec<u8> {
    let ls = alloc::format!("{line}\0");
    let cs = alloc::format!("{code}\0");
    let fz = cz(file);
    let ins: [*const u8; 3] = [ls.as_ptr(), fz.as_ptr(), cs.as_ptr()];
    let mut head = vec![0u8; 512];
    let mut n = 0u32;
    let rc = unsafe { os2::msg::DosGetMessage(ins.as_ptr(), 3, head.as_mut_ptr(), head.len() as u32, 100, b"REX.MSG\0".as_ptr(), &mut n) };
    let head: Vec<u8> = if rc == 0 {
        head.truncate(n as usize);
        head
    } else {
        alloc::format!("Error {code} running {}, line {line}: ", core::str::from_utf8(file).unwrap_or("")).into_bytes()
    };
    let mut h0 = head.clone();
    h0.push(0);
    let empty = b"\0";
    let ins2: [*const u8; 2] = [h0.as_ptr(), empty.as_ptr()];
    let mut out = vec![0u8; 1024];
    let mut n2 = 0u32;
    let rc = unsafe { os2::msg::DosGetMessage(ins2.as_ptr(), 2, out.as_mut_ptr(), out.len() as u32, code, b"REX.MSG\0".as_ptr(), &mut n2) };
    if rc == 0 {
        out.truncate(n2 as usize);
        out
    } else {
        let mut v = alloc::format!("REX{code:04}: ").into_bytes();
        v.extend_from_slice(&head);
        v.extend_from_slice(&message_text(code).unwrap_or_default());
        v.extend_from_slice(b"\r\n");
        v
    }
}

// ---- exits ----

impl OsHost {
    fn exit(&self, num: usize) -> Option<&[u8]> {
        self.exits.get(num).and_then(|e| e.as_deref())
    }

    /// Call exit `num`; Some(true) handled, Some(false) not handled, None:
    /// no exit; Err: the exit failed.
    fn call_exit(&mut self, st: &mut State, num: usize, sub: i32, parm: *mut u8) -> Result<bool, ()> {
        let Some(name) = self.exit(num) else { return Ok(false) };
        let nz = cz(name);
        let r = crate::with_state(st, || unsafe { RexxCallExit(nz.as_ptr(), null(), num as i32, sub, parm) });
        match r {
            RXEXIT_HANDLED => Ok(true),
            RXEXIT_RAISE_ERROR => Err(()),
            _ => Ok(false),
        }
    }

    /// Session I/O output exit (SAY 1, trace 2).
    fn sio_out(&mut self, st: &mut State, sub: i32, line: &[u8]) -> bool {
        if self.exit(RXSIO).is_none() {
            return false;
        }
        let mut copy = line.to_vec();
        copy.push(0);
        let mut parm = RxString { len: line.len() as u32, ptr: copy.as_mut_ptr() };
        matches!(self.call_exit(st, RXSIO, sub, &mut parm as *mut RxString as *mut u8), Ok(true))
    }

    /// Session I/O input exit (terminal read 3, trace read 4).
    fn sio_in(&mut self, st: &mut State, sub: i32) -> Option<Option<Vec<u8>>> {
        self.exit(RXSIO)?;
        let mut buf = vec![0u8; 256];
        let mut parm = RxString { len: buf.len() as u32, ptr: buf.as_mut_ptr() };
        match self.call_exit(st, RXSIO, sub, &mut parm as *mut RxString as *mut u8) {
            Ok(true) => {
                let v = rx_bytes(&parm);
                if !parm.ptr.is_null() && parm.ptr != buf.as_mut_ptr() {
                    unsafe { DosFreeMem(parm.ptr) };
                }
                Some(Some(v))
            }
            _ => None,
        }
    }
}

/// RXSUBCOM_ERROR (1) and RXSUBCOM_FAILURE (2) as condition numbers.
fn flags_cond(flags: u16) -> u8 {
    if flags & 1 != 0 {
        1
    } else if flags & 2 != 0 {
        2
    } else {
        0
    }
}

/// Run a command through CMD.EXE (the CMD environment when nobody
/// registered one: REXX run by a program other than CMD).
fn run_cmd(cmd: &[u8]) -> CmdResult {
    let comspec = env_get(b"COMSPEC").unwrap_or_else(|| b"CMD.EXE".to_vec());
    let mut args = comspec.clone();
    args.push(0);
    args.extend_from_slice(b"/C ");
    args.extend_from_slice(cmd);
    args.push(0);
    args.push(0);
    let env = env_block();
    let pz = cz(&comspec);
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    let rc = unsafe { DosExecPgm(fail.as_mut_ptr(), fail.len() as u32, 0, args.as_ptr(), env.as_ptr(), &mut res, pz.as_ptr()) };
    if rc != 0 {
        return CmdResult { rc: b"-3".to_vec(), cond: 2 };
    }
    let code = res.code_result as i32;
    CmdResult { rc: alloc::format!("{code}").into_bytes(), cond: if code != 0 { 1 } else { 0 } }
}

/// Search for a REXX program file: the name as given (adding .CMD when it
/// has no extension), in the current directory, then along PATH.
fn find_program(name: &[u8]) -> Option<Vec<u8>> {
    let base = name.rsplit(|&c| c == b'\\' || c == b'/' || c == b':').next().unwrap_or(name);
    let mut cands = Vec::new();
    if !base.contains(&b'.') {
        let mut n = name.to_vec();
        n.extend_from_slice(b".CMD");
        cands.push(n);
    } else {
        cands.push(name.to_vec());
    }
    for c in cands {
        let has_path = c.iter().any(|&x| x == b'\\' || x == b'/' || x == b':');
        if has_path {
            if let Some(f) = full_name(&c) {
                return Some(f);
            }
            continue;
        }
        let cz_ = cz(&c);
        let mut buf = [0u8; 260];
        // SEARCH_CUR_DIRECTORY | SEARCH_ENVIRONMENT
        if unsafe { DosSearchPath(3, b"PATH\0".as_ptr(), cz_.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0 {
            return Some(cstr(buf.as_ptr()).to_vec());
        }
    }
    None
}

/// The fully qualified name of an existing file.
pub fn full_name(name: &[u8]) -> Option<Vec<u8>> {
    let nz = cz(name);
    let mut st = [0u8; 64];
    if unsafe { DosQueryPathInfo(nz.as_ptr(), 1, st.as_mut_ptr(), st.len() as u32) } != 0 {
        return None;
    }
    // FILESTATUS3.attrFile at +20: directories are not files
    if u32::from_le_bytes([st[20], st[21], st[22], st[23]]) & 0x10 != 0 {
        return None;
    }
    let mut buf = [0u8; 260];
    if unsafe { DosQueryPathInfo(nz.as_ptr(), 5, buf.as_mut_ptr(), buf.len() as u32) } != 0 {
        return Some(name.to_vec());
    }
    Some(cstr(buf.as_ptr()).to_vec())
}

pub fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    let z = cz(path);
    let (mut hf, mut act) = (0u32, 0u32);
    if unsafe { DosOpen(z.as_ptr(), &mut hf, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYWRITE, null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = vec![0u8; 16384];
    loop {
        let mut n = 0u32;
        if unsafe { DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut n) } != 0 || n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { DosClose(hf) };
    Some(out)
}

/// Arguments as RXSTRINGs (each value followed by a zero, omitted ones
/// null).
pub struct ArgList {
    _data: Vec<Vec<u8>>,
    pub rx: Vec<RxString>,
}

impl ArgList {
    pub fn new(args: &[Option<Vec<u8>>]) -> ArgList {
        let mut data: Vec<Vec<u8>> = Vec::new();
        let mut rx = Vec::new();
        for a in args {
            match a {
                Some(v) => {
                    let mut z = v.clone();
                    z.push(0);
                    data.push(z);
                    let d = data.last_mut().unwrap();
                    rx.push(RxString { len: v.len() as u32, ptr: d.as_mut_ptr() });
                }
                None => rx.push(RxString { len: 0, ptr: null_mut() }),
            }
        }
        ArgList { _data: data, rx }
    }
}

impl Host for OsHost {
    fn say(&mut self, st: &mut State, line: &[u8]) {
        if !self.sio_out(st, 1, line) {
            let mut v = line.to_vec();
            v.extend_from_slice(b"\r\n");
            write_handle(1, &v);
        }
    }

    fn trace_out(&mut self, st: &mut State, line: &[u8]) {
        if !self.sio_out(st, 2, line) {
            let mut v = line.to_vec();
            v.extend_from_slice(b"\r\n");
            write_handle(2, &v);
        }
    }

    fn pull_terminal(&mut self, st: &mut State) -> Option<Vec<u8>> {
        if let Some(r) = self.sio_in(st, 3) {
            return r;
        }
        read_line(0)
    }

    fn trace_in(&mut self, st: &mut State) -> Option<Vec<u8>> {
        if let Some(r) = self.sio_in(st, 4) {
            return r;
        }
        read_line(0)
    }

    fn command(&mut self, st: &mut State, env: &[u8], cmd: &[u8]) -> CmdResult {
        // the command exit
        if self.exit(RXCMD).is_some() {
            let mut envz = cz(env);
            let mut cmdz = cz(cmd);
            let mut ret = vec![0u8; 256];
            let mut p = [0u8; 0x1d];
            p[1..5].copy_from_slice(&(envz.as_mut_ptr() as u32).to_le_bytes());
            p[5..7].copy_from_slice(&(env.len() as u16).to_le_bytes());
            p[0xd..0x11].copy_from_slice(&(cmd.len() as u32).to_le_bytes());
            p[0x11..0x15].copy_from_slice(&(cmdz.as_mut_ptr() as u32).to_le_bytes());
            p[0x15..0x19].copy_from_slice(&(ret.len() as u32).to_le_bytes());
            p[0x19..0x1d].copy_from_slice(&(ret.as_mut_ptr() as u32).to_le_bytes());
            match self.call_exit(st, RXCMD, 1, p.as_mut_ptr()) {
                Ok(true) => {
                    let len = u32::from_le_bytes([p[0x15], p[0x16], p[0x17], p[0x18]]);
                    let ptr = u32::from_le_bytes([p[0x19], p[0x1a], p[0x1b], p[0x1c]]) as *mut u8;
                    let rc = if ptr.is_null() { b"0".to_vec() } else { rx_bytes(&RxString { len, ptr }) };
                    if !ptr.is_null() && ptr != ret.as_mut_ptr() {
                        unsafe { DosFreeMem(ptr) };
                    }
                    let cond = if p[0] & 1 != 0 { 2 } else if p[0] & 2 != 0 { 1 } else { 0 };
                    return CmdResult { rc, cond };
                }
                Err(()) => return CmdResult { rc: b"-3".to_vec(), cond: 2 },
                Ok(false) => {}
            }
        }
        // "ENV.DLL" names the environment's DLL too
        let (ename, dll) = match env.iter().position(|&c| c == b'.') {
            Some(p) => (&env[..p], Some(&env[p + 1..])),
            None => (env, None),
        };
        let is_cmd = ename.eq_ignore_ascii_case(b"CMD");
        let envz = cz(ename);
        let dllz = dll.map(cz);
        let mut cmdz = cz(cmd);
        let rcmd = RxString { len: cmd.len() as u32, ptr: cmdz.as_mut_ptr() };
        let mut buf = vec![0u8; 256];
        let mut ret = RxString { len: buf.len() as u32, ptr: buf.as_mut_ptr() };
        let mut flags: u16 = 0;
        let mut hrc: u16 = 0;
        let dp = dllz.as_ref().map_or(null(), |d| d.as_ptr());
        let r = crate::with_state(st, || unsafe { RexxCallSubcom(envz.as_ptr(), dp, &rcmd, &mut flags, &mut hrc, &mut ret) });
        if r == 30 && is_cmd {
            // nobody registered CMD: run the command through CMD.EXE
            return run_cmd(cmd);
        }
        if r != 0 {
            // RC is the API's return code (30: no such environment)
            return CmdResult { rc: alloc::format!("{r}").into_bytes(), cond: if flags == 0 { 2 } else { flags_cond(flags) } };
        }
        // a handler's own nonzero return code is RC, else its result string
        let rc = if hrc != 0 {
            alloc::format!("{}", hrc as i16).into_bytes()
        } else if ret.ptr.is_null() || ret.len == 0 {
            b"0".to_vec()
        } else {
            rx_bytes(&ret)
        };
        if !ret.ptr.is_null() && ret.ptr != buf.as_mut_ptr() {
            unsafe { DosFreeMem(ret.ptr) };
        }
        let mut cond = flags_cond(flags);
        // CMD's "not a command" (SYS1041) is a failure
        if is_cmd && rc.as_slice() == b"1041" {
            cond = 2;
        }
        CmdResult { rc, cond }
    }

    fn queued(&mut self, st: &mut State) -> usize {
        if self.exit(RXMSQ).is_some() {
            let mut n: u32 = 0;
            if let Ok(true) = self.call_exit(st, RXMSQ, 3, &mut n as *mut u32 as *mut u8) {
                return n as usize;
            }
        }
        let q = cz(&queue_name());
        let mut n = 0u32;
        if unsafe { RexxQueryQueue(q.as_ptr(), &mut n) } != 0 {
            return 0;
        }
        n as usize
    }

    fn queue_add(&mut self, st: &mut State, line: &[u8], lifo: bool) {
        let mut data = line.to_vec();
        data.push(0);
        if self.exit(RXMSQ).is_some() {
            let mut p = [0u8; 9];
            p[0] = lifo as u8;
            p[1..5].copy_from_slice(&(line.len() as u32).to_le_bytes());
            p[5..9].copy_from_slice(&(data.as_mut_ptr() as u32).to_le_bytes());
            if let Ok(true) = self.call_exit(st, RXMSQ, 2, p.as_mut_ptr()) {
                return;
            }
        }
        let q = cz(&queue_name());
        let r = RxString { len: line.len() as u32, ptr: data.as_mut_ptr() };
        unsafe { RexxAddQueue(q.as_ptr(), &r, lifo as u32) };
    }

    fn queue_pull(&mut self, st: &mut State) -> Option<Vec<u8>> {
        if self.exit(RXMSQ).is_some() {
            let mut r = RxString { len: 0, ptr: null_mut() };
            if let Ok(true) = self.call_exit(st, RXMSQ, 1, &mut r as *mut RxString as *mut u8) {
                if r.ptr.is_null() {
                    return None;
                }
                let v = rx_bytes(&r);
                unsafe { DosFreeMem(r.ptr) };
                return Some(v);
            }
        }
        let q = cz(&queue_name());
        let mut r = RxString { len: 0, ptr: null_mut() };
        let mut dt = [0u8; 12];
        if unsafe { RexxPullQueue(q.as_ptr(), &mut r, dt.as_mut_ptr(), 0) } != 0 {
            return None;
        }
        let v = rx_bytes(&r);
        if !r.ptr.is_null() {
            unsafe { DosFreeMem(r.ptr) };
        }
        Some(v)
    }

    fn rxqueue(&mut self, st: &mut State, op: &[u8], name: Option<&[u8]>) -> Result<Vec<u8>, u32> {
        match op {
            b"GET" => Ok(queue_name()),
            b"SET" => {
                let n = name.unwrap_or(b"").to_ascii_uppercase();
                let old = queue_name();
                if self.exit(RXMSQ).is_some() {
                    let mut nz = cz(&n);
                    let mut r = RxString { len: n.len() as u32, ptr: nz.as_mut_ptr() };
                    let _ = self.call_exit(st, RXMSQ, 20, &mut r as *mut RxString as *mut u8);
                }
                set_queue_name(n);
                Ok(old)
            }
            b"DELETE" => {
                let nz = cz(name.unwrap_or(b""));
                let r = unsafe { RexxDeleteQueue(nz.as_ptr()) };
                Ok(alloc::format!("{r}").into_bytes())
            }
            _ => {
                let mut buf = [0u8; 256];
                let mut dup = 0u32;
                let nz = name.map(cz);
                let r = unsafe { RexxCreateQueue(buf.as_mut_ptr(), buf.len() as u32, nz.as_ref().map_or(null(), |z| z.as_ptr()), &mut dup) };
                if r != 0 {
                    return Err(if r == 5 { 40 } else { 48 });
                }
                Ok(cstr(buf.as_ptr()).to_vec())
            }
        }
    }

    fn external(&mut self, st: &mut State, name: &[u8], args: &[Option<Vec<u8>>], func: bool) -> External {
        let mut al = ArgList::new(args);
        // macros searched first
        let mut pos: u16 = 0;
        let nz = cz(name);
        let is_macro = unsafe { RexxQueryMacro(nz.as_ptr(), &mut pos) } == 0;
        if is_macro && pos == 1 {
            if let Some(e) = macro_program(name) {
                return e;
            }
        }
        // the function exit
        if self.exit(RXFNC).is_some() {
            let mut buf = vec![0u8; 256];
            let mut qn = cz(&queue_name());
            let mut namez = nz.clone();
            let mut p = [0u8; 0x1b];
            p[0] = if func { 0 } else { 4 };
            p[1..5].copy_from_slice(&(namez.as_mut_ptr() as u32).to_le_bytes());
            p[5..7].copy_from_slice(&(name.len() as u16).to_le_bytes());
            p[7..0xb].copy_from_slice(&(qn.as_mut_ptr() as u32).to_le_bytes());
            p[0xb..0xd].copy_from_slice(&((qn.len() - 1) as u16).to_le_bytes());
            p[0xd..0xf].copy_from_slice(&(args.len() as u16).to_le_bytes());
            p[0xf..0x13].copy_from_slice(&(al.rx.as_mut_ptr() as u32).to_le_bytes());
            p[0x13..0x17].copy_from_slice(&(buf.len() as u32).to_le_bytes());
            p[0x17..0x1b].copy_from_slice(&(buf.as_mut_ptr() as u32).to_le_bytes());
            match self.call_exit(st, RXFNC, 1, p.as_mut_ptr()) {
                Ok(true) => {
                    if p[0] & 1 != 0 {
                        return External::Error(40);
                    }
                    if p[0] & 2 != 0 {
                        return External::Error(43);
                    }
                    let len = u32::from_le_bytes([p[0x13], p[0x14], p[0x15], p[0x16]]);
                    let ptr = u32::from_le_bytes([p[0x17], p[0x18], p[0x19], p[0x1a]]) as *mut u8;
                    if ptr.is_null() {
                        return External::Value(None);
                    }
                    let v = rx_bytes(&RxString { len, ptr });
                    if ptr != buf.as_mut_ptr() {
                        unsafe { DosFreeMem(ptr) };
                    }
                    return External::Value(Some(v));
                }
                Err(()) => return External::Error(48),
                Ok(false) => {}
            }
        }
        // function packages
        let mut buf = vec![0u8; 256];
        let mut ret = RxString { len: buf.len() as u32, ptr: buf.as_mut_ptr() };
        let mut rc: u16 = 0;
        let qn = cz(&queue_name());
        let r = crate::with_state(st, || unsafe { RexxCallFunction(nz.as_ptr(), args.len() as u32, al.rx.as_ptr(), &mut rc, &mut ret, qn.as_ptr()) });
        if r == 0 {
            if rc != 0 {
                return External::Error(40);
            }
            if ret.ptr.is_null() {
                return External::Value(None);
            }
            let v = rx_bytes(&ret);
            if ret.ptr != buf.as_mut_ptr() {
                unsafe { DosFreeMem(ret.ptr) };
            }
            return External::Value(Some(v));
        }
        if r != 30 {
            return External::Error(40);
        }
        // programs on disk
        if let Some(path) = find_program(name) {
            if let Some(src) = read_file(&path) {
                return External::Program { source: src, path };
            }
        }
        if is_macro {
            if let Some(e) = macro_program(name) {
                return e;
            }
        }
        External::NotFound
    }

    fn rxfunc(&mut self, op: u8, name: &[u8], module: &[u8], proc_: &[u8]) -> u32 {
        let nz = cz(name);
        match op {
            b'A' => {
                let mz = cz(module);
                let pz = cz(proc_);
                unsafe { RexxRegisterFunctionDll(nz.as_ptr(), mz.as_ptr(), pz.as_ptr()) }
            }
            b'D' => unsafe { RexxDeregisterFunction(nz.as_ptr()) },
            _ => (unsafe { RexxQueryFunction(nz.as_ptr()) } != 0) as u32,
        }
    }

    fn env_get(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        env_get(name)
    }

    fn env_set(&mut self, name: &[u8], val: Option<&[u8]>) {
        env_set(name, val.filter(|v| !v.is_empty()));
    }

    fn directory(&mut self, new: Option<&[u8]>) -> Option<Vec<u8>> {
        if let Some(d) = new {
            if d.len() >= 2 && d[1] == b':' {
                let drive = d[0].to_ascii_uppercase();
                if !drive.is_ascii_uppercase() || unsafe { DosSetDefaultDisk((drive - b'@') as u32) } != 0 {
                    return Some(Vec::new());
                }
            }
            let rest = if d.len() >= 2 && d[1] == b':' { &d[2..] } else { d };
            if !rest.is_empty() {
                let z = cz(d);
                if unsafe { DosSetCurrentDir(z.as_ptr()) } != 0 {
                    return Some(Vec::new());
                }
            }
        }
        Some(current_dir())
    }

    fn setlocal(&mut self) -> bool {
        let (mut disk, mut map) = (0u32, 0u32);
        unsafe { DosQueryCurrentDisk(&mut disk, &mut map) };
        self.locals.push((env_block(), disk, current_dir()));
        true
    }

    fn endlocal(&mut self) -> bool {
        let Some((env, disk, dir)) = self.locals.pop() else { return false };
        let entries: Vec<Vec<u8>> = env_entries(&env).iter().map(|e| e.to_vec()).collect();
        env_put(&entries);
        unsafe { DosSetDefaultDisk(disk) };
        let z = cz(&dir);
        unsafe { DosSetCurrentDir(z.as_ptr()) };
        true
    }

    fn now(&mut self) -> Now {
        let mut dt = DateTime::default();
        unsafe { DosGetDateTime(&mut dt) };
        Now {
            year: dt.year as u32,
            month: dt.month as u32,
            day: dt.day as u32,
            hour: dt.hours as u32,
            min: dt.minutes as u32,
            sec: dt.seconds as u32,
            micros: dt.hundredths as u32 * 10000,
        }
    }

    fn halt(&mut self, st: &mut State) -> bool {
        if crate::take_halt(self.tid) {
            return true;
        }
        if self.exit(RXHLT).is_some() {
            let mut f = [0u8; 4];
            if let Ok(true) = self.call_exit(st, RXHLT, 2, f.as_mut_ptr()) {
                if f[0] & 1 != 0 {
                    let mut c = [0u8; 4];
                    let _ = self.call_exit(st, RXHLT, 1, c.as_mut_ptr());
                    return true;
                }
            }
        }
        false
    }

    fn trace_request(&mut self) -> Option<bool> {
        crate::take_trace(self.tid)
    }

    fn beep(&mut self, freq: u32, ms: u32) {
        unsafe { DosBeep(freq, ms) };
    }

    fn message(&mut self, num: u32) -> Option<Vec<u8>> {
        message_text(num)
    }

    fn report_error(&mut self, st: &mut State, code: u32, file: &[u8], line: u32) {
        let text = error_report(code, file, line);
        if self.exit(RXSIO).is_some() {
            let mut t = text.clone();
            while t.last().is_some_and(|&c| c == b'\n' || c == b'\r') {
                t.pop();
            }
            if self.sio_out(st, 2, &t) {
                return;
            }
        }
        unsafe { os2::msg::DosPutMessage(2, text.len() as u32, text.as_ptr()) };
    }

    fn report_trace_error(&mut self, st: &mut State, code: u32) {
        // message `code` with message 126 ("Error n in interactive trace:")
        let cs = alloc::format!("{code}\0");
        let ins: [*const u8; 1] = [cs.as_ptr()];
        let mut head = vec![0u8; 256];
        let mut n = 0u32;
        let rc = unsafe { os2::msg::DosGetMessage(ins.as_ptr(), 1, head.as_mut_ptr(), head.len() as u32, 126, b"REX.MSG\0".as_ptr(), &mut n) };
        let mut head = if rc == 0 {
            head.truncate(n as usize);
            head
        } else {
            alloc::format!("Error {code} in interactive trace: ").into_bytes()
        };
        head.push(0);
        let ins2: [*const u8; 2] = [head.as_ptr(), b"\0".as_ptr()];
        let mut out = vec![0u8; 1024];
        let mut n2 = 0u32;
        let rc = unsafe { os2::msg::DosGetMessage(ins2.as_ptr(), 2, out.as_mut_ptr(), out.len() as u32, code, b"REX.MSG\0".as_ptr(), &mut n2) };
        let text = if rc == 0 {
            out.truncate(n2 as usize);
            out
        } else {
            let mut v = head[..head.len() - 1].to_vec();
            v.extend_from_slice(&message_text(code).unwrap_or_default());
            v.extend_from_slice(b"\r\n");
            v
        };
        if self.exit(RXSIO).is_some() {
            let mut t = text.clone();
            while t.last().is_some_and(|&c| c == b'\n' || c == b'\r') {
                t.pop();
            }
            if self.sio_out(st, 2, &t) {
                return;
            }
        }
        unsafe { os2::msg::DosPutMessage(2, text.len() as u32, text.as_ptr()) };
    }

    fn init(&mut self, st: &mut State) {
        if self.exit(RXINI).is_some() {
            let _ = self.call_exit(st, RXINI, 1, null_mut());
        }
    }

    fn term(&mut self, st: &mut State) {
        if self.exit(RXTER).is_some() {
            let _ = self.call_exit(st, RXTER, 1, null_mut());
        }
    }

    fn f_open(&mut self, name: &[u8], mode: OpenMode) -> Result<u32, u32> {
        let mut n = name.to_vec();
        // devices may be written with a colon (COM1:, CON:)
        if n.len() > 2 && n.ends_with(b":") {
            n.pop();
        }
        let z = cz(&n);
        let (action, access) = match mode {
            OpenMode::Read => (OPEN_ACTION_FAIL_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READONLY | OPEN_SHARE_DENYNONE),
            OpenMode::Write => (OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_WRITEONLY | OPEN_SHARE_DENYWRITE),
            OpenMode::Both => (OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_OPEN_IF_EXISTS, OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYWRITE),
        };
        let (mut hf, mut act) = (0u32, 0u32);
        let rc = unsafe { DosOpen(z.as_ptr(), &mut hf, &mut act, 0, 0, action, access | OPEN_FLAGS_NOINHERIT, null_mut()) };
        if rc != 0 { Err(rc) } else { Ok(hf) }
    }

    fn f_read(&mut self, h: u32, buf: &mut [u8]) -> Result<usize, u32> {
        let mut n = 0u32;
        let rc = unsafe { DosRead(h, buf.as_mut_ptr(), buf.len() as u32, &mut n) };
        if rc != 0 { Err(rc) } else { Ok(n as usize) }
    }

    fn f_write(&mut self, h: u32, data: &[u8]) -> Result<usize, u32> {
        let mut n = 0u32;
        let rc = unsafe { DosWrite(h, data.as_ptr(), data.len() as u32, &mut n) };
        if rc != 0 { Err(rc) } else { Ok(n as usize) }
    }

    fn f_seek(&mut self, h: u32, pos: u64) -> Result<(), u32> {
        let mut a = 0u32;
        let rc = unsafe { DosSetFilePtr(h, pos as i32, 0, &mut a) };
        if rc != 0 { Err(rc) } else { Ok(()) }
    }

    fn f_size(&mut self, h: u32) -> Option<u64> {
        let (mut t, mut a) = (0u32, 0u32);
        if unsafe { DosQueryHType(h, &mut t, &mut a) } != 0 || t & 0xff != 0 {
            return None;
        }
        let mut st = [0u8; 64];
        if unsafe { DosQueryFileInfo(h, 1, st.as_mut_ptr(), st.len() as u32) } != 0 {
            return None;
        }
        Some(u32::from_le_bytes([st[12], st[13], st[14], st[15]]) as u64)
    }

    fn f_close(&mut self, h: u32) {
        unsafe { DosClose(h) };
    }

    fn f_exists(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        full_name(name)
    }

    fn f_info(&mut self, name: &[u8]) -> Option<FileInfo> {
        let z = cz(name);
        let mut st = [0u8; 64];
        if unsafe { DosQueryPathInfo(z.as_ptr(), 1, st.as_mut_ptr(), st.len() as u32) } != 0 {
            return None;
        }
        let date = u16::from_le_bytes([st[8], st[9]]);
        let time = u16::from_le_bytes([st[10], st[11]]);
        let size = u32::from_le_bytes([st[12], st[13], st[14], st[15]]) as u64;
        let (d, m, y) = (date & 31, (date >> 5) & 15, ((date >> 9) + 1980) % 100);
        let (s, mi, h) = ((time & 31) * 2, (time >> 5) & 63, time >> 11);
        Some(FileInfo { size, datetime: alloc::format!("{m:02}-{d:02}-{y:02}  {h:02}:{mi:02}:{s:02}").into_bytes() })
    }

    fn f_is_device(&mut self, h: u32) -> bool {
        let (mut t, mut a) = (0u32, 0u32);
        let rc = unsafe { DosQueryHType(h, &mut t, &mut a) };
        rc == 0 && t & 0xff == 1
    }
}

/// A macro's program.
fn macro_program(name: &[u8]) -> Option<External> {
    let nz = cz(name);
    let mut img = RxString { len: 0, ptr: null_mut() };
    if unsafe { RexxExecuteMacroFunction(nz.as_ptr(), &mut img) } != 0 {
        return None;
    }
    Some(External::Program { source: rx_bytes(&img), path: name.to_vec() })
}
