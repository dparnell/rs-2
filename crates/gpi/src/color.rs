//! Colours: RGB values and the PM default colour table.

/// 0x00RRGGBB.
pub type Rgb = u32;

// Colour indices of the default logical colour table (CLR_*).
/// All bits 0 / all bits 1 (black and white on the display).
pub const CLR_FALSE: i32 = -5;
pub const CLR_TRUE: i32 = -4;
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
        CLR_WHITE | CLR_TRUE => 0xffffff,
        CLR_BLACK | CLR_DEFAULT | CLR_FALSE => 0x000000,
        0..=15 => DEFAULT_TABLE[i as usize],
        _ => 0x000000,
    }
}

/// Distance between two colours (squared, per channel).
pub fn distance(a: Rgb, b: Rgb) -> u32 {
    let d = |s: u32| {
        let (x, y) = ((a >> s) & 0xff, (b >> s) & 0xff);
        let v = x.abs_diff(y);
        v * v
    };
    d(16) + d(8) + d(0)
}

/// Index of the entry of `table` nearest to `c`.
pub fn nearest(table: &[Rgb], c: Rgb) -> usize {
    (0..table.len()).min_by_key(|&i| distance(table[i], c)).unwrap_or(0)
}

/// Resolver for system colour indices (SYSCLR_*, -50..=-4), supplied by
/// the window system.
pub type SysColors = fn(i32) -> Rgb;

fn no_sys(_: i32) -> Rgb {
    0xcccccc
}

/// LCOLF_* formats of GpiCreateLogColorTable.
pub const LCOLF_DEFAULT: i32 = 0;
pub const LCOLF_INDRGB: i32 = 1;
pub const LCOLF_CONSECRGB: i32 = 2;
pub const LCOLF_RGB: i32 = 3;
pub const LCOL_RESET: u32 = 1;

/// A presentation space's logical colour table: indexed (the default 16
/// colours, optionally redefined or extended) or RGB mode.
#[derive(Clone)]
pub struct ColorTable {
    pub rgb_mode: bool,
    pub entries: alloc::vec::Vec<Rgb>,
    /// A selected palette replaces the table (indices are palette slots).
    pub palette: Option<alloc::vec::Vec<Rgb>>,
    pub sys: SysColors,
}

impl Default for ColorTable {
    fn default() -> Self {
        ColorTable { rgb_mode: false, entries: DEFAULT_TABLE.to_vec(), palette: None, sys: no_sys }
    }
}

impl ColorTable {
    pub fn with_sys(sys: SysColors) -> Self {
        ColorTable { sys, ..ColorTable::default() }
    }

    pub fn reset(&mut self) {
        self.rgb_mode = false;
        self.entries = DEFAULT_TABLE.to_vec();
    }

    /// RGB of a colour value as passed to GpiSetColor.
    pub fn rgb(&self, c: i32) -> Rgb {
        match c {
            CLR_WHITE => 0xffffff,
            CLR_BLACK => 0x000000,
            CLR_DEFAULT => self.rgb(CLR_NEUTRAL),
            CLR_TRUE => 0xffffff,
            CLR_FALSE => 0x000000,
            -50..=-6 => (self.sys)(c),
            _ if c >= 0 && self.rgb_mode => c as u32 & 0xff_ffff,
            _ if c >= 0 => {
                if let Some(p) = &self.palette {
                    return p.get(c as usize).copied().unwrap_or(0);
                }
                // Undefined indices fall back to the default table.
                self.entries.get(c as usize).or_else(|| DEFAULT_TABLE.get(c as usize)).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// GpiCreateLogColorTable.
    pub fn create(&mut self, opts: u32, format: i32, start: i32, data: &[i32]) -> bool {
        if opts & LCOL_RESET != 0 {
            self.reset();
        }
        match format {
            LCOLF_DEFAULT => self.reset(),
            LCOLF_RGB => self.rgb_mode = true,
            LCOLF_CONSECRGB | LCOLF_INDRGB => {
                if start < 0 {
                    return false;
                }
                self.rgb_mode = false;
                let mut set = |i: i32, v: i32| {
                    if !(0..=0xffff).contains(&i) {
                        return;
                    }
                    let i = i as usize;
                    if self.entries.len() <= i {
                        self.entries.resize(i + 1, 0);
                    }
                    self.entries[i] = v as u32 & 0xff_ffff;
                };
                if format == LCOLF_CONSECRGB {
                    for (k, &v) in data.iter().enumerate() {
                        set(start + k as i32, v);
                    }
                } else {
                    for pair in data.chunks(2) {
                        if let [i, v] = pair {
                            set(*i, *v);
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// The defined entries (palette when selected).
    pub fn table(&self) -> &[Rgb] {
        self.palette.as_deref().unwrap_or(&self.entries)
    }

    /// GpiQueryColorIndex: the index nearest to an RGB value.
    pub fn index_of(&self, c: Rgb) -> i32 {
        if self.rgb_mode { (c & 0xff_ffff) as i32 } else { nearest(self.table(), c) as i32 }
    }
}
