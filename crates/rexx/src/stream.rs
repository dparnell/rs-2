//! Character and line streams (CHARIN, CHAROUT, CHARS, LINEIN, LINEOUT,
//! LINES, STREAM) over the host's file calls. Streams open on first use
//! and stay open until closed or the program ends. STDIN, STDOUT and
//! STDERR (with or without a colon) are the standard handles; "QUEUE:"
//! reads the external data queue.

use alloc::vec::Vec;

use crate::exec::{Frame, Interp, R, Val};
use crate::parse::Cond;
use crate::{Err, Host, OpenMode};

#[derive(Clone, Copy, PartialEq)]
pub enum SState {
    Ready,
    NotReady,
    Error,
    Unknown,
}

pub struct Stream {
    /// Upper-cased name.
    pub key: Val,
    pub name: Val,
    pub h: u32,
    pub mode: OpenMode,
    /// Persistent (a file) or transient (device, pipe, standard stream).
    pub persistent: bool,
    pub rpos: u64,
    pub wpos: u64,
    pub state: SState,
    pub desc: Val,
    pub std: bool,
    /// A character read ahead from a transient stream.
    pub ahead: Option<u8>,
    /// Read buffer (persistent streams): data starting at buf_pos.
    pub buf: Vec<u8>,
    pub buf_pos: u64,
}

#[derive(Default)]
pub struct Streams {
    pub list: Vec<Stream>,
}

impl Streams {
    pub fn close_all(&mut self, host: &mut dyn Host) {
        for s in self.list.drain(..) {
            if !s.std {
                host.f_close(s.h);
            }
        }
    }
}

fn upper(s: &[u8]) -> Val {
    s.iter().map(|c| c.to_ascii_uppercase()).collect()
}

fn std_handle(key: &[u8]) -> Option<u32> {
    match key {
        b"STDIN" | b"STDIN:" => Some(0),
        b"STDOUT" | b"STDOUT:" => Some(1),
        b"STDERR" | b"STDERR:" => Some(2),
        _ => None,
    }
}

