//! Reader for Intel/Microsoft/IBM Object Module Format (OMF) object modules
//! and libraries, the input of LINK and LINK386.
//!
//! An object module is a sequence of records (type byte, u16 length, body,
//! checksum). [`Object::parse`] turns one module into names, segments,
//! groups, symbols and data records with their fix-ups resolved to OMF's
//! indexes; the linker then maps those indexes onto its own tables. The
//! format is the Toolkit's `omfref.htm` (IBM's "OMF reference").

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// What went wrong reading a module (the linker reports L1101/L1102).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The data ends inside a record (L1102).
    Eof,
    /// A record that does not make sense (L1101), with the record type.
    Invalid(u8, &'static str),
}

/// Segment alignment (the A field of SEGDEF's ACBP byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Absolute,
    Byte,
    Word,
    Para,
    Page,
    Dword,
    Page4K,
}

impl Align {
    pub fn bytes(self) -> u32 {
        match self {
            Align::Absolute | Align::Byte => 1,
            Align::Word => 2,
            Align::Para => 16,
            Align::Page => 256,
            Align::Dword => 4,
            Align::Page4K => 4096,
        }
    }
}

/// Segment combination (the C field of ACBP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    Private,
    Public,
    Stack,
    Common,
}

#[derive(Debug, Clone)]
pub struct SegDef {
    pub name: String,
    pub class: String,
    pub overlay: String,
    pub align: Align,
    pub combine: Combine,
    pub use32: bool,
    pub length: u32,
    /// Frame number of an absolute segment.
    pub frame: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct GrpDef {
    pub name: String,
    /// 1-based segment indexes of this module.
    pub segs: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtKind {
    /// EXTDEF/LEXTDEF/CEXTDEF.
    Extern,
    /// COMDEF/LCOMDEF: a communal variable of this many bytes.
    Common { far: bool, size: u32 },
}

#[derive(Debug, Clone)]
pub struct Ext {
    pub name: String,
    /// LEXTDEF/LCOMDEF: refers to an LPUBDEF of the same module.
    pub local: bool,
    pub kind: ExtKind,
}

#[derive(Debug, Clone)]
pub struct Pub {
    pub name: String,
    pub group: u16,
    pub seg: u16,
    /// Frame number when `seg` is 0 (an absolute symbol).
    pub frame: u16,
    pub offset: u32,
    pub local: bool,
}

/// Location types of a fix-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loc {
    LoByte,
    Offset16,
    Base,
    Ptr1616,
    HiByte,
    /// Loader-resolved 16-bit offset (location 5).
    LoaderOffset16,
    Offset32,
    Ptr1632,
    LoaderOffset32,
}

impl Loc {
    pub fn size(self) -> usize {
        match self {
            Loc::LoByte | Loc::HiByte => 1,
            Loc::Offset16 | Loc::Base | Loc::LoaderOffset16 => 2,
            Loc::Ptr1616 | Loc::Offset32 | Loc::LoaderOffset32 => 4,
            Loc::Ptr1632 => 6,
        }
    }
}

/// How the frame (the segment or group an offset is relative to) is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    Seg(u16),
    Grp(u16),
    Ext(u16),
    Abs(u16),
    /// The frame of the location being fixed up.
    Location,
    /// The frame of the target.
    Target,
    None,
}

/// What a fix-up points at (indexes are this module's, 1-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Seg(u16),
    Grp(u16),
    Ext(u16),
    Abs(u16),
}

#[derive(Debug, Clone, Copy)]
pub struct Fixup {
    /// Offset of the location within the data record's bytes.
    pub offset: u32,
    pub loc: Loc,
    /// Segment-relative (M=1); self-relative otherwise.
    pub seg_rel: bool,
    pub frame: Frame,
    pub target: Target,
    pub disp: u32,
}

