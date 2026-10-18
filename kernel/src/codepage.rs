//! Code page to Unicode tables (generated into `nls_data.rs` by
//! tools/scripts/gen_nls.py) for the text screen, the keyboard and NLS.

use crate::nls_data::CODEPAGES;

/// A single-byte code page with a Unicode table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodePage(u8);

#[allow(non_upper_case_globals)]
impl CodePage {
    pub const Cp437: CodePage = CodePage(0);
    pub const Cp850: CodePage = CodePage(1);

    pub fn from_number(n: u16) -> Option<Self> {
        CODEPAGES.iter().position(|e| e.0 == n).map(|i| CodePage(i as u8))
    }

    pub fn number(self) -> u16 {
        CODEPAGES[self.0 as usize].0
    }

    /// The character displayed for byte `b` (control positions show the
    /// PC graphic characters).
    pub fn glyph(self, b: u8) -> char {
        char::from_u32(CODEPAGES[self.0 as usize].1[b as usize] as u32).unwrap_or('?')
    }

    /// The character byte `b` stands for in text (control codes are
    /// themselves).
    pub fn to_char(self, b: u8) -> char {
        if b < 0x20 || b == 0x7f {
            return b as char;
        }
        self.glyph(b)
    }

    /// Map a Unicode character back to this code page.
    pub fn from_char(self, c: char) -> Option<u8> {
        if (' '..='~').contains(&c) && self.glyph(c as u8) == c {
            return Some(c as u8);
        }
        if (c as u32) < 0x20 {
            return Some(c as u8);
        }
        let t = &CODEPAGES[self.0 as usize].1;
        (0x20..=255usize).chain(0..0x20).find(|&b| t[b] == c as u32 as u16).map(|b| b as u8)
    }
}
