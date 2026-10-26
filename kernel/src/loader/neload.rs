//! Loading 16-bit segmented executables (NE).

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use os2_exe::ne::{self, flags, sflags, target, Entry, SourceKind};

use super::{err, import_target, load_imports, Export, Format, ImportBy, Module, Object, Start, Target};
use crate::vm::{self, pag, AddressSpace, PageState, SegKind, SharedMem, PAGE};

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub fn load(
    path: &str,
    data: Arc<[u8]>,
    header: usize,
    dll: bool,
    mut space: Option<&mut AddressSpace>,
) -> Result<Arc<Module>, u32> {
    let m = ne::Module::parse(&data, header).map_err(|_| err::BAD_EXE_FORMAT)?;
    let h = &m.header;
    if h.flags & flags::NOT_LOADABLE != 0 {
        return Err(err::EXE_MARKED_INVALID);
    }
    if (h.flags & flags::LIBRARY != 0) != dll {
        return Err(err::INVALID_MODULETYPE);
    }
    // Windows modules (docs/WIN16.md) import KERNEL, USER, GDI... from
    // rs2's Win16 layer. Self-loading programs bring their own segment
    // loader, which needs Windows' loader interface: not supported.
    let windows = h.target_os == target::WINDOWS;
    if windows && h.flags & flags::SELF_LOADING != 0 {
        crate::trace!("loader: {path}: self-loading Windows programs are not supported");
        return Err(err::BAD_EXE_FORMAT);
    }
    let segs = m.segments().map_err(|_| err::BAD_EXE_FORMAT)?;
    let import_names: Vec<String> =
        m.import_modules().map_err(|_| err::BAD_EXE_FORMAT)?.iter().map(|n| s(n)).collect();
    let (load_names, ord_bases): (Vec<String>, Vec<u32>) = if windows {
        // Windows finds a program's DLLs in the program's directory first.
        let dir = &path[..path.rfind('\\').map_or(0, |i| i + 1)];
        import_names
            .iter()
            .map(|n| match super::win16_import(n) {
                (m, 0) if !dir.is_empty() && super::find_loaded(&m).is_none() => {
                    let full = alloc::format!("{dir}{m}.DLL");
                    (if crate::fs::read_file(&full).is_some() { full } else { m }, 0)
                }
                r => r,
            })
            .unzip()
    } else {
        (import_names.clone(), alloc::vec![0; import_names.len()])
    };
    let imports = load_imports(&load_names)?;
    let autodata = (h.autodata != 0).then(|| h.autodata as usize - 1);

    // Segment sizes: the automatic data segment also holds the local heap
    // and (for programs whose SS is DGROUP) the stack.
    let mut sizes = Vec::with_capacity(segs.len());
    // A Windows program's is 64 KB: its stack follows the static data,
    // and the local heap gets the rest (it grows in place, as LocalAlloc
    // grows DGROUP on Windows).
    for (i, sg) in segs.iter().enumerate() {
        let mut size = sg.min_alloc.max(sg.file_size);
        if Some(i) == autodata {
            size += h.heap as u32;
            if !dll && h.ss as usize == i + 1 {
                size += h.stack as u32;
            }
            size = if windows { 0x10000 } else { size.min(0x10000) };
        }
        sizes.push(size.max(1));
    }

    // Addresses: one 64 KB tile per segment.
    let mut bases = Vec::with_capacity(segs.len());
    for &size in &sizes {
        let base = if dll {
            vm::reserve_shared(size, None).ok_or(err::NOT_ENOUGH_MEMORY)?
        } else {
            let sp = space.as_deref_mut().unwrap();
            // Reserve as we go so later segments get distinct tiles.
            let b = sp.find_private(size).ok_or(err::NOT_ENOUGH_MEMORY)?;
            sp.alloc(size, pag::READ | pag::WRITE | pag::COMMIT, SegKind::Data, Some(b))
                .map_err(|_| err::NOT_ENOUGH_MEMORY)?;
            b
        };
        bases.push(base);
    }
    let sel_of = |seg: usize| vm::tile_selector(bases[seg]);

    // Segment images.
    let mut images: Vec<Vec<u8>> = Vec::with_capacity(segs.len());
    for (i, sg) in segs.iter().enumerate() {
        let mut img = vec![0u8; (sizes[i] as usize).next_multiple_of(PAGE as usize)];
        if sg.file_size > 0 {
            let raw = m.segment_data(sg).map_err(|_| err::BAD_EXE_FORMAT)?;
            if sg.flags & sflags::ITERATED != 0 {
                os2_exe::lx::expand_iterated(raw, &mut img).map_err(|_| err::BAD_EXE_FORMAT)?;
            } else {
                let n = raw.len().min(img.len());
                img[..n].copy_from_slice(&raw[..n]);
            }
        }
        images.push(img);
    }

    // A Windows DLL's exported procedures load DS from AX (`push ds; pop
    // ax; nop` or `mov ax, ds; nop`, then `mov ds, ax`): Windows' loader
    // makes the first instruction `mov ax, DGROUP`, so that they get the
    // DLL's data whoever calls them.
    if windows && dll {
        if let Some(a) = autodata {
            let [lo, hi] = sel_of(a).to_le_bytes();
            for e in m.entries().map_err(|_| err::BAD_EXE_FORMAT)? {
                let (Entry::Fixed { segment, offset, flags: fl } | Entry::Movable { segment, offset, flags: fl }) = e else { continue };
                let i = segment as usize;
                if fl & 1 == 0 || i == 0 || i > segs.len() || segs[i - 1].flags & sflags::DATA != 0 {
                    continue;
                }
                let img = &mut images[i - 1];
                let o = offset as usize;
                if o + 3 <= img.len() && matches!(img[o..o + 3], [0x1e, 0x58, 0x90] | [0x8c, 0xd8, 0x90]) {
                    img[o..o + 3].copy_from_slice(&[0xb8, lo, hi]);
                }
            }
        }
    }

    // Exports.
    let seg_target = |seg: u8, off: u16| -> Option<Target> {
        let i = (seg as usize).checked_sub(1)?;
        (i < segs.len()).then(|| Target::far16(sel_of(i), off, bases[i] + off as u32))
    };
    let mut exports = Vec::new();
    for e in m.entries().map_err(|_| err::BAD_EXE_FORMAT)? {
        exports.push(match e {
            Entry::Unused => Export::None,
            Entry::Fixed { segment, offset, .. } | Entry::Movable { segment, offset, .. } => {
                seg_target(segment, offset).map_or(Export::None, Export::At)
            }
            Entry::Constant { value, .. } => Export::At(Target::constant(value)),
        });
    }
    let mut names = BTreeMap::new();
    for (n, ord) in m.exported_names().map_err(|_| err::BAD_EXE_FORMAT)? {
        names.insert(s(n), ord);
    }

    // Relocations.
    for (i, sg) in segs.iter().enumerate() {
        for r in m.relocs(sg).map_err(|_| err::BAD_EXE_FORMAT)? {
            let t = match r.target {
                ne::Target::Internal { segment, offset } => {
                    seg_target(segment, offset).ok_or(err::INVALID_SEGMENT_NUMBER)?
                }
                ne::Target::InternalEntry { ordinal } => match exports.get(ordinal as usize - 1) {
                    Some(Export::At(t)) => *t,
                    _ => return Err(err::INVALID_ORDINAL),
                },
                ne::Target::ImportOrdinal { module, ordinal } => {
                    let base = ord_bases.get(module as usize - 1).copied().unwrap_or(0);
                    let constant = (base != 0)
                        .then(|| super::WIN16_CONSTANTS.iter().find(|c| c.0 == base + ordinal as u32))
                        .flatten();
                    if let Some(&(_, v)) = constant {
                        Target::constant(v)
                    } else {
                        import_target(&imports, module, ImportBy::Ordinal(base + ordinal as u32)).map_err(|e| {
                            crate::trace!(
                                "loader: {}: import {}.{} not found",
                                path,
                                import_names.get(module as usize - 1).map(|s| s.as_str()).unwrap_or("?"),
                                ordinal
                            );
                            super::set_fail(import_names.get(module as usize - 1).map(|s| s.as_str()).unwrap_or("?"));
                            e
                        })?
                    }
                }
                ne::Target::ImportName { module, name_offset } => {
                    let name = m.imported_name(name_offset).map(s).map_err(|_| err::BAD_EXE_FORMAT)?;
                    import_target(&imports, module, ImportBy::Name(&name)).map_err(|e| {
                        crate::trace!("loader: {}: import {} not found", path, name);
                        super::set_fail(import_names.get(module as usize - 1).map(|s| s.as_str()).unwrap_or("?"));
                        e
                    })?
                }
                ne::Target::OsFixup { .. } => continue,
            };
            let img = &mut images[i];
            if r.additive {
                apply(img, r.offset as usize, r.kind, &t, true)?;
            } else {
                // The source locations form a chain through the segment.
                let mut off = r.offset as usize;
                let mut guard = 0;
                loop {
                    if off + 2 > img.len() || guard > 0x8000 {
                        return Err(err::RELOC_CHAIN_XEEDS_SEGLIM);
                    }
                    let next = u16::from_le_bytes([img[off], img[off + 1]]);
                    apply(img, off, r.kind, &t, false)?;
                    // Byte fixups do not chain.
                    if next == 0xffff || r.kind == SourceKind::LoByte {
                        break;
                    }
                    off = next as usize;
                    guard += 1;
                }
            }
        }
    }

    // Place segments.
    let mut objects = Vec::with_capacity(segs.len());
    for (i, sg) in segs.iter().enumerate() {
        let data_seg = sg.flags & sflags::DATA != 0;
        let mut prot = pag::READ;
        if data_seg {
            if sg.flags & sflags::READONLY == 0 {
                prot |= pag::WRITE;
            }
        } else {
            prot |= pag::EXECUTE;
        }
        let seg = if data_seg { SegKind::Data } else { SegKind::Code16 };
        let base = bases[i];
        let size = sizes[i].next_multiple_of(PAGE);
        let limit = Some(sizes[i] - 1);
        let img = core::mem::take(&mut images[i]);
        let mut obj =
            Object { base, size, prot, seg, is16: true, code: !data_seg, limit, shared: None, pristine: None };
        if dll {
            // A Windows DLL's data is the process's own: each Windows task
            // is a process, and the DLL is initialised in each.
            let shared = !data_seg || (!windows && sg.flags & sflags::SHARED != 0) || prot & pag::WRITE == 0;
            if shared {
                let sm = SharedMem::new(base, size, prot, None);
                for p in 0..(size / PAGE) as usize {
                    let fr = sm.frame(p).ok_or(err::NOT_ENOUGH_MEMORY)?;
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            img[p * PAGE as usize..].as_ptr(),
                            crate::mm::phys::phys_to_virt(fr),
                            PAGE as usize,
                        )
                    };
                    sm.pages.lock()[p] = PageState::Committed;
                }
                obj.shared = Some(sm);
            } else {
                obj.pristine = Some(img);
            }
        } else {
            let sp = space.as_deref_mut().unwrap();
            // Replace the placeholder reservation with the real protection.
            sp.free(base).map_err(|_| err::NOT_ENOUGH_MEMORY)?;
            sp.alloc(size, prot | pag::COMMIT, seg, Some(base)).map_err(|_| err::NOT_ENOUGH_MEMORY)?;
            sp.set_seg_limit(base, sizes[i] - 1);
            if !sp.poke_bytes(base, &img) {
                return Err(err::NOT_ENOUGH_MEMORY);
            }
        }
        objects.push(obj);
    }

    let start = seg_target(h.cs as u8, h.ip).map(|entry| {
        let ss = h.ss as usize;
        let sp = if h.sp == 0 && ss >= 1 && ss <= sizes.len() {
            if windows {
                // Windows: the stack ends after the data and the stack size.
                let sg = &segs[ss - 1];
                ((sg.min_alloc.max(sg.file_size) + h.stack as u32).min(0xfffe) & !1) as u16
            } else {
                sizes[ss - 1] as u16
            }
        } else {
            h.sp
        };
        let stack = if ss >= 1 && ss <= segs.len() {
            Target::far16(sel_of(ss - 1), sp, bases[ss - 1] + sp as u32)
        } else {
            Target::default()
        };
        Start {
            entry,
            stack,
            autodata,
            autodata_size: autodata.map(|a| sizes[a] & 0xffff).unwrap_or(0),
            windows: windows.then_some((h.heap, h.stack)),
        }
    });
    let name = m.name().map(s).unwrap_or_else(|_| super::module_name(path));
    Ok(Arc::new(Module {
        hmte: super::alloc_hmte(),
        name: name.to_ascii_uppercase(),
        path: path.to_string(),
        format: Format::Ne,
        is_dll: dll,
        is16: true,
        app_type: super::app_type_from_flags((h.flags as u32 & 0x0700) >> 8),
        objects,
        exports,
        names,
        imports,
        start: if dll { None } else { start },
        init: if dll { start } else { None },
        per_process_init: windows || h.flags & flags::PER_PROCESS_INIT != 0,
        per_process_term: false,
        global_init_done: core::sync::atomic::AtomicBool::new(false),
        image: Some(data.clone()),
    }))
}

