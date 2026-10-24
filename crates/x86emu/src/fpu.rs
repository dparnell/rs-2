//! The x87 coprocessor, computing in f64 (programs see 80-bit registers
//! in FSAVE images and m80 loads/stores, converted on the way).

use crate::exec::Rm;
use crate::*;

#[derive(Clone)]
pub struct Fpu {
    pub st: [f64; 8],
    /// Tag word: 2 bits per physical register (0 valid, 1 zero, 2
    /// special, 3 empty).
    pub tag: u16,
    pub cw: u16,
    pub sw: u16,
    pub top: u8,
}

impl Default for Fpu {
    fn default() -> Self {
        Fpu { st: [0.0; 8], tag: 0xffff, cw: 0x037f, sw: 0, top: 0 }
    }
}

const C0: u16 = 0x0100;
const C1: u16 = 0x0200;
const C2: u16 = 0x0400;
const C3: u16 = 0x4000;
const IE: u16 = 0x0001;
const ZE: u16 = 0x0004;
const SFL: u16 = 0x0040;

impl Fpu {
    fn phys(&self, i: u8) -> usize {
        ((self.top + i) & 7) as usize
    }
    pub fn get(&self, i: u8) -> f64 {
        self.st[self.phys(i)]
    }
    fn set(&mut self, i: u8, v: f64) {
        let p = self.phys(i);
        self.st[p] = v;
        self.set_tag(p, Self::tag_of(v));
    }
    fn tag_of(v: f64) -> u16 {
        if v == 0.0 {
            1
        } else if v.is_finite() {
            0
        } else {
            2
        }
    }
    fn set_tag(&mut self, p: usize, t: u16) {
        self.tag = (self.tag & !(3 << (p * 2))) | t << (p * 2);
    }
    fn empty(&self, i: u8) -> bool {
        (self.tag >> (self.phys(i) * 2)) & 3 == 3
    }
    pub fn push(&mut self, v: f64) {
        self.top = (self.top + 7) & 7;
        if !self.empty(0) {
            self.sw |= IE | SFL | C1;
        }
        self.set(0, v);
    }
    pub fn pop(&mut self) -> f64 {
        let v = self.get(0);
        let p = self.phys(0);
        self.set_tag(p, 3);
        self.top = (self.top + 1) & 7;
        v
    }
    pub fn status(&self) -> u16 {
        (self.sw & !0x3800) | (self.top as u16) << 11
    }
    fn round(&self, v: f64) -> f64 {
        match (self.cw >> 10) & 3 {
            0 => {
                // Nearest, ties to even.
                let r = libm::round(v);
                if (r - v).abs() == 0.5 && libm::fmod(r, 2.0) != 0.0 { r - v.signum() } else { r }
            }
            1 => libm::floor(v),
            2 => libm::ceil(v),
            _ => libm::trunc(v),
        }
    }
    fn compare(&mut self, a: f64, b: f64) {
        self.sw &= !(C0 | C2 | C3);
        if a.is_nan() || b.is_nan() {
            self.sw |= C0 | C2 | C3;
        } else if a < b {
            self.sw |= C0;
        } else if a == b {
            self.sw |= C3;
        }
    }
    /// Arithmetic op 0-7 of the D8/DC encoding: st0 = st0 op v.
    fn arith(&mut self, op: u8, a: f64, b: f64) -> f64 {
        match op {
            0 => a + b,
            1 => a * b,
            4 => a - b,
            5 => b - a,
            6 => {
                if b == 0.0 {
                    self.sw |= ZE;
                }
                a / b
            }
            _ => {
                if a == 0.0 {
                    self.sw |= ZE;
                }
                b / a
            }
        }
    }
}

/// FPREM (truncating) or FPREM1 (rounding) with the quotient's low bits
/// in C0, C3, C1.
fn fprem(f: &mut Fpu, nearest: bool) {
    let a = f.get(0);
    let m = f.get(1);
    let q = if nearest { libm::round(a / m) } else { libm::trunc(a / m) };
    f.set(0, a - q * m);
    let qi = q.abs() as u64;
    f.sw &= !(C0 | C1 | C2 | C3);
    if qi & 1 != 0 {
        f.sw |= C1;
    }
    if qi & 2 != 0 {
        f.sw |= C3;
    }
    if qi & 4 != 0 {
        f.sw |= C0;
    }
}

