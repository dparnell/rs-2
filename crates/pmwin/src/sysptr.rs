//! rs2's own system pointers and icons (WinQuerySysPointer's SPTR_*):
//! the mouse pointers as small black shapes with a white outline, the
//! message box and file icons drawn from simple shapes, each at 32 x 32
//! and as a 16 x 16 mini icon. Nothing here is copied from IBM's.

use alloc::vec::Vec;

use gpi::icon::{rgb, Image, TRANSPARENT};
use gpi::Point;

pub const SPTR_ARROW: u32 = 1;
pub const SPTR_TEXT: u32 = 2;
pub const SPTR_WAIT: u32 = 3;
pub const SPTR_SIZE: u32 = 4;
pub const SPTR_MOVE: u32 = 5;
pub const SPTR_SIZENWSE: u32 = 6;
pub const SPTR_SIZENESW: u32 = 7;
pub const SPTR_SIZEWE: u32 = 8;
pub const SPTR_SIZENS: u32 = 9;
pub const SPTR_APPICON: u32 = 10;
pub const SPTR_ICONINFORMATION: u32 = 11;
pub const SPTR_ICONQUESTION: u32 = 12;
pub const SPTR_ICONERROR: u32 = 13;
pub const SPTR_ICONWARNING: u32 = 14;
pub const SPTR_ILLEGAL: u32 = 18;
pub const SPTR_FILE: u32 = 19;
pub const SPTR_FOLDER: u32 = 20;
pub const SPTR_MULTFILE: u32 = 21;
pub const SPTR_PROGRAM: u32 = 22;
/// Last system pointer id (SPTR_CPTR).
pub const SPTR_CPTR: u32 = 39;

/// A system pointer or icon.
pub struct SysPointer {
    pub image: Image,
    pub mini: Image,
    /// Hot spot of `image`, from its bottom-left corner.
    pub hot: Point,
    pub pointer: bool,
}

pub(crate) const BLACK: u32 = 0xff00_0000;
pub(crate) const WHITE: u32 = 0xffff_ffff;

/// Size of system pointers and icons.
pub const SIZE: i32 = 32;
pub const MINI: i32 = 16;

/// A pointer from art: '#' black, '.' white, anything else transparent;
/// with `outline`, transparent pixels next to black ones turn white.
/// Placed at the top-left of a 32 x 32 image (`at` = offset of the art's
/// top-left corner from the image's top-left).
fn art(rows: &[&str], at: (i32, i32), outline: bool) -> Image {
    let mut img = Image::new(SIZE, SIZE);
    for (j, r) in rows.iter().enumerate() {
        for (i, c) in r.bytes().enumerate() {
            let v = match c {
                b'#' => BLACK,
                b'.' => WHITE,
                _ => continue,
            };
            img.set(at.0 + i as i32, SIZE - 1 - (at.1 + j as i32), v);
        }
    }
    if outline {
        let mut out = img.clone();
        for y in 0..SIZE {
            for x in 0..SIZE {
                if img.at(x, y) != TRANSPARENT {
                    continue;
                }
                let near = (-1..=1).any(|dy| (-1..=1).any(|dx| img.at(x + dx, y + dy) == BLACK));
                if near {
                    out.set(x, y, WHITE);
                }
            }
        }
        img = out;
    }
    img
}

/// A pointer centred in the image with its hot spot at the art's centre.
fn centred(rows: &[&str]) -> (Image, Point) {
    let (w, h) = (rows.iter().map(|r| r.len()).max().unwrap_or(0) as i32, rows.len() as i32);
    let at = ((SIZE - w) / 2, (SIZE - h) / 2);
    (art(rows, at, true), Point::new(at.0 + w / 2, SIZE - 1 - (at.1 + h / 2)))
}

const ARROW: [&str; 19] = [
    "#           ",
    "##          ",
    "#.#         ",
    "#..#        ",
    "#...#       ",
    "#....#      ",
    "#.....#     ",
    "#......#    ",
    "#.......#   ",
    "#........#  ",
    "#.........# ",
    "#......#####",
    "#...#..#    ",
    "#..# #..#   ",
    "#.#  #..#   ",
    "##    #..#  ",
    "#     #..#  ",
    "       #..# ",
    "        ##  ",
];

