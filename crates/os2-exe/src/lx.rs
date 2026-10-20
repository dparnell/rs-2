//! 32-bit linear executable (LX) format.
//!
//! Field offsets follow the "IBM OS/2 16/32-bit Object Module Format (OMF)
//! and Linear eXecutable Module Format (LX)" specification.

use alloc::vec::Vec;

use crate::read::{Error, Reader};

/// Module flags (`e32_mflags`).
pub mod mflags {
    pub const PER_PROCESS_INIT: u32 = 0x0000_0004;
    pub const INTERNAL_FIXUPS_REMOVED: u32 = 0x0000_0010;
    pub const EXTERNAL_FIXUPS_REMOVED: u32 = 0x0000_0020;
    pub const PM_INCOMPATIBLE: u32 = 0x0000_0100;
    pub const PM_COMPATIBLE: u32 = 0x0000_0200;
    pub const PM_APP: u32 = 0x0000_0300;
    pub const APP_TYPE_MASK: u32 = 0x0000_0700;
    pub const NOT_LOADABLE: u32 = 0x0000_2000;
    pub const MODULE_TYPE_MASK: u32 = 0x0003_8000;
    pub const PROGRAM: u32 = 0x0000_0000;
    pub const LIBRARY: u32 = 0x0000_8000;
    pub const PROT_LIBRARY: u32 = 0x0001_8000;
    pub const PHYS_DEVICE: u32 = 0x0002_0000;
    pub const VIRT_DEVICE: u32 = 0x0002_8000;
    pub const PER_PROCESS_TERM: u32 = 0x4000_0000;
}

/// Object flags (`o32_flags`).
pub mod oflags {
    pub const READ: u32 = 0x0001;
    pub const WRITE: u32 = 0x0002;
    pub const EXEC: u32 = 0x0004;
    pub const RESOURCE: u32 = 0x0008;
    pub const DISCARDABLE: u32 = 0x0010;
    pub const SHARED: u32 = 0x0020;
    pub const PRELOAD: u32 = 0x0040;
    pub const INVALID: u32 = 0x0080;
    pub const ZEROFILL: u32 = 0x0100;
    pub const RESIDENT: u32 = 0x0200;
    pub const RESIDENT_CONTIG: u32 = 0x0300;
    pub const RESIDENT_LONG_LOCKABLE: u32 = 0x0400;
    pub const ALIAS_16_16: u32 = 0x1000;
    pub const BIG: u32 = 0x2000;
    pub const CONFORMING: u32 = 0x4000;
    pub const IOPL: u32 = 0x8000;
}

/// Parsed LX header (the subset of fields the loader needs).
#[derive(Debug, Clone, Default)]
pub struct Header {
    pub cpu: u16,
    pub os: u16,
    pub module_version: u32,
    pub module_flags: u32,
    pub page_count: u32,
    pub eip_object: u32,
    pub eip: u32,
    pub esp_object: u32,
    pub esp: u32,
    pub page_size: u32,
    pub page_shift: u32,
    pub fixup_size: u32,
    pub loader_size: u32,
    pub object_table: u32,
    pub object_count: u32,
    pub page_table: u32,
    pub iter_pages: u32,
    pub resource_table: u32,
    pub resource_count: u32,
    pub resident_names: u32,
    pub entry_table: u32,
    pub fixup_page_table: u32,
    pub fixup_records: u32,
    pub import_modules: u32,
    pub import_module_count: u32,
    pub import_procs: u32,
    pub data_pages: u32,
    pub preload_pages: u32,
    pub nonresident_names: u32,
    pub nonresident_names_len: u32,
    pub autodata_object: u32,
    pub debug_info: u32,
    pub debug_len: u32,
    pub heap_size: u32,
    pub stack_size: u32,
}

impl Header {
    pub fn is_library(&self) -> bool {
        matches!(
            self.module_flags & mflags::MODULE_TYPE_MASK,
            mflags::LIBRARY | mflags::PROT_LIBRARY
        )
    }
}

/// One entry of the object table.
#[derive(Debug, Clone, Copy)]
pub struct Object {
    pub virtual_size: u32,
    pub base: u32,
    pub flags: u32,
    /// 1-based index into the object page table.
    pub page_index: u32,
    pub page_count: u32,
}

