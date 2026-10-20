//! Compile IPF source: `cargo run -p ipf --example ipfc SRC.IPF OUT.HLP|OUT.INF`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, src, out] = &a[..] else {
        eprintln!("usage: ipfc SOURCE.IPF OUTPUT.HLP|OUTPUT.INF");
        std::process::exit(2);
    };
    let text = std::fs::read(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    let help = !out.to_ascii_uppercase().ends_with(".INF");
    match ipf::compile::compile(&text, help) {
        Ok(f) => std::fs::write(out, f).unwrap(),
        Err(e) => {
            eprintln!("{src}: {e}");
            std::process::exit(1);
        }
    }
}