/// An LEDATA or (expanded) LIDATA record and its fix-ups.
#[derive(Debug, Clone)]
pub struct Data {
    pub seg: u16,
    pub offset: u32,
    pub bytes: Vec<u8>,
    pub fixups: Vec<Fixup>,
}

#[derive(Debug, Clone)]
pub struct Comdat {
    pub name: String,
    pub local: bool,
    pub continuation: bool,
    /// Selection criteria (high nibble of the attributes byte).
    pub select: u8,
    /// Allocation type: 0 explicit (in `seg`), 1 far code, 2 far data,
    /// 3 32-bit code, 4 32-bit data.
    pub alloc: u8,
    /// Alignment byte (0: the segment's).
    pub align: u8,
    pub group: u16,
    pub seg: u16,
    pub offset: u32,
    pub bytes: Vec<u8>,
    pub fixups: Vec<Fixup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImpEntry {
    Ordinal(u16),
    /// Imported by this name (the internal name when the record's was empty).
    Name(String),
}

/// COMENT A0/01: an import definition (from IMPLIB).
#[derive(Debug, Clone)]
pub struct ImpDef {
    pub internal: String,
    pub module: String,
    pub entry: ImpEntry,
}

/// COMENT A0/02: an export definition (`_export`).
#[derive(Debug, Clone)]
pub struct ExpDef {
    pub name: String,
    pub internal: String,
    pub ordinal: Option<u16>,
    pub resident: bool,
    pub nodata: bool,
    pub parms: u8,
}

/// BAKPAT/NBKPAT: values added to already placed data.
#[derive(Debug, Clone)]
pub struct BakPat {
    pub seg: u16,
    /// For NBKPAT, the COMDAT's name.
    pub comdat: Option<String>,
    /// 0 byte, 1 word, 2 dword.
    pub kind: u8,
    pub items: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, Default)]
pub struct Object {
    pub name: String,
    pub lnames: Vec<String>,
    pub segs: Vec<SegDef>,
    pub grps: Vec<GrpDef>,
    pub exts: Vec<Ext>,
    pub pubs: Vec<Pub>,
    pub data: Vec<Data>,
    pub comdats: Vec<Comdat>,
    pub bakpats: Vec<BakPat>,
    /// COMENT 9F: default libraries.
    pub libs: Vec<String>,
    pub impdefs: Vec<ImpDef>,
    pub expdefs: Vec<ExpDef>,
    /// COMENT A8: (weak extern index, default resolution index).
    pub weak: Vec<(u16, u16)>,
    /// ALIAS records: (alias, substitute).
    pub aliases: Vec<(String, String)>,
    /// COMENT 9E.
    pub dosseg: bool,
    /// COMENT A0/04: a protected-memory DLL.
    pub protmem: bool,
    /// MODEND's start address (location fields unused).
    pub start: Option<Fixup>,
    /// Bytes of the module in its file (libraries: to the MODEND).
    pub size: usize,
    /// Whether any record had line numbers or debug information.
    pub has_debug: bool,
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
    ty: u8,
}

impl<'a> Rd<'a> {
    fn eof(&self) -> bool {
        self.p >= self.b.len()
    }
    fn u8(&mut self) -> Result<u8, Error> {
        let v = *self.b.get(self.p).ok_or(Error::Invalid(self.ty, "record too short"))?;
        self.p += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(self.u8()? as u16 | (self.u8()? as u16) << 8)
    }
    fn u24(&mut self) -> Result<u32, Error> {
        Ok(self.u16()? as u32 | (self.u8()? as u32) << 16)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(self.u16()? as u32 | (self.u16()? as u32) << 16)
    }
    /// A 16- or 32-bit field depending on the record type's low bit.
    fn word(&mut self) -> Result<u32, Error> {
        if self.ty & 1 != 0 { self.u32() } else { Ok(self.u16()? as u32) }
    }
    fn index(&mut self) -> Result<u16, Error> {
        let a = self.u8()?;
        if a & 0x80 != 0 { Ok(((a as u16 & 0x7f) << 8) | self.u8()? as u16) } else { Ok(a as u16) }
    }
    fn name(&mut self) -> Result<String, Error> {
        let n = self.u8()? as usize;
        let s = self.b.get(self.p..self.p + n).ok_or(Error::Invalid(self.ty, "name too long"))?;
        self.p += n;
        Ok(s.iter().map(|&c| c as char).collect())
    }
    /// COMDEF's variable-length numbers.
    fn varlen(&mut self) -> Result<u32, Error> {
        match self.u8()? {
            n @ 0..=0x80 => Ok(n as u32),
            0x81 => Ok(self.u16()? as u32),
            0x84 => self.u24(),
            0x88 => self.u32(),
            _ => Err(Error::Invalid(self.ty, "communal length")),
        }
    }
    fn rest(&mut self) -> &'a [u8] {
        let r = &self.b[self.p.min(self.b.len())..];
        self.p = self.b.len();
        r
    }
}

