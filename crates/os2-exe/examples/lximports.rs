//! List the procedures an LX module imports: `MODULE ordinal` or
//! `MODULE name`, one per line, sorted and without duplicates.
//!
//! `cargo run -p os2-exe --example lximports -- FILE...`

use os2_exe::lx::{Module, Target};
use std::collections::BTreeSet;

fn main() {
    let mut out = BTreeSet::new();
    for f in std::env::args().skip(1) {
        let data = std::fs::read(&f).expect("read module");
        let off = if data.starts_with(b"MZ") { u32::from_le_bytes(data[0x3c..0x40].try_into().unwrap()) as usize } else { 0 };
        let Ok(m) = Module::parse(&data, off) else { continue };
        let mods = m.import_modules().unwrap_or_default();
        let name = |i: u16| String::from_utf8_lossy(mods.get(i as usize - 1).copied().unwrap_or(b"?")).to_uppercase();
        for o in m.objects().unwrap_or_default() {
            for p in o.page_index..o.page_index + o.page_count {
                for fx in m.fixups(p).unwrap_or_default() {
                    match fx.target {
                        Target::ImportOrdinal { module, ordinal } => {
                            out.insert(format!("{} {}", name(module), ordinal));
                        }
                        Target::ImportName { module, name_offset } => {
                            let n = m.import_proc_name(name_offset).unwrap_or(b"?");
                            out.insert(format!("{} {}", name(module), String::from_utf8_lossy(n)));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    for l in out {
        println!("{l}");
    }
}
