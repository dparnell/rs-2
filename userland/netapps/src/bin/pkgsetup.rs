//! PKGSETUP: install RPM and yum on a running rs2 (docs/PACKAGES.md).
//! `PKGSETUP [/FIRSTBOOT] [drive:]`.
//!
//! yum needs Python and RPM to install anything, so this puts them on the
//! disk itself: the packages of `\OS2\PKGLIST.TXT` come from the netlabs
//! repository over HTTP, each checked against the SHA-256 in that list (the
//! connection proves nothing), and their files are unpacked below the drive,
//! the UNIXROOT. Symbolic links become copies, as OS/2 has none. CONFIG.SYS
//! gets what the programs need, and RPM is told what is there.
//!
//! The installer leaves `CALL=...PKGSETUP.EXE /FIRSTBOOT` in CONFIG.SYS: that
//! line goes again whatever the outcome, and the computer restarts for the
//! new LIBPATH. What was downloaded is kept until the end, so running
//! PKGSETUP again goes on where it stopped.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use netapps::*;
use os2::path::cstr;
use os2::*;
use utils::disk::fs_name;
use utils::drive_arg;
use utils::files::{create, exists, make_dirs, read_file as read_whole, write_all};
os2::entry!(main);

unsafe extern "C" {
    #[link_name = "DOSCALLS.415"]
    fn DosShutdown(reserved: u32) -> APIRET;
}

const HOST: &str = "rpm.netlabs.org";
const BASE: &str = "/release/00/i386/";
/// What the files and the packages take on the disk, with room to work.
const NEEDS_MB: u64 = 600;
/// Tries of a download.
const TRIES: u32 = 3;
/// Seconds to wait for the network at boot.
const NET_WAIT: u32 = 90;
/// Not unpacked: the same as `tools/scripts/rpmdisk.py`.
const SKIP: [&str; 5] = ["usr/share/doc/", "usr/share/man/", "usr/share/info/", "usr/share/locale/", "usr/include/"];

// ---- SHA-256 ----

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
    0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
    0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

struct Sha256 {
    h: [u32; 8],
    block: [u8; 64],
    fill: usize,
    total: u64,
}

impl Sha256 {
    fn new() -> Sha256 {
        Sha256 {
            h: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
            block: [0; 64],
            fill: 0,
            total: 0,
        }
    }

    fn round(&mut self) {
        let mut w = [0u32; 64];
        for (i, c) in self.block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = self.h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let t2 = s0.wrapping_add((v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]));
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (h, x) in self.h.iter_mut().zip(v) {
            *h = h.wrapping_add(x);
        }
    }

    fn update(&mut self, mut d: &[u8]) {
        self.total += d.len() as u64;
        while !d.is_empty() {
            let n = (64 - self.fill).min(d.len());
            self.block[self.fill..self.fill + n].copy_from_slice(&d[..n]);
            self.fill += n;
            d = &d[n..];
            if self.fill == 64 {
                self.round();
                self.fill = 0;
            }
        }
    }

    fn hex(mut self) -> String {
        let bits = self.total * 8;
        self.update(&[0x80]);
        while self.fill != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        self.h.iter().map(|x| format!("{x:08x}")).collect()
    }
}

// ---- Inflate (RFC 1951) ----

struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..15 {
            offs[i + 1] = offs[i] + count[i];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Huffman { count, symbol }
    }
}

struct Bits<'a> {
    d: &'a [u8],
    at: usize,
    buf: u32,
    n: u32,
}

impl Bits<'_> {
    fn bits(&mut self, want: u32) -> Option<u32> {
        while self.n < want {
            self.buf |= (*self.d.get(self.at)? as u32) << self.n;
            self.at += 1;
            self.n += 8;
        }
        let v = self.buf & ((1u32 << want) - 1);
        self.buf >>= want;
        self.n -= want;
        Some(v)
    }

    fn decode(&mut self, h: &Huffman) -> Option<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= self.bits(1)? as i32;
            let c = h.count[len] as i32;
            if code - c < first {
                return h.symbol.get((index + code - first) as usize).copied();
            }
            index += c;
            first = (first + c) << 1;
            code <<= 1;
        }
        None
    }
}

