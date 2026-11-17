//! Binding resources into executables (LX and NE), replacing the
//! resources already bound and keeping everything else (code, fixups,
//! names, debug information after the image).
//!
//! LX: the old resource objects (the last objects of the module) and
//! their pages are dropped; the new resources go into new objects, one
//! per kind of loading (preload, discardable, pure), sorted by type and
//! ID, 4-byte aligned, after the module's own pages. NE: the resources
//! are the last segments; the segment and resource tables are rebuilt.

use alloc::vec;
use alloc::vec::Vec;

use crate::res::{Name, Resource};
use crate::flags;

#[derive(Clone, Copy, Default)]
pub struct BindOptions {
    /// `-x1`/`-x2`: compress the resource pages (EXEPACK 1 or 2).
    pub compress: u8,
    /// `-p`: no resource crosses a 64K boundary.
    pub pack: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BindError {
    /// Not an LX or NE module (R2031).
    Format,
    /// The module's tables are not laid out as expected (R2026).
    Layout(&'static str),
    /// A resource has a string name (they need numbers in modules).
    NamedResource,
}

fn r16(d: &[u8], o: usize) -> Result<u16, BindError> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or(BindError::Layout("truncated"))
}

fn r32(d: &[u8], o: usize) -> Result<u32, BindError> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(BindError::Layout("truncated"))
}

