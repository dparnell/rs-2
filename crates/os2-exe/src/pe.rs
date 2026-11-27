//! The Portable Executable format (PE32) of 32-bit Windows programs and
//! DLLs, as rs2's Win32 layer loads them (docs/WIN32.md).
//!
//! Only what a loader needs: the file and optional headers, the sections,
//! the data directories, the import and export tables, base relocations
//! and the TLS directory. Addresses in the image are relative virtual
//! addresses (RVAs) from the image base; [`Module::rva`] finds the bytes
//! of one in the file.

use alloc::vec::Vec;

use crate::read::{Error, Reader};

/// `IMAGE_FILE_*` characteristics.
pub mod chars {
    pub const RELOCS_STRIPPED: u16 = 0x0001;
    pub const EXECUTABLE_IMAGE: u16 = 0x0002;
    pub const LARGE_ADDRESS_AWARE: u16 = 0x0020;
    pub const MACHINE_32BIT: u16 = 0x0100;
    pub const DLL: u16 = 0x2000;
}

/// `IMAGE_SCN_*` section characteristics.
pub mod scn {
    pub const CNT_CODE: u32 = 0x0000_0020;
    pub const CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
    pub const CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;
    pub const MEM_DISCARDABLE: u32 = 0x0200_0000;
    pub const MEM_SHARED: u32 = 0x1000_0000;
    pub const MEM_EXECUTE: u32 = 0x2000_0000;
    pub const MEM_READ: u32 = 0x4000_0000;
    pub const MEM_WRITE: u32 = 0x8000_0000;
}

/// Data directory indexes.
pub mod dir {
    pub const EXPORT: usize = 0;
    pub const IMPORT: usize = 1;
    pub const RESOURCE: usize = 2;
    pub const EXCEPTION: usize = 3;
    pub const SECURITY: usize = 4;
    pub const BASERELOC: usize = 5;
    pub const DEBUG: usize = 6;
    pub const TLS: usize = 9;
    pub const LOAD_CONFIG: usize = 10;
    pub const BOUND_IMPORT: usize = 11;
    pub const IAT: usize = 12;
    pub const DELAY_IMPORT: usize = 13;
    pub const CLR: usize = 14;
}

/// Subsystems.
pub mod subsystem {
    pub const NATIVE: u16 = 1;
    pub const WINDOWS_GUI: u16 = 2;
    pub const WINDOWS_CUI: u16 = 3;
}

pub const MACHINE_I386: u16 = 0x014c;
pub const MAGIC_PE32: u16 = 0x010b;
pub const MAGIC_PE32_PLUS: u16 = 0x020b;

/// The file header and the PE32 optional header.
#[derive(Debug, Clone, Default)]
pub struct Header {
    pub machine: u16,
    pub sections: u16,
    pub timestamp: u32,
    pub optional_size: u16,
    pub characteristics: u16,
    pub magic: u16,
    pub linker: (u8, u8),
    pub code_size: u32,
    pub entry: u32,
    pub image_base: u32,
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub os_version: (u16, u16),
    pub subsystem_version: (u16, u16),
    pub image_size: u32,
    pub headers_size: u32,
    pub subsystem: u16,
    pub dll_characteristics: u16,
    pub stack_reserve: u32,
    pub stack_commit: u32,
    pub heap_reserve: u32,
    pub heap_commit: u32,
    /// (RVA, size) of each data directory.
    pub dirs: Vec<(u32, u32)>,
}

impl Header {
    pub fn is_dll(&self) -> bool {
        self.characteristics & chars::DLL != 0
    }

    pub fn relocs_stripped(&self) -> bool {
        self.characteristics & chars::RELOCS_STRIPPED != 0
    }

