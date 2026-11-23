#[link(name = "cdy", kind = "dylib")]
unsafe extern "C" {
    fn cdy_sum(n: u32) -> u32;
}
fn main() {
    println!("from the DLL: {}", unsafe { cdy_sum(100) });
    let r = std::panic::catch_unwind(|| {
        let v: Vec<u32> = Vec::new();
        v[std::env::args().count()]
    });
    println!("caught: {}", r.is_err());
}