const TEXT: [&str; 16] = [
    "### ###", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ",
    "   #   ", "   #   ", "### ###",
];

const WAIT: [&str; 17] = [
    "#############",
    " #.........# ",
    " #.........# ",
    "  #.......#  ",
    "  #.#####.#  ",
    "   #.###.#   ",
    "    #.#.#    ",
    "     #.#     ",
    "     #.#     ",
    "    #...#    ",
    "   #..#..#   ",
    "  #...#...#  ",
    "  #..###..#  ",
    " #..#####..# ",
    " #.#######.# ",
    " ########### ",
    "#############",
];

const MOVE: [&str; 19] = [
    "         #         ",
    "        ###        ",
    "       #####       ",
    "      #######      ",
    "         #         ",
    "         #         ",
    "   #     #     #   ",
    "  ##     #     ##  ",
    " ###     #     ### ",
    "###################",
    " ###     #     ### ",
    "  ##     #     ##  ",
    "   #     #     #   ",
    "         #         ",
    "         #         ",
    "      #######      ",
    "       #####       ",
    "        ###        ",
    "         #         ",
];

const SIZENS: [&str; 19] = [
    "   #   ", "  ###  ", " ##### ", "#######", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ", "   #   ",
    "   #   ", "   #   ", "#######", " ##### ", "  ###  ", "   #   ",
];

const SIZENWSE: [&str; 15] = [
    "######         ",
    "#####          ",
    "####           ",
    "#####          ",
    "## ###         ",
    "#   ###        ",
    "     ###       ",
    "      ###      ",
    "       ###     ",
    "        ###   #",
    "         ### ##",
    "          #####",
    "           ####",
    "          #####",
    "         ######",
];

fn transpose(rows: &[&str]) -> Vec<alloc::string::String> {
    let w = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    (0..w).map(|i| rows.iter().map(|r| r.as_bytes().get(i).copied().unwrap_or(b' ') as char).collect()).collect()
}

fn mirror(rows: &[&str]) -> Vec<alloc::string::String> {
    rows.iter().map(|r| r.chars().rev().collect()).collect()
}

fn strs(v: &[alloc::string::String]) -> Vec<&str> {
    v.iter().map(|s| s.as_str()).collect()
}

// ---- Shapes for icons, drawn at any size (coordinates in 1/32 of it) ----

pub(crate) struct Pen {
    pub(crate) img: Image,
    /// Pixels per unit of the 32-unit design grid.
    k: f64,
}

impl Pen {
    pub(crate) fn new(size: i32) -> Pen {
        Pen { img: Image::new(size, size), k: size as f64 / 32.0 }
    }

