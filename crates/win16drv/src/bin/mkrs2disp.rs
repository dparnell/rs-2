//! mkrs2disp VGA.DRV RS2DISP.DRV: make rs2's display driver from an
//! installed VGA.DRV (as APPINST does), for testing with dosrun.

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 3 {
        eprintln!("usage: mkrs2disp VGA.DRV RS2DISP.DRV");
        std::process::exit(2);
    }
    let vga = std::fs::read(&a[1]).expect("read VGA.DRV");
    match win16drv::build(&vga) {
        Ok(d) => std::fs::write(&a[2], d).expect("write"),
        Err(e) => {
            eprintln!("mkrs2disp: {e}");
            std::process::exit(1);
        }
    }
}