/// 80-bit extended precision to f64.
pub fn f80_to_f64(m: u64, se: u16) -> f64 {
    let sign = se & 0x8000 != 0;
    let e = (se & 0x7fff) as i32;
    let v = if e == 0 && m == 0 {
        0.0
    } else if e == 0x7fff {
        if m << 1 == 0 { f64::INFINITY } else { f64::NAN }
    } else {
        libm::ldexp(m as f64, e - 16383 - 63)
    };
    if sign { -v } else { v }
}

/// f64 to 80-bit extended precision (mantissa, sign/exponent).
pub fn f64_to_f80(v: f64) -> (u64, u16) {
    let bits = v.to_bits();
    let sign = if bits >> 63 != 0 { 0x8000 } else { 0 };
    let e = ((bits >> 52) & 0x7ff) as i32;
    let f = bits & ((1 << 52) - 1);
    if e == 0 && f == 0 {
        return (0, sign);
    }
    if e == 0x7ff {
        return (if f == 0 { 1 << 63 } else { 0xc000_0000_0000_0000 }, sign | 0x7fff);
    }
    if e == 0 {
        // Denormal: normalise.
        let sh = f.leading_zeros() - 11;
        let m = f << (11 + sh);
        return (m, sign | (1 - 1023 + 16383 - sh as i32) as u16);
    }
    ((1 << 63) | f << 11, sign | (e - 1023 + 16383) as u16)
}

impl Cpu {
    fn fpu_ref(&mut self) -> &mut Fpu {
        self.fpu.as_mut().unwrap()
    }

    fn rd_f32<B: Bus>(&mut self, b: &mut B, s: usize, o: u32) -> R<f64> {
        Ok(f32::from_bits(self.rd32(b, s, o)?) as f64)
    }
    fn rd_f64<B: Bus>(&mut self, b: &mut B, s: usize, o: u32) -> R<f64> {
        let lo = self.rd32(b, s, o)? as u64;
        let hi = self.rd32(b, s, o.wrapping_add(4))? as u64;
        Ok(f64::from_bits(hi << 32 | lo))
    }
    fn rd_f80<B: Bus>(&mut self, b: &mut B, s: usize, o: u32) -> R<f64> {
        let lo = self.rd32(b, s, o)? as u64;
        let hi = self.rd32(b, s, o.wrapping_add(4))? as u64;
        let se = self.rd16(b, s, o.wrapping_add(8))?;
        Ok(f80_to_f64(hi << 32 | lo, se))
    }
    fn wr_f64<B: Bus>(&mut self, b: &mut B, s: usize, o: u32, v: f64) -> R<()> {
        let x = v.to_bits();
        self.wr32(b, s, o, x as u32)?;
        self.wr32(b, s, o.wrapping_add(4), (x >> 32) as u32)
    }
    fn wr_f80<B: Bus>(&mut self, b: &mut B, s: usize, o: u32, v: f64) -> R<()> {
        let (m, se) = f64_to_f80(v);
        self.wr32(b, s, o, m as u32)?;
        self.wr32(b, s, o.wrapping_add(4), (m >> 32) as u32)?;
        self.wr16(b, s, o.wrapping_add(8), se)
    }
    fn rd_int<B: Bus>(&mut self, b: &mut B, s: usize, o: u32, sz: u8) -> R<f64> {
        Ok(match sz {
            2 => self.rd16(b, s, o)? as i16 as f64,
            4 => self.rd32(b, s, o)? as i32 as f64,
            _ => {
                let lo = self.rd32(b, s, o)? as u64;
                let hi = self.rd32(b, s, o.wrapping_add(4))? as u64;
                (hi << 32 | lo) as i64 as f64
            }
        })
    }
    fn wr_int<B: Bus>(&mut self, b: &mut B, s: usize, o: u32, sz: u8, v: f64) -> R<()> {
        let r = self.fpu_ref().round(v);
        match sz {
            2 => {
                let x = if (-32768.0..=32767.0).contains(&r) { r as i16 } else { i16::MIN };
                self.wr16(b, s, o, x as u16)
            }
            4 => {
                let x = if (-2147483648.0..=2147483647.0).contains(&r) { r as i32 } else { i32::MIN };
                self.wr32(b, s, o, x as u32)
            }
            _ => {
                let x = if r.abs() < 9.2e18 { r as i64 } else { i64::MIN };
                self.wr32(b, s, o, x as u32)?;
                self.wr32(b, s, o.wrapping_add(4), (x >> 32) as u32)
            }
        }
    }