impl Interp<'_> {
    /// Find or open a stream. `write`: the operation writes.
    fn stream(&mut self, name: &[u8], write: bool) -> Result<usize, u32> {
        let key = upper(name);
        if let Some(i) = self.st.streams.list.iter().position(|s| s.key == key) {
            let s = &self.st.streams.list[i];
            if write && s.mode == OpenMode::Read && !s.std {
                // reopen for writing too
                let (rpos, h) = (s.rpos, s.h);
                self.host.f_close(h);
                match self.host.f_open(name, OpenMode::Both) {
                    Ok(nh) => {
                        let size = self.host.f_size(nh).unwrap_or(0);
                        let s = &mut self.st.streams.list[i];
                        s.h = nh;
                        s.mode = OpenMode::Both;
                        s.rpos = rpos;
                        s.wpos = size;
                        s.buf.clear();
                    }
                    Err(e) => {
                        self.st.streams.list.remove(i);
                        return Err(e);
                    }
                }
            }
            return Ok(i);
        }
        let mode = if write { OpenMode::Both } else { OpenMode::Read };
        self.open_stream(name, mode)
    }

    fn open_stream(&mut self, name: &[u8], mode: OpenMode) -> Result<usize, u32> {
        let key = upper(name);
        let (h, std) = match std_handle(&key) {
            Some(h) => (h, true),
            None => {
                let h = match self.host.f_open(name, mode) {
                    Ok(h) => h,
                    Err(e) if mode == OpenMode::Both => {
                        // a read-only file can still be read
                        let _ = e;
                        self.host.f_open(name, OpenMode::Write)?
                    }
                    Err(e) => return Err(e),
                };
                (h, false)
            }
        };
        let size = if std { None } else { self.host.f_size(h) };
        let s = Stream {
            key,
            name: name.to_vec(),
            h,
            mode,
            persistent: size.is_some(),
            rpos: 0,
            wpos: if matches!(mode, OpenMode::Read | OpenMode::Replace) { 0 } else { size.unwrap_or(0) },
            state: SState::Ready,
            desc: Vec::new(),
            std,
            ahead: None,
            buf: Vec::new(),
            buf_pos: 0,
        };
        self.st.streams.list.push(s);
        Ok(self.st.streams.list.len() - 1)
    }

    fn close_stream(&mut self, name: &[u8]) -> bool {
        let key = upper(name);
        if let Some(i) = self.st.streams.list.iter().position(|s| s.key == key) {
            let s = self.st.streams.list.remove(i);
            if !s.std {
                self.host.f_close(s.h);
            }
            true
        } else {
            false
        }
    }

    fn notready(&mut self, f: &mut Frame, name: &[u8], i: Option<usize>, desc: &[u8]) -> R<()> {
        if let Some(i) = i {
            if let Some(s) = self.st.streams.list.get_mut(i) {
                s.state = SState::NotReady;
                s.desc = desc.to_vec();
            }
        }
        self.raise(f, Cond::NotReady, name.to_vec())?;
        Ok(())
    }

    /// Read one character; None at the end.
    fn getc(&mut self, i: usize) -> Option<u8> {
        let s = &mut self.st.streams.list[i];
        if let Some(c) = s.ahead.take() {
            return Some(c);
        }
        if s.persistent {
            let pos = s.rpos;
            if pos >= s.buf_pos && pos < s.buf_pos + s.buf.len() as u64 {
                let c = s.buf[(pos - s.buf_pos) as usize];
                s.rpos += 1;
                return Some(c);
            }
            let h = s.h;
            if self.host.f_seek(h, pos).is_err() {
                return None;
            }
            let mut b = alloc::vec![0u8; 4096];
            let n = self.host.f_read(h, &mut b).unwrap_or(0);
            let s = &mut self.st.streams.list[i];
            b.truncate(n);
            s.buf = b;
            s.buf_pos = pos;
            if n == 0 {
                return None;
            }
            s.rpos += 1;
            Some(s.buf[0])
        } else {
            let h = s.h;
            let mut b = [0u8; 1];
            match self.host.f_read(h, &mut b) {
                Ok(1) => Some(b[0]),
                _ => None,
            }
        }
    }

    fn size_of(&mut self, i: usize) -> Option<u64> {
        let s = &self.st.streams.list[i];
        if !s.persistent {
            return None;
        }
        let h = s.h;
        self.host.f_size(h)
    }

    fn put(&mut self, i: usize, data: &[u8]) -> Result<(), u32> {
        let s = &self.st.streams.list[i];
        let (h, persistent, pos) = (s.h, s.persistent, s.wpos);
        if persistent {
            self.host.f_seek(h, pos)?;
        }
        let n = self.host.f_write(h, data)?;
        let s = &mut self.st.streams.list[i];
        s.wpos += n as u64;
        // the read buffer may hold old data
        s.buf.clear();
        if n < data.len() { Err(112) } else { Ok(()) }
    }

    /// Read a line (without its CR LF); None at the end of the stream.
    fn read_line(&mut self, i: usize) -> Option<Val> {
        let mut line = Vec::new();
        let mut any = false;
        loop {
            match self.getc(i) {
                None => return if any { Some(line) } else { None },
                Some(0x1a) if self.st.streams.list[i].persistent => {
                    // Ctrl+Z: the end of the text; stay before it
                    self.st.streams.list[i].rpos -= 1;
                    return if any { Some(line) } else { None };
                }
                Some(b'\n') => return Some(line),
                Some(b'\r') => {
                    any = true;
                    match self.getc(i) {
                        Some(b'\n') | None => return Some(line),
                        Some(c) => {
                            let s = &mut self.st.streams.list[i];
                            if s.persistent {
                                s.rpos -= 1;
                            } else {
                                s.ahead = Some(c);
                            }
                            return Some(line);
                        }
                    }
                }
                Some(c) => {
                    any = true;
                    line.push(c);
                }
            }
        }
    }
}

fn pos_arg(v: &Option<Val>, digits: usize) -> R<Option<i64>> {
    match v {
        None => Ok(None),
        Some(x) => {
            let n = crate::exec::whole(x, digits).map_err(|_| Err::new(40))?;
            Ok(Some(n))
        }
    }
}

fn name_arg(v: Option<&Option<Val>>) -> Val {
    match v {
        Some(Some(n)) if !n.is_empty() => n.clone(),
        _ => b"STDIN".to_vec(),
    }
}

fn out_name(v: Option<&Option<Val>>) -> Val {
    match v {
        Some(Some(n)) if !n.is_empty() => n.clone(),
        _ => b"STDOUT".to_vec(),
    }
}

/// PARSE LINEIN: a line from STDIN.
pub fn linein_default(it: &mut Interp, f: &mut Frame) -> R<Val> {
    linein(it, f, &[]).map(|v| v)
}

