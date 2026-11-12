//! rs2's own icons for the Hardware Manager and its device objects
//! (PMWP's WPHwManager and WPDevice classes): drawn from simple shapes
//! like `shellicons`, at 32 x 32 and 16 x 16; nothing here is IBM's.

use gpi::icon::{rgb, Image};

use crate::sysptr::{self, Pen, DARK, FACE, GREY, NAVY, WHITE};

/// The Hardware Manager: a folder with a chip on it.
pub const MANAGER: u32 = 1;
/// A device without a class of its own: an adapter card.
pub const DEVICE: u32 = 2;
/// A processor: a square chip with pins on every side.
pub const CPU: u32 = 3;
/// Memory: a module with chips.
pub const MEMORY: u32 = 4;
/// A bus: a board with slots.
pub const BUS: u32 = 5;
/// A display adapter: a monitor.
pub const DISPLAY: u32 = 6;
/// A keyboard.
pub const KEYBOARD: u32 = 7;
/// A mouse.
pub const MOUSE: u32 = 8;
/// A hard disk or its controller.
pub const HARD_DISK: u32 = 9;
/// A diskette drive: a diskette.
pub const DISKETTE: u32 = 10;
/// A CD-ROM drive.
pub const CD_ROM: u32 = 11;
/// A tape drive: a cassette.
pub const TAPE: u32 = 12;
/// A serial port: a 9-pin connector.
pub const SERIAL: u32 = 13;
/// A parallel port: a 25-pin connector.
pub const PARALLEL: u32 = 14;
/// A system peripheral (interrupt or DMA controller): a chip with arrows.
pub const PERIPHERAL: u32 = 15;
/// Audio: a loudspeaker.
pub const AUDIO: u32 = 16;
/// A timer or clock: a stopwatch.
pub const TIMER: u32 = 17;
pub const LAST: u32 = 17;

const GREEN: u32 = 0xff30_8040;
const CHIP: u32 = 0xff30_3038;
const PIN: u32 = 0xffc0_c0b0;

/// A chip in the box (x0, y0)-(x1, y1) with pins on its long sides
/// (or on all four).
fn chip(p: &mut Pen, x0: f64, y0: f64, x1: f64, y1: f64, all: bool) {
    let n = ((x1 - x0) / 3.0) as i32;
    for i in 0..n {
        let x = x0 + 1.5 + i as f64 * 3.0;
        p.rect(x, y0 - 2.0, x + 1.2, y0, PIN);
        p.rect(x, y1, x + 1.2, y1 + 2.0, PIN);
    }
    if all {
        let n = ((y1 - y0) / 3.0) as i32;
        for i in 0..n {
            let y = y0 + 1.5 + i as f64 * 3.0;
            p.rect(x0 - 2.0, y, x0, y + 1.2, PIN);
            p.rect(x1, y, x1 + 2.0, y + 1.2, PIN);
        }
    }
    p.rect(x0, y0, x1, y1, CHIP);
    p.frame(x0, y0, x1, y1, DARK);
}

/// A connector shell (trapezoid) with `top` and `bottom` pins.
fn connector(p: &mut Pen, top: i32, bottom: i32, fill: u32) {
    p.polygon(&[(2.0, 20.0), (30.0, 20.0), (27.0, 10.0), (5.0, 10.0)], fill, DARK);
    let row = |p: &mut Pen, n: i32, y: f64, x0: f64, x1: f64| {
        let step = (x1 - x0) / (n - 1).max(1) as f64;
        for i in 0..n {
            p.disc(x0 + i as f64 * step, y, 0.9, rgb(0xf0e070), rgb(0x806020));
        }
    };
    row(p, top, 17.0, 6.0, 26.0);
    row(p, bottom, 13.0, 8.0, 24.0);
}

fn hard_disk(p: &mut Pen) {
    p.rect(2.0, 9.0, 30.0, 22.0, FACE);
    p.rect(2.0, 9.0, 30.0, 13.0, rgb(0x9a9aa2));
    p.frame(2.0, 9.0, 30.0, 22.0, DARK);
    p.rect(5.0, 16.0, 20.0, 17.0, DARK);
    p.disc(25.5, 16.5, 1.8, rgb(0x30d040), rgb(0x107020));
}