const WINDOW: usize = 32768;
const FLUSH: usize = 1 << 20;
const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Inflate `d` into `out`, a piece at a time.
fn inflate(d: &[u8], out: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<(), String> {
    let bad = || String::from("the compressed data is damaged");
    let mut b = Bits { d, at: 0, buf: 0, n: 0 };
    let mut o: Vec<u8> = Vec::with_capacity(FLUSH + WINDOW + 512);
    // What of `o` was handed out already (the window stays).
    let mut done = 0;
    loop {
        let last = b.bits(1).ok_or_else(bad)?;
        let kind = b.bits(2).ok_or_else(bad)?;
        match kind {
            0 => {
                b.buf = 0;
                b.n = 0;
                let h = d.get(b.at..b.at + 4).ok_or_else(bad)?;
                let n = u16::from_le_bytes([h[0], h[1]]) as usize;
                b.at += 4;
                o.extend_from_slice(d.get(b.at..b.at + n).ok_or_else(bad)?);
                b.at += n;
            }
            1 | 2 => {
                let (lit, dist) = if kind == 1 {
                    let mut l = [8u8; 288];
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    (Huffman::new(&l), Huffman::new(&[5u8; 30]))
                } else {
                    tables(&mut b).ok_or_else(bad)?
                };
                loop {
                    let s = b.decode(&lit).ok_or_else(bad)? as usize;
                    if s < 256 {
                        o.push(s as u8);
                    } else if s == 256 {
                        break;
                    } else {
                        let i = s - 257;
                        let len = *LEN_BASE.get(i).ok_or_else(bad)? as usize + b.bits(LEN_EXTRA[i] as u32).ok_or_else(bad)? as usize;
                        let k = b.decode(&dist).ok_or_else(bad)? as usize;
                        let back = *DIST_BASE.get(k).ok_or_else(bad)? as usize + b.bits(DIST_EXTRA[k] as u32).ok_or_else(bad)? as usize;
                        if back > o.len() {
                            return Err(bad());
                        }
                        let from = o.len() - back;
                        for j in 0..len {
                            o.push(o[from + j]);
                        }
                    }
                    if o.len() >= FLUSH + WINDOW {
                        out(&o[done..])?;
                        o.drain(..o.len() - WINDOW);
                        done = o.len();
                    }
                }
            }
            _ => return Err(bad()),
        }
        if last == 1 {
            return out(&o[done..]);
        }
    }
}

fn tables(b: &mut Bits) -> Option<(Huffman, Huffman)> {
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let nlen = b.bits(5)? as usize + 257;
    let ndist = b.bits(5)? as usize + 1;
    let ncode = b.bits(4)? as usize + 4;
    let mut l = [0u8; 320];
    for &i in &ORDER[..ncode] {
        l[i] = b.bits(3)? as u8;
    }
    let code = Huffman::new(&l[..19]);
    l = [0u8; 320];
    let mut i = 0;
    while i < nlen + ndist {
        let s = b.decode(&code)?;
        if s < 16 {
            l[i] = s as u8;
            i += 1;
            continue;
        }
        let (v, n) = match s {
            16 => (*l.get(i.checked_sub(1)?)?, 3 + b.bits(2)?),
            17 => (0, 3 + b.bits(3)?),
            _ => (0, 11 + b.bits(7)?),
        };
        for _ in 0..n {
            *l.get_mut(i)? = v;
            i += 1;
        }
    }
    Some((Huffman::new(&l[..nlen]), Huffman::new(&l[nlen..nlen + ndist])))
}

// ---- RPM files ----

/// Where the compressed payload starts: after the lead, the signature
/// (padded to 8 bytes) and the header.
fn payload(d: &[u8]) -> Option<&[u8]> {
    if d.get(..4)? != [0xed, 0xab, 0xee, 0xdb] {
        return None;
    }
    let header = |o: usize| -> Option<usize> {
        let h = d.get(o..o + 16)?;
        let n = u32::from_be_bytes([h[8], h[9], h[10], h[11]]) as usize;
        let s = u32::from_be_bytes([h[12], h[13], h[14], h[15]]) as usize;
        Some(o + 16 + n * 16 + s)
    };
    let sig = header(96)?;
    let body = header((sig + 7) & !7)?;
    d.get(body..)
}

/// The deflate data of a gzip file.
fn gunzip(d: &[u8]) -> Option<&[u8]> {
    if d.get(..3)? != [0x1f, 0x8b, 8] {
        return None;
    }
    let flags = d[3];
    let mut o = 10;
    if flags & 4 != 0 {
        o += 2 + u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]) as usize;
    }
    for bit in [8, 16] {
        if flags & bit != 0 {
            o += d.get(o..)?.iter().position(|&c| c == 0)? + 1;
        }
    }
    if flags & 2 != 0 {
        o += 2;
    }
    d.get(o..)
}

