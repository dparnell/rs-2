//! Loading 32-bit Windows modules (PE32) for rs2's Win32 layer
//! (docs/WIN32.md).
//!
//! A PE module is mapped as one object: its headers and sections at their
//! RVAs from the base, read, write and execute. A program goes into its
//! process's private arena at its preferred base (most are linked at
//! 0x400000, many without relocations); a DLL goes into the shared arena
//! like an LX DLL, but all of it is per-process instance data (each
//! process gets its own copy of the relocated, bound image), as sections
//! need not start on the 64 KB boundaries the arena's objects have.
//!
//! Imports are bound here: the system DLLs' names map to rs2's Win32 DLLs
//! ([`super::win32_import`]: KERNEL32 is W32KERN, an LX DLL exporting
//! Windows' names), any other DLL is looked for in the program's
//! directory, then along the LIBPATH. A program also gets W32NTDLL, whose
//! start routine runs the DLLs' entry points and then the program's
//! (task/process.rs). DLL entry points (DllMain) and TLS are W32NTDLL's
//! business: the kernel runs no initialisation for a PE module.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use os2_exe::pe;

use super::{err, Export, Format, ImportBy, Imports, Module, Object, Pending, Start, Target};
use crate::vm::{self, pag, AddressSpace, SegKind, PAGE, TILE};

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The module W32NTDLL, which every Win32 program gets.
pub const NTDLL: &str = "W32NTDLL";

/// Where an import of the PE module at `path` of DLL `dll` is loaded
/// from: an rs2 Win32 DLL, the program's directory, or by name (LIBPATH).
fn load_name(path: &str, dll: &str) -> String {
    let mapped = super::win32_import(dll);
    if !mapped.eq_ignore_ascii_case(&super::module_name(dll)) {
        return mapped;
    }
    let dir = &path[..path.rfind('\\').map_or(0, |i| i + 1)];
    let file = if dll.contains('.') { dll.to_string() } else { alloc::format!("{dll}.DLL") };
    if !dir.is_empty() && super::find_loaded(&mapped).is_none() {
        let f = alloc::format!("{dir}{file}");
        if crate::fs::read_file(&f).is_some() {
            return f;
        }
    }
    mapped
}

