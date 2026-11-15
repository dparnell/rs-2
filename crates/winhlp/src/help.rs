//! The internal files of a help file read into one structure: `|SYSTEM`,
//! `|FONT`, the topics and the lookup trees.

use crate::hfs::{cstr, u16_at, u32_at, BTree, Cur, Hfs};
use crate::topic::{self, Phrases, TopicFile, TL_TOPICHDR};
use crate::Error;
use alloc::vec::Vec;

/// A secondary window definition (`|SYSTEM` record 6).
#[derive(Debug, Clone, Default)]
pub struct Window {
    pub kind: Vec<u8>,
    pub name: Vec<u8>,
    pub caption: Vec<u8>,
    /// Position and size in 1/1024 of the screen (None: not given).
    pub x: Option<u16>,
    pub y: Option<u16>,
    pub cx: Option<u16>,
    pub cy: Option<u16>,
    pub maximize: bool,
}

/// A font descriptor of `|FONT`.
#[derive(Debug, Clone, Default)]
pub struct Font {
    /// 0x01 bold, 0x02 italic, 0x04 underline, 0x08 strikeout, 0x10
    /// double underline, 0x20 small caps.
    pub attr: u8,
    pub half_points: u8,
    /// 1 modern, 2 roman, 3 swiss, 4 script, 5 decorative.
    pub family: u8,
    pub face: Vec<u8>,
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
}

/// A topic: where its records are and what it is called.
#[derive(Debug, Clone)]
pub struct Topic {
    /// TOPICOFFSET (3.1) or TOPICPOS (3.0) of its header.
    pub offset: u32,
    pub title: Vec<u8>,
    /// Its topic number (3.1 header; 3.0: from `|TOMAP`).
    pub num: Option<u32>,
    /// The header's stream position.
    pub at: usize,
}

pub struct Help<'a> {
    pub hfs: Hfs<'a>,
    pub minor: u16,
    pub v30: bool,
    pub compressed: bool,
    pub title: Vec<u8>,
    pub copyright: Vec<u8>,
    /// The contents topic (TOPICOFFSET/TOPICPOS), if given.
    pub contents: Option<u32>,
    pub windows: Vec<Window>,
    /// Startup macros (`|SYSTEM` record 4).
    pub macros: Vec<Vec<u8>>,
    pub fonts: Vec<Font>,
    pub phrases: Option<Phrases>,
    pub topics: TopicFile,
    /// Topics in file order.
    pub list: Vec<Topic>,
    /// 3.0: `|TOMAP`, TOPICPOS by topic number.
    pub tomap: Vec<u32>,
}

