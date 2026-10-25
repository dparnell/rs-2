//! 16-bit segmented executable (NE) format as used by OS/2 1.x.

use alloc::vec::Vec;

use crate::read::{Error, Reader};

/// Module flags (`ne_flags`).
pub mod flags {
    pub const SINGLE_DATA: u16 = 0x0001;
    pub const MULTIPLE_DATA: u16 = 0x0002;
    pub const PER_PROCESS_INIT: u16 = 0x0004;
    pub const PROTECTED_MODE: u16 = 0x0008;
    pub const PM_INCOMPATIBLE: u16 = 0x0100;
    pub const PM_COMPATIBLE: u16 = 0x0200;
    pub const PM_APP: u16 = 0x0300;
    /// Windows: the program loads its own segments (a loader in segment 1).
    pub const SELF_LOADING: u16 = 0x0800;
    pub const NOT_LOADABLE: u16 = 0x2000;
    pub const LIBRARY: u16 = 0x8000;
}

/// The target operating system byte (header offset 0x36).
pub mod target {
    pub const OS2: u8 = 1;
    pub const WINDOWS: u8 = 2;
}

/// Segment flags.
pub mod sflags {
    pub const DATA: u16 = 0x0001;
    pub const ITERATED: u16 = 0x0008;
    pub const MOVEABLE: u16 = 0x0010;
    pub const SHARED: u16 = 0x0020;
    pub const PRELOAD: u16 = 0x0040;
    /// Execute-only for code, read-only for data.
    pub const READONLY: u16 = 0x0080;
    pub const RELOCS: u16 = 0x0100;
    pub const CONFORMING: u16 = 0x0200;
    pub const DPL_MASK: u16 = 0x0c00;
    pub const DISCARDABLE: u16 = 0x1000;
    pub const BIG: u16 = 0x2000;
}

#[derive(Debug, Clone, Default)]
pub struct Header {
    pub entry_table: u16,
    pub entry_table_len: u16,
    pub flags: u16,
    pub autodata: u16,
    pub heap: u16,
    pub stack: u16,
    pub ip: u16,
    pub cs: u16,
    pub sp: u16,
    pub ss: u16,
    pub segment_count: u16,
    pub module_ref_count: u16,
    pub nonresident_len: u16,
    pub segment_table: u16,
    pub resource_table: u16,
    pub resident_names: u16,
    pub module_refs: u16,
    pub imported_names: u16,
    pub nonresident_names: u32,
    pub movable_entries: u16,
    pub align_shift: u16,
    pub resource_count: u16,
    pub target_os: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct Segment {
    /// File offset of the segment data (0 if none).
    pub file_offset: u32,
    /// Bytes of data in the file.
    pub file_size: u32,
    pub flags: u16,
    /// Minimum allocation in bytes.
    pub min_alloc: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    LoByte,
    Selector,
    Pointer1616,
    Offset16,
    Pointer1632,
    Offset32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Fixed segment (1-based) and offset.
    Internal { segment: u8, offset: u16 },
    /// Movable segment reached through the entry table.
    InternalEntry { ordinal: u16 },
    ImportOrdinal { module: u16, ordinal: u16 },
    ImportName { module: u16, name_offset: u16 },
    /// Floating point fixup (OSFIXUP).
    OsFixup { kind: u16 },
}

#[derive(Debug, Clone, Copy)]
pub struct Reloc {
    pub kind: SourceKind,
    pub additive: bool,
    pub offset: u16,
    pub target: Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Unused,
    Fixed { segment: u8, flags: u8, offset: u16 },
    Movable { segment: u8, flags: u8, offset: u16 },
    Constant { flags: u8, value: u16 },
}

pub struct Module<'a> {
    data: &'a [u8],
    base: usize,
    pub header: Header,
}

impl<'a> Module<'a> {
    pub fn parse(data: &'a [u8], base: usize) -> Result<Self, Error> {
        let r = Reader::new(data);
        if r.bytes(base, 2)? != b"NE" {
            return Err(Error::BadMagic);
        }
        let w = |o: usize| r.u16(base + o);
        let header = Header {
            entry_table: w(0x04)?,
            entry_table_len: w(0x06)?,
            flags: w(0x0c)?,
            autodata: w(0x0e)?,
            heap: w(0x10)?,
            stack: w(0x12)?,
            ip: w(0x14)?,
            cs: w(0x16)?,
            sp: w(0x18)?,
            ss: w(0x1a)?,
            segment_count: w(0x1c)?,
            module_ref_count: w(0x1e)?,
            nonresident_len: w(0x20)?,
            segment_table: w(0x22)?,
            resource_table: w(0x24)?,
            resident_names: w(0x26)?,
            module_refs: w(0x28)?,
            imported_names: w(0x2a)?,
            nonresident_names: r.u32(base + 0x2c)?,
            movable_entries: w(0x30)?,
            align_shift: w(0x32)?,
            resource_count: w(0x34)?,
            target_os: r.u8(base + 0x36)?,
        };
        Ok(Self { data, base, header })
    }