    /// A data directory, if present.
    pub fn dir(&self, i: usize) -> Option<(u32, u32)> {
        self.dirs.get(i).copied().filter(|&(rva, size)| rva != 0 && size != 0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Section {
    pub name: [u8; 8],
    pub virtual_size: u32,
    pub rva: u32,
    pub raw_size: u32,
    pub raw_offset: u32,
    pub characteristics: u32,
}

impl Section {
    pub fn name(&self) -> &[u8] {
        let n = self.name.iter().position(|&b| b == 0).unwrap_or(8);
        &self.name[..n]
    }

    /// The size the section takes in memory.
    pub fn mem_size(&self) -> u32 {
        if self.virtual_size != 0 { self.virtual_size } else { self.raw_size }
    }
}

/// What an import names: an ordinal or a name (with its hint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportBy<'a> {
    Ordinal(u16),
    Name { hint: u16, name: &'a [u8] },
}

/// One imported module: its name, and for each entry of its import
/// address table the RVA of the slot and what goes in it.
#[derive(Debug, Clone)]
pub struct Import<'a> {
    pub dll: &'a [u8],
    pub entries: Vec<(u32, ImportBy<'a>)>,
}

/// An export: at an RVA in the image, or forwarded ("DLL.Name",
/// "DLL.#12").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Export<'a> {
    Rva(u32),
    Forward(&'a [u8]),
}

/// The export directory: the module's name, the first ordinal, the
/// entries by ordinal (index `ordinal - base`, None for an unused one) and
/// the names with the index of their entry.
#[derive(Debug, Clone, Default)]
pub struct Exports<'a> {
    pub name: &'a [u8],
    pub base: u32,
    pub entries: Vec<Option<Export<'a>>>,
    pub names: Vec<(&'a [u8], u16)>,
}

/// The TLS directory (as virtual addresses, which base relocations
/// cover).
#[derive(Debug, Clone, Copy, Default)]
pub struct Tls {
    pub start: u32,
    pub end: u32,
    pub index: u32,
    pub callbacks: u32,
    pub zero_fill: u32,
    pub characteristics: u32,
}

/// Base relocation types.
pub mod reloc {
    pub const ABSOLUTE: u8 = 0;
    pub const HIGH: u8 = 1;
    pub const LOW: u8 = 2;
    pub const HIGHLOW: u8 = 3;
    pub const HIGHADJ: u8 = 4;
}

pub struct Module<'a> {
    data: &'a [u8],
    pub header: Header,
    pub sections: Vec<Section>,
}

fn cstr<'a>(r: &Reader<'a>, at: usize) -> Result<&'a [u8], Error> {
    let mut n = 0;
    loop {
        if r.u8(at + n)? == 0 {
            return r.bytes(at, n);
        }
        n += 1;
        if n > 4096 {
            return Err(Error::Invalid("string"));
        }
    }
}

impl<'a> Module<'a> {
    /// Parse a PE image whose `PE\0\0` signature is at `header`.
    pub fn parse(data: &'a [u8], header: usize) -> Result<Self, Error> {
        let r = Reader::new(data);
        if r.bytes(header, 4)? != b"PE\0\0" {
            return Err(Error::BadMagic);
        }
        let f = header + 4;
        let mut h = Header {
            machine: r.u16(f)?,
            sections: r.u16(f + 2)?,
            timestamp: r.u32(f + 4)?,
            optional_size: r.u16(f + 16)?,
            characteristics: r.u16(f + 18)?,
            ..Default::default()
        };
        let o = f + 20;
        h.magic = r.u16(o)?;
        if h.magic != MAGIC_PE32 {
            return Err(Error::Unsupported("PE32+ image"));
        }
        h.linker = (r.u8(o + 2)?, r.u8(o + 3)?);
        h.code_size = r.u32(o + 4)?;
        h.entry = r.u32(o + 16)?;
        h.image_base = r.u32(o + 28)?;
        h.section_alignment = r.u32(o + 32)?;
        h.file_alignment = r.u32(o + 36)?;
        h.os_version = (r.u16(o + 40)?, r.u16(o + 42)?);
        h.subsystem_version = (r.u16(o + 48)?, r.u16(o + 50)?);
        h.image_size = r.u32(o + 56)?;
        h.headers_size = r.u32(o + 60)?;
        h.subsystem = r.u16(o + 68)?;
        h.dll_characteristics = r.u16(o + 70)?;
        h.stack_reserve = r.u32(o + 72)?;
        h.stack_commit = r.u32(o + 76)?;
        h.heap_reserve = r.u32(o + 80)?;
        h.heap_commit = r.u32(o + 84)?;
        let ndirs = (r.u32(o + 92)? as usize).min(16);
        for i in 0..ndirs {
            if 96 + i * 8 + 8 > h.optional_size as usize {
                break;
            }
            h.dirs.push((r.u32(o + 96 + i * 8)?, r.u32(o + 100 + i * 8)?));
        }
        let mut sections = Vec::with_capacity(h.sections as usize);
        let s0 = o + h.optional_size as usize;
        for i in 0..h.sections as usize {
            let s = s0 + i * 40;
            let mut name = [0u8; 8];
            name.copy_from_slice(r.bytes(s, 8)?);
            sections.push(Section {
                name,
                virtual_size: r.u32(s + 8)?,
                rva: r.u32(s + 12)?,
                raw_size: r.u32(s + 16)?,
                raw_offset: r.u32(s + 20)?,
                characteristics: r.u32(s + 36)?,
            });
        }
        if h.image_size == 0 {
            return Err(Error::Invalid("image size"));
        }
        Ok(Module { data, header: h, sections })
    }