impl<'a> Help<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let hfs = Hfs::new(data)?;
        let sys = hfs.file(b"|SYSTEM").ok_or(Error::Invalid("no |SYSTEM"))?;
        if u16_at(sys, 0) != Some(0x036c) {
            return Err(Error::Invalid("|SYSTEM magic"));
        }
        let minor = u16_at(sys, 2).ok_or(Error::Truncated)?;
        let flags = u16_at(sys, 10).ok_or(Error::Truncated)?;
        let v30 = minor <= 16;
        let compressed = !v30 && flags & 0x0c != 0;
        if !v30 && flags & 0x08 != 0 {
            // 2 KB blocks: WinHelp 4 files.
            return Err(Error::Invalid("WinHelp 4 topic blocks"));
        }
        let mut h = Help {
            hfs,
            minor,
            v30,
            compressed,
            title: Vec::new(),
            copyright: Vec::new(),
            contents: None,
            windows: Vec::new(),
            macros: Vec::new(),
            fonts: Vec::new(),
            phrases: None,
            topics: TopicFile::new(&[], v30, false),
            list: Vec::new(),
            tomap: Vec::new(),
        };
        if v30 {
            h.title = cstr(sys, 12).0.to_vec();
        } else {
            h.system_records(&sys[12.min(sys.len())..]);
        }
        h.fonts = fonts(h.hfs.file(b"|FONT").unwrap_or(&[]));
        h.phrases = if let Some(p) = h.hfs.file(b"|Phrases") {
            Phrases::old(p, v30, compressed)
        } else if let (Some(i), Some(m)) = (h.hfs.file(b"|PhrIndex"), h.hfs.file(b"|PhrImage")) {
            Phrases::hall(i, m)
        } else {
            None
        };
        let t = h.hfs.file(b"|TOPIC").ok_or(Error::Invalid("no |TOPIC"))?;
        h.topics = TopicFile::new(t, v30, compressed);
        if v30 {
            if let Some(m) = h.hfs.file(b"|TOMAP") {
                h.tomap = m.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            }
        }
        let mut list = Vec::new();
        h.topics.links(None, |l| {
            if l.kind == TL_TOPICHDR {
                let d1 = h.topics.data1(l);
                let num = topic::header(d1, v30).num;
                list.push(Topic { offset: l.offset, title: Vec::new(), num, at: l.at });
            }
            true
        });
        // Titles: the header's LinkData2 up to its first NUL (macros
        // follow), phrase-expanded.
        h.titles(&mut list);
        if v30 {
            // Topic numbers from |TOMAP.
            for (n, &p) in h.tomap.iter().enumerate().skip(1) {
                if let Ok(i) = list.binary_search_by_key(&p, |t| t.offset) {
                    list[i].num.get_or_insert(n as u32);
                }
            }
            if h.contents.is_none() {
                h.contents = h.tomap.first().copied();
            }
        }
        h.list = list;
        Ok(h)
    }

    fn titles(&self, list: &mut [Topic]) {
        // Walk again with phrases, only taking the headers' text.
        let mut i = 0;
        self.topics.links(self.phrases.as_ref(), |l| {
            if l.kind == TL_TOPICHDR {
                if let Some(t) = list.get_mut(i) {
                    t.title = cstr(&l.data2, 0).0.to_vec();
                }
                i += 1;
            }
            true
        });
    }

    /// `|SYSTEM` records (3.1): u16 type, u16 size, data.
    fn system_records(&mut self, d: &[u8]) {
        let mut c = Cur::new(d);
        while c.left() >= 4 {
            let (Some(ty), Some(n)) = (c.u16(), c.u16()) else { break };
            let Some(r) = c.bytes(n as usize) else { break };
            match ty {
                1 => self.title = cstr(r, 0).0.to_vec(),
                2 => self.copyright = cstr(r, 0).0.to_vec(),
                3 => self.contents = u32_at(r, 0),
                4 => self.macros.push(cstr(r, 0).0.to_vec()),
                6 if r.len() >= 90 => {
                    let f = u16_at(r, 0).unwrap_or(0);
                    let s = |o: usize, n: usize| cstr(&r[o..o + n], 0).0.to_vec();
                    let num = |o: usize, bit: u16| if f & bit != 0 { u16_at(r, o) } else { None };
                    self.windows.push(Window {
                        kind: s(2, 10),
                        name: s(12, 9),
                        caption: s(21, 51),
                        x: num(72, 0x08),
                        y: num(74, 0x10),
                        cx: num(76, 0x20),
                        cy: num(78, 0x40),
                        maximize: f & 0x80 != 0 && u16_at(r, 80).unwrap_or(0) != 0,
                    });
                }
                _ => {}
            }
        }
    }

    /// The topic (index into `list`) a TOPICOFFSET/TOPICPOS falls in.
    pub fn topic_at(&self, off: u32) -> Option<usize> {
        match self.list.binary_search_by_key(&off, |t| t.offset) {
            Ok(mut i) => {
                // Topics without text share an offset: the first.
                while i > 0 && self.list[i - 1].offset == off {
                    i -= 1;
                }
                Some(i)
            }
            Err(0) => None,
            Err(i) => Some(i - 1),
        }
    }

    /// 3.0: the topic of a topic number (|TOMAP).
    pub fn topic_by_num(&self, n: u32) -> Option<usize> {
        let p = *self.tomap.get(n as usize)?;
        if p == 0 {
            return None;
        }
        self.topic_at(p)
    }

    /// Where a 3.0 jump (a topic number) leads.
    pub fn jump_target(&self, v: u32) -> Option<usize> {
        if self.v30 { self.topic_by_num(v) } else { None }
    }

    /// `|CONTEXT`: (hash, topic offset), sorted by hash.
    pub fn contexts(&self) -> Vec<(u32, u32)> {
        let mut v = Vec::new();
        if let Some(t) = self.hfs.file(b"|CONTEXT").and_then(|d| BTree::new(d).ok()) {
            t.for_each(|e| {
                v.push((u32_at(e, 0)?, u32_at(e, 4)?));
                Some(8)
            });
        }
        v.sort_unstable();
        v
    }

    /// `|CTXOMAP`: (map number, topic offset).
    pub fn map(&self) -> Vec<(u32, u32)> {
        let mut v = Vec::new();
        if let Some(d) = self.hfs.file(b"|CTXOMAP") {
            let n = u16_at(d, 0).unwrap_or(0) as usize;
            for i in 0..n {
                let (Some(id), Some(off)) = (u32_at(d, 2 + 8 * i), u32_at(d, 6 + 8 * i)) else { break };
                v.push((id, off));
            }
        }
        v
    }

    /// `|KWBTREE` and `|KWDATA`: keywords and their topic offsets.
    pub fn keywords(&self) -> Vec<(Vec<u8>, Vec<u32>)> {
        let mut v = Vec::new();
        let (Some(t), Some(data)) = (self.hfs.file(b"|KWBTREE").and_then(|d| BTree::new(d).ok()), self.hfs.file(b"|KWDATA")) else {
            return v;
        };
        t.for_each(|e| {
            let (k, o) = cstr(e, 0);
            let n = u16_at(e, o)? as usize;
            let at = u32_at(e, o + 2)? as usize;
            let offs = (0..n).filter_map(|i| u32_at(data, at + 4 * i)).collect();
            v.push((k.to_vec(), offs));
            Some(o + 6)
        });
        v
    }

    /// `|TTLBTREE`: (topic offset, title).
    pub fn ttl(&self) -> Vec<(u32, Vec<u8>)> {
        let mut v = Vec::new();
        if let Some(t) = self.hfs.file(b"|TTLBTREE").and_then(|d| BTree::new(d).ok()) {
            t.for_each(|e| {
                let off = u32_at(e, 0)?;
                let (s, o) = cstr(e, 4);
                v.push((off, s.to_vec()));
                Some(o)
            });
        }
        v
    }
}

