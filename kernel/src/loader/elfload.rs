//! Loading 64-bit programs and DLLs (ELF64, docs/API64.md).
//!
//! A 64-bit module is an x86-64 ELF shared object (`ET_DYN`) linked with
//! rs2's `link64.ld`: every `PT_LOAD` segment starts on a 64 KB boundary,
//! so each one becomes a loader [`Object`] as an LX object would. The image
//! is position independent; all segments move by one delta.
//!
//! Imports are undefined dynamic symbols named as `elf2lx` names them:
//! `MODULE.123` (by ordinal) or `MODULE.Name` (by name). Kernel API modules
//! (DOSCALLS...) resolve to the same gate stubs 32-bit programs use; other
//! modules are 64-bit DLLs (loader::load_global64). Exports are the
//! module's defined dynamic symbols. A data symbol `RS2_APPTYPE` (a u32)
//! gives a program's type (3 = PM; 2, window compatible, by default).
//! A DLL that exports `_DLL_InitTerm` has it called as
//! `unsigned long _DLL_InitTerm(unsigned long hmod, unsigned long flag)`
//! (System V), once per process: flag 0 when the process starts using
//! the DLL, 1 when it stops; 0 back from an initialisation fails the load
//! (api/module.rs runs these as it runs LX DLLs' init/term routines).

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use super::{err, Export, Format, Module, Object, Start, Target};
use crate::vm::{self, pag, AddressSpace, PageState, SegKind, SharedMem, PAGE, TILE};

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PF_X: u32 = 1;
const PF_W: u32 = 2;

const DT_NULL: u64 = 0;
const DT_PLTRELSZ: u64 = 2;
const DT_HASH: u64 = 4;
const DT_STRTAB: u64 = 5;
const DT_SYMTAB: u64 = 6;
const DT_RELA: u64 = 7;
const DT_RELASZ: u64 = 8;
const DT_STRSZ: u64 = 10;
const DT_JMPREL: u64 = 23;

const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;

/// Is this an x86-64 ELF64 file?
pub fn is_elf64(data: &[u8]) -> bool {
    data.len() >= 64 && &data[..4] == b"\x7fELF" && data[4] == 2 && data[5] == 1 && u16le(data, 18) == 62
}

fn u16le(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]))
}
fn u32le(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()))
}
fn u64le(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8).map_or(0, |x| u64::from_le_bytes(x.try_into().unwrap()))
}

struct Seg {
    vaddr: u64,
    offset: u64,
    filesz: u64,
    memsz: u64,
    flags: u32,
}

struct Sym {
    name: String,
    value: u64,
    shndx: u16,
    bind: u8,
}

/// The loaded image of all segments, addressed by link-time virtual
/// address (`lo` is the lowest segment's).
struct Image {
    lo: u64,
    bytes: Vec<u8>,
}

impl Image {
    fn slice(&self, vaddr: u64, len: usize) -> Option<&[u8]> {
        let o = vaddr.checked_sub(self.lo)? as usize;
        self.bytes.get(o..o.checked_add(len)?)
    }
    fn put_u64(&mut self, vaddr: u64, v: u64) -> bool {
        let Some(o) = vaddr.checked_sub(self.lo).map(|o| o as usize) else { return false };
        match self.bytes.get_mut(o..o + 8) {
            Some(d) => {
                d.copy_from_slice(&v.to_le_bytes());
                true
            }
            None => false,
        }
    }
}

/// The loadable segments (by address) and the dynamic section's address.
fn segments(d: &[u8]) -> (Vec<Seg>, Option<u64>) {
    let phoff = u64le(d, 32) as usize;
    let phentsize = u16le(d, 54) as usize;
    let phnum = u16le(d, 56) as usize;
    let mut segs = Vec::new();
    let mut dynamic = None;
    for i in 0..phnum {
        let p = phoff + i * phentsize;
        let ty = u32le(d, p);
        let seg = Seg {
            flags: u32le(d, p + 4),
            offset: u64le(d, p + 8),
            vaddr: u64le(d, p + 16),
            filesz: u64le(d, p + 32),
            memsz: u64le(d, p + 40),
        };
        match ty {
            PT_LOAD if seg.memsz > 0 => segs.push(seg),
            PT_DYNAMIC => dynamic = Some(seg.vaddr),
            _ => {}
        }
    }
    segs.sort_by_key(|s| s.vaddr);
    (segs, dynamic)
}

