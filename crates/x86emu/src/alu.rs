//! Arithmetic and logic with their flags. Operand sizes are in bytes
//! (1, 2, 4); values are kept zero-extended in a u32.

use crate::flags::*;
use crate::Cpu;

#[inline]
pub(crate) const fn mask(sz: u8) -> u32 {
    match sz {
        1 => 0xff,
        2 => 0xffff,
        _ => 0xffff_ffff,
    }
}

#[inline]
pub(crate) const fn sign(sz: u8) -> u32 {
    match sz {
        1 => 0x80,
        2 => 0x8000,
        _ => 0x8000_0000,
    }
}

/// Sign-extend a value of size `sz` to 32 bits.
#[inline]
pub(crate) const fn sext(v: u32, sz: u8) -> u32 {
    match sz {
        1 => v as u8 as i8 as i32 as u32,
        2 => v as u16 as i16 as i32 as u32,
        _ => v,
    }
}

/// PF for each byte value (i686 has no POPCNT to count bits fast).
static PARITY: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = if (i as u8).count_ones() & 1 == 0 { PF as u8 } else { 0 };
        i += 1;
    }
    t
};

/// SF, ZF and PF of result `r` of size `sz`.
#[inline(always)]
fn szp(r: u32, sz: u8) -> u32 {
    let bits = sz as u32 * 8;
    let sf = (r >> (bits - 1)) & 1;
    PARITY[(r & 0xff) as usize] as u32 | ((r & mask(sz) == 0) as u32) << 6 | sf << 7
}

impl Cpu {
    #[inline(always)]
    pub(crate) fn set_szp(&mut self, r: u32, sz: u8) {
        self.flags = (self.flags & !(ZF | SF | PF)) | szp(r, sz);
    }

    #[inline]
    pub(crate) fn set_cf_of(&mut self, cf: bool, of: bool) {
        self.flags = (self.flags & !(CF | OF)) | if cf { CF } else { 0 } | if of { OF } else { 0 };
    }

    #[inline]
    pub(crate) fn add(&mut self, a: u32, b: u32, c: u32, sz: u8) -> u32 {
        let m = mask(sz);
        let wide = a as u64 + b as u64 + c as u64;
        let r = wide as u32 & m;
        let bits = sz as u32 * 8;
        let cf = (wide > m as u64) as u32;
        let of = (((a ^ r) & (b ^ r)) >> (bits - 1)) & 1;
        let af = (a ^ b ^ r) & AF;
        self.flags = (self.flags & !ARITH) | szp(r, sz) | cf | af | of << 11;
        r
    }

    #[inline]
    pub(crate) fn sub(&mut self, a: u32, b: u32, c: u32, sz: u8) -> u32 {
        let m = mask(sz);
        let r = a.wrapping_sub(b).wrapping_sub(c) & m;
        let bits = sz as u32 * 8;
        let cf = ((b as u64 + c as u64) > a as u64) as u32;
        let of = (((a ^ b) & (a ^ r)) >> (bits - 1)) & 1;
        let af = (a ^ b ^ r) & AF;
        self.flags = (self.flags & !ARITH) | szp(r, sz) | cf | af | of << 11;
        r
    }

    #[inline]
    pub(crate) fn logic(&mut self, r: u32, sz: u8) -> u32 {
        let r = r & mask(sz);
        self.flags = (self.flags & !ARITH) | szp(r, sz);
        r
    }

    /// ALU operation 0-7 (ADD OR ADC SBB AND SUB XOR CMP); CMP returns
    /// `a` unchanged.
    #[inline]
    pub(crate) fn alu(&mut self, op: u8, a: u32, b: u32, sz: u8) -> u32 {
        match op {
            0 => self.add(a, b, 0, sz),
            1 => self.logic(a | b, sz),
            2 => {
                let c = self.flags & CF;
                self.add(a, b, c, sz)
            }
            3 => {
                let c = self.flags & CF;
                self.sub(a, b, c, sz)
            }
            4 => self.logic(a & b, sz),
            5 => self.sub(a, b, 0, sz),
            6 => self.logic(a ^ b, sz),
            _ => {
                self.sub(a, b, 0, sz);
                a
            }
        }
    }

    pub(crate) fn inc(&mut self, a: u32, sz: u8) -> u32 {
        let cf = self.flags & CF;
        let r = self.add(a, 1, 0, sz);
        self.flags = (self.flags & !CF) | cf;
        r
    }

    pub(crate) fn dec(&mut self, a: u32, sz: u8) -> u32 {
        let cf = self.flags & CF;
        let r = self.sub(a, 1, 0, sz);
        self.flags = (self.flags & !CF) | cf;
        r
    }

    pub(crate) fn neg(&mut self, a: u32, sz: u8) -> u32 {
        let r = self.sub(0, a, 0, sz);
        self.set_flag(CF, a & mask(sz) != 0);
        r
    }