/// `|FONT`: u16 face names, u16 descriptors, u16 offset of the names,
/// u16 offset of the descriptors; names of a fixed size (20 bytes in
/// 3.x); descriptors of 11 bytes: u8 attributes, u8 half points, u8
/// family, u16 face, u8 fg\[3\], u8 bg\[3\].
fn fonts(d: &[u8]) -> Vec<Font> {
    let mut v = Vec::new();
    let (Some(nf), Some(nd), Some(fo), Some(dof)) = (u16_at(d, 0), u16_at(d, 2), u16_at(d, 4), u16_at(d, 6)) else {
        return v;
    };
    let (nf, nd, fo, dof) = (nf as usize, nd as usize, fo as usize, dof as usize);
    let fsize = if nf > 0 && dof > fo { (dof - fo) / nf } else { 20 };
    for i in 0..nd {
        let o = dof + 11 * i;
        let Some(r) = d.get(o..o + 11) else { break };
        let face_i = u16_at(r, 3).unwrap_or(0) as usize;
        let face = if face_i < nf {
            let s = fo + face_i * fsize;
            cstr(d.get(s..(s + fsize).min(d.len())).unwrap_or(&[]), 0).0.to_vec()
        } else {
            Vec::new()
        };
        v.push(Font { attr: r[0], half_points: r[1], family: r[2], face, fg: (r[5], r[6], r[7]), bg: (r[8], r[9], r[10]) });
    }
    v
}

/// WinHelp's context string hash: for each character, hash * 43 plus the
/// character's value (letters case-insensitive), wrapping at 32 bits.
pub fn hash(s: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in s {
        h = h.wrapping_mul(43).wrapping_add(char_value(c) as i32 as u32);
    }
    h
}

/// The values: digits 1-9 are 1-9 and 0 is 10, '.' 12, '_' 13, letters
/// 17-42 (either case); other characters their code minus 0x30 (as a
/// signed byte).
fn char_value(c: u8) -> i8 {
    match c {
        0 => 0,
        b'0' => 10,
        b'1'..=b'9' => (c - b'0') as i8,
        b'a'..=b'z' => (c - b'a' + 0x11) as i8,
        b'!' => 0x0b,
        b'.' => 0x0c,
        b'_' => 0x0d,
        _ => c.wrapping_sub(0x30) as i8,
    }
}
