//! rs2's own icons for the multimedia objects of the Workplace Shell
//! (PMWP's MMDataFile classes, the Multimedia folder, Volume Control and
//! the CD Player): drawn from simple shapes like `shellicons`, at 32 x 32
//! and 16 x 16; nothing here is IBM's.

use gpi::icon::{rgb, Image};

use crate::sysptr::{self, Pen, DARK, FACE, GREY, NAVY, WHITE};

/// A sound file: a page with a loudspeaker and sound waves.
pub const AUDIO: u32 = 1;
/// A MIDI file: a page with two quavers.
pub const MIDI: u32 = 2;
/// A movie: a strip of film.
pub const VIDEO: u32 = 3;
/// An image: a framed picture of hills and the sun.
pub const IMAGE: u32 = 4;
/// The Multimedia folder: a folder with a quaver.
pub const FOLDER: u32 = 5;
/// Volume Control: a loudspeaker over a slider.
pub const VOLUME: u32 = 6;
/// The CD Player: a disc with a play arrow.
pub const CD_PLAYER: u32 = 7;
pub const LAST: u32 = 7;

/// A sheet of paper with a folded corner.
fn page(p: &mut Pen) {
    p.polygon(&[(5.0, 2.0), (27.0, 2.0), (27.0, 24.0), (21.0, 30.0), (5.0, 30.0)], WHITE, DARK);
    p.polygon(&[(21.0, 30.0), (21.0, 24.0), (27.0, 24.0)], rgb(0xd8d8d8), DARK);
}

/// A quaver with its head at (x, y).
fn quaver(p: &mut Pen, x: f64, y: f64, s: f64, c: u32) {
    p.disc(x, y, 2.6 * s, c, c);
    p.line((x + 2.2 * s, y), (x + 2.2 * s, y + 10.0 * s), 1.4 * s.max(0.7), c);
    p.line((x + 2.2 * s, y + 10.0 * s), (x + 6.0 * s, y + 6.5 * s), 1.4 * s.max(0.7), c);
}

fn speaker(p: &mut Pen, x: f64, y: f64, s: f64) {
    let r = |a: f64, b: f64| (x + a * s, y + b * s);
    let (a, b) = (r(0.0, 4.0), r(4.0, 10.0));
    p.rect(a.0, a.1, b.0, b.1, GREY);
    p.polygon(&[r(4.0, 4.0), r(9.0, 0.0), r(9.0, 14.0), r(4.0, 10.0)], GREY, DARK);
}

fn draw(id: u32, size: i32) -> Option<Image> {
    let mut p = Pen::new(size);
    match id {
        AUDIO => {
            page(&mut p);
            speaker(&mut p, 8.0, 9.0, 1.0);
            for (k, x) in [(3.0, 20.0), (6.0, 23.5)] {
                p.line((x, 16.0 - k), (x + 2.0, 16.0), 1.3, NAVY);
                p.line((x + 2.0, 16.0), (x, 16.0 + k), 1.3, NAVY);
            }
        }
        MIDI => {
            page(&mut p);
            for y in [8.0, 11.0, 14.0, 17.0, 20.0] {
                p.line((7.0, y), (25.0, y), 0.7, GREY);
            }
            quaver(&mut p, 11.0, 9.5, 1.0, rgb(0x202060));
            quaver(&mut p, 19.0, 12.5, 1.0, rgb(0x202060));
        }
        VIDEO => {
            p.rect(3.0, 4.0, 29.0, 28.0, rgb(0x303038));
            for y in [6.0, 11.0, 16.0, 21.0, 25.0] {
                p.rect(4.5, y, 7.0, y + 2.0, WHITE);
                p.rect(25.0, y, 27.5, y + 2.0, WHITE);
            }
            p.rect(9.0, 6.0, 23.0, 15.0, rgb(0x60a0e0));
            p.rect(9.0, 17.0, 23.0, 26.0, rgb(0xe0a040));
            p.frame(3.0, 4.0, 29.0, 28.0, DARK);
        }
        IMAGE => {
            p.rect(3.0, 5.0, 29.0, 27.0, rgb(0xa06830));
            p.rect(5.5, 7.5, 26.5, 24.5, rgb(0x90c8f0));
            p.polygon(&[(5.5, 7.5), (5.5, 13.0), (12.0, 19.0), (18.0, 12.0), (22.0, 15.0), (26.5, 11.0), (26.5, 7.5)], rgb(0x40a040), rgb(0x40a040));
            p.disc(21.0, 20.5, 2.5, rgb(0xf0d020), rgb(0xf0d020));
            p.frame(3.0, 5.0, 29.0, 27.0, DARK);
        }
        FOLDER => {
            p.polygon(&[(2.0, 4.0), (30.0, 4.0), (30.0, 24.0), (14.0, 24.0), (12.0, 27.0), (3.0, 27.0), (2.0, 25.0)], rgb(0xf0c040), rgb(0x806020));
            p.rect(3.0, 21.5, 29.0, 22.5, rgb(0xc09030));
            quaver(&mut p, 13.0, 8.0, 1.0, rgb(0x6020a0));
            p.disc(22.0, 13.0, 4.5, rgb(0xc8d8f0), DARK);
            p.disc(22.0, 13.0, 1.2, WHITE, DARK);
        }
        VOLUME => {
            speaker(&mut p, 3.0, 17.0, 0.9);
            p.line((14.0, 20.5), (16.0, 23.5), 1.2, NAVY);
            p.line((16.0, 23.5), (14.0, 26.5), 1.2, NAVY);
            p.rect(3.0, 6.0, 29.0, 10.0, FACE);
            p.frame(3.0, 6.0, 29.0, 10.0, DARK);
            p.rect(4.0, 7.0, 19.0, 9.0, rgb(0x30a050));
            p.rect(17.0, 3.0, 21.0, 13.0, GREY);
            p.frame(17.0, 3.0, 21.0, 13.0, DARK);
        }
        CD_PLAYER => {
            p.disc(14.0, 17.0, 12.5, rgb(0xe0e0e8), DARK);
            p.disc(14.0, 17.0, 8.5, rgb(0xc8d8f0), rgb(0xb0c0e0));
            p.disc(14.0, 17.0, 2.0, WHITE, DARK);
            p.polygon(&[(20.0, 3.0), (20.0, 13.0), (29.0, 8.0)], rgb(0x30a050), DARK);
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
}
