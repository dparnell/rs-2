use std::{env, fs, path::Path};
fn main() {
    let out = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out).join("gen.rs"), "pub const GENERATED: u32 = 7;").unwrap();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=FROM_BUILD={}", env::var("TARGET").unwrap());
}
