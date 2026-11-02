//! The packing side (rs2's PACK/PACK2): FTCOMP streams with LZ-coded
//! chunks in blocks that skip the entropy stage (`total == 0xFFFF`), which
//! IBM's UNPACK2 reads like any other block. Matches are found with hash
//! chains over the whole window, including the preset dictionary.

use alloc::vec;
use alloc::vec::Vec;

use crate::decode::BLOCK_MAGIC;
use crate::{tables, ALGORITHM, ALGORITHM_VERSION, HEADER_SIZE, MAGIC};

const WINDOW: usize = 0xcfdc;
const ESCAPE: u8 = 0x9e;
/// Input bytes per block and output bytes per chunk, as IBM's packer.
const BLOCK_IN: usize = 0x6000;
const CHUNK_OUT: usize = 0x3000;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 0x43 + 0xff;
/// The farthest back a match may reach (well inside the window).
const MAX_DIST: usize = WINDOW - 0x100;
const HASH_BITS: u32 = 15;
const CHAIN_TRIES: usize = 64;

/// The window as the decoder presets it.
fn preset() -> Vec<u8> {
    let mut w = vec![0u8; 0xbc62];
    w.resize(0xbc62 + 0x140, b' ');
    w.resize(0xbc62 + 0x280, 0xff);
    w.resize(0xbc62 + 0x3c0, 0);
    w.extend_from_slice(&tables::DICTIONARY);
    w
}

fn hash(b: &[u8]) -> usize {
    let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
    (v.wrapping_mul(2654435761) >> (32 - HASH_BITS)) as usize
}

struct Matcher {
    buf: Vec<u8>,
    head: Vec<u32>,
    prev: Vec<u32>,
    /// Positions below this are in the hash chains.
    indexed: usize,
}

impl Matcher {
    fn new(data: &[u8]) -> Self {
        let mut buf = preset();
        buf.extend_from_slice(data);
        let n = buf.len();
        let mut m = Matcher { buf, head: vec![u32::MAX; 1 << HASH_BITS], prev: vec![u32::MAX; n], indexed: 0 };
        // The dictionary part of the preset is worth matching against.
        m.index_to(WINDOW);
        m
    }

    fn index_to(&mut self, end: usize) {
        while self.indexed < end && self.indexed + MIN_MATCH <= self.buf.len() {
            let h = hash(&self.buf[self.indexed..]);
            self.prev[self.indexed] = self.head[h];
            self.head[h] = self.indexed as u32;
            self.indexed += 1;
        }
        self.indexed = self.indexed.max(end);
    }

    /// Longest match for position `pos`, at most `limit` bytes long.
    fn find(&self, pos: usize, limit: usize) -> Option<(usize, usize)> {
        if limit < MIN_MATCH || pos + MIN_MATCH > self.buf.len() {
            return None;
        }
        let mut cand = self.head[hash(&self.buf[pos..])];
        let mut best: Option<(usize, usize)> = None;
        let mut tries = 0;
        while cand != u32::MAX && tries < CHAIN_TRIES {
            let c = cand as usize;
            if c >= pos {
                cand = self.prev[c];
                continue;
            }
            let dist = pos - c - 1;
            if dist >= MAX_DIST {
                break;
            }
            let mut n = 0;
            while n < limit && self.buf[c + n] == self.buf[pos + n] {
                n += 1;
            }
            // Word distances need a length of 4 or more (3 with a word
            // distance would read as the literal 0x9E escape).
            let ok = n >= MIN_MATCH && (dist < 0x100 || n >= 4);
            if ok && best.is_none_or(|(bn, _)| n > bn) {
                best = Some((n, dist));
                if n == limit {
                    break;
                }
            }
            cand = self.prev[c];
            tries += 1;
        }
        best
    }
}

fn put_match(out: &mut Vec<u8>, len: usize, dist: usize) {
    if len <= 3 + 0x3f && dist < 0x100 {
        out.extend_from_slice(&[ESCAPE, (len - 3) as u8, dist as u8]);
    } else if len <= 3 + 0x3f {
        out.extend_from_slice(&[ESCAPE, 0x40 | (len - 3) as u8, dist as u8, (dist >> 8) as u8]);
    } else {
        out.extend_from_slice(&[ESCAPE, 0x80, (len - 0x43) as u8, dist as u8, (dist >> 8) as u8]);
    }
}

