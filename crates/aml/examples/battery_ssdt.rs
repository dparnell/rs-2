//! Writes the test battery SSDT for QEMU's `-acpitable file=...`:
//! `cargo run -p aml --example battery_ssdt -- OUT.aml [--discharging]`.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| String::from("battery.aml"));
    let t = aml::asm::battery_ssdt(args.iter().any(|a| a == "--discharging"));
    std::fs::write(&out, t).expect("write");
    println!("{out}");
}
