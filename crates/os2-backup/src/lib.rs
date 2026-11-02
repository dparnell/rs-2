//! OS/2 BACKUP sets: the format IBM's BACKUP and RESTORE (OS/2 Warp 4.5,
//! the JFS-aware `BKUPJFS` version) write and read.
//!
//! A backup on a disk is the directory `\BACKUP` holding, per backup disk
//! `nnn` (001 for a hard disk target), a data file `BACKUP.nnn` (the file
//! contents and extended attributes, one after another) and a control file
//! `CONTROL.nnn` describing them. The backup date is the control file's
//! own date.
//!
//! ```text
//! header (139 bytes)
//!   +0x00 u8   0x8B (header length)
//!   +0x01      "BKUPJFS "
//!   +0x09 u8   disk sequence number (1 = first)
//!   +0x8A u8   0xFF on the last disk of the set, else 0
//! directory record (one per directory, followed by its file records)
//!   +0x00 u16  record length (0x20 + path length)
//!   +0x02 u16  number of file records that follow
//!   +0x04 u64  offset of the next directory record in this control
//!              file, all ones for the last
//!   +0x0C u8   0xD4
//!   +0x0D u64  offset of the directory's EAs in the data file
//!   +0x15 u64  their length (0 when the directory has none)
//!   +0x1D u16  path length
//!   +0x1F      path from the root without the leading '\' ("" for the
//!              root, "OS2\DLL"), then 0
//! file record
//!   +0x00 u16  record length (0x36 + name length)
//!   +0x02 u8   flags: 1 last part of the file, 2 backed up completely
//!   +0x03 u64  file size
//!   +0x0B u16  sequence number of the part (disk)
//!   +0x0D u64  offset of this part's data in the data file
//!   +0x15 u64  length of this part
//!   +0x1D u16  attributes
//!   +0x1F u16  time of last write (FAT format)
//!   +0x21 u16  date of last write
//!   +0x23 u64  offset of the file's EAs in the data file (before its data)
//!   +0x2B u64  their length (0: none)
//!   +0x33 u16  name length
//!   +0x35      name, then 0
//! ```
//!
//! EAs are stored as a 16-bit FEALIST: `u32 cbList`, then entries
//! `u8 fEA, u8 cbName, u16 cbValue, name, 0, value`; IBM's BACKUP stores
//! none when the list is shorter than 5 bytes.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub const HEADER_LEN: usize = 0x8B;
pub const SIGNATURE: &[u8; 8] = b"BKUPJFS ";
/// Older (OS/2 2.x-4.0) control files: same records, this signature.
pub const SIGNATURE_OLD: &[u8; 8] = b"BACKUP  ";
pub const DIR_MARK: u8 = 0xD4;
pub const FLAG_LAST_PART: u8 = 1;
pub const FLAG_COMPLETE: u8 = 2;
const DIR_FIXED: usize = 0x1F;
const FILE_FIXED: usize = 0x35;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Error {
    /// Not a control file.
    NotBackup,
    /// A record runs past the end or has a bad length.
    Corrupt,
}

pub type Result<T> = core::result::Result<T, Error>;

fn u16at(d: &[u8], o: usize) -> Result<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or(Error::Corrupt)
}

fn u64at(d: &[u8], o: usize) -> Result<u64> {
    let b = d.get(o..o + 8).ok_or(Error::Corrupt)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(b);
    Ok(u64::from_le_bytes(a))
}

