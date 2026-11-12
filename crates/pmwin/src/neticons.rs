//! rs2's own icons for the Workplace Shell's network objects (PMWP's
//! network groups, servers, shared directories and network printers):
//! drawn from simple shapes like `shellicons`, at 32 x 32 and 16 x 16;
//! nothing here is IBM's.

use gpi::icon::{rgb, Image};

use crate::sysptr::{self, Pen, DARK, FACE, GREY, NAVY, WHITE};

/// A network group (workgroup or domain): three computers inside a
/// ring.
pub const GROUP: u32 = 1;
/// A server: a tower with drive bays and lights.
pub const SERVER: u32 = 2;
/// A shared directory: a folder on a network cable.
pub const SHARED_DIR: u32 = 3;
/// A network printer: a printer on a network cable.
pub const NET_PRINTER: u32 = 4;
/// A network printer that cannot be reached: greyed, with a red cross.
pub const NET_PRINTER_OFF: u32 = 5;
pub const LAST: u32 = 5;

const CABLE: u32 = 0xff20_6030;

fn tiny_computer(p: &mut Pen, x: f64, y: f64, screen: u32) {
    p.rect(x, y, x + 9.0, y + 7.0, FACE);
    p.frame(x, y, x + 9.0, y + 7.0, DARK);
    p.rect(x + 1.5, y + 1.5, x + 7.5, y + 5.5, screen);
    p.rect(x + 2.5, y - 1.5, x + 6.5, y, GREY);
}

/// The network cable under an object: a line down to a bar with three
/// taps.
fn cable(p: &mut Pen, x: f64, top: f64) {
    p.line((x, top), (x, 3.5), 1.6, CABLE);
    p.line((3.0, 3.0), (29.0, 3.0), 2.0, CABLE);
    for tx in [6.0, 26.0] {
        p.line((tx, 3.0), (tx, 5.5), 1.4, CABLE);
    }
}

fn folder(p: &mut Pen, y: f64) {
    p.polygon(&[(3.0, y), (29.0, y), (29.0, y + 18.0), (15.0, y + 18.0), (13.0, y + 21.0), (4.0, y + 21.0), (3.0, y + 19.0)], rgb(0xf0c040), rgb(0x806020));
    p.rect(4.0, y + 15.5, 28.0, y + 16.5, rgb(0xc09030));
}

fn printer(p: &mut Pen, y: f64, body: u32, paper: u32) {
    p.rect(9.0, y + 10.0, 23.0, y + 20.0, paper);
    p.frame(9.0, y + 10.0, 23.0, y + 20.0, DARK);
    p.rect(3.0, y + 2.0, 29.0, y + 12.0, body);
    p.frame(3.0, y + 2.0, 29.0, y + 12.0, DARK);
    p.rect(8.0, y + 4.0, 24.0, y + 6.0, DARK);
}

fn draw(id: u32, size: i32) -> Option<Image> {
    let mut p = Pen::new(size);
    match id {
        GROUP => {
            p.disc(16.0, 16.0, 14.5, rgb(0xd8e8f8), NAVY);
            let screen = rgb(0x2e6e8e);
            tiny_computer(&mut p, 5.0, 17.0, screen);
            tiny_computer(&mut p, 18.0, 17.0, screen);
            tiny_computer(&mut p, 11.5, 6.0, screen);
            p.line((9.5, 15.5), (13.0, 13.5), 1.0, CABLE);
            p.line((22.5, 15.5), (19.0, 13.5), 1.0, CABLE);
        }
        SERVER => {
            p.rect(9.0, 2.0, 23.0, 30.0, FACE);
            p.frame(9.0, 2.0, 23.0, 30.0, DARK);
            for k in 0..3 {
                let y = 22.0 - k as f64 * 5.0;
                p.rect(11.0, y, 21.0, y + 3.5, rgb(0x9a9aa2));
                p.frame(11.0, y, 21.0, y + 3.5, DARK);
                p.rect(12.5, y + 1.3, 17.0, y + 2.2, DARK);
            }
            p.disc(13.0, 6.0, 1.3, rgb(0x30d040), rgb(0x107020));
            p.disc(17.0, 6.0, 1.3, rgb(0xe0a020), rgb(0x806010));
        }
        SHARED_DIR => {
            folder(&mut p, 9.0);
            cable(&mut p, 16.0, 9.0);
        }
        NET_PRINTER => {
            printer(&mut p, 8.0, FACE, WHITE);
            p.disc(26.0, 16.0, 1.3, rgb(0x30d040), rgb(0x107020));
            cable(&mut p, 16.0, 10.0);
        }
        NET_PRINTER_OFF => {
            printer(&mut p, 8.0, rgb(0xb0b0b0), rgb(0xe0e0e0));
            cable(&mut p, 16.0, 10.0);
            let red = rgb(0xd02020);
            p.line((18.0, 14.0), (30.0, 26.0), 3.0, red);
            p.line((18.0, 26.0), (30.0, 14.0), 3.0, red);
        }
        _ => return None,
    }
    Some(p.img)
}

/// Icon `id` as (32 x 32 image, 16 x 16 mini image).
pub fn icon(id: u32) -> Option<(Image, Image)> {
    Some((draw(id, sysptr::SIZE)?, draw(id, sysptr::MINI)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_drawn() {
        for id in 1..=LAST {
            let (a, b) = icon(id).unwrap_or_else(|| panic!("icon {id}"));
            assert_eq!((a.w, a.h, b.w, b.h), (32, 32, 16, 16));
            assert!(!a.is_blank() && !b.is_blank(), "icon {id} blank");
        }
        assert!(icon(LAST + 1).is_none());
    }

    /// `cargo test -p pmwin neticons -- --ignored`: the icons on a grey
    /// sheet, magnified, as rs2-neticons.ppm in the temporary directory.
    #[test]
    #[ignore]
    fn write_sheet() {
        extern crate std;
        let z = 3;
        let cell = 34 * z + 18 * z;
        let n = LAST as i32;
        let (w, h) = (cell * n, 36 * z);
        let mut px = std::vec![0x00a0_a0a0u32; (w * h) as usize];
        for id in 1..=LAST {
            let (big, mini) = icon(id).unwrap();
            let x0 = (id as i32 - 1) * cell;
            for (img, x0) in [(&big, x0), (&mini, x0 + 34 * z)] {
                for y in 0..img.h {
                    for x in 0..img.w {
                        let c = img.at(x, y);
                        if c == gpi::icon::TRANSPARENT {
                            continue;
                        }
                        for dy in 0..z {
                            for dx in 0..z {
                                let (sx, sy) = (x0 + x * z + dx, (img.h - 1 - y) * z + dy + z);
                                px[(sy * w + sx) as usize] = c & 0xffffff;
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
        std::fs::write(std::env::temp_dir().join("rs2-neticons.ppm"), out).unwrap();
    }
}
