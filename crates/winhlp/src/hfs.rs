//! The help file system: the internal files of a WinHelp file, their
//! B+trees, LZ77 decompression and the compressed numbers of the topic
//! and picture records (see the crate documentation).

use crate::Error;
use alloc::vec::Vec;

pub fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

pub fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}

pub fn i16_at(d: &[u8], o: usize) -> Option<i16> {
    u16_at(d, o).map(|v| v as i16)
}

/// A zero-terminated string at `o` (to the end of `d` when there is no
/// terminator) and the offset after it.
pub fn cstr(d: &[u8], o: usize) -> (&[u8], usize) {
    let s = d.get(o..).unwrap_or(&[]);
    match s.iter().position(|&c| c == 0) {
        Some(n) => (&s[..n], o + n + 1),
        None => (s, d.len()),
    }
}

/// A cursor over little-endian data with WinHelp's compressed numbers.
#[derive(Clone)]
pub struct Cur<'a> {
    pub d: &'a [u8],
    pub p: usize,
}

impl<'a> Cur<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Cur { d, p: 0 }
    }
    pub fn at(d: &'a [u8], p: usize) -> Self {
        Cur { d, p }
    }
    pub fn left(&self) -> usize {
        self.d.len().saturating_sub(self.p)
    }
    pub fn u8(&mut self) -> Option<u8> {
        let v = *self.d.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    pub fn u16(&mut self) -> Option<u16> {
        let v = u16_at(self.d, self.p)?;
        self.p += 2;
        Some(v)
    }
    pub fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
    pub fn u32(&mut self) -> Option<u32> {
        let v = u32_at(self.d, self.p)?;
        self.p += 4;
        Some(v)
    }
    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let v = self.d.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(v)
    }
    pub fn cstr(&mut self) -> &'a [u8] {
        let (s, e) = cstr(self.d, self.p);
        self.p = e;
        s
    }
    /// Compressed unsigned short: one byte (bit 0 clear) or two, the
    /// value shifted left by one.
    pub fn cword(&mut self) -> Option<u16> {
        if *self.d.get(self.p)? & 1 == 0 {
            Some(self.u8()? as u16 >> 1)
        } else {
            Some(self.u16()? >> 1)
        }
    }
    /// Compressed signed short: as [`Cur::cword`], biased by 0x40 or
    /// 0x4000.
    pub fn cshort(&mut self) -> Option<i16> {
        if *self.d.get(self.p)? & 1 == 0 {
            Some((self.u8()? >> 1) as i16 - 0x40)
        } else {
            Some((self.u16()? >> 1) as i16 - 0x4000)
        }
    }
    /// Compressed unsigned long: two bytes (bit 0 clear) or four.
    pub fn cdword(&mut self) -> Option<u32> {
        if *self.d.get(self.p)? & 1 == 0 {
            Some(self.u16()? as u32 >> 1)
        } else {
            Some(self.u32()? >> 1)
        }
    }
    /// Compressed signed long: as [`Cur::cdword`], biased by 0x4000 or
    /// 0x40000000.
    pub fn clong(&mut self) -> Option<i32> {
        if *self.d.get(self.p)? & 1 == 0 {
            Some((self.u16()? >> 1) as i32 - 0x4000)
        } else {
            Some((self.u32()? >> 1) as i32 - 0x4000_0000)
        }
    }
}

/// WinHelp's LZ77: a flag byte for each eight items (bit 0 first); a
/// set bit is a `u16` back reference (low 12 bits: distance - 1, high 4:
/// length - 3), a clear one a literal byte. Appends to `out`, at most
/// `limit` bytes.
pub fn lz77(src: &[u8], out: &mut Vec<u8>, limit: usize) {
    let start = out.len();
    let mut p = 0;
    while p < src.len() {
        let flags = src[p];
        p += 1;
        for bit in 0..8 {
            if p >= src.len() || out.len() - start >= limit {
                return;
            }
            if flags & (1 << bit) != 0 {
                let Some(w) = u16_at(src, p) else { return };
                p += 2;
                let len = (w >> 12) as usize + 3;
                let dist = (w & 0xfff) as usize + 1;
                if dist > out.len() - start {
                    // A reference before the start: zeroes.
                    for _ in 0..len {
                        out.push(0);
                    }
                    continue;
                }
                let from = out.len() - dist;
                for i in 0..len {
                    let c = out[from + i];
                    out.push(c);
                }
            } else {
                out.push(src[p]);
                p += 1;
            }
        }
    }
}

