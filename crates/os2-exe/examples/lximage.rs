//! Lay out an LX module's objects as loaded, for disassemblers.
//!
//! `cargo run -p os2-exe --example lximage -- FILE OUTDIR [SYMFILE]`
//!
//! Writes OUTDIR/objN.bin for every object as loaded at its preferred
//! base, with internal fixups applied and every imported procedure given a
//! fake address in an import area after the last object; OUTDIR/layout.txt
//! (`file base code|data 16|32`, then `imports base size`); and
//! OUTDIR/symbols.txt, `address name` for exports, imports and, given a
//! MAPSYM .sym file, its symbols. Used by tools/ghidra/lxload.sh.

use os2_exe::lx::{Entry, Module, SourceKind, Target};
use std::collections::BTreeMap;
use std::fmt::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: lximage FILE OUTDIR [SYMFILE]");
        std::process::exit(2);
    }
    let data = std::fs::read(&args[0]).expect("read module");
    let lx_off = if data.starts_with(b"MZ") {
        u32::from_le_bytes(data[0x3c..0x40].try_into().unwrap()) as usize
    } else {
        0
    };
    let m = Module::parse(&data, lx_off).expect("LX module");
    let mut objects = m.objects().expect("object table");
    // Objects without a preferred base (resources) go after the others.
    let mut next = objects.iter().map(|o| (o.base + o.virtual_size + 0xffff) & !0xffff).max().unwrap_or(0x10000);
    for o in objects.iter_mut().filter(|o| o.base == 0) {
        o.base = next;
        next = (next + o.virtual_size + 0xffff) & !0xffff;
    }
    let page_size = m.header.page_size as usize;

    let lo = objects.iter().map(|o| o.base).min().unwrap_or(0x10000);
    let hi = objects.iter().map(|o| o.base + o.virtual_size.max(1)).max().unwrap_or(lo);
    let imports_base = (hi + 0xfff) & !0xfff;
    let mut imports: BTreeMap<String, u32> = BTreeMap::new();
    let modules = m.import_modules().unwrap_or_default();
    let import_name = |t: &Target| -> Option<String> {
        let module = |i: u16| String::from_utf8_lossy(modules[i as usize - 1]).into_owned();
        match *t {
            Target::ImportOrdinal { module: md, ordinal } => Some(format!("{}.{}", module(md), ordinal)),
            Target::ImportName { module: md, name_offset } => Some(format!(
                "{}.{}",
                module(md),
                String::from_utf8_lossy(m.import_proc_name(name_offset).unwrap_or(b"?"))
            )),
            _ => None,
        }
    };
    let entries = m.entries().unwrap_or_default();
    let entry_addr = |ord: u16| -> u32 {
        match entries.get(ord as usize - 1) {
            Some(Entry::Entry32 { object, offset, .. }) => objects[*object as usize - 1].base + offset,
            Some(Entry::Entry16 { object, offset, .. }) => {
                objects[*object as usize - 1].base + *offset as u32
            }
            _ => 0,
        }
    };

    let mut image = vec![0u8; (imports_base - lo) as usize];
    let mut info = String::new();
    for (i, o) in objects.iter().enumerate() {
        writeln!(
            info,
            "object {} base {:#x} size {:#x} flags {:#x} {}",
            i + 1,
            o.base,
            o.virtual_size,
            o.flags,
            if o.is_32bit() { "32-bit" } else { "16-bit" }
        )
        .unwrap();
        for p in 0..o.page_count {
            let page = o.page_index + p;
            let at = (o.base - lo) as usize + p as usize * page_size;
            let end = (at + page_size).min(image.len());
            let mut buf = vec![0u8; page_size];
            if m.load_page(page, &mut buf).is_err() {
                continue;
            }
            image[at..end].copy_from_slice(&buf[..end - at]);
        }
    }
    // Fixups, after all pages are in place.
    for o in &objects {
        for p in 0..o.page_count {
            let page_at = (o.base - lo) as i64 + p as i64 * page_size as i64;
            for f in m.fixups(o.page_index + p).unwrap_or_default() {
                if let (true, Target::Internal { object, offset }) = (f.chained, f.target) {
                    let start = page_at as usize;
                    let page = image[start..(start + page_size).min(image.len())].to_vec();
                    let base = objects[object as usize - 1].base;
                    for (s, off) in os2_exe::lx::fixup_chain(&page, f.sources[0], offset) {
                        let at = start + s as usize;
                        let v = base.wrapping_add(off).wrapping_add(f.additive);
                        image[at..at + 4].copy_from_slice(&v.to_le_bytes());
                    }
                    continue;
                }
                let value = match f.target {
                    Target::Internal { object, offset } => {
                        objects[object as usize - 1].base.wrapping_add(offset)
                    }
                    Target::InternalEntry { ordinal } => entry_addr(ordinal),
                    ref t => {
                        let name = import_name(t).unwrap();
                        let n = imports.len() as u32;
                        *imports.entry(name).or_insert(imports_base + n * 4)
                    }
                }
                .wrapping_add(f.additive);
                for &s in &f.sources {
                    let at = page_at + s as i64;
                    if at < 0 {
                        continue;
                    }
                    let at = at as usize;
                    let put = |img: &mut Vec<u8>, b: &[u8]| {
                        if at + b.len() <= img.len() {
                            img[at..at + b.len()].copy_from_slice(b);
                        }
                    };
                    match f.kind {
                        SourceKind::Offset32 => put(&mut image, &value.to_le_bytes()),
                        SourceKind::Relative32 => {
                            let next = lo.wrapping_add(at as u32 + 4);
                            put(&mut image, &value.wrapping_sub(next).to_le_bytes())
                        }
                        SourceKind::Offset16 => put(&mut image, &(value as u16).to_le_bytes()),
                        _ => {}
                    }
                }
            }
        }
    }

    for (i, e) in entries.iter().enumerate() {
        if matches!(e, Entry::Entry32 { .. } | Entry::Entry16 { .. }) {
            writeln!(info, "entry {} {:#010x}", i + 1, entry_addr(i as u16 + 1)).unwrap();
        }
        if let Entry::Forwarder { module, by_ordinal, value } = *e {
            let md = String::from_utf8_lossy(modules.get(module as usize - 1).copied().unwrap_or(b"?")).into_owned();
            let target = if by_ordinal {
                format!("{value}")
            } else {
                String::from_utf8_lossy(m.import_proc_name(value).unwrap_or(b"?")).into_owned()
            };
            writeln!(info, "forward {} {md}.{target}", i + 1).unwrap();
        }
    }
    for r in m.resources().unwrap_or_default() {
        let base = objects.get((r.object as usize).wrapping_sub(1)).map_or(0, |o| o.base);
        writeln!(info, "resource type {} id {} size {:#x} at {:#x} (object {})", r.type_id, r.name_id, r.size, base + r.offset, r.object).unwrap();
    }
    let mut syms = String::new();
    for (name, ord) in m.exported_names().unwrap_or_default() {
        let a = entry_addr(ord);
        writeln!(info, "export {ord} {}", String::from_utf8_lossy(name)).unwrap();
        if a != 0 {
            writeln!(syms, "{a:#010x} {}", String::from_utf8_lossy(name)).unwrap();
        }
    }
    for (name, a) in &imports {
        writeln!(syms, "{a:#010x} imp_{}", name.replace('.', "_")).unwrap();
    }
    if let Some(sym) = args.get(2) {
        let s = std::fs::read(sym).expect("read sym");
        for (seg, value, name) in mapsym(&s) {
            if let Some(o) = objects.get(seg) {
                writeln!(syms, "{:#010x} {name}", o.base + value).unwrap();
            }
        }
    }
    std::fs::create_dir_all(&args[1]).unwrap();
    let mut layout = String::new();
    for (i, o) in objects.iter().enumerate() {
        let at = (o.base - lo) as usize;
        let size = (o.virtual_size as usize + 0xfff) & !0xfff;
        let end = (at + size).min(image.len());
        std::fs::write(format!("{}/obj{}.bin", args[1], i + 1), &image[at..end]).unwrap();
        let kind = if o.flags & 4 != 0 { "code" } else { "data" };
        let bits = if o.is_32bit() { 32 } else { 16 };
        writeln!(layout, "obj{}.bin {:#x} {kind} {bits}", i + 1, o.base).unwrap();
    }
    writeln!(layout, "imports {imports_base:#x} {:#x}", imports.len() * 4).unwrap();
    std::fs::write(format!("{}/layout.txt", args[1]), layout).unwrap();
    std::fs::write(format!("{}/symbols.txt", args[1]), syms).unwrap();
    print!("{info}");
    println!("image base {lo:#x}, imports at {imports_base:#x} ({} imports)", imports.len());
}

/// Symbols of a MAPSYM .sym file: (segment index, value, name).
fn mapsym(s: &[u8]) -> Vec<(usize, u32, String)> {
    let u16_at = |o: usize| u16::from_le_bytes([s[o], s[o + 1]]) as usize;
    let mut out = Vec::new();
    let segs = u16_at(0x0a);
    let mut seg = u16_at(0x0c) * 16;
    for _ in 0..segs {
        if seg == 0 || seg + 0x16 > s.len() {
            break;
        }
        let count = u16_at(seg + 2);
        let table = seg + u16_at(seg + 4);
        let big = s[seg + 0x0e] & 1 != 0;
        let i = u16_at(seg + 6).saturating_sub(1);
        for k in 0..count {
            let p = seg + u16_at(table + k * 2);
            let (value, at) = if big {
                (u32::from_le_bytes(s[p..p + 4].try_into().unwrap()), p + 4)
            } else {
                (u16_at(p) as u32, p + 2)
            };
            let len = s[at] as usize;
            out.push((i, value, String::from_utf8_lossy(&s[at + 1..at + 1 + len]).into_owned()));
        }
        seg = u16_at(seg) * 16;
    }
    out
}
