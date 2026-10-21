//! Dump an FLI/FLC animation's frames as PPM images, for eyeballing:
//! `cargo run -p mmfmt --example flcdump FILE.FLC OUTDIR [EVERY]`
//! writes OUTDIR/frameNNN.ppm for every EVERY-th frame (default 10).

use mmfmt::flc::Decoder;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: flcdump FILE OUTDIR [EVERY]");
        std::process::exit(1);
    }
    let every: u16 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10).max(1);
    let mut d = Decoder::new(std::fs::read(&args[1]).expect("read")).expect("not an FLC");
    let h = *d.header();
    println!("{}x{} {} frames, {} ms/frame, magic {:#x}", h.width, h.height, h.frames, h.delay_ms(), h.magic);
    std::fs::create_dir_all(&args[2]).unwrap();
    for _ in 0..h.frames {
        let f = d.next_frame().expect("decode");
        if f.index % every == 0 || f.index + 1 == h.frames {
            let mut ppm = format!("P6\n{} {}\n255\n", d.width(), d.height()).into_bytes();
            for &p in d.frame() {
                ppm.extend_from_slice(&d.palette()[p as usize]);
            }
            std::fs::write(format!("{}/frame{:03}.ppm", args[2], f.index), ppm).unwrap();
        }
    }
}