    /// Fill the design-grid rectangle [x0, x1) x [y0, y1) (y up).
    pub(crate) fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, c: u32) {
        let k = self.k;
        let (a, b, cc, d) = ((x0 * k) as i32, (y0 * k) as i32, (x1 * k + 0.5) as i32, (y1 * k + 0.5) as i32);
        for y in b..d.max(b + 1) {
            for x in a..cc.max(a + 1) {
                self.img.set(x, y, c);
            }
        }
    }

    /// One-pixel outline of a design-grid rectangle.
    pub(crate) fn frame(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, c: u32) {
        let k = self.k;
        let (a, b, cc, d) = ((x0 * k) as i32, (y0 * k) as i32, ((x1 * k + 0.5) as i32 - 1), ((y1 * k + 0.5) as i32 - 1));
        for x in a..=cc {
            self.img.set(x, b, c);
            self.img.set(x, d, c);
        }
        for y in b..=d {
            self.img.set(a, y, c);
            self.img.set(cc, y, c);
        }
    }

    /// A filled disc with a one-pixel border.
    pub(crate) fn disc(&mut self, cx: f64, cy: f64, r: f64, fill: u32, border: u32) {
        let n = self.img.w;
        for y in 0..n {
            for x in 0..n {
                let (px, py) = ((x as f64 + 0.5) / self.k - cx, (y as f64 + 0.5) / self.k - cy);
                let d2 = px * px + py * py;
                if d2 <= r * r {
                    let inner = r - 1.0 / self.k;
                    self.img.set(x, y, if d2 >= inner * inner { border } else { fill });
                }
            }
        }
    }

    /// A filled polygon (design grid, even-odd), then its outline.
    pub(crate) fn polygon(&mut self, pts: &[(f64, f64)], fill: u32, border: u32) {
        let n = self.img.w;
        let inside = |x: f64, y: f64| {
            let mut c = false;
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                    c = !c;
                }
            }
            c
        };
        let mut mask = alloc::vec![false; (n * n) as usize];
        for y in 0..n {
            for x in 0..n {
                mask[(y * n + x) as usize] = inside((x as f64 + 0.5) / self.k, (y as f64 + 0.5) / self.k);
            }
        }
        for y in 0..n {
            for x in 0..n {
                if !mask[(y * n + x) as usize] {
                    continue;
                }
                let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dy)| {
                    let (a, b) = (x + dx, y + dy);
                    a < 0 || b < 0 || a >= n || b >= n || !mask[(b * n + a) as usize]
                });
                self.img.set(x, y, if edge { border } else { fill });
            }
        }
    }

    /// A thick line between design-grid points.
    pub(crate) fn line(&mut self, a: (f64, f64), b: (f64, f64), width: f64, c: u32) {
        let n = self.img.w;
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        for y in 0..n {
            for x in 0..n {
                let (px, py) = ((x as f64 + 0.5) / self.k, (y as f64 + 0.5) / self.k);
                let t = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
                let (qx, qy) = (a.0 + t * dx - px, a.1 + t * dy - py);
                if qx * qx + qy * qy <= width * width / 4.0 {
                    self.img.set(x, y, c);
                }
            }
        }
    }

    /// A character of the system font, its inked part scaled to `h`
    /// design units tall and centred on (cx, cy).
    pub(crate) fn glyph(&mut self, ch: u8, cx: f64, cy: f64, h: f64, c: u32) {
        let f = gpi::font::vio();
        let rows: Vec<u8> = (0..16).map(|r| f.row(ch, r)).collect();
        let top = rows.iter().position(|&r| r != 0).unwrap_or(0);
        let bot = rows.iter().rposition(|&r| r != 0).unwrap_or(15);
        let left = rows.iter().map(|r| r.leading_zeros()).min().unwrap_or(0) as i32;
        let right = 8 - rows.iter().map(|r| r.trailing_zeros()).min().unwrap_or(0) as i32;
        let (gw, gh) = ((right - left).max(1) as f64, (bot - top + 1) as f64);
        let s = h / gh;
        let (w, hh) = (gw * s, gh * s);
        let (x0, y0) = (cx - w / 2.0, cy - hh / 2.0);
        let n = self.img.w;
        for y in 0..n {
            for x in 0..n {
                let (px, py) = ((x as f64 + 0.5) / self.k, (y as f64 + 0.5) / self.k);
                if px < x0 || py < y0 || px >= x0 + w || py >= y0 + hh {
                    continue;
                }
                let gx = left + ((px - x0) / s) as i32;
                let gy = bot as i32 - ((py - y0) / s) as i32;
                if (0..8).contains(&gx) && (0..16).contains(&gy) && rows[gy as usize] & (0x80 >> gx) != 0 {
                    self.img.set(x, y, c);
                }
            }
        }
    }
}

pub(crate) const NAVY: u32 = 0xff00_0080;
pub(crate) const GREY: u32 = 0xff80_8080;
pub(crate) const FACE: u32 = 0xffcc_cccc;
pub(crate) const DARK: u32 = 0xff40_4040;