    /// The file offset of an RVA (in the headers or a section's raw data).
    pub fn offset(&self, rva: u32) -> Option<usize> {
        if rva < self.header.headers_size.max(self.sections.first().map_or(0, |s| s.rva.min(0x1000))) {
            return Some(rva as usize);
        }
        self.sections.iter().find_map(|s| {
            (rva >= s.rva && rva - s.rva < s.raw_size.min(s.mem_size().max(s.raw_size))).then(|| (s.raw_offset + rva - s.rva) as usize)
        })
    }

    /// `len` bytes at an RVA, from the file.
    pub fn rva(&self, rva: u32, len: usize) -> Result<&'a [u8], Error> {
        let o = self.offset(rva).ok_or(Error::Invalid("RVA"))?;
        Reader::new(self.data).bytes(o, len)
    }

    fn u32_at(&self, rva: u32) -> Result<u32, Error> {
        let b = self.rva(rva, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u16_at(&self, rva: u32) -> Result<u16, Error> {
        let b = self.rva(rva, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn str_at(&self, rva: u32) -> Result<&'a [u8], Error> {
        let o = self.offset(rva).ok_or(Error::Invalid("RVA"))?;
        cstr(&Reader::new(self.data), o)
    }

    /// The image as it is laid out in memory (headers and sections at
    /// their RVAs, uninitialised parts zero), `image_size` bytes rounded
    /// up to pages.
    pub fn image(&self) -> Result<Vec<u8>, Error> {
        let size = (self.header.image_size as usize).next_multiple_of(0x1000);
        let mut img = alloc::vec![0u8; size];
        let hs = (self.header.headers_size as usize).min(self.data.len()).min(size);
        img[..hs].copy_from_slice(&self.data[..hs]);
        for s in &self.sections {
            let n = s.raw_size.min(s.mem_size().max(s.raw_size)) as usize;
            let n = n.min(self.data.len().saturating_sub(s.raw_offset as usize));
            let at = s.rva as usize;
            if s.raw_offset == 0 || n == 0 || at >= size {
                continue;
            }
            let n = n.min(size - at);
            img[at..at + n].copy_from_slice(&self.data[s.raw_offset as usize..s.raw_offset as usize + n]);
        }
        Ok(img)
    }

    /// The import table.
    pub fn imports(&self) -> Result<Vec<Import<'a>>, Error> {
        let mut v = Vec::new();
        let Some((mut at, _)) = self.header.dir(dir::IMPORT) else { return Ok(v) };
        loop {
            let ilt = self.u32_at(at)?;
            let name = self.u32_at(at + 12)?;
            let iat = self.u32_at(at + 16)?;
            if name == 0 && iat == 0 {
                break;
            }
            let dll = self.str_at(name)?;
            // The lookup table names the imports; without one (old
            // Borland linkers), the address table does.
            let mut t = if ilt != 0 { ilt } else { iat };
            let mut slot = iat;
            let mut entries = Vec::new();
            loop {
                let e = self.u32_at(t)?;
                if e == 0 {
                    break;
                }
                let by = if e & 0x8000_0000 != 0 {
                    ImportBy::Ordinal(e as u16)
                } else {
                    ImportBy::Name { hint: self.u16_at(e)?, name: self.str_at(e + 2)? }
                };
                entries.push((slot, by));
                t += 4;
                slot += 4;
                if entries.len() > 65536 {
                    return Err(Error::Invalid("import table"));
                }
            }
            v.push(Import { dll, entries });
            at += 20;
            if v.len() > 4096 {
                return Err(Error::Invalid("import directory"));
            }
        }
        Ok(v)
    }

    /// The export table.
    pub fn exports(&self) -> Result<Exports<'a>, Error> {
        let Some((at, size)) = self.header.dir(dir::EXPORT) else { return Ok(Exports::default()) };
        let name = self.u32_at(at + 12)?;
        let base = self.u32_at(at + 16)?;
        let nfuncs = self.u32_at(at + 20)?.min(65536);
        let nnames = self.u32_at(at + 24)?.min(65536);
        let funcs = self.u32_at(at + 28)?;
        let names = self.u32_at(at + 32)?;
        let ords = self.u32_at(at + 36)?;
        let mut e = Exports { name: if name != 0 { self.str_at(name)? } else { b"" }, base, ..Default::default() };
        for i in 0..nfuncs {
            let rva = self.u32_at(funcs + i * 4)?;
            e.entries.push(if rva == 0 {
                None
            } else if rva >= at && rva < at + size {
                Some(Export::Forward(self.str_at(rva)?))
            } else {
                Some(Export::Rva(rva))
            });
        }
        for i in 0..nnames {
            let n = self.str_at(self.u32_at(names + i * 4)?)?;
            e.names.push((n, self.u16_at(ords + i * 2)?));
        }
        Ok(e)
    }

    /// The base relocations: (RVA, type).
    pub fn relocations(&self) -> Result<Vec<(u32, u8)>, Error> {
        let mut v = Vec::new();
        let Some((mut at, size)) = self.header.dir(dir::BASERELOC) else { return Ok(v) };
        let end = at + size;
        while at + 8 <= end {
            let page = self.u32_at(at)?;
            let len = self.u32_at(at + 4)?;
            if len < 8 {
                break;
            }
            for i in 0..(len - 8) / 2 {
                let e = self.u16_at(at + 8 + i * 2)?;
                let ty = (e >> 12) as u8;
                if ty != reloc::ABSOLUTE {
                    v.push((page + (e & 0xfff) as u32, ty));
                }
            }
            at += len;
        }
        Ok(v)
    }

    /// The TLS directory.
    pub fn tls(&self) -> Result<Option<Tls>, Error> {
        let Some((at, _)) = self.header.dir(dir::TLS) else { return Ok(None) };
        Ok(Some(Tls {
            start: self.u32_at(at)?,
            end: self.u32_at(at + 4)?,
            index: self.u32_at(at + 8)?,
            callbacks: self.u32_at(at + 12)?,
            zero_fill: self.u32_at(at + 16)?,
            characteristics: self.u32_at(at + 20)?,
        }))
    }
}

/// Apply base relocations to an image laid out by [`Module::image`],
/// moved by `delta` (new base - preferred base).
pub fn relocate(img: &mut [u8], relocs: &[(u32, u8)], delta: u32) {
    let rd = |img: &[u8], at: usize| img.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    for &(rva, ty) in relocs {
        let at = rva as usize;
        match ty {
            reloc::HIGHLOW => {
                if let Some(v) = rd(img, at) {
                    img[at..at + 4].copy_from_slice(&v.wrapping_add(delta).to_le_bytes());
                }
            }
            reloc::HIGH => {
                if let Some(b) = img.get_mut(at..at + 2) {
                    let v = u16::from_le_bytes([b[0], b[1]]).wrapping_add((delta >> 16) as u16);
                    b.copy_from_slice(&v.to_le_bytes());
                }
            }
            reloc::LOW => {
                if let Some(b) = img.get_mut(at..at + 2) {
                    let v = u16::from_le_bytes([b[0], b[1]]).wrapping_add(delta as u16);
                    b.copy_from_slice(&v.to_le_bytes());
                }
            }
            _ => {}
        }
    }
}