/// A fix-up thread: method and index.
#[derive(Clone, Copy, Default)]
struct Thread {
    method: u8,
    index: u16,
    set: bool,
}

fn frame_of(method: u8, index: u16) -> Frame {
    match method {
        0 => Frame::Seg(index),
        1 => Frame::Grp(index),
        2 => Frame::Ext(index),
        3 => Frame::Abs(index),
        4 => Frame::Location,
        5 => Frame::Target,
        _ => Frame::None,
    }
}

fn target_of(method: u8, index: u16) -> Target {
    match method & 3 {
        0 => Target::Seg(index),
        1 => Target::Grp(index),
        2 => Target::Ext(index),
        _ => Target::Abs(index),
    }
}

/// Expand LIDATA's iterated blocks. `src` gets, for each output byte, its
/// offset in the record's data (fix-ups are given against those).
fn expand(r: &mut Rd, base: usize, out: &mut Vec<u8>, src: &mut Vec<u32>, depth: u32) -> Result<(), Error> {
    if depth > 16 {
        return Err(Error::Invalid(r.ty, "LIDATA nested too deep"));
    }
    let repeat = r.word()?;
    let blocks = r.u16()?;
    let start = r.p;
    if repeat > 0x0100_0000 {
        return Err(Error::Invalid(r.ty, "LIDATA record too large"));
    }
    if repeat == 0 {
        // Skip the block without producing anything.
        let mut o = Vec::new();
        let mut s = Vec::new();
        if blocks == 0 {
            let n = r.u8()? as usize;
            r.p += n;
        } else {
            for _ in 0..blocks {
                expand(r, base, &mut o, &mut s, depth + 1)?;
            }
        }
        return Ok(());
    }
    for _ in 0..repeat {
        r.p = start;
        if blocks == 0 {
            let n = r.u8()? as usize;
            let at = r.p;
            let b = r.b.get(at..at + n).ok_or(Error::Invalid(r.ty, "LIDATA content"))?;
            out.extend_from_slice(b);
            src.extend((at..at + n).map(|o| (o - base) as u32));
            r.p += n;
        } else {
            for _ in 0..blocks {
                expand(r, base, out, src, depth + 1)?;
            }
        }
        if out.len() > 0x0400_0000 {
            return Err(Error::Invalid(r.ty, "LIDATA record too large"));
        }
    }
    Ok(())
}

/// Where the next FIXUPP's fix-ups go.
enum Last {
    None,
    Data(usize, Option<Vec<u32>>),
    Comdat(usize, Option<Vec<u32>>),
}