    /// FSTENV/FLDENV image size and layout (real or protected, 16/32).
    fn fpu_env_store<B: Bus>(&mut self, b: &mut B, s: usize, o: u32, op32: bool) -> R<u32> {
        let f = self.fpu.as_ref().unwrap().clone();
        let w = [f.cw as u32 | 0xffff_0000, f.status() as u32 | 0xffff_0000, f.tag as u32 | 0xffff_0000, 0, 0, 0, 0];
        if op32 {
            for (i, v) in w.iter().enumerate() {
                self.wr32(b, s, o.wrapping_add(i as u32 * 4), *v)?;
            }
            Ok(28)
        } else {
            for (i, v) in w.iter().enumerate() {
                self.wr16(b, s, o.wrapping_add(i as u32 * 2), *v as u16)?;
            }
            Ok(14)
        }
    }
    fn fpu_env_load<B: Bus>(&mut self, b: &mut B, s: usize, o: u32, op32: bool) -> R<u32> {
        let st = if op32 { 4 } else { 2 };
        let cw = self.rd16(b, s, o)?;
        let sw = self.rd16(b, s, o.wrapping_add(st))?;
        let tag = self.rd16(b, s, o.wrapping_add(2 * st))?;
        let f = self.fpu_ref();
        f.cw = cw;
        f.sw = sw & !0x3800;
        f.top = ((sw >> 11) & 7) as u8;
        f.tag = tag;
        Ok(if op32 { 28 } else { 14 })
    }