    fn r(&self) -> Reader<'a> {
        Reader::new(self.data)
    }

    fn rel(&self, off: u16) -> usize {
        self.base + off as usize
    }

    fn shift(&self) -> u32 {
        // A shift count of 0 means the default of 9 (512-byte sectors).
        if self.header.align_shift == 0 { 9 } else { self.header.align_shift as u32 }
    }

    pub fn segments(&self) -> Result<Vec<Segment>, Error> {
        let r = self.r();
        let mut v = Vec::new();
        for i in 0..self.header.segment_count as usize {
            let o = self.rel(self.header.segment_table) + i * 8;
            let sector = r.u16(o)? as u32;
            let len = r.u16(o + 2)? as u32;
            let flags = r.u16(o + 4)?;
            let min = r.u16(o + 6)? as u32;
            v.push(Segment {
                file_offset: sector << self.shift(),
                file_size: if sector == 0 { 0 } else if len == 0 { 0x10000 } else { len },
                flags,
                min_alloc: if min == 0 { 0x10000 } else { min },
            });
        }
        Ok(v)
    }

    /// Raw file data of a segment.
    pub fn segment_data(&self, seg: &Segment) -> Result<&'a [u8], Error> {
        self.r().bytes(seg.file_offset as usize, seg.file_size as usize)
    }

    /// Relocation records of a segment (empty if it has none).
    pub fn relocs(&self, seg: &Segment) -> Result<Vec<Reloc>, Error> {
        let mut v = Vec::new();
        if seg.flags & sflags::RELOCS == 0 || seg.file_offset == 0 {
            return Ok(v);
        }
        let r = self.r();
        let mut o = (seg.file_offset + seg.file_size) as usize;
        let n = r.u16(o)?;
        o += 2;
        for _ in 0..n {
            let kind = match r.u8(o)? {
                0 => SourceKind::LoByte,
                2 => SourceKind::Selector,
                3 => SourceKind::Pointer1616,
                5 => SourceKind::Offset16,
                11 => SourceKind::Pointer1632,
                13 => SourceKind::Offset32,
                _ => return Err(Error::Invalid("NE relocation source type")),
            };
            let flags = r.u8(o + 1)?;
            let offset = r.u16(o + 2)?;
            let a = r.u16(o + 4)?;
            let b = r.u16(o + 6)?;
            let target = match flags & 3 {
                0 => {
                    let segment = (a & 0xff) as u8;
                    if segment == 0xff {
                        Target::InternalEntry { ordinal: b }
                    } else {
                        Target::Internal { segment, offset: b }
                    }
                }
                1 => Target::ImportOrdinal { module: a, ordinal: b },
                2 => Target::ImportName { module: a, name_offset: b },
                _ => Target::OsFixup { kind: a },
            };
            v.push(Reloc { kind, additive: flags & 4 != 0, offset, target });
            o += 8;
        }
        Ok(v)
    }

    /// Name of the 1-based referenced module.
    pub fn import_module(&self, index: u16) -> Result<&'a [u8], Error> {
        if index == 0 || index > self.header.module_ref_count {
            return Err(Error::Invalid("module reference"));
        }
        let r = self.r();
        let off = r.u16(self.rel(self.header.module_refs) + (index as usize - 1) * 2)?;
        r.pstr(self.rel(self.header.imported_names) + off as usize)
    }

    pub fn import_modules(&self) -> Result<Vec<&'a [u8]>, Error> {
        (1..=self.header.module_ref_count).map(|i| self.import_module(i)).collect()
    }

    pub fn imported_name(&self, offset: u16) -> Result<&'a [u8], Error> {
        self.r().pstr(self.rel(self.header.imported_names) + offset as usize)
    }

    pub fn name(&self) -> Result<&'a [u8], Error> {
        self.r().pstr(self.rel(self.header.resident_names))
    }

    /// Exported names (resident then non-resident), excluding the module
    /// name and description.
    pub fn exported_names(&self) -> Result<Vec<(&'a [u8], u16)>, Error> {
        let r = self.r();
        let mut v = Vec::new();
        let mut table = |mut o: usize, end: usize| -> Result<(), Error> {
            let mut first = true;
            while o < end {
                let s = r.pstr(o)?;
                if s.is_empty() {
                    break;
                }
                let ord = r.u16(o + 1 + s.len())?;
                o += 3 + s.len();
                if !core::mem::take(&mut first) {
                    v.push((s, ord));
                }
            }
            Ok(())
        };
        table(self.rel(self.header.resident_names), usize::MAX)?;
        if self.header.nonresident_names != 0 {
            let o = self.header.nonresident_names as usize;
            table(o, o + self.header.nonresident_len as usize)?;
        }
        Ok(v)
    }

    /// Decode the entry table. Index 0 of the result is ordinal 1.
    pub fn entries(&self) -> Result<Vec<Entry>, Error> {
        let r = self.r();
        let mut o = self.rel(self.header.entry_table);
        let end = o + self.header.entry_table_len as usize;
        let mut v = Vec::new();
        while o < end {
            let count = r.u8(o)?;
            if count == 0 {
                break;
            }
            let seg = r.u8(o + 1)?;
            o += 2;
            for _ in 0..count {
                match seg {
                    0 => v.push(Entry::Unused),
                    0xff => {
                        v.push(Entry::Movable {
                            flags: r.u8(o)?,
                            segment: r.u8(o + 3)?,
                            offset: r.u16(o + 4)?,
                        });
                        o += 6;
                    }
                    0xfe => {
                        v.push(Entry::Constant { flags: r.u8(o)?, value: r.u16(o + 1)? });
                        o += 3;
                    }
                    s => {
                        v.push(Entry::Fixed { segment: s, flags: r.u8(o)?, offset: r.u16(o + 1)? });
                        o += 3;
                    }
                }
            }
        }
        Ok(v)
    }
}
