//! Print a PE module's headers, sections, imports and exports:
//! `cargo run -p os2-exe --example peinfo -- FILE.EXE`.

use os2_exe::pe;

fn main() {
    let path = std::env::args().nth(1).expect("usage: peinfo FILE");
    let data = std::fs::read(&path).expect("read");
    let Ok(os2_exe::Format::Pe { header }) = os2_exe::detect(&data) else { panic!("not a PE file") };
    let m = pe::Module::parse(&data, header).expect("parse");
    let h = &m.header;
    println!("base {:#x} entry {:#x} size {:#x} subsystem {} chars {:#x} stack {:#x}/{:#x}", h.image_base, h.entry, h.image_size, h.subsystem, h.characteristics, h.stack_reserve, h.stack_commit);
    for s in &m.sections {
        println!("  {:8} rva {:#07x} size {:#07x} raw {:#07x} {:#010x}", String::from_utf8_lossy(s.name()), s.rva, s.mem_size(), s.raw_size, s.characteristics);
    }
    for i in m.imports().expect("imports") {
        let names: Vec<String> = i.entries.iter().map(|(_, b)| match b {
            pe::ImportBy::Ordinal(o) => format!("#{o}"),
            pe::ImportBy::Name { name, .. } => String::from_utf8_lossy(name).into_owned(),
        }).collect();
        println!("{} ({}): {}", String::from_utf8_lossy(i.dll), names.len(), names.join(" "));
    }
    let e = m.exports().expect("exports");
    if !e.entries.is_empty() {
        println!("exports of {} (base {}): {} entries, {} names", String::from_utf8_lossy(e.name), e.base, e.entries.len(), e.names.len());
    }
    println!("{} relocations, tls {:?}", m.relocations().expect("relocs").len(), m.tls().expect("tls"));
}
