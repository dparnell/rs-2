//! MSG: message retrieval from MKMSGF message files and from message
//! segments bound into executables by MSGBIND.
//!
//! Message file layout: `FF "MKMSGF" 00`, component id (3 chars), message
//! count, first message number, index entry width flag, version, index
//! offset, extended header offset; each message starts with its class
//! character (E, W, I, H, P, or ? for an unused number). The version 2
//! extended header lists the code pages and the language of the file.
//!
//! Bound messages: the DosGetMessage stub from OS2286.LIB / OS2386.LIB
//! lives in a segment that starts with a header, `FF "MKMSGSEG" 00`
//! (16-bit) or `FF "MSGSEG32" 00` (32-bit), followed by a version byte at
//! +0x0A and the offset of the message data at +0x0E. The stub hands a
//! pointer to that header to DosTrueGetMessage. MSGBIND fills in the data
//! (for version 2 headers it moves the stub and the data into a segment
//! or object of their own). All offsets inside are 16-bit and relative to
//! the start of the header's 64K segment (the selector of a 16:16
//! pointer, the 64K tile of a flat one). The data, as DOSCALL1's RAMFIND
//! reads it:
//!
//! ```text
//! u16 n; u16 name[n]; u16 component[n]        names of bound message files
//! name:      "OSO001.MSG" 0                   (version 2: then u16, and a
//!            code page block: u16 next, the extended header of the file
//!            (+5 language, +7 sublanguage, +9 code page count, code pages),
//!            +0x2B the file name the block describes)
//! component: char id[3]; u16 count; u16 message[count]
//! message:   u16 number; u16 length; class char; text[length - 1]
//! ```
//!
//! DosGetMessage looks in the caller's bound messages first and then in
//! the message file: as named, along DPATH, in the root of the boot drive.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::sync::SpinLock;

/// Column width DosPutMessage wraps text to.
const PUT_WIDTH: usize = 80;

struct MsgFile {
    data: Arc<[u8]>,
    component: [u8; 3],
    first: u32,
    count: u32,
    offsets: Vec<u32>,
}

impl MsgFile {
    fn parse(data: Arc<[u8]>) -> Option<MsgFile> {
        if data.len() < 0x1f || &data[0..8] != b"\xffMKMSGF\x00" {
            return None;
        }
        let r16 = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]) as u32;
        let count = r16(11);
        let first = r16(13);
        let off16 = data[15] != 0;
        let idx = r16(18) as usize;
        let width = if off16 { 2 } else { 4 };
        if idx + count as usize * width > data.len() {
            return None;
        }
        let mut offsets = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let o = if off16 {
                r16(idx + i * 2)
            } else {
                let p = idx + i * 4;
                u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]])
            };
            offsets.push(o);
        }
        let mut component = [0u8; 3];
        component.copy_from_slice(&data[8..11]);
        Some(MsgFile { data: data.clone(), component, first, count, offsets })
    }

    /// (class, text) of a message.
    fn get(&self, num: u32) -> Option<(u8, &[u8])> {
        if num < self.first || num >= self.first + self.count {
            return None;
        }
        let i = (num - self.first) as usize;
        let start = self.offsets[i] as usize;
        let end = self.offsets.get(i + 1).map_or(self.data.len(), |&o| o as usize);
        if start >= end || end > self.data.len() {
            return None;
        }
        Some((self.data[start], &self.data[start + 1..end]))
    }

    /// The DosQueryMessageCP answer: code page count, code pages,
    /// language and sublanguage (version 2 files only).
    fn code_pages(&self) -> Result<Vec<u16>, u32> {
        let d = &self.data;
        let r16 = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        if r16(0x10) != Some(2) {
            return Err(err::MR_INV_MSGF_FORMAT);
        }
        let ext = r16(0x14).ok_or(err::MR_INV_MSGF_FORMAT)? as usize;
        let n = r16(ext + 7).ok_or(err::MR_UN_PERFORM)? as usize;
        let mut v = Vec::new();
        for i in 0..=n {
            v.push(r16(ext + 7 + 2 * i).ok_or(err::MR_UN_PERFORM)?);
        }
        v.push(r16(ext + 3).ok_or(err::MR_UN_PERFORM)?);
        v.push(r16(ext + 5).ok_or(err::MR_UN_PERFORM)?);
        Ok(v)
    }
}

