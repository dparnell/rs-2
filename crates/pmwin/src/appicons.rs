//! rs2's own icons for its programs (the RT_POINTER 1 resource xtask
//! puts in each executable: its file and program object icon and its
//! frame's icon), drawn from simple shapes like `mmicons`, at 32 x 32
//! and 16 x 16; nothing here is IBM's.

use alloc::vec::Vec;

use gpi::icon::{rgb, Image};

use crate::mmicons;
use crate::sysptr::{self, Pen, DARK, FACE, GREY, NAVY, WHITE};

const RED: u32 = 0xffc0_3030;
const GREEN: u32 = 0xff30_a050;
const BLUE: u32 = 0xff30_60c0;
const GOLD: u32 = 0xfff0_c040;

/// A sheet of paper with a folded corner.
fn page(p: &mut Pen) {
    p.polygon(&[(5.0, 2.0), (27.0, 2.0), (27.0, 24.0), (21.0, 30.0), (5.0, 30.0)], WHITE, DARK);
    p.polygon(&[(21.0, 30.0), (21.0, 24.0), (27.0, 24.0)], rgb(0xd8d8d8), DARK);
}

/// Text lines on a page from `y0` down.
fn lines(p: &mut Pen, x0: f64, x1: f64, y0: f64, n: usize) {
    for i in 0..n {
        let y = y0 - 4.0 * i as f64;
        p.rect(x0, y, x1 - if i % 2 == 1 { 4.0 } else { 0.0 }, y + 1.5, GREY);
    }
}

/// A monitor: screen colour `screen`, on a stand.
fn monitor(p: &mut Pen, screen: u32) {
    p.rect(12.0, 3.0, 20.0, 7.0, GREY);
    p.rect(8.0, 2.0, 24.0, 4.0, FACE);
    p.rect(2.0, 7.0, 30.0, 29.0, FACE);
    p.frame(2.0, 7.0, 30.0, 29.0, DARK);
    p.rect(4.5, 9.5, 27.5, 26.5, screen);
}

/// A hard disk drive, front view.
fn disk(p: &mut Pen) {
    p.rect(2.0, 8.0, 30.0, 22.0, FACE);
    p.frame(2.0, 8.0, 30.0, 22.0, DARK);
    p.rect(5.0, 13.0, 21.0, 15.0, GREY);
    p.disc(25.5, 14.0, 1.6, GREEN, DARK);
}

/// A magnifying glass with its lens at (x, y).
fn magnifier(p: &mut Pen, x: f64, y: f64, r: f64) {
    p.line((x + 0.7 * r, y - 0.7 * r), (x + 1.7 * r, y - 1.7 * r), 0.55 * r, rgb(0x704020));
    p.disc(x, y, r, rgb(0xc8e0f8), DARK);
}

/// A playing card with its lower left corner at (x, y).
fn card(p: &mut Pen, x: f64, y: f64, suit: u8, colour: u32) {
    p.rect(x, y, x + 14.0, y + 20.0, WHITE);
    p.frame(x, y, x + 14.0, y + 20.0, DARK);
    p.glyph(suit, x + 7.0, y + 10.0, 9.0, colour);
}

