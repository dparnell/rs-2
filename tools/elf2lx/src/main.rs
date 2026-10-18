//! Convert an i686 ELF executable into an OS/2 LX module.
//!
//! ```text
//! elf2lx INPUT.elf OUTPUT [--dll] [--name NAME] [--stack BYTES] [--exports FILE] [--pm]
//! ```
//!
//! The ELF must be statically linked with `--emit-relocs` (so relocation
//! records survive) and `--unresolved-symbols=ignore-all`. Undefined symbols
//! name OS/2 imports: `MODULE.123` imports by ordinal, `MODULE.Name` by name.
//!
//! Allocated sections become LX objects: one read/execute object for code
//! and read-only data, one read/write object for data and bss (each starts on
//! a 64 KB boundary in the link). Executables also get a stack object.
//! The exports file lists `NAME [@ordinal]` per line (DLLs).

use std::collections::BTreeMap;
use std::process::ExitCode;

const PAGE: u32 = 4096;

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

struct Section {
    name: String,
    ty: u32,
    flags: u32,
    addr: u32,
    offset: u32,
    size: u32,
    link: u32,
    info: u32,
    entsize: u32,
}

struct Sym {
    name: String,
    value: u32,
    shndx: u16,
    bind: u8,
}

struct Elf {
    data: Vec<u8>,
    entry: u32,
    sections: Vec<Section>,
    symbols: Vec<Sym>,
}

fn cstr(b: &[u8], o: usize) -> String {
    let end = b[o..].iter().position(|&c| c == 0).map_or(b.len(), |e| o + e);
    String::from_utf8_lossy(&b[o..end]).into_owned()
}

fn parse(data: Vec<u8>) -> Elf {
    assert!(&data[0..4] == b"\x7fELF" && data[4] == 1 && data[5] == 1, "not a 32-bit little-endian ELF");
    let entry = u32le(&data, 24);
    let shoff = u32le(&data, 32) as usize;
    let shentsize = u16le(&data, 46) as usize;
    let shnum = u16le(&data, 48) as usize;
    let shstrndx = u16le(&data, 50) as usize;
    let mut sections = Vec::new();
    for i in 0..shnum {
        let o = shoff + i * shentsize;
        sections.push(Section {
            name: String::new(),
            ty: u32le(&data, o + 4),
            flags: u32le(&data, o + 8),
            addr: u32le(&data, o + 12),
            offset: u32le(&data, o + 16),
            size: u32le(&data, o + 20),
            link: u32le(&data, o + 24),
            info: u32le(&data, o + 28),
            entsize: u32le(&data, o + 36),
        });
        let _ = u32le(&data, o);
    }
    let strtab_off = sections[shstrndx].offset as usize;
    for i in 0..shnum {
        let name_off = u32le(&data, shoff + i * shentsize) as usize;
        sections[i].name = cstr(&data, strtab_off + name_off);
    }
    let mut symbols = Vec::new();
    if let Some(st) = sections.iter().find(|s| s.ty == 2) {
        let strs = sections[st.link as usize].offset as usize;
        let n = st.size / 16;
        for i in 0..n as usize {
            let o = st.offset as usize + i * 16;
            symbols.push(Sym {
                name: cstr(&data, strs + u32le(&data, o) as usize),
                value: u32le(&data, o + 4),
                shndx: u16le(&data, o + 14),
                bind: data[o + 12] >> 4,
            });
        }
    }
    Elf { data, entry, sections, symbols }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Offset32,
    Relative32,
}

#[derive(Clone)]
enum Target {
    Internal { object: u16, offset: u32 },
    ImportOrdinal { module: u16, ordinal: u32 },
    ImportName { module: u16, name_off: u32 },
}

struct Fixup {
    /// Linear address of the fixed-up location.
    at: u32,
    kind: Kind,
    target: Target,
    additive: u32,
}

struct Object {
    base: u32,
    size: u32,
    flags: u32,
    data: Vec<u8>,
}

