//! Code page to Unicode tables for the text screen.

/// Glyphs for control-code positions 0x00-0x1F and 0x7F, shared by the PC
/// code pages when characters are displayed rather than interpreted.
const LOW: &str = " ☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼";

const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐\
└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

const CP850_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐\
└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodePage {
    Cp437,
    Cp850,
}

impl CodePage {
    pub fn from_number(n: u16) -> Option<Self> {
        match n {
            437 => Some(Self::Cp437),
            850 => Some(Self::Cp850),
            _ => None,
        }
    }

    pub fn number(self) -> u16 {
        match self {
            Self::Cp437 => 437,
            Self::Cp850 => 850,
        }
    }

    /// The character displayed for byte `b`.
    pub fn glyph(self, b: u8) -> char {
        match b {
            0x00..=0x1f => LOW.chars().nth(b as usize).unwrap_or(' '),
            0x7f => '⌂',
            0x20..=0x7e => b as char,
            _ => {
                let t = match self {
                    Self::Cp437 => CP437_HIGH,
                    Self::Cp850 => CP850_HIGH,
                };
                t.chars().nth((b - 0x80) as usize).unwrap_or('?')
            }
        }
    }

    /// Map a Unicode character back to this code page.
    pub fn from_char(self, c: char) -> Option<u8> {
        if (' '..='~').contains(&c) {
            return Some(c as u8);
        }
        (0..=255u8).find(|&b| self.glyph(b) == c)
    }
}
