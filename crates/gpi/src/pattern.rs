//! Area fill patterns (PATSYM_*), line types (LINETYPE_*) and marker
//! symbols (MARKSYM_*).

/// An 8x8 pattern, top row first, bit 7 leftmost; 1 = foreground.
pub type Pattern = [u8; 8];

pub const PATSYM_DEFAULT: i32 = 0;
pub const PATSYM_SOLID: i32 = 16;
pub const PATSYM_NOSHADE: i32 = 15;
pub const PATSYM_BLANK: i32 = 64;

/// The standard pattern for a PATSYM_* symbol.
pub fn standard(sym: i32) -> Option<Pattern> {
    Some(match sym {
        0 | 16 => [0xff; 8],
        // DENSE1 (densest) .. DENSE8 (sparsest).
        1 => [0xff, 0xdd, 0xff, 0x77, 0xff, 0xdd, 0xff, 0x77],
        2 => [0xee, 0xbb, 0xee, 0xbb, 0xee, 0xbb, 0xee, 0xbb],
        3 => [0xdd, 0x77, 0xaa, 0x77, 0xdd, 0x77, 0xaa, 0x77],
        4 => [0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55],
        5 => [0x22, 0x88, 0x55, 0x88, 0x22, 0x88, 0x55, 0x88],
        6 => [0x11, 0x44, 0x11, 0x44, 0x11, 0x44, 0x11, 0x44],
        7 => [0x00, 0x22, 0x00, 0x88, 0x00, 0x22, 0x00, 0x88],
        8 => [0x00, 0x20, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00],
        9 => [0x88; 8],                                              // VERT
        10 => [0xff, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00],     // HORIZ
        11 => [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01],     // DIAG1 (\)
        12 => [0xc0, 0x60, 0x30, 0x18, 0x0c, 0x06, 0x03, 0x81],     // DIAG2
        13 => [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80],     // DIAG3 (/)
        14 => [0x03, 0x06, 0x0c, 0x18, 0x30, 0x60, 0xc0, 0x81],     // DIAG4
        15 | 64 => [0x00; 8],                                       // NOSHADE, BLANK
        17 => [0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55],     // HALFTONE
        18 => [0xff, 0x88, 0x88, 0x88, 0xff, 0x88, 0x88, 0x88],     // HATCH
        19 => [0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81],     // DIAGHATCH
        _ => return None,
    })
}

/// Whether the pattern is set at device pixel (x, y), aligned to `org`.
pub fn bit(p: &Pattern, x: i32, y: i32, org: (i32, i32)) -> bool {
    let row = 7 - (y - org.1).rem_euclid(8) as usize;
    let col = (x - org.0).rem_euclid(8);
    p[row] & (0x80 >> col) != 0
}

/// Dash pattern for a LINETYPE_*: alternating on/off run lengths (pels),
/// None for solid.
pub fn line_dashes(t: i32) -> Option<&'static [u8]> {
    Some(match t {
        1 => &[1, 2],                   // DOT
        2 => &[4, 4],                   // SHORTDASH
        3 => &[8, 3, 2, 3],             // DASHDOT
        4 => &[1, 2, 1, 6],             // DOUBLEDOT
        5 => &[12, 4],                  // LONGDASH
        6 => &[8, 3, 1, 3, 1, 3],       // DASHDOUBLEDOT
        8 => &[0, 1],                   // INVISIBLE
        9 => &[1, 1],                   // ALTERNATE
        _ => return None,               // DEFAULT, SOLID
    })
}

/// Whether pel number `n` along a line of type `t` is drawn.
pub fn dash_on(t: i32, n: u32) -> bool {
    let Some(d) = line_dashes(t) else { return true };
    let total: u32 = d.iter().map(|&x| x as u32).sum();
    let mut k = n % total.max(1);
    for (i, &len) in d.iter().enumerate() {
        if k < len as u32 {
            return i % 2 == 0;
        }
        k -= len as u32;
    }
    false
}

/// Marker symbols on a 9x9 grid ('#' set), centred on the point.
pub fn marker(sym: i32) -> Option<[&'static [u8; 9]; 9]> {
    Some(match sym {
        0 | 1 => [
            b"#.......#", b".#.....#.", b"..#...#..", b"...#.#...", b"....#....", b"...#.#...", b"..#...#..", b".#.....#.", b"#.......#",
        ],
        2 => [
            b"....#....", b"....#....", b"....#....", b"....#....", b"#########", b"....#....", b"....#....", b"....#....", b"....#....",
        ],
        3 => [
            b"....#....", b"...#.#...", b"..#...#..", b".#.....#.", b"#.......#", b".#.....#.", b"..#...#..", b"...#.#...", b"....#....",
        ],
        4 => [
            b"#########", b"#.......#", b"#.......#", b"#.......#", b"#.......#", b"#.......#", b"#.......#", b"#.......#", b"#########",
        ],
        5 => [
            b"....#....", b"....#....", b"##..#..##", b"..#####..", b"....#....", b"..#####..", b"##..#..##", b"....#....", b"....#....",
        ],
        6 => [
            b"#...#...#", b".#..#..#.", b"..#.#.#..", b"...###...", b"#########", b"...###...", b"..#.#.#..", b".#..#..#.", b"#...#...#",
        ],
        7 => [
            b"....#....", b"...###...", b"..#####..", b".#######.", b"#########", b".#######.", b"..#####..", b"...###...", b"....#....",
        ],
        8 => [b"#########"; 9],
        9 => [
            b".........", b".........", b".........", b"...###...", b"...###...", b"...###...", b".........", b".........", b".........",
        ],
        10 => [
            b".........", b"..#####..", b".#.....#.", b".#.....#.", b".#.....#.", b".#.....#.", b".#.....#.", b"..#####..", b".........",
        ],
        64 => [b"........."; 9],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes() {
        assert!(dash_on(0, 5));
        assert!(dash_on(1, 0) && !dash_on(1, 1) && !dash_on(1, 2) && dash_on(1, 3));
        assert!(!dash_on(8, 0));
        let p = standard(10).unwrap();
        assert!(bit(&p, 3, 7, (0, 0)));
        assert!(!bit(&p, 3, 6, (0, 0)));
        assert!(standard(99).is_none());
    }
}