static CACHE: SpinLock<BTreeMap<String, Arc<MsgFile>>> = SpinLock::new(BTreeMap::new());

/// The file name part of a path.
fn base_name(name: &str) -> &str {
    name.rsplit(['\\', '/', ':']).next().unwrap_or(name)
}

/// Find a message file: the name as given (relative to the current
/// directory), then the file name along DPATH and in the root of the boot
/// drive (DOSCALL1's FIND_FILE), then the system directories.
fn open(c: &Ctx, name: &str) -> Result<Arc<MsgFile>, u32> {
    let mut candidates: Vec<String> = Vec::new();
    candidates.push(String::from(name));
    let base = base_name(name);
    if let Some(dp) = super::doscalls::env_value(c, "DPATH") {
        for d in dp.split(';').filter(|s| !s.is_empty()) {
            candidates.push(alloc::format!("{}\\{}", d.trim_end_matches('\\'), base));
        }
    }
    candidates.push(alloc::format!("C:\\{base}"));
    candidates.push(alloc::format!("C:\\OS2\\SYSTEM\\{base}"));
    candidates.push(alloc::format!("C:\\OS2\\{base}"));
    let mut found_bad = false;
    for cand in candidates {
        let Ok(p) = super::file::resolve(c, &cand) else { continue };
        let key = p.display().to_ascii_uppercase();
        if let Some(m) = CACHE.lock().get(&key) {
            return Ok(m.clone());
        }
        let Some(data) = crate::fs::read_file(&p.display()) else { continue };
        if let Some(m) = MsgFile::parse(data) {
            let m = Arc::new(m);
            CACHE.lock().insert(key, m.clone());
            return Ok(m);
        }
        found_bad = true;
    }
    Err(if found_bad { err::MR_INV_MSGF_FORMAT } else { err::FILE_NOT_FOUND })
}

// ---- Bound message segments ----

/// A bound message segment in the caller's memory.
struct MsgSeg<'a> {
    c: &'a Ctx<'a>,
    /// Linear address that the 16-bit offsets are relative to.
    base: u32,
    version: u8,
    /// Offset of the data (the file name table).
    data: u16,
}