pub fn load(
    path: &str,
    data: Arc<[u8]>,
    header: usize,
    dll: bool,
    specific: bool,
    mut space: Option<&mut AddressSpace>,
) -> Result<Arc<Module>, u32> {
    // A file with a PE signature that is not a PE32 image is not an
    // executable rs2 knows (as OS/2 says of every PE file).
    let m = pe::Module::parse(&data, header).map_err(|e| {
        crate::trace!("loader: {path}: {e}");
        err::INVALID_EXE_SIGNATURE
    })?;
    let h = &m.header;
    if h.machine != pe::MACHINE_I386 {
        crate::trace!("loader: {path}: PE machine {:#x} is not i386", h.machine);
        return Err(err::BAD_EXE_FORMAT);
    }
    if h.is_dll() != dll {
        return Err(err::INVALID_MODULETYPE);
    }
    let mut img = m.image().map_err(|_| err::BAD_EXE_FORMAT)?;
    let size = img.len() as u32;
    let relocs = m.relocations().map_err(|_| err::BAD_EXE_FORMAT)?;
    let movable = !h.relocs_stripped() && !relocs.is_empty();

    // The base.
    let preferred = h.image_base;
    let base = if dll {
        let at = (preferred % TILE == 0).then(|| vm::reserve_shared(size, Some(preferred))).flatten();
        match at {
            Some(b) => b,
            None if movable => vm::reserve_shared(size, None).ok_or(err::NOT_ENOUGH_MEMORY)?,
            None => {
                crate::trace!("loader: {path}: its base {preferred:#x} is taken and it has no relocations");
                return Err(err::NOT_ENOUGH_MEMORY);
            }
        }
    } else {
        let sp = space.as_deref_mut().unwrap();
        let below_shared = |b: u32| b.checked_add(size).is_some_and(|e| e <= vm::SHARED_FLOOR);
        if preferred % TILE == 0 && preferred >= vm::ARENA_START && below_shared(preferred) && sp.is_free(preferred, size) {
            preferred
        } else if movable {
            let mut b = vm::ARENA_START;
            while !sp.is_free(b, size) {
                b += TILE;
                if !below_shared(b) {
                    return Err(err::NOT_ENOUGH_MEMORY);
                }
            }
            b
        } else {
            crate::trace!("loader: {path}: its base {preferred:#x} is not free and it has no relocations");
            return Err(err::NOT_ENOUGH_MEMORY);
        }
    };
    if base != preferred {
        pe::relocate(&mut img, &relocs, base.wrapping_sub(preferred));
        // The image's own header says where it is (GetModuleHandle(NULL)
        // and the PE readers in C runtimes look).
        let at = header + 4 + 20 + 28;
        if let Some(b) = img.get_mut(at..at + 4) {
            b.copy_from_slice(&base.to_le_bytes());
        }
    }
    if crate::api::tracing() {
        crate::trace!("    {path}: PE image at {base:#x} ({size:#x} bytes, preferred {preferred:#x})");
    }

    let own = super::module_name(path);

    // Exports: Windows ordinal n is entry n - base of the table; the
    // module's export list is indexed by ordinal - 1.
    let ex = m.exports().map_err(|_| err::BAD_EXE_FORMAT)?;
    let mut exports = Vec::new();
    let mut names = BTreeMap::new();
    if !ex.entries.is_empty() {
        exports.resize((ex.base as usize + ex.entries.len()).saturating_sub(1), Export::None);
        for (i, e) in ex.entries.iter().enumerate() {
            let ord = ex.base as usize + i;
            if ord == 0 {
                continue;
            }
            exports[ord - 1] = match e {
                None => Export::None,
                Some(pe::Export::Rva(rva)) => Export::At(Target::flat32(base.wrapping_add(*rva), true)),
                Some(pe::Export::Forward(f)) => forwarder(f),
            };
        }
        for (n, i) in &ex.names {
            let ord = ex.base + *i as u32;
            if ord != 0 && ord <= u16::MAX as u32 {
                names.insert(s(n), ord as u16);
            }
        }
    }

    // Imports.
    let imports_pe = m.imports().map_err(|e| {
        crate::trace!("loader: {path}: import table: {e}");
        err::BAD_EXE_FORMAT
    })?;
    let mut load_names: Vec<String> = imports_pe.iter().map(|i| load_name(path, &s(i.dll))).collect();
    if !dll && !load_names.iter().any(|n| n.eq_ignore_ascii_case(NTDLL)) {
        load_names.push(String::from(NTDLL));
    }
    let _pending = dll.then(|| Pending::add(&own, path, specific, &exports, &names));
    let mut imports = Imports::load(&own, &load_names).inspect_err(|_| {
        crate::trace!("loader: {path}: an imported module did not load");
    })?;
    for e in &exports {
        if let Export::ForwardOrdinal { module, .. } | Export::ForwardName { module, .. } = e {
            if !imports.has(module) && !Pending::has(module) {
                if let Ok(d) = super::load_global(module) {
                    imports.push(d);
                }
            }
        }
    }
    let mut missing = 0;
    for (mi, imp) in imports_pe.iter().enumerate() {
        for (slot, by) in &imp.entries {
            let t = match by {
                pe::ImportBy::Ordinal(o) => imports.target(mi as u16 + 1, ImportBy::Ordinal(*o as u32)),
                pe::ImportBy::Name { name, .. } => imports.target(mi as u16 + 1, ImportBy::Name(&s(name))),
            };
            match t {
                Ok(t) => {
                    if let Some(b) = img.get_mut(*slot as usize..*slot as usize + 4) {
                        b.copy_from_slice(&t.flat.to_le_bytes());
                    }
                }
                Err(_) => {
                    let what = match by {
                        pe::ImportBy::Ordinal(o) => alloc::format!("#{o}"),
                        pe::ImportBy::Name { name, .. } => s(name),
                    };
                    crate::trace!("loader: {path}: import {}.{what} not found (in {})", s(imp.dll), load_names[mi]);
                    missing += 1;
                }
            }
        }
    }
    if missing != 0 {
        super::set_fail(&load_names.first().cloned().unwrap_or_default());
        return Err(err::PROC_NOT_FOUND);
    }

    // Place the image.
    let prot = pag::READ | pag::WRITE | pag::EXECUTE;
    let mut obj = Object { base, size, prot, seg: SegKind::Data, is16: false, code: true, limit: None, shared: None, pristine: None };
    if dll {
        obj.pristine = Some(img);
    } else {
        let sp = space.as_deref_mut().unwrap();
        sp.alloc(size, prot | pag::COMMIT, SegKind::Data, Some(base)).map_err(|_| err::NOT_ENOUGH_MEMORY)?;
        if !sp.poke_bytes(base, &img) {
            return Err(err::NOT_ENOUGH_MEMORY);
        }
    }
    let _ = PAGE;

    let start = (!dll && h.entry != 0).then(|| Start {
        entry: Target::flat32(base.wrapping_add(h.entry), true),
        ..Default::default()
    });
    // A console program runs in a text window, a GUI program is a PM
    // program (FAPPTYP_WINDOWCOMPAT 2, WINDOWAPI 3).
    let app_type = if h.subsystem == pe::subsystem::WINDOWS_GUI { 3 } else { 2 };
    Ok(Arc::new(Module {
        hmte: super::alloc_hmte(),
        name: own,
        path: path.to_string(),
        specific,
        format: Format::Pe,
        is_dll: dll,
        is16: false,
        app_type,
        objects: alloc::vec![obj],
        exports,
        names,
        imports: imports.modules(),
        cyclic: crate::sync::SpinLock::new(Vec::new()),
        start,
        init: None,
        per_process_init: false,
        per_process_term: false,
        global_init_done: core::sync::atomic::AtomicBool::new(true),
        tls: Default::default(),
        image: Some(data.clone()),
    }))
}

/// A forwarder, "DLL.Name" or "DLL.#12", to a module of Win32's names.
fn forwarder(f: &[u8]) -> Export {
    let f = s(f);
    let Some((dll, what)) = f.rsplit_once('.') else { return Export::None };
    let module = super::win32_import(dll);
    match what.strip_prefix('#').map(|n| n.parse::<u32>()) {
        Some(Ok(ordinal)) => Export::ForwardOrdinal { module, ordinal },
        _ => Export::ForwardName { module, name: what.to_string() },
    }
}

/// The stack sizes (reserve, commit) a PE program asks for.
pub fn stack_sizes(data: &[u8]) -> Option<(u32, u32)> {
    let Ok(os2_exe::Format::Pe { header }) = os2_exe::detect(data) else { return None };
    let m = pe::Module::parse(data, header).ok()?;
    Some((m.header.stack_reserve, m.header.stack_commit))
}
