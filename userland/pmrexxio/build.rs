fn main() {
    // DLLs live in the shared arena (between REXXUTIL at 0x15940000 and
    // REXXINIT at 0x15a00000: keep PMREXXIO under 256 KB).
    println!("cargo:rustc-link-arg=--defsym=__image_base=0x159c0000");
    // Keep every exported function through --gc-sections.
    let defs = std::fs::read_to_string("exports.def").unwrap();
    for line in defs.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with(';')) {
        let name = line.split('@').next().unwrap().trim();
        let name = name.rsplit('=').next().unwrap().trim();
        println!("cargo:rustc-link-arg=--undefined={name}");
    }
    println!("cargo:rerun-if-changed=exports.def");
}