impl Object {
    pub fn is_32bit(&self) -> bool {
        self.flags & oflags::BIG != 0
    }
}

/// How a page is stored in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Legal,
    Iterated,
    Invalid,
    Zeroed,
    Range,
    Compressed,
}

#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub offset: u32,
    pub size: u16,
    pub kind: PageKind,
}

/// Target of a fixup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Object number (1-based) and offset within it.
    Internal { object: u16, offset: u32 },
    /// Imported by ordinal: module ordinal (1-based) and procedure ordinal.
    ImportOrdinal { module: u16, ordinal: u32 },
    /// Imported by name: module ordinal and offset into the import procedure
    /// name table.
    ImportName { module: u16, name_offset: u32 },
    /// Internal reference through the entry table (1-based entry ordinal).
    InternalEntry { ordinal: u16 },
}

/// Kind of location being patched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Byte,
    Selector16,
    Pointer1616,
    Offset16,
    Pointer1632,
    Offset32,
    Relative32,
}

impl SourceKind {
    fn from_raw(v: u8) -> Result<Self, Error> {
        Ok(match v & 0x0f {
            0x0 => SourceKind::Byte,
            0x2 => SourceKind::Selector16,
            0x3 => SourceKind::Pointer1616,
            0x5 => SourceKind::Offset16,
            0x6 => SourceKind::Pointer1632,
            0x7 => SourceKind::Offset32,
            0x8 => SourceKind::Relative32,
            _ => return Err(Error::Invalid("fixup source type")),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Fixup {
    pub kind: SourceKind,
    /// Fixup applies to the 16:16 alias of the target.
    pub alias: bool,
    /// Offsets within the page being fixed up. May be negative when a fixup
    /// straddles the page boundary from the previous page.
    pub sources: Vec<i16>,
    pub target: Target,
    pub additive: u32,
    /// An internal fixup chain (target flag 0x08, from Warp 4.5's linker):
    /// `sources[0]` is the first location, the rest follow from the page
    /// contents (see [`fixup_chain`]).
    pub chained: bool,
}

/// An entry point exported through the entry table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Unused,
    Entry16 { object: u16, flags: u8, offset: u16 },
    CallGate { object: u16, flags: u8, offset: u16, selector: u16 },
    Entry32 { object: u16, flags: u8, offset: u32 },
    /// Forwarded to another module. If `by_ordinal`, `value` is an ordinal,
    /// otherwise an offset into the import procedure name table.
    Forwarder { module: u16, by_ordinal: bool, value: u32 },
}

/// One resource table entry.
#[derive(Debug, Clone, Copy)]
pub struct Resource {
    pub type_id: u16,
    pub name_id: u16,
    pub size: u32,
    pub object: u16,
    pub offset: u32,
}

/// A parsed LX module borrowing the file contents.
pub struct Module<'a> {
    data: &'a [u8],
    /// Offset of the LX header in the file.
    base: usize,
    pub header: Header,
}

impl<'a> Module<'a> {
    /// Parse the LX header located at `base`.
    pub fn parse(data: &'a [u8], base: usize) -> Result<Self, Error> {
        let r = Reader::new(data);
        if r.bytes(base, 2)? != b"LX" {
            return Err(Error::BadMagic);
        }
        if r.u8(base + 2)? != 0 || r.u8(base + 3)? != 0 {
            return Err(Error::Unsupported("big-endian LX"));
        }
        let f = |off: usize| r.u32(base + off);
        let header = Header {
            cpu: r.u16(base + 0x08)?,
            os: r.u16(base + 0x0a)?,
            module_version: f(0x0c)?,
            module_flags: f(0x10)?,
            page_count: f(0x14)?,
            eip_object: f(0x18)?,
            eip: f(0x1c)?,
            esp_object: f(0x20)?,
            esp: f(0x24)?,
            page_size: f(0x28)?,
            page_shift: f(0x2c)?,
            fixup_size: f(0x30)?,
            loader_size: f(0x38)?,
            object_table: f(0x40)?,
            object_count: f(0x44)?,
            page_table: f(0x48)?,
            iter_pages: f(0x4c)?,
            resource_table: f(0x50)?,
            resource_count: f(0x54)?,
            resident_names: f(0x58)?,
            entry_table: f(0x5c)?,
            fixup_page_table: f(0x68)?,
            fixup_records: f(0x6c)?,
            import_modules: f(0x70)?,
            import_module_count: f(0x74)?,
            import_procs: f(0x78)?,
            data_pages: f(0x80)?,
            preload_pages: f(0x84)?,
            nonresident_names: f(0x88)?,
            nonresident_names_len: f(0x8c)?,
            autodata_object: f(0x94)?,
            debug_info: f(0x98)?,
            debug_len: f(0x9c)?,
            heap_size: f(0xa8)?,
            stack_size: f(0xac)?,
        };
        if header.page_size == 0 || header.page_size > 0x10000 {
            return Err(Error::Invalid("page size"));
        }
        Ok(Self { data, base, header })
    }

    fn r(&self) -> Reader<'a> {
        Reader::new(self.data)
    }

    /// Offset in the file of a header-relative table.
    fn rel(&self, off: u32) -> usize {
        self.base + off as usize
    }

    pub fn objects(&self) -> Result<Vec<Object>, Error> {
        let r = self.r();
        let mut v = Vec::with_capacity(self.header.object_count as usize);
        for i in 0..self.header.object_count as usize {
            let o = self.rel(self.header.object_table) + i * 24;
            v.push(Object {
                virtual_size: r.u32(o)?,
                base: r.u32(o + 4)?,
                flags: r.u32(o + 8)?,
                page_index: r.u32(o + 12)?,
                page_count: r.u32(o + 16)?,
            });
        }
        Ok(v)
    }

    /// Object page table entry for 1-based page number `page`.
    pub fn page(&self, page: u32) -> Result<Page, Error> {
        if page == 0 || page > self.header.page_count {
            return Err(Error::Invalid("page number"));
        }
        let r = self.r();
        let o = self.rel(self.header.page_table) + (page as usize - 1) * 8;
        let kind = match r.u16(o + 6)? {
            0 => PageKind::Legal,
            1 => PageKind::Iterated,
            2 => PageKind::Invalid,
            3 => PageKind::Zeroed,
            4 => PageKind::Range,
            5 => PageKind::Compressed,
            _ => return Err(Error::Invalid("page flags")),
        };
        Ok(Page { offset: r.u32(o)?, size: r.u16(o + 4)?, kind })
    }

    /// Decode 1-based page `page` into `out`, which must be `page_size`
    /// bytes long. Bytes beyond the stored data are zeroed.
    pub fn load_page(&self, page: u32, out: &mut [u8]) -> Result<(), Error> {
        let p = self.page(page)?;
        out.fill(0);
        let file_off = |p: &Page| -> usize {
            let base = match p.kind {
                PageKind::Iterated if self.header.iter_pages != 0 => self.header.iter_pages,
                _ => self.header.data_pages,
            };
            base as usize + ((p.offset as usize) << self.header.page_shift)
        };
        match p.kind {
            PageKind::Zeroed | PageKind::Invalid => Ok(()),
            PageKind::Legal => {
                let src = self.r().bytes(file_off(&p), p.size as usize)?;
                let n = src.len().min(out.len());
                out[..n].copy_from_slice(&src[..n]);
                Ok(())
            }
            PageKind::Iterated => {
                let src = self.r().bytes(file_off(&p), p.size as usize)?;
                expand_iterated(src, out)
            }
            PageKind::Compressed => {
                let src = self.r().bytes(file_off(&p), p.size as usize)?;
                expand_exepack2(src, out).map(|_| ())
            }
            PageKind::Range => Err(Error::Unsupported("range pages")),
        }
    }

    /// Fixup records for 1-based page `page`.
    pub fn fixups(&self, page: u32) -> Result<Vec<Fixup>, Error> {
        if page == 0 || page > self.header.page_count {
            return Err(Error::Invalid("page number"));
        }
        let r = self.r();
        let fpt = self.rel(self.header.fixup_page_table) + (page as usize - 1) * 4;
        let start = r.u32(fpt)? as usize;
        let end = r.u32(fpt + 4)? as usize;
        let base = self.rel(self.header.fixup_records);
        let mut pos = base + start;
        let end = base + end;
        let mut out = Vec::new();
        while pos < end {
            let (fixup, next) = parse_fixup(&r, pos)?;
            out.push(fixup);
            pos = next;
        }
        Ok(out)
    }

    /// Name of 1-based import module `index`.
    pub fn import_module(&self, index: u16) -> Result<&'a [u8], Error> {
        let r = self.r();
        let mut o = self.rel(self.header.import_modules);
        for _ in 1..index {
            o += 1 + r.u8(o)? as usize;
        }
        r.pstr(o)
    }

