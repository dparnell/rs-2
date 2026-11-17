//! LINK386's output: OS/2 32-bit linear executables (LX).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::def::{AppType, Kind};
use crate::link::{Linked, Linker, RTarget, RelocKind};
use crate::omf::{Combine, ImpEntry};
use crate::{pstr, put16, put32};

const PAGE: u32 = 4096;

pub mod of {
    pub const READ: u32 = 0x0001;
    pub const WRITE: u32 = 0x0002;
    pub const EXEC: u32 = 0x0004;
    pub const DISCARD: u32 = 0x0010;
    pub const SHARED: u32 = 0x0020;
    pub const PRELOAD: u32 = 0x0040;
    pub const ALIAS16: u32 = 0x1000;
    pub const BIG: u32 = 0x2000;
    pub const CONFORM: u32 = 0x4000;
    pub const IOPL: u32 = 0x8000;
}

fn src_type(k: RelocKind) -> Option<u8> {
    Some(match k {
        RelocKind::LoByte => 0x00,
        RelocKind::Selector => 0x02,
        RelocKind::Ptr1616 => 0x03,
        RelocKind::Offset16 => 0x05,
        RelocKind::Ptr1632 => 0x06,
        RelocKind::Offset32 => 0x07,
        RelocKind::SelfRel32 => 0x08,
        RelocKind::SelfRel16 => return None,
    })
}

fn src_size(k: RelocKind) -> u32 {
    match k {
        RelocKind::LoByte => 1,
        RelocKind::Selector | RelocKind::Offset16 | RelocKind::SelfRel16 => 2,
        RelocKind::Ptr1616 | RelocKind::Offset32 | RelocKind::SelfRel32 => 4,
        RelocKind::Ptr1632 => 6,
    }
}