fn w16(d: &mut [u8], o: usize, v: u16) {
    d[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn w32(d: &mut [u8], o: usize, v: u32) {
    d[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// True if the module is NE (for R3011/R3012: no compression or packing).
pub fn is_ne(exe: &[u8]) -> bool {
    matches!(os2_exe::detect(exe), Ok(os2_exe::Format::Ne { .. }))
}

/// `exe` with `res` bound in place of its resources.
pub fn bind(exe: &[u8], res: &[Resource], o: BindOptions) -> Result<Vec<u8>, BindError> {
    let ne_file = is_ne(exe);
    let mut list: Vec<(u16, u16, u16, &[u8])> = Vec::new();
    // Named resources: NE modules get ID 0 (as IBM's RC); LX modules get
    // a number and an RT_RESNAMES table per type (count, then ID and name).
    let mut names: Vec<(u16, Vec<(u16, Vec<u8>)>)> = Vec::new();
    let mut next_id: u16 = 0xffff;
    for r in res {
        let id = match &r.name {
            Name::Id(id) => *id,
            Name::Str(_) if ne_file => 0,
            Name::Str(s) => {
                while list.iter().any(|x| x.1 == next_id) || res.iter().any(|x| x.name == Name::Id(next_id)) {
                    next_id -= 1;
                }
                let id = next_id;
                next_id -= 1;
                match names.iter_mut().find(|n| n.0 == r.ty) {
                    Some(n) => n.1.push((id, s.clone())),
                    None => names.push((r.ty, vec![(id, s.clone())])),
                }
                id
            }
        };
        // Later duplicates are dropped (the first is bound).
        if list.iter().any(|x| x.0 == r.ty && x.1 == id) {
            continue;
        }
        list.push((r.ty, id, r.flags, &r.data));
    }
    let mut tables: Vec<(u16, Vec<u8>)> = Vec::new();
    for (ty, n) in &names {
        let mut t = Vec::new();
        t.extend_from_slice(&(n.len() as u32).to_le_bytes());
        for (id, s) in n {
            t.extend_from_slice(&(*id as u32).to_le_bytes());
            t.extend_from_slice(s);
            t.push(0);
        }
        tables.push((*ty, t));
    }
    for (ty, t) in &tables {
        list.push((crate::rt::RESNAMES, *ty, flags::PURE, t));
    }
    match os2_exe::detect(exe) {
        Ok(os2_exe::Format::Lx { header }) => lx(exe, header, &mut list, o),
        Ok(os2_exe::Format::Ne { header }) => ne(exe, header, &list),
        _ => Err(BindError::Format),
    }
}

// ---- LX ----

const H_PAGES: usize = 0x14;
const H_PAGESIZE: usize = 0x28;
const H_SHIFT: usize = 0x2c;
const H_FIXSIZE: usize = 0x30;
const H_LDRSIZE: usize = 0x38;
const H_OBJTAB: usize = 0x40;
const H_NOBJ: usize = 0x44;
const H_PAGETAB: usize = 0x48;
const H_ITER: usize = 0x4c;
const H_RESTAB: usize = 0x50;
const H_NRES: usize = 0x54;
const H_RESNAMES: usize = 0x58;
const H_ENTRY: usize = 0x5c;
const H_DIR: usize = 0x60;
const H_FPT: usize = 0x68;
const H_FRT: usize = 0x6c;
const H_IMOD: usize = 0x70;
const H_IPROC: usize = 0x78;
const H_CHK: usize = 0x7c;
const H_DATA: usize = 0x80;
const H_NONRES: usize = 0x88;
const H_NONRESLEN: usize = 0x8c;
const H_DEBUG: usize = 0x98;
const H_DEBUGLEN: usize = 0x9c;

const OBJ_READ: u32 = 0x0001;
const OBJ_RESOURCE: u32 = 0x0008;
const OBJ_DISCARD: u32 = 0x0010;
const OBJ_SHARED: u32 = 0x0020;
const OBJ_PRELOAD: u32 = 0x0040;
const OBJ_BIG: u32 = 0x2000;

/// The LX object for resources with load/memory flags `f`, as IBM's RC
/// groups them: PRELOAD ones, the other MOVEABLE ones (both readable,
/// shared, discardable), and FIXED ones (not discardable).
fn lx_obj_flags(f: u16) -> u32 {
    let base = OBJ_READ | OBJ_RESOURCE | OBJ_SHARED | OBJ_BIG;
    if f & flags::PRELOAD != 0 {
        base | OBJ_DISCARD | OBJ_PRELOAD
    } else if f & flags::MOVEABLE != 0 {
        base | OBJ_DISCARD
    } else {
        base
    }
}

fn lx(d: &[u8], h: usize, list: &mut Vec<(u16, u16, u16, &[u8])>, o: BindOptions) -> Result<Vec<u8>, BindError> {
    let g = |off: usize| r32(d, h + off);
    let page_size = g(H_PAGESIZE)?.max(1) as usize;
    let shift = g(H_SHIFT)?;
    let nobj = g(H_NOBJ)? as usize;
    let objtab = g(H_OBJTAB)? as usize;
    let pagetab = g(H_PAGETAB)? as usize;
    let restab = g(H_RESTAB)? as usize;
    let nres = g(H_NRES)? as usize;
    let resnames = g(H_RESNAMES)? as usize;
    let fpt = g(H_FPT)? as usize;
    let frt = g(H_FRT)? as usize;
    let npages = g(H_PAGES)? as usize;
    let data_pages = g(H_DATA)? as usize;
    if !(objtab < resnames && pagetab <= resnames && (nres == 0 || restab <= resnames) && resnames <= fpt && fpt <= frt) {
        return Err(BindError::Layout("loader tables"));
    }
    // The objects that hold resources: the trailing ones go.
    let mut res_objs: Vec<usize> = Vec::new();
    for i in 0..nres {
        let ob = r16(d, h + restab + i * 14 + 8)? as usize;
        if !res_objs.contains(&ob) {
            res_objs.push(ob);
        }
    }
    let mut keep = nobj;
    while keep > 0 && res_objs.contains(&keep) && r32(d, h + objtab + (keep - 1) * 24 + 8)? & OBJ_RESOURCE != 0 {
        keep -= 1;
    }
    // Pages of the kept objects.
    let mut kept_pages = 0usize;
    for i in 0..keep {
        let e = h + objtab + i * 24;
        let (first, count) = (r32(d, e + 12)? as usize, r32(d, e + 16)? as usize);
        if count > 0 {
            kept_pages = kept_pages.max(first + count - 1);
        }
    }
    if kept_pages > npages {
        return Err(BindError::Layout("page table"));
    }
    // Where the kept pages' data ends (relative to the data pages).
    let mut kept_end = 0usize;
    for p in 0..kept_pages {
        let e = h + pagetab + p * 8;
        let (off, size, fl) = (r32(d, e)? as usize, r16(d, e + 4)? as usize, r16(d, e + 6)?);
        if fl == 3 || fl == 4 || size == 0 {
            continue; // invalid / zero-filled: no data
        }
        kept_end = kept_end.max((off << shift) + size);
    }

    // New resource objects: one per kind of loading, sorted by (type, ID).
    list.sort_by_key(|r| (r.0, r.1));
    let mut groups: Vec<(u32, Vec<usize>)> = Vec::new();
    for (i, r) in list.iter().enumerate() {
        let k = lx_obj_flags(r.2);
        match groups.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.1.push(i),
            None => groups.push((k, vec![i])),
        }
    }
    // PRELOAD objects first, as IBM's RC writes them.
    groups.sort_by_key(|g| core::cmp::Reverse(g.0));
    struct NewObj {
        flags: u32,
        data: Vec<u8>,
    }
    let mut objs: Vec<NewObj> = Vec::new();
    let mut entries: Vec<(u16, u16, u32, u16, u32)> = Vec::new();
    for (k, idx) in &groups {
        let mut data: Vec<u8> = Vec::new();
        for &i in idx {
            let (ty, id, _, bytes) = list[i];
            if o.pack && !data.is_empty() {
                // Keep a resource within one 64K tile if it fits in one.
                let start = data.len();
                if bytes.len() <= 0x10000 && start / 0x10000 != (start + bytes.len().max(1) - 1) / 0x10000 {
                    data.resize((start + 0xffff) & !0xffff, 0);
                }
            }
            entries.push((ty, id, bytes.len() as u32, (keep + objs.len() + 1) as u16, data.len() as u32));
            data.extend_from_slice(bytes);
            while data.len() % 4 != 0 {
                data.push(0);
            }
        }
        objs.push(NewObj { flags: *k, data });
    }
    // Resource table in (type, ID) order.
    entries.sort_by_key(|e| (e.0, e.1));

    // Loader section: object, page and resource tables, then the rest.
    let new_npages = kept_pages + objs.iter().map(|ob| ob.data.len().div_ceil(page_size)).sum::<usize>();
    let new_nobj = keep + objs.len();
    let mut ldr = Vec::new();
    let new_objtab = objtab;
    // Kept objects.
    ldr.extend_from_slice(d.get(h + objtab..h + objtab + keep * 24).ok_or(BindError::Layout("objects"))?);
    let mut page = kept_pages + 1;
    for ob in &objs {
        let n = ob.data.len().div_ceil(page_size);
        let mut e = [0u8; 24];
        w32(&mut e, 0, ob.data.len() as u32);
        w32(&mut e, 4, 0);
        w32(&mut e, 8, ob.flags);
        w32(&mut e, 12, if n == 0 { 0 } else { page as u32 });
        w32(&mut e, 16, n as u32);
        ldr.extend_from_slice(&e);
        page += n;
    }
    let new_pagetab = new_objtab + ldr.len();
    ldr.extend_from_slice(d.get(h + pagetab..h + pagetab + kept_pages * 8).ok_or(BindError::Layout("pages"))?);
    // New pages after the kept data, aligned to the page shift.
    let align = 1usize << shift;
    let mut at = kept_end.div_ceil(align) * align;
    let mut new_data: Vec<u8> = Vec::new();
    let data_base = at;
    for ob in &objs {
        for chunk in ob.data.chunks(page_size) {
            let rel = at;
            let mut e = [0u8; 8];
            w32(&mut e, 0, (rel >> shift) as u32);
            // -x: an iterated (EXEPACK 1) page when that is smaller.
            let packed = if o.compress != 0 { pack_iterated(chunk) } else { None };
            let (chunk, kind): (&[u8], u16) = match &packed {
                Some(p) => (p, 1),
                None => (chunk, 0),
            };
            // Page sizes are whole alignment units (zero padded).
            let len = chunk.len().div_ceil(align) * align;
            w16(&mut e, 4, len as u16);
            w16(&mut e, 6, kind);
            ldr.extend_from_slice(&e);
            // Bytes from data_base on are written after the kept data.
            let pos = rel - data_base;
            if new_data.len() < pos {
                new_data.resize(pos, 0);
            }
            new_data.extend_from_slice(chunk);
            new_data.resize(pos + len, 0);
            at = rel + len;
        }
    }
    let new_restab = new_objtab + ldr.len();
    for (ty, id, size, ob, off) in &entries {
        let mut e = [0u8; 14];
        w16(&mut e, 0, *ty);
        w16(&mut e, 2, *id);
        w32(&mut e, 4, *size);
        w16(&mut e, 8, *ob);
        w32(&mut e, 10, *off);
        ldr.extend_from_slice(&e);
    }
    let delta = (new_objtab + ldr.len()) as isize - resnames as isize;
    // Resident names .. the fixup section, moved. A loader section size
    // that reaches into the fixup section (some of IBM's DLLs) is kept,
    // the extra bytes zero, as IBM's RC writes it.
    let old_ldr_size = r32(d, h + H_LDRSIZE)? as usize;
    ldr.extend_from_slice(d.get(h + resnames..h + fpt).ok_or(BindError::Layout("loader"))?);
    ldr.resize(ldr.len() + (objtab + old_ldr_size).saturating_sub(fpt), 0);
    let new_fpt = new_objtab + ldr.len();
    // Fixup page table: one entry per page and one more.
    let mut fix = Vec::new();
    let old_fpt_n = npages + 1;
    for p in 0..=kept_pages {
        fix.extend_from_slice(&r32(d, h + fpt + p * 4)?.to_le_bytes());
    }
    let end_val = r32(d, h + fpt + kept_pages * 4)?;
    for _ in kept_pages..new_npages {
        fix.extend_from_slice(&end_val.to_le_bytes());
    }
    let delta2 = (new_npages + 1) as isize * 4 - old_fpt_n as isize * 4;
    let fix_shift = new_fpt as isize - fpt as isize + delta2;
    // Fixup records and import tables, to the end of the import procedure
    // table (a 0 byte, then names up to an empty one); padding is dropped.
    let iproc = r32(d, h + H_IPROC)? as usize;
    let limit = data_pages.checked_sub(h).ok_or(BindError::Layout("data pages"))?;
    let mut iproc_end = iproc + 1;
    while iproc_end < limit && d[h + iproc_end] != 0 {
        iproc_end += 1 + d[h + iproc_end] as usize;
    }
    let iproc_end = iproc_end.min(limit);
    let rest_from = fpt + old_fpt_n * 4;
    if rest_from > iproc_end {
        return Err(BindError::Layout("fixup section"));
    }
    fix.extend_from_slice(d.get(h + rest_from..h + iproc_end).ok_or(BindError::Layout("fixups"))?);

    // The new header.
    let mut hdr = d[h..h + objtab].to_vec();
    let sh = |v: usize, from: usize, by: isize| -> u32 { if v >= from { (v as isize + by) as u32 } else { v as u32 } };
    w32(&mut hdr, H_PAGES, new_npages as u32);
    w32(&mut hdr, H_NOBJ, new_nobj as u32);
    w32(&mut hdr, H_OBJTAB, new_objtab as u32);
    w32(&mut hdr, H_PAGETAB, new_pagetab as u32);
    w32(&mut hdr, H_RESTAB, if entries.is_empty() { 0 } else { new_restab as u32 });
    w32(&mut hdr, H_NRES, entries.len() as u32);
    for f in [H_RESNAMES, H_ENTRY, H_DIR, H_CHK] {
        let v = r32(d, h + f)? as usize;
        if v != 0 {
            w32(&mut hdr, f, sh(v, resnames, delta));
        }
    }
    w32(&mut hdr, H_FPT, new_fpt as u32);
    for f in [H_FRT, H_IMOD, H_IPROC] {
        let v = r32(d, h + f)? as usize;
        w32(&mut hdr, f, (v as isize + fix_shift) as u32);
    }
    w32(&mut hdr, H_LDRSIZE, (old_ldr_size as isize + delta) as u32);
    w32(&mut hdr, H_FIXSIZE, (r32(d, h + H_FIXSIZE)? as isize + delta2) as u32);
    // The data pages start on a page-shift boundary after the tables.
    let unaligned = h + new_fpt + fix.len();
    let new_data_pages = unaligned.div_ceil(align) * align;
    // Preload pages: the old resource objects' go, the new ones' count.
    let mut old_preload = 0u32;
    for i in keep..nobj {
        let e = h + objtab + i * 24;
        if r32(d, e + 8)? & OBJ_PRELOAD != 0 {
            old_preload += r32(d, e + 16)?;
        }
    }
    let preload_pages: usize = objs.iter().filter(|ob| ob.flags & OBJ_PRELOAD != 0).map(|ob| ob.data.len().div_ceil(page_size)).sum();
    w32(&mut hdr, 0x84, r32(d, h + 0x84)?.saturating_sub(old_preload) + preload_pages as u32);
    w32(&mut hdr, H_DATA, new_data_pages as u32);
    if r32(d, h + H_ITER)? as usize == data_pages {
        w32(&mut hdr, H_ITER, new_data_pages as u32);
    }

    let mut out = Vec::with_capacity(d.len() + new_data.len());
    out.extend_from_slice(&d[..h]);
    out.extend_from_slice(&hdr);
    out.extend_from_slice(&ldr);
    out.extend_from_slice(&fix);
    out.resize(new_data_pages, 0);
    // Kept pages as they were, then the resources.
    out.extend_from_slice(d.get(data_pages..data_pages + kept_end).ok_or(BindError::Layout("page data"))?);
    out.resize(new_data_pages + data_base, 0);
    out.extend_from_slice(&new_data);
    let mut old_end = data_pages + kept_end;
    // Non-resident names and debug information.
    let hh = h;
    for (off_f, len_f) in [(H_NONRES, H_NONRESLEN), (H_DEBUG, H_DEBUGLEN)] {
        let (off, len) = (r32(d, h + off_f)? as usize, r32(d, h + len_f)? as usize);
        if off == 0 || len == 0 {
            continue;
        }
        let blob = d.get(off..off + len).ok_or(BindError::Layout("names/debug"))?;
        old_end = old_end.max(off + len);
        let at = out.len() as u32;
        w32(&mut out, hh + off_f, at);
        out.extend_from_slice(blob);
    }
    // Anything else after the image (overlays, signatures) stays at the end.
    let mut tail_from = data_pages;
    for p in 0..npages {
        let e = h + pagetab + p * 8;
        let (off, size) = (r32(d, e)? as usize, r16(d, e + 4)? as usize);
        tail_from = tail_from.max(data_pages + (off << shift) + size);
    }
    tail_from = tail_from.max(old_end);
    if tail_from < d.len() {
        out.extend_from_slice(&d[tail_from..]);
    }
    Ok(out)
}

/// EXEPACK 1 (iterated data) of a page: records of (count, length,
/// bytes), runs of one byte packed; `None` if that is not smaller.
fn pack_iterated(page: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let lit = |out: &mut Vec<u8>, b: &[u8]| {
        if !b.is_empty() {
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&(b.len() as u16).to_le_bytes());
            out.extend_from_slice(b);
        }
    };
    let (mut i, mut from) = (0, 0);
    while i < page.len() {
        let mut r = 1;
        while i + r < page.len() && page[i + r] == page[i] {
            r += 1;
        }
        if r >= 8 {
            lit(&mut out, &page[from..i]);
            out.extend_from_slice(&(r as u16).to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.push(page[i]);
            i += r;
            from = i;
        } else {
            i += r;
        }
    }
    lit(&mut out, &page[from..]);
    (out.len() + 4 < page.len()).then_some(out)
}

// ---- NE ----

/// NE segment flags of a resource: its load/memory flags, a data segment,
/// DPL 3 and the 0x80 bit, as IBM's RC writes them.
fn ne_seg_flags(f: u16) -> u16 {
    (f & (flags::MOVEABLE | flags::PURE | flags::PRELOAD | flags::DISCARDABLE)) | 0x0c81
}

fn ne(d: &[u8], h: usize, list: &[(u16, u16, u16, &[u8])]) -> Result<Vec<u8>, BindError> {
    let w = |o: usize| r16(d, h + o);
    let nseg = w(0x1c)? as usize;
    let segtab = w(0x22)? as usize;
    let resnames = w(0x26)? as usize;
    let modref = w(0x28)? as usize;
    let imp = w(0x2a)? as usize;
    let entry = w(0x04)? as usize;
    let entry_len = w(0x06)? as usize;
    let nonres = r32(d, h + 0x2c)? as usize;
    let nonres_len = w(0x20)? as usize;
    let shift = w(0x32)? as u32;
    let nres = w(0x34)? as usize;
    let sector = 1usize << if shift == 0 { 9 } else { shift };
    if nres > nseg || resnames < segtab + nseg * 8 {
        return Err(BindError::Layout("NE tables"));
    }
    let keep = nseg - nres;
    // End of the resident tables (relative to the header).
    let mut tables_end = (entry + entry_len).max(imp).max(modref).max(resnames);
    // A non-resident name table right after them moves with them.
    let nonres_in_tables = nonres >= h + resnames && nonres <= h + tables_end + 16;
    if nonres_in_tables {
        tables_end = tables_end.max(nonres - h + nonres_len);
    }
    // A segment: (sector offset, file end with relocations).
    let seg = |i: usize| -> Result<(usize, usize), BindError> {
        let e = h + segtab + i * 8;
        let (off, len, fl) = (r16(d, e)? as usize, r16(d, e + 2)? as usize, r16(d, e + 4)?);
        let start = off * sector;
        let len = if len == 0 && off != 0 { 0x10000 } else { len };
        let mut end = start + len;
        if off != 0 && fl & 0x100 != 0 {
            let n = r16(d, end)? as usize;
            end += 2 + n * 8;
        }
        Ok((off, end))
    };
    let mut first_data = usize::MAX;
    let mut kept_end = 0usize;
    for i in 0..keep {
        let (off, end) = seg(i)?;
        if off != 0 {
            first_data = first_data.min(off * sector);
            kept_end = kept_end.max(end);
        }
    }
    let mut old_end = kept_end;
    for i in keep..nseg {
        old_end = old_end.max(seg(i)?.1);
    }
    if !nonres_in_tables && nonres != 0 {
        old_end = old_end.max(nonres + nonres_len);
    }
    // New tables: segments, resources, then the rest moved.
    let mut tab = Vec::new();
    tab.extend_from_slice(&d[h + segtab..h + segtab + keep * 8]);
    let seg_entries_at = tab.len();
    tab.resize(tab.len() + list.len() * 8, 0);
    let new_restab = segtab + tab.len();
    let mut sorted: Vec<&(u16, u16, u16, &[u8])> = list.iter().collect();
    sorted.sort_by_key(|r| (r.0, r.1));
    for r in &sorted {
        tab.extend_from_slice(&r.0.to_le_bytes());
        tab.extend_from_slice(&r.1.to_le_bytes());
    }
    let delta = (segtab + tab.len()) as isize - resnames as isize;
    tab.extend_from_slice(d.get(h + resnames..h + tables_end).ok_or(BindError::Layout("NE tables"))?);
    let hdr_end = h + segtab + tab.len();
    // Kept segment data moves by whole sectors if the tables grew into it.
    let move_by = if first_data == usize::MAX || hdr_end <= first_data { 0 } else { (hdr_end - first_data).div_ceil(sector) };
    let mut out = Vec::with_capacity(d.len() + 4096);
    out.extend_from_slice(&d[..h + segtab]);
    out.extend_from_slice(&tab);
    if first_data != usize::MAX {
        out.resize(first_data + move_by * sector, 0);
        out.extend_from_slice(&d[first_data..kept_end]);
    }
    for i in 0..keep {
        let e = h + segtab + i * 8;
        let off = r16(d, e)? as usize;
        if off != 0 {
            w16(&mut out, e, (off + move_by) as u16);
        }
    }
    // Resources, each on a sector boundary.
    for (k, r) in sorted.iter().enumerate() {
        let at = out.len().div_ceil(sector) * sector;
        out.resize(at, 0);
        out.extend_from_slice(r.3);
        let e = h + segtab + seg_entries_at + k * 8;
        w16(&mut out, e, (at / sector) as u16);
        w16(&mut out, e + 2, r.3.len() as u16);
        w16(&mut out, e + 4, ne_seg_flags(r.2));
        w16(&mut out, e + 6, r.3.len() as u16);
    }
    let pad = out.len().div_ceil(sector) * sector;
    out.resize(pad, 0);
    // Header fields.
    let sh = |v: usize| -> u16 { if v >= resnames { (v as isize + delta) as u16 } else { v as u16 } };
    w16(&mut out, h + 0x1c, (keep + list.len()) as u16);
    w16(&mut out, h + 0x24, new_restab as u16);
    w16(&mut out, h + 0x34, list.len() as u16);
    w16(&mut out, h + 0x26, sh(resnames));
    w16(&mut out, h + 0x28, sh(modref));
    w16(&mut out, h + 0x2a, sh(imp));
    w16(&mut out, h + 0x04, sh(entry));
    if nonres_in_tables {
        w32(&mut out, h + 0x2c, (nonres as isize + delta) as u32);
    } else if nonres != 0 {
        let at = out.len() as u32;
        out.extend_from_slice(d.get(nonres..nonres + nonres_len).ok_or(BindError::Layout("NE names"))?);
        w32(&mut out, h + 0x2c, at);
    }
    // Anything after the image (debug information) stays at the end;
    // padding does not.
    if old_end < d.len() && d[old_end..].iter().any(|&b| b != 0) {
        out.extend_from_slice(&d[old_end..]);
    }
    Ok(out)
}
