//! Dump an INF/HLP file's structures: `cargo run -p ipf --example ipfdump FILE [panel]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let doc = ipf::Doc::parse(&d).unwrap();
    println!("{:#x?}", doc.header);
    println!("{:#x?}", doc.ext);
    println!("byte 0x52 = {:#x}", d[0x52]);
    let h = &doc.header;
    println!("nls: {:02x?}", &d[h.nls_start as usize..(h.nls_start + h.nls_size) as usize]);
    if let Some(e) = &doc.ext {
        let o = h.ext_start as usize;
        println!("ext raw: {:02x?}", &d[o..o + 0x40]);
        println!("fonts: {:?}", doc.fonts());
        println!("child: {:?}", doc.child_pages());
        println!("ctrl: {:02x?}", &d[e.ctrl_start as usize..(e.ctrl_start + e.ctrl_size) as usize]);
    }
    for i in 0..h.toc_count.min(8) {
        let t = doc.toc(i).unwrap();
        println!("toc {i}: res={:?} {:x?} {}", doc.res_of_panel(i), t, ipf::latin1(&t.title));
    }
    for e in doc.index().iter().take(5) {
        println!("index {:x?}", e);
    }
    if let Some(p) = a.get(2) {
        for p in p.split(',') {
        let p: u16 = p.parse().unwrap();
        let t = doc.toc(p).unwrap();
        println!("== panel {p} res={:?} {}", doc.res_of_panel(p), ipf::latin1(&t.title));
        for c in t.cells {
            let cell = doc.cell(c).unwrap();
            println!("cell {c}: {:02x?}", cell.text);
            let mut o = String::new();
            for i in ipf::text::decode(&doc, &cell) {
                match i {
                    ipf::text::Item::Word(w) => { o += &ipf::latin1(w); o.push(' '); }
                    ipf::text::Item::ToggleSpacing => o += "~ ",
                    ipf::text::Item::Space => o += "_ ",
                    x => { o += &format!("<{:?}> ", x); }
                }
            }
            println!("{o}");
        }
        }
    }
    // Layout order of the sections.
    let mut secs = vec![("toc", h.toc_start), ("tocoff", h.toc_offsets), ("res", h.res_start), ("names", h.name_start), ("index", h.index_start), ("search", h.search_start & 0x7fffffff), ("celloff", h.cell_offsets), ("dict", h.dict_start), ("img", h.image_start), ("nls", h.nls_start), ("ext", h.ext_start)];
    if let Some(e) = &doc.ext { secs.extend([("fonts", e.font_start), ("db", e.db_start), ("gname", e.gname_start), ("child", e.child_start), ("ctrl", e.ctrl_start)]); }
    secs.sort_by_key(|s| s.1);
    println!("{:x?} len {:#x}", secs, d.len());
    let c0 = d.get(h.cell_offsets as usize..h.cell_offsets as usize + 8).map(|b| u32::from_le_bytes([b[0],b[1],b[2],b[3]]));
    println!("cell0 at {:x?}", c0);
}
