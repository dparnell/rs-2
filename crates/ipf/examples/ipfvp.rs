//! Survey of window data in INF/HLP files: TOC entries with an extended
//! block and links with link data (`vpx=`, `group=`, `auto` ...):
//! `cargo run -p ipf --example ipfvp FILE...`.
use ipf::text::{decode, Item};
use std::collections::BTreeMap;

fn main() {
    let mut toc: BTreeMap<String, usize> = BTreeMap::new();
    let mut links: BTreeMap<String, usize> = BTreeMap::new();
    for f in std::env::args().skip(1) {
        let Ok(d) = std::fs::read(&f) else { continue };
        let Ok(doc) = ipf::Doc::parse(&d) else { continue };
        for i in 0..doc.header.toc_count {
            let Ok(t) = doc.toc(i) else { continue };
            if t.ext != 0 {
                let k = format!("ext {:#06x} o={:x?} s={:x?} style={:x?} g={:?}", t.ext, t.origin, t.size, t.style, t.group.is_some());
                *toc.entry(k).or_default() += 1;
            }
            for c in &t.cells {
                let Ok(cell) = doc.cell(*c) else { continue };
                for it in decode(&doc, &cell) {
                    if let Item::Link { data: Some(d), target } = &it {
                        let k = format!("{:?} flags {:#06x} o={:x?} s={:x?} style={:x?} g={:?} [{}]", std::mem::discriminant(target), d.flags, d.origin, d.size, d.style, d.group.is_some(), f.rsplit('/').next().unwrap());
                        *links.entry(k).or_default() += 1;
                    }
                    if let Item::Other(ty, _) = &it {
                        *links.entry(format!("other escape {ty:#x} [{}]", f.rsplit('/').next().unwrap())).or_default() += 1;
                    }
                }
            }
        }
    }
    for (k, n) in toc { println!("{n:6} toc {k}"); }
    for (k, n) in links { println!("{n:6} link {k}"); }
}
