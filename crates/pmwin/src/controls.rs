//! The logic of the standard controls that can be tested on the host:
//! spin button values, slider scales and value set grids.

use alloc::vec::Vec;

// ---- Spin buttons ----

/// The value of a spin button: a number in [lo, hi] or an index into an
/// array of strings. Spinning wraps around at either end, as on OS/2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spin {
    lo: i32,
    hi: i32,
    value: i32,
    array: Option<Vec<Vec<u8>>>,
}

impl Default for Spin {
    fn default() -> Spin {
        Spin::new()
    }
}

impl Spin {
    pub fn new() -> Spin {
        Spin { lo: 0, hi: 0, value: 0, array: None }
    }

    /// Numeric limits (either order); the value is kept within them.
    pub fn set_limits(&mut self, a: i32, b: i32) {
        (self.lo, self.hi) = (a.min(b), a.max(b));
        self.array = None;
        self.value = self.value.clamp(self.lo, self.hi);
    }

    pub fn set_array(&mut self, items: Vec<Vec<u8>>) {
        self.array = Some(items);
        self.value = 0;
    }

    /// (lowest, highest) value: array indices for an array.
    pub fn limits(&self) -> (i32, i32) {
        match &self.array {
            Some(a) => (0, a.len() as i32 - 1),
            None => (self.lo, self.hi),
        }
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    /// Set the value (or array index); false if out of range.
    pub fn set_value(&mut self, v: i32) -> bool {
        let (lo, hi) = self.limits();
        if hi < lo || v < lo || v > hi {
            return false;
        }
        self.value = v;
        true
    }

    /// Step `n` values (negative: down), wrapping around.
    pub fn spin(&mut self, n: i32) {
        let (lo, hi) = self.limits();
        if hi < lo {
            return;
        }
        let span = hi as i64 - lo as i64 + 1;
        let v = (self.value as i64 - lo as i64 + n as i64).rem_euclid(span) + lo as i64;
        self.value = v as i32;
    }

    /// The text shown: the number (zero-padded to the width of the
    /// limits when `pad`) or the array item.
    pub fn text(&self, pad: bool) -> Vec<u8> {
        if let Some(a) = &self.array {
            return a.get(self.value as usize).cloned().unwrap_or_default();
        }
        let mut s = alloc::format!("{}", self.value.unsigned_abs()).into_bytes();
        if pad {
            let w = alloc::format!("{}", self.hi.unsigned_abs().max(self.lo.unsigned_abs())).len();
            while s.len() < w {
                s.insert(0, b'0');
            }
        }
        if self.value < 0 {
            s.insert(0, b'-');
        }
        s
    }

    /// Take a typed value; false (unchanged) if it is not one.
    pub fn set_from_text(&mut self, t: &[u8]) -> bool {
        let t = trim(t);
        if let Some(a) = &self.array {
            return match a.iter().position(|x| x.eq_ignore_ascii_case(t)) {
                Some(i) => {
                    self.value = i as i32;
                    true
                }
                None => false,
            };
        }
        let (neg, digits) = match t.split_first() {
            Some((b'-', rest)) => (true, rest),
            Some((b'+', rest)) => (false, rest),
            _ => (false, t),
        };
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return false;
        }
        let mut v: i64 = 0;
        for &d in digits {
            v = v * 10 + (d - b'0') as i64;
            if v > i32::MAX as i64 + 1 {
                return false;
            }
        }
        let v = if neg { -v } else { v };
        if v < self.lo as i64 || v > self.hi as i64 {
            return false;
        }
        self.value = v as i32;
        true
    }
}

fn trim(t: &[u8]) -> &[u8] {
    let s = t.iter().position(|&c| c != b' ').unwrap_or(t.len());
    let e = t.iter().rposition(|&c| c != b' ').map_or(s, |i| i + 1);
    &t[s..e.max(s)]
}

// ---- Sliders ----

/// A slider's primary scale: `increments` positions along a shaft of
/// `len` pels; the arm is `arm` pels wide and its centre goes from
/// arm/2 to len - arm/2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale {
    pub increments: u32,
    pub len: i32,
    pub arm: i32,
}

impl Scale {
    /// Pels between two increments (0 with fewer than two).
    pub fn spacing(&self) -> f64 {
        if self.increments < 2 {
            return 0.0;
        }
        (self.len - self.arm).max(0) as f64 / (self.increments - 1) as f64
    }