fn object_in(objects: &[Object], addr: u32) -> Option<usize> {
    objects.iter().position(|o| addr >= o.base && addr < o.base + o.size.max(1))
}

const OBJ_READ: u32 = 0x0001;
const OBJ_WRITE: u32 = 0x0002;
const OBJ_EXEC: u32 = 0x0004;
const OBJ_SHARED: u32 = 0x0010;
const OBJ_BIG: u32 = 0x2000;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: elf2lx INPUT OUTPUT [--dll] [--name NAME] [--stack BYTES] [--exports FILE] [--pm]");
        return ExitCode::FAILURE;
    }
    let mut dll = false;
    let mut pm = false;
    let mut name = std::path::Path::new(&args[1]).file_stem().unwrap().to_string_lossy().to_ascii_uppercase();
    let mut stack = 0x10000u32;
    let mut exports_file = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--dll" => dll = true,
            "--pm" => pm = true,
            "--name" => {
                i += 1;
                name = args[i].to_ascii_uppercase();
            }
            "--stack" => {
                i += 1;
                stack = args[i].parse().unwrap();
            }
            "--exports" => {
                i += 1;
                exports_file = Some(args[i].clone());
            }
            _ => {}
        }
        i += 1;
    }
    let elf = parse(std::fs::read(&args[0]).expect("read input"));

    // Objects: gather allocated sections into code, data and shared data
    // (sections named .shared*, one copy for all processes of a DLL).
    let alloc: Vec<&Section> = elf.sections.iter().filter(|s| s.flags & 2 != 0 && s.size > 0).collect();
    let kind = |s: &Section| -> u8 {
        if s.name.starts_with(".shared") {
            2
        } else {
            (s.flags & 1 != 0) as u8
        }
    };
    let mut groups: Vec<(u8, u32, u32)> = Vec::new(); // (kind, start, end)
    for s in &alloc {
        let w = kind(s);
        match groups.iter_mut().find(|g| g.0 == w) {
            Some(g) => {
                g.1 = g.1.min(s.addr);
                g.2 = g.2.max(s.addr + s.size);
            }
            None => groups.push((w, s.addr, s.addr + s.size)),
        }
    }
    groups.sort_by_key(|g| g.1);
    let mut objects: Vec<Object> = Vec::new();
    for g in &groups {
        let base = g.1 & !0xffff;
        let size = g.2 - base;
        let mut data = vec![0u8; size as usize];
        for s in alloc.iter().filter(|s| kind(s) == g.0) {
            if s.ty != 8 {
                // not NOBITS
                let from = &elf.data[s.offset as usize..(s.offset + s.size) as usize];
                let at = (s.addr - base) as usize;
                data[at..at + from.len()].copy_from_slice(from);
            }
        }
        let flags = OBJ_READ | OBJ_BIG | match g.0 {
            0 => OBJ_EXEC,
            1 => OBJ_WRITE,
            _ => OBJ_WRITE | OBJ_SHARED,
        };
        objects.push(Object { base, size, flags, data });
    }

    // Imports.
    let mut modules: Vec<String> = Vec::new();
    let mut procs: Vec<u8> = vec![0]; // import procedure name table
    let mut proc_offsets: BTreeMap<String, u32> = BTreeMap::new();
    let mut import_target = |sym: &str| -> Option<Target> {
        let (m, p) = sym.split_once('.')?;
        let m = m.to_ascii_uppercase();
        let mi = match modules.iter().position(|x| *x == m) {
            Some(i) => i,
            None => {
                modules.push(m.clone());
                modules.len() - 1
            }
        } as u16
            + 1;
        if let Ok(ord) = p.parse::<u32>() {
            Some(Target::ImportOrdinal { module: mi, ordinal: ord })
        } else {
            let off = *proc_offsets.entry(p.to_string()).or_insert_with(|| {
                let off = procs.len() as u32;
                procs.push(p.len() as u8);
                procs.extend_from_slice(p.as_bytes());
                off
            });
            Some(Target::ImportName { module: mi, name_off: off })
        }
    };

    // Relocations.
    let mut fixups: Vec<Fixup> = Vec::new();
    let mut unknown = 0;
    for rs in elf.sections.iter().filter(|s| s.ty == 9) {
        let target_sec = &elf.sections[rs.info as usize];
        if target_sec.flags & 2 == 0 {
            continue; // relocations of non-allocated (debug) sections
        }
        let n = rs.size / rs.entsize.max(8);
        for k in 0..n {
            let o = (rs.offset + k * 8) as usize;
            let at = u32le(&elf.data, o);
            let info = u32le(&elf.data, o + 4);
            let symi = (info >> 8) as usize;
            let ty = info & 0xff;
            let Some(oi) = object_in(&objects, at) else { continue };
            let in_obj = (at - objects[oi].base) as usize;
            let cur = u32le(&objects[oi].data, in_obj);
            let sym = &elf.symbols[symi];
            let undefined = sym.shndx == 0 && symi != 0;
            match ty {
                1 => {
                    // R_386_32: S + A, already applied for defined symbols.
                    if undefined {
                        match import_target(&sym.name) {
                            Some(t) => fixups.push(Fixup { at, kind: Kind::Offset32, target: t, additive: cur }),
                            None => {
                                eprintln!("unresolved symbol {}", sym.name);
                                unknown += 1;
                            }
                        }
                    } else if let Some(to) = object_in(&objects, cur) {
                        let offset = cur - objects[to].base;
                        fixups.push(Fixup {
                            at,
                            kind: Kind::Offset32,
                            target: Target::Internal { object: to as u16 + 1, offset },
                            additive: 0,
                        });
                    }
                }
                // GOT-relative forms produced by position-independent
                // library code: constants once the image sits at its
                // linked base, which is where executables are loaded.
                3 | 9 | 10 => {}
                2 | 4 => {
                    // R_386_PC32 / R_386_PLT32: S + A - P.
                    if undefined {
                        match import_target(&sym.name) {
                            Some(t) => {
                                // The loader stores target - (P + 4).
                                let a = cur.wrapping_add(at);
                                let additive = a.wrapping_add(4);
                                fixups.push(Fixup { at, kind: Kind::Relative32, target: t, additive });
                            }
                            None => {
                                eprintln!("unresolved symbol {}", sym.name);
                                unknown += 1;
                            }
                        }
                    } else {
                        let dest = cur.wrapping_add(at).wrapping_add(4);
                        if let Some(to) = object_in(&objects, dest) {
                            if to != oi {
                                fixups.push(Fixup {
                                    at,
                                    kind: Kind::Relative32,
                                    target: Target::Internal { object: to as u16 + 1, offset: dest - objects[to].base },
                                    additive: 0,
                                });
                            }
                        }
                    }
                }
                0 => {}
                _ => {
                    eprintln!("unsupported relocation type {ty} at {at:#x}");
                    unknown += 1;
                }
            }
        }
    }
    if unknown > 0 {
        return ExitCode::FAILURE;
    }
    // GOT slots hold absolute addresses without relocation records.
    for got in elf.sections.iter().filter(|s| s.name == ".got" || s.name == ".got.plt") {
        for k in 0..got.size / 4 {
            let at = got.addr + k * 4;
            let Some(oi) = object_in(&objects, at) else { continue };
            let v = u32le(&objects[oi].data, (at - objects[oi].base) as usize);
            if let Some(to) = object_in(&objects, v) {
                fixups.push(Fixup {
                    at,
                    kind: Kind::Offset32,
                    target: Target::Internal { object: to as u16 + 1, offset: v - objects[to].base },
                    additive: 0,
                });
            }
        }
    }

    // Stack object for executables.
    let mut esp_obj = 0u32;
    let mut esp = 0u32;
    if !dll {
        let base = objects.last().map_or(0x10000, |o| (o.base + o.size + 0xffff) & !0xffff);
        objects.push(Object { base, size: stack, flags: OBJ_READ | OBJ_WRITE | OBJ_BIG, data: Vec::new() });
        esp_obj = objects.len() as u32;
        esp = stack;
    }

    // Exports.
    let mut exports: Vec<(String, u16, u16, u32)> = Vec::new(); // name, ordinal, object, offset
    if let Some(f) = exports_file {
        let text = std::fs::read_to_string(f).expect("exports file");
        let mut next_ord = 1u16;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with(';')) {
            let (n, ord) = match line.split_once('@') {
                Some((n, o)) => (n.trim().to_string(), o.trim().parse::<u16>().unwrap()),
                None => (line.to_string(), next_ord),
            };
            next_ord = ord + 1;
            // "Name=symbol": export a symbol under another name.
            let (n, symname) = match n.split_once('=') {
                Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                None => (n.clone(), n),
            };
            let sym = elf
                .symbols
                .iter()
                .find(|s| s.name == symname && s.shndx != 0 && s.bind != 0)
                .unwrap_or_else(|| panic!("export {symname} not found"));
            let oi = object_in(&objects, sym.value).expect("export outside objects");
            exports.push((n, ord, oi as u16 + 1, sym.value - objects[oi].base));
        }
    }
    exports.sort_by_key(|e| e.1);

    let lx = write_lx(&name, dll, pm, &elf, &objects, &fixups, &modules, &procs, &exports, esp_obj, esp);
    std::fs::write(&args[1], lx).expect("write output");
    ExitCode::SUCCESS
}

