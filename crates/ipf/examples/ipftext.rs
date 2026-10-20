//! Print every panel of an INF/HLP file as plain text: `cargo run -p ipf --example ipftext FILE`.
fn main() {
    let path = std::env::args().nth(1).expect("usage: ipftext FILE");
    let d = std::fs::read(&path).unwrap();
    let doc = ipf::Doc::parse(&d).unwrap();
    for i in 0..doc.header.toc_count {
        let t = doc.toc(i).unwrap();
        println!("==== {} (res {:#x?})", ipf::latin1(&t.title), doc.res_of_panel(i));
        println!("{}", ipf::latin1(&ipf::text::plain(&doc, i)).replace("\r\n", "\n").replace('\x07', "*"));
    }
}