    pub(crate) fn fpu_exec<B: Bus>(&mut self, b: &mut B, op: u8, reg: u8, rm: Rm, op32: bool) -> R<()> {
        let mem = Self::fpu_mem(rm);
        let i = match rm {
            Rm::Reg(r) => r,
            _ => 0,
        };
        match (op, mem) {
            (0xd8, Some((s, o))) | (0xda, Some((s, o))) | (0xdc, Some((s, o))) | (0xde, Some((s, o))) => {
                let v = match op {
                    0xd8 => self.rd_f32(b, s, o)?,
                    0xda => self.rd_int(b, s, o, 4)?,
                    0xdc => self.rd_f64(b, s, o)?,
                    _ => self.rd_int(b, s, o, 2)?,
                };
                let f = self.fpu_ref();
                let a = f.get(0);
                match reg {
                    2 => f.compare(a, v),
                    3 => {
                        f.compare(a, v);
                        f.pop();
                    }
                    _ => {
                        let r = f.arith(reg, a, v);
                        f.set(0, r);
                    }
                }
            }
            (0xd8, None) => {
                let f = self.fpu_ref();
                let a = f.get(0);
                let v = f.get(i);
                match reg {
                    2 => f.compare(a, v),
                    3 => {
                        f.compare(a, v);
                        f.pop();
                    }
                    _ => {
                        let r = f.arith(reg, a, v);
                        f.set(0, r);
                    }
                }
            }
            (0xdc, None) | (0xde, None) => {
                // st(i) = st(i) op st0 (SUB/SUBR and DIV/DIVR swap here).
                let f = self.fpu_ref();
                if op == 0xde && reg == 3 {
                    // FCOMPP (DE D9)
                    if i != 1 {
                        return Err(UD);
                    }
                    let a = f.get(0);
                    let v = f.get(1);
                    f.compare(a, v);
                    f.pop();
                    f.pop();
                    return Ok(());
                }
                let a = f.get(i);
                let v = f.get(0);
                match reg {
                    2 => f.compare(v, a),
                    3 => {
                        f.compare(v, a);
                        f.pop();
                    }
                    _ => {
                        let r = f.arith(reg ^ if reg >= 4 { 1 } else { 0 }, a, v);
                        f.set(i, r);
                        if op == 0xde {
                            f.pop();
                        }
                    }
                }
            }
            (0xd9, Some((s, o))) => match reg {
                0 => {
                    let v = self.rd_f32(b, s, o)?;
                    self.fpu_ref().push(v);
                }
                2 | 3 => {
                    let v = self.fpu_ref().get(0);
                    self.wr32(b, s, o, (v as f32).to_bits())?;
                    if reg == 3 {
                        self.fpu_ref().pop();
                    }
                }
                4 => {
                    self.fpu_env_load(b, s, o, op32)?;
                }
                5 => {
                    let cw = self.rd16(b, s, o)?;
                    self.fpu_ref().cw = cw;
                }
                6 => {
                    self.fpu_env_store(b, s, o, op32)?;
                }
                7 => {
                    let cw = self.fpu_ref().cw;
                    self.wr16(b, s, o, cw)?;
                }
                _ => return Err(UD),
            },
            (0xd9, None) => {
                let f = self.fpu_ref();
                match (reg, i) {
                    (0, _) => {
                        let v = f.get(i);
                        f.push(v);
                    }
                    (1, _) => {
                        let a = f.get(0);
                        let v = f.get(i);
                        f.set(0, v);
                        f.set(i, a);
                    }
                    (2, 0) => {}
                    (3, _) => {
                        // FSTP1 (undocumented alias)
                        let v = f.get(0);
                        f.set(i, v);
                        f.pop();
                    }
                    (4, 0) => {
                        let v = f.get(0);
                        f.set(0, -v);
                    }
                    (4, 1) => {
                        let v = f.get(0);
                        f.set(0, v.abs());
                    }
                    (4, 4) => {
                        let v = f.get(0);
                        f.compare(v, 0.0);
                    }
                    (4, 5) => {
                        // FXAM
                        let v = f.get(0);
                        f.sw &= !(C0 | C1 | C2 | C3);
                        if v.is_sign_negative() {
                            f.sw |= C1;
                        }
                        if f.empty(0) {
                            f.sw |= C0 | C3;
                        } else if v.is_nan() {
                            f.sw |= C0;
                        } else if v.is_infinite() {
                            f.sw |= C0 | C2;
                        } else if v == 0.0 {
                            f.sw |= C3;
                        } else {
                            f.sw |= C2;
                        }
                    }
                    (5, k) => {
                        let v = match k {
                            0 => 1.0,
                            1 => core::f64::consts::LOG2_10,
                            2 => core::f64::consts::LOG2_E,
                            3 => core::f64::consts::PI,
                            4 => core::f64::consts::LOG10_2,
                            5 => core::f64::consts::LN_2,
                            6 => 0.0,
                            _ => return Err(UD),
                        };
                        f.push(v);
                    }
                    (6, k) => match k {
                        0 => {
                            let v = f.get(0);
                            f.set(0, libm::exp2(v) - 1.0);
                        }
                        1 => {
                            let x = f.get(0);
                            let y = f.get(1);
                            f.pop();
                            f.set(0, y * libm::log2(x));
                        }
                        2 => {
                            let v = f.get(0);
                            f.set(0, libm::tan(v));
                            f.push(1.0);
                            f.sw &= !C2;
                        }
                        3 => {
                            let x = f.get(0);
                            let y = f.get(1);
                            f.pop();
                            f.set(0, libm::atan2(y, x));
                        }
                        4 => {
                            // FXTRACT
                            let v = f.get(0);
                            let (m, e) = libm::frexp(v);
                            f.set(0, (e - 1) as f64);
                            f.push(m * 2.0);
                        }
                        5 => fprem(f, true),
                        6 => f.top = (f.top + 7) & 7,
                        _ => f.top = (f.top + 1) & 7,
                    },
                    (7, k) => match k {
                        0 => fprem(f, false),
                        1 => {
                            let x = f.get(0);
                            let y = f.get(1);
                            f.pop();
                            f.set(0, y * libm::log2(x + 1.0));
                        }
                        2 => {
                            let v = f.get(0);
                            if v < 0.0 {
                                f.sw |= IE;
                            }
                            f.set(0, libm::sqrt(v));
                        }
                        3 => {
                            let v = f.get(0);
                            f.set(0, libm::sin(v));
                            f.push(libm::cos(v));
                            f.sw &= !C2;
                        }
                        4 => {
                            let v = f.get(0);
                            let r = f.round(v);
                            f.set(0, r);
                        }
                        5 => {
                            let a = f.get(0);
                            let s = libm::trunc(f.get(1));
                            f.set(0, libm::ldexp(a, s.clamp(-20000.0, 20000.0) as i32));
                        }
                        6 => {
                            let v = f.get(0);
                            f.set(0, libm::sin(v));
                            f.sw &= !C2;
                        }
                        _ => {
                            let v = f.get(0);
                            f.set(0, libm::cos(v));
                            f.sw &= !C2;
                        }
                    },
                    _ => return Err(UD),
                }
            }
            (0xda, None) => {
                if reg == 5 && i == 1 {
                    let f = self.fpu_ref();
                    let a = f.get(0);
                    let v = f.get(1);
                    f.compare(a, v);
                    f.pop();
                    f.pop();
                } else {
                    return Err(UD);
                }
            }
            (0xdb, Some((s, o))) => match reg {
                0 => {
                    let v = self.rd_int(b, s, o, 4)?;
                    self.fpu_ref().push(v);
                }
                2 | 3 => {
                    let v = self.fpu_ref().get(0);
                    self.wr_int(b, s, o, 4, v)?;
                    if reg == 3 {
                        self.fpu_ref().pop();
                    }
                }
                5 => {
                    let v = self.rd_f80(b, s, o)?;
                    self.fpu_ref().push(v);
                }
                7 => {
                    let v = self.fpu_ref().get(0);
                    self.wr_f80(b, s, o, v)?;
                    self.fpu_ref().pop();
                }
                _ => return Err(UD),
            },
            (0xdb, None) => match (reg, i) {
                (4, 0) | (4, 1) | (4, 4) => {}
                (4, 2) => self.fpu_ref().sw &= 0x7f00,
                (4, 3) => *self.fpu_ref() = Fpu::default(),
                _ => return Err(UD),
            },
            (0xdd, Some((s, o))) => match reg {
                0 => {
                    let v = self.rd_f64(b, s, o)?;
                    self.fpu_ref().push(v);
                }
                2 | 3 => {
                    let v = self.fpu_ref().get(0);
                    self.wr_f64(b, s, o, v)?;
                    if reg == 3 {
                        self.fpu_ref().pop();
                    }
                }
                4 => {
                    // FRSTOR
                    let n = self.fpu_env_load(b, s, o, op32)?;
                    for k in 0..8u8 {
                        let v = self.rd_f80(b, s, o.wrapping_add(n + k as u32 * 10))?;
                        let f = self.fpu_ref();
                        let p = f.phys(k);
                        f.st[p] = v;
                    }
                }
                6 => {
                    // FNSAVE
                    let n = self.fpu_env_store(b, s, o, op32)?;
                    for k in 0..8u8 {
                        let v = self.fpu_ref().get(k);
                        self.wr_f80(b, s, o.wrapping_add(n + k as u32 * 10), v)?;
                    }
                    *self.fpu_ref() = Fpu::default();
                }
                7 => {
                    let sw = self.fpu_ref().status();
                    self.wr16(b, s, o, sw)?;
                }
                _ => return Err(UD),
            },
            (0xdd, None) => {
                let f = self.fpu_ref();
                match reg {
                    0 => {
                        let p = f.phys(i);
                        f.set_tag(p, 3);
                    }
                    2 | 3 => {
                        let v = f.get(0);
                        f.set(i, v);
                        if reg == 3 {
                            f.pop();
                        }
                    }
                    4 | 5 => {
                        let a = f.get(0);
                        let v = f.get(i);
                        f.compare(a, v);
                        if reg == 5 {
                            f.pop();
                        }
                    }
                    _ => return Err(UD),
                }
            }
            (0xdf, Some((s, o))) => match reg {
                0 => {
                    let v = self.rd_int(b, s, o, 2)?;
                    self.fpu_ref().push(v);
                }
                2 | 3 => {
                    let v = self.fpu_ref().get(0);
                    self.wr_int(b, s, o, 2, v)?;
                    if reg == 3 {
                        self.fpu_ref().pop();
                    }
                }
                4 => {
                    // FBLD: 18 BCD digits and a sign byte.
                    let mut v = 0f64;
                    for k in (0..9).rev() {
                        let x = self.rd8(b, s, o.wrapping_add(k))?;
                        v = v * 100.0 + ((x >> 4) * 10 + (x & 15)) as f64;
                    }
                    if self.rd8(b, s, o.wrapping_add(9))? & 0x80 != 0 {
                        v = -v;
                    }
                    self.fpu_ref().push(v);
                }
                5 => {
                    let v = self.rd_int(b, s, o, 8)?;
                    self.fpu_ref().push(v);
                }
                6 => {
                    // FBSTP
                    let v = self.fpu_ref().get(0);
                    let r = self.fpu_ref().round(v);
                    let mut n = r.abs() as u64;
                    for k in 0..9 {
                        let lo = (n % 10) as u8;
                        n /= 10;
                        let hi = (n % 10) as u8;
                        n /= 10;
                        self.wr8(b, s, o.wrapping_add(k), hi << 4 | lo)?;
                    }
                    self.wr8(b, s, o.wrapping_add(9), if r < 0.0 { 0x80 } else { 0 })?;
                    self.fpu_ref().pop();
                }
                7 => {
                    let v = self.fpu_ref().get(0);
                    self.wr_int(b, s, o, 8, v)?;
                    self.fpu_ref().pop();
                }
                _ => return Err(UD),
            },
            (0xdf, None) => {
                if reg == 4 && i == 0 {
                    let sw = self.fpu_ref().status();
                    self.set_ax(sw);
                } else if reg == 0 {
                    // FFREEP
                    let f = self.fpu_ref();
                    let p = f.phys(i);
                    f.set_tag(p, 3);
                    f.pop();
                } else {
                    return Err(UD);
                }
            }
            _ => return Err(UD),
        }
        Ok(())
    }
}
