// The resources (pmdemo64.rc) as the table os2::resources!() embeds.
fn main() {
    let rc = std::path::Path::new("pmdemo64.rc");
    let table = os2_rc::host::compile_table64(rc).unwrap_or_else(|e| panic!("{e}"));
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("resources.bin");
    std::fs::write(out, table).unwrap();
    println!("cargo:rerun-if-changed=pmdemo64.rc");
}