/// Compress one stream (a file's data or its EAs).
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80, 0x60, 0, 0];
    let mut m = Matcher::new(data);
    let mut pos = 0usize;
    while pos < data.len() {
        let block_end = (pos + BLOCK_IN).min(data.len());
        let mut tokens = Vec::new();
        while pos < block_end {
            let chunk_end = (pos + CHUNK_OUT).min(block_end);
            let mut lz = vec![1u8];
            let mut p = pos;
            while p < chunk_end {
                let at = WINDOW + p;
                m.index_to(at);
                match m.find(at, (chunk_end - p).min(MAX_MATCH)) {
                    Some((n, d)) => {
                        put_match(&mut lz, n, d);
                        p += n;
                    }
                    None => {
                        let b = data[p];
                        if b == ESCAPE {
                            lz.extend_from_slice(&[ESCAPE, 0x40]);
                        } else {
                            lz.push(b);
                        }
                        p += 1;
                    }
                }
            }
            let raw = &data[pos..chunk_end];
            let chunk: Vec<u8> = if lz.len() <= raw.len() {
                lz
            } else {
                let mut s = vec![0u8];
                s.extend_from_slice(raw);
                s
            };
            tokens.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
            tokens.extend_from_slice(&chunk);
            pos = chunk_end;
        }
        out.extend_from_slice(&BLOCK_MAGIC);
        out.extend_from_slice(&0xffffu16.to_le_bytes());
        out.extend_from_slice(&(tokens.len() as u16).to_le_bytes());
        out.extend_from_slice(&tokens);
    }
    out
}

/// A file to pack.
pub struct Source<'a> {
    /// The path stored in the header (`\OS2\FOO.EXE`, or just a name).
    pub path: &'a [u8],
    pub date: u16,
    pub time: u16,
    pub attributes: u32,
    pub data: &'a [u8],
    /// The file's extended attributes as a FEA2LIST (empty: none).
    pub eas: &'a [u8],
}

/// Append one entry to a packed file under construction. `file` holds the
/// entries so far (without the trailing count); the previous last entry's
/// next-header field is linked to the new one.
pub fn add_entry(file: &mut Vec<u8>, last_header: Option<usize>, src: &Source) -> usize {
    let at = file.len();
    if let Some(prev) = last_header {
        file[prev + 0x14..prev + 0x18].copy_from_slice(&(at as u32).to_le_bytes());
    }
    let mut path = src.path.to_vec();
    path.push(0);
    file.extend_from_slice(&MAGIC);
    file.extend_from_slice(&src.date.to_le_bytes());
    file.extend_from_slice(&src.time.to_le_bytes());
    file.extend_from_slice(&src.attributes.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes()); // EA offset, below
    file.extend_from_slice(&(src.data.len() as u32).to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes()); // next entry
    let mut name = [0u8; 9];
    name[..ALGORITHM.len()].copy_from_slice(ALGORITHM);
    file.extend_from_slice(&name);
    file.extend_from_slice(&ALGORITHM_VERSION.to_le_bytes());
    let ea_size = if src.eas.len() > 4 { src.eas.len() as u32 } else { 4 };
    file.extend_from_slice(&ea_size.to_le_bytes());
    file.extend_from_slice(&(path.len() as u16).to_le_bytes());
    debug_assert_eq!(file.len() - at, HEADER_SIZE);
    file.extend_from_slice(&path);
    file.extend(compress(src.data));
    if src.eas.len() > 4 {
        let ea_at = file.len() as u32;
        file[at + 0x0c..at + 0x10].copy_from_slice(&ea_at.to_le_bytes());
        file.extend(compress(src.eas));
    }
    at
}

/// A whole packed file from its entries.
pub fn pack(sources: &[Source]) -> Vec<u8> {
    let mut f = Vec::new();
    let mut last = None;
    for s in sources {
        last = Some(add_entry(&mut f, last, s));
    }
    f.extend_from_slice(&(sources.len() as u32).to_le_bytes());
    f
}