impl Object {
    /// Parse one object module starting at the beginning of `data` (which
    /// may continue past its MODEND, as in a library).
    pub fn parse(data: &[u8]) -> Result<Object, Error> {
        let mut o = Object::default();
        let mut p = 0usize;
        let mut tthreads = [Thread::default(); 4];
        let mut fthreads = [Thread::default(); 4];
        let mut last = Last::None;
        let mut first = true;
        loop {
            if p + 3 > data.len() {
                return Err(Error::Eof);
            }
            let ty = data[p];
            let len = u16::from_le_bytes([data[p + 1], data[p + 2]]) as usize;
            if len == 0 || p + 3 + len > data.len() {
                return Err(Error::Eof);
            }
            // The body without the checksum byte.
            let body = &data[p + 3..p + 3 + len - 1];
            p += 3 + len;
            if first && ty != 0x80 && ty != 0x82 {
                return Err(Error::Invalid(ty, "module does not start with THEADR"));
            }
            first = false;
            let mut r = Rd { b: body, p: 0, ty };
            match ty {
                0x80 | 0x82 => {
                    let n = r.name()?;
                    if o.name.is_empty() {
                        o.name = n;
                    }
                }
                0x88 => o.coment(&mut r)?,
                0x8a | 0x8b => {
                    if !r.eof() {
                        let mt = r.u8()?;
                        if mt & 0x40 != 0 {
                            let f = Self::fixdat(&mut r, &fthreads, &tthreads, 0, Loc::Offset16, true)?;
                            o.start = Some(f);
                        }
                    }
                    break;
                }
                0x8c | 0xb4 | 0xbc => {
                    while !r.eof() {
                        let name = if ty == 0xbc {
                            let i = r.index()?;
                            o.lname(i, ty)?
                        } else {
                            r.name()?
                        };
                        r.index()?;
                        o.exts.push(Ext { name, local: ty == 0xb4, kind: ExtKind::Extern });
                    }
                }
                0xb0 | 0xb8 => {
                    while !r.eof() {
                        let name = r.name()?;
                        r.index()?;
                        let dt = r.u8()?;
                        let kind = match dt {
                            0x61 => {
                                let n = r.varlen()?;
                                let e = r.varlen()?;
                                ExtKind::Common { far: true, size: n.saturating_mul(e) }
                            }
                            0x62 => ExtKind::Common { far: false, size: r.varlen()? },
                            // Borland's segment-index form: treat as near.
                            1..=0x5f => ExtKind::Common { far: false, size: r.varlen()? },
                            _ => return Err(Error::Invalid(ty, "communal type")),
                        };
                        o.exts.push(Ext { name, local: ty == 0xb8, kind });
                    }
                }
                0x90 | 0x91 | 0xb6 | 0xb7 => {
                    let group = r.index()?;
                    let seg = r.index()?;
                    let frame = if seg == 0 { r.u16()? } else { 0 };
                    while !r.eof() {
                        let name = r.name()?;
                        let offset = r.word()?;
                        r.index()?;
                        o.pubs.push(Pub { name, group, seg, frame, offset, local: ty >= 0xb6 });
                    }
                }
                0x94 | 0x95 | 0xc4 | 0xc5 | 0x8e => o.has_debug = true,
                0x96 | 0xca => {
                    while !r.eof() {
                        let n = r.name()?;
                        o.lnames.push(n);
                    }
                }
                0x98 | 0x99 => {
                    let acbp = r.u8()?;
                    let a = acbp >> 5;
                    let align = match a {
                        0 => Align::Absolute,
                        1 => Align::Byte,
                        2 => Align::Word,
                        3 => Align::Para,
                        4 => Align::Page,
                        5 => Align::Dword,
                        6 => Align::Page4K,
                        _ => return Err(Error::Invalid(ty, "segment alignment")),
                    };
                    let frame = if a == 0 {
                        let f = r.u16()?;
                        r.u8()?;
                        Some(f)
                    } else {
                        None
                    };
                    let combine = match (acbp >> 2) & 7 {
                        0 => Combine::Private,
                        5 => Combine::Stack,
                        6 => Combine::Common,
                        _ => Combine::Public,
                    };
                    let mut length = r.word()?;
                    if acbp & 2 != 0 {
                        length = if ty == 0x99 { 0xffff_ffff } else { 0x10000 };
                    }
                    let name = r.index()?;
                    let class = r.index()?;
                    let ovl = r.index()?;
                    let (name, class) = (o.lname(name, ty)?, o.lname(class, ty)?);
                    let overlay = if ovl == 0 { String::new() } else { o.lname(ovl, ty).unwrap_or_default() };
                    let use32 = acbp & 1 != 0;
                    o.segs.push(SegDef { name, class, overlay, align, combine, use32, length, frame });
                }
                0x9a => {
                    let n = r.index()?;
                    let name = o.lname(n, ty)?;
                    let mut segs = Vec::new();
                    while !r.eof() {
                        match r.u8()? {
                            0xff => segs.push(r.index()?),
                            // Other component descriptors (old Intel forms)
                            // are not used for OS/2.
                            0xfe => {
                                r.index()?;
                            }
                            _ => return Err(Error::Invalid(ty, "group component")),
                        }
                    }
                    o.grps.push(GrpDef { name, segs });
                }
                0x9c | 0x9d => {
                    while !r.eof() {
                        let b = r.u8()?;
                        if b & 0x80 == 0 {
                            // A thread.
                            let d = b & 0x40 != 0;
                            let method = (b >> 2) & 7;
                            let num = (b & 3) as usize;
                            let index = if (d && method < 3) || (!d && method < 3) { r.index()? } else { 0 };
                            let t = Thread { method, index, set: true };
                            if d { fthreads[num] = t } else { tthreads[num] = t }
                            continue;
                        }
                        let b2 = r.u8()?;
                        let offset = (((b as u32) & 3) << 8) | b2 as u32;
                        let seg_rel = b & 0x40 != 0;
                        let loc = match (b >> 2) & 0xf {
                            0 => Loc::LoByte,
                            1 => Loc::Offset16,
                            2 => Loc::Base,
                            3 => Loc::Ptr1616,
                            4 => Loc::HiByte,
                            5 => Loc::LoaderOffset16,
                            9 => Loc::Offset32,
                            11 => Loc::Ptr1632,
                            13 => Loc::LoaderOffset32,
                            _ => return Err(Error::Invalid(ty, "fix-up location type")),
                        };
                        let f = Self::fixdat(&mut r, &fthreads, &tthreads, offset, loc, seg_rel)?;
                        match &mut last {
                            Last::Data(i, src) => push_fixup(&mut o.data[*i].fixups, f, src.as_deref()),
                            Last::Comdat(i, src) => push_fixup(&mut o.comdats[*i].fixups, f, src.as_deref()),
                            Last::None => return Err(Error::Invalid(ty, "fix-up without data")),
                        }
                    }
                    continue;
                }
                0xa0 | 0xa1 => {
                    let seg = r.index()?;
                    let offset = r.word()?;
                    let bytes = r.rest().to_vec();
                    o.data.push(Data { seg, offset, bytes, fixups: Vec::new() });
                    last = Last::Data(o.data.len() - 1, None);
                    continue;
                }
                0xa2 | 0xa3 => {
                    let seg = r.index()?;
                    let offset = r.word()?;
                    let base = r.p;
                    let mut bytes = Vec::new();
                    let mut src = Vec::new();
                    while !r.eof() {
                        expand(&mut r, base, &mut bytes, &mut src, 0)?;
                    }
                    o.data.push(Data { seg, offset, bytes, fixups: Vec::new() });
                    last = Last::Data(o.data.len() - 1, Some(src));
                    continue;
                }
                0xb2 | 0xb3 | 0xc8 | 0xc9 => {
                    let (seg, comdat) = if ty >= 0xc8 {
                        let kind = r.u8()?;
                        let n = r.index()?;
                        let name = o.lname(n, ty)?;
                        o.bakpat_items(&mut r, 0, Some(name), kind)?;
                        continue;
                    } else {
                        (r.index()?, None)
                    };
                    let kind = r.u8()?;
                    o.bakpat_items(&mut r, seg, comdat, kind)?;
                }
                0xc2 | 0xc3 => {
                    let flags = r.u8()?;
                    let attr = r.u8()?;
                    let align = r.u8()?;
                    let offset = r.word()?;
                    r.index()?;
                    let alloc = attr & 0xf;
                    let (group, seg) = if alloc == 0 {
                        let g = r.index()?;
                        let s = r.index()?;
                        if s == 0 {
                            r.u16()?;
                        }
                        (g, s)
                    } else {
                        (0, 0)
                    };
                    let n = r.index()?;
                    let name = o.lname(n, ty)?;
                    let base = r.p;
                    let (bytes, src) = if flags & 2 != 0 {
                        let mut b = Vec::new();
                        let mut s = Vec::new();
                        while !r.eof() {
                            expand(&mut r, base, &mut b, &mut s, 0)?;
                        }
                        (b, Some(s))
                    } else {
                        (r.rest().to_vec(), None)
                    };
                    o.comdats.push(Comdat {
                        name,
                        local: flags & 4 != 0,
                        continuation: flags & 1 != 0,
                        select: attr >> 4,
                        alloc,
                        align,
                        group,
                        seg,
                        offset,
                        bytes,
                        fixups: Vec::new(),
                    });
                    last = Last::Comdat(o.comdats.len() - 1, src);
                    continue;
                }
                0xc6 => {
                    while !r.eof() {
                        let a = r.name()?;
                        let s = r.name()?;
                        o.aliases.push((a, s));
                    }
                }
                // Overlay, block, debug-symbol and other obsolete records.
                0x84 | 0x86 | 0x92 | 0x8d | 0xa4 | 0xa6 | 0xa8 | 0xc0 => {}
                0xf0 | 0xf1 => return Err(Error::Invalid(ty, "library record inside a module")),
                _ => return Err(Error::Invalid(ty, "unknown record type")),
            }
            // Fix-ups follow only data records directly (COMENTs aside).
            if ty != 0x88 {
                last = Last::None;
            }
        }
        o.size = p;
        Ok(o)
    }