impl<'a> MsgSeg<'a> {
    /// Find the header for the msgseg argument (16:16 for 16-bit callers,
    /// flat for 32-bit ones). None when there is no usable bound data.
    fn new(c: &'a Ctx<'a>, raw: u32) -> Option<MsgSeg<'a>> {
        if raw == 0 {
            return None;
        }
        let (base, hdr) = if c.is16 {
            let base = c.far16_to_flat(raw & 0xffff_0000);
            (base, base.wrapping_add(raw & 0xffff))
        } else {
            (raw & !0xffff, raw)
        };
        let mut sig = [0u8; 10];
        if !c.read(hdr, &mut sig) || (&sig != b"\xffMKMSGSEG\0" && &sig != b"\xffMSGSEG32\0") {
            return None;
        }
        let version = c.read_u8(hdr + 0x0a)?;
        let data = c.read_u16(hdr + 0x0e)?;
        if data == 0 || version == 0 {
            return None;
        }
        Some(MsgSeg { c, base, version, data })
    }

    fn r16(&self, off: u32) -> Option<u16> {
        self.c.read_u16(self.base + (off & 0xffff))
    }

    fn name_matches(&self, off: u16, want: &[u8]) -> Option<bool> {
        for (i, &w) in want.iter().chain(core::iter::once(&0)).enumerate() {
            let b = self.c.read_u8(self.base + ((off as u32 + i as u32) & 0xffff))?;
            if !b.eq_ignore_ascii_case(&w) {
                return Some(false);
            }
        }
        Some(true)
    }

    /// Number of bound files (the names table), sanity-checked: the data
    /// pointer of an unbound stub points at whatever follows it.
    fn file_count(&self) -> Option<u16> {
        let n = self.r16(self.data as u32)?;
        (n > 0 && n <= 64).then_some(n)
    }

    /// FNSRCH: index of `name` in the names table and the offset just
    /// past the matching name's terminator.
    fn find_file(&self, name: &[u8]) -> Option<(u16, u16)> {
        let n = self.file_count()?;
        for i in 0..n {
            let off = self.r16(self.data as u32 + 2 + 2 * i as u32)?;
            if self.name_matches(off, name)? {
                return Some((i, off.wrapping_add(name.len() as u16 + 1)));
            }
        }
        None
    }

    /// Search component `index` of the table for a message.
    fn in_component(&self, index: u16, num: u32) -> Option<Found> {
        let n = self.file_count()?;
        let comp = self.r16(self.data as u32 + 2 + 2 * n as u32 + 2 * index as u32)? as u32;
        let mut component = [0u8; 3];
        for (i, b) in component.iter_mut().enumerate() {
            *b = self.c.read_u8(self.base + ((comp + i as u32) & 0xffff))?;
        }
        let count = self.r16(comp + 3)?;
        for j in 0..count as u32 {
            let rec = self.r16(comp + 5 + 2 * j)? as u32;
            if self.r16(rec)? as u32 != num {
                continue;
            }
            let len = self.r16(rec + 2)? as u32;
            let class = self.c.read_u8(self.base + ((rec + 4) & 0xffff))?;
            let text = self.c.read_vec(self.base + ((rec + 5) & 0xffff), len.saturating_sub(1))?;
            return Some(Found { component, class, text });
        }
        None
    }

    /// The code page list of a version 2 block, if `cp` is in it (an
    /// empty list matches any code page).
    fn block_has_cp(&self, block: u32, cp: u16) -> Option<bool> {
        if cp == 0 {
            return Some(true);
        }
        let n = self.r16(block + 9)?;
        if n == 0 {
            return Some(true);
        }
        for i in 1..=n as u32 {
            if self.r16(block + 9 + 2 * i)? == cp {
                return Some(true);
            }
        }
        Some(false)
    }

    /// RAMFIND: a bound message of file `name`.
    fn find(&self, name: &[u8], num: u32, cp: u16) -> Option<Found> {
        let (index, past) = self.find_file(name)?;
        if self.version < 2 {
            return self.in_component(index, num);
        }
        // Version 2: a chain of code page blocks; the first describes the
        // named file itself, later ones name alternative files.
        let mut block = past.wrapping_add(2) as u32;
        let mut first = true;
        for _ in 0..16 {
            if self.block_has_cp(block, cp)? {
                let found = if first {
                    self.in_component(index, num)
                } else {
                    let mut alt = Vec::new();
                    let mut a = block + 0x2b;
                    while alt.len() < 13 {
                        let b = self.c.read_u8(self.base + (a & 0xffff))?;
                        if b == 0 {
                            break;
                        }
                        alt.push(b);
                        a += 1;
                    }
                    self.find_file(&alt).and_then(|(i, _)| self.in_component(i, num))
                };
                if found.is_some() {
                    return found;
                }
            }
            first = false;
            block = self.r16(block)? as u32;
            if block == 0 {
                return None;
            }
        }
        None
    }

    /// TRY_RAM of DosQueryMessageCP (version 2 only).
    fn code_pages(&self, name: &[u8]) -> Option<Vec<u16>> {
        if self.version < 2 {
            return None;
        }
        let (_, past) = self.find_file(name)?;
        let block = past.wrapping_add(2) as u32;
        let n = self.r16(block + 9)?;
        let mut v = Vec::new();
        for i in 0..=n as u32 {
            v.push(self.r16(block + 9 + 2 * i)?);
        }
        v.push(self.r16(block + 5)?);
        v.push(self.r16(block + 7)?);
        Some(v)
    }
}

struct Found {
    component: [u8; 3],
    class: u8,
    text: Vec<u8>,
}

// ---- Insertion ----

