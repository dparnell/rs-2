//! imgconv IN OUT: decodes any supported image and writes it as a BMP,
//! PPM, JPEG, GIF, PCX, TIFF or Targa file (by OUT's extension).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [src, dst] = &args[..] else {
        eprintln!("usage: imgconv IN OUT.{{bmp,ppm,jpg,gif,pcx,tif,tga}}");
        return ExitCode::FAILURE;
    };
    let data = match std::fs::read(src) {
        Ok(d) => d,
        Err(e) => { eprintln!("{src}: {e}"); return ExitCode::FAILURE; }
    };
    let fmt = mmimage::identify(&data, data.len() as u64);
    let img = match mmimage::decode(&data) {
        Ok(i) => i,
        Err(e) => { eprintln!("{src}: {fmt:?}: {e:?}"); return ExitCode::FAILURE; }
    };
    println!("{src}: {fmt:?} {}x{} {} bits, {} palette entries", img.width, img.height, img.bits, img.palette.len());
    let ext = dst.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let out = match ext.as_str() {
        "bmp" => mmimage::bmp::encode(&img, mmimage::bmp::Kind::Win3),
        "jpg" | "jpeg" => mmimage::jpeg::encode(&img, 85),
        "gif" => mmimage::gif::encode(&img),
        "pcx" => mmimage::pcx::encode(&img),
        "tif" | "tiff" => mmimage::tiff::encode(&img, false, true),
        "tga" => mmimage::targa::encode(&img, true),
        _ => {
            let mut o = format!("P6\n{} {}\n255\n", img.width, img.height).into_bytes();
            for y in 0..img.height {
                for x in 0..img.width { o.extend_from_slice(&img.rgb(x, y)); }
            }
            o
        }
    };
    if let Err(e) = std::fs::write(dst, out) {
        eprintln!("{dst}: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