/// A resource of a 64-bit module: (address, size). The module's resource
/// table is its exported `RS2_RESOURCES` (os2-rc's `bind::table64`):
/// "RS2RES" and two NULs, a u32 count, a u32 0, then per resource a u16
/// type, a u16 id, a u32 offset from the table, a u32 size and a u32 0.
pub fn resource(m: &Module, ty: u16, id: u16) -> Option<(u32, u32)> {
    let d = &m.image.as_ref()?[..];
    let ord = *m.names.get("RS2_RESOURCES")?;
    let Some(Export::At(t)) = m.exports.get(ord as usize - 1) else { return None };
    let (segs, _) = segments(d);
    let delta = (m.objects.first()?.base as u64).wrapping_sub(segs.first()?.vaddr);
    let vaddr = (t.flat as u64).wrapping_sub(delta);
    let seg = segs.iter().find(|s| (s.vaddr..s.vaddr + s.filesz).contains(&vaddr))?;
    let table = d.get((seg.offset + vaddr - seg.vaddr) as usize..)?;
    if table.get(..8)? != b"RS2RES\0\0" {
        return None;
    }
    let count = u32le(table, 8) as usize;
    (0..count.min(4096)).map(|i| 16 + i * 16).find_map(|e| {
        (u16le(table, e) == ty && u16le(table, e + 2) == id).then(|| (t.flat + u32le(table, e + 4), u32le(table, e + 8)))
    })
}

