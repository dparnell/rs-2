//! LINK.EXE's output: OS/2 16-bit segmented executables (NE).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::def::{AppType, Kind};
use crate::link::{Linked, Linker, RTarget, RelocKind};
use crate::omf::ImpEntry;
use crate::{pstr, put16, put32};

/// The DOS program in front of the new header: it says the program needs
/// OS/2 and ends. 0x80 bytes; the new header follows it.
pub fn dos_stub() -> Vec<u8> {
    let msg = b"This program runs under OS/2 only.\r\n$";
    let mut s = vec![0u8; 0x40];
    s[0] = b'M';
    s[1] = b'Z';
    // Code at 0x40: push cs; pop ds; mov dx,msg; mov ah,9; int 21h;
    // mov ax,4C01h; int 21h.
    let code: [u8; 14] = [0x0e, 0x1f, 0xba, 0x0e, 0x00, 0xb4, 0x09, 0xcd, 0x21, 0xb8, 0x01, 0x4c, 0xcd, 0x21];
    s.extend_from_slice(&code);
    s.extend_from_slice(msg);
    s.resize(0x80, 0);
    let total = s.len() as u16;
    // e_cblp, e_cp, e_crlc, e_cparhdr, minalloc, maxalloc, ss, sp, csum,
    // ip, cs, lfarlc, ovno.
    let w = |s: &mut Vec<u8>, o: usize, v: u16| s[o..o + 2].copy_from_slice(&v.to_le_bytes());
    w(&mut s, 2, total % 512);
    w(&mut s, 4, total.div_ceil(512));
    w(&mut s, 6, 0);
    w(&mut s, 8, 4);
    w(&mut s, 0x0a, 0);
    w(&mut s, 0x0c, 0xffff);
    w(&mut s, 0x0e, 0);
    w(&mut s, 0x10, 0xb8);
    w(&mut s, 0x14, 0);
    w(&mut s, 0x16, 0);
    w(&mut s, 0x18, 0x40);
    s
}

/// A stub from the STUB statement: an MZ program, with its new-header
/// pointer to be filled in. Its size is rounded up to 16 bytes and at
/// least 0x40 header bytes.
pub fn load_stub(l: &mut Linker, name: &str) -> Option<Vec<u8>> {
    let dirs = l.opt.lib_dirs.clone();
    let Some((_, mut d)) = crate::cmd::find_file(l.host, name, &dirs) else {
        l.msgs.fatal(l.host, "", 1082, &format!("{name} : stub file not found"));
        return None;
    };
    if d.len() < 0x40 || &d[0..2] != b"MZ" {
        l.msgs.fatal(l.host, "", 1100, &format!("{name} : the stub is not a DOS program"));
        return None;
    }
    // Only the DOS program itself (its own size from the header).
    let pages = u16::from_le_bytes([d[4], d[5]]) as usize;
    let last = u16::from_le_bytes([d[2], d[3]]) as usize;
    let size = if pages == 0 { d.len() } else { (pages - 1) * 512 + if last == 0 { 512 } else { last } };
    d.truncate(size.min(d.len()));
    // Relocations must be at 0x40 or later, as for any new-format program.
    if u16::from_le_bytes([d[0x18], d[0x19]]) < 0x40 {
        d[0x18..0x1a].copy_from_slice(&0x40u16.to_le_bytes());
    }
    d.resize(d.len().max(0x40).next_multiple_of(16), 0);
    Some(d)
}

fn reloc_type(k: RelocKind) -> u8 {
    match k {
        RelocKind::LoByte => 0,
        RelocKind::Selector => 2,
        RelocKind::Ptr1616 => 3,
        RelocKind::Offset16 | RelocKind::SelfRel16 => 5,
        RelocKind::Ptr1632 => 11,
        RelocKind::Offset32 | RelocKind::SelfRel32 => 13,
    }
}

