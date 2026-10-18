//! MSG: message retrieval from MKMSGF message files.
//!
//! File layout: `FF "MKMSGF" 00`, component id (3 chars), message count,
//! first message number, index entry width flag, version, index offset;
//! each message starts with its class character (E, W, I, H, P, ?).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{err, Ctx};
use crate::sync::SpinLock;

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
}

static CACHE: SpinLock<BTreeMap<String, Arc<MsgFile>>> = SpinLock::new(BTreeMap::new());

/// Find a message file: an explicit path, else along DPATH and the
/// system directories.
fn open(c: &Ctx, name: &str) -> Option<Arc<MsgFile>> {
    let mut candidates: Vec<String> = Vec::new();
    if name.contains(['\\', ':', '/']) {
        candidates.push(String::from(name));
    } else {
        if let Some(dp) = super::doscalls::env_value(c, "DPATH") {
            for d in dp.split(';').filter(|s| !s.is_empty()) {
                candidates.push(alloc::format!("{}\\{}", d.trim_end_matches('\\'), name));
            }
        }
        candidates.push(alloc::format!("C:\\OS2\\SYSTEM\\{name}"));
        candidates.push(alloc::format!("C:\\OS2\\{name}"));
        candidates.push(String::from(name));
    }
    for cand in candidates {
        let Ok(p) = super::file::resolve(c, &cand) else { continue };
        let key = p.display().to_ascii_uppercase();
        if let Some(m) = CACHE.lock().get(&key) {
            return Some(m.clone());
        }
        let Some(data) = crate::fs::read_file(&p.display()) else { continue };
        if let Some(m) = MsgFile::parse(data) {
            let m = Arc::new(m);
            CACHE.lock().insert(key, m.clone());
            return Some(m);
        }
    }
    None
}

/// Replace %1..%9 with insertion strings; "%0" ends the message without a
/// line end. Returns the text and whether an invalid insertion was used.
fn insert(text: &[u8], ins: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 32);
    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        if b == b'%' && i + 1 < text.len() {
            let d = text[i + 1];
            if d == b'0' {
                // Prompt: drop the rest (normally the trailing CR LF).
                break;
            }
            if (b'1'..=b'9').contains(&d) {
                let k = (d - b'1') as usize;
                if let Some(s) = ins.get(k) {
                    out.extend_from_slice(s);
                } else {
                    out.extend_from_slice(&text[i..i + 2]);
                }
                i += 2;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    out
}

/// Build a message; returns (OS/2 error code, text).
fn get_message(c: &Ctx, ins: &[Vec<u8>], num: u32, file: &str) -> (u32, Vec<u8>) {
    if ins.len() > 9 {
        return (err::MR_INV_IVCOUNT, Vec::new());
    }
    let Some(mf) = open(c, file) else {
        let t = alloc::format!("SYS0318: Message file {} cannot be found.\r\n", file.to_ascii_uppercase());
        return (err::MR_UN_ACC_MSGF, t.into_bytes());
    };
    let Some((class, text)) = mf.get(num) else {
        let t = alloc::format!(
            "SYS0317: The system cannot find message {:04} in message file {}.\r\n",
            num,
            file.to_ascii_uppercase()
        );
        return (err::MR_MID_NOT_FOUND, t.into_bytes());
    };
    let mut out = Vec::new();
    if matches!(class, b'E' | b'W') {
        let comp: String = mf.component.iter().map(|&b| b as char).collect();
        out.extend_from_slice(alloc::format!("{comp}{num:04}: ").as_bytes());
    }
    out.extend_from_slice(&insert(text, ins));
    (0, out)
}

/// Read an insertion table of `count` pointers.
fn read_table(c: &Ctx, table: u32, count: u32) -> Option<Vec<Vec<u8>>> {
    let mut v = Vec::new();
    for i in 0..count.min(9) {
        let raw = c.read_u32(table + i * 4)?;
        let p = if c.is16 { c.far16_to_flat(raw) } else { raw };
        let s = c.read_str(p, 1024)?;
        v.push(s.into_bytes());
    }
    Some(v)
}

fn deliver(c: &Ctx, text: &[u8], buf: u32, cb: u32) -> (u32, u32) {
    let n = text.len().min(cb as usize);
    c.write(buf, &text[..n]);
    (if n < text.len() { err::MR_MSG_TOO_LONG } else { 0 }, n as u32)
}

/// DosTrueGetMessage (16-bit MSG.2 and 32-bit MSG.6).
pub fn dos_true_get_message(c: &mut Ctx) -> u32 {
    let (table, count, buf, cb, num, pfile, pcb) = if c.is16 {
        (c.ptr(0), c.arg(1), c.ptr(2), c.arg(3), c.arg(4), c.ptr(5), c.ptr(6))
    } else {
        (c.arg(0), c.arg(1), c.arg(2), c.arg(3), c.arg(4), c.arg(5), c.arg(6))
    };
    let Some(file) = c.read_str(pfile, 260) else { return err::INVALID_ADDRESS };
    let Some(ins) = read_table(c, table, count) else { return err::INVALID_ADDRESS };
    if super::tracing() {
        crate::trace!("    message {num} from {file}");
    }
    let (e, text) = get_message(c, &ins, num, &file);
    let (e2, n) = deliver(c, &text, buf, cb);
    if c.is16 { c.write_u16(pcb, n as u16) } else { c.write_u32(pcb, n) };
    if e != 0 { e } else { e2 }
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
    let text = insert(&msg, &ins);
    let (e, n) = deliver(c, &text, buf, cb);
    if c.is16 { c.write_u16(pcb, n as u16) } else { c.write_u32(pcb, n) };
    e
}

/// DosPutMessage: write a message to a handle.
pub fn dos_put_message(c: &mut Ctx) -> u32 {
    let (hf, cb, p) = if c.is16 { (c.arg(0), c.arg(1), c.ptr(2)) } else { (c.arg(0), c.arg(1), c.arg(2)) };
    let (e, _) = super::file::write_handle(c, hf, p, cb);
    e
}

pub fn dos_query_message_cp(c: &mut Ctx) -> u32 {
    // DosIQueryMessageCP(pb, cb, pszFile, pcbBuf, pMsgSeg): list of code
    // pages the message file supports (we report the current one).
    let (buf, cb, pcb) = if c.is16 { (c.ptr(0), c.arg(1), c.ptr(3)) } else { (c.arg(0), c.arg(1), c.arg(3)) };
    // Format: count, codepages..., language id, sublanguage id.
    let words = [1u16, 850, 1, 0];
    let mut b = Vec::new();
    for w in words {
        b.extend_from_slice(&w.to_le_bytes());
    }
    let n = b.len().min(cb as usize);
    c.write(buf, &b[..n]);
    if c.is16 { c.write_u16(pcb, n as u16) } else { c.write_u32(pcb, n as u32) };
    0
}
