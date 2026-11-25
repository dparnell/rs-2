//! The disk's archives: zip files, stored or deflated (no ZIP64).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use os2::*;

pub struct Entry {
    pub name: String,
    pub size: u32,
    pub date: u16,
    pub time: u16,
    method: u16,
    crc: u32,
    packed: u32,
    at: u32,
}

pub struct Archive {
    path: String,
    pub entries: Vec<Entry>,
}

fn w(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn l(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// An open file.
struct File(u32);

impl Drop for File {
    fn drop(&mut self) {
        unsafe { DosClose(self.0) };
    }
}

impl File {
    fn open(path: &str) -> Result<File, String> {
        utils::open_read(path).map(File).map_err(|e| alloc::format!("cannot be opened (error {e})"))
    }

    /// `n` bytes from `at` (from the end: method 2).
    fn read(&self, at: i32, from: u32, n: usize) -> Result<Vec<u8>, String> {
        let mut pos = 0;
        let mut v = alloc::vec![0u8; n];
        let mut done = 0;
        let mut rc = unsafe { DosSetFilePtr(self.0, at, from, &mut pos) };
        while rc == 0 && done < n {
            let mut got = 0;
            rc = unsafe { DosRead(self.0, v[done..].as_mut_ptr(), (n - done).min(1 << 20) as u32, &mut got) };
            if got == 0 && rc == 0 {
                return Err("ends too early".to_string());
            }
            done += got as usize;
        }
        if rc != 0 { Err(alloc::format!("cannot be read (error {rc})")) } else { Ok(v) }
    }

    fn len(&self) -> u32 {
        let mut n = 0;
        unsafe { DosSetFilePtr(self.0, 0, 2, &mut n) };
        n
    }
}

/// The name stays below the directory it is unpacked into.
fn safe(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('/') && !n.contains(['\\', ':', '\0']) && !n.split('/').any(|c| c == "..")
}

impl Archive {
    pub fn open(path: &str) -> Result<Archive, String> {
        let f = File::open(path)?;
        let bad = || "is not the archive it should be".to_string();
        let len = f.len();
        let n = len.min(65557);
        let tail = f.read((len - n) as i32, 0, n as usize)?;
        let end = (0..tail.len().saturating_sub(21)).rev().find(|&i| tail[i..].starts_with(b"PK\x05\x06")).ok_or_else(bad)?;
        let count = w(&tail, end + 10).ok_or_else(bad)?;
        let size = l(&tail, end + 12).ok_or_else(bad)?;
        let at = l(&tail, end + 16).ok_or_else(bad)?;
        if size > len || at > len - size {
            return Err(bad());
        }
        let d = f.read(at as i32, 0, size as usize)?;
        let mut entries = Vec::with_capacity(count as usize);
        let mut o = 0;
        for _ in 0..count {
            if d.get(o..o + 4) != Some(b"PK\x01\x02") {
                return Err(bad());
            }
            let g = |k: usize| w(&d, o + k).ok_or_else(bad);
            let gl = |k: usize| l(&d, o + k).ok_or_else(bad);
            let (nl, xl, cl) = (g(28)? as usize, g(30)? as usize, g(32)? as usize);
            let name = String::from_utf8_lossy(d.get(o + 46..o + 46 + nl).ok_or_else(bad)?).to_string();
            let e = Entry { name, size: gl(24)?, date: g(14)?, time: g(12)?, method: g(10)?, crc: gl(16)?, packed: gl(20)?, at: gl(42)? };
            if !safe(&e.name) || !matches!(e.method, 0 | 8) || e.at >= len || e.packed > len {
                return Err(bad());
            }
            entries.push(e);
            o += 46 + nl + xl + cl;
        }
        Ok(Archive { path: path.to_string(), entries })
    }

    /// The files' bytes.
    pub fn bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.size as u64).sum()
    }

    /// What they take on a disk, about: 4 KB units and a directory entry.
    pub fn disk_bytes(&self) -> u64 {
        self.entries.iter().map(|e| (e.size as u64).next_multiple_of(4096) + 1024).sum()
    }

    /// A file's data, in pieces; its CRC is checked.
    pub fn read(&self, e: &Entry, sink: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<(), String> {
        let f = File::open(&self.path)?;
        let h = f.read(e.at as i32, 0, 30)?;
        let damaged = || "the archive is damaged here".to_string();
        if !h.starts_with(b"PK\x03\x04") {
            return Err(damaged());
        }
        let skip = 30 + w(&h, 26).unwrap_or(0) as u32 + w(&h, 28).unwrap_or(0) as u32;
        let d = f.read((e.at + skip) as i32, 0, e.packed as usize)?;
        let (mut crc, mut n) = (0, 0u64);
        let mut out = |p: &[u8]| {
            crc = inflate::crc32(crc, p);
            n += p.len() as u64;
            sink(p)
        };
        if e.method == 0 {
            for p in d.chunks(1 << 20) {
                out(p)?;
            }
        } else {
            inflate::inflate(&d, &mut out).map_err(|x| match x {
                inflate::Error::Damaged => damaged(),
                inflate::Error::Sink(m) => m,
            })?;
        }
        if crc != e.crc || n != e.size as u64 { Err(damaged()) } else { Ok(()) }
    }
}
