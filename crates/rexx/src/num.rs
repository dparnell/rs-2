//! REXX decimal arithmetic: numbers are strings, arithmetic is done in
//! decimal to NUMERIC DIGITS significant digits (rounded half up), and
//! results are formatted back by REXX's rules (plain notation unless the
//! integer part would need more than DIGITS digits or the fraction more
//! than twice DIGITS, then scientific or engineering notation).

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::Err;

/// NUMERIC settings.
#[derive(Clone, Copy, PartialEq)]
pub struct Numeric {
    pub digits: usize,
    pub fuzz: usize,
    pub engineering: bool,
}

impl Default for Numeric {
    fn default() -> Self {
        Numeric { digits: 9, fuzz: 0, engineering: false }
    }
}

/// A decimal number: (-1)^neg * coefficient * 10^exp. The coefficient
/// has no leading zeros; zero has an empty coefficient.
#[derive(Clone, Debug)]
pub struct Num {
    pub neg: bool,
    pub dig: Vec<u8>,
    pub exp: i64,
}

fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

impl Num {
    pub fn zero() -> Num {
        Num { neg: false, dig: Vec::new(), exp: 0 }
    }

    pub fn from_i64(v: i64) -> Num {
        let mut dig = Vec::new();
        let mut m = v.unsigned_abs();
        while m > 0 {
            dig.push((m % 10) as u8);
            m /= 10;
        }
        dig.reverse();
        Num { neg: v < 0 && !dig.is_empty(), dig, exp: 0 }
    }

    pub fn is_zero(&self) -> bool {
        self.dig.is_empty()
    }

    /// Parse a REXX number: blanks, an optional sign (blanks may follow
    /// it), digits with at most one period, an optional exponent, blanks.
    pub fn parse(s: &[u8]) -> Option<Num> {
        let mut i = 0;
        let n = s.len();
        while i < n && is_blank(s[i]) {
            i += 1;
        }
        let mut neg = false;
        if i < n && (s[i] == b'+' || s[i] == b'-') {
            neg = s[i] == b'-';
            i += 1;
            while i < n && is_blank(s[i]) {
                i += 1;
            }
        }
        let mut dig = Vec::new();
        let mut exp: i64 = 0;
        let mut seen_digit = false;
        let mut seen_point = false;
        while i < n {
            let c = s[i];
            if c.is_ascii_digit() {
                seen_digit = true;
                if !(dig.is_empty() && c == b'0') {
                    dig.push(c - b'0');
                } else if seen_point {
                    // leading zero after the point still scales
                }
                if seen_point {
                    exp -= 1;
                }
            } else if c == b'.' && !seen_point {
                seen_point = true;
            } else {
                break;
            }
            i += 1;
        }
        if !seen_digit {
            return None;
        }
        if i < n && (s[i] == b'e' || s[i] == b'E') {
            i += 1;
            let mut eneg = false;
            if i < n && (s[i] == b'+' || s[i] == b'-') {
                eneg = s[i] == b'-';
                i += 1;
            }
            let start = i;
            let mut e: i64 = 0;
            while i < n && s[i].is_ascii_digit() {
                e = e.saturating_mul(10).saturating_add((s[i] - b'0') as i64);
                i += 1;
            }
            if i == start {
                return None;
            }
            exp += if eneg { -e } else { e };
        }
        while i < n && is_blank(s[i]) {
            i += 1;
        }
        if i != n {
            return None;
        }
        let mut r = Num { neg, dig, exp };
        r.normalize_zero();
        Some(r)
    }

    fn normalize_zero(&mut self) {
        // strip leading zeros (can appear after arithmetic)
        let lead = self.dig.iter().take_while(|&&d| d == 0).count();
        if lead > 0 {
            self.dig.drain(..lead);
        }
        if self.dig.is_empty() {
            self.neg = false;
            self.exp = 0;
        }
    }