    /// Shift/rotate group 2: ROL ROR RCL RCR SHL SHR SAL SAR by `count`
    /// (already masked to 5 bits).
    pub(crate) fn shift(&mut self, op: u8, a: u32, count: u32, sz: u8) -> u32 {
        let bits = sz as u32 * 8;
        let m = mask(sz);
        let a = a & m;
        if count == 0 {
            return a;
        }
        let sb = sign(sz);
        match op {
            0 => {
                // ROL
                let n = count % bits;
                let r = if n == 0 { a } else { ((a << n) | (a >> (bits - n))) & m };
                let cf = r & 1 != 0;
                let of = (r & sb != 0) != cf;
                self.set_cf_of(cf, of);
                r
            }
            1 => {
                // ROR
                let n = count % bits;
                let r = if n == 0 { a } else { ((a >> n) | (a << (bits - n))) & m };
                let cf = r & sb != 0;
                let of = (r & sb != 0) != (r & (sb >> 1) != 0);
                self.set_cf_of(cf, of);
                r
            }
            2 => {
                // RCL through CF: a (bits+1)-bit rotate.
                let n = count % (bits + 1);
                if n == 0 {
                    return a;
                }
                let mut r = a as u64 | if self.flags & CF != 0 { 1u64 << bits } else { 0 };
                let w = bits as u64 + 1;
                let mw = (1u64 << w) - 1;
                r = ((r << n) | (r >> (w - n as u64))) & mw;
                let cf = r >> bits & 1 != 0;
                let res = r as u32 & m;
                let of = (res & sb != 0) != cf;
                self.set_cf_of(cf, of);
                res
            }
            3 => {
                // RCR
                let n = count % (bits + 1);
                if n == 0 {
                    return a;
                }
                let of = (a & sb != 0) != (self.flags & CF != 0);
                let mut r = a as u64 | if self.flags & CF != 0 { 1u64 << bits } else { 0 };
                let w = bits as u64 + 1;
                let mw = (1u64 << w) - 1;
                r = ((r >> n) | (r << (w - n as u64))) & mw;
                let cf = r >> bits & 1 != 0;
                let res = r as u32 & m;
                // OF as for a one-bit rotate: the sign before XOR CF.
                let of = if n == 1 { of } else { (res & sb != 0) != (res & (sb >> 1) != 0) };
                self.set_cf_of(cf, of);
                res
            }
            4 | 6 => {
                // SHL
                let (r, cf) = if count <= bits { (((a as u64) << count) as u32 & m, (a as u64) << count >> bits & 1 != 0) } else { (0, false) };
                let of = (r & sb != 0) != cf;
                self.set_szp(r, sz);
                self.set_cf_of(cf, of);
                self.flags &= !AF;
                r
            }
            5 => {
                // SHR
                let (r, cf) = if count <= bits { ((a as u64 >> count) as u32, (a as u64) >> (count - 1) & 1 != 0) } else { (0, false) };
                let of = if count == 1 { a & sb != 0 } else { false };
                self.set_szp(r, sz);
                self.set_cf_of(cf, of);
                self.flags &= !AF;
                r
            }
            _ => {
                // SAR
                let s = sext(a, sz) as i32;
                let n = count.min(bits - 1);
                let r = (s >> n) as u32 & m;
                let cf = if count >= bits { s < 0 } else { (s >> (count - 1)) & 1 != 0 };
                self.set_szp(r, sz);
                self.set_cf_of(cf, false);
                self.flags &= !AF;
                r
            }
        }
    }

    /// SHLD/SHRD: shift `a` by `count` (masked to 5 bits), filling from `b`.
    pub(crate) fn shiftd(&mut self, left: bool, a: u32, b: u32, count: u32, sz: u8) -> u32 {
        let count = count & 31;
        if count == 0 {
            return a & mask(sz);
        }
        let bits = sz as u32 * 8;
        let m = mask(sz);
        let (r, cf) = if sz == 2 {
            // 16-bit: a 32-bit a:b (or b:a) value; counts above 16 are
            // undefined, take the Intel behaviour of a 32-bit pattern.
            if left {
                let v = ((a & 0xffff) << 16 | (b & 0xffff)) as u64;
                let v = v << 32 | v;
                let r = (v << count >> 48) as u32 & m;
                let cf = (v << (count - 1)) >> 63 != 0 && count <= 32;
                (r, if count <= 16 { (a >> (16 - count)) & 1 != 0 } else { cf })
            } else {
                let v = ((b & 0xffff) << 16 | (a & 0xffff)) as u64;
                let v = v << 32 | v;
                let r = (v >> count) as u32 & m;
                (r, (v >> (count - 1)) & 1 != 0)
            }
        } else if left {
            let v = (a as u64) << 32 | b as u64;
            ((v << count >> 32) as u32, (a >> (bits - count)) & 1 != 0)
        } else {
            let v = (b as u64) << 32 | a as u64;
            ((v >> count) as u32, (a >> (count - 1)) & 1 != 0)
        };
        let of = (r ^ a) & sign(sz) != 0;
        self.set_szp(r, sz);
        self.set_cf_of(cf, of);
        r
    }
}
