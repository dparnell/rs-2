//! Running the command-line tool behind a window, as OS/2's PMCHKDSK and
//! PMFORMAT do: its standard input and output on pipes, the `MSG_<n>,...`
//! lines it writes passed to a window, answers written back. (Shared by
//! pmchkdsk and pmformat: pmformat includes this file.)

use alloc::boxed::Box;
use alloc::vec::Vec;

use pmkit::*;

/// A program running on pipes.
pub struct Child {
    pub pid: u32,
    /// Our end of its standard input.
    pub input: u32,
}

struct Reader {
    out: u32,
    pid: u32,
    hwnd: u32,
    msg_line: u32,
    msg_end: u32,
    /// Every line goes to the window, not only those with `MSG_`.
    all: bool,
}

/// The full name of the first of `names` found in `<boot>:\OS2`, then
/// along PATH.
pub fn find_program(names: &[&[u8]]) -> Option<Vec<u8>> {
    let dir = [&[boot_drive()][..], b":\\OS2\0"].concat();
    for n in names {
        let n = z(n);
        let mut buf = [0u8; 260];
        if unsafe { DosSearchPath(0, dir.as_ptr(), n.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0
            || unsafe { DosSearchPath(2, b"PATH\0".as_ptr(), n.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } == 0
        {
            return Some(until_nul(&buf).to_vec());
        }
    }
    None
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.228"]
    fn DosSearchPath(flags: u32, path: *const u8, name: *const u8, buf: *mut u8, cb: u32) -> u32;
}

/// Start `path` with `args` on pipes. Each line of its output with
/// `MSG_` in it goes to `hwnd` as `msg_line` (mp1: the message number,
/// mp2: a `Box<Vec<u8>>` of the line from after the number's comma,
/// which the window must take back with `take_line`); when it ends,
/// `msg_end` (mp1: its result code).
pub fn start(path: &[u8], args: &[u8], hwnd: u32, msg_line: u32, msg_end: u32) -> Result<Child, u32> {
    start_lines(path, args, hwnd, msg_line, msg_end, false)
}

/// `start`; with `all`, every line of its output goes to `hwnd` (mp1 0).
pub fn start_lines(path: &[u8], args: &[u8], hwnd: u32, msg_line: u32, msg_end: u32, all: bool) -> Result<Child, u32> {
    let (mut in_r, mut in_w, mut out_r, mut out_w) = (0u32, 0u32, 0u32, 0u32);
    unsafe {
        let rc = os2::DosCreatePipe(&mut in_r, &mut in_w, 2048);
        if rc != 0 {
            return Err(rc);
        }
        let rc = os2::DosCreatePipe(&mut out_r, &mut out_w, 2048);
        if rc != 0 {
            os2::DosClose(in_r);
            os2::DosClose(in_w);
            return Err(rc);
        }
        // Our ends stay ours (OPEN_FLAGS_NOINHERIT).
        os2::DosSetFHState(in_w, 0x80);
        os2::DosSetFHState(out_r, 0x80);
    }
    // The child inherits handles 0 and 1: point them at the pipes for
    // the start, then put ours back.
    let (mut save0, mut save1) = (0xffff_ffffu32, 0xffff_ffffu32);
    let had0 = unsafe { os2::DosDupHandle(0, &mut save0) } == 0;
    let had1 = unsafe { os2::DosDupHandle(1, &mut save1) } == 0;
    let (mut h0, mut h1) = (0u32, 1u32);
    unsafe {
        os2::DosDupHandle(in_r, &mut h0);
        os2::DosDupHandle(out_w, &mut h1);
    }
    let mut a = Vec::new();
    a.extend_from_slice(file_name(path));
    a.push(0);
    a.push(b' ');
    a.extend_from_slice(args);
    a.extend_from_slice(&[0, 0]);
    let p = z(path);
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let mut fail = [0u8; 64];
    // EXEC_ASYNCRESULT: the result is kept for DosWaitChild.
    let rc = unsafe { os2::DosExecPgm(fail.as_mut_ptr(), 64, 2, a.as_ptr(), core::ptr::null(), &mut res, p.as_ptr()) };
    unsafe {
        let (mut h0, mut h1) = (0u32, 1u32);
        if had0 {
            os2::DosDupHandle(save0, &mut h0);
            os2::DosClose(save0);
        } else {
            os2::DosClose(0);
        }
        if had1 {
            os2::DosDupHandle(save1, &mut h1);
            os2::DosClose(save1);
        } else {
            os2::DosClose(1);
        }
        os2::DosClose(in_r);
        os2::DosClose(out_w);
    }
    if rc != 0 {
        unsafe {
            os2::DosClose(in_w);
            os2::DosClose(out_r);
        }
        return Err(rc);
    }
    let pid = res.code_terminate;
    let r = Box::into_raw(Box::new(Reader { out: out_r, pid, hwnd, msg_line, msg_end, all }));
    let mut tid = 0;
    unsafe { os2::DosCreateThread(&mut tid, reader, r as u32, 0, 0x8000) };
    Ok(Child { pid, input: in_w })
}

/// Take back a line passed with `msg_line`.
pub fn take_line(mp2: u32) -> Vec<u8> {
    if mp2 == 0 {
        return Vec::new();
    }
    *unsafe { Box::from_raw(mp2 as *mut Vec<u8>) }
}

/// The parameters of a line (after the message number), split at commas.
pub fn params(rest: &[u8]) -> Vec<Vec<u8>> {
    if rest.is_empty() {
        return Vec::new();
    }
    rest.split(|&c| c == b',').map(|p| p.to_vec()).collect()
}

/// `MSG_<n>[,params]` somewhere in a line: (n, params text).
pub fn parse(line: &[u8]) -> Option<(u32, Vec<u8>)> {
    let at = line.windows(4).position(|w| w == b"MSG_")?;
    let rest = &line[at + 4..];
    let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let n = core::str::from_utf8(&rest[..digits]).ok()?.parse().ok()?;
    let after = &rest[digits..];
    let params = after.strip_prefix(b",").unwrap_or(after);
    Some((n, params.to_vec()))
}

unsafe extern "C" fn reader(p: u32) {
    let r = unsafe { Box::from_raw(p as *mut Reader) };
    let mut line = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        let mut got = 0;
        if unsafe { os2::DosRead(r.out, buf.as_mut_ptr(), buf.len() as u32, &mut got) } != 0 || got == 0 {
            break;
        }
        for &c in &buf[..got as usize] {
            if c == b'\n' || c == b'\r' {
                if r.all {
                    post(r.hwnd, r.msg_line, 0, Box::into_raw(Box::new(core::mem::take(&mut line))) as u32);
                } else if let Some((n, rest)) = parse(&line) {
                    let b = Box::into_raw(Box::new(rest));
                    post(r.hwnd, r.msg_line, n, b as u32);
                }
                line.clear();
            } else if c >= 0x20 {
                line.push(c);
            }
        }
    }
    if r.all {
        post(r.hwnd, r.msg_line, 0, Box::into_raw(Box::new(line)) as u32);
    } else if let Some((n, rest)) = parse(&line) {
        post(r.hwnd, r.msg_line, n, Box::into_raw(Box::new(rest)) as u32);
    }
    unsafe { os2::DosClose(r.out) };
    let mut res = os2::ResultCodes { code_terminate: 0, code_result: 0 };
    let mut pid = 0;
    // DCWA_PROCESS, DCWW_WAIT.
    let rc = unsafe { os2::DosWaitChild(0, 0, &mut res, &mut pid, r.pid) };
    let code = if rc != 0 { 0 } else if res.code_terminate != 0 { 0xffff } else { res.code_result };
    post(r.hwnd, r.msg_end, code, 0);
}

impl Child {
    /// Write an answer line (`Y`, `N` or nothing) to the program.
    pub fn answer(&self, text: &[u8]) {
        let mut l = text.to_vec();
        l.extend_from_slice(b"\r\n");
        let mut done = 0;
        unsafe { os2::DosWrite(self.input, l.as_ptr(), l.len() as u32, &mut done) };
    }

    /// End the program (Stop).
    pub fn kill(&self) {
        // DKP_PROCESSTREE.
        unsafe { os2::DosKillProcess(0, self.pid) };
    }

    pub fn close(&mut self) {
        if self.input != 0 {
            unsafe { os2::DosClose(self.input) };
            self.input = 0;
        }
    }
}