    /// Round to `digits` significant digits (half up).
    pub fn round(&mut self, digits: usize) {
        self.normalize_zero();
        if self.dig.len() <= digits {
            return;
        }
        let cut = self.dig.len() - digits;
        let up = self.dig[digits] >= 5;
        self.dig.truncate(digits);
        self.exp += cut as i64;
        if up {
            let mut i = digits;
            loop {
                if i == 0 {
                    self.dig.insert(0, 1);
                    // now digits+1 long: drop the last (a zero)
                    self.dig.pop();
                    self.exp += 1;
                    break;
                }
                i -= 1;
                if self.dig[i] == 9 {
                    self.dig[i] = 0;
                } else {
                    self.dig[i] += 1;
                    break;
                }
            }
        }
        self.normalize_zero();
    }

    /// Remove trailing zeros of the fraction (division results).
    fn strip_fraction_zeros(&mut self) {
        while self.exp < 0 && self.dig.last() == Some(&0) {
            self.dig.pop();
            self.exp += 1;
        }
        self.normalize_zero();
    }

    /// Remove all trailing zeros (FORMAT/whole-number checks).
    pub fn strip_all_zeros(&mut self) {
        while self.dig.last() == Some(&0) {
            self.dig.pop();
            self.exp += 1;
        }
        self.normalize_zero();
    }

    /// An operand rounded to the working precision.
    pub fn operand(&self, digits: usize) -> Num {
        let mut r = self.clone();
        r.round(digits);
        r
    }

    fn cmp_abs(a: &Num, b: &Num) -> Ordering {
        if a.is_zero() || b.is_zero() {
            return a.dig.len().cmp(&b.dig.len()).then(Ordering::Equal).max(if a.is_zero() && b.is_zero() {
                Ordering::Equal
            } else if a.is_zero() {
                Ordering::Less
            } else {
                Ordering::Greater
            });
        }
        let ma = a.dig.len() as i64 + a.exp;
        let mb = b.dig.len() as i64 + b.exp;
        if ma != mb {
            return ma.cmp(&mb);
        }
        let n = a.dig.len().max(b.dig.len());
        for i in 0..n {
            let da = a.dig.get(i).copied().unwrap_or(0);
            let db = b.dig.get(i).copied().unwrap_or(0);
            if da != db {
                return da.cmp(&db);
            }
        }
        Ordering::Equal
    }

    /// Coefficient aligned to exponent `e` (e <= self.exp).
    fn aligned(&self, e: i64) -> Vec<u8> {
        let mut v = self.dig.clone();
        for _ in 0..(self.exp - e) {
            v.push(0);
        }
        v
    }

    fn add_mag(a: &[u8], b: &[u8]) -> Vec<u8> {
        let n = a.len().max(b.len());
        let mut r = vec![0u8; n + 1];
        let mut carry = 0;
        for i in 0..n {
            let da = if i < a.len() { a[a.len() - 1 - i] } else { 0 };
            let db = if i < b.len() { b[b.len() - 1 - i] } else { 0 };
            let s = da + db + carry;
            r[n - i] = s % 10;
            carry = s / 10;
        }
        r[0] = carry;
        r
    }

    /// a - b with |a| >= |b|.
    fn sub_mag(a: &[u8], b: &[u8]) -> Vec<u8> {
        let n = a.len();
        let mut r = vec![0u8; n];
        let mut borrow = 0i8;
        for i in 0..n {
            let da = a[n - 1 - i] as i8;
            let db = if i < b.len() { b[b.len() - 1 - i] as i8 } else { 0 };
            let mut s = da - db - borrow;
            if s < 0 {
                s += 10;
                borrow = 1;
            } else {
                borrow = 0;
            }
            r[n - 1 - i] = s as u8;
        }
        r
    }

    pub fn add(a: &Num, b: &Num, nm: &Numeric) -> Num {
        let a = a.operand(nm.digits);
        let b = b.operand(nm.digits);
        let mut r = Num::add_exact(&a, &b);
        r.round(nm.digits);
        r
    }

