//! Loading 32-bit linear executables (LX).

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use os2_exe::lx::{self, mflags, oflags, Entry, SourceKind};

use super::{err, import_target, load_imports, patch, Export, Format, ImportBy, Module, Object, Start, Target};
use crate::vm::{self, pag, AddressSpace, PageState, SegKind, SharedMem, PAGE, TILE};

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
    let m = lx::Module::parse(&data, header).map_err(|_| err::BAD_EXE_FORMAT)?;
    let h = &m.header;
    if h.module_flags & mflags::NOT_LOADABLE != 0 {
        return Err(err::EXE_MARKED_INVALID);
    }
    if h.is_library() != dll {
        return Err(err::INVALID_MODULETYPE);
    }
    let lxobjs = m.objects().map_err(|_| err::BAD_EXE_FORMAT)?;
    let import_names: Vec<String> =
        m.import_modules().map_err(|_| err::BAD_EXE_FORMAT)?.iter().map(|n| s(n)).collect();
    let mut imports = load_imports(&import_names)?;

    // Choose an address for each object.
    let fixups_stripped = h.module_flags & mflags::INTERNAL_FIXUPS_REMOVED != 0;
    let mut bases = Vec::with_capacity(lxobjs.len());
    for o in &lxobjs {
        let size = o.virtual_size.max(1);
        let base = if dll {
            match vm::reserve_shared(size, Some(o.base)) {
                Some(b) => b,
                None if !fixups_stripped => vm::reserve_shared(size, None).ok_or(err::NOT_ENOUGH_MEMORY)?,
                None => return Err(err::NOT_ENOUGH_MEMORY),
            }
        } else {
            let sp = space.as_deref_mut().unwrap();
            let span = size.next_multiple_of(TILE);
            let fits = |b: u32| {
                sp.is_free(b, size)
                    && bases.iter().all(|&(ob, os): &(u32, u32)| b + span <= ob || b >= ob + os.next_multiple_of(TILE))
            };
            if o.base % TILE == 0 && fits(o.base) {
                o.base
            } else if fixups_stripped {
                return Err(err::NOT_ENOUGH_MEMORY);
            } else {
                let mut b = vm::ARENA_START;
                while !fits(b) {
                    b += TILE;
                    if b >= vm::TILED_TOP {
                        return Err(err::NOT_ENOUGH_MEMORY);
                    }
                }
                b
            }
        };
        bases.push((base, size));
    }

    // Decode pages into per-object images.
    let ps = h.page_size as usize;
    let mut images: Vec<Vec<u8>> = Vec::with_capacity(lxobjs.len());
    for o in &lxobjs {
        let len = (o.virtual_size as usize).max(o.page_count as usize * ps).next_multiple_of(PAGE as usize);
        let mut img = vec![0u8; len.max(PAGE as usize)];
        for i in 0..o.page_count {
            let at = i as usize * ps;
            m.load_page(o.page_index + i, &mut img[at..at + ps]).map_err(|_| err::BAD_EXE_FORMAT)?;
        }
        images.push(img);
    }

    let is16 = |o: &lx::Object| o.flags & oflags::BIG == 0;
    let target_in = |obj: usize, off: u32| -> Target {
        let o = &lxobjs[obj];
        let base = bases[obj].0;
        if is16(o) {
            Target::far16(vm::tile_selector(base), off as u16, base + off)
        } else {
            Target::flat32(base + off, o.flags & oflags::EXEC != 0)
        }
    };

    // Exports.
    let entries = m.entries().map_err(|_| err::BAD_EXE_FORMAT)?;
    let mut exports = Vec::with_capacity(entries.len());
    for e in &entries {
        exports.push(match *e {
            Entry::Unused => Export::None,
            Entry::Entry16 { object, offset, .. } | Entry::CallGate { object, offset, .. } => {
                Export::At(target_in(object as usize - 1, offset as u32))
            }
            Entry::Entry32 { object, offset, .. } => Export::At(target_in(object as usize - 1, offset)),
            Entry::Forwarder { module, by_ordinal, value } => {
                let mname = import_names.get(module as usize - 1).cloned().unwrap_or_default();
                if by_ordinal {
                    Export::ForwardOrdinal { module: mname, ordinal: value }
                } else {
                    let name = m.import_proc_name(value).map(s).unwrap_or_default();
                    Export::ForwardName { module: mname, name }
                }
            }
        });
    }
    // Forwarder targets are dependencies.
    for e in &exports {
        if let Export::ForwardOrdinal { module, .. } | Export::ForwardName { module, .. } = e {
            if !imports.iter().any(|i| i.name.eq_ignore_ascii_case(&super::module_name(module))) {
                if let Ok(dep) = super::load_global(module) {
                    imports.push(dep);
                }
            }
        }
    }
    let mut names = BTreeMap::new();
    for (n, ord) in m.exported_names().map_err(|_| err::BAD_EXE_FORMAT)? {
        names.insert(s(n), ord);
    }

    // Fixups.
    for (oi, o) in lxobjs.iter().enumerate() {
        for pi in 0..o.page_count {
            let page = o.page_index + pi;
            let fixups = m.fixups(page).map_err(|_| err::BAD_EXE_FORMAT)?;
            for f in fixups {
                let t = match f.target {
                    lx::Target::Internal { object, offset } => {
                        let oi2 = object as usize - 1;
                        if oi2 >= lxobjs.len() {
                            return Err(err::BAD_EXE_FORMAT);
                        }
                        target_in(oi2, offset.wrapping_add(f.additive))
                    }
                    lx::Target::InternalEntry { ordinal } => {
                        match exports.get(ordinal as usize - 1) {
                            Some(Export::At(t)) => add(*t, f.additive),
                            _ => return Err(err::INVALID_ORDINAL),
                        }
                    }
                    lx::Target::ImportOrdinal { module, ordinal } => {
                        let t = import_target(&imports, module, ImportBy::Ordinal(ordinal)).map_err(|e| {
                            crate::println!(
                                "loader: {}: import {}.{} not found",
                                path,
                                import_names.get(module as usize - 1).map(|s| s.as_str()).unwrap_or("?"),
                                ordinal
                            );
                            e
                        })?;
                        add(t, f.additive)
                    }
                    lx::Target::ImportName { module, name_offset } => {
                        let name = m.import_proc_name(name_offset).map(s).map_err(|_| err::BAD_EXE_FORMAT)?;
                        let t = import_target(&imports, module, ImportBy::Name(&name)).map_err(|e| {
                            crate::println!("loader: {}: import {} not found", path, name);
                            e
                        })?;
                        add(t, f.additive)
                    }
                };
                let img = &mut images[oi];
                let page_off = pi as isize * ps as isize;
                for &src in &f.sources {
                    let at = page_off + src as isize;
                    let src_flat = bases[oi].0 as i64 + at as i64;
                    apply(img, at, f.kind, &t, src_flat as u32, f.alias);
                }
            }
        }
    }

    // Place the objects.
    let mut objects = Vec::with_capacity(lxobjs.len());
    for (i, o) in lxobjs.iter().enumerate() {
        let mut prot = 0;
        if o.flags & oflags::READ != 0 {
            prot |= pag::READ;
        }
        if o.flags & oflags::WRITE != 0 {
            prot |= pag::WRITE;
        }
        if o.flags & oflags::EXEC != 0 {
            prot |= pag::EXECUTE | pag::READ;
        }
        let code = o.flags & oflags::EXEC != 0;
        let seg = match (is16(o), code) {
            (true, true) => SegKind::Code16,
            _ => SegKind::Data,
        };
        let (base, size) = bases[i];
        let size = size.next_multiple_of(PAGE);
        let limit = if is16(o) && o.virtual_size <= 0x10000 { Some(o.virtual_size.max(1) - 1) } else { None };
        let mut img = core::mem::take(&mut images[i]);
        img.truncate(size as usize);
        let mut obj = Object {
            base,
            size,
            prot,
            seg,
            is16: is16(o),
            code,
            limit,
            shared: None,
            pristine: None,
        };
        if dll {
            let shared = o.flags & oflags::SHARED != 0 || o.flags & oflags::WRITE == 0;
            if shared {
                let sm = SharedMem::new(base, size, prot, None);
                for p in 0..(size / PAGE) as usize {
                    let fr = sm.frame(p).ok_or(err::NOT_ENOUGH_MEMORY)?;
                    let src = &img[p * PAGE as usize..(p + 1) * PAGE as usize];
                    unsafe {
                        core::ptr::copy_nonoverlapping(src.as_ptr(), crate::mm::phys::phys_to_virt(fr), PAGE as usize)
                    };
                    sm.pages.lock()[p] = PageState::Committed;
                }
                obj.shared = Some(sm);
            } else {
                obj.pristine = Some(img);
            }
        } else {
            let sp = space.as_deref_mut().unwrap();
            sp.alloc(size, prot | pag::COMMIT, seg, Some(base)).map_err(|_| err::NOT_ENOUGH_MEMORY)?;
            if let Some(l) = limit {
                sp.set_seg_limit(base, l);
            }
            if !sp.poke_bytes(base, &img) {
                return Err(err::NOT_ENOUGH_MEMORY);
            }
        }
        objects.push(obj);
    }

    let target_of = |obj: u32, off: u32| -> Option<Target> {
        (obj != 0 && (obj as usize) <= lxobjs.len()).then(|| target_in(obj as usize - 1, off))
    };
    let autodata = (h.autodata_object != 0).then(|| h.autodata_object as usize - 1);
    let start_info = target_of(h.eip_object, h.eip).map(|entry| Start {
        entry,
        stack: target_of(h.esp_object, h.esp).unwrap_or_default(),
        autodata,
        autodata_size: autodata.map(|a| lxobjs[a].virtual_size).unwrap_or(0),
    });
    let name = m.name().map(s).unwrap_or_else(|_| super::module_name(path));
    let all16 = lxobjs.iter().all(|o| is16(o));
    Ok(Arc::new(Module {
        hmte: super::alloc_hmte(),
        name: name.to_ascii_uppercase(),
        path: path.to_string(),
        format: Format::Lx,
        is_dll: dll,
        is16: all16,
        app_type: super::app_type_from_flags((h.module_flags & mflags::APP_TYPE_MASK) >> 8),
        objects,
        exports,
        names,
        imports,
        start: if dll { None } else { start_info },
        init: if dll { start_info } else { None },
        per_process_init: h.module_flags & mflags::PER_PROCESS_INIT != 0,
        global_init_done: core::sync::atomic::AtomicBool::new(false),
        image: Some(data.clone()),
    }))
}