    pub fn import_modules(&self) -> Result<Vec<&'a [u8]>, Error> {
        let r = self.r();
        let mut o = self.rel(self.header.import_modules);
        let mut v = Vec::new();
        for _ in 0..self.header.import_module_count {
            let s = r.pstr(o)?;
            o += 1 + s.len();
            v.push(s);
        }
        Ok(v)
    }

    /// Procedure name at `offset` in the import procedure name table.
    pub fn import_proc_name(&self, offset: u32) -> Result<&'a [u8], Error> {
        self.r().pstr(self.rel(self.header.import_procs) + offset as usize)
    }

    /// Module name (first entry of the resident name table).
    pub fn name(&self) -> Result<&'a [u8], Error> {
        self.r().pstr(self.rel(self.header.resident_names))
    }

    /// All exported names as `(name, ordinal)` pairs, resident and
    /// non-resident, excluding the module name itself.
    pub fn exported_names(&self) -> Result<Vec<(&'a [u8], u16)>, Error> {
        let mut v = Vec::new();
        let r = self.r();
        read_name_table(&r, self.rel(self.header.resident_names), usize::MAX, &mut v)?;
        if self.header.nonresident_names != 0 {
            read_name_table(
                &r,
                self.header.nonresident_names as usize,
                self.header.nonresident_names_len as usize,
                &mut v,
            )?;
        }
        Ok(v)
    }

    /// Decode the entry table. Index 0 of the result is ordinal 1.
    pub fn entries(&self) -> Result<Vec<Entry>, Error> {
        let r = self.r();
        let mut o = self.rel(self.header.entry_table);
        let mut v = Vec::new();
        loop {
            let count = r.u8(o)?;
            if count == 0 {
                break;
            }
            let ty = r.u8(o + 1)?;
            o += 2;
            if ty == 0 {
                v.extend(core::iter::repeat_n(Entry::Unused, count as usize));
                continue;
            }
            let object = r.u16(o)?;
            o += 2;
            for _ in 0..count {
                let flags = r.u8(o)?;
                let e = match ty {
                    1 => {
                        let e = Entry::Entry16 { object, flags, offset: r.u16(o + 1)? };
                        o += 3;
                        e
                    }
                    2 => {
                        let e = Entry::CallGate {
                            object,
                            flags,
                            offset: r.u16(o + 1)?,
                            selector: r.u16(o + 3)?,
                        };
                        o += 5;
                        e
                    }
                    3 => {
                        let e = Entry::Entry32 { object, flags, offset: r.u32(o + 1)? };
                        o += 5;
                        e
                    }
                    4 => {
                        let e = Entry::Forwarder {
                            module: r.u16(o + 1)?,
                            by_ordinal: flags & 1 != 0,
                            value: r.u32(o + 3)?,
                        };
                        o += 7;
                        e
                    }
                    _ => return Err(Error::Invalid("entry bundle type")),
                };
                v.push(e);
            }
        }
        Ok(v)
    }

    pub fn resources(&self) -> Result<Vec<Resource>, Error> {
        let r = self.r();
        let mut v = Vec::new();
        for i in 0..self.header.resource_count as usize {
            let o = self.rel(self.header.resource_table) + i * 14;
            v.push(Resource {
                type_id: r.u16(o)?,
                name_id: r.u16(o + 2)?,
                size: r.u32(o + 4)?,
                object: r.u16(o + 8)?,
                offset: r.u32(o + 10)?,
            });
        }
        Ok(v)
    }
}

