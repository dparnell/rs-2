//! Inspect OS/2 executables.
//!
//! ```text
//! lxdump info FILE          header, objects, imports, exports
//! lxdump imports FILE...    one line per imported symbol: MODULE.ORD or MODULE.NAME
//! lxdump verify FILE...     decode every page and fixup, report failures
//! lxdump page FILE N        hex dump of decoded LX page N
//! lxdump fixups FILE N      fixup records of LX page N
//! ```

use std::collections::BTreeSet;
use std::process::ExitCode;

use os2_exe::{Format, lx, ne};

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("usage: lxdump info|imports|verify|page FILE...");
        return ExitCode::FAILURE;
    };
    let mut status = ExitCode::SUCCESS;
    match cmd.as_str() {
        "page" => {
            let data = std::fs::read(&args[1]).unwrap();
            let n: u32 = args[2].parse().unwrap();
            let Ok(Format::Lx { header }) = os2_exe::detect(&data) else { panic!("not LX") };
            let m = lx::Module::parse(&data, header).unwrap();
            let mut buf = vec![0; m.header.page_size as usize];
            m.load_page(n, &mut buf).unwrap();
            for (i, c) in buf.chunks(16).enumerate() {
                let hex: Vec<String> = c.iter().map(|b| format!("{b:02x}")).collect();
                println!("{:04x}: {}", i * 16, hex.join(" "));
            }
        }
        "res" => {
            // lxdump res FILE TYPE ID: hex dump of one resource.
            let data = std::fs::read(&args[1]).unwrap();
            let (ty, id): (u16, u16) = (args[2].parse().unwrap(), args[3].parse().unwrap());
            let Ok(Format::Lx { header }) = os2_exe::detect(&data) else { panic!("not LX") };
            let m = lx::Module::parse(&data, header).unwrap();
            let objs = m.objects().unwrap();
            let Some(r) = m.resources().unwrap().into_iter().find(|r| r.type_id == ty && r.name_id == id) else {
                eprintln!("no resource {ty}.{id}");
                return ExitCode::FAILURE;
            };
            let o = &objs[r.object as usize - 1];
            let mut bytes = Vec::new();
            let mut buf = vec![0; m.header.page_size as usize];
            for p in 0..o.page_count {
                m.load_page(o.page_index + p, &mut buf).unwrap();
                bytes.extend_from_slice(&buf);
            }
            let res = &bytes[r.offset as usize..(r.offset + r.size) as usize];
            for (i, c) in res.chunks(16).enumerate() {
                let hex: Vec<String> = c.iter().map(|b| format!("{b:02x}")).collect();
                let asc: String = c.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
                println!("{:04x}: {:<48} {asc}", i * 16, hex.join(" "));
            }
        }
        "fixups" => {
            let data = std::fs::read(&args[1]).unwrap();
            let n: u32 = args[2].parse().unwrap();
            let Ok(Format::Lx { header }) = os2_exe::detect(&data) else { panic!("not LX") };
            let m = lx::Module::parse(&data, header).unwrap();
            for f in m.fixups(n).unwrap() {
                let srcs: Vec<String> = f.sources.iter().map(|s| format!("{s:#x}")).collect();
                println!(
                    "{} {:?}{} -> {:?} +{:#x}",
                    srcs.join(","),
                    f.kind,
                    if f.alias { " alias" } else { "" },
                    f.target,
                    f.additive
                );
            }
        }
        "xref" => {
            // lxdump xref FILE MODULE ORDINAL: fixup sites importing it.
            let data = std::fs::read(&args[1]).unwrap();
            let want_mod = args[2].to_ascii_uppercase();
            let want_ord: u32 = args[3].parse().unwrap();
            let Ok(Format::Lx { header }) = os2_exe::detect(&data) else { panic!("not LX") };
            let m = lx::Module::parse(&data, header).unwrap();
            for (oi, o) in m.objects().unwrap().iter().enumerate() {
                for p in 0..o.page_count {
                    for f in m.fixups(o.page_index + p).unwrap() {
                        if let lx::Target::ImportOrdinal { module, ordinal } = f.target {
                            if ordinal == want_ord && s(m.import_module(module).unwrap()).to_ascii_uppercase() == want_mod {
                                for src in &f.sources {
                                    println!("obj {} offset {:#x} {:?}", oi + 1, p as i64 * 4096 + *src as i64, f.kind);
                                }
                            }
                        }
                    }
                }
            }
        }
        "fixups" => {
            // lxdump fixups FILE OBJ: every fixup of an object.
            let data = std::fs::read(&args[1]).unwrap();
            let want: usize = args[2].parse().unwrap();
            let Ok(Format::Lx { header }) = os2_exe::detect(&data) else { panic!("not LX") };
            let m = lx::Module::parse(&data, header).unwrap();
            let o = m.objects().unwrap()[want - 1];
            for p in 0..o.page_count {
                for f in m.fixups(o.page_index + p).unwrap() {
                    let t = match f.target {
                        lx::Target::ImportOrdinal { module, ordinal } => {
                            format!("{}.{}", s(m.import_module(module).unwrap()), ordinal)
                        }
                        lx::Target::ImportName { module, name_offset } => format!(
                            "{}.{}",
                            s(m.import_module(module).unwrap()),
                            s(m.import_proc_name(name_offset).unwrap())
                        ),
                        t => format!("{t:?}"),
                    };
                    for src in &f.sources {
                        println!("{:#06x} {:?} {}", p as i64 * 4096 + *src as i64, f.kind, t);
                    }
                }
            }
        }
        "object" => {
            // lxdump object FILE N OUT: write the decoded image of object N.
            let data = std::fs::read(&args[1]).unwrap();
            let n: usize = args[2].parse().unwrap();
            let Ok(Format::Lx { header }) = os2_exe::detect(&data) else { panic!("not LX") };
            let m = lx::Module::parse(&data, header).unwrap();
            let o = m.objects().unwrap()[n - 1];
            let ps = m.header.page_size as usize;
            let mut img = vec![0u8; (o.virtual_size as usize).max(o.page_count as usize * ps)];
            for i in 0..o.page_count {
                let at = i as usize * ps;
                m.load_page(o.page_index + i, &mut img[at..at + ps]).unwrap();
            }
            img.truncate(o.virtual_size as usize);
            std::fs::write(&args[3], &img).unwrap();
            println!("object {n} base {:#x} size {:#x} flags {:#x}", o.base, o.virtual_size, o.flags);
        }
        _ => {
            for path in &args[1..] {
                let Ok(data) = std::fs::read(path) else { continue };
                let r = match cmd.as_str() {
                    "info" => info(&data),
                    "imports" => imports(&data, path),
                    "verify" => verify(&data),
                    _ => Err(format!("unknown command {cmd}")),
                };
                if let Err(e) = r {
                    eprintln!("{path}: {e}");
                    status = ExitCode::FAILURE;
                }
            }
        }
    }
    status
}