pub fn linein(it: &mut Interp, f: &mut Frame, a: &[Option<Val>]) -> R<Val> {
    if a.len() > 3 {
        return Err(Err::new(40));
    }
    let name = name_arg(a.first());
    let line = pos_arg(a.get(1).unwrap_or(&None), f.numeric.digits)?;
    let count = pos_arg(a.get(2).unwrap_or(&None), f.numeric.digits)?.unwrap_or(1);
    if !(0..=1).contains(&count) || line.is_some_and(|l| l != 1) {
        return Err(Err::new(40));
    }
    if upper(&name).as_slice() == b"QUEUE:" {
        if count == 0 {
            return Ok(Vec::new());
        }
        return Ok(it.pull_line());
    }
    let i = match it.stream(&name, false) {
        Ok(i) => i,
        Err(_) => {
            it.notready(f, &name, None, b"")?;
            return Ok(Vec::new());
        }
    };
    if line == Some(1) {
        let s = &mut it.st.streams.list[i];
        if !s.persistent {
            return Err(Err::new(40));
        }
        s.rpos = 0;
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    match it.read_line(i) {
        Some(l) => Ok(l),
        None => {
            it.notready(f, &name, Some(i), b"EOF")?;
            Ok(Vec::new())
        }
    }
}

pub fn lineout(it: &mut Interp, f: &mut Frame, a: &[Option<Val>]) -> R<Val> {
    if a.len() > 3 {
        return Err(Err::new(40));
    }
    let name = out_name(a.first());
    let s = a.get(1).cloned().unwrap_or(None);
    let line = pos_arg(a.get(2).unwrap_or(&None), f.numeric.digits)?;
    if line.is_some_and(|l| l != 1) {
        return Err(Err::new(40));
    }
    if s.is_none() && line.is_none() {
        // close
        if std_handle(&upper(&name)).is_none() {
            it.close_stream(&name);
        }
        return Ok(b"0".to_vec());
    }
    let i = match it.stream(&name, true) {
        Ok(i) => i,
        Err(_) => {
            it.notready(f, &name, None, b"")?;
            return Ok(b"1".to_vec());
        }
    };
    if line == Some(1) {
        let st = &mut it.st.streams.list[i];
        if st.persistent {
            st.wpos = 0;
        }
    }
    if let Some(mut s) = s {
        s.extend_from_slice(b"\r\n");
        if it.put(i, &s).is_err() {
            it.notready(f, &name, Some(i), b"")?;
            return Ok(b"1".to_vec());
        }
    }
    Ok(b"0".to_vec())
}

pub fn lines(it: &mut Interp, f: &mut Frame, a: &[Option<Val>]) -> R<Val> {
    if a.len() > 1 {
        return Err(Err::new(40));
    }
    let _ = f;
    let name = name_arg(a.first());
    if upper(&name).as_slice() == b"QUEUE:" {
        let n = it.queued_count();
        return Ok(if n > 0 { b"1".to_vec() } else { b"0".to_vec() });
    }
    let Ok(i) = it.stream(&name, false) else { return Ok(b"0".to_vec()) };
    let s = &it.st.streams.list[i];
    if !s.persistent {
        let r = it.transient_ready(i);
        return Ok(if r { b"1".to_vec() } else { b"0".to_vec() });
    }
    let pos = s.rpos;
    let size = it.size_of(i).unwrap_or(0);
    if pos >= size {
        return Ok(b"0".to_vec());
    }
    // a lone Ctrl+Z at the end is no line
    if pos + 1 == size {
        let save = it.st.streams.list[i].rpos;
        let c = it.getc(i);
        it.st.streams.list[i].rpos = save;
        if c == Some(0x1a) {
            return Ok(b"0".to_vec());
        }
    }
    Ok(b"1".to_vec())
}

pub fn chars(it: &mut Interp, f: &mut Frame, a: &[Option<Val>]) -> R<Val> {
    if a.len() > 1 {
        return Err(Err::new(40));
    }
    let _ = f;
    let name = name_arg(a.first());
    let Ok(i) = it.stream(&name, false) else { return Ok(b"0".to_vec()) };
    let s = &it.st.streams.list[i];
    if !s.persistent {
        let r = it.transient_ready(i);
        return Ok(if r { b"1".to_vec() } else { b"0".to_vec() });
    }
    let pos = s.rpos;
    let size = it.size_of(i).unwrap_or(0);
    Ok(crate::num::int_str(size.saturating_sub(pos) as i64))
}

pub fn charin(it: &mut Interp, f: &mut Frame, a: &[Option<Val>]) -> R<Val> {
    if a.len() > 3 {
        return Err(Err::new(40));
    }
    let name = name_arg(a.first());
    let start = pos_arg(a.get(1).unwrap_or(&None), f.numeric.digits)?;
    let len = pos_arg(a.get(2).unwrap_or(&None), f.numeric.digits)?.unwrap_or(1);
    if len < 0 || start.is_some_and(|s| s < 1) {
        return Err(Err::new(40));
    }
    let i = match it.stream(&name, false) {
        Ok(i) => i,
        Err(_) => {
            it.notready(f, &name, None, b"")?;
            return Ok(Vec::new());
        }
    };
    if let Some(s) = start {
        let st = &mut it.st.streams.list[i];
        if !st.persistent {
            return Err(Err::new(40));
        }
        st.rpos = (s - 1) as u64;
    }
    let mut out = Vec::new();
    for _ in 0..len {
        match it.getc(i) {
            Some(c) => out.push(c),
            None => {
                it.notready(f, &name, Some(i), b"EOF")?;
                break;
            }
        }
    }
    Ok(out)
}

pub fn charout(it: &mut Interp, f: &mut Frame, a: &[Option<Val>]) -> R<Val> {
    if a.len() > 3 {
        return Err(Err::new(40));
    }
    let name = out_name(a.first());
    let s = a.get(1).cloned().unwrap_or(None);
    let start = pos_arg(a.get(2).unwrap_or(&None), f.numeric.digits)?;
    if start.is_some_and(|s| s < 1) {
        return Err(Err::new(40));
    }
    if s.is_none() && start.is_none() {
        if std_handle(&upper(&name)).is_none() {
            it.close_stream(&name);
        }
        return Ok(b"0".to_vec());
    }
    let i = match it.stream(&name, true) {
        Ok(i) => i,
        Err(_) => {
            it.notready(f, &name, None, b"")?;
            return Ok(crate::num::int_str(s.map_or(0, |s| s.len()) as i64));
        }
    };
    if let Some(p) = start {
        let st = &mut it.st.streams.list[i];
        if st.persistent {
            st.wpos = (p - 1) as u64;
        }
    }
    if let Some(s) = s {
        if it.put(i, &s).is_err() {
            it.notready(f, &name, Some(i), b"")?;
            return Ok(crate::num::int_str(s.len() as i64));
        }
    }
    Ok(b"0".to_vec())
}

pub fn stream_fn(it: &mut Interp, f: &mut Frame, a: &[Option<Val>]) -> R<Val> {
    if a.is_empty() || a.len() > 3 {
        return Err(Err::new(40));
    }
    let name = a[0].clone().ok_or(Err::new(40))?;
    let op = match a.get(1) {
        Some(Some(o)) if !o.is_empty() => o[0].to_ascii_uppercase(),
        Some(Some(_)) => return Err(Err::new(40)),
        _ => b'S',
    };
    let key = upper(&name);
    let state = |it: &Interp| it.st.streams.list.iter().find(|s| s.key == key).map(|s| (s.state, s.desc.clone()));
    match op {
        b'S' | b'D' => {
            if a.len() > 2 {
                return Err(Err::new(40));
            }
            let (s, d) = state(it).unwrap_or((SState::Unknown, Vec::new()));
            let mut v = match s {
                SState::Ready => b"READY".to_vec(),
                SState::NotReady => b"NOTREADY".to_vec(),
                SState::Error => b"ERROR".to_vec(),
                SState::Unknown => b"UNKNOWN".to_vec(),
            };
            if op == b'D' {
                v.push(b':');
                v.extend_from_slice(&d);
            }
            Ok(v)
        }
        b'C' => {
            let cmd = a.get(2).cloned().flatten().ok_or(Err::new(40))?;
            let cmdu = upper(&cmd);
            let w = crate::exec::words(&cmdu);
            let w0 = w.first().copied().unwrap_or(b"");
            match w0 {
                b"OPEN" => {
                    let mut mode = match w.get(1).copied() {
                        None => OpenMode::Both,
                        Some(b"READ") => OpenMode::Read,
                        Some(b"WRITE") => OpenMode::Write,
                        Some(b"BOTH") if it.oo() => OpenMode::Both,
                        _ if it.oo() => OpenMode::Both,
                        _ => return Err(Err::new(40)),
                    };
                    if it.oo() {
                        // Object REXX: READ|WRITE|BOTH [APPEND|REPLACE] and
                        // sharing and buffering options
                        for o in &w[1..] {
                            match *o {
                                b"READ" | b"WRITE" | b"BOTH" | b"APPEND" | b"SHARED" | b"SHAREREAD" | b"SHAREWRITE" | b"NOBUFFER" | b"BINARY" | b"SHARE" => {}
                                b"REPLACE" => {
                                    if mode != OpenMode::Read {
                                        mode = OpenMode::Replace;
                                    }
                                }
                                _ if o.iter().all(|c| c.is_ascii_digit()) => {}
                                b"RECLENGTH" => {}
                                _ => return Err(Err::new(40)),
                            }
                        }
                    } else if w.len() > 2 {
                        return Err(Err::new(40));
                    }
                    it.close_stream(&name);
                    match it.open_stream(&name, mode) {
                        Ok(_) => Ok(b"READY:".to_vec()),
                        Err(e) => Ok(alloc::format!("NOTREADY:{e}").into_bytes()),
                    }
                }
                b"CLOSE" => {
                    if it.close_stream(&name) { Ok(b"READY:".to_vec()) } else { Ok(Vec::new()) }
                }
                b"SEEK" => {
                    let rest = cmdu[4..].iter().skip_while(|&&c| c == b' ').copied().collect::<Vec<u8>>();
                    let (kind, num) = match rest.first() {
                        Some(b'=') | Some(b'<') | Some(b'+') | Some(b'-') => (rest[0], &rest[1..]),
                        _ => (b'=', &rest[..]),
                    };
                    let n = crate::exec::whole(crate::exec::strip_blanks(num), f.numeric.digits).map_err(|_| Err::new(40))?;
                    let Some(i) = it.st.streams.list.iter().position(|s| s.key == key) else {
                        return Ok(b"ERROR:".to_vec());
                    };
                    let size = it.size_of(i).unwrap_or(0) as i64;
                    let s = &mut it.st.streams.list[i];
                    if !s.persistent {
                        return Ok(b"ERROR:".to_vec());
                    }
                    let cur = s.rpos as i64;
                    let np = match kind {
                        b'=' => n - 1,
                        b'<' => size - n,
                        b'+' => cur + n,
                        _ => cur - n,
                    };
                    if np < 0 || np > size {
                        return Ok(b"ERROR:".to_vec());
                    }
                    s.rpos = np as u64;
                    s.wpos = np as u64;
                    s.ahead = None;
                    Ok(crate::num::int_str(np + 1))
                }
                b"QUERY" => {
                    let q = w.get(1).copied().unwrap_or(b"");
                    match q {
                        b"EXISTS" => Ok(it.host.f_exists(&name).unwrap_or_default()),
                        b"EXIST" if it.oo() => Ok(it.host.f_exists(&name).unwrap_or_default()),
                        b"SIZE" => Ok(it.host.f_info(&name).map(|i| crate::num::int_str(i.size as i64)).unwrap_or_default()),
                        b"DATETIME" => {
                            let d = it.host.f_info(&name).map(|i| i.datetime).unwrap_or_default();
                            // Object REXX: one blank between date and time
                            if it.oo() { Ok(crate::exec::words(&d).join(&b" "[..])) } else { Ok(d) }
                        }
                        _ => Err(Err::new(40)),
                    }
                }
                _ => Err(Err::new(40)),
            }
        }
        _ => Err(Err::new(40)),
    }
}

impl Interp<'_> {
    /// Whether a transient stream has data: devices always do; pipes and
    /// redirected input are read one character ahead (end of data: no).
    fn transient_ready(&mut self, i: usize) -> bool {
        let s = &self.st.streams.list[i];
        if s.ahead.is_some() {
            return true;
        }
        let h = s.h;
        if self.host.f_is_device(h) {
            return true;
        }
        let mut b = [0u8; 1];
        match self.host.f_read(h, &mut b) {
            Ok(1) => {
                self.st.streams.list[i].ahead = Some(b[0]);
                true
            }
            _ => false,
        }
    }

    pub fn queued_count(&mut self) -> usize {
        let mut st = core::mem::replace(&mut self.st, crate::exec::State::empty());
        let n = self.host.queued(&mut st);
        self.st = st;
        n
    }
}
