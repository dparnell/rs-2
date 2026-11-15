//! HID report descriptors: find the mouse in a device that is not a boot
//! mouse, and read its reports.
//!
//! A Surface Type Cover's touchpad is a "precision touchpad": its
//! interface is not a boot mouse, and its report descriptor holds a
//! digitizer (fingers, contacts) and an ordinary Mouse application
//! collection. Until the host switches it to touchpad mode with a feature
//! report (which we never do) it sends mouse reports, which are all this
//! needs. Tablets (QEMU's usb-tablet, KVM switches) are mice with absolute
//! X and Y.

use alloc::vec::Vec;

/// One field of an input report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    /// Bit offset after the report ID byte.
    pub bit: u32,
    pub size: u32,
    pub signed: bool,
    pub min: i32,
    pub max: i32,
    pub relative: bool,
}

impl Field {
    pub fn read(&self, data: &[u8]) -> Option<i32> {
        if self.size == 0 || self.size > 32 {
            return None;
        }
        let mut v: u64 = 0;
        for k in 0..self.size {
            let b = self.bit + k;
            let byte = *data.get((b / 8) as usize)?;
            v |= (((byte >> (b % 8)) & 1) as u64) << k;
        }
        Some(if self.signed && self.size < 64 && v & (1 << (self.size - 1)) != 0 {
            (v as i64 - (1i64 << self.size)) as i32
        } else {
            v as i32
        })
    }
}

/// Where a mouse's buttons, X, Y and wheel are in its reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseLayout {
    /// The report ID its reports start with, if the device uses IDs.
    pub id: Option<u8>,
    /// Buttons 1-3 (bit offset of each).
    pub buttons: [Option<u32>; 3],
    pub x: Field,
    pub y: Field,
    pub wheel: Option<Field>,
}

impl MouseLayout {
    /// The payload of `report` if it is one of this mouse's.
    pub fn payload<'a>(&self, report: &'a [u8]) -> Option<&'a [u8]> {
        match self.id {
            Some(id) => (report.first() == Some(&id)).then(|| &report[1..]),
            None => Some(report),
        }
    }

    pub fn buttons(&self, p: &[u8]) -> u8 {
        let mut b = 0;
        for (n, bit) in self.buttons.iter().enumerate() {
            if let Some(bit) = bit {
                if p.get((bit / 8) as usize).is_some_and(|&v| v >> (bit % 8) & 1 != 0) {
                    b |= 1 << n;
                }
            }
        }
        b
    }
}

const PAGE_DESKTOP: u32 = 1;
const PAGE_BUTTON: u32 = 9;
const USAGE_MOUSE: u32 = 2;
const USAGE_X: u32 = 0x30;
const USAGE_Y: u32 = 0x31;
const USAGE_WHEEL: u32 = 0x38;

#[derive(Clone, Copy, Default)]
struct Globals {
    page: u32,
    min: i32,
    max: i32,
    size: u32,
    count: u32,
    id: u8,
}