#[allow(clippy::too_many_arguments)]
fn write_lx(
    name: &str,
    dll: bool,
    pm: bool,
    elf: &Elf,
    objects: &[Object],
    fixups: &[Fixup],
    modules: &[String],
    procs: &[u8],
    exports: &[(String, u16, u16, u32)],
    esp_obj: u32,
    esp: u32,
) -> Vec<u8> {
    let object_of = |a: u32| object_in(objects, a);
    // Pages: every object's data split into 4 KB pages (zero-filled tail
    // pages and the stack have no data).
    struct PageRec {
        obj: usize,
        index: u32,
        data: Vec<u8>,
    }
    let mut pages: Vec<PageRec> = Vec::new();
    let mut page_index = Vec::new();
    for (oi, o) in objects.iter().enumerate() {
        page_index.push(pages.len() as u32 + 1);
        let n = (o.data.len() as u32).div_ceil(PAGE);
        for p in 0..n {
            let s = (p * PAGE) as usize;
            let e = (s + PAGE as usize).min(o.data.len());
            let mut d = o.data[s..e].to_vec();
            while d.last() == Some(&0) {
                d.pop();
            }
            pages.push(PageRec { obj: oi, index: p, data: d });
        }
    }

    // Fixup records per page.
    let mut per_page: Vec<Vec<u8>> = vec![Vec::new(); pages.len()];
    let mut emit = |page: usize, src: i16, f: &Fixup| {
        let mut r = Vec::new();
        let src_type: u8 = match f.kind {
            Kind::Offset32 => 0x07,
            Kind::Relative32 => 0x08,
        };
        let mut flags: u8 = 0;
        let mut tail = Vec::new();
        match &f.target {
            Target::Internal { object, offset } => {
                flags |= 0x00 | 0x10; // internal, 32-bit offset
                if *object > 255 {
                    flags |= 0x40;
                    tail.extend_from_slice(&object.to_le_bytes());
                } else {
                    tail.push(*object as u8);
                }
                tail.extend_from_slice(&offset.to_le_bytes());
            }
            Target::ImportOrdinal { module, ordinal } => {
                flags |= 0x01;
                if *module > 255 {
                    flags |= 0x40;
                    tail.extend_from_slice(&module.to_le_bytes());
                } else {
                    tail.push(*module as u8);
                }
                if *ordinal < 256 {
                    flags |= 0x80;
                    tail.push(*ordinal as u8);
                } else if *ordinal < 65536 {
                    tail.extend_from_slice(&(*ordinal as u16).to_le_bytes());
                } else {
                    flags |= 0x10;
                    tail.extend_from_slice(&ordinal.to_le_bytes());
                }
            }
            Target::ImportName { module, name_off } => {
                flags |= 0x02 | 0x10;
                tail.push(*module as u8);
                tail.extend_from_slice(&name_off.to_le_bytes());
            }
        }
        if f.additive != 0 {
            flags |= 0x04 | 0x20;
            tail.extend_from_slice(&f.additive.to_le_bytes());
        }
        r.push(src_type);
        r.push(flags);
        r.extend_from_slice(&src.to_le_bytes());
        r.extend_from_slice(&tail);
        per_page[page].extend_from_slice(&r);
    };
    for f in fixups {
        let oi = object_of(f.at).unwrap();
        let off = f.at - objects[oi].base;
        let p = (off / PAGE) as usize;
        let page = page_index[oi] as usize - 1 + p;
        emit(page, (off % PAGE) as i16, f);
        // A fixup crossing into the next page is repeated there.
        if off % PAGE > PAGE - 4 && page + 1 < pages.len() && pages[page + 1].obj == oi {
            emit(page + 1, ((off % PAGE) as i32 - PAGE as i32) as i16, f);
        }
    }

    // Layout of the loader section, relative to the LX header.
    let hdr_size = 0xc4usize;
    let mut out = Vec::new();
    let obj_table = hdr_size;
    let page_table = obj_table + objects.len() * 24;
    let resname = page_table + pages.len() * 8;
    let mut resnames = Vec::new();
    resnames.push(name.len() as u8);
    resnames.extend_from_slice(name.as_bytes());
    resnames.extend_from_slice(&0u16.to_le_bytes());
    for (n, ord, _, _) in exports {
        resnames.push(n.len() as u8);
        resnames.extend_from_slice(n.as_bytes());
        resnames.extend_from_slice(&ord.to_le_bytes());
    }
    resnames.push(0);
    let entry_table = resname + resnames.len();
    let mut entries = Vec::new();
    let mut ord = 1u16;
    for (_, eord, obj, off) in exports {
        if *eord > ord {
            // Skip unused ordinals.
            let mut gap = eord - ord;
            while gap > 0 {
                let n = gap.min(255);
                entries.push(n as u8);
                entries.push(0);
                gap -= n;
            }
            ord = *eord;
        }
        entries.push(1);
        entries.push(3); // 32-bit entry
        entries.extend_from_slice(&obj.to_le_bytes());
        entries.push(3); // exported, shared data
        entries.extend_from_slice(&off.to_le_bytes());
        ord += 1;
    }
    entries.push(0);
    let fixup_page_table = entry_table + entries.len();
    let fixup_records = fixup_page_table + (pages.len() + 1) * 4;
    let fixup_size: usize = per_page.iter().map(|v| v.len()).sum();
    let import_modules = fixup_records + fixup_size;
    let mut imods = Vec::new();
    for m in modules {
        imods.push(m.len() as u8);
        imods.extend_from_slice(m.as_bytes());
    }
    let import_procs = import_modules + imods.len();
    let loader_end = import_procs + procs.len();

    // MZ stub.
    let stub_size = 0x80usize;
    let mut mz = vec![0u8; stub_size];
    mz[0..2].copy_from_slice(b"MZ");
    mz[2..4].copy_from_slice(&(stub_size as u16).to_le_bytes());
    mz[4..6].copy_from_slice(&1u16.to_le_bytes());
    mz[8..10].copy_from_slice(&4u16.to_le_bytes());
    mz[0x18..0x1a].copy_from_slice(&0x40u16.to_le_bytes());
    mz[0x3c..0x40].copy_from_slice(&(stub_size as u32).to_le_bytes());
    out.extend_from_slice(&mz);

    let data_pages = (stub_size + loader_end).next_multiple_of(512);
    let w32 = |v: &mut Vec<u8>, o: usize, x: u32| v[o..o + 4].copy_from_slice(&x.to_le_bytes());
    let mut h = vec![0u8; hdr_size];
    h[0..2].copy_from_slice(b"LX");
    h[8..10].copy_from_slice(&2u16.to_le_bytes()); // cpu: 386
    h[10..12].copy_from_slice(&1u16.to_le_bytes()); // OS/2
    let mut mflags: u32 = if dll { 0x8000 } else { 0 };
    mflags |= if pm { 0x300 } else { 0x200 };
    if dll {
        mflags |= 0x4000_0004; // per-process init/term
    }
    w32(&mut h, 0x10, mflags);
    w32(&mut h, 0x14, pages.len() as u32);
    let entry_obj = object_of(elf.entry).map_or(0, |o| o as u32 + 1);
    w32(&mut h, 0x18, entry_obj);
    w32(&mut h, 0x1c, if entry_obj > 0 { elf.entry - objects[entry_obj as usize - 1].base } else { 0 });
    w32(&mut h, 0x20, esp_obj);
    w32(&mut h, 0x24, esp);
    w32(&mut h, 0x28, PAGE);
    w32(&mut h, 0x2c, 0);
    w32(&mut h, 0x30, (import_procs + procs.len() - fixup_page_table) as u32);
    w32(&mut h, 0x38, (loader_end - obj_table) as u32);
    w32(&mut h, 0x40, obj_table as u32);
    w32(&mut h, 0x44, objects.len() as u32);
    w32(&mut h, 0x48, page_table as u32);
    w32(&mut h, 0x58, resname as u32);
    w32(&mut h, 0x5c, entry_table as u32);
    w32(&mut h, 0x68, fixup_page_table as u32);
    w32(&mut h, 0x6c, fixup_records as u32);
    w32(&mut h, 0x70, import_modules as u32);
    w32(&mut h, 0x74, modules.len() as u32);
    w32(&mut h, 0x78, import_procs as u32);
    w32(&mut h, 0x80, data_pages as u32);
    let autodata = objects.iter().position(|o| o.flags & OBJ_WRITE != 0 && o.flags & OBJ_SHARED == 0 && !o.data.is_empty());
    w32(&mut h, 0x94, autodata.map_or(0, |a| a as u32 + 1));
    w32(&mut h, 0xac, if dll { 0 } else { esp });
    out.extend_from_slice(&h);

    for (oi, o) in objects.iter().enumerate() {
        let mut e = vec![0u8; 24];
        w32(&mut e, 0, o.size);
        w32(&mut e, 4, o.base);
        w32(&mut e, 8, o.flags);
        w32(&mut e, 12, page_index[oi]);
        w32(&mut e, 16, (o.data.len() as u32).div_ceil(PAGE));
        out.extend_from_slice(&e);
    }
    let mut file_off = 0u32;
    for p in &pages {
        let mut e = vec![0u8; 8];
        w32(&mut e, 0, file_off);
        e[4..6].copy_from_slice(&(p.data.len() as u16).to_le_bytes());
        e[6..8].copy_from_slice(&(if p.data.is_empty() { 3u16 } else { 0 }).to_le_bytes());
        out.extend_from_slice(&e);
        file_off += p.data.len() as u32;
        let _ = p.index;
    }
    out.extend_from_slice(&resnames);
    out.extend_from_slice(&entries);
    let mut acc = 0u32;
    for v in &per_page {
        out.extend_from_slice(&acc.to_le_bytes());
        acc += v.len() as u32;
    }
    out.extend_from_slice(&acc.to_le_bytes());
    for v in &per_page {
        out.extend_from_slice(v);
    }
    out.extend_from_slice(&imods);
    out.extend_from_slice(procs);
    out.resize(data_pages, 0);
    for p in &pages {
        out.extend_from_slice(&p.data);
    }
    out
}
