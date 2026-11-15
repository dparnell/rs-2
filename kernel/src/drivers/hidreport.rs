//! HID report descriptors: find the mouse and the keyboard in a device
//! that is not a boot device, and read their reports.
//!
//! A Surface Type Cover's touchpad is a "precision touchpad": its
//! interface is not a boot mouse, and its report descriptor holds a
//! digitizer (fingers, contacts) and an ordinary Mouse application
//! collection. Until the host switches it to touchpad mode with a feature
//! report (which we never do) it sends mouse reports, which are all this
//! needs. Its keyboard is on the same interface, under its own report
//! ID, so it is no boot keyboard either. Tablets (QEMU's usb-tablet, KVM switches) are mice with absolute
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

/// Where a keyboard's modifiers and keys are in its reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KbdLayout {
    pub id: Option<u8>,
    /// Bit of each modifier (usages E0h-E7h: left Ctrl ... right GUI).
    pub mods: [Option<u32>; 8],
    /// Key array: bit offset, bits per entry, entries, and the usage of
    /// value `logical minimum`.
    pub array: Option<(u32, u32, u32, i32, u32)>,
    /// Key bitmap (one bit per usage): bit offset, first usage, count.
    pub bitmap: Option<(u32, u32, u32)>,
}

impl KbdLayout {
    /// `report` as a boot keyboard report (modifiers, 0, six usages), if
    /// it is one of this keyboard's.
    pub fn boot_report(&self, report: &[u8]) -> Option<[u8; 8]> {
        let p = match self.id {
            Some(id) => (report.first() == Some(&id)).then(|| &report[1..])?,
            None => report,
        };
        let bit = |b: u32| p.get((b / 8) as usize).is_some_and(|&v| v >> (b % 8) & 1 != 0);
        let mut out = [0u8; 8];
        for (n, m) in self.mods.iter().enumerate() {
            if m.is_some_and(bit) {
                out[0] |= 1 << n;
            }
        }
        let mut keys = 2;
        let mut add = |u: u32| {
            // Modifiers reported as keys go in the modifier byte.
            if (0xe0..=0xe7).contains(&u) {
                out[0] |= 1 << (u - 0xe0);
            } else if u > 0 && u < 0xe0 && keys < 8 {
                out[keys] = u as u8;
                keys += 1;
            }
        };
        if let Some((at, size, count, min, base)) = self.array {
            for k in 0..count {
                let f = Field { bit: at + k * size, size, signed: min < 0, min, max: 0, relative: false };
                if let Some(v) = f.read(p) {
                    let u = v - min + base as i32;
                    if v >= min && u > 0 {
                        add(u as u32);
                    }
                }
            }
        }
        if let Some((at, first, count)) = self.bitmap {
            for k in 0..count {
                if bit(at + k) {
                    add(first + k);
                }
            }
        }
        Some(out)
    }
}

/// What a report descriptor holds that we can use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub mouse: Option<MouseLayout>,
    pub keyboard: Option<KbdLayout>,
}

const PAGE_KEYS: u32 = 7;
const USAGE_KEYBOARD: u32 = 6;
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
    parse(d).mouse
}

/// The first Mouse application collection with an X and a Y, and the
/// first Keyboard application collection with keys, in report
/// descriptor `d`.
pub fn parse(d: &[u8]) -> Found {
    let mut g = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    // Local items: usages (page << 16 | usage) and a usage range.
    let mut usages: Vec<u32> = Vec::new();
    let mut range: (Option<u32>, Option<u32>) = (None, None);
    // Collections open: 1 = a Mouse application, 2 = a Keyboard one.
    let mut depth: Vec<u8> = Vec::new();
    let mut kbd: Option<KbdLayout> = None;
    let mut kcur: Option<KbdLayout> = None;
    // Input bits used so far per report ID.
    let mut offsets: Vec<(u8, u32)> = Vec::new();
    let mut found: Option<MouseLayout> = None;
    let mut cur: Option<MouseLayout> = None;
    let mut off = 0;
    while off < d.len() {
        let b = d[off];
        if b == 0xfe {
            // Long item: size, tag, data.
            let Some(&n) = d.get(off + 1) else { break };
            let n = n as usize;
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
                        let in_mouse = depth.contains(&1);
                        let in_kbd = depth.contains(&2);
                        let array = u & 2 == 0;
                        if in_kbd && !constant && g.page == PAGE_KEYS && g.size > 0 {
                            let base = offsets[at].1;
                            let k = kcur.get_or_insert(KbdLayout { id: (id != 0).then_some(id), mods: [None; 8], array: None, bitmap: None });
                            if k.id == (id != 0).then_some(id) {
                                if array {
                                    let first = range.0.or(usages.first().copied()).unwrap_or(PAGE_KEYS << 16) & 0xffff;
                                    if k.array.is_none() {
                                        k.array = Some((base, g.size, g.count, g.min, first));
                                    }
                                } else if g.size == 1 {
                                    for n in 0..g.count {
                                        let usage = if usages.is_empty() { None } else { Some(usages[(n as usize).min(usages.len() - 1)] & 0xffff) };
                                        match usage {
                                            Some(u @ 0xe0..=0xe7) => k.mods[(u - 0xe0) as usize] = Some(base + n),
                                            Some(u) if n == 0 && k.bitmap.is_none() => k.bitmap = Some((base, u, g.count)),
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
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
                        let app = |x: u32| u == 1 && usage == Some(PAGE_DESKTOP << 16 | x);
                        depth.push(if app(USAGE_MOUSE) {
                            1
                        } else if app(USAGE_KEYBOARD) {
                            2
                        } else {
                            0
                        });
                    }
                    // End collection.
                    0xc => match depth.pop() {
                        Some(1) => {
                            if let Some(m) = cur.take() {
                                if m.x.size != 0 && m.y.size != 0 && found.is_none() {
                                    found = Some(m);
                                }
                            }
                        }
                        Some(2) => {
                            if let Some(k) = kcur.take() {
                                if (k.array.is_some() || k.bitmap.is_some()) && kbd.is_none() {
                                    kbd = Some(k);
                                }
                            }
                        }
                        _ => {}
                    },
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
        if found.is_some() && kbd.is_some() {
            break;
        }
    }
    Found { mouse: found, keyboard: kbd }
}