    fn add_exact(a: &Num, b: &Num) -> Num {
        if a.is_zero() && b.is_zero() {
            // keep the larger number of decimals as REXX does (0.0 + 0 = 0)
            return Num::zero();
        }
        // An operand that is zero still contributes its exponent so that
        // 1.50 + 0.00 keeps the decimals, but zero has no coefficient.
        let e = if a.is_zero() { b.exp } else if b.is_zero() { a.exp } else { a.exp.min(b.exp) };
        let av = if a.is_zero() { Vec::new() } else { a.aligned(e) };
        let bv = if b.is_zero() { Vec::new() } else { b.aligned(e) };
        let ta = Num { neg: a.neg, dig: av, exp: e };
        let tb = Num { neg: b.neg, dig: bv, exp: e };
        let mut r = if ta.neg == tb.neg {
            Num { neg: ta.neg, dig: Num::add_mag(&ta.dig, &tb.dig), exp: e }
        } else {
            match Num::cmp_abs(&ta, &tb) {
                Ordering::Equal => return Num::zero(),
                Ordering::Greater => Num { neg: ta.neg, dig: Num::sub_mag(&ta.dig, &tb.dig), exp: e },
                Ordering::Less => Num { neg: tb.neg, dig: Num::sub_mag(&tb.dig, &ta.dig), exp: e },
            }
        };
        r.normalize_zero();
        r
    }

    pub fn neg(&self) -> Num {
        let mut r = self.clone();
        if !r.is_zero() {
            r.neg = !r.neg;
        }
        r
    }

    pub fn sub(a: &Num, b: &Num, nm: &Numeric) -> Num {
        Num::add(a, &b.neg(), nm)
    }

    pub fn mul(a: &Num, b: &Num, nm: &Numeric) -> Num {
        let a = a.operand(nm.digits);
        let b = b.operand(nm.digits);
        let mut r = Num::mul_exact(&a, &b);
        r.round(nm.digits);
        r
    }

    fn mul_exact(a: &Num, b: &Num) -> Num {
        if a.is_zero() || b.is_zero() {
            return Num::zero();
        }
        let mut r = vec![0u32; a.dig.len() + b.dig.len()];
        for (i, &da) in a.dig.iter().enumerate().rev() {
            for (j, &db) in b.dig.iter().enumerate().rev() {
                r[i + j + 1] += da as u32 * db as u32;
            }
        }
        for k in (1..r.len()).rev() {
            let c = r[k] / 10;
            r[k] %= 10;
            r[k - 1] += c;
        }
        let mut n = Num { neg: a.neg != b.neg, dig: r.into_iter().map(|d| d as u8).collect(), exp: a.exp + b.exp };
        n.normalize_zero();
        n
    }

    /// Long division: (quotient digits, exponent) to `prec` digits, plus
    /// whether there is a remainder.
    fn div_digits(a: &Num, b: &Num, prec: usize) -> Num {
        // Scale so both coefficients are integers; compute
        // a.dig * 10^k / b.dig with enough k for prec digits.
        let mut rem: Vec<u8> = Vec::new();
        let mut q: Vec<u8> = Vec::new();
        let mut exp = a.exp - b.exp;
        let mut ai = 0usize;
        loop {
            // bring down the next digit
            let d = if ai < a.dig.len() {
                a.dig[ai]
            } else {
                if rem.iter().all(|&x| x == 0) {
                    break;
                }
                exp -= 1;
                0
            };
            ai += 1;
            rem.push(d);
            let lead = rem.iter().take_while(|&&x| x == 0).count();
            rem.drain(..lead.min(rem.len().saturating_sub(1)));
            let mut count = 0u8;
            while Num::cmp_vec(&rem, &b.dig) != Ordering::Less {
                rem = Num::sub_mag(&rem, &b.dig);
                let lead = rem.iter().take_while(|&&x| x == 0).count();
                rem.drain(..lead.min(rem.len().saturating_sub(1)));
                count += 1;
            }
            if !(q.is_empty() && count == 0) {
                q.push(count);
            }
            if q.len() > prec {
                break;
            }
        }
        // digits of a beyond what was consumed scale the exponent
        if ai < a.dig.len() {
            exp += (a.dig.len() - ai) as i64;
        }
        let mut n = Num { neg: a.neg != b.neg, dig: q, exp };
        n.normalize_zero();
        n
    }

