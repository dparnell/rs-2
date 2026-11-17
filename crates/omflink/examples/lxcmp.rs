//! Compare the expanded pages of two LX modules (e.g. a /EXEPACK link with
//! a plain one): `cargo run -p omflink --example lxcmp -- A.EXE B.EXE`.
use os2_exe::{Format, detect, lx};

fn pages(path: &str) -> Vec<Vec<u8>> {
    let d = std::fs::read(path).unwrap();
    let Ok(Format::Lx { header }) = detect(&d) else { panic!("{path}: not LX") };
    let m = lx::Module::parse(&d, header).unwrap();
    (1..=m.header.page_count)
        .map(|p| {
            let mut b = vec![0u8; m.header.page_size as usize];
            m.load_page(p, &mut b).unwrap();
            b
        })
        .collect()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (x, y) = (pages(&a[0]), pages(&a[1]));
    assert_eq!(x.len(), y.len(), "page counts differ");
    let mut bad = 0;
    for (i, (p, q)) in x.iter().zip(&y).enumerate() {
        if p != q {
            println!("page {} differs", i + 1);
            bad += 1;
        }
    }
    println!("{} pages, {} differ", x.len(), bad);
}