    fn lname(&self, i: u16, ty: u8) -> Result<String, Error> {
        if i == 0 {
            return Ok(String::new());
        }
        self.lnames.get(i as usize - 1).cloned().ok_or(Error::Invalid(ty, "name index"))
    }

    fn bakpat_items(&mut self, r: &mut Rd, seg: u16, comdat: Option<String>, kind: u8) -> Result<(), Error> {
        let mut items = Vec::new();
        while !r.eof() {
            let off = r.word()?;
            let val = r.word()?;
            items.push((off, val));
        }
        self.bakpats.push(BakPat { seg, comdat, kind, items });
        Ok(())
    }

    fn fixdat(r: &mut Rd, ft: &[Thread; 4], tt: &[Thread; 4], offset: u32, loc: Loc, seg_rel: bool) -> Result<Fixup, Error> {
        let fd = r.u8()?;
        let frame = if fd & 0x80 != 0 {
            let t = ft[((fd >> 4) & 3) as usize];
            if !t.set {
                return Err(Error::Invalid(r.ty, "undefined frame thread"));
            }
            frame_of(t.method, t.index)
        } else {
            let m = (fd >> 4) & 7;
            let i = if m < 3 { r.index()? } else if m == 3 { r.u16()? } else { 0 };
            frame_of(m, i)
        };
        let p = fd & 4 != 0;
        let target = if fd & 8 != 0 {
            let t = tt[(fd & 3) as usize];
            if !t.set {
                return Err(Error::Invalid(r.ty, "undefined target thread"));
            }
            target_of(t.method, t.index)
        } else {
            let m = fd & 3;
            let i = if m < 3 { r.index()? } else { r.u16()? };
            target_of(m, i)
        };
        let disp = if p { 0 } else { r.word()? };
        Ok(Fixup { offset, loc, seg_rel, frame, target, disp })
    }