fn read_name_table<'a>(
    r: &Reader<'a>,
    mut o: usize,
    limit: usize,
    out: &mut Vec<(&'a [u8], u16)>,
) -> Result<(), Error> {
    let end = o.saturating_add(limit);
    let mut first = true;
    while o < end {
        let s = r.pstr(o)?;
        if s.is_empty() {
            break;
        }
        let ord = r.u16(o + 1 + s.len())?;
        o += 3 + s.len();
        if first {
            // The first entry names the module (resident) or holds the
            // description (non-resident).
            first = false;
            continue;
        }
        out.push((s, ord));
    }
    Ok(())
}

fn parse_fixup(r: &Reader<'_>, mut pos: usize) -> Result<(Fixup, usize), Error> {
    let src = r.u8(pos)?;
    let flags = r.u8(pos + 1)?;
    pos += 2;
    let kind = SourceKind::from_raw(src)?;
    let alias = src & 0x10 != 0;
    let list = src & 0x20 != 0;
    let mut sources = Vec::new();
    let mut count = 0;
    if list {
        count = r.u8(pos)?;
        pos += 1;
    } else {
        sources.push(r.u16(pos)? as i16);
        pos += 2;
    }
    let wide_ord = flags & 0x40 != 0;
    let read_ord = |pos: &mut usize| -> Result<u16, Error> {
        if wide_ord {
            let v = r.u16(*pos)?;
            *pos += 2;
            Ok(v)
        } else {
            let v = r.u8(*pos)? as u16;
            *pos += 1;
            Ok(v)
        }
    };
    let wide_off = flags & 0x10 != 0;
    let read_off = |pos: &mut usize| -> Result<u32, Error> {
        if wide_off {
            let v = r.u32(*pos)?;
            *pos += 4;
            Ok(v)
        } else {
            let v = r.u16(*pos)? as u32;
            *pos += 2;
            Ok(v)
        }
    };
    let target = match flags & 3 {
        0 => {
            let object = read_ord(&mut pos)?;
            let offset = if kind == SourceKind::Selector16 { 0 } else { read_off(&mut pos)? };
            Target::Internal { object, offset }
        }
        1 => {
            let module = read_ord(&mut pos)?;
            let ordinal = if flags & 0x80 != 0 {
                let v = r.u8(pos)? as u32;
                pos += 1;
                v
            } else {
                read_off(&mut pos)?
            };
            Target::ImportOrdinal { module, ordinal }
        }
        2 => {
            let module = read_ord(&mut pos)?;
            let name_offset = read_off(&mut pos)?;
            Target::ImportName { module, name_offset }
        }
        _ => Target::InternalEntry { ordinal: read_ord(&mut pos)? },
    };
    let mut additive = 0;
    if flags & 0x04 != 0 {
        if flags & 0x20 != 0 {
            additive = r.u32(pos)?;
            pos += 4;
        } else {
            additive = r.u16(pos)? as u32;
            pos += 2;
        }
    }
    for _ in 0..count {
        sources.push(r.u16(pos)? as i16);
        pos += 2;
    }
    Ok((Fixup { kind, alias, sources, target, additive, chained: flags & 0x08 != 0 }, pos))
}

