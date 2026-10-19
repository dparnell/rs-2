//! The program list (PrfAddProgram, PrfCreateGroup, ...) as rs2's PMSHAPI
//! keeps it in a profile (OS2.INI for HINI_USERPROFILE), so that a desktop
//! shell can read it with the ordinary Prf calls or this module.
//!
//! Everything is in one application, [`APP`]. Each group and program is
//! one key named by its handle as eight upper-case hex digits
//! (`"0000000C"`), in creation order; the key [`NEXT`] holds the next
//! handle to give out (a 4-byte little-endian number), so a removed
//! entry's handle is not reused. Readers skip keys that are not eight hex
//! digits. The value of an entry key ([`Entry::to_bytes`]):
//!
//! | Offset | Size | Contents |
//! |--------|------|----------|
//! | 0 | 4 | layout version, 1 |
//! | 4 | 4 | handle of the group the entry is in; [`SGH_ROOT`] for a group |
//! | 8 | 4 | PROGTYPE.progc ([`PROG_GROUP`] for a group) |
//! | 12 | 4 | PROGTYPE.fbVisible (SHE_VISIBLE 0, SHE_INVISIBLE 1, SHE_PROTECTED 2) |
//! | 16 | 36 | SWP swpInitial, as given |
//! | 52 | ... | six fields: title, executable, parameters, startup directory, icon, environment; each a u16 length (0xFFFF: the pointer was NULL) and that many bytes, no NUL |
//!
//! The environment field holds the strings, each with its NUL, without
//! the final NUL that ends the list.

use alloc::vec::Vec;

pub const APP: &[u8] = b"PM_RS2_ProgramList";
pub const NEXT: &[u8] = b"NextHandle";
pub const SGH_ROOT: u32 = 0xffff_ffff;
pub const PROG_GROUP: u32 = 5;
const VERSION: u32 = 1;
const ABSENT: u16 = 0xffff;

/// A group or a program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub group: u32,
    pub progc: u32,
    pub visible: u32,
    pub swp: [u32; 9],
    /// Title, executable, parameters, startup directory, icon,
    /// environment (see the module documentation).
    pub fields: [Option<Vec<u8>>; 6],
}

pub const TITLE: usize = 0;
pub const EXECUTABLE: usize = 1;
pub const ENVIRONMENT: usize = 5;

impl Entry {
    pub fn is_group(&self) -> bool {
        self.progc == PROG_GROUP && self.group == SGH_ROOT
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        for x in [VERSION, self.group, self.progc, self.visible] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        for x in self.swp {
            v.extend_from_slice(&x.to_le_bytes());
        }
        for f in &self.fields {
            match f {
                Some(s) => {
                    let n = s.len().min(ABSENT as usize - 1);
                    v.extend_from_slice(&(n as u16).to_le_bytes());
                    v.extend_from_slice(&s[..n]);
                }
                None => v.extend_from_slice(&ABSENT.to_le_bytes()),
            }
        }
        v
    }

    pub fn parse(b: &[u8]) -> Option<Entry> {
        let u = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        if u(0)? != VERSION {
            return None;
        }
        let mut e = Entry { group: u(4)?, progc: u(8)?, visible: u(12)?, ..Entry::default() };
        for (i, w) in e.swp.iter_mut().enumerate() {
            *w = u(16 + 4 * i)?;
        }
        let mut p = 52;
        for f in e.fields.iter_mut() {
            let n = u16::from_le_bytes([*b.get(p)?, *b.get(p + 1)?]);
            p += 2;
            if n != ABSENT {
                *f = Some(b.get(p..p + n as usize)?.to_vec());
                p += n as usize;
            }
        }
        Some(e)
    }
}

/// The key name of handle `h`.
pub fn key(h: u32) -> [u8; 8] {
    let mut k = [0u8; 8];
    for (i, c) in k.iter_mut().enumerate() {
        *c = b"0123456789ABCDEF"[(h >> (28 - 4 * i) & 15) as usize];
    }
    k
}

/// The handle a key names, if it is an entry key.
pub fn handle(key: &[u8]) -> Option<u32> {
    if key.len() != 8 {
        return None;
    }
    key.iter().try_fold(0u32, |h, &c| Some(h << 4 | (c as char).to_digit(16)? as u32))
}