    /// Centre of increment `i` from the home end of the shaft.
    pub fn pos(&self, i: u32) -> i32 {
        let i = i.min(self.increments.saturating_sub(1));
        self.arm / 2 + (i as f64 * self.spacing() + 0.5) as i32
    }

    /// Increment nearest to pel `x` from the home end.
    pub fn index_at(&self, x: i32) -> u32 {
        let span = (self.len - self.arm).max(0) as i64;
        if self.increments < 2 || span == 0 {
            return 0;
        }
        let d = (x - self.arm / 2).max(0) as i64;
        let n = (self.increments - 1) as i64;
        let i = (2 * d * n + span) / (2 * span);
        (i as u32).min(self.increments - 1)
    }
}

// ---- Value sets ----

/// A value set's grid: `rows` x `cols` cells in a `w` x `h` area,
/// `space` pels between cells. Rows and columns count from 1, row 1 at
/// the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub rows: u16,
    pub cols: u16,
    pub w: i32,
    pub h: i32,
    pub space: i32,
}

impl Grid {
    /// Cell (row, col) as (left, bottom, right, top) in the area
    /// (y grows upwards).
    pub fn cell(&self, row: u16, col: u16) -> (i32, i32, i32, i32) {
        let (r, c) = (self.rows.max(1) as i32, self.cols.max(1) as i32);
        let cw = (self.w - self.space * (c + 1)) / c;
        let ch = (self.h - self.space * (r + 1)) / r;
        let left = self.space + (col as i32 - 1) * (cw + self.space);
        let top = self.h - self.space - (row as i32 - 1) * (ch + self.space);
        (left, top - ch, left + cw, top)
    }

    /// Cell under a point, if any.
    pub fn hit(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        for row in 1..=self.rows {
            for col in 1..=self.cols {
                let (l, b, r, t) = self.cell(row, col);
                if x >= l - self.space / 2 - 1 && x < r + self.space / 2 + 1 && y >= b - self.space / 2 - 1 && y < t + self.space / 2 + 1 {
                    return Some((row, col));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn spin_numbers() {
        let mut s = Spin::new();
        s.set_limits(100, 0);
        assert_eq!(s.limits(), (0, 100));
        assert!(s.set_value(42));
        assert!(!s.set_value(101));
        s.spin(1);
        assert_eq!(s.value(), 43);
        s.set_value(100);
        s.spin(1);
        assert_eq!(s.value(), 0);
        s.spin(-1);
        assert_eq!(s.value(), 100);
        s.set_value(7);
        assert_eq!(s.text(true), b"007");
        assert_eq!(s.text(false), b"7");
        assert!(s.set_from_text(b" 55 "));
        assert_eq!(s.value(), 55);
        assert!(!s.set_from_text(b"5x"));
        assert!(!s.set_from_text(b"500"));
        assert_eq!(s.value(), 55);
        s.set_limits(-5, 5);
        s.set_value(-3);
        assert_eq!(s.text(true), b"-3");
        assert!(s.set_from_text(b"-5"));
    }

    #[test]
    fn spin_array() {
        let mut s = Spin::new();
        s.set_array(vec![b"Red".to_vec(), b"Green".to_vec(), b"Blue".to_vec()]);
        assert_eq!(s.limits(), (0, 2));
        assert_eq!(s.text(false), b"Red");
        s.spin(-1);
        assert_eq!(s.text(false), b"Blue");
        assert!(s.set_from_text(b"green"));
        assert_eq!(s.value(), 1);
        assert!(!s.set_from_text(b"Pink"));
    }

    #[test]
    fn slider_scale() {
        let s = Scale { increments: 11, len: 210, arm: 10 };
        assert_eq!(s.spacing(), 20.0);
        assert_eq!(s.pos(0), 5);
        assert_eq!(s.pos(10), 205);
        assert_eq!(s.index_at(5), 0);
        assert_eq!(s.index_at(24), 1);
        assert_eq!(s.index_at(1000), 10);
        assert_eq!(s.index_at(-50), 0);
    }

    #[test]
    fn value_set_grid() {
        let g = Grid { rows: 2, cols: 3, w: 100, h: 50, space: 4 };
        let (l, b, r, t) = g.cell(1, 1);
        assert_eq!((l, t), (4, 46));
        assert!(r > l && t > b);
        assert_eq!(g.hit(l + 1, t - 1), Some((1, 1)));
        let (l, b, _, _) = g.cell(2, 3);
        assert_eq!(g.hit(l + 1, b + 1), Some((2, 3)));
    }
}
