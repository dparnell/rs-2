//! Colours: RGB values and the PM default colour table.

/// 0x00RRGGBB.
pub type Rgb = u32;

// Colour indices of the default logical colour table (CLR_*).
pub const CLR_DEFAULT: i32 = -3;
pub const CLR_WHITE: i32 = -2;
pub const CLR_BLACK: i32 = -1;
pub const CLR_BACKGROUND: i32 = 0;
pub const CLR_BLUE: i32 = 1;
pub const CLR_RED: i32 = 2;
pub const CLR_PINK: i32 = 3;
pub const CLR_GREEN: i32 = 4;
pub const CLR_CYAN: i32 = 5;
pub const CLR_YELLOW: i32 = 6;
pub const CLR_NEUTRAL: i32 = 7;
pub const CLR_DARKGRAY: i32 = 8;
pub const CLR_DARKBLUE: i32 = 9;
pub const CLR_DARKRED: i32 = 10;
pub const CLR_DARKPINK: i32 = 11;
pub const CLR_DARKGREEN: i32 = 12;
pub const CLR_DARKCYAN: i32 = 13;
pub const CLR_BROWN: i32 = 14;
pub const CLR_PALEGRAY: i32 = 15;

/// RGB of the default colour table entries 0..=15.
const DEFAULT_TABLE: [Rgb; 16] = [
    0xffffff, 0x0000ff, 0xff0000, 0xff00ff, 0x00ff00, 0x00ffff, 0xffff00, 0x000000, 0x808080, 0x000080, 0x800000,
    0x800080, 0x008000, 0x008080, 0x808000, 0xcccccc,
];

/// RGB for a colour index (CLR_NEUTRAL and CLR_DEFAULT are the foreground,
/// black; CLR_BACKGROUND white).
pub fn index_to_rgb(i: i32) -> Rgb {
    match i {
        CLR_WHITE => 0xffffff,
        CLR_BLACK | CLR_DEFAULT => 0x000000,
        0..=15 => DEFAULT_TABLE[i as usize],
        _ => 0x000000,
    }
}