fn text(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Header {
    /// Disk sequence number, from 1.
    pub sequence: u8,
    /// The last disk of the set.
    pub last: bool,
}

impl Header {
    pub fn parse(d: &[u8]) -> Result<Header> {
        if d.len() < HEADER_LEN || d[0] as usize != HEADER_LEN || (&d[1..9] != SIGNATURE && &d[1..9] != SIGNATURE_OLD) {
            return Err(Error::NotBackup);
        }
        Ok(Header { sequence: d[9], last: d[0x8A] == 0xFF })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        let mut h = [0u8; HEADER_LEN];
        h[0] = HEADER_LEN as u8;
        h[1..9].copy_from_slice(SIGNATURE);
        h[9] = self.sequence;
        h[0x8A] = if self.last { 0xFF } else { 0 };
        out.extend_from_slice(&h);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DirRecord {
    /// Path from the root without the leading backslash.
    pub path: String,
    pub files: u16,
    /// Offset of the next directory record, `None` for the last.
    pub next: Option<u64>,
    pub ea_offset: u64,
    pub ea_len: u64,
}

impl DirRecord {
    pub fn len(&self) -> usize {
        DIR_FIXED + self.path.len() + 1
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        let p = bytes(&self.path);
        out.extend_from_slice(&(self.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.files.to_le_bytes());
        out.extend_from_slice(&self.next.unwrap_or(u64::MAX).to_le_bytes());
        out.push(DIR_MARK);
        out.extend_from_slice(&self.ea_offset.to_le_bytes());
        out.extend_from_slice(&self.ea_len.to_le_bytes());
        out.extend_from_slice(&(p.len() as u16).to_le_bytes());
        out.extend_from_slice(&p);
        out.push(0);
    }

    fn parse(d: &[u8]) -> Result<DirRecord> {
        let n = u16at(d, 0x1D)? as usize;
        let p = d.get(DIR_FIXED..DIR_FIXED + n).ok_or(Error::Corrupt)?;
        let next = u64at(d, 4)?;
        Ok(DirRecord {
            path: text(p),
            files: u16at(d, 2)?,
            next: (next != u64::MAX).then_some(next),
            ea_offset: u64at(d, 0x0D)?,
            ea_len: u64at(d, 0x15)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileRecord {
    pub name: String,
    pub flags: u8,
    pub size: u64,
    pub part: u16,
    pub offset: u64,
    pub part_len: u64,
    pub attr: u16,
    pub time: u16,
    pub date: u16,
    pub ea_offset: u64,
    pub ea_len: u64,
}

impl FileRecord {
    pub fn len(&self) -> usize {
        FILE_FIXED + self.name.len() + 1
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        let n = bytes(&self.name);
        out.extend_from_slice(&(self.len() as u16).to_le_bytes());
        out.push(self.flags);
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&self.part.to_le_bytes());
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.extend_from_slice(&self.part_len.to_le_bytes());
        out.extend_from_slice(&self.attr.to_le_bytes());
        out.extend_from_slice(&self.time.to_le_bytes());
        out.extend_from_slice(&self.date.to_le_bytes());
        out.extend_from_slice(&self.ea_offset.to_le_bytes());
        out.extend_from_slice(&self.ea_len.to_le_bytes());
        out.extend_from_slice(&(n.len() as u16).to_le_bytes());
        out.extend_from_slice(&n);
        out.push(0);
    }

    fn parse(d: &[u8]) -> Result<FileRecord> {
        let n = u16at(d, 0x33)? as usize;
        let name = d.get(FILE_FIXED..FILE_FIXED + n).ok_or(Error::Corrupt)?;
        Ok(FileRecord {
            name: text(name),
            flags: *d.get(2).ok_or(Error::Corrupt)?,
            size: u64at(d, 3)?,
            part: u16at(d, 0x0B)?,
            offset: u64at(d, 0x0D)?,
            part_len: u64at(d, 0x15)?,
            attr: u16at(d, 0x1D)?,
            time: u16at(d, 0x1F)?,
            date: u16at(d, 0x21)?,
            ea_offset: u64at(d, 0x23)?,
            ea_len: u64at(d, 0x2B)?,
        })
    }
}

/// A parsed control file: the header and its directories, each with its
/// files, in file order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Control {
    pub header: Header,
    pub dirs: Vec<(DirRecord, Vec<FileRecord>)>,
}

impl Control {
    pub fn parse(d: &[u8]) -> Result<Control> {
        let header = Header::parse(d)?;
        let mut dirs = Vec::new();
        let mut at = HEADER_LEN;
        while at + 2 <= d.len() {
            let len = u16at(d, at)? as usize;
            if len < DIR_FIXED + 1 || at + len > d.len() {
                return Err(Error::Corrupt);
            }
            let dir = DirRecord::parse(&d[at..at + len])?;
            at += len;
            let mut files = Vec::new();
            for _ in 0..dir.files {
                let len = u16at(d, at)? as usize;
                if len < FILE_FIXED + 1 || at + len > d.len() {
                    return Err(Error::Corrupt);
                }
                files.push(FileRecord::parse(&d[at..at + len])?);
                at += len;
            }
            dirs.push((dir, files));
        }
        Ok(Control { header, dirs })
    }

    /// Serialise, linking the directory records in order.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.header.write(&mut out);
        let mut at = HEADER_LEN as u64;
        let n = self.dirs.len();
        for (i, (d, files)) in self.dirs.iter().enumerate() {
            let size = d.len() as u64 + files.iter().map(|f| f.len() as u64).sum::<u64>();
            let mut d = d.clone();
            d.files = files.len() as u16;
            d.next = (i + 1 < n).then_some(at + size);
            d.write(&mut out);
            for f in files {
                f.write(&mut out);
            }
            at += size;
        }
        out
    }
}

/// One extended attribute: flags, name, value (including its EAT_ type
/// word).
#[derive(Debug, Clone, PartialEq)]
pub struct Ea {
    pub flags: u8,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

/// Encode EAs as a 16-bit FEALIST (empty: nothing at all).
pub fn fealist(eas: &[Ea]) -> Vec<u8> {
    if eas.is_empty() {
        return Vec::new();
    }
    let mut v = alloc::vec![0u8; 4];
    for e in eas {
        v.push(e.flags);
        v.push(e.name.len() as u8);
        v.extend_from_slice(&(e.value.len() as u16).to_le_bytes());
        v.extend_from_slice(&e.name);
        v.push(0);
        v.extend_from_slice(&e.value);
    }
    let n = v.len() as u32;
    v[0..4].copy_from_slice(&n.to_le_bytes());
    v
}

/// Decode a 16-bit FEALIST.
pub fn parse_fealist(d: &[u8]) -> Result<Vec<Ea>> {
    let mut v = Vec::new();
    if d.len() < 4 {
        return Ok(v);
    }
    let total = (u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize).min(d.len());
    let mut at = 4;
    while at + 4 <= total {
        let flags = d[at];
        let nl = d[at + 1] as usize;
        let vl = u16at(d, at + 2)? as usize;
        let name = d.get(at + 4..at + 4 + nl).ok_or(Error::Corrupt)?.to_vec();
        let value = d.get(at + 5 + nl..at + 5 + nl + vl).ok_or(Error::Corrupt)?.to_vec();
        v.push(Ea { flags, name, value });
        at += 5 + nl + vl;
    }
    Ok(v)
}

/// Lay out a 32-bit FEA2LIST (for DosSetPathInfo level 2) from EAs.
pub fn fea2list(eas: &[Ea]) -> Vec<u8> {
    let mut v = alloc::vec![0u8; 4];
    let n = eas.len();
    for (i, e) in eas.iter().enumerate() {
        let start = v.len();
        v.extend_from_slice(&[0u8; 4]);
        v.push(e.flags);
        v.push(e.name.len() as u8);
        v.extend_from_slice(&(e.value.len() as u16).to_le_bytes());
        v.extend_from_slice(&e.name);
        v.push(0);
        v.extend_from_slice(&e.value);
        while (v.len() - start) % 4 != 0 {
            v.push(0);
        }
        if i + 1 < n {
            let next = (v.len() - start) as u32;
            v[start..start + 4].copy_from_slice(&next.to_le_bytes());
        }
    }
    let total = v.len() as u32;
    v[0..4].copy_from_slice(&total.to_le_bytes());
    v
}

/// Parse a 32-bit FEA2LIST (DosQueryPathInfo level 4 / DosEnumAttribute).
pub fn parse_fea2list(d: &[u8]) -> Vec<Ea> {
    let mut v = Vec::new();
    if d.len() < 4 {
        return v;
    }
    let total = (u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize).min(d.len());
    let mut at = 4;
    while at + 8 <= total {
        let next = u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) as usize;
        let flags = d[at + 4];
        let nl = d[at + 5] as usize;
        let vl = u16::from_le_bytes([d[at + 6], d[at + 7]]) as usize;
        let (Some(name), Some(value)) = (d.get(at + 8..at + 8 + nl), d.get(at + 9 + nl..at + 9 + nl + vl)) else { break };
        v.push(Ea { flags, name: name.to_vec(), value: value.to_vec() });
        if next == 0 {
            break;
        }
        at += next;
    }
    v
}

/// Does a backed-up path (no leading backslash) and name match a
/// restore specification (directory without the drive and leading
/// backslash, and a wildcard name), optionally including
/// subdirectories?
pub fn matches(dir: &str, name: &str, spec_dir: &str, spec_name: &str, sub: bool) -> bool {
    let d = dir.trim_matches('\\');
    let s = spec_dir.trim_matches('\\');
    let dir_ok = d.eq_ignore_ascii_case(s)
        || (sub && (s.is_empty() || (d.len() > s.len() && d[..s.len()].eq_ignore_ascii_case(s) && d.as_bytes()[s.len()] == b'\\')));
    dir_ok && wild(spec_name, name)
}

/// OS/2 wildcard match (`*`, `?`; `*.*` matches names without a dot).
pub fn wild(pat: &str, name: &str) -> bool {
    if pat == "*.*" || pat == "*" {
        return true;
    }
    fn go(p: &[u8], n: &[u8]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some(b'*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some(b'?'), Some(_)) => go(&p[1..], &n[1..]),
            // `?` and `.` at the end of the name match nothing.
            (Some(b'?') | Some(b'.'), None) => go(&p[1..], n),
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b) && go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    go(pat.as_bytes(), name.as_bytes())
}

#[cfg(test)]
mod tests;
