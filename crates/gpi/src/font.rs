//! Bitmap fonts. The built-in font is unscii-16 (8x16, public domain) laid
//! out in code page 850, the system's default.

/// Records of 20 bytes: Unicode code point (u32 LE) and 16 rows.
static UNSCII: &[u8] = include_bytes!("../../../kernel/assets/unscii-16.bin");

const CP850_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐\
└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}";

/// A monospaced bitmap font.
pub struct Font {
    pub name: &'static str,
    pub width: i32,
    pub height: i32,
    /// Pixels below the baseline.
    pub descender: i32,
    glyphs: [[u8; 16]; 256],
}

fn lookup(c: char) -> Option<[u8; 16]> {
    let n = UNSCII.len() / 20;
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let rec = &UNSCII[mid * 20..mid * 20 + 20];
        let cp = u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]);
        match cp.cmp(&(c as u32)) {
            core::cmp::Ordering::Equal => return rec[4..20].try_into().ok(),
            core::cmp::Ordering::Less => lo = mid + 1,
            core::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

/// The graphic characters of code page 850's control positions
/// (0x01-0x1F, as OS/2's fonts have them; Klondike draws its suits with
/// them). BEL to CR stay blank: text passes tabs and line ends.
const CP850_LOW: &str = "\u{263a}\u{263b}\u{2665}\u{2666}\u{2663}\u{2660}       \u{266b}\u{263c}\u{25ba}\u{25c4}\u{2195}\u{203c}\u{b6}\u{a7}\u{25ac}\u{21a8}\u{2191}\u{2193}\u{2192}\u{2190}\u{221f}\u{2194}\u{25b2}\u{25bc}";

fn cp850(b: u8) -> char {
    match b {
        0x01..=0x1f => CP850_LOW.chars().nth((b - 1) as usize).unwrap_or(' '),
        0x20..=0x7e => b as char,
        0x80..=0xff => CP850_HIGH.chars().nth((b - 0x80) as usize).unwrap_or('?'),
        _ => ' ',
    }
}

impl Font {
    fn build() -> Font {
        let mut glyphs = [[0u8; 16]; 256];
        for (b, g) in glyphs.iter_mut().enumerate() {
            *g = lookup(cp850(b as u8)).or_else(|| lookup('?')).unwrap_or([0; 16]);
        }
        Font { name: "System VIO", width: 8, height: 16, descender: 4, glyphs }
    }

    /// Row `r` (0 = top) of the glyph for byte `b`; bit 7 is the leftmost
    /// pixel.
    pub fn row(&self, b: u8, r: usize) -> u8 {
        self.glyphs[b as usize][r]
    }

    pub fn text_width(&self, s: &[u8]) -> i32 {
        self.width * s.len() as i32
    }
}

static mut SYSTEM: Option<Font> = None;

/// The default font (built on first use; not thread safe on the very first
/// call, which PMGPI makes during initialisation).
pub fn system() -> &'static Font {
    unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(SYSTEM);
        slot.get_or_insert_with(Font::build)
    }
}
