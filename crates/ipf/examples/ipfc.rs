//! Compile IPF source: `cargo run -p ipf --example ipfc SRC.IPF OUT.HLP|OUT.INF`.
//! `:artwork.` files are read from the source's directory.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, src, out] = &a[..] else {
        eprintln!("usage: ipfc SOURCE.IPF OUTPUT.HLP|OUTPUT.INF");
        std::process::exit(2);
    };
    let text = std::fs::read(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    let help = !out.to_ascii_uppercase().ends_with(".INF");
    let dir = std::path::Path::new(src).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let load = |name: &[u8]| std::fs::read(dir.join(String::from_utf8_lossy(name).as_ref())).ok();
    match ipf::compile::compile_with(&text, help, &load) {
        Ok(f) => std::fs::write(out, f).unwrap(),
        Err(e) => {
            eprintln!("{src}: {e}");
            std::process::exit(1);
        }
    }
}