    fn cmp_vec(a: &[u8], b: &[u8]) -> Ordering {
        let a0 = a.iter().take_while(|&&x| x == 0).count();
        let b0 = b.iter().take_while(|&&x| x == 0).count();
        let a = &a[a0..];
        let b = &b[b0..];
        a.len().cmp(&b.len()).then_with(|| a.cmp(b))
    }

    pub fn div(a: &Num, b: &Num, nm: &Numeric) -> Result<Num, Err> {
        let a = a.operand(nm.digits);
        let b = b.operand(nm.digits);
        if b.is_zero() {
            return Err(Err::new(42));
        }
        if a.is_zero() {
            return Ok(Num::zero());
        }
        let mut r = Num::div_digits(&a, &b, nm.digits + 1);
        r.round(nm.digits);
        r.strip_fraction_zeros();
        Ok(r)
    }

    /// Integer division (truncated toward zero) and remainder, exact.
    fn int_div_rem(a: &Num, b: &Num, nm: &Numeric) -> Result<(Num, Num), Err> {
        if b.is_zero() {
            return Err(Err::new(42));
        }
        // integer part of a/b
        let mut q = Num::div_digits(a, b, nm.digits + 2 + a.exp.unsigned_abs().min(1000) as usize);
        // truncate fraction
        if q.exp < 0 {
            let cut = (-q.exp) as usize;
            if cut >= q.dig.len() {
                q = Num::zero();
            } else {
                q.dig.truncate(q.dig.len() - cut);
                q.exp = 0;
            }
        }
        q.normalize_zero();
        // the integer quotient must fit in DIGITS
        let intlen = if q.is_zero() { 0 } else { q.dig.len() as i64 + q.exp };
        if intlen > nm.digits as i64 {
            return Err(Err::new(26));
        }
        let prod = Num::mul_exact(&q, b);
        let rem = Num::add_exact(a, &prod.neg());
        Ok((q, rem))
    }

    pub fn idiv(a: &Num, b: &Num, nm: &Numeric) -> Result<Num, Err> {
        let a = a.operand(nm.digits);
        let b = b.operand(nm.digits);
        let (mut q, _) = Num::int_div_rem(&a, &b, nm)?;
        q.round(nm.digits);
        Ok(q)
    }

    pub fn rem(a: &Num, b: &Num, nm: &Numeric) -> Result<Num, Err> {
        let a = a.operand(nm.digits);
        let b = b.operand(nm.digits);
        let (_, mut r) = Num::int_div_rem(&a, &b, nm)?;
        if !r.is_zero() {
            r.neg = a.neg;
        }
        r.round(nm.digits);
        Ok(r)
    }

    pub fn power(a: &Num, b: &Num, nm: &Numeric) -> Result<Num, Err> {
        let n = b.to_whole(nm.digits).ok_or(Err::new(26))?;
        let a = a.operand(nm.digits);
        if n == 0 {
            return Ok(Num::from_i64(1));
        }
        let mut e = n.unsigned_abs();
        let elen = {
            let mut l = 0;
            let mut t = e;
            while t > 0 {
                l += 1;
                t /= 10;
            }
            l
        };
        let work = nm.digits + elen + 1;
        // left-to-right binary method
        let mut r = Num::from_i64(1);
        let mut started = false;
        let bits = 64 - e.leading_zeros();
        for i in (0..bits).rev() {
            if started {
                r = Num::mul_exact(&r, &r);
                r.round(work);
            }
            if (e >> i) & 1 == 1 {
                r = Num::mul_exact(&r, &a);
                r.round(work);
                started = true;
            }
        }
        e = 0;
        let _ = e;
        if n < 0 {
            let nm2 = Numeric { digits: work, ..*nm };
            if r.is_zero() {
                return Err(Err::new(42));
            }
            r = Num::div(&Num::from_i64(1), &r, &nm2)?;
            r.round(nm.digits);
            r.strip_fraction_zeros();
        } else {
            r.round(nm.digits);
        }
        Ok(r)
    }