fn info(data: &[u8]) -> Result<(), String> {
    match os2_exe::detect(data).map_err(|e| e.to_string())? {
        Format::Lx { header } => {
            let m = lx::Module::parse(data, header).map_err(|e| e.to_string())?;
            let h = &m.header;
            println!("LX module {}", s(m.name().unwrap_or(b"?")));
            println!("  flags {:#010x} library={}", h.module_flags, h.is_library());
            println!("  entry obj {} eip {:#x}  stack obj {} esp {:#x}", h.eip_object, h.eip, h.esp_object, h.esp);
            println!("  pages {} page size {:#x} shift {}", h.page_count, h.page_size, h.page_shift);
            println!("  autodata {} heap {:#x} stack {:#x}", h.autodata_object, h.heap_size, h.stack_size);
            for (i, o) in m.objects().map_err(|e| e.to_string())?.iter().enumerate() {
                println!(
                    "  obj {} base {:#010x} size {:#x} flags {:#06x} pages {}+{}",
                    i + 1,
                    o.base,
                    o.virtual_size,
                    o.flags,
                    o.page_index,
                    o.page_count
                );
            }
            for (i, n) in m.import_modules().map_err(|e| e.to_string())?.iter().enumerate() {
                println!("  import {} {}", i + 1, s(n));
            }
            let names = m.exported_names().map_err(|e| e.to_string())?;
            for (i, e) in m.entries().map_err(|e| e.to_string())?.iter().enumerate() {
                if *e == lx::Entry::Unused {
                    continue;
                }
                let ord = (i + 1) as u16;
                let name = names.iter().find(|n| n.1 == ord).map(|n| s(n.0)).unwrap_or_default();
                println!("  export @{ord} {name} {e:?}");
            }
            for r in m.resources().map_err(|e| e.to_string())? {
                println!("  resource {r:?}");
            }
        }
        Format::Ne { header } => {
            let m = ne::Module::parse(data, header).map_err(|e| e.to_string())?;
            let h = &m.header;
            println!("NE module {}", s(m.name().unwrap_or(b"?")));
            println!("  flags {:#06x} cs:ip {}:{:#x} ss:sp {}:{:#x}", h.flags, h.cs, h.ip, h.ss, h.sp);
            println!("  autodata {} heap {:#x} stack {:#x}", h.autodata, h.heap, h.stack);
            for (i, sg) in m.segments().map_err(|e| e.to_string())?.iter().enumerate() {
                println!(
                    "  seg {} file {:#x}+{:#x} flags {:#06x} alloc {:#x}",
                    i + 1,
                    sg.file_offset,
                    sg.file_size,
                    sg.flags,
                    sg.min_alloc
                );
            }
            for (i, n) in m.import_modules().map_err(|e| e.to_string())?.iter().enumerate() {
                println!("  import {} {}", i + 1, s(n));
            }
            let names = m.exported_names().map_err(|e| e.to_string())?;
            for (i, e) in m.entries().map_err(|e| e.to_string())?.iter().enumerate() {
                if *e == ne::Entry::Unused {
                    continue;
                }
                let ord = (i + 1) as u16;
                let name = names.iter().find(|n| n.1 == ord).map(|n| s(n.0)).unwrap_or_default();
                println!("  export @{ord} {name} {e:?}");
            }
        }
        f => println!("{f:?}"),
    }
    Ok(())
}