/// The first Mouse application collection in report descriptor `d` with
/// an X and a Y, or None.
pub fn find_mouse(d: &[u8]) -> Option<MouseLayout> {
    let mut g = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    // Local items: usages (page << 16 | usage) and a usage range.
    let mut usages: Vec<u32> = Vec::new();
    let mut range: (Option<u32>, Option<u32>) = (None, None);
    // Collections open, and how many of them are Mouse applications.
    let mut depth: Vec<bool> = Vec::new();
    // Input bits used so far per report ID.
    let mut offsets: Vec<(u8, u32)> = Vec::new();
    let mut found: Option<MouseLayout> = None;
    let mut cur: Option<MouseLayout> = None;
    let mut off = 0;
    while off < d.len() {
        let b = d[off];
        if b == 0xfe {
            // Long item: size, tag, data.
            let n = *d.get(off + 1)? as usize;
            off += 3 + n;
            continue;
        }
        let n = [0, 1, 2, 4][(b & 3) as usize];
        if off + 1 + n > d.len() {
            break;
        }
        let raw = &d[off + 1..off + 1 + n];
        let mut u: u32 = 0;
        for (k, &x) in raw.iter().enumerate() {
            u |= (x as u32) << (8 * k);
        }
        let sv = match n {
            1 => raw[0] as i8 as i32,
            2 => u as u16 as i16 as i32,
            4 => u as i32,
            _ => 0,
        };
        off += 1 + n;
        let (kind, tag) = ((b >> 2) & 3, b >> 4);
        let full = |usage: u32, n: usize, page: u32| if n == 4 { usage } else { page << 16 | usage };
        match kind {
            // Main.
            0 => {
                match tag {
                    // Input.
                    8 => {
                        let id = g.id;
                        let at = match offsets.iter().position(|o| o.0 == id) {
                            Some(p) => p,
                            None => {
                                offsets.push((id, 0));
                                offsets.len() - 1
                            }
                        };
                        let constant = u & 1 != 0;
                        let relative = u & 4 != 0;
                        if let (Some(lo), Some(hi)) = range {
                            for x in lo..=hi.min(lo + 255) {
                                usages.push(x);
                            }
                        }
                        let in_mouse = depth.iter().any(|&m| m);
                        for k in 0..g.count {
                            let bit = offsets[at].1 + k * g.size;
                            if constant || !in_mouse || usages.is_empty() {
                                continue;
                            }
                            let usage = usages[(k as usize).min(usages.len() - 1)];
                            let (page, id16) = (usage >> 16, usage & 0xffff);
                            let f = Field { bit, size: g.size, signed: g.min < 0, min: g.min, max: g.max, relative };
                            let m = cur.get_or_insert(MouseLayout {
                                id: (id != 0).then_some(id),
                                buttons: [None; 3],
                                x: Field { bit: 0, size: 0, signed: false, min: 0, max: 0, relative: true },
                                y: Field { bit: 0, size: 0, signed: false, min: 0, max: 0, relative: true },
                                wheel: None,
                            });
                            if m.id != (id != 0).then_some(id) {
                                // Another report of the same collection.
                                continue;
                            }
                            match (page, id16) {
                                (PAGE_BUTTON, 1..=3) => m.buttons[id16 as usize - 1] = Some(bit),
                                (PAGE_DESKTOP, USAGE_X) if m.x.size == 0 => m.x = f,
                                (PAGE_DESKTOP, USAGE_Y) if m.y.size == 0 => m.y = f,
                                (PAGE_DESKTOP, USAGE_WHEEL) if m.wheel.is_none() => m.wheel = Some(f),
                                _ => {}
                            }
                        }
                        offsets[at].1 += g.count * g.size;
                    }
                    // Collection: 1 = application.
                    0xa => {
                        let usage = usages.first().copied().or(range.0);
                        let mouse = u == 1 && usage == Some(PAGE_DESKTOP << 16 | USAGE_MOUSE);
                        depth.push(mouse);
                    }
                    // End collection.
                    0xc => {
                        if depth.pop() == Some(true) {
                            if let Some(m) = cur.take() {
                                if m.x.size != 0 && m.y.size != 0 && found.is_none() {
                                    found = Some(m);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                usages.clear();
                range = (None, None);
            }
            // Global.
            1 => match tag {
                0 => g.page = u,
                1 => g.min = sv,
                // A maximum that only fits unsigned (0xff with one byte).
                2 => g.max = if g.min >= 0 { u as i32 } else { sv },
                7 => g.size = u,
                8 => g.id = u as u8,
                9 => g.count = u,
                0xa => stack.push(g),
                0xb => g = stack.pop().unwrap_or(g),
                _ => {}
            },
            // Local.
            2 => match tag {
                0 => usages.push(full(u, n, g.page)),
                1 => range.0 = Some(full(u, n, g.page)),
                2 => range.1 = Some(full(u, n, g.page)),
                _ => {}
            },
            _ => {}
        }
        if found.is_some() {
            break;
        }
    }
    found
}