/// DosInsMessage's insertion: %1..%9 are replaced by the insertion
/// strings when that many were given; anything else after % (including
/// %0, which MKMSGF consumes when it builds the file) stays as it is. When
/// the result does not fit in `cb` bytes it is cut and its last two bytes
/// become CR LF (MR_MSG_TOO_LONG).
pub fn insert(text: &[u8], ins: &[Vec<u8>], cb: usize) -> (u32, Vec<u8>) {
    if ins.len() > 9 {
        return (err::MR_INV_IVCOUNT, Vec::new());
    }
    let mut out = Vec::with_capacity(text.len() + 32);
    let mut i = 0;
    let overflow = |mut out: Vec<u8>| {
        out.truncate(cb);
        if cb >= 2 {
            out[cb - 2] = b'\r';
            out[cb - 1] = b'\n';
        }
        (err::MR_MSG_TOO_LONG, out)
    };
    while i < text.len() {
        if out.len() >= cb {
            return overflow(out);
        }
        let b = text[i];
        if b == b'%' && i + 1 < text.len() {
            let d = text[i + 1];
            if (b'1'..=b'9').contains(&d) && ((d - b'0') as usize) <= ins.len() {
                for &ch in &ins[(d - b'1') as usize] {
                    if out.len() >= cb {
                        return overflow(out);
                    }
                    out.push(ch);
                }
                i += 2;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    (0, out)
}

// ---- Messages of the message retriever itself ----

/// Texts for the retriever's own errors (DOSCALL1 carries them bound).
fn own_text(num: u32) -> &'static [u8] {
    match num {
        316 => b"The message passed to the message retriever was truncated.\r\n",
        317 => b"The system cannot find message %1\r\nin message file %2.\r\n",
        318 => b"Message file %2 cannot be found for message %1.\r\n",
        319 => b"The system cannot read message file %1.\r\n",
        320 => b"A programming error occurred when using the message retriever.\r\n",
        _ => b"The system cannot display the message.\r\n",
    }
}

/// The message DosGetMessage returns in place of the requested one after
/// error `e` (DOSCALL1's error path): `SYS0318: ...` with the message
/// number (as " nnnn") and the file name inserted.
fn error_message(e: u32, num: u32, file: &str, cb: usize) -> Vec<u8> {
    let e = if e == err::FILE_NOT_FOUND { err::MR_UN_ACC_MSGF } else { e };
    let numstr = alloc::format!(" {:04}", num % 10000).into_bytes();
    let file = Vec::from(file.as_bytes());
    let ins: Vec<Vec<u8>> = match e {
        317 | 318 => alloc::vec![numstr, file],
        319 => alloc::vec![file],
        _ => Vec::new(),
    };
    let prefix = alloc::format!("SYS{:04}: ", e).into_bytes();
    let (pe, mut out) = insert(&prefix, &[], cb);
    if pe != 0 {
        return out;
    }
    let (_, text) = insert(own_text(e), &ins, cb - out.len());
    out.extend_from_slice(&text);
    out
}

/// The calling process's current code page.
fn process_cp(c: &Ctx) -> u16 {
    crate::nls::process_cp(c.process.pid)
}

/// Build a message; returns (OS/2 error code, text).
fn get_message(c: &Ctx, ins: &[Vec<u8>], num: u32, file: &str, msgseg: u32, cb: usize) -> (u32, Vec<u8>) {
    if ins.len() > 9 {
        return (err::MR_INV_IVCOUNT, error_message(err::MR_INV_IVCOUNT, num, file, cb));
    }
    let bound = MsgSeg::new(c, msgseg).and_then(|s| s.find(base_name(file).as_bytes(), num, process_cp(c)));
    let found = match bound {
        Some(f) => Ok(f),
        None => open(c, file).and_then(|mf| match mf.get(num) {
            Some((class, text)) => Ok(Found { component: mf.component, class, text: Vec::from(text) }),
            None => Err(err::MR_MID_NOT_FOUND),
        }),
    };
    let mut f = match found {
        Ok(f) if f.class != b'?' => f,
        Ok(_) => return (err::MR_MID_NOT_FOUND, error_message(err::MR_MID_NOT_FOUND, num, file, cb)),
        Err(e) => return (e, error_message(e, num, file, cb)),
    };
    // A trailing NUL or end-of-file byte is not part of the text.
    if matches!(f.text.last(), Some(0) | Some(0x1a)) {
        f.text.pop();
    }
    let mut out = Vec::new();
    if matches!(f.class, b'E' | b'W') {
        let comp: String = f.component.iter().map(|&b| b as char).collect();
        let prefix = alloc::format!("{comp}{:04}: ", num % 10000).into_bytes();
        let (e, p) = insert(&prefix, &[], cb);
        if e != 0 {
            return (e, p);
        }
        out = p;
    }
    let (e, text) = insert(&f.text, ins, cb - out.len());
    if e == err::MR_INV_IVCOUNT {
        return (e, error_message(e, num, file, cb));
    }
    out.extend_from_slice(&text);
    (e, out)
}

/// Read an insertion table of `count` pointers.
fn read_table(c: &Ctx, table: u32, count: u32) -> Option<Vec<Vec<u8>>> {
    let mut v = Vec::new();
    for i in 0..count.min(9) {
        let raw = c.read_u32(table + i * 4)?;
        let p = if c.is16 { c.far16_to_flat(raw) } else { raw };
        // DOSCALL1 skips entries whose offset is 0xFFFF.
        if p == 0 || raw & 0xffff == 0xffff {
            v.push(Vec::new());
            continue;
        }
        let s = c.read_str(p, 1024)?;
        v.push(s.into_bytes());
    }
    Some(v)
}

fn write_count(c: &Ctx, pcb: u32, n: u32) {
    if c.is16 {
        c.write_u16(pcb, n as u16);
    } else {
        c.write_u32(pcb, n);
    }
}

/// DosTrueGetMessage (16-bit MSG.2 and 32-bit MSG.6).
pub fn dos_true_get_message(c: &mut Ctx) -> u32 {
    // The 32-bit entry gets the bound message segment first (the
    // DosGetMessage library stub pushes it in front of the arguments and
    // the API pops it: see Ctx::pop32); the 16-bit one gets it last.
    let (msgseg, table, count, buf, cb, num, pfile, pcb) = if c.is16 {
        (c.arg(7), c.ptr(0), c.arg(1), c.ptr(2), c.arg(3), c.arg(4), c.ptr(5), c.ptr(6))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4), c.arg(5), c.arg(6), c.arg(7))
    };
    let Some(file) = c.read_str(pfile, 260) else { return err::INVALID_ADDRESS };
    let Some(ins) = read_table(c, table, count) else { return err::INVALID_ADDRESS };
    if super::tracing() {
        crate::trace!("    message {num} from {file} (msgseg {msgseg:#x})");
    }
    let (e, text) = if count > 9 {
        (err::MR_INV_IVCOUNT, error_message(err::MR_INV_IVCOUNT, num, &file, cb as usize))
    } else {
        get_message(c, &ins, num, &file, msgseg, cb as usize)
    };
    c.write(buf, &text);
    write_count(c, pcb, text.len() as u32);
    e
}