fn imports(data: &[u8], path: &str) -> Result<(), String> {
    let mut set = BTreeSet::new();
    match os2_exe::detect(data).map_err(|e| e.to_string())? {
        Format::Lx { header } => {
            let m = lx::Module::parse(data, header).map_err(|e| e.to_string())?;
            for p in 1..=m.header.page_count {
                for f in m.fixups(p).map_err(|e| e.to_string())? {
                    match f.target {
                        lx::Target::ImportOrdinal { module, ordinal } => {
                            set.insert(format!("{}.{}", s(m.import_module(module).unwrap_or(b"?")), ordinal));
                        }
                        lx::Target::ImportName { module, name_offset } => {
                            set.insert(format!(
                                "{}.{}",
                                s(m.import_module(module).unwrap_or(b"?")),
                                s(m.import_proc_name(name_offset).unwrap_or(b"?"))
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
        Format::Ne { header } => {
            let m = ne::Module::parse(data, header).map_err(|e| e.to_string())?;
            for sg in m.segments().map_err(|e| e.to_string())? {
                for r in m.relocs(&sg).map_err(|e| e.to_string())? {
                    match r.target {
                        ne::Target::ImportOrdinal { module, ordinal } => {
                            set.insert(format!("{}.{}", s(m.import_module(module).unwrap_or(b"?")), ordinal));
                        }
                        ne::Target::ImportName { module, name_offset } => {
                            set.insert(format!(
                                "{}.{}",
                                s(m.import_module(module).unwrap_or(b"?")),
                                s(m.imported_name(name_offset).unwrap_or(b"?"))
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => return Ok(()),
    }
    for i in set {
        println!("{path}\t{i}");
    }
    Ok(())
}

fn verify(data: &[u8]) -> Result<(), String> {
    let Ok(Format::Lx { header }) = os2_exe::detect(data) else {
        return Ok(());
    };
    let m = lx::Module::parse(data, header).map_err(|e| e.to_string())?;
    let mut buf = vec![0; m.header.page_size as usize];
    for p in 1..=m.header.page_count {
        let page = m.page(p).map_err(|e| e.to_string())?;
        m.load_page(p, &mut buf).map_err(|e| format!("page {p} {:?}: {e}", page.kind))?;
        m.fixups(p).map_err(|e| format!("fixups page {p}: {e}"))?;
    }
    m.entries().map_err(|e| e.to_string())?;
    Ok(())
}