/// Walk an internal fixup chain in a page before it is patched: the
/// record gives the first source and its target offset; every source dword
/// holds the page offset of the next source in its top 12 bits (0xFFF ends
/// the chain) and, after the first, its own target offset in the low 20
/// bits. Returns `(source, target offset)` pairs.
pub fn fixup_chain(page: &[u8], first: i16, first_target: u32) -> Vec<(i16, u32)> {
    let mut out = Vec::new();
    let (mut src, mut target) = (first, first_target);
    while src >= 0 && (src as usize) + 4 <= page.len() && out.len() < page.len() / 4 {
        let at = src as usize;
        let raw = u32::from_le_bytes([page[at], page[at + 1], page[at + 2], page[at + 3]]);
        out.push((src, target));
        let next = raw >> 20;
        if next == 0xfff {
            break;
        }
        src = next as i16;
        let at = src as usize;
        if at + 4 > page.len() {
            break;
        }
        target = u32::from_le_bytes([page[at], page[at + 1], page[at + 2], page[at + 3]]) & 0xf_ffff;
    }
    out
}

/// Expand an EXEPACK:1 ("iterated data") page.
pub fn expand_iterated(mut src: &[u8], out: &mut [u8]) -> Result<(), Error> {
    let mut d = 0;
    while src.len() >= 4 {
        let iters = u16::from_le_bytes([src[0], src[1]]) as usize;
        let len = u16::from_le_bytes([src[2], src[3]]) as usize;
        if iters == 0 {
            break;
        }
        let pat = src.get(4..4 + len).ok_or(Error::Invalid("iterated record"))?;
        for _ in 0..iters {
            let dst = out.get_mut(d..d + len).ok_or(Error::Invalid("iterated overflow"))?;
            dst.copy_from_slice(pat);
            d += len;
        }
        src = &src[4 + len..];
    }
    Ok(())
}

