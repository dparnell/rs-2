//! The record WinStoreWindowPos writes to OS2.INI and WinRestoreWindowPos
//! reads back, in the Workplace Shell's layout (so records written by
//! OS/2 are read, and the other way round):
//!
//! | Offset | Contents |
//! |--------|----------|
//! | 0  | USHORT 0x7B6A |
//! | 2  | ULONG SWP flags (WinQueryWindowPos) |
//! | 6  | SHORT x, y, cx, cy |
//! | 14 | ULONG restore position valid |
//! | 18 | SHORT x, y, cx, cy to restore to (QWS_XRESTORE...) |
//! | 26 | ULONG minimize position valid |
//! | 30 | SHORT x, y when minimized (QWS_XMINIMIZE, QWS_YMINIMIZE) |
//! | 34 | ULONG screen width, height when stored |
//! | 42 | LONG border width, height (WM_QUERYBORDERSIZE), -1: the default |
//! | 50 | ULONG presentation parameters that follow (0: none) |
//!
//! Positions are scaled to the current screen when it differs.

use alloc::vec::Vec;

pub const MAGIC: u16 = 0x7b6a;
/// Size of the record without presentation parameters.
pub const SIZE: usize = 54;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WinPos {
    pub fl: u32,
    pub pos: [i16; 4],
    pub restore: Option<[i16; 4]>,
    pub minimize: Option<[i16; 2]>,
    pub screen: (u32, u32),
    pub border: Option<(i32, i32)>,
    /// The presentation parameter count/size field and data as stored.
    pub pres: u32,
}

fn s16(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([d[o], d[o + 1]])
}

fn u32at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

impl WinPos {
    pub fn encode(&self) -> Vec<u8> {
        let mut d = Vec::with_capacity(SIZE);
        d.extend_from_slice(&MAGIC.to_le_bytes());
        d.extend_from_slice(&self.fl.to_le_bytes());
        for v in self.pos {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&(self.restore.is_some() as u32).to_le_bytes());
        for v in self.restore.unwrap_or([0; 4]) {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&(self.minimize.is_some() as u32).to_le_bytes());
        for v in self.minimize.unwrap_or([0; 2]) {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&self.screen.0.to_le_bytes());
        d.extend_from_slice(&self.screen.1.to_le_bytes());
        let (bx, by) = self.border.unwrap_or((-1, -1));
        d.extend_from_slice(&bx.to_le_bytes());
        d.extend_from_slice(&by.to_le_bytes());
        d.extend_from_slice(&self.pres.to_le_bytes());
        d
    }

    pub fn decode(d: &[u8]) -> Option<WinPos> {
        if d.len() < SIZE || u16::from_le_bytes([d[0], d[1]]) != MAGIC {
            return None;
        }
        let four = |o| [s16(d, o), s16(d, o + 2), s16(d, o + 4), s16(d, o + 6)];
        let (bx, by) = (u32at(d, 42) as i32, u32at(d, 46) as i32);
        Some(WinPos {
            fl: u32at(d, 2),
            pos: four(6),
            restore: (u32at(d, 14) != 0).then(|| four(18)),
            minimize: (u32at(d, 26) != 0).then(|| [s16(d, 30), s16(d, 32)]),
            screen: (u32at(d, 34), u32at(d, 38)),
            border: (bx != -1 && by != -1).then_some((bx, by)),
            pres: u32at(d, 50),
        })
    }

    /// Scale the positions from the screen it was stored on to `screen`.
    pub fn scale_to(&mut self, screen: (u32, u32)) {
        let (ow, oh) = (self.screen.0 as i32, self.screen.1 as i32);
        if ow == 0 || oh == 0 || (self.screen == screen) {
            return;
        }
        let (nw, nh) = (screen.0 as i32, screen.1 as i32);
        let pt = |p: &mut [i16]| {
            p[0] = (p[0] as i32 * nw / ow) as i16;
            p[1] = (p[1] as i32 * nh / oh) as i16;
        };
        pt(&mut self.pos[0..2]);
        pt(&mut self.pos[2..4]);
        if let Some(r) = &mut self.restore {
            pt(&mut r[0..2]);
            pt(&mut r[2..4]);
        }
        if let Some(m) = &mut self.minimize {
            pt(&mut m[..]);
        }
        self.screen = screen;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_scale() {
        let w = WinPos { fl: 0x109f, pos: [10, 20, 300, 200], restore: Some([10, 20, 300, 200]), minimize: None, screen: (1024, 768), border: None, pres: 0 };
        let d = w.encode();
        assert_eq!(d.len(), SIZE);
        assert_eq!(&d[..2], &[0x6a, 0x7b]);
        assert_eq!(WinPos::decode(&d), Some(w.clone()));
        let mut s = w.clone();
        s.scale_to((512, 384));
        assert_eq!(s.pos, [5, 10, 150, 100]);
        assert_eq!(s.restore, Some([5, 10, 150, 100]));
        let mut d2 = d.clone();
        d2[0] = 0;
        assert_eq!(WinPos::decode(&d2), None);
        assert_eq!(WinPos::decode(&d[..20]), None);
    }
}