/// A file being written from the archive.
struct Open {
    hf: u32,
    path: String,
    ino: u32,
}

/// Reads a cpio archive (the "newc" format) as it comes.
struct Cpio<'a> {
    root: &'a str,
    /// Bytes not yet used.
    buf: Vec<u8>,
    /// Bytes of the current file's data still to come, and its padding.
    left: usize,
    pad: usize,
    file: Option<Open>,
    /// The target of a symbolic link is being read for this path.
    link: Option<String>,
    links: &'a mut Vec<(String, String)>,
    /// Hard links: the data comes with the last name.
    hard: Vec<(u32, String)>,
    made: String,
    done: bool,
    skipped: &'a mut u32,
}

fn hex8(b: &[u8]) -> Option<u32> {
    u32::from_str_radix(core::str::from_utf8(b).ok()?, 16).ok()
}

impl Cpio<'_> {
    fn feed(&mut self, mut d: &[u8]) -> Result<(), String> {
        // File data goes straight through; only headers are gathered.
        while !self.done && self.buf.is_empty() && self.link.is_none() && self.left + self.pad > 0 && !d.is_empty() {
            if self.left > 0 {
                let n = self.left.min(d.len());
                if let Some(f) = &self.file {
                    write_all(f.hf, &d[..n]).map_err(|e| format!("writing {} failed (error {e})", f.path))?;
                }
                self.left -= n;
                d = &d[n..];
                if self.left == 0 {
                    self.close();
                }
            } else {
                let n = self.pad.min(d.len());
                self.pad -= n;
                d = &d[n..];
            }
        }
        if self.done || d.is_empty() {
            return Ok(());
        }
        self.buf.extend_from_slice(d);
        self.parse()
    }

    fn close(&mut self) {
        let Some(f) = self.file.take() else { return };
        unsafe { DosClose(f.hf) };
        // The names that came before it.
        let names: Vec<String> = self.hard.iter().filter(|h| h.0 == f.ino).map(|h| h.1.clone()).collect();
        self.hard.retain(|h| h.0 != f.ino);
        for n in names {
            let _ = copy_file(&f.path, &n);
        }
    }

    /// Entries that are all in `buf`, up to the data of a file.
    fn parse(&mut self) -> Result<(), String> {
        let bad = || String::from("the archive in the package is damaged");
        loop {
            if self.link.is_none() {
                let n = self.pad.min(self.buf.len());
                self.buf.drain(..n);
                self.pad -= n;
                if self.pad > 0 {
                    return Ok(());
                }
            }
            if let Some(path) = self.link.take() {
                if self.buf.len() < self.left {
                    self.link = Some(path);
                    return Ok(());
                }
                let t = String::from_utf8_lossy(&self.buf[..self.left]).to_string();
                self.buf.drain(..self.left);
                self.left = 0;
                self.links.push((path, t));
                continue;
            }
            if self.buf.len() < 110 {
                return Ok(());
            }
            if &self.buf[..6] != b"070701" {
                return Err(bad());
            }
            let f = |i: usize| hex8(&self.buf[6 + i * 8..14 + i * 8]);
            let (ino, mode, nlink, size, namesize) = (f(0).ok_or_else(bad)?, f(1).ok_or_else(bad)?, f(4).ok_or_else(bad)?, f(6).ok_or_else(bad)? as usize, f(11).ok_or_else(bad)? as usize);
            let head = (110 + namesize + 3) & !3;
            if self.buf.len() < head {
                return Ok(());
            }
            let name = String::from_utf8_lossy(&self.buf[110..110 + namesize - 1]).to_string();
            self.buf.drain(..head);
            if name == "TRAILER!!!" {
                self.done = true;
                return Ok(());
            }
            self.left = size;
            self.pad = (4 - size % 4) % 4;
            let path = self.target(&name);
            match (mode & 0o170000, path) {
                (0o040000, Some(p)) => {
                    make_dirs(&p);
                }
                (0o120000, Some(p)) => {
                    self.link = Some(p);
                    continue;
                }
                (0o100000, Some(p)) => {
                    self.parent(&p);
                    if size == 0 && nlink > 1 {
                        self.hard.push((ino, p));
                    } else {
                        let hf = create(&p, 0).map_err(|e| format!("cannot create {p} (error {e})"))?;
                        self.file = Some(Open { hf, path: p, ino });
                        if size == 0 {
                            self.close();
                        }
                    }
                }
                _ => {}
            }
            if size > 0 {
                // The caller writes it (or passes over it).
                let n = self.left.min(self.buf.len());
                if let Some(f) = &self.file {
                    write_all(f.hf, &self.buf[..n]).map_err(|e| format!("writing {} failed (error {e})", f.path))?;
                }
                self.buf.drain(..n);
                self.left -= n;
                if self.left > 0 {
                    return Ok(());
                }
                self.close();
            }
        }
    }

    /// Where a name of the archive goes, if it goes anywhere.
    fn target(&mut self, name: &str) -> Option<String> {
        let rel = name.strip_prefix("./@unixroot/")?;
        if SKIP.iter().any(|s| rel.starts_with(s)) {
            return None;
        }
        if rel.bytes().any(|c| b":*?\"<>|\\".contains(&c)) {
            *self.skipped += 1;
            return None;
        }
        Some(format!("{}\\{}", self.root, rel.replace('/', "\\")))
    }

    fn parent(&mut self, p: &str) {
        let dir = &p[..p.rfind('\\').unwrap_or(0)];
        if dir != self.made {
            make_dirs(dir);
            self.made = dir.to_string();
        }
    }
}

