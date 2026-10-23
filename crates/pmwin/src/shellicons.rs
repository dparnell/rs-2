//! rs2's own icons for the desktop shell (RS2SHELL.EXE): drives, the
//! Drives and Command Prompts folders, command prompts, shutdown, the
//! minimized window viewer and the window list. Drawn from simple shapes
//! in the style of `sysptr`, at 32 x 32 and 16 x 16; nothing here is
//! copied from IBM's.

use gpi::icon::{rgb, Image};

use crate::sysptr::{self, Pen, BLACK, DARK, FACE, GREY, NAVY, WHITE};

pub const HARD_DISK: u32 = 1;
pub const CD_ROM: u32 = 2;
pub const DRIVES: u32 = 3;
pub const SHUTDOWN: u32 = 4;
pub const PROMPT_WINDOW: u32 = 5;
pub const PROMPT_FULL_SCREEN: u32 = 6;
pub const PROMPTS: u32 = 7;
pub const MINIMIZED: u32 = 8;
pub const WINDOW_LIST: u32 = 9;
pub const DESKTOP: u32 = 10;
/// The Assistance Center folder (a lifebuoy).
pub const ASSISTANCE: u32 = 11;
/// The Information folder (a folder with an "i").
pub const INFO_FOLDER: u32 = 12;
/// The Help Index and Glossary (a help book).
pub const INDEX: u32 = 13;
/// Internet objects (a globe).
pub const INTERNET: u32 = 14;
/// Highest icon id.
pub const LAST: u32 = 14;

const SILVER: u32 = 0xffe0_e0e8;

fn hard_disk(p: &mut Pen, dx: f64, dy: f64, s: f64) {
    let (x0, y0) = (dx, dy);
    let r = |x: f64, y: f64| (x0 + x * s, y0 + y * s);
    let (a, b) = (r(2.0, 9.0), r(30.0, 22.0));
    p.rect(a.0, a.1, b.0, b.1, FACE);
    let (c, d) = (r(2.0, 9.0), r(30.0, 13.0));
    p.rect(c.0, c.1, d.0, d.1, rgb(0x9a9aa2));
    p.frame(a.0, a.1, b.0, b.1, DARK);
    let (e, f) = (r(5.0, 16.0), r(20.0, 17.0));
    p.rect(e.0, e.1, f.0, f.1, DARK);
    let led = r(25.5, 16.5);
    p.disc(led.0, led.1, 1.8 * s.max(0.6), rgb(0x30d040), rgb(0x107020));
}

fn folder(p: &mut Pen) {
    p.polygon(&[(2.0, 4.0), (30.0, 4.0), (30.0, 24.0), (14.0, 24.0), (12.0, 27.0), (3.0, 27.0), (2.0, 25.0)], rgb(0xf0c040), rgb(0x806020));
    p.rect(3.0, 21.5, 29.0, 22.5, rgb(0xc09030));
}

/// A window: frame, title bar and a client of the given colour.
fn window(p: &mut Pen, x0: f64, y0: f64, x1: f64, y1: f64, client: u32) {
    p.rect(x0, y0, x1, y1, FACE);
    p.frame(x0, y0, x1, y1, DARK);
    let t = (y1 - y0) / 6.0;
    p.rect(x0 + 1.0, y1 - t - 1.0, x1 - 1.0, y1 - 1.0, NAVY);
    p.rect(x0 + 2.0, y0 + 2.0, x1 - 2.0, y1 - t - 2.0, client);
}

fn prompt_text(p: &mut Pen, cx: f64, cy: f64, h: f64) {
    p.glyph(b'>', cx, cy, h, rgb(0xd0d0d0));
    p.rect(cx + h * 0.55, cy - h * 0.5, cx + h * 1.25, cy - h * 0.5 + h * 0.2, rgb(0xd0d0d0));
}