/// One of the programs (by its executable's name, upper case).
fn draw(name: &str, size: i32) -> Option<Image> {
    let mut p = Pen::new(size);
    match name {
        // Clipboard Viewer: a clipboard with a clip and a sheet.
        "CLIPOS2" => {
            p.rect(5.0, 2.0, 27.0, 28.0, rgb(0xa06830));
            p.frame(5.0, 2.0, 27.0, 28.0, DARK);
            p.rect(8.0, 4.0, 24.0, 24.0, WHITE);
            lines(&mut p, 10.0, 22.0, 19.0, 4);
            p.rect(11.0, 25.0, 21.0, 30.0, GREY);
            p.frame(11.0, 25.0, 21.0, 30.0, DARK);
        }
        // Pulse: a monitor with a load graph.
        "MPCPUMON" => {
            monitor(&mut p, rgb(0x102010));
            let pts = [(5.0, 13.0), (9.0, 14.0), (12.0, 22.0), (15.0, 12.0), (19.0, 18.0), (22.0, 15.0), (27.0, 24.0)];
            for w in pts.windows(2) {
                p.line(w[0], w[1], 1.6, rgb(0x40f040));
            }
        }
        // Check Disk: a drive and a check mark.
        "PMCHKDSK" => {
            disk(&mut p);
            p.line((14.0, 24.0), (19.0, 18.0), 3.0, GREEN);
            p.line((19.0, 18.0), (29.0, 30.0), 3.0, GREEN);
        }
        // Format: a drive being swept by a brush.
        "PMFORMAT" => {
            disk(&mut p);
            p.line((17.0, 24.0), (28.0, 30.0), 2.2, rgb(0x704020));
            p.polygon(&[(9.0, 21.0), (18.0, 21.0), (20.0, 26.0), (13.0, 26.0)], GOLD, rgb(0x806020));
        }
        // Picture Viewer: a framed picture and a magnifier.
        "PICVIEW" => {
            p.rect(2.0, 8.0, 26.0, 29.0, rgb(0xa06830));
            p.rect(4.5, 10.5, 23.5, 26.5, rgb(0x90c8f0));
            p.polygon(&[(4.5, 10.5), (4.5, 15.0), (10.0, 20.0), (15.0, 14.0), (23.5, 18.0), (23.5, 10.5)], rgb(0x40a040), rgb(0x40a040));
            p.disc(18.0, 22.5, 2.3, rgb(0xf0d020), rgb(0xf0d020));
            p.frame(2.0, 8.0, 26.0, 29.0, DARK);
            magnifier(&mut p, 22.0, 9.0, 5.0);
        }
        // Package Manager: a parcel, and an arrow going into it.
        "PKGMGR" => {
            let tan = rgb(0xd8a860);
            p.polygon(&[(3.0, 20.0), (16.0, 26.0), (29.0, 20.0), (16.0, 14.0)], rgb(0xf0cc88), DARK);
            p.polygon(&[(3.0, 20.0), (16.0, 14.0), (16.0, 2.0), (3.0, 8.0)], tan, DARK);
            p.polygon(&[(16.0, 14.0), (29.0, 20.0), (29.0, 8.0), (16.0, 2.0)], rgb(0xb08040), DARK);
            p.polygon(&[(13.0, 31.0), (19.0, 31.0), (19.0, 23.0), (24.0, 23.0), (16.0, 15.0), (8.0, 23.0), (13.0, 23.0)], GREEN, DARK);
        }
        // Seek and Scan Files: a page under a magnifier.
        "PMSEEK" => {
            page(&mut p);
            lines(&mut p, 8.0, 22.0, 24.0, 5);
            magnifier(&mut p, 16.0, 14.0, 6.5);
        }
        // Icon Editor: a pixel grid and a pencil.
        "ICONEDIT" => {
            p.rect(2.0, 6.0, 26.0, 30.0, WHITE);
            let colours = [RED, GOLD, GREEN, BLUE];
            for gy in 0..4 {
                for gx in 0..4 {
                    if (gx + gy) % 2 == 0 {
                        let (x, y) = (2.0 + 6.0 * gx as f64, 6.0 + 6.0 * gy as f64);
                        p.rect(x, y, x + 6.0, y + 6.0, colours[(gx + gy) as usize % 4]);
                    }
                }
            }
            p.frame(2.0, 6.0, 26.0, 30.0, DARK);
            p.polygon(&[(14.0, 4.0), (28.0, 18.0), (31.0, 15.0), (17.0, 1.0)], GOLD, DARK);
            p.polygon(&[(14.0, 4.0), (17.0, 1.0), (12.0, 0.5)], rgb(0xe0c090), DARK);
        }
        // Locale: a globe.
        "GLOCALE" => {
            p.disc(16.0, 16.0, 14.0, rgb(0x4080d0), DARK);
            p.polygon(&[(8.0, 22.0), (14.0, 25.0), (17.0, 20.0), (13.0, 15.0), (14.0, 9.0), (9.0, 12.0), (6.0, 17.0)], GREEN, rgb(0x207030));
            p.polygon(&[(19.0, 12.0), (24.0, 14.0), (26.0, 9.0), (21.0, 6.0)], GREEN, rgb(0x207030));
            p.line((2.5, 16.0), (29.5, 16.0), 0.6, rgb(0xc0d8f0));
        }
        // WebRS2: a globe in front of a browser window.
        "WEB" => {
            p.rect(2.0, 6.0, 30.0, 30.0, WHITE);
            p.rect(2.0, 25.0, 30.0, 30.0, NAVY);
            p.frame(2.0, 6.0, 30.0, 30.0, DARK);
            p.disc(15.0, 13.0, 10.5, rgb(0x3070d0), DARK);
            p.disc(11.0, 15.0, 4.0, GREEN, GREEN);
            p.disc(19.0, 9.0, 3.5, GREEN, GREEN);
            p.line((5.0, 13.0), (25.0, 13.0), 0.7, rgb(0xd0e0f0));
            p.line((15.0, 3.0), (15.0, 23.0), 0.7, rgb(0xd0e0f0));
        }
        // WEBTEXT: the globe on a text screen.
        "WEBTEXT" => {
            monitor(&mut p, rgb(0x000000));
            for i in 0..3 {
                let y = 22.0 - 4.0 * i as f64;
                p.rect(6.0, y, 14.0, y + 1.5, rgb(0xc0c0c0));
            }
            p.disc(21.0, 18.0, 5.5, rgb(0x3070d0), rgb(0xc0c0c0));
            p.disc(19.5, 19.5, 2.0, GREEN, GREEN);
        }
        // Klondike Solitaire: two fanned cards.
        "KLONDIKE" => {
            card(&mut p, 3.0, 8.0, 6, DARK);
            card(&mut p, 15.0, 3.0, 3, RED);
        }
        // Mahjongg: a stack of tiles.
        "MAHJONGG" => {
            for (x, y) in [(3.0, 3.0), (15.0, 3.0), (9.0, 13.0)] {
                p.rect(x + 2.0, y - 1.0, x + 14.0, y + 14.0, rgb(0x308060));
                p.rect(x, y, x + 12.0, y + 15.0, rgb(0xf8f0d8));
                p.frame(x, y, x + 12.0, y + 15.0, DARK);
            }
            p.disc(15.0, 20.5, 3.0, RED, RED);
            p.disc(15.0, 20.5, 1.2, rgb(0xf8f0d8), rgb(0xf8f0d8));
        }
        // OS/2 Chess: a pawn on a board.
        "OS2CHESS" => {
            for gy in 0..4 {
                for gx in 0..4 {
                    let (x, y) = (2.0 + 7.0 * gx as f64, 2.0 + 7.0 * gy as f64);
                    p.rect(x, y, x + 7.0, y + 7.0, if (gx + gy) % 2 == 0 { rgb(0xa06830) } else { rgb(0xf0d8b0) });
                }
            }
            p.frame(2.0, 2.0, 30.0, 30.0, DARK);
            p.polygon(&[(9.0, 6.0), (23.0, 6.0), (20.0, 10.0), (18.0, 19.0), (14.0, 19.0), (12.0, 10.0)], rgb(0x202020), WHITE);
            p.disc(16.0, 22.0, 4.0, rgb(0x202020), WHITE);
        }
        // Media Player: a play button.
        "MPPM" => {
            p.disc(16.0, 16.0, 14.0, rgb(0x2050a0), DARK);
            p.polygon(&[(12.0, 8.0), (12.0, 24.0), (25.0, 16.0)], WHITE, WHITE);
        }
        // Digital Audio: a waveform.
        "WEPM" => {
            p.rect(2.0, 5.0, 30.0, 27.0, rgb(0x102010));
            p.frame(2.0, 5.0, 30.0, 27.0, DARK);
            for (i, a) in [3.0, 7.0, 10.0, 5.0, 9.0, 4.0, 8.0, 2.0, 6.0].iter().enumerate() {
                let x = 5.0 + 2.7 * i as f64;
                p.line((x, 16.0 - a), (x, 16.0 + a), 1.3, rgb(0x40f040));
            }
        }
        "CDPM" => return mmicons::icon(mmicons::CD_PLAYER).map(|(a, b)| if size == sysptr::SIZE { a } else { b }),
        "MMVOLUME" => return mmicons::icon(mmicons::VOLUME).map(|(a, b)| if size == sysptr::SIZE { a } else { b }),
        // Multimedia Setup: a quaver in front of a gear.
        "STPM" => {
            const D: f64 = core::f64::consts::FRAC_1_SQRT_2;
            for (c, s) in [(1.0, 0.0), (D, D), (0.0, 1.0), (-D, D), (-1.0, 0.0), (-D, -D), (0.0, -1.0), (D, -D)] {
                p.line((16.0 + 9.0 * c, 16.0 + 9.0 * s), (16.0 + 14.0 * c, 16.0 + 14.0 * s), 4.0, GREY);
            }
            p.disc(16.0, 16.0, 10.5, FACE, DARK);
            p.disc(13.0, 10.5, 3.0, NAVY, NAVY);
            p.line((15.5, 10.5), (15.5, 23.0), 1.6, NAVY);
            p.line((15.5, 23.0), (21.0, 19.0), 1.6, NAVY);
        }
        // TCP/IP Configuration: two computers on a network.
        "TCPCFG2" => {
            for x in [2.0, 18.0] {
                p.rect(x, 16.0, x + 12.0, 27.0, FACE);
                p.frame(x, 16.0, x + 12.0, 27.0, DARK);
                p.rect(x + 2.0, 18.5, x + 10.0, 25.0, rgb(0x3060a0));
                p.rect(x + 4.0, 12.0, x + 8.0, 16.0, GREY);
            }
            p.line((8.0, 12.0), (8.0, 6.0), 1.4, DARK);
            p.line((24.0, 12.0), (24.0, 6.0), 1.4, DARK);
            p.line((3.0, 6.0), (29.0, 6.0), 1.6, RED);
        }
        // The System Editor: a page and a pencil.
        "E" => {
            page(&mut p);
            lines(&mut p, 8.0, 22.0, 24.0, 4);
            p.polygon(&[(12.0, 6.0), (24.0, 18.0), (27.0, 15.0), (15.0, 3.0)], GOLD, DARK);
            p.polygon(&[(12.0, 6.0), (15.0, 3.0), (10.0, 2.5)], rgb(0xe0c090), DARK);
        }
        // VIEW: an open book.
        "VIEW" => {
            p.polygon(&[(2.0, 6.0), (16.0, 4.0), (16.0, 26.0), (2.0, 28.0)], WHITE, DARK);
            p.polygon(&[(16.0, 4.0), (30.0, 6.0), (30.0, 28.0), (16.0, 26.0)], WHITE, DARK);
            for i in 0..4 {
                let y = 21.0 - 4.0 * i as f64;
                p.line((5.0, y + 1.0), (13.0, y), 1.0, GREY);
                p.line((19.0, y), (27.0, y + 1.0), 1.0, GREY);
            }
            p.rect(15.0, 3.0, 17.0, 27.0, BLUE);
        }
        // Install OS/2 Applications and Install rs2: a box and an arrow.
        "APPINST" | "PMINSTAL" => {
            p.rect(4.0, 2.0, 28.0, 16.0, rgb(0xc89050));
            p.frame(4.0, 2.0, 28.0, 16.0, DARK);
            p.rect(4.0, 13.0, 28.0, 16.0, rgb(0xa06830));
            let c = if name == "APPINST" { GREEN } else { BLUE };
            p.polygon(&[(12.0, 31.0), (20.0, 31.0), (20.0, 23.0), (25.0, 23.0), (16.0, 14.0), (7.0, 23.0), (12.0, 23.0)], c, DARK);
        }
        // Display: a monitor with a desktop on it.
        "DISPLAY" => {
            monitor(&mut p, rgb(0x308080));
            p.rect(7.0, 12.0, 17.0, 20.0, FACE);
            p.rect(7.0, 18.5, 17.0, 20.0, NAVY);
            p.rect(14.0, 15.0, 24.0, 24.0, WHITE);
            p.rect(14.0, 22.5, 24.0, 24.0, NAVY);
        }
        _ => return None,
    }
    Some(p.img)
}