fn add(t: Target, additive: u32) -> Target {
    if additive == 0 {
        return t;
    }
    Target { flat: t.flat.wrapping_add(additive), off: t.off.wrapping_add(additive as u16), ..t }
}

fn apply(img: &mut [u8], at: isize, kind: SourceKind, t: &Target, src_flat: u32, alias: bool) {
    match kind {
        SourceKind::Byte => patch(img, at, &[t.off as u8]),
        // A selector fixup to a 32-bit object means its flat selector
        // (thunks load FLAT DS this way); with the alias flag, its tile.
        SourceKind::Selector16 if !t.is16 && !alias && t.flat_sel != 0 => patch(img, at, &t.flat_sel.to_le_bytes()),
        SourceKind::Selector16 => patch(img, at, &t.sel.to_le_bytes()),
        SourceKind::Pointer1616 => {
            patch(img, at, &t.off.to_le_bytes());
            patch(img, at + 2, &t.sel.to_le_bytes());
        }
        SourceKind::Offset16 => patch(img, at, &t.off.to_le_bytes()),
        SourceKind::Pointer1632 => {
            let (off, sel) = if t.is16 { (t.off as u32, t.sel) } else { (t.flat, t.flat_sel) };
            patch(img, at, &off.to_le_bytes());
            patch(img, at + 4, &sel.to_le_bytes());
        }
        SourceKind::Offset32 => patch(img, at, &t.flat.to_le_bytes()),
        SourceKind::Relative32 => {
            let rel = t.flat.wrapping_sub(src_flat.wrapping_add(4));
            patch(img, at, &rel.to_le_bytes());
        }
    }
}
