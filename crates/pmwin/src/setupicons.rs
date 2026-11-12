//! rs2's own icons for the System Setup objects that IBM keeps in
//! WPCONFIG.DLL and WINCFG.DLL: Country, Power, Special Needs and
//! WIN-OS/2 Setup. Drawn from simple shapes in the style of `shellicons`,
//! at 32 x 32 and 16 x 16; nothing here is copied from IBM's.

use gpi::icon::{rgb, Image};

use crate::sysptr::{self, Pen, DARK, FACE, GREY, NAVY, WHITE};

pub const COUNTRY: u32 = 1;
pub const POWER: u32 = 2;
pub const SPECIAL_NEEDS: u32 = 3;
pub const WINOS2: u32 = 4;
pub const LAST: u32 = 4;

fn draw(id: u32, size: i32) -> Option<Image> {
    let mut p = Pen::new(size);
    match id {
        COUNTRY => {
            // A globe: blue sea, green land, the meridians and the equator.
            let sea = rgb(0x3070c8);
            p.disc(16.0, 16.0, 14.0, sea, DARK);
            let land = rgb(0x40a048);
            p.polygon(&[(8.0, 22.0), (13.0, 26.0), (17.0, 24.0), (15.0, 19.0), (11.0, 17.0), (7.0, 18.0)], land, land);
            p.polygon(&[(18.0, 15.0), (24.0, 17.0), (26.0, 12.0), (22.0, 6.0), (18.0, 9.0)], land, land);
            p.polygon(&[(9.0, 12.0), (12.0, 13.0), (12.0, 8.0), (10.0, 7.0)], land, land);
            let wire = rgb(0xd8e8f8);
            p.line((2.5, 16.0), (29.5, 16.0), 0.8, wire);
            p.line((16.0, 2.5), (16.0, 29.5), 0.8, wire);
            for (a, b) in [((10.0, 3.8), (10.0, 28.2)), ((22.0, 3.8), (22.0, 28.2))] {
                p.line(a, b, 0.7, wire);
            }
        }
        POWER => {
            // A plug with its cable, and a lightning bolt beside it.
            p.rect(6.0, 12.0, 18.0, 24.0, FACE);
            p.frame(6.0, 12.0, 18.0, 24.0, DARK);
            p.rect(8.0, 24.0, 10.0, 30.0, GREY);
            p.rect(14.0, 24.0, 16.0, 30.0, GREY);
            p.line((12.0, 12.0), (12.0, 6.0), 2.0, DARK);
            p.line((12.0, 6.0), (4.0, 3.0), 2.0, DARK);
            let bolt = rgb(0xf0c020);
            p.polygon(&[(24.0, 30.0), (19.0, 17.0), (23.0, 17.0), (20.0, 4.0), (29.0, 20.0), (25.0, 20.0), (28.0, 30.0)], bolt, rgb(0x806010));
        }
        SPECIAL_NEEDS => {
            // A key with a hand above it: the keyboard made easier.
            p.rect(4.0, 3.0, 28.0, 13.0, FACE);
            p.frame(4.0, 3.0, 28.0, 13.0, DARK);
            for k in 0..4 {
                let x = 6.0 + k as f64 * 5.5;
                p.rect(x, 5.0, x + 4.5, 7.5, WHITE);
                p.rect(x, 9.0, x + 4.5, 11.5, WHITE);
            }
            let skin = rgb(0xf0c8a0);
            let edge = rgb(0x906040);
            p.polygon(&[(12.0, 14.0), (22.0, 14.0), (23.0, 22.0), (21.0, 22.0), (21.0, 29.0), (18.0, 29.0), (18.0, 24.0), (16.5, 24.0), (16.5, 30.0), (13.5, 30.0), (13.5, 21.0), (10.0, 23.0), (9.0, 21.0)], skin, edge);
            p.disc(26.0, 26.0, 4.0, rgb(0x2050c0), DARK);
            p.glyph(b'i', 26.0, 26.0, 5.0, WHITE);
        }
        WINOS2 => {
            // A window in a window: a Windows program on the OS/2 desktop.
            p.rect(2.0, 3.0, 30.0, 29.0, FACE);
            p.frame(2.0, 3.0, 30.0, 29.0, DARK);
            p.rect(3.0, 25.0, 29.0, 28.0, NAVY);
            p.rect(3.0, 4.0, 29.0, 25.0, rgb(0x2e6e8e));
            p.rect(8.0, 7.0, 27.0, 22.0, WHITE);
            p.frame(8.0, 7.0, 27.0, 22.0, DARK);
            p.rect(9.0, 19.0, 26.0, 21.5, rgb(0x0000a8));
            p.glyph(b'W', 17.5, 13.0, 8.0, rgb(0x0000a8));
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