/// NE segment flags.
pub mod sf {
    pub const DATA: u16 = 0x0001;
    pub const ITERATED: u16 = 0x0008;
    pub const MOVEABLE: u16 = 0x0010;
    pub const SHARED: u16 = 0x0020;
    pub const PRELOAD: u16 = 0x0040;
    pub const READONLY: u16 = 0x0080;
    pub const RELOCS: u16 = 0x0100;
    pub const CONFORMING: u16 = 0x0200;
    pub const DPL3: u16 = 0x0c00;
    pub const IOPL: u16 = 0x0800;
    pub const DISCARDABLE: u16 = 0x1000;
    pub const BIG: u16 = 0x2000;
}

pub fn write(l: &mut Linker, o: &mut Linked) -> Option<Vec<u8>> {
    let dll = o.dll;
    let def = o.def.clone();
    let shift: u32 = l.opt.align.unwrap_or(512).trailing_zeros();
    let units = &o.units;
    // Imported names table: a leading empty name, then names as needed.
    let mut imp_names: Vec<u8> = vec![0];
    let mut imp_off: BTreeMap<String, u16> = BTreeMap::new();
    let mut name_off = |s: &str, t: &mut Vec<u8>| -> u16 {
        if let Some(&o) = imp_off.get(s) {
            return o;
        }
        let o = t.len() as u16;
        pstr(t, s);
        imp_off.insert(String::from(s), o);
        o
    };
    let mut modref: Vec<u16> = Vec::new();
    for m in &o.imports {
        modref.push(name_off(m, &mut imp_names));
    }
    // Relocations per segment, chained as the loader expects.
    let mut seg_relocs: Vec<Vec<[u8; 8]>> = Vec::new();
    let mut iterated: Vec<bool> = Vec::new();
    let mut seg_data: Vec<Vec<u8>> = Vec::new();
    for u in units {
        let mut data = u.data.clone();
        let mut recs: Vec<[u8; 8]> = Vec::new();
        // (type, target bytes) -> index of its record (chains).
        let mut chains: BTreeMap<(u8, [u8; 6]), usize> = BTreeMap::new();
        for r in &u.relocs {
            let ty = reloc_type(r.kind);
            let (flags, tgt): (u8, [u8; 6]) = match &r.target {
                RTarget::Internal { unit, offset } => {
                    let s = (*unit + 1) as u8;
                    let mut t = [0u8; 6];
                    t[0] = s;
                    t[2..4].copy_from_slice(&(*offset as u16).to_le_bytes());
                    (0, t)
                }
                RTarget::ImportOrd { module, ordinal } => {
                    let mut t = [0u8; 6];
                    t[0..2].copy_from_slice(&((*module + 1) as u16).to_le_bytes());
                    t[2..4].copy_from_slice(&ordinal.to_le_bytes());
                    (1, t)
                }
                RTarget::ImportName { module, name } => {
                    let no = name_off(name, &mut imp_names);
                    let mut t = [0u8; 6];
                    t[0..2].copy_from_slice(&((*module + 1) as u16).to_le_bytes());
                    t[2..4].copy_from_slice(&no.to_le_bytes());
                    (2, t)
                }
            };
            let at = r.offset as usize;
            let additive = r.add != 0 || r.kind == RelocKind::LoByte;
            if additive {
                if r.kind != RelocKind::LoByte {
                    data[at..at + 2].copy_from_slice(&(r.add as u16).to_le_bytes());
                }
                let mut rec = [0u8; 8];
                rec[0] = ty;
                rec[1] = flags | 4;
                rec[2..4].copy_from_slice(&(r.offset as u16).to_le_bytes());
                rec[4..8].copy_from_slice(&tgt[0..4]);
                recs.push(rec);
                continue;
            }
            match chains.get(&(ty, tgt)) {
                Some(&i) => {
                    // Link this location in front of the chain.
                    let head = u16::from_le_bytes([recs[i][2], recs[i][3]]);
                    data[at..at + 2].copy_from_slice(&head.to_le_bytes());
                    recs[i][2..4].copy_from_slice(&(r.offset as u16).to_le_bytes());
                }
                None => {
                    data[at..at + 2].copy_from_slice(&0xffffu16.to_le_bytes());
                    let mut rec = [0u8; 8];
                    rec[0] = ty;
                    rec[1] = flags;
                    rec[2..4].copy_from_slice(&(r.offset as u16).to_le_bytes());
                    rec[4..8].copy_from_slice(&tgt[0..4]);
                    chains.insert((ty, tgt), recs.len());
                    recs.push(rec);
                }
            }
        }
        // The file holds the data up to the last non-zero byte.
        while data.last() == Some(&0) {
            data.pop();
        }
        // Internal references first, then imports.
        recs.sort_by_key(|r| (r[1] & 3) != 0);
        // /EXEPACK: iterated data when it is smaller.
        let mut it = false;
        if l.opt.exepack > 0 && !data.is_empty() {
            let p = crate::pack::iterate(&data);
            if p.len() < data.len() {
                data = p;
                it = true;
            }
        }
        iterated.push(it);
        seg_data.push(data);
        seg_relocs.push(recs);
    }
    // Entry table.
    let mut exports = o.exports.clone();
    exports.sort_by_key(|e| e.ordinal);
    let mut entry: Vec<u8> = Vec::new();
    let mut movable = 0u16;
    {
        let mut i = 0;
        let mut next_ord = 1u16;
        while i < exports.len() {
            let e = &exports[i];
            if e.ordinal > next_ord {
                // Unused ordinals.
                let mut gap = e.ordinal - next_ord;
                while gap > 0 {
                    let n = gap.min(255);
                    entry.push(n as u8);
                    entry.push(0);
                    gap -= n;
                }
                next_ord = e.ordinal;
            }
            // A bundle of consecutive entries of the same kind.
            let kind = |e: &crate::link::ExportEnt| -> u8 {
                match (e.at, e.absolute) {
                    (Some((u, _)), _) => (u + 1) as u8,
                    (None, Some(_)) => 0xfe,
                    _ => 0xfe,
                }
            };
            let k = kind(e);
            let mut j = i;
            while j < exports.len() && j - i < 255 && exports[j].ordinal == next_ord + (j - i) as u16 && kind(&exports[j]) == k {
                j += 1;
            }
            entry.push((j - i) as u8);
            entry.push(k);
            for e in &exports[i..j] {
                let mut fl = 1u8;
                if !e.nodata && dll {
                    fl |= 2;
                }
                fl |= (e.pwords & 0x1f) << 3;
                entry.push(fl);
                let off = e.at.map(|a| a.1).or(e.absolute).unwrap_or(0) as u16;
                put16(&mut entry, off);
            }
            next_ord += (j - i) as u16;
            i = j;
        }
        entry.push(0);
        if exports.is_empty() {
            entry.push(0);
        }
        let _ = &mut movable;
    }
    // Resident and non-resident names.
    let mut resident: Vec<u8> = Vec::new();
    pstr(&mut resident, &o.module_name);
    put16(&mut resident, 0);
    let mut nonres: Vec<u8> = Vec::new();
    let desc = if o.description.is_empty() { o.module_name.clone() } else { o.description.clone() };
    if !o.description.is_empty() || true {
        pstr(&mut nonres, &desc);
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
    // Segment flags.
    let mut segflags = Vec::new();
    for (i, u) in units.iter().enumerate() {
        let a = &u.attrs;
        let mut f = if a.moveable == Some(true) { sf::MOVEABLE } else { 0 };
        if !u.code {
            f |= sf::DATA;
        }
        if a.preload == Some(true) {
            f |= sf::PRELOAD;
        }
        if a.readonly == Some(true) {
            f |= sf::READONLY;
        }
        if a.conforming == Some(true) && u.code {
            f |= sf::CONFORMING;
        }
        f |= if a.iopl == Some(true) { sf::IOPL } else { sf::DPL3 };
        let shared = if u.code {
            a.shared == Some(true)
        } else if Some(i) == o.autodata {
            // The automatic data segment of a DLL with DATA SINGLE is
            // shared; programs get an instance each.
            dll && a.instance != Some(2) && a.shared != Some(false)
        } else {
            a.shared == Some(true)
        };
        if shared {
            f |= sf::SHARED;
        }
        if a.discardable == Some(true) {
            f |= sf::DISCARDABLE;
        }
        if !seg_relocs[i].is_empty() {
            f |= sf::RELOCS;
        }
        if u.use32 {
            f |= sf::BIG;
        }
        if iterated[i] {
            f |= sf::ITERATED;
        }
        segflags.push(f);
    }
    // Module flags.
    let mut flags: u16 = 0;
    let autodata = o.autodata.map(|u| u as u16 + 1).unwrap_or(0);
    if autodata != 0 {
        let inst = def.data.instance;
        flags |= match inst {
            Some(0) => 0,
            Some(1) => 1,
            Some(2) => 2,
            _ => {
                if dll { 1 } else { 2 }
            }
        };
    }
    if dll && def.init_instance == Some(true) {
        flags |= 0x0004;
    }
    if def.protmode {
        flags |= 0x0008;
    }
    let app = l.opt.pmtype.unwrap_or(def.app);
    flags |= match app {
        AppType::NoVio => 0x0100,
        AppType::Vio => 0x0200,
        AppType::Pm => 0x0300,
        AppType::Unspecified => 0,
    };
    if dll {
        flags |= 0x8000;
    }
    let errors = l.msgs.errors > 0 || o.errors > 0;
    if errors {
        flags |= 0x2000;
    }
    // Stack: the STACK segment if any, else the stack size on top of DGROUP.
    let mut stack = l.opt.stack.or(def.stacksize).unwrap_or(0);
    let heap = def.heapsize.unwrap_or(0);
    let (mut ss, mut sp) = (0u16, 0u16);
    if !dll {
        let stack_seg = o.lsegs.iter().find(|s| s.combine == crate::omf::Combine::Stack && s.absolute.is_none());
        if let Some(s) = stack_seg {
            let su = s.unit;
            // Relative to the frame: the group if the segment is in one.
            ss = su as u16 + 1;
            sp = (s.offset + s.size) as u16;
            if l.opt.stack.or(def.stacksize).is_none() {
                stack = 0;
            }
            let _ = su;
        } else if autodata != 0 {
            ss = autodata;
            sp = 0;
        } else if stack == 0 {
            l.msgs.warn(l.host, "", 4021, "no stack segment");
        }
    }
    if autodata != 0 && !dll {
        let au = o.autodata.unwrap();
        let total = units[au].size as u64 + stack as u64 + heap as u64;
        if total > 0x10000 && sp == 0 {
            l.msgs.error(l.host, "", 2041, "the stack plus the data are larger than 64K");
        }
    }
    let (cs, ip) = match o.start {
        Some((u, off)) => (u as u16 + 1, off as u16),
        None => {
            if !dll {
                l.msgs.warn(l.host, "", 4038, "the program has no starting address");
            }
            (0, 0)
        }
    };
    // Layout of the header tables.
    let stub = match (&def.stub, def.stub_none) {
        (Some(s), _) => load_stub(l, s)?,
        (None, true) => Vec::new(),
        (None, false) => dos_stub(),
    };
    let hdr = stub.len();
    let seg_table = 0x40usize;
    let res_table = seg_table + 8 * units.len();
    let resn = res_table; // no resources
    let modref_off = resn + resident.len();
    let impn = modref_off + 2 * modref.len();
    let entry_off = impn + imp_names.len();
    let nonres_off = hdr + entry_off + entry.len();
    let mut file_end = nonres_off + nonres.len();
    let align = 1usize << shift;
    // Segment data positions.
    let mut seg_pos = Vec::new();
    for (i, d) in seg_data.iter().enumerate() {
        if d.is_empty() && seg_relocs[i].is_empty() {
            seg_pos.push(0usize);
            continue;
        }
        file_end = file_end.next_multiple_of(align);
        seg_pos.push(file_end);
        file_end += d.len();
        if !seg_relocs[i].is_empty() {
            file_end += 2 + 8 * seg_relocs[i].len();
        }
    }
    let mut f: Vec<u8> = stub;
    if f.len() >= 0x40 {
        f[0x3c..0x40].copy_from_slice(&(hdr as u32).to_le_bytes());
    }
    let mut h = Vec::new();
    h.extend_from_slice(b"NE");
    // Linker version 5.10, as the format's other linkers write it.
    h.push(5);
    h.push(10);
    put16(&mut h, entry_off as u16);
    put16(&mut h, entry.len() as u16);
    put32(&mut h, 0);
    put16(&mut h, flags);
    put16(&mut h, autodata);
    put16(&mut h, heap as u16);
    put16(&mut h, stack as u16);
    put16(&mut h, ip);
    put16(&mut h, cs);
    put16(&mut h, sp);
    put16(&mut h, ss);
    put16(&mut h, units.len() as u16);
    put16(&mut h, modref.len() as u16);
    put16(&mut h, nonres.len() as u16);
    put16(&mut h, seg_table as u16);
    put16(&mut h, res_table as u16);
    put16(&mut h, resn as u16);
    put16(&mut h, modref_off as u16);
    put16(&mut h, impn as u16);
    put32(&mut h, nonres_off as u32);
    put16(&mut h, movable);
    put16(&mut h, shift as u16);
    put16(&mut h, 0);
    h.push(1);
    h.push(0);
    put16(&mut h, 0);
    put16(&mut h, 0);
    put16(&mut h, 0);
    put16(&mut h, 0);
    debug_assert_eq!(h.len(), 0x40);
    for (i, u) in units.iter().enumerate() {
        put16(&mut h, (seg_pos[i] >> shift) as u16);
        put16(&mut h, seg_data[i].len() as u16);
        put16(&mut h, segflags[i]);
        put16(&mut h, if u.size >= 0x10000 { 0 } else { u.size as u16 });
    }
    h.extend_from_slice(&resident);
    for m in &modref {
        put16(&mut h, *m);
    }
    h.extend_from_slice(&imp_names);
    h.extend_from_slice(&entry);
    f.extend_from_slice(&h);
    f.extend_from_slice(&nonres);
    for (i, d) in seg_data.iter().enumerate() {
        if seg_pos[i] == 0 {
            continue;
        }
        f.resize(seg_pos[i], 0);
        f.extend_from_slice(d);
        if !seg_relocs[i].is_empty() {
            put16(&mut f, seg_relocs[i].len() as u16);
            for r in &seg_relocs[i] {
                f.extend_from_slice(r);
            }
        }
    }
    let _ = (Kind::Exe, ImpEntry::Ordinal(0));
    // The DOS header gives the whole file's size, as IBM's linker writes it.
    if f.len() >= 0x40 && def.stub.is_none() {
        let n = f.len();
        f[2..4].copy_from_slice(&((n % 512) as u16).to_le_bytes());
        f[4..6].copy_from_slice(&(n.div_ceil(512) as u16).to_le_bytes());
    }
    // The checksum: the sum of the file's dwords (this field as zero).
    let mut sum = 0u32;
    for c in f.chunks(4) {
        let mut b = [0u8; 4];
        b[..c.len()].copy_from_slice(c);
        sum = sum.wrapping_add(u32::from_le_bytes(b));
    }
    f[hdr + 8..hdr + 12].copy_from_slice(&sum.to_le_bytes());
    Some(f)
}