fn copy_file(from: &str, to: &str) -> Result<(), u32> {
    let d = read_whole(from)?;
    let hf = create(to, 0)?;
    let r = write_all(hf, &d);
    unsafe { DosClose(hf) };
    r
}

fn unpack(file: &str, root: &str, links: &mut Vec<(String, String)>, skipped: &mut u32) -> Result<(), String> {
    let d = read_whole(file).map_err(|e| format!("cannot read it (error {e})"))?;
    let z = payload(&d).and_then(gunzip).ok_or("it is not a gzip-compressed RPM")?;
    let mut c = Cpio { root, buf: Vec::new(), left: 0, pad: 0, file: None, link: None, links, hard: Vec::new(), made: String::new(), done: false, skipped };
    inflate(z, &mut |b| c.feed(b))
}

/// Links become copies; a link to a link waits for its turn.
fn resolve_links(root: &str, links: Vec<(String, String)>) -> u32 {
    let mut todo = links;
    loop {
        let before = todo.len();
        todo.retain(|(path, target)| {
            let from = match target.strip_prefix("/@unixroot/") {
                Some(t) => format!("{root}\\{}", t.replace('/', "\\")),
                None => normal(&format!("{}\\{}", &path[..path.rfind('\\').unwrap_or(0)], target.replace('/', "\\"))),
            };
            if !exists(&from) || is_dir(&from) {
                return true;
            }
            copy_file(&from, path).is_err()
        });
        if todo.is_empty() || todo.len() == before {
            return todo.len() as u32;
        }
    }
}

/// `..` and `.` out of a path.
fn normal(p: &str) -> String {
    let mut v: Vec<&str> = Vec::new();
    for s in p.split('\\') {
        match s {
            "." | "" => {}
            ".." => {
                v.pop();
            }
            _ => v.push(s),
        }
    }
    v.join("\\")
}

// ---- Download ----