fn draw(id: u32, size: i32) -> Option<Image> {
    let mut p = Pen::new(size);
    match id {
        MANAGER => {
            p.polygon(&[(2.0, 4.0), (30.0, 4.0), (30.0, 24.0), (14.0, 24.0), (12.0, 27.0), (3.0, 27.0), (2.0, 25.0)], rgb(0xf0c040), rgb(0x806020));
            p.rect(3.0, 21.5, 29.0, 22.5, rgb(0xc09030));
            chip(&mut p, 10.0, 8.0, 22.0, 17.0, true);
        }
        DEVICE => {
            p.rect(3.0, 8.0, 29.0, 26.0, GREEN);
            p.frame(3.0, 8.0, 29.0, 26.0, DARK);
            for i in 0..8 {
                let x = 6.0 + i as f64 * 2.8;
                p.rect(x, 4.0, x + 1.8, 8.0, rgb(0xe0c040));
            }
            chip(&mut p, 7.0, 14.0, 17.0, 21.0, false);
            p.rect(20.0, 13.0, 26.0, 22.0, CHIP);
            p.rect(0.0, 22.0, 3.0, 28.0, GREY);
        }
        CPU => {
            chip(&mut p, 6.0, 6.0, 26.0, 26.0, true);
            p.rect(11.0, 11.0, 21.0, 21.0, rgb(0xb0b0b8));
            p.frame(11.0, 11.0, 21.0, 21.0, DARK);
        }
        MEMORY => {
            p.rect(1.0, 10.0, 31.0, 22.0, GREEN);
            p.frame(1.0, 10.0, 31.0, 22.0, DARK);
            for i in 0..4 {
                let x = 3.0 + i as f64 * 7.0;
                p.rect(x, 13.0, x + 5.0, 20.0, CHIP);
            }
            for i in 0..10 {
                let x = 2.5 + i as f64 * 2.9;
                p.rect(x, 8.0, x + 1.6, 10.0, rgb(0xe0c040));
            }
        }
        BUS => {
            p.rect(2.0, 3.0, 30.0, 29.0, GREEN);
            p.frame(2.0, 3.0, 30.0, 29.0, DARK);
            for i in 0..4 {
                let y = 6.0 + i as f64 * 6.0;
                p.rect(6.0, y, 26.0, y + 3.0, rgb(0xf0f0e8));
                p.frame(6.0, y, 26.0, y + 3.0, DARK);
            }
        }
        DISPLAY => {
            p.rect(2.0, 9.0, 30.0, 29.0, FACE);
            p.frame(2.0, 9.0, 30.0, 29.0, DARK);
            p.rect(5.0, 12.0, 27.0, 26.0, rgb(0x2e6e8e));
            p.rect(13.0, 5.0, 19.0, 9.0, GREY);
            p.rect(8.0, 3.0, 24.0, 5.0, FACE);
            p.frame(8.0, 3.0, 24.0, 5.0, DARK);
        }
        KEYBOARD => {
            p.rect(1.0, 8.0, 31.0, 24.0, FACE);
            p.frame(1.0, 8.0, 31.0, 24.0, DARK);
            for r in 0..3 {
                for c in 0..7 {
                    let (x, y) = (3.5 + c as f64 * 3.8, 19.0 - r as f64 * 3.5);
                    p.rect(x, y, x + 2.8, y + 2.5, WHITE);
                }
            }
            p.rect(8.0, 9.5, 24.0, 12.0, WHITE);
        }
        MOUSE => {
            p.line((16.0, 26.0), (16.0, 31.0), 1.2, DARK);
            p.disc(16.0, 15.0, 10.5, FACE, DARK);
            p.line((16.0, 25.0), (16.0, 17.0), 1.0, DARK);
            p.line((6.5, 17.0), (25.5, 17.0), 1.0, DARK);
        }
        HARD_DISK => hard_disk(&mut p),
        DISKETTE => {
            p.polygon(&[(4.0, 3.0), (28.0, 3.0), (28.0, 25.0), (25.0, 28.0), (4.0, 28.0)], rgb(0x303060), DARK);
            p.rect(10.0, 19.0, 22.0, 28.0, rgb(0xc0c0c8));
            p.rect(17.0, 21.0, 20.0, 26.5, rgb(0x303060));
            p.rect(8.0, 4.0, 24.0, 15.0, WHITE);
            for y in [7.0, 10.0, 13.0] {
                p.line((10.0, y), (22.0, y), 0.6, GREY);
            }
        }
        CD_ROM => {
            p.disc(16.0, 16.0, 14.0, rgb(0xe0e0e8), DARK);
            p.disc(16.0, 16.0, 10.0, rgb(0xc8d8f0), rgb(0xb0c0e0));
            p.disc(16.0, 16.0, 5.0, rgb(0xa0a0a8), DARK);
            p.disc(16.0, 16.0, 2.2, WHITE, DARK);
        }
        TAPE => {
            p.rect(2.0, 7.0, 30.0, 25.0, rgb(0x404048));
            p.frame(2.0, 7.0, 30.0, 25.0, DARK);
            p.rect(5.0, 14.0, 27.0, 22.0, WHITE);
            p.disc(10.5, 18.0, 3.0, rgb(0x806040), DARK);
            p.disc(21.5, 18.0, 3.0, rgb(0x806040), DARK);
            p.polygon(&[(8.0, 7.0), (24.0, 7.0), (22.0, 11.0), (10.0, 11.0)], GREY, DARK);
        }
        SERIAL => connector(&mut p, 5, 4, rgb(0x5080c0)),
        PARALLEL => connector(&mut p, 9, 8, rgb(0xc060a0)),
        PERIPHERAL => {
            chip(&mut p, 9.0, 9.0, 23.0, 23.0, true);
            p.line((2.0, 28.0), (8.0, 22.0), 1.5, NAVY);
            p.polygon(&[(8.0, 22.0), (8.0, 25.5), (4.5, 22.0)], NAVY, NAVY);
            p.line((30.0, 4.0), (24.0, 10.0), 1.5, NAVY);
            p.polygon(&[(24.0, 10.0), (24.0, 6.5), (27.5, 10.0)], NAVY, NAVY);
        }
        AUDIO => {
            p.rect(4.0, 11.0, 10.0, 21.0, GREY);
            p.frame(4.0, 11.0, 10.0, 21.0, DARK);
            p.polygon(&[(10.0, 11.0), (18.0, 4.0), (18.0, 28.0), (10.0, 21.0)], GREY, DARK);
            for (k, x) in [(4.0, 22.0), (7.0, 26.0)] {
                p.line((x, 16.0 - k), (x + 2.5, 16.0), 1.4, NAVY);
                p.line((x + 2.5, 16.0), (x, 16.0 + k), 1.4, NAVY);
            }
        }
        TIMER => {
            p.rect(14.0, 26.0, 18.0, 30.0, GREY);
            p.disc(16.0, 14.0, 12.5, WHITE, DARK);
            for k in 0..12 {
                let a = k as f64 * core::f64::consts::PI / 6.0;
                let (s, c) = (libm_sin(a), libm_cos(a));
                p.line((16.0 + 9.5 * s, 14.0 + 9.5 * c), (16.0 + 11.0 * s, 14.0 + 11.0 * c), 0.9, DARK);
            }
            p.line((16.0, 14.0), (16.0, 22.0), 1.4, rgb(0xc03028));
            p.line((16.0, 14.0), (21.0, 11.0), 1.4, DARK);
        }
        _ => return None,
    }
    Some(p.img)
}

/// sin and cos without the standard library (Taylor series; |a| < 2π).
fn libm_sin(a: f64) -> f64 {
    let mut x = a;
    while x > core::f64::consts::PI {
        x -= 2.0 * core::f64::consts::PI;
    }
    let (mut term, mut sum) = (x, x);
    for n in 1..10 {
        term *= -x * x / ((2 * n) as f64 * (2 * n + 1) as f64);
        sum += term;
    }
    sum
}

fn libm_cos(a: f64) -> f64 {
    libm_sin(a + core::f64::consts::FRAC_PI_2)
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