/// Expand an EXEPACK:2 compressed page. Returns the number of bytes
/// produced.
///
/// Mirrors the kernel's `EP2_32_Decompress`: the output bound is only
/// checked between opcodes and back-references are not checked against the
/// start of the page. A handful of shipped modules rely on both, so bytes
/// beyond `out` are dropped and bytes before it read as zero.
pub fn expand_exepack2(src: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    let bad = Error::Invalid("exepack2 stream");
    let mut s = 0;
    let mut d = 0;
    let byte = |i: usize| src.get(i).copied().ok_or(bad);
    let put = |out: &mut [u8], i: usize, v: u8| {
        if let Some(b) = out.get_mut(i) {
            *b = v;
        }
    };
    let literal = |out: &mut [u8], s: usize, n: usize, d: usize| -> Result<(), Error> {
        let from = src.get(s..s + n).ok_or(bad)?;
        for (i, &b) in from.iter().enumerate() {
            put(out, d + i, b);
        }
        Ok(())
    };
    // Byte-wise copy: the source may overlap the destination.
    let back = |out: &mut [u8], d: usize, dist: usize, n: usize| {
        for i in 0..n {
            let v = (d + i).checked_sub(dist).and_then(|s| out.get(s).copied()).unwrap_or(0);
            put(out, d + i, v);
        }
    };

    while s < src.len() && d < out.len() {
        let b0 = byte(s)?;
        match b0 & 3 {
            0 => {
                if b0 == 0 {
                    let n = byte(s + 1)? as usize;
                    if n == 0 {
                        break;
                    }
                    let fill = byte(s + 2)?;
                    for i in 0..n {
                        put(out, d + i, fill);
                    }
                    s += 3;
                    d += n;
                } else {
                    let n = (b0 >> 2) as usize;
                    literal(out, s + 1, n, d)?;
                    s += 1 + n;
                    d += n;
                }
            }
            1 => {
                let w = u16::from_le_bytes([b0, byte(s + 1)?]) as usize;
                let lit = (w >> 2) & 3;
                let len = ((w >> 4) & 7) + 3;
                let dist = w >> 7;
                literal(out, s + 2, lit, d)?;
                s += 2 + lit;
                d += lit;
                back(out, d, dist, len);
                d += len;
            }
            2 => {
                let w = u16::from_le_bytes([b0, byte(s + 1)?]) as usize;
                let len = ((w >> 2) & 3) + 3;
                let dist = w >> 4;
                s += 2;
                back(out, d, dist, len);
                d += len;
            }
            _ => {
                let v = u32::from_le_bytes([b0, byte(s + 1)?, byte(s + 2)?, 0]) as usize;
                let lit = (v >> 2) & 0xf;
                let len = (v >> 6) & 0x3f;
                let dist = (v >> 12) & 0xfff;
                literal(out, s + 3, lit, d)?;
                s += 3 + lit;
                d += lit;
                back(out, d, dist, len);
                d += len;
            }
        }
    }
    Ok(d.min(out.len()))
}

#[cfg(test)]
mod tests {
    use super::fixup_chain;
    use alloc::vec;

    #[test]
    fn chain_follows_links_in_the_page() {
        // As in HELPMGR.DLL: a switch table at 0x550/0x55c behind a head at 0x54a.
        let mut page = vec![0u8; 0x1000];
        page[0x54a..0x54e].copy_from_slice(&0x55c0_0000u32.to_le_bytes());
        page[0x55c..0x560].copy_from_slice(&0x5500_8570u32.to_le_bytes());
        page[0x550..0x554].copy_from_slice(&0xfff0_8578u32.to_le_bytes());
        assert_eq!(fixup_chain(&page, 0x54a, 0x8550), [(0x54a, 0x8550), (0x55c, 0x8570), (0x550, 0x8578)]);
    }
}