pub(crate) fn icon(id: u32, size: i32) -> Option<Image> {
    let mut p = Pen::new(size);
    match id {
        SPTR_ICONINFORMATION => {
            p.disc(16.0, 16.0, 14.5, rgb(0x2050c0), DARK);
            p.glyph(b'i', 16.0, 16.0, 18.0, WHITE);
        }
        SPTR_ICONQUESTION => {
            p.disc(16.0, 16.0, 14.5, rgb(0x20a060), DARK);
            p.glyph(b'?', 16.0, 16.0, 18.0, WHITE);
        }
        SPTR_ICONERROR => {
            let o = [(10.0, 2.0), (22.0, 2.0), (30.0, 10.0), (30.0, 22.0), (22.0, 30.0), (10.0, 30.0), (2.0, 22.0), (2.0, 10.0)];
            p.polygon(&o, rgb(0xd02020), DARK);
            p.line((10.0, 10.0), (22.0, 22.0), 4.0, WHITE);
            p.line((10.0, 22.0), (22.0, 10.0), 4.0, WHITE);
        }
        SPTR_ICONWARNING => {
            p.polygon(&[(16.0, 31.0), (31.0, 2.0), (1.0, 2.0)], rgb(0xffd020), DARK);
            p.glyph(b'!', 16.0, 13.0, 16.0, BLACK);
        }
        SPTR_APPICON | SPTR_PROGRAM => {
            p.rect(2.0, 4.0, 30.0, 28.0, FACE);
            p.frame(2.0, 4.0, 30.0, 28.0, DARK);
            p.rect(3.0, 23.0, 29.0, 27.0, NAVY);
            if id == SPTR_PROGRAM {
                p.rect(4.0, 6.0, 28.0, 22.0, BLACK);
                p.glyph(b'>', 9.0, 14.0, 8.0, rgb(0xc0c0c0));
                p.rect(13.0, 10.0, 18.0, 11.5, rgb(0xc0c0c0));
            } else {
                p.rect(4.0, 6.0, 28.0, 22.0, WHITE);
            }
        }
        SPTR_FILE | SPTR_MULTFILE => {
            let page = |p: &mut Pen, dx: f64, dy: f64| {
                let pts = [(6.0 + dx, 2.0 + dy), (24.0 + dx, 2.0 + dy), (24.0 + dx, 22.0 + dy), (18.0 + dx, 28.0 + dy), (6.0 + dx, 28.0 + dy)];
                p.polygon(&pts, WHITE, DARK);
                p.line((18.0 + dx, 28.0 + dy), (18.0 + dx, 22.0 + dy), 1.0, DARK);
                p.line((18.0 + dx, 22.0 + dy), (24.0 + dx, 22.0 + dy), 1.0, DARK);
                for k in 0..4 {
                    let y = 7.0 + dy + k as f64 * 4.0;
                    p.rect(9.0 + dx, y, 21.0 + dx, y + 1.0, GREY);
                }
            };
            if id == SPTR_MULTFILE {
                page(&mut p, 2.0, 2.0);
                page(&mut p, -2.0, -2.0);
            } else {
                page(&mut p, 1.0, 0.0);
            }
        }
        SPTR_FOLDER => {
            p.polygon(&[(2.0, 4.0), (30.0, 4.0), (30.0, 24.0), (14.0, 24.0), (12.0, 27.0), (3.0, 27.0), (2.0, 25.0)], rgb(0xf0c040), rgb(0x806020));
            p.rect(3.0, 21.5, 29.0, 22.5, rgb(0xc09030));
        }
        SPTR_ILLEGAL => {
            p.disc(16.0, 16.0, 11.0, rgb(0xd02020), DARK);
            p.disc(16.0, 16.0, 7.5, WHITE, WHITE);
            p.line((10.5, 21.5), (21.5, 10.5), 3.5, rgb(0xd02020));
        }
        _ => return None,
    }
    Some(p.img)
}

