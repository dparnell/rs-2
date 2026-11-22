fn main() {
    // 64-bit DLLs live in the shared arena; this base is free of rs2's
    // 32-bit DLLs (the loader moves the image if it is taken).
    println!("cargo:rustc-link-arg=--defsym=__image_base=0x18000000");
}