    /// Compare numerically with DIGITS - FUZZ precision.
    pub fn compare(a: &Num, b: &Num, nm: &Numeric) -> Ordering {
        let d = nm.digits.saturating_sub(nm.fuzz).max(1);
        let n2 = Numeric { digits: d, ..*nm };
        let r = Num::sub(a, b, &n2);
        if r.is_zero() {
            Ordering::Equal
        } else if r.neg {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    /// The value as a whole number, if it is one within `digits`.
    pub fn to_whole(&self, digits: usize) -> Option<i64> {
        let mut r = self.clone();
        r.round(digits);
        if r.is_zero() {
            return Some(0);
        }
        // fraction digits must be zero
        if r.exp < 0 {
            let cut = (-r.exp) as usize;
            if cut > r.dig.len() {
                return None;
            }
            if r.dig[r.dig.len() - cut..].iter().any(|&d| d != 0) {
                return None;
            }
            r.dig.truncate(r.dig.len() - cut);
            r.exp = 0;
        }
        let mut v: i64 = 0;
        for &d in &r.dig {
            v = v.checked_mul(10)?.checked_add(d as i64)?;
        }
        for _ in 0..r.exp {
            v = v.checked_mul(10)?;
        }
        Some(if r.neg { -v } else { v })
    }

    /// Format for display (the result of an arithmetic operation, already
    /// rounded to `nm.digits`).
    pub fn format(&self, nm: &Numeric) -> Vec<u8> {
        if self.is_zero() {
            return b"0".to_vec();
        }
        let mut out = Vec::new();
        if self.neg {
            out.push(b'-');
        }
        let n = self.dig.len() as i64;
        let e = self.exp;
        let digits = nm.digits as i64;
        let plain = if e >= 0 { n + e <= digits } else { -e <= 2 * digits && n + e <= digits };
        let digit = |d: u8| d + b'0';
        if plain {
            if e >= 0 {
                out.extend(self.dig.iter().map(|&d| digit(d)));
                for _ in 0..e {
                    out.push(b'0');
                }
            } else {
                let ip = n + e; // digits before the point
                if ip > 0 {
                    out.extend(self.dig[..ip as usize].iter().map(|&d| digit(d)));
                    out.push(b'.');
                    out.extend(self.dig[ip as usize..].iter().map(|&d| digit(d)));
                } else {
                    out.extend_from_slice(b"0.");
                    for _ in 0..(-ip) {
                        out.push(b'0');
                    }
                    out.extend(self.dig.iter().map(|&d| digit(d)));
                }
            }
            return out;
        }
        // exponential notation
        let mut expo = e + n - 1;
        let mut before = 1i64;
        if nm.engineering {
            let m = expo.rem_euclid(3);
            expo -= m;
            before += m;
        }
        let mut ds: Vec<u8> = self.dig.iter().map(|&d| digit(d)).collect();
        while (ds.len() as i64) < before {
            ds.push(b'0');
        }
        out.extend_from_slice(&ds[..before as usize]);
        if ds.len() as i64 > before {
            out.push(b'.');
            out.extend_from_slice(&ds[before as usize..]);
        }
        if expo != 0 {
            out.push(b'E');
            out.push(if expo < 0 { b'-' } else { b'+' });
            out.extend_from_slice(alloc::format!("{}", expo.unsigned_abs()).as_bytes());
        }
        out
    }
}

/// Format an integer as a REXX string.
pub fn int_str(v: i64) -> Vec<u8> {
    alloc::format!("{v}").into_bytes()
}
