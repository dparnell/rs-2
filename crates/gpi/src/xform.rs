//! Transforms: 2-D affine matrices in PM's row-vector convention
//! ([x y 1] * M), the MATRIXLF layout, and page units.

use crate::math::{fx, round, sin_cos, to_fx, PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    pub m11: f64,
    pub m12: f64,
    pub m21: f64,
    pub m22: f64,
    pub m31: f64,
    pub m32: f64,
}

pub const TRANSFORM_REPLACE: i32 = 0;
pub const TRANSFORM_ADD: i32 = 1;
pub const TRANSFORM_PREEMPT: i32 = 2;

impl Default for Matrix {
    fn default() -> Self {
        Matrix::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, m31: 0.0, m32: 0.0 };

    pub fn translate(tx: f64, ty: f64) -> Matrix {
        Matrix { m31: tx, m32: ty, ..Matrix::IDENTITY }
    }

    pub fn scale(sx: f64, sy: f64) -> Matrix {
        Matrix { m11: sx, m22: sy, ..Matrix::IDENTITY }
    }

    /// Counter-clockwise rotation by `deg` degrees about the origin.
    pub fn rotate(deg: f64) -> Matrix {
        let (s, c) = sin_cos(deg * PI / 180.0);
        Matrix { m11: c, m12: s, m21: -s, m22: c, m31: 0.0, m32: 0.0 }
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.m11 + y * self.m21 + self.m31, x * self.m12 + y * self.m22 + self.m32)
    }

    /// `self` followed by `o`.
    pub fn then(&self, o: &Matrix) -> Matrix {
        Matrix {
            m11: self.m11 * o.m11 + self.m12 * o.m21,
            m12: self.m11 * o.m12 + self.m12 * o.m22,
            m21: self.m21 * o.m11 + self.m22 * o.m21,
            m22: self.m21 * o.m12 + self.m22 * o.m22,
            m31: self.m31 * o.m11 + self.m32 * o.m21 + o.m31,
            m32: self.m31 * o.m12 + self.m32 * o.m22 + o.m32,
        }
    }

    pub fn det(&self) -> f64 {
        self.m11 * self.m22 - self.m12 * self.m21
    }

    pub fn invert(&self) -> Option<Matrix> {
        let d = self.det();
        if d == 0.0 {
            return None;
        }
        let (a, b, c, e) = (self.m22 / d, -self.m12 / d, -self.m21 / d, self.m11 / d);
        Some(Matrix { m11: a, m12: b, m21: c, m22: e, m31: -(self.m31 * a + self.m32 * c), m32: -(self.m31 * b + self.m32 * e) })
    }

    /// Combine with an existing matrix per TRANSFORM_* option.
    pub fn combine(cur: &Matrix, new: &Matrix, opt: i32) -> Option<Matrix> {
        match opt {
            TRANSFORM_REPLACE => Some(*new),
            TRANSFORM_ADD => Some(cur.then(new)),
            TRANSFORM_PREEMPT => Some(new.then(cur)),
            _ => None,
        }
    }

    /// From the first `n` elements of a MATRIXLF (fxM11, fxM12, lM13,
    /// fxM21, fxM22, lM23, lM31, lM32, lM33); missing elements are
    /// identity.
    pub fn from_lf(v: &[i32]) -> Matrix {
        let id = Matrix::IDENTITY.to_lf();
        let g = |i: usize| v.get(i).copied().unwrap_or(id[i]);
        Matrix { m11: fx(g(0)), m12: fx(g(1)), m21: fx(g(3)), m22: fx(g(4)), m31: g(6) as f64, m32: g(7) as f64 }
    }

    pub fn to_lf(&self) -> [i32; 9] {
        [to_fx(self.m11), to_fx(self.m12), 0, to_fx(self.m21), to_fx(self.m22), 0, round(self.m31), round(self.m32), 1]
    }

    pub fn is_identity(&self) -> bool {
        *self == Matrix::IDENTITY
    }

    /// Only scaling and translation (axis-aligned rectangles stay so).
    pub fn is_axis_aligned(&self) -> bool {
        self.m12 == 0.0 && self.m21 == 0.0
    }
}

// Page units (GpiCreatePS flOptions & 0x1c).
pub const PU_ARBITRARY: u32 = 0x04;
pub const PU_PELS: u32 = 0x08;
pub const PU_LOMETRIC: u32 = 0x0c;
pub const PU_HIMETRIC: u32 = 0x10;
pub const PU_LOENGLISH: u32 = 0x14;
pub const PU_HIENGLISH: u32 = 0x18;
pub const PU_TWIPS: u32 = 0x1c;

/// Display resolution in pels per metre (96 dpi).
pub const PELS_PER_METRE: i32 = 3780;

/// Device pels per page unit.
pub fn page_unit_scale(units: u32) -> f64 {
    let ppm = PELS_PER_METRE as f64;
    match units {
        PU_LOMETRIC => ppm / 10_000.0,
        PU_HIMETRIC => ppm / 100_000.0,
        PU_LOENGLISH => ppm * 0.0254 / 100.0,
        PU_HIENGLISH => ppm * 0.0254 / 1000.0,
        PU_TWIPS => ppm * 0.0254 / 1440.0,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_and_invert() {
        let m = Matrix::scale(2.0, 3.0).then(&Matrix::translate(10.0, -5.0));
        assert_eq!(m.apply(1.0, 1.0), (12.0, -2.0));
        let i = m.invert().unwrap();
        let (x, y) = i.apply(12.0, -2.0);
        assert!((x - 1.0).abs() < 1e-12 && (y - 1.0).abs() < 1e-12);
        let r = Matrix::rotate(90.0);
        let (x, y) = r.apply(1.0, 0.0);
        assert!(x.abs() < 1e-9 && (y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn lf_round_trip() {
        let m = Matrix { m11: 1.5, m12: 0.0, m21: 0.0, m22: -2.0, m31: 7.0, m32: 9.0 };
        assert_eq!(Matrix::from_lf(&m.to_lf()), m);
        // Partial: only the first 4 elements.
        let p = Matrix::from_lf(&[0x20000, 0, 0, 0]);
        assert_eq!(p.m11, 2.0);
        assert_eq!(p.m21, 0.0);
        assert_eq!(p.m22, 1.0);
        let add = Matrix::combine(&Matrix::translate(1.0, 0.0), &Matrix::scale(2.0, 2.0), TRANSFORM_ADD).unwrap();
        assert_eq!(add.apply(0.0, 0.0), (2.0, 0.0));
        let pre = Matrix::combine(&Matrix::translate(1.0, 0.0), &Matrix::scale(2.0, 2.0), TRANSFORM_PREEMPT).unwrap();
        assert_eq!(pre.apply(0.0, 0.0), (1.0, 0.0));
    }
}