fn draw(id: u32, size: i32) -> Option<Image> {
    let mut p = Pen::new(size);
    match id {
        HARD_DISK => hard_disk(&mut p, 0.0, 0.0, 1.0),
        CD_ROM => {
            p.disc(16.0, 16.0, 14.0, SILVER, DARK);
            p.disc(16.0, 16.0, 10.0, rgb(0xc8d8f0), rgb(0xb0c0e0));
            p.disc(16.0, 16.0, 5.0, rgb(0xa0a0a8), DARK);
            p.disc(16.0, 16.0, 2.2, WHITE, DARK);
            p.line((9.0, 23.0), (12.5, 19.5), 1.5, WHITE);
        }
        DRIVES => {
            folder(&mut p);
            hard_disk(&mut p, 6.0, 1.0, 0.65);
        }
        SHUTDOWN => {
            p.disc(16.0, 16.0, 14.5, rgb(0xc03028), DARK);
            p.disc(16.0, 15.0, 8.5, WHITE, WHITE);
            p.disc(16.0, 15.0, 6.0, rgb(0xc03028), rgb(0xc03028));
            p.rect(13.0, 18.0, 19.0, 26.0, rgb(0xc03028));
            p.line((16.0, 15.5), (16.0, 25.0), 3.0, WHITE);
        }
        PROMPT_WINDOW => {
            window(&mut p, 2.0, 4.0, 30.0, 28.0, BLACK);
            prompt_text(&mut p, 9.0, 14.0, 8.0);
        }
        PROMPT_FULL_SCREEN => {
            // A monitor on a stand.
            p.rect(12.0, 3.0, 20.0, 6.0, GREY);
            p.rect(8.0, 2.0, 24.0, 3.5, DARK);
            p.rect(2.0, 6.0, 30.0, 29.0, FACE);
            p.frame(2.0, 6.0, 30.0, 29.0, DARK);
            p.rect(4.0, 8.0, 28.0, 27.0, BLACK);
            prompt_text(&mut p, 9.0, 17.0, 8.0);
        }
        PROMPTS => {
            folder(&mut p);
            window(&mut p, 9.0, 5.0, 25.0, 19.0, BLACK);
            p.glyph(b'>', 13.0, 10.5, 5.0, rgb(0xd0d0d0));
        }
        MINIMIZED => {
            window(&mut p, 2.0, 4.0, 30.0, 28.0, WHITE);
            for (i, c) in [rgb(0x2050c0), rgb(0x20a060), rgb(0xd09020)].into_iter().enumerate() {
                let x = 5.0 + i as f64 * 8.0;
                p.rect(x, 8.0, x + 6.0, 14.0, c);
                p.frame(x, 8.0, x + 6.0, 14.0, DARK);
                p.rect(x, 16.0, x + 6.0, 17.0, GREY);
            }
        }
        WINDOW_LIST => {
            window(&mut p, 2.0, 4.0, 30.0, 28.0, WHITE);
            p.rect(5.0, 18.0, 27.0, 21.0, NAVY);
            for k in 0..3 {
                let y = 7.0 + k as f64 * 4.0;
                p.rect(5.0, y, 24.0, y + 1.5, GREY);
            }
        }
        ASSISTANCE => {
            // A lifebuoy: red with four white bands, a "?" in its middle.
            let red = rgb(0xd03828);
            p.disc(16.0, 16.0, 14.5, red, DARK);
            for (dx, dy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                p.line((16.0 + dx * 6.0, 16.0 + dy * 6.0), (16.0 + dx * 10.5, 16.0 + dy * 10.5), 5.0, WHITE);
            }
            p.disc(16.0, 16.0, 7.0, rgb(0x2050c0), DARK);
            p.glyph(b'?', 16.0, 16.0, 9.0, WHITE);
        }
        INFO_FOLDER => {
            folder(&mut p);
            p.disc(16.0, 12.5, 7.0, rgb(0x2050c0), DARK);
            p.glyph(b'i', 16.0, 12.5, 9.0, WHITE);
        }
        INDEX => {
            // A closed book (blue cover, white pages) with a "?".
            p.rect(7.0, 3.0, 26.0, 7.0, WHITE);
            p.frame(7.0, 3.0, 26.0, 7.0, DARK);
            p.rect(5.0, 5.0, 24.0, 29.0, rgb(0x2050c0));
            p.frame(5.0, 5.0, 24.0, 29.0, DARK);
            p.rect(7.0, 5.0, 8.0, 29.0, rgb(0x103080));
            p.glyph(b'?', 16.0, 17.0, 12.0, WHITE);
        }
        INTERNET => {
            // A globe: blue sea, green land, meridians and parallels.
            let sea = rgb(0x3070d0);
            p.disc(16.0, 16.0, 14.0, sea, DARK);
            let land = rgb(0x40a040);
            p.disc(11.0, 19.0, 5.0, land, land);
            p.disc(20.0, 10.0, 4.5, land, land);
            p.disc(22.0, 22.0, 3.0, land, land);
            for y in [9.0, 16.0, 23.0] {
                p.line((4.0, y), (28.0, y), 0.8, rgb(0xd0e0f0));
            }
            p.line((16.0, 2.5), (16.0, 29.5), 0.8, rgb(0xd0e0f0));
        }
        DESKTOP => {
            p.rect(2.0, 5.0, 30.0, 27.0, rgb(0x2e6e8e));
            p.frame(2.0, 5.0, 30.0, 27.0, DARK);
            let small = sysptr::icon(sysptr::SPTR_FOLDER, (size / 3).max(6))?;
            let off = (size as f64 * 0.16) as i32;
            for y in 0..small.h {
                for x in 0..small.w {
                    let v = small.at(x, y);
                    if v != gpi::icon::TRANSPARENT {
                        p.img.set(off + x, size - off - small.h + y - size / 10, v);
                    }
                }
            }
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
        assert_ne!(icon(HARD_DISK).unwrap().0, icon(CD_ROM).unwrap().0);
    }

    /// `cargo test -p pmwin shellicons -- --ignored`: the icons on a grey
    /// sheet, magnified, as rs2-shellicons.ppm in the temporary directory.
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
        std::fs::write(std::env::temp_dir().join("rs2-shellicons.ppm"), out).unwrap();
    }
}