/// System pointer or icon `id` (SPTR_*).
pub fn system(id: u32) -> Option<SysPointer> {
    let pointer = |img: Image, hot: Point| {
        let mini = img.scaled(MINI, MINI);
        Some(SysPointer { image: img, mini, hot, pointer: true })
    };
    match id {
        SPTR_ARROW => pointer(art(&ARROW, (0, 0), false), Point::new(0, SIZE - 1)),
        SPTR_TEXT => {
            let (i, h) = centred(&TEXT);
            pointer(i, h)
        }
        SPTR_WAIT => {
            let (i, h) = centred(&WAIT);
            pointer(i, h)
        }
        SPTR_SIZE | SPTR_MOVE => {
            let (i, h) = centred(&MOVE);
            pointer(i, h)
        }
        SPTR_SIZENS => {
            let (i, h) = centred(&SIZENS);
            pointer(i, h)
        }
        SPTR_SIZEWE => {
            let t = transpose(&SIZENS);
            let (i, h) = centred(&strs(&t));
            pointer(i, h)
        }
        SPTR_SIZENWSE => {
            let (i, h) = centred(&SIZENWSE);
            pointer(i, h)
        }
        SPTR_SIZENESW => {
            let m = mirror(&SIZENWSE);
            let (i, h) = centred(&strs(&m));
            pointer(i, h)
        }
        _ => {
            let image = icon(id, SIZE)?;
            let mini = icon(id, MINI)?;
            let pointer = id == SPTR_ILLEGAL;
            Some(SysPointer { image, mini, hot: Point::new(SIZE / 2, SIZE / 2), pointer })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_defined() {
        for id in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 18, 19, 20, 21, 22] {
            let p = system(id).unwrap_or_else(|| panic!("SPTR {id}"));
            assert_eq!((p.image.w, p.image.h), (SIZE, SIZE));
            assert_eq!((p.mini.w, p.mini.h), (MINI, MINI));
            assert!(!p.image.is_blank() && !p.mini.is_blank(), "SPTR {id} blank");
            assert!(p.hot.x >= 0 && p.hot.x < SIZE && p.hot.y >= 0 && p.hot.y < SIZE);
        }
        assert!(system(15).is_none());
    }

    #[test]
    fn arrow_hot_spot_is_its_tip() {
        let p = system(SPTR_ARROW).unwrap();
        assert_eq!(p.hot, Point::new(0, 31));
        assert_eq!(p.image.at(0, 31), BLACK);
    }

    #[test]
    fn pointers_have_white_outlines() {
        let p = system(SPTR_TEXT).unwrap();
        let (x, y) = (p.hot.x, p.hot.y);
        assert_eq!(p.image.at(x, y), BLACK);
        assert_eq!(p.image.at(x - 1, y), WHITE);
        assert_eq!(p.image.at(x - 3, y), TRANSPARENT);
    }

    #[test]
    fn message_icons_differ() {
        let a = system(SPTR_ICONINFORMATION).unwrap().image;
        let b = system(SPTR_ICONERROR).unwrap().image;
        assert_ne!(a, b);
        // Corners are transparent, the middle is not.
        assert_eq!(a.at(0, 0), TRANSPARENT);
        assert_ne!(b.at(16, 16), TRANSPARENT);
    }

    /// `cargo test -p pmwin sysptr -- --ignored`: every system pointer
    /// and icon (and its mini version) on a grey sheet, magnified, as a
    /// PPM in the system's temporary directory.
    #[test]
    #[ignore]
    fn write_sheet() {
        extern crate std;
        let ids = [1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 18, 19, 20, 21, 22];
        let z = 3;
        let cell = 34 * z + 18 * z;
        let (w, h) = (cell * ids.len() as i32, 36 * z);
        let mut px = std::vec![0x00a0_a0a0u32; (w * h) as usize];
        for (n, &id) in ids.iter().enumerate() {
            let p = system(id).unwrap();
            for (img, x0) in [(&p.image, n as i32 * cell), (&p.mini, n as i32 * cell + 34 * z)] {
                for y in 0..img.h {
                    for x in 0..img.w {
                        let c = match img.at(x, y) {
                            TRANSPARENT => continue,
                            gpi::icon::INVERT => 0xff00ff,
                            v => v & 0xffffff,
                        };
                        for dy in 0..z {
                            for dx in 0..z {
                                let (sx, sy) = (x0 + x * z + dx, (img.h - 1 - y) * z + dy + z);
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
        std::fs::write(std::env::temp_dir().join("rs2-sysptr.ppm"), out).unwrap();
    }
}