pub fn write(l: &mut Linker, o: &mut Linked) -> Option<Vec<u8>> {
    let dll = o.dll;
    let def = o.def.clone();
    let shift: u32 = l.opt.align.unwrap_or(512).trailing_zeros();
    let base: u32 = match (dll, l.opt.base.or(def.base)) {
        (false, Some(b)) if b != 0x10000 => {
            l.msgs.warn(l.host, "", 4083, &format!("{b:#x} : a program's base must be 0x10000; using 0x10000"));
            0x10000
        }
        (_, Some(b)) => b.next_multiple_of(0x10000),
        _ => 0x10000,
    };
    // The stack: a STACK segment, or STACKSIZE added to the data object.
    let mut stack = l.opt.stack.or(def.stacksize).unwrap_or(0);
    let mut esp: Option<(usize, u32)> = None;
    let autodata = o.autodata.or_else(|| o.units.iter().position(|u| !u.code && u.attrs.shared != Some(true)));
    if !dll {
        if let Some(s) = o.lsegs.iter().find(|s| s.combine == Combine::Stack && s.absolute.is_none()) {
            let (u, end) = (s.unit, s.offset + s.size);
            // A stack segment smaller than the requested size grows.
            let have = s.size;
            if stack > have && o.units[u].lsegs.last() == Some(&o.lsegs.iter().position(|x| core::ptr::eq(x, s)).unwrap()) {
                o.units[u].size += stack - have;
                esp = Some((u, end + stack - have));
            } else {
                esp = Some((u, end));
            }
            stack = stack.max(have);
        } else if let Some(a) = autodata {
            if stack == 0 {
                l.msgs.warn(l.host, "", 4021, "no stack segment");
            }
            let u = &mut o.units[a];
            u.size = u.size.next_multiple_of(4) + stack;
            esp = Some((a, u.size));
        } else if stack > 0 {
            // No data object at all: a stack object of its own.
            o.units.push(crate::link::Unit {
                size: stack,
                name: String::from("STACK"),
                class: String::from("STACK"),
                use32: true,
                ..Default::default()
            });
            let n = o.units.len() - 1;
            esp = Some((n, stack));
        } else {
            l.msgs.warn(l.host, "", 4021, "no stack segment");
        }
    }
    // Object addresses.
    let mut va = base;
    for u in o.units.iter_mut() {
        u.base = va;
        va = (va + u.size.max(1)).next_multiple_of(0x10000);
    }
    let units = &o.units;
    // Flat offsets in the data hold the final addresses.
    let mut datas: Vec<Vec<u8>> = units.iter().map(|u| u.data.clone()).collect();
    for (ui, u) in units.iter().enumerate() {
        for r in &u.relocs {
            if let RTarget::Internal { unit, offset } = r.target {
                if unit == usize::MAX {
                    continue;
                }
                let at = r.offset as usize;
                match r.kind {
                    RelocKind::Offset32 => {
                        let v = units[unit].base.wrapping_add(offset);
                        datas[ui][at..at + 4].copy_from_slice(&v.to_le_bytes());
                    }
                    RelocKind::SelfRel32 => {
                        let v = units[unit].base.wrapping_add(offset).wrapping_sub(u.base + r.offset + 4);
                        datas[ui][at..at + 4].copy_from_slice(&v.to_le_bytes());
                    }
                    RelocKind::Ptr1632 if !units[unit].use32 => {
                        datas[ui][at..at + 4].copy_from_slice(&offset.to_le_bytes());
                    }
                    RelocKind::Ptr1632 => {
                        let v = units[unit].base.wrapping_add(offset);
                        datas[ui][at..at + 4].copy_from_slice(&v.to_le_bytes());
                    }
                    RelocKind::Ptr1616 | RelocKind::Offset16 => {
                        datas[ui][at..at + 2].copy_from_slice(&(offset as u16).to_le_bytes());
                    }
                    _ => {}
                }
            }
        }
    }
    // Internal fix-ups a program does not need (it always loads at its base).
    let strip_internal = !dll;
    // Import procedure names.
    let mut procs: Vec<u8> = vec![0];
    let mut proc_off: BTreeMap<String, u32> = BTreeMap::new();
    let mut proc_name = |s: &str, t: &mut Vec<u8>| -> u32 {
        if let Some(&o) = proc_off.get(s) {
            return o;
        }
        let o = t.len() as u32;
        pstr(t, s);
        proc_off.insert(String::from(s), o);
        o
    };
    // Pages.
    struct Pg {
        data: Vec<u8>,
        fix: Vec<u8>,
    }
    let mut pages: Vec<Pg> = Vec::new();
    let mut first_page: Vec<u32> = Vec::new();
    let mut npages: Vec<u32> = Vec::new();
    for (ui, u) in units.iter().enumerate() {
        first_page.push(pages.len() as u32 + 1);
        let mut init = (u.init as usize).min(datas[ui].len());
        for r in &u.relocs {
            init = init.max((r.offset + src_size(r.kind)) as usize).min(datas[ui].len());
        }
        let n = (init as u32).div_ceil(PAGE);
        npages.push(n);
        for p in 0..n {
            let s = (p * PAGE) as usize;
            let e = (s + PAGE as usize).min(init);
            let mut d = datas[ui][s..e].to_vec();
            while d.last() == Some(&0) {
                d.pop();
            }
            pages.push(Pg { data: d, fix: Vec::new() });
        }
    }
    let mut bad = 0;
    for (ui, u) in units.iter().enumerate() {
        for r in &u.relocs {
            let mut st = match src_type(r.kind) {
                Some(s) => s,
                None => {
                    bad += 1;
                    continue;
                }
            };
            let mut tail: Vec<u8> = Vec::new();
            let mut flags: u8;
            match &r.target {
                RTarget::Internal { unit, offset } => {
                    if *unit == usize::MAX {
                        continue;
                    }
                    if strip_internal && matches!(r.kind, RelocKind::Offset32 | RelocKind::SelfRel32) {
                        continue;
                    }
                    if strip_internal && r.kind == RelocKind::SelfRel32 {
                        continue;
                    }
                    flags = 0;
                    let obj = (*unit + 1) as u16;
                    if obj > 255 {
                        flags |= 0x40;
                        put16(&mut tail, obj);
                    } else {
                        tail.push(obj as u8);
                    }
                    if r.kind != RelocKind::Selector {
                        if *offset > 0xffff {
                            flags |= 0x10;
                            put32(&mut tail, *offset);
                        } else {
                            put16(&mut tail, *offset as u16);
                        }
                    }
                    // 16:16 references to a 32-bit object go through its alias.
                    if units[*unit].use32 && matches!(r.kind, RelocKind::Ptr1616 | RelocKind::Selector | RelocKind::Offset16) {
                        st |= 0x10;
                    }
                }
                RTarget::ImportOrd { module, ordinal } => {
                    flags = 1;
                    let m = (*module + 1) as u16;
                    if m > 255 {
                        flags |= 0x40;
                        put16(&mut tail, m);
                    } else {
                        tail.push(m as u8);
                    }
                    if *ordinal < 256 {
                        flags |= 0x80;
                        tail.push(*ordinal as u8);
                    } else {
                        put16(&mut tail, *ordinal);
                    }
                }
                RTarget::ImportName { module, name } => {
                    flags = 2;
                    let m = (*module + 1) as u16;
                    if m > 255 {
                        flags |= 0x40;
                        put16(&mut tail, m);
                    } else {
                        tail.push(m as u8);
                    }
                    let po = proc_name(name, &mut procs);
                    if po > 0xffff {
                        flags |= 0x10;
                        put32(&mut tail, po);
                    } else {
                        put16(&mut tail, po as u16);
                    }
                }
            }
            if r.add != 0 && !matches!(r.target, RTarget::Internal { .. }) {
                if r.add as u32 > 0xffff {
                    flags |= 0x04 | 0x20;
                    put32(&mut tail, r.add as u32);
                } else {
                    flags |= 0x04;
                    put16(&mut tail, r.add as u16);
                }
            }
            // The record goes to every page the location touches.
            let size = src_size(r.kind);
            let first = r.offset / PAGE;
            let last = (r.offset + size - 1) / PAGE;
            for p in first..=last {
                if p >= npages[ui] {
                    break;
                }
                let pg = &mut pages[(first_page[ui] - 1 + p) as usize];
                let srcoff = (r.offset as i64 - (p * PAGE) as i64) as i16;
                pg.fix.push(st);
                pg.fix.push(flags);
                pg.fix.extend_from_slice(&srcoff.to_le_bytes());
                pg.fix.extend_from_slice(&tail);
            }
        }
    }
    if bad > 0 {
        l.msgs.error(l.host, "", 2005, "16-bit self-relative fix-ups to other objects are not possible in LX");
    }
    // Entry table.
    let mut exports = o.exports.clone();
    exports.sort_by_key(|e| e.ordinal);
    let mut entry: Vec<u8> = Vec::new();
    {
        let mut i = 0;
        let mut next = 1u16;
        // Bundle kinds: (type, object).
        let kind = |e: &crate::link::ExportEnt| -> (u8, u16) {
            if e.forward.is_some() {
                (4, 0)
            } else if let Some((u, _)) = e.at {
                if !units[u].use32 {
                    if e.pwords > 0 { (2, u as u16 + 1) } else { (1, u as u16 + 1) }
                } else {
                    (3, u as u16 + 1)
                }
            } else {
                (3, 0)
            }
        };
        while i < exports.len() {
            if exports[i].ordinal < next {
                i += 1;
                continue;
            }
            if exports[i].ordinal > next {
                let mut gap = exports[i].ordinal - next;
                while gap > 0 {
                    let n = gap.min(255);
                    entry.push(n as u8);
                    entry.push(0);
                    gap -= n;
                }
                next = exports[i].ordinal;
            }
            let k = kind(&exports[i]);
            let mut j = i;
            while j < exports.len() && j - i < 255 && exports[j].ordinal == next + (j - i) as u16 && kind(&exports[j]) == k {
                j += 1;
            }
            entry.push((j - i) as u8);
            entry.push(k.0);
            put16(&mut entry, k.1);
            for e in &exports[i..j] {
                let mut fl = 1u8;
                if k.0 != 4 {
                    if !e.nodata && (k.0 == 1 || k.0 == 2) {
                        fl |= 2;
                    }
                    fl |= (e.pwords & 0x1f) << 3;
                }
                match k.0 {
                    1 => {
                        entry.push(fl);
                        put16(&mut entry, e.at.map(|a| a.1).unwrap_or(0) as u16);
                    }
                    2 => {
                        entry.push(fl);
                        put16(&mut entry, e.at.map(|a| a.1).unwrap_or(0) as u16);
                        put16(&mut entry, 0);
                    }
                    3 => {
                        entry.push(fl);
                        put32(&mut entry, e.at.map(|a| a.1).or(e.absolute).unwrap_or(0));
                    }
                    _ => {
                        let (m, ent) = e.forward.clone().unwrap();
                        match ent {
                            ImpEntry::Ordinal(n) => {
                                entry.push(1);
                                put16(&mut entry, m as u16 + 1);
                                put32(&mut entry, n as u32);
                            }
                            ImpEntry::Name(nm) => {
                                entry.push(0);
                                put16(&mut entry, m as u16 + 1);
                                let po = proc_name(&nm, &mut procs);
                                put32(&mut entry, po);
                            }
                        }
                    }
                }
            }
            next += (j - i) as u16;
            i = j;
        }
        entry.push(0);
    }
    // Names.
    let mut resident: Vec<u8> = Vec::new();
    pstr(&mut resident, &o.module_name);
    put16(&mut resident, 0);
    let mut nonres: Vec<u8> = Vec::new();
    if !o.description.is_empty() {
        pstr(&mut nonres, &o.description);
        put16(&mut nonres, 0);
    }
    for e in &exports {
        if e.noname {
            continue;
        }
        if e.resident {
            pstr(&mut resident, &e.name);
            put16(&mut resident, e.ordinal);
        } else {
            pstr(&mut nonres, &e.name);
            put16(&mut nonres, e.ordinal);
        }
    }
    resident.push(0);
    nonres.push(0);
    let mut imods: Vec<u8> = Vec::new();
    for m in &o.imports {
        pstr(&mut imods, m);
    }
    // Object flags.
    let mut oflags = Vec::new();
    for (i, u) in units.iter().enumerate() {
        let a = &u.attrs;
        let mut f = of::READ;
        if u.code {
            f |= of::EXEC;
        } else if a.readonly != Some(true) {
            f |= of::WRITE;
        }
        if u.use32 {
            f |= of::BIG;
        }
        if !u.use32 || a.alias || a.mixed1632 {
            f |= of::ALIAS16;
        }
        let shared = if u.code {
            a.shared == Some(true)
        } else if dll {
            // DLL data is shared unless DATA MULTIPLE / NONSHARED.
            a.shared != Some(false) && !(Some(i) == autodata && def.data.instance == Some(2))
        } else {
            a.shared == Some(true)
        };
        if shared {
            f |= of::SHARED;
        }
        if a.preload == Some(true) {
            f |= of::PRELOAD;
        }
        if a.discardable == Some(true) {
            f |= of::DISCARD;
        }
        if a.conforming == Some(true) && u.code {
            f |= of::CONFORM;
        }
        if a.iopl == Some(true) {
            f |= of::IOPL;
        }
        oflags.push(f);
    }
    // Module flags.
    let mut mflags: u32 = 0;
    if strip_internal {
        mflags |= 0x10;
    }
    let app = l.opt.pmtype.unwrap_or(def.app);
    mflags |= match app {
        AppType::NoVio => 0x100,
        AppType::Vio => 0x200,
        AppType::Pm => 0x300,
        AppType::Unspecified => 0,
    };
    match def.kind {
        Kind::PhysicalDevice => mflags |= 0x20000,
        Kind::VirtualDevice => mflags |= 0x28000,
        _ => {
            if dll {
                mflags |= 0x8000;
            }
        }
    }
    if dll && def.init_instance == Some(true) {
        mflags |= 0x4;
    }
    if dll && def.term_instance == Some(true) {
        mflags |= 0x4000_0000;
    }
    if l.msgs.errors > 0 || o.errors > 0 {
        mflags |= 0x2000;
    }
    // Layout.
    let hdr_size = 0xc4usize;
    let obj_table = hdr_size;
    let page_table = obj_table + units.len() * 24;
    let rsrc_table = page_table + pages.len() * 8;
    let resname = rsrc_table;
    let entry_off = resname + resident.len();
    let loader_end = entry_off + entry.len();
    let fixpage = loader_end;
    let fixrec = fixpage + (pages.len() + 1) * 4;
    let fixsize: usize = pages.iter().map(|p| p.fix.len()).sum();
    let imod_off = fixrec + fixsize;
    let iproc_off = imod_off + imods.len();
    let fix_end = iproc_off + procs.len();
    let stub = match (&def.stub, def.stub_none) {
        (Some(s), _) => crate::ne::load_stub(l, s)?,
        (None, true) => Vec::new(),
        (None, false) => crate::ne::dos_stub(),
    };
    let hdr = stub.len();
    let align = 1usize << shift;
    let nonres_off = hdr + fix_end;
    let data_off = (nonres_off + nonres.len()).next_multiple_of(align);
    let mut f = stub;
    if f.len() >= 0x40 {
        f[0x3c..0x40].copy_from_slice(&(hdr as u32).to_le_bytes());
    }
    let mut h = vec![0u8; hdr_size];
    let w32 = |h: &mut Vec<u8>, o: usize, v: u32| h[o..o + 4].copy_from_slice(&v.to_le_bytes());
    h[0..2].copy_from_slice(b"LX");
    h[8..10].copy_from_slice(&2u16.to_le_bytes());
    h[10..12].copy_from_slice(&1u16.to_le_bytes());
    w32(&mut h, 0x10, mflags);
    w32(&mut h, 0x14, pages.len() as u32);
    let (eip_o, eip) = o.start.map(|(u, off)| (u as u32 + 1, off)).unwrap_or((0, 0));
    if o.start.is_none() && !dll {
        l.msgs.warn(l.host, "", 4038, "the program has no starting address");
    }
    w32(&mut h, 0x18, eip_o);
    w32(&mut h, 0x1c, eip);
    let (esp_o, espv) = esp.map(|(u, e)| (u as u32 + 1, e)).unwrap_or((0, 0));
    w32(&mut h, 0x20, esp_o);
    w32(&mut h, 0x24, espv);
    w32(&mut h, 0x28, PAGE);
    w32(&mut h, 0x2c, shift);
    w32(&mut h, 0x30, (fix_end - fixpage) as u32);
    w32(&mut h, 0x38, (loader_end - obj_table) as u32);
    w32(&mut h, 0x40, obj_table as u32);
    w32(&mut h, 0x44, units.len() as u32);
    w32(&mut h, 0x48, page_table as u32);
    w32(&mut h, 0x50, rsrc_table as u32);
    w32(&mut h, 0x58, resname as u32);
    w32(&mut h, 0x5c, entry_off as u32);
    w32(&mut h, 0x68, fixpage as u32);
    w32(&mut h, 0x6c, fixrec as u32);
    w32(&mut h, 0x70, imod_off as u32);
    w32(&mut h, 0x74, o.imports.len() as u32);
    w32(&mut h, 0x78, iproc_off as u32);
    w32(&mut h, 0x80, data_off as u32);
    w32(&mut h, 0x88, nonres_off as u32);
    w32(&mut h, 0x8c, nonres.len() as u32);
    w32(&mut h, 0x94, autodata.map(|a| a as u32 + 1).unwrap_or(0));
    w32(&mut h, 0xa8, def.heapsize.unwrap_or(0));
    w32(&mut h, 0xac, stack);
    f.extend_from_slice(&h);
    for (i, u) in units.iter().enumerate() {
        put32(&mut f, u.size);
        put32(&mut f, u.base);
        put32(&mut f, oflags[i]);
        put32(&mut f, if npages[i] == 0 { 0 } else { first_page[i] });
        put32(&mut f, npages[i]);
        put32(&mut f, 0);
    }
    let mut off = 0u32;
    for p in &pages {
        put32(&mut f, if p.data.is_empty() { 0 } else { off >> shift });
        put16(&mut f, p.data.len() as u16);
        put16(&mut f, if p.data.is_empty() { 3 } else { 0 });
        if !p.data.is_empty() {
            off = (off + p.data.len() as u32).next_multiple_of(align as u32);
        }
    }
    f.extend_from_slice(&resident);
    f.extend_from_slice(&entry);
    let mut acc = 0u32;
    for p in &pages {
        put32(&mut f, acc);
        acc += p.fix.len() as u32;
    }
    put32(&mut f, acc);
    for p in &pages {
        f.extend_from_slice(&p.fix);
    }
    f.extend_from_slice(&imods);
    f.extend_from_slice(&procs);
    f.extend_from_slice(&nonres);
    f.resize(data_off, 0);
    for p in &pages {
        if p.data.is_empty() {
            continue;
        }
        f.extend_from_slice(&p.data);
        f.resize(f.len().next_multiple_of(align), 0);
    }
    Some(f)
}