/// WinHelp's run lengths: a byte n with bit 7 set copies the next n & 0x7F
/// bytes, else the next byte is repeated n times.
pub fn rle(src: &[u8], out: &mut Vec<u8>, limit: usize) {
    let mut p = 0;
    while p < src.len() && out.len() < limit {
        let n = src[p] as usize;
        p += 1;
        if n & 0x80 != 0 {
            let n = n & 0x7f;
            let e = (p + n).min(src.len());
            out.extend_from_slice(&src[p..e]);
            p = e;
        } else {
            let Some(&c) = src.get(p) else { break };
            p += 1;
            for _ in 0..n {
                out.push(c);
            }
        }
    }
}

/// The help file: its internal files by name.
pub struct Hfs<'a> {
    pub data: &'a [u8],
    /// (name, offset of the file header), in directory order.
    pub files: Vec<(&'a [u8], usize)>,
}

/// A B+tree's header fields and its pages.
pub struct BTree<'a> {
    page_size: usize,
    root: u16,
    levels: u16,
    pages: &'a [u8],
}

impl<'a> BTree<'a> {
    /// The B+tree in internal file `d`.
    pub fn new(d: &'a [u8]) -> Result<Self, Error> {
        if u16_at(d, 0) != Some(0x293b) {
            return Err(Error::Invalid("B+tree magic"));
        }
        let page_size = u16_at(d, 4).ok_or(Error::Truncated)? as usize;
        let root = u16_at(d, 26).ok_or(Error::Truncated)?;
        let levels = u16_at(d, 32).ok_or(Error::Truncated)?;
        if page_size < 16 {
            return Err(Error::Invalid("B+tree page size"));
        }
        Ok(BTree { page_size, root, levels, pages: &d[38.min(d.len())..] })
    }

    fn page(&self, n: u16) -> Option<&'a [u8]> {
        let o = n as usize * self.page_size;
        self.pages.get(o..o + self.page_size)
    }

    /// Calls `f` with the bytes of each leaf entry (from the entry to the
    /// page's end; `f` returns the entry's size), in key order.
    pub fn for_each(&self, mut f: impl FnMut(&'a [u8]) -> Option<usize>) {
        // Down the leftmost children to the first leaf.
        let mut n = self.root;
        for _ in 1..self.levels {
            let Some(p) = self.page(n) else { return };
            n = u16_at(p, 4).unwrap_or(0);
        }
        let mut seen = 0;
        loop {
            let Some(p) = self.page(n) else { return };
            let count = u16_at(p, 2).unwrap_or(0);
            let next = i16_at(p, 6).unwrap_or(-1);
            let mut o = 8;
            for _ in 0..count {
                let Some(len) = f(&p[o.min(p.len())..]) else { break };
                o += len;
                if o > p.len() {
                    break;
                }
            }
            seen += 1;
            if next < 0 || seen > 0x10000 {
                return;
            }
            n = next as u16;
        }
    }
}

impl<'a> Hfs<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, Error> {
        if u32_at(data, 0) != Some(crate::MAGIC) {
            return Err(Error::BadMagic);
        }
        let dir = u32_at(data, 4).ok_or(Error::Truncated)? as usize;
        let mut h = Hfs { data, files: Vec::new() };
        let d = h.file_at(dir).ok_or(Error::Truncated)?;
        let tree = BTree::new(d)?;
        let mut files = Vec::new();
        tree.for_each(|e| {
            let (name, o) = cstr(e, 0);
            let off = u32_at(e, o)? as usize;
            files.push((name, off));
            Some(o + 4)
        });
        h.files = files;
        Ok(h)
    }

    /// The internal file whose header is at `o`.
    fn file_at(&self, o: usize) -> Option<&'a [u8]> {
        let used = u32_at(self.data, o + 4)? as usize;
        let s = o + 9;
        let e = s.checked_add(used)?.min(self.data.len());
        self.data.get(s..e)
    }

    /// The internal file `name` (as the directory has it, `|SYSTEM`).
    pub fn file(&self, name: &[u8]) -> Option<&'a [u8]> {
        let &(_, o) = self.files.iter().find(|(n, _)| *n == name)?;
        self.file_at(o)
    }
}