pub fn load(path: &str, data: Arc<[u8]>, dll: bool, specific: bool, mut space: Option<&mut AddressSpace>) -> Result<Arc<Module>, u32> {
    let d = &data[..];
    if !is_elf64(d) || u16le(d, 16) != 3 {
        return Err(err::BAD_EXE_FORMAT);
    }
    let entry = u64le(d, 24);
    let (segs, dynamic) = segments(d);
    if segs.is_empty() {
        return Err(err::BAD_EXE_FORMAT);
    }
    // Phase 1 of docs/API64.md: everything below 4 GB.
    let lo = segs[0].vaddr;
    let hi = segs.iter().map(|s| s.vaddr + s.memsz).max().unwrap();
    if segs.iter().any(|s| s.vaddr % TILE as u64 != 0) || hi - lo > 0x4000_0000 || hi > u32::MAX as u64 {
        crate::println!("{path}: 64-bit module not linked with link64.ld (segments must start on 64 KB)");
        return Err(err::BAD_EXE_FORMAT);
    }
    let span = (hi - lo) as u32;

    // The image, as linked.
    let mut img = Image { lo, bytes: vec![0u8; (span as usize).next_multiple_of(PAGE as usize)] };
    for s in &segs {
        let at = (s.vaddr - lo) as usize;
        let src = d.get(s.offset as usize..(s.offset + s.filesz) as usize).ok_or(err::BAD_EXE_FORMAT)?;
        img.bytes[at..at + src.len()].copy_from_slice(src);
    }

    // Choose the base: the link address when it is free, else anywhere
    // (the image is position independent).
    let base: u32 = if dll {
        match vm::reserve_shared(span, Some(lo as u32)) {
            Some(b) => b,
            None => vm::reserve_shared(span, None).ok_or(err::NOT_ENOUGH_MEMORY)?,
        }
    } else {
        let sp = space.as_deref_mut().unwrap();
        let fits = |b: u32| segs.iter().all(|s| sp.is_free(b + (s.vaddr - lo) as u32, s.memsz as u32));
        if fits(lo as u32) {
            lo as u32
        } else {
            let mut b = vm::ARENA_START;
            while !fits(b) {
                b += TILE;
                if b + span >= vm::TILED_TOP {
                    return Err(err::NOT_ENOUGH_MEMORY);
                }
            }
            b
        }
    };
    let delta = (base as u64).wrapping_sub(lo);
    if crate::api::tracing() {
        crate::trace!("    {path}: 64-bit image at {base:x} ({span:x} bytes)");
    }

    // The dynamic section.
    let mut tags: BTreeMap<u64, u64> = BTreeMap::new();
    if let Some(dv) = dynamic {
        let mut at = dv;
        loop {
            let Some(e) = img.slice(at, 16) else { break };
            let (tag, val) = (u64le(e, 0), u64le(e, 8));
            if tag == DT_NULL {
                break;
            }
            tags.entry(tag).or_insert(val);
            at += 16;
        }
    }
    let strtab = tags.get(&DT_STRTAB).copied().unwrap_or(0);
    let strsz = tags.get(&DT_STRSZ).copied().unwrap_or(0) as usize;
    let symtab = tags.get(&DT_SYMTAB).copied().unwrap_or(0);
    let strings = if strtab != 0 { img.slice(strtab, strsz).ok_or(err::BAD_EXE_FORMAT)?.to_vec() } else { Vec::new() };
    let cstr = |o: u32| -> String {
        let o = o as usize;
        let s = strings.get(o..).unwrap_or(&[]);
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        s[..end].iter().map(|&b| b as char).collect()
    };
    // Symbol count: the SysV hash table's chain count.
    let nsyms = match tags.get(&DT_HASH) {
        Some(&h) => img.slice(h + 4, 4).map_or(0, |b| u32le(b, 0)) as usize,
        None => 0,
    };
    let mut syms = Vec::with_capacity(nsyms);
    for i in 0..nsyms {
        let Some(e) = img.slice(symtab + i as u64 * 24, 24) else { return Err(err::BAD_EXE_FORMAT) };
        syms.push(Sym { name: cstr(u32le(e, 0)), bind: e[4] >> 4, shndx: u16le(e, 6), value: u64le(e, 8) });
    }

    // Exports: the defined global symbols, numbered in symbol order.
    let mut exports = Vec::new();
    let mut names = BTreeMap::new();
    let mut app_type = 2;
    let mut init_term = None;
    for s in &syms {
        if s.shndx == 0 || s.name.is_empty() || (s.bind != 1 && s.bind != 2) {
            continue;
        }
        let addr = s.value.wrapping_add(delta) as u32;
        if s.name == "RS2_APPTYPE" {
            if let Some(b) = img.slice(s.value, 4) {
                app_type = u32le(b, 0);
            }
        }
        if s.name == "_DLL_InitTerm" {
            init_term = Some(addr);
        }
        exports.push(Export::At(Target::flat32(addr, true)));
        names.insert(s.name.clone(), exports.len() as u16);
    }

    // Imports: the modules undefined symbols name.
    let own = super::module_name(path);
    let mut import_names: Vec<String> = Vec::new();
    for s in &syms {
        if s.shndx == 0 && !s.name.is_empty() {
            if let Some((m, _)) = s.name.split_once('.') {
                let m = super::module_name(m);
                if !import_names.contains(&m) {
                    import_names.push(m);
                }
            }
        }
    }
    let mut imports: Vec<Arc<Module>> = Vec::new();
    for n in &import_names {
        match super::load_global64(n) {
            Ok(m) => imports.push(m),
            Err(e) => {
                crate::trace!("loader: {path}: 64-bit module {n} not found (error {e})");
                super::set_fail(n);
                return Err(e);
            }
        }
    }
    let resolve = |s: &Sym| -> Result<u64, u32> {
        if s.shndx != 0 {
            return Ok(s.value.wrapping_add(delta));
        }
        let Some((m, what)) = s.name.split_once('.') else {
            // An undefined weak symbol is 0.
            if s.bind == 2 {
                return Ok(0);
            }
            crate::println!("{path}: unresolved symbol {}", s.name);
            return Err(err::PROC_NOT_FOUND);
        };
        let mname = super::module_name(m);
        let module = imports.iter().find(|x| x.name.eq_ignore_ascii_case(&mname)).ok_or(err::MOD_NOT_FOUND)?;
        let t = match what.parse::<u32>() {
            Ok(o) => module.resolve_ordinal(o, 0),
            Err(_) => module.resolve_name(what, 0),
        };
        t.map(|t| t.flat as u64).map_err(|e| {
            crate::trace!("loader: {path}: import {} not found", s.name);
            super::set_fail(&mname);
            e
        })
    };

    // Relocations (RELA and the PLT's, bound now).
    let mut relocs = Vec::new();
    if let (Some(&at), Some(&sz)) = (tags.get(&DT_RELA), tags.get(&DT_RELASZ)) {
        relocs.push((at, sz));
    }
    if let (Some(&at), Some(&sz)) = (tags.get(&DT_JMPREL), tags.get(&DT_PLTRELSZ)) {
        relocs.push((at, sz));
    }
    for (at, sz) in relocs {
        for i in 0..sz / 24 {
            let Some(r) = img.slice(at + i * 24, 24) else { return Err(err::BAD_EXE_FORMAT) };
            let (off, info, addend) = (u64le(r, 0), u64le(r, 8), u64le(r, 16));
            let (ty, sym) = (info as u32, (info >> 32) as usize);
            let v = match ty {
                R_X86_64_NONE => continue,
                R_X86_64_RELATIVE => delta.wrapping_add(addend),
                R_X86_64_64 | R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                    let s = syms.get(sym).ok_or(err::BAD_EXE_FORMAT)?;
                    let v = resolve(s)?;
                    if ty == R_X86_64_64 { v.wrapping_add(addend) } else { v }
                }
                _ => {
                    crate::println!("{path}: unsupported relocation type {ty}");
                    return Err(err::BAD_EXE_FORMAT);
                }
            };
            if !img.put_u64(off, v) {
                return Err(err::BAD_EXE_FORMAT);
            }
        }
    }

    // Place the segments.
    let mut objects = Vec::with_capacity(segs.len());
    for s in &segs {
        let mut prot = pag::READ;
        if s.flags & PF_W != 0 {
            prot |= pag::WRITE;
        }
        if s.flags & PF_X != 0 {
            prot |= pag::EXECUTE;
        }
        let obase = (s.vaddr + delta) as u32;
        let size = (s.memsz as u32).next_multiple_of(PAGE);
        let at = (s.vaddr - lo) as usize;
        let mut bytes = vec![0u8; size as usize];
        let n = (img.bytes.len() - at).min(size as usize);
        bytes[..n].copy_from_slice(&img.bytes[at..at + n]);
        let mut obj = Object {
            base: obase,
            size,
            prot,
            seg: SegKind::Data,
            is16: false,
            code: s.flags & PF_X != 0,
            limit: None,
            shared: None,
            pristine: None,
        };
        if dll {
            if s.flags & PF_W == 0 {
                let sm = SharedMem::new(obase, size, prot, None);
                for p in 0..(size / PAGE) as usize {
                    let fr = sm.frame(p).ok_or(err::NOT_ENOUGH_MEMORY)?;
                    let src = &bytes[p * PAGE as usize..(p + 1) * PAGE as usize];
                    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), crate::mm::phys::phys_to_virt(fr), PAGE as usize) };
                    sm.pages.lock()[p] = PageState::Committed;
                }
                obj.shared = Some(sm);
            } else {
                obj.pristine = Some(bytes);
            }
        } else {
            let sp = space.as_deref_mut().unwrap();
            sp.alloc(size, prot | pag::COMMIT, SegKind::Data, Some(obase)).map_err(|_| err::NOT_ENOUGH_MEMORY)?;
            if !sp.poke_bytes(obase, &bytes) {
                return Err(err::NOT_ENOUGH_MEMORY);
            }
        }
        objects.push(obj);
    }

    let start = (!dll).then(|| Start {
        entry: Target::flat32(entry.wrapping_add(delta) as u32, true),
        ..Default::default()
    });
    Ok(Arc::new(Module {
        hmte: super::alloc_hmte(),
        name: own,
        path: path.to_string(),
        specific,
        format: Format::Elf64,
        is_dll: dll,
        is16: false,
        app_type,
        objects,
        exports,
        names,
        imports,
        cyclic: crate::sync::SpinLock::new(Vec::new()),
        start,
        init: init_term.filter(|_| dll).map(|a| Start { entry: Target::flat32(a, true), ..Default::default() }),
        per_process_init: true,
        per_process_term: init_term.is_some(),
        global_init_done: core::sync::atomic::AtomicBool::new(true),
        image: Some(data.clone()),
    }))
}