/// The names of the programs with an icon here.
pub const PROGRAMS: &[&str] = &[
    "CLIPOS2", "MPCPUMON", "PMCHKDSK", "PMFORMAT", "PICVIEW", "PMSEEK", "PKGMGR", "ICONEDIT", "GLOCALE", "KLONDIKE", "MAHJONGG", "OS2CHESS", "MPPM", "WEPM",
    "CDPM", "MMVOLUME", "STPM", "TCPCFG2", "E", "VIEW", "APPINST", "PMINSTAL", "DISPLAY", "WEB", "WEBTEXT",
];

/// Program `name`'s icon as an OS/2 icon file (32 x 32 and 16 x 16).
pub fn icon_file(name: &str) -> Option<Vec<u8>> {
    Some(gpi::icon::to_array(&[draw(name, sysptr::SIZE)?, draw(name, sysptr::MINI)?]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_drawn() {
        for name in PROGRAMS {
            let e = gpi::icon::parse(&icon_file(name).unwrap_or_else(|| panic!("{name}")));
            assert_eq!(e.len(), 2, "{name}");
            assert!(e.iter().all(|e| !e.image.is_blank()), "{name} blank");
            assert_eq!((e[0].image.w, e[1].image.w), (32, 16), "{name}");
        }
        assert!(icon_file("NOSUCH").is_none());
    }

    /// `cargo test -p pmwin appicons -- --ignored`: every program icon
    /// (and its mini version) on a grey sheet, magnified, as a PPM in the
    /// system's temporary directory.
    #[test]
    #[ignore]
    fn write_sheet() {
        extern crate std;
        let z = 3;
        let cell = 34 * z + 18 * z;
        let per_row = 6;
        let rows = PROGRAMS.len().div_ceil(per_row) as i32;
        let (w, h) = (cell * per_row as i32, 36 * z * rows);
        let mut px = std::vec![0x00a0_a0a0u32; (w * h) as usize];
        for (n, name) in PROGRAMS.iter().enumerate() {
            let (cx, cy) = ((n % per_row) as i32 * cell, (n / per_row) as i32 * 36 * z);
            let (a, b) = (draw(name, sysptr::SIZE).unwrap(), draw(name, sysptr::MINI).unwrap());
            for (img, x0) in [(&a, cx), (&b, cx + 34 * z)] {
                for y in 0..img.h {
                    for x in 0..img.w {
                        let c = match img.at(x, y) {
                            gpi::icon::TRANSPARENT => continue,
                            gpi::icon::INVERT => 0xff00ff,
                            v => v & 0xffffff,
                        };
                        for dy in 0..z {
                            for dx in 0..z {
                                let (sx, sy) = (x0 + x * z + dx, cy + (img.h - 1 - y) * z + dy + z);
                                px[(sy * w + sx) as usize] = c;
                            }
                        }
                    }
                }
            }
        }
        let mut out = std::format!("P6\n{w} {h}\n255\n").into_bytes();
        for c in px {
            out.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        }
        std::fs::write(std::env::temp_dir().join("rs2-appicons.ppm"), out).unwrap();
    }
}
