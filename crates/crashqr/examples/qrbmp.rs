//! Write a crash QR code as a BMP: `qrbmp OUT.bmp FILE` packs FILE like
//! the panic screen does (`--raw TEXT` encodes TEXT as it is). For checking
//! the encoder with an independent decoder (tools/scripts/qrdecode.swift).

use crashqr::{deflate::Deflate, qr::{Ecc, QrCode}};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text: Vec<u8> = if a[2] == "--raw" {
        a[3].as_bytes().to_vec()
    } else {
        let report = std::fs::read(&a[2]).unwrap();
        let mut z = Box::new(Deflate::new());
        let mut tmp = vec![0u8; 1 << 20];
        let mut out = vec![0u8; 1 << 21];
        let n = crashqr::pack(&mut z, &report, &mut tmp, &mut out).unwrap();
        out.truncate(n);
        out
    };
    let mut q = Box::new(QrCode::new());
    let v = q.encode(&text, Ecc::L, 1, 40).expect("too long");
    eprintln!("version {v}, {} modules, {} characters", q.size(), text.len());
    let scale = 4;
    let side = (q.size() + 8) * scale;
    let row = (side * 3).next_multiple_of(4);
    let mut bmp = Vec::new();
    let size = 54 + row * side;
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(size as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(side as i32).to_le_bytes());
    bmp.extend_from_slice(&(-(side as i32)).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&[0; 24]);
    for py in 0..side {
        let mut line = vec![0u8; row];
        for px in 0..side {
            let (x, y) = ((px / scale) as isize - 4, (py / scale) as isize - 4);
            let dark = x >= 0 && y >= 0 && (x as usize) < q.size() && (y as usize) < q.size() && q.dark(x as usize, y as usize);
            let c = if dark { 0 } else { 255 };
            line[px * 3..px * 3 + 3].fill(c);
        }
        bmp.extend_from_slice(&line);
    }
    std::fs::write(&a[1], bmp).unwrap();
}