/// DosInsMessage / DosInsertMessage.
pub fn dos_ins_message(c: &mut Ctx) -> u32 {
    let (table, count, pmsg, cbmsg, buf, cb, pcb) = if c.is16 {
        (c.ptr(0), c.arg(1), c.ptr(2), c.arg(3), c.ptr(4), c.arg(5), c.ptr(6))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4), c.arg(5), c.arg(6))
    };
    if count > 9 {
        return err::MR_INV_IVCOUNT;
    }
    let Some(ins) = read_table(c, table, count) else { return err::INVALID_ADDRESS };
    let Some(msg) = c.read_vec(pmsg, cbmsg) else { return err::INVALID_ADDRESS };
    let (e, text) = insert(&msg, &ins, cb as usize);
    c.write(buf, &text);
    write_count(c, pcb, text.len() as u32);
    e
}

/// DosPutMessage's line breaking (SBCS): text is written in lines of at
/// most 80 columns, broken between words; a word that does not fit on the
/// current line starts a new one (CR LF, or a blank when one column is
/// left), blanks at the start of a continuation line are dropped, and a
/// word longer than a line is split after 78 characters. Returns the
/// pieces in the order DOSCALL1 writes them.
pub fn wrap(msg: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    let mut col = PUT_WIDTH;
    let mut i = 0;
    let n = msg.len();
    let at = |j: usize| msg.get(j).copied().unwrap_or(0);
    let flush = |line: &mut Vec<u8>, out: &mut Vec<Vec<u8>>| {
        if !line.is_empty() {
            out.push(core::mem::take(line));
        }
    };
    while i < n {
        // Measure the next token: a run of blanks or a word, with the line
        // end that follows it. `last` = the token ends the line or text.
        let blanks = at(i) == b' ';
        let mut len = 0;
        let mut j = i;
        let mut last = false;
        loop {
            if j >= n {
                last = true;
                break;
            }
            let ch = at(j);
            let stop = if blanks { ch != b' ' } else { ch == b' ' || ch == b'\r' };
            if stop {
                break;
            }
            len += 1;
            j += 1;
        }
        if !last && at(j) == b'\r' {
            last = true;
            len += 1;
            if n - j >= 2 && at(j + 1) == b'\n' {
                len += 1;
            }
        }
        let fits = if last { col >= len } else { col >= len + 2 };
        let copy = |len: usize, col: &mut usize, i: &mut usize, line: &mut Vec<u8>, out: &mut Vec<Vec<u8>>| {
            let len = if *col >= len { len } else { PUT_WIDTH - 2 };
            let len = len.min(n - *i);
            *col -= len;
            line.extend_from_slice(&msg[*i..*i + len]);
            *i += len;
            if *i >= n {
                flush(line, out);
            }
        };
        if fits {
            copy(len, &mut col, &mut i, &mut line, &mut out);
            if last && i < n {
                flush(&mut line, &mut out);
                col = PUT_WIDTH;
            }
            continue;
        }
        // Start a new line.
        if col != PUT_WIDTH {
            if col > 1 {
                line.extend_from_slice(b"\r\n");
            } else if col == 1 {
                line.push(b' ');
            }
            flush(&mut line, &mut out);
            col = PUT_WIDTH;
        }
        // Drop blanks (and a line end after them) at the line start.
        if at(i) != b'\r' {
            while i < n && at(i) == b' ' {
                i += 1;
            }
            if i < n && at(i) == b'\r' {
                i += 1;
                if i < n && at(i) == b'\n' {
                    i += 1;
                }
            }
        }
        if !blanks {
            copy(len, &mut col, &mut i, &mut line, &mut out);
        }
    }
    flush(&mut line, &mut out);
    out
}