    fn coment(&mut self, r: &mut Rd) -> Result<(), Error> {
        let _flags = r.u8()?;
        let class = r.u8()?;
        match class {
            0x9f | 0x81 => {
                let s: String = r.rest().iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
                let s = String::from(s.trim());
                if !s.is_empty() && !self.libs.iter().any(|l| l.eq_ignore_ascii_case(&s)) {
                    self.libs.push(s);
                }
            }
            0x9e => self.dosseg = true,
            0xa0 => {
                let sub = r.u8()?;
                match sub {
                    1 => {
                        let byord = r.u8()? != 0;
                        let internal = r.name()?;
                        let module = r.name()?;
                        let entry = if byord {
                            ImpEntry::Ordinal(r.u16()?)
                        } else {
                            let n = if r.eof() { String::new() } else { r.name()? };
                            ImpEntry::Name(if n.is_empty() { internal.clone() } else { n })
                        };
                        self.impdefs.push(ImpDef { internal, module, entry });
                    }
                    2 => {
                        let f = r.u8()?;
                        let name = r.name()?;
                        let internal = r.name()?;
                        let ordinal = if f & 0x80 != 0 { Some(r.u16()?) } else { None };
                        let internal = if internal.is_empty() { name.clone() } else { internal };
                        self.expdefs.push(ExpDef {
                            name,
                            internal,
                            ordinal,
                            resident: f & 0x40 != 0,
                            nodata: f & 0x20 != 0,
                            parms: f & 0x1f,
                        });
                    }
                    4 => self.protmem = true,
                    _ => {}
                }
            }
            0xa1 => self.has_debug = true,
            0xa8 => {
                while !r.eof() {
                    let w = r.index()?;
                    let d = r.index()?;
                    self.weak.push((w, d));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn push_fixup(v: &mut Vec<Fixup>, f: Fixup, src: Option<&[u32]>) {
    match src {
        None => v.push(f),
        Some(src) => {
            // LIDATA: once per copy of the byte the fix-up names.
            for (i, &s) in src.iter().enumerate() {
                if s == f.offset {
                    v.push(Fixup { offset: i as u32, ..f });
                }
            }
        }
    }
}

/// A library: its object modules and their offsets in the file.
pub struct Library {
    pub page: usize,
    pub case_sensitive: bool,
    /// File offsets of the modules.
    pub modules: Vec<usize>,
}

/// Whether `data` starts with a library header record.
pub fn is_library(data: &[u8]) -> bool {
    data.first() == Some(&0xf0)
}

impl Library {
    /// Find the modules of a library (the dictionary is not needed: the
    /// linker indexes the modules' public names itself).
    pub fn parse(data: &[u8]) -> Result<Library, Error> {
        if data.len() < 10 || data[0] != 0xf0 {
            return Err(Error::Invalid(0xf0, "not a library"));
        }
        let page = u16::from_le_bytes([data[1], data[2]]) as usize + 3;
        if !page.is_power_of_two() || page < 16 {
            return Err(Error::Invalid(0xf0, "library page size"));
        }
        let case_sensitive = data.get(9).is_some_and(|f| f & 1 != 0);
        let mut modules = vec![];
        let mut p = page;
        while p + 3 <= data.len() {
            match data[p] {
                0xf1 => break,
                0x80 | 0x82 => {}
                // Padding between modules.
                0 => {
                    p += 1;
                    continue;
                }
                _ => return Err(Error::Invalid(data[p], "library module")),
            }
            modules.push(p);
            // Walk the records to the MODEND.
            loop {
                if p + 3 > data.len() {
                    return Err(Error::Eof);
                }
                let ty = data[p];
                let len = u16::from_le_bytes([data[p + 1], data[p + 2]]) as usize;
                p += 3 + len;
                if ty == 0x8a || ty == 0x8b {
                    break;
                }
            }
            p = p.next_multiple_of(page);
        }
        Ok(Library { page, case_sensitive, modules })
    }
}