/// GET `path` into `file`: its SHA-256, or a message.
fn download(addr: [u8; 4], path: &str, file: &str) -> Result<String, String> {
    let s = Sock::connect_addr(addr, 80).map_err(|e| format!("cannot connect (error {e})"))?;
    s.set_timeout(60_000);
    s.send_str(&format!("GET {path} HTTP/1.0\r\nHost: {HOST}\r\nUser-Agent: rs2-pkgsetup\r\nConnection: close\r\n\r\n")).map_err(|e| format!("cannot send (error {e})"))?;
    let mut buf = vec![0u8; 65536];
    let mut head: Vec<u8> = Vec::new();
    let end = loop {
        let n = s.recv(&mut buf).map_err(|e| format!("cannot read (error {e})"))?;
        if n == 0 {
            return Err(String::from("the server closed the connection"));
        }
        head.extend_from_slice(&buf[..n]);
        if let Some(i) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let status = String::from_utf8_lossy(&head[..head.iter().position(|&c| c == b'\r').unwrap_or(0)]).to_string();
    if status.split_whitespace().nth(1) != Some("200") {
        return Err(format!("the server answered: {status}"));
    }
    let hf = create(file, 0).map_err(|e| format!("cannot create {file} (error {e})"))?;
    let mut sha = Sha256::new();
    let mut put = |b: &[u8]| {
        sha.update(b);
        write_all(hf, b)
    };
    let r = (|| -> Result<(), String> {
        put(&head[end..]).map_err(|e| format!("cannot write (error {e})"))?;
        loop {
            let n = s.recv(&mut buf).map_err(|e| format!("cannot read (error {e})"))?;
            if n == 0 {
                return Ok(());
            }
            put(&buf[..n]).map_err(|e| format!("cannot write (error {e})"))?;
        }
    })();
    unsafe { DosClose(hf) };
    r?;
    Ok(sha.hex())
}

struct Package {
    sha: String,
    size: u64,
    path: String,
}

fn package_list(root: &str) -> Option<Vec<Package>> {
    let text = read_file(&format!("{root}\\OS2\\PKGLIST.TXT")).or_else(|| read_file("C:\\OS2\\PKGLIST.TXT"))?;
    let mut v = Vec::new();
    for l in utils::lines(&text) {
        let mut w = l.split_whitespace();
        let (Some(sha), Some(size), Some(path)) = (w.next(), w.next(), w.next()) else { continue };
        if sha.starts_with('#') {
            continue;
        }
        v.push(Package { sha: sha.to_string(), size: size.parse().ok()?, path: path.to_string() });
    }
    Some(v)
}

fn file_sha(path: &str) -> Option<String> {
    let d = read_whole(path).ok()?;
    let mut s = Sha256::new();
    s.update(&d);
    Some(s.hex())
}

fn fetch_all(list: &[Package], cache: &str) -> Result<(), String> {
    outln("Waiting for the network...");
    let mut addr = None;
    for _ in 0..NET_WAIT / 3 {
        if let Ok(a) = lookup(HOST) {
            addr = Some(a);
            break;
        }
        unsafe { DosSleep(3000) };
    }
    let addr = addr.ok_or(format!("{HOST} cannot be reached: is the network up?"))?;
    let total: u64 = list.iter().map(|p| p.size).sum();
    let mut got = 0;
    for (i, p) in list.iter().enumerate() {
        let name = p.path.rsplit('/').next().unwrap_or(&p.path);
        let file = format!("{cache}\\{name}");
        got += p.size;
        if file_sha(&file).as_deref() == Some(&p.sha) {
            continue;
        }
        outln(&format!("[{}/{}, {} of {} MB] {name}", i + 1, list.len(), got >> 20, total >> 20));
        let tmp = format!("{cache}\\DOWNLOAD.TMP");
        let mut last = String::new();
        for _ in 0..TRIES {
            match download(addr, &format!("{BASE}{}", p.path), &tmp) {
                Ok(sha) if sha == p.sha => {
                    last.clear();
                    break;
                }
                Ok(_) => last = String::from("its SHA-256 is not the one expected"),
                Err(e) => last = e,
            }
        }
        if !last.is_empty() {
            return Err(format!("{name}: {last}"));
        }
        let (a, b) = (cstr(&tmp), cstr(&file));
        unsafe {
            DosDelete(b.as_ptr());
            DosMove(a.as_ptr(), b.as_ptr());
        }
    }
    Ok(())
}

// ---- CONFIG.SYS ----

/// CONFIG.SYS with what the programs need, without the /FIRSTBOOT call.
fn edit_config(text: &str, root: &str) -> String {
    let sets = [
        ("UNIXROOT", root.to_string()),
        ("HOME", format!("{root}\\HOME")),
        ("TMP", format!("{root}\\TMP")),
        ("TEMP", format!("{root}\\TMP")),
        ("TMPDIR", format!("{root}\\TMP")),
        ("TERM", String::from("dumb")),
        ("LOGNAME", String::from("root")),
        ("USER", String::from("root")),
        ("LANG", String::from("en_US")),
    ];
    let bin = format!("{root}\\USR\\BIN;");
    let lib = format!("{root}\\USR\\LOCAL\\LIB;{root}\\USR\\LIB;");
    let mut out = String::new();
    for l in text.lines() {
        let l = l.trim_end_matches('\r');
        let u = l.to_ascii_uppercase();
        if u.starts_with("CALL=") && u.contains("PKGSETUP") {
            continue;
        }
        if let Some(v) = u.strip_prefix("LIBPATH=") {
            if !v.contains("\\USR\\LIB") {
                let rest = l["LIBPATH=".len()..].trim_start_matches(".;");
                out.push_str(&format!("LIBPATH=.;{lib}{rest}\r\n"));
                continue;
            }
        }
        if let Some(v) = u.strip_prefix("SET PATH=") {
            if !v.contains("\\USR\\BIN") {
                out.push_str(&format!("SET PATH={bin}{}\r\n", &l["SET PATH=".len()..]));
                continue;
            }
        }
        out.push_str(l);
        out.push_str("\r\n");
    }
    for (k, v) in sets {
        if !text.to_ascii_uppercase().contains(&format!("SET {k}=")) {
            out.push_str(&format!("SET {k}={v}\r\n"));
        }
    }
    out
}

fn drop_firstboot(config: &str) {
    if let Some(t) = read_file(config) {
        let t = String::from_utf8_lossy(&t);
        let kept: Vec<&str> = t.lines().filter(|l| !(l.to_ascii_uppercase().starts_with("CALL=") && l.to_ascii_uppercase().contains("PKGSETUP"))).collect();
        let _ = utils::files::write_file(config, (kept.join("\n") + "\n").replace("\r\n", "\n").replace('\n', "\r\n").as_bytes());
    }
}

// ---- RPM's database ----

/// `rpm --justdb` for everything unpacked: its exit code.
fn register(root: &str, cache: &str) -> u32 {
    let lib = cstr(&format!("{root}\\USR\\LOCAL\\LIB;{root}\\USR\\LIB"));
    unsafe { DosSetExtLIBPATH(lib.as_ptr(), 1) };
    let unix = cache.replace('\\', "/");
    let script = format!("rpm --initdb && rpm -ivh --justdb --nodeps --noscripts --notriggers --force {unix}/*.rpm");
    let bash = format!("{root}\\USR\\BIN\\BASH.EXE");
    let mut args = bash.clone().into_bytes();
    args.push(0);
    args.extend_from_slice(format!("-c \"{script}\"").as_bytes());
    args.extend_from_slice(&[0, 0]);
    let mut env = Vec::new();
    for e in [
        format!("PATH={root}\\USR\\BIN;C:\\OS2"),
        format!("UNIXROOT={root}"),
        format!("HOME={root}\\HOME"),
        format!("TMP={root}\\TMP"),
        format!("TEMP={root}\\TMP"),
        format!("TMPDIR={root}\\TMP"),
        String::from("TERM=dumb"),
        String::from("LOGNAME=root"),
        String::from("USER=root"),
        String::from("LANG=en_US"),
    ] {
        env.extend_from_slice(e.as_bytes());
        env.push(0);
    }
    env.push(0);
    let pgm = cstr(&bash);
    let mut res = ResultCodes { code_terminate: 0, code_result: 0 };
    let rc = unsafe { DosExecPgm(core::ptr::null_mut(), 0, 0, args.as_ptr(), env.as_ptr(), &mut res, pgm.as_ptr()) };
    if rc != 0 { rc } else { res.code_result }
}

fn free_mb(drive: u8) -> u64 {
    let mut b = [0u8; 18];
    if unsafe { DosQueryFSInfo(drive as u32 + 1, 1, b.as_mut_ptr(), b.len() as u32) } != 0 {
        return 0;
    }
    let w = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as u64;
    w(4) * w(12) * u16::from_le_bytes([b[16], b[17]]) as u64 >> 20
}

fn setup(drive: u8) -> Result<(), String> {
    let root = format!("{}:", (b'A' + drive) as char);
    let fs = fs_name(drive).unwrap_or_default();
    if fs != "HPFS" && fs != "JFS" {
        return Err(format!("{root} is {fs}: RPM and yum need the long names of HPFS or JFS."));
    }
    if exists(&format!("{root}\\USR\\BIN\\YUM")) && exists(&format!("{root}\\VAR\\LIB\\RPM\\Packages")) {
        outln("RPM and yum are installed already.");
        return Ok(());
    }
    if free_mb(drive) < NEEDS_MB {
        return Err(format!("{root} has {} MB free; {NEEDS_MB} MB are needed.", free_mb(drive)));
    }
    let list = package_list(&root).ok_or("\\OS2\\PKGLIST.TXT cannot be read.")?;
    let cache = format!("{root}\\VAR\\CACHE\\PKGSETUP");
    make_dirs(&cache);
    outln(&format!("Downloading {} packages from {HOST}.", list.len()));
    fetch_all(&list, &cache)?;
    outln("Unpacking.");
    let (mut links, mut skipped) = (Vec::new(), 0);
    for (i, p) in list.iter().enumerate() {
        let name = p.path.rsplit('/').next().unwrap_or(&p.path);
        outln(&format!("[{}/{}] {name}", i + 1, list.len()));
        unpack(&format!("{cache}\\{name}"), &root, &mut links, &mut skipped).map_err(|e| format!("{name}: {e}"))?;
    }
    let left = resolve_links(&root, links);
    if left + skipped > 0 {
        outln(&format!("{left} links and {skipped} files with names OS/2 does not allow were left out."));
    }
    for d in ["HOME", "TMP", "VAR\\LOG", "VAR\\RUN"] {
        make_dirs(&format!("{root}\\{d}"));
    }
    outln("Setting up CONFIG.SYS.");
    let config = format!("{root}\\CONFIG.SYS");
    let text = read_file(&config).ok_or("CONFIG.SYS cannot be read.")?;
    let text = String::from_utf8_lossy(&text);
    utils::files::write_file(&config, edit_config(&text, &root).as_bytes()).map_err(|e| format!("CONFIG.SYS cannot be written (error {e})."))?;
    outln("Telling RPM what is installed.");
    let rc = register(&root, &cache);
    if rc != 0 {
        return Err(format!("rpm ended with {rc}."));
    }
    for p in &list {
        let name = p.path.rsplit('/').next().unwrap_or(&p.path);
        let c = cstr(&format!("{cache}\\{name}"));
        unsafe { DosDelete(c.as_ptr()) };
    }
    Ok(())
}

fn restart() {
    let c = cstr("DOS$");
    let (mut h, mut act) = (0, 0);
    unsafe {
        if DosOpen(c.as_ptr(), &mut h, &mut act, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS, OPEN_SHARE_DENYNONE | OPEN_ACCESS_READWRITE, core::ptr::null_mut()) == 0 {
            DosShutdown(0);
            let (mut pl, mut dl) = (0, 0);
            DosDevIOCtl(h, 0xd5, 0xab, core::ptr::null_mut(), 0, &mut pl, core::ptr::null_mut(), 0, &mut dl);
        }
    }
}

fn main(s: &os2::Startup) -> u32 {
    let (mut first, mut drive) = (false, None);
    for a in args(s) {
        if a.eq_ignore_ascii_case("/FIRSTBOOT") {
            first = true;
        } else if a.starts_with('/') {
            eprint("Usage: PKGSETUP [/FIRSTBOOT] [drive:]\r\n");
            return 87;
        } else {
            drive = Some(a);
        }
    }
    let Some(d) = drive_arg(Some(drive.as_deref().unwrap_or("C:"))) else {
        eprint("PKGSETUP: that is not a drive.\r\n");
        return 15;
    };
    outln("Installing RPM and yum. They come from the netlabs repository and are not part of rs2.");
    let r = setup(d);
    let config = format!("{}:\\CONFIG.SYS", (b'A' + d) as char);
    if first {
        drop_firstboot(&config);
    }
    match r {
        Ok(()) => {
            outln("RPM and yum are installed.");
            if first {
                outln("The computer restarts now for the new CONFIG.SYS.");
                restart();
            } else {
                outln("Restart the computer for the new CONFIG.SYS.");
            }
            0
        }
        Err(e) => {
            outln(&format!("PKGSETUP: {e}"));
            outln("Nothing is lost: run PKGSETUP at a command prompt to try again.");
            if first {
                unsafe { DosSleep(10_000) };
            }
            1
        }
    }
}