/// Write kernel bytes to a handle through a scratch area below the
/// caller's stack pointer (the thread is in the kernel, so that part of
/// its stack is free).
fn write_bytes(c: &Ctx, hf: u32, data: &[u8]) -> u32 {
    let sp = if c.is16 { c.f.rsp as u32 & 0xffff } else { c.f.rsp as u32 };
    if sp < 0x200 {
        return err::MR_UN_PERFORM;
    }
    let Some(lin) = c.space().linear(c.f.ss as u16, sp - 0x180) else { return err::MR_UN_PERFORM };
    for chunk in data.chunks(0x100) {
        if !c.write(lin, chunk) {
            return err::MR_UN_PERFORM;
        }
        let (e, n) = super::file::write_handle(c, hf, lin, chunk.len() as u32);
        if e != 0 {
            return if e == err::INVALID_HANDLE || e == 19 { e } else { err::MR_UN_PERFORM };
        }
        if n as usize != chunk.len() {
            return err::MR_UN_PERFORM;
        }
    }
    0
}

/// DosPutMessage: write a message to a handle, wrapped to 80 columns.
pub fn dos_put_message(c: &mut Ctx) -> u32 {
    let (hf, cb, p) = if c.is16 { (c.arg(0), c.arg(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    if cb == 0 {
        return 0;
    }
    let Some(msg) = c.read_vec(p, cb) else { return err::INVALID_ADDRESS };
    if crate::nls::is_dbcs(process_cp(c)) {
        // No line breaking inside double-byte text.
        let (e, _) = super::file::write_handle(c, hf, p, cb);
        return e;
    }
    for piece in wrap(&msg) {
        let e = write_bytes(c, hf, &piece);
        if e != 0 {
            return e;
        }
    }
    0
}

/// DosIQueryMessageCP(pb, cb, pszFile, pcbBuf, pMsgSeg): the code pages,
/// language and sublanguage of a message file, as words. The library
/// stubs append the bound message segment as the last argument (the
/// 32-bit stub shifts the caller's arguments and returns with a plain
/// RET, so nothing extra is popped).
pub fn dos_query_message_cp(c: &mut Ctx) -> u32 {
    let (buf, cb, pfile, pcb, msgseg) = if c.is16 {
        (c.ptr(0), c.arg(1), c.ptr(2), c.ptr(3), c.arg(4))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4))
    };
    let Some(file) = c.read_str(pfile, 260) else { return err::INVALID_ADDRESS };
    let ram = MsgSeg::new(c, msgseg).and_then(|s| s.code_pages(base_name(&file).as_bytes()));
    let words = match ram {
        Some(w) => w,
        None => match open(c, &file).and_then(|m| m.code_pages()) {
            Ok(w) => w,
            Err(e) => {
                write_count(c, pcb, 0);
                return e;
            }
        },
    };
    let mut b = Vec::new();
    for w in words {
        b.extend_from_slice(&w.to_le_bytes());
    }
    let n = b.len().min(cb as usize);
    c.write(buf, &b[..n]);
    write_count(c, pcb, n as u32);
    if n < b.len() { err::BUFFER_OVERFLOW } else { 0 }
}