fn apply(img: &mut [u8], at: usize, kind: SourceKind, t: &Target, additive: bool) -> Result<(), u32> {
    let rd16 = |img: &[u8], o: usize| u16::from_le_bytes([img[o], img[o + 1]]);
    let need = match kind {
        SourceKind::LoByte => 1,
        SourceKind::Selector | SourceKind::Offset16 => 2,
        SourceKind::Pointer1616 | SourceKind::Offset32 => 4,
        SourceKind::Pointer1632 => 6,
    };
    if at + need > img.len() {
        return Err(err::RELOC_CHAIN_XEEDS_SEGLIM);
    }
    let base_off = if additive && need >= 2 { rd16(img, at) } else { 0 };
    let off = t.off.wrapping_add(base_off);
    match kind {
        SourceKind::LoByte => img[at] = img[at].wrapping_add(t.off as u8),
        SourceKind::Selector => img[at..at + 2].copy_from_slice(&t.sel.to_le_bytes()),
        SourceKind::Offset16 => img[at..at + 2].copy_from_slice(&off.to_le_bytes()),
        SourceKind::Pointer1616 => {
            img[at..at + 2].copy_from_slice(&off.to_le_bytes());
            img[at + 2..at + 4].copy_from_slice(&t.sel.to_le_bytes());
        }
        SourceKind::Offset32 => {
            let v = if t.is16 { off as u32 } else { t.flat.wrapping_add(base_off as u32) };
            img[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        SourceKind::Pointer1632 => {
            let (o, sel) = if t.is16 { (off as u32, t.sel) } else { (t.flat, t.flat_sel) };
            img[at..at + 4].copy_from_slice(&o.to_le_bytes());
            img[at + 4..at + 6].copy_from_slice(&sel.to_le_bytes());
        }
    }
    Ok(())
}
