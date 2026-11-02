//! Built-in functions.

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::exec::{Frame, Interp, R, State, Val, compare, strip_blanks, whole, words};
use crate::num::{Num, Numeric, int_str};
use crate::parse::Cond;
use crate::run::cond_index;
use crate::{Err, stream};

fn e40<T>() -> R<T> {
    Err(Err::new(40))
}

/// Argument access with REXX's checks (error 40 on misuse).
struct A<'a> {
    a: &'a [Option<Val>],
    digits: usize,
}

impl<'a> A<'a> {
    fn count(&self, min: usize, max: usize) -> R<()> {
        // omitted trailing arguments count as absent
        let n = self.a.len();
        if n > max {
            return e40();
        }
        for i in 0..min {
            if self.a.get(i).is_none_or(|x| x.is_none()) {
                return e40();
            }
        }
        Ok(())
    }
    fn has(&self, i: usize) -> bool {
        self.a.get(i).is_some_and(|x| x.is_some())
    }
    fn s(&self, i: usize) -> &'a [u8] {
        self.a.get(i).and_then(|x| x.as_deref()).unwrap_or(b"")
    }
    fn opt(&self, i: usize) -> Option<&'a [u8]> {
        self.a.get(i).and_then(|x| x.as_deref())
    }
    fn whole(&self, i: usize) -> R<Option<i64>> {
        match self.opt(i) {
            None => Ok(None),
            Some(v) => whole(v, self.digits).map(Some).map_err(|_| Err::new(40)),
        }
    }
    /// Non-negative whole number.
    fn nonneg(&self, i: usize, def: i64) -> R<i64> {
        let v = self.whole(i)?.unwrap_or(def);
        if v < 0 { e40() } else { Ok(v) }
    }
    /// Positive whole number.
    fn pos(&self, i: usize, def: i64) -> R<i64> {
        let v = self.whole(i)?.unwrap_or(def);
        if v < 1 { e40() } else { Ok(v) }
    }
    fn pad(&self, i: usize) -> R<u8> {
        match self.opt(i) {
            None => Ok(b' '),
            Some(p) if p.len() == 1 => Ok(p[0]),
            _ => e40(),
        }
    }
    /// Option letter (first character, upper case).
    fn letter(&self, i: usize, def: u8, allowed: &[u8]) -> R<u8> {
        match self.opt(i) {
            None => Ok(def),
            Some(o) if !o.is_empty() => {
                let c = o[0].to_ascii_uppercase();
                if allowed.contains(&c) { Ok(c) } else { e40() }
            }
            _ => e40(),
        }
    }
    fn num(&self, i: usize) -> R<Num> {
        Num::parse(self.s(i)).ok_or(Err::new(40))
    }
}

fn fmt(n: &Num, nm: &Numeric) -> Val {
    let mut r = n.operand(nm.digits);
    r.round(nm.digits);
    r.format(nm)
}

fn hexval(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// Validate a hex string with optional blanks between byte pairs; returns
/// the digits (or None).
fn hex_digits(s: &[u8]) -> Option<Vec<u8>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    if s[0] == b' ' || s[s.len() - 1] == b' ' {
        return None;
    }
    let groups: Vec<&[u8]> = s.split(|&c| c == b' ').filter(|g| !g.is_empty()).collect();
    let mut d = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        if i > 0 && g.len() % 2 != 0 {
            return None;
        }
        for &c in g.iter() {
            d.push(hexval(c)?);
        }
    }
    Some(d)
}

fn bin_digits(s: &[u8]) -> Option<Vec<u8>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    if s[0] == b' ' || s[s.len() - 1] == b' ' {
        return None;
    }
    let groups: Vec<&[u8]> = s.split(|&c| c == b' ').filter(|g| !g.is_empty()).collect();
    let mut d = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        if i > 0 && g.len() % 4 != 0 {
            return None;
        }
        for &c in g.iter() {
            match c {
                b'0' => d.push(0),
                b'1' => d.push(1),
                _ => return None,
            }
        }
    }
    Some(d)
}

fn x2c(s: &[u8]) -> R<Val> {
    let mut d = hex_digits(s).ok_or(Err::new(40))?;
    if d.len() % 2 == 1 {
        d.insert(0, 0);
    }
    Ok(d.chunks(2).map(|p| p[0] << 4 | p[1]).collect())
}

fn c2x(s: &[u8]) -> Val {
    let h = b"0123456789ABCDEF";
    let mut v = Vec::with_capacity(s.len() * 2);
    for &c in s {
        v.push(h[(c >> 4) as usize]);
        v.push(h[(c & 15) as usize]);
    }
    v
}

/// Unsigned big integer (decimal digits) from bytes.
fn bytes_to_dec(b: &[u8]) -> Vec<u8> {
    let mut dec: Vec<u8> = vec![0];
    for &byte in b {
        // dec = dec * 256 + byte
        let mut carry = byte as u32;
        for d in dec.iter_mut().rev() {
            let v = *d as u32 * 256 + carry;
            *d = (v % 10) as u8;
            carry = v / 10;
        }
        while carry > 0 {
            dec.insert(0, (carry % 10) as u8);
            carry /= 10;
        }
    }
    let lead = dec.iter().take_while(|&&d| d == 0).count();
    if lead == dec.len() {
        return vec![0];
    }
    dec[lead..].to_vec()
}

fn dec_str(d: &[u8], neg: bool) -> Val {
    let mut v = Vec::new();
    if neg && !(d.len() == 1 && d[0] == 0) {
        v.push(b'-');
    }
    v.extend(d.iter().map(|x| x + b'0'));
    v
}

/// Two's complement negate in place.
fn negate(b: &mut [u8]) {
    for x in b.iter_mut() {
        *x = !*x;
    }
    for x in b.iter_mut().rev() {
        let (v, o) = x.overflowing_add(1);
        *x = v;
        if !o {
            break;
        }
    }
}

/// Whole number to bytes (two's complement) of `len` bytes, or minimal.
fn int_to_bytes(n: &Num, len: Option<usize>) -> R<Val> {
    // decimal digits of |n| (n is whole)
    let mut digits: Vec<u8> = n.dig.clone();
    for _ in 0..n.exp.max(0) {
        digits.push(0);
    }
    if n.exp < 0 {
        return e40();
    }
    let mut bytes: Vec<u8> = Vec::new();
    // repeated division by 256
    while !(digits.is_empty() || digits.iter().all(|&d| d == 0)) {
        let mut rem = 0u32;
        let mut q = Vec::with_capacity(digits.len());
        for &d in &digits {
            let v = rem * 10 + d as u32;
            q.push((v / 256) as u8);
            rem = v % 256;
        }
        bytes.insert(0, rem as u8);
        let lead = q.iter().take_while(|&&d| d == 0).count();
        digits = q[lead..].to_vec();
    }
    match len {
        None => {
            if n.neg {
                return e40();
            }
            if bytes.is_empty() {
                bytes.push(0);
            }
            Ok(bytes)
        }
        Some(l) => {
            let mut out = vec![0u8; l];
            let k = bytes.len().min(l);
            out[l - k..].copy_from_slice(&bytes[bytes.len() - k..]);
            if n.neg {
                negate(&mut out);
            }
            Ok(out)
        }
    }
}

/// DATE/TIME helpers.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // days since 0001-01-01 (Base date: Jan 1, 0001 = 0)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    // days since 0000-03-01, shift to 0001-01-01
    era * 146097 + doe - 306
}

const MONTHS: [&[u8]; 12] =
    [b"January", b"February", b"March", b"April", b"May", b"June", b"July", b"August", b"September", b"October", b"November", b"December"];
const DAYS: [&[u8]; 7] = [b"Monday", b"Tuesday", b"Wednesday", b"Thursday", b"Friday", b"Saturday", b"Sunday"];

fn two(v: u32) -> Val {
    alloc::format!("{:02}", v).into_bytes()
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// FORMAT(number, before, after, expp, expt).
fn format_fn(n: &Num, before: Option<i64>, after: Option<i64>, expp: Option<i64>, expt: Option<i64>, nm: &Numeric) -> R<Val> {
    let mut x = n.operand(nm.digits);
    x.round(nm.digits);
    // decide on exponential notation
    let digits = nm.digits as i64;
    let mut use_exp = false;
    if !x.is_zero() {
        let nd = x.dig.len() as i64;
        let ip = nd + x.exp; // digits before the point
        let thresh = expt.unwrap_or(digits);
        if ip > thresh || (x.exp < 0 && -x.exp > 2 * thresh && ip <= 0) || ip > digits {
            use_exp = true;
        }
        if expt == Some(0) && expp != Some(0) {
            use_exp = true;
        }
        if expp == Some(0) {
            use_exp = false;
        }
    }
    let (mut mant, mut expo) = (x.clone(), 0i64);
    if use_exp {
        let nd = x.dig.len() as i64;
        expo = x.exp + nd - 1;
        if nm.engineering {
            let m = expo.rem_euclid(3);
            expo -= m;
        }
        mant.exp = x.exp - expo;
    }
    // round the mantissa to `after` places
    if let Some(a) = after {
        if !mant.is_zero() && -mant.exp > a {
            let keep = mant.dig.len() as i64 + mant.exp + a;
            if keep <= 0 {
                // rounds to zero or to one unit in the last place
                let first = if keep == 0 { mant.dig[0] } else { 0 };
                if first >= 5 {
                    mant = Num { neg: mant.neg, dig: vec![1], exp: -a };
                } else {
                    mant = Num::zero();
                }
            } else {
                mant.round(keep as usize);
            }
            if use_exp && !mant.is_zero() {
                // rounding may have made the mantissa 10.x: renormalise
                let ip = mant.dig.len() as i64 + mant.exp;
                let lim = if nm.engineering { 3 } else { 1 };
                if ip > lim {
                    mant.exp -= if nm.engineering { 3 } else { 1 };
                    expo += if nm.engineering { 3 } else { 1 };
                }
            }
        }
    }
    // integer and fraction digits
    let (int_part, frac_part): (Vec<u8>, Vec<u8>) = if mant.is_zero() {
        (vec![b'0'], Vec::new())
    } else {
        let d: Vec<u8> = mant.dig.iter().map(|x| x + b'0').collect();
        if mant.exp >= 0 {
            let mut ip = d.clone();
            ip.extend(core::iter::repeat_n(b'0', mant.exp as usize));
            (ip, Vec::new())
        } else {
            let fl = (-mant.exp) as usize;
            if fl >= d.len() {
                let mut fp = vec![b'0'; fl - d.len()];
                fp.extend_from_slice(&d);
                (vec![b'0'], fp)
            } else {
                (d[..d.len() - fl].to_vec(), d[d.len() - fl..].to_vec())
            }
        }
    };
    let mut frac = frac_part;
    if let Some(a) = after {
        let a = a as usize;
        if frac.len() < a {
            frac.extend(core::iter::repeat_n(b'0', a - frac.len()));
        } else {
            frac.truncate(a);
        }
    }
    let neg = mant.neg && !(int_part.iter().all(|&c| c == b'0') && frac.iter().all(|&c| c == b'0'));
    let mut left = Vec::new();
    if neg {
        left.push(b'-');
    }
    left.extend_from_slice(&int_part);
    if let Some(b) = before {
        let b = b as usize;
        if left.len() > b {
            return e40();
        }
        let mut p = vec![b' '; b - left.len()];
        p.extend_from_slice(&left);
        left = p;
    }
    let mut out = left;
    if !frac.is_empty() {
        out.push(b'.');
        out.extend_from_slice(&frac);
    }
    if use_exp {
        let es = alloc::format!("{}", expo.unsigned_abs()).into_bytes();
        if expo != 0 || expp.is_some() {
            if let Some(p) = expp {
                let p = p as usize;
                if es.len() > p {
                    return e40();
                }
                if expo == 0 {
                    out.extend(core::iter::repeat_n(b' ', p + 2));
                    return Ok(out);
                }
                out.push(b'E');
                out.push(if expo < 0 { b'-' } else { b'+' });
                out.extend(core::iter::repeat_n(b'0', p - es.len()));
                out.extend_from_slice(&es);
            } else {
                out.push(b'E');
                out.push(if expo < 0 { b'-' } else { b'+' });
                out.extend_from_slice(&es);
            }
        }
    } else if let Some(p) = expp {
        if p > 0 && expt.is_some() {
            out.extend(core::iter::repeat_n(b' ', p as usize + 2));
        }
    }
    Ok(out)
}

fn datatype(s: &[u8], t: u8, digits: usize) -> bool {
    match t {
        b'A' => !s.is_empty() && s.iter().all(|c| c.is_ascii_alphanumeric()),
        b'B' => bin_digits(s).is_some(),
        b'L' => !s.is_empty() && s.iter().all(|c| c.is_ascii_lowercase()),
        b'M' => !s.is_empty() && s.iter().all(|c| c.is_ascii_alphabetic()),
        b'N' => Num::parse(s).is_some(),
        b'S' => !s.is_empty() && s.iter().all(|&c| crate::lex::is_sym_char(c)),
        b'U' => !s.is_empty() && s.iter().all(|c| c.is_ascii_uppercase()),
        b'W' => Num::parse(s).and_then(|n| n.to_whole(digits)).is_some(),
        b'X' => hex_digits(s).is_some(),
        b'D' | b'C' => false,
        _ => false,
    }
}

/// Call a built-in function; None if `name` is not one.
pub fn call(it: &mut Interp, f: &mut Frame, name: &[u8], args: &[Option<Val>]) -> Option<R<Val>> {
    let a = A { a: args, digits: f.numeric.digits };
    let nm = f.numeric;
    let r: R<Val> = (|| -> R<Val> {
        Ok(match name {
            b"ABBREV" => {
                a.count(2, 3)?;
                let (info, ab) = (a.s(0), a.s(1));
                let l = a.nonneg(2, ab.len() as i64)? as usize;
                bool_v(ab.len() >= l && info.starts_with(ab))
            }
            b"ABS" => {
                a.count(1, 1)?;
                let mut n = a.num(0)?;
                n.neg = false;
                fmt(&n, &nm)
            }
            b"ADDRESS" => {
                a.count(0, 0)?;
                f.addr.0.clone()
            }
            b"ARG" => {
                a.count(0, 2)?;
                let args = &it.st.args;
                match a.opt(0) {
                    None => {
                        if a.has(1) {
                            return e40();
                        }
                        // the number of arguments up to the last one given
                        let n = args.iter().rposition(|x| x.is_some()).map_or(0, |p| p + 1);
                        int_str(n as i64)
                    }
                    Some(_) => {
                        let n = a.pos(0, 1)? as usize;
                        match a.opt(1) {
                            None => args.get(n - 1).cloned().flatten().map(|v| v.fast_string()).unwrap_or_default(),
                            Some(_) => {
                                let o = a.letter(1, b'N', b"EO")?;
                                let exists = args.get(n - 1).is_some_and(|x| x.is_some());
                                bool_v(if o == b'E' { exists } else { !exists })
                            }
                        }
                    }
                }
            }
            b"BEEP" => {
                a.count(2, 2)?;
                let fr = a.whole(0)?.unwrap_or(0);
                let d = a.whole(1)?.unwrap_or(0);
                if !(37..=32767).contains(&fr) || !(0..=60000).contains(&d) {
                    return e40();
                }
                it.host.beep(fr as u32, d as u32);
                Vec::new()
            }
            b"BITAND" | b"BITOR" | b"BITXOR" => {
                a.count(1, 3)?;
                let s1 = a.s(0);
                let s2 = a.s(1);
                let pad = match a.opt(2) {
                    None => None,
                    Some(p) if p.len() == 1 => Some(p[0]),
                    _ => return e40(),
                };
                let n = s1.len().max(s2.len());
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let (x, y) = (s1.get(i).copied(), s2.get(i).copied());
                    let v = match (x, y, pad) {
                        (Some(x), Some(y), _) | (Some(x), None, Some(y)) | (None, Some(y), Some(x)) => match name {
                            b"BITAND" => x & y,
                            b"BITOR" => x | y,
                            _ => x ^ y,
                        },
                        (Some(x), None, None) => x,
                        (None, Some(y), None) => y,
                        _ => 0,
                    };
                    out.push(v);
                }
                out
            }
            b"B2X" => {
                a.count(1, 1)?;
                let mut d = bin_digits(a.s(0)).ok_or(Err::new(40))?;
                while d.len() % 4 != 0 {
                    d.insert(0, 0);
                }
                let h = b"0123456789ABCDEF";
                d.chunks(4).map(|q| h[(q[0] << 3 | q[1] << 2 | q[2] << 1 | q[3]) as usize]).collect()
            }
            b"CENTER" | b"CENTRE" => {
                a.count(2, 3)?;
                let s = a.s(0);
                let l = a.nonneg(1, 0)? as usize;
                let pad = a.pad(2)?;
                if s.len() >= l {
                    let cut = s.len() - l;
                    let left = cut / 2;
                    s[left..left + l].to_vec()
                } else {
                    let total = l - s.len();
                    let left = total / 2;
                    let mut v = vec![pad; left];
                    v.extend_from_slice(s);
                    v.extend(core::iter::repeat_n(pad, total - left));
                    v
                }
            }
            b"CHARIN" => return stream::charin(it, f, args),
            b"CHAROUT" => return stream::charout(it, f, args),
            b"CHARS" => return stream::chars(it, f, args),
            b"LINEIN" => return stream::linein(it, f, args),
            b"LINEOUT" => return stream::lineout(it, f, args),
            b"LINES" => return stream::lines(it, f, args),
            b"STREAM" => return stream::stream_fn(it, f, args),
            b"COMPARE" => {
                a.count(2, 3)?;
                let (s1, s2) = (a.s(0), a.s(1));
                let pad = a.pad(2)?;
                let n = s1.len().max(s2.len());
                let mut r = 0;
                for i in 0..n {
                    if s1.get(i).copied().unwrap_or(pad) != s2.get(i).copied().unwrap_or(pad) {
                        r = i + 1;
                        break;
                    }
                }
                int_str(r as i64)
            }
            b"CONDITION" => {
                a.count(0, 1)?;
                let o = a.letter(0, b'I', b"CDIS")?;
                match &f.cond {
                    None => Vec::new(),
                    Some(c) => match o {
                        b'C' => c.cond.name().to_vec(),
                        b'D' => c.desc.clone(),
                        b'I' => {
                            if c.call {
                                b"CALL".to_vec()
                            } else {
                                b"SIGNAL".to_vec()
                            }
                        }
                        _ => {
                            let t = &f.traps[cond_index(c.cond)];
                            match t.state {
                                crate::exec::TrapState::On => b"ON".to_vec(),
                                crate::exec::TrapState::Off => b"OFF".to_vec(),
                                crate::exec::TrapState::Delay => b"DELAY".to_vec(),
                            }
                        }
                    },
                }
            }
            b"COPIES" => {
                a.count(2, 2)?;
                let n = a.nonneg(1, 0)? as usize;
                a.s(0).repeat(n)
            }
            b"C2D" => {
                a.count(1, 2)?;
                let s = a.s(0);
                match a.whole(1)? {
                    None => {
                        let d = bytes_to_dec(s);
                        if d.len() > nm.digits {
                            return e40();
                        }
                        dec_str(&d, false)
                    }
                    Some(n) => {
                        if n < 0 {
                            return e40();
                        }
                        let n = n as usize;
                        if n == 0 {
                            return Ok(b"0".to_vec());
                        }
                        let mut b: Vec<u8> = if s.len() >= n {
                            s[s.len() - n..].to_vec()
                        } else {
                            let mut v = vec![0u8; n - s.len()];
                            v.extend_from_slice(s);
                            v
                        };
                        let neg = b[0] & 0x80 != 0;
                        if neg {
                            negate(&mut b);
                        }
                        let d = bytes_to_dec(&b);
                        if d.len() > nm.digits {
                            return e40();
                        }
                        dec_str(&d, neg)
                    }
                }
            }
            b"C2X" => {
                a.count(1, 1)?;
                c2x(a.s(0))
            }
            b"DATATYPE" => {
                a.count(1, 2)?;
                let s = a.s(0);
                match a.opt(1) {
                    None => {
                        if Num::parse(s).is_some() {
                            b"NUM".to_vec()
                        } else {
                            b"CHAR".to_vec()
                        }
                    }
                    Some(_) => {
                        let t = a.letter(1, b'N', b"ABCDLMNSUWX")?;
                        bool_v(datatype(s, t, nm.digits))
                    }
                }
            }
            b"DATE" => {
                a.count(0, 1)?;
                let o = a.letter(0, b'N', b"BDEMNOSUW")?;
                let now = it.host.now();
                let (y, m, d) = (now.year, now.month, now.day);
                let base = days_from_civil(y as i64, m as i64, d as i64);
                match o {
                    b'B' => int_str(base),
                    b'D' => int_str(base - days_from_civil(y as i64, 1, 1) + 1),
                    b'E' => [two(d), b"/".to_vec(), two(m), b"/".to_vec(), two(y % 100)].concat(),
                    b'M' => MONTHS[(m - 1) as usize].to_vec(),
                    b'N' => {
                        let mut v = alloc::format!("{d} ").into_bytes();
                        v.extend_from_slice(&MONTHS[(m - 1) as usize][..3]);
                        v.extend_from_slice(alloc::format!(" {y}").as_bytes());
                        v
                    }
                    b'O' => [two(y % 100), b"/".to_vec(), two(m), b"/".to_vec(), two(d)].concat(),
                    b'S' => alloc::format!("{y:04}{m:02}{d:02}").into_bytes(),
                    b'U' => [two(m), b"/".to_vec(), two(d), b"/".to_vec(), two(y % 100)].concat(),
                    _ => DAYS[(base.rem_euclid(7)) as usize].to_vec(),
                }
            }
            b"DELSTR" => {
                a.count(2, 3)?;
                let s = a.s(0);
                let n = a.pos(1, 1)? as usize;
                let l = if a.has(2) { Some(a.nonneg(2, 0)? as usize) } else { None };
                if n > s.len() {
                    s.to_vec()
                } else {
                    let end = match l {
                        None => s.len(),
                        Some(l) => (n - 1 + l).min(s.len()),
                    };
                    let mut v = s[..n - 1].to_vec();
                    v.extend_from_slice(&s[end..]);
                    v
                }
            }
            b"DELWORD" => {
                a.count(2, 3)?;
                let s = a.s(0);
                let n = a.pos(1, 1)? as usize;
                let l = if a.has(2) { Some(a.nonneg(2, 0)? as usize) } else { None };
                let idx = word_starts(s);
                if n > idx.len() {
                    s.to_vec()
                } else {
                    let start = idx[n - 1].0;
                    let end = match l {
                        None => s.len(),
                        Some(0) => start,
                        Some(l) => {
                            if n - 1 + l >= idx.len() {
                                s.len()
                            } else {
                                idx[n - 1 + l].0
                            }
                        }
                    };
                    let mut v = s[..start].to_vec();
                    v.extend_from_slice(&s[end..]);
                    v
                }
            }
            b"DIGITS" => {
                a.count(0, 0)?;
                int_str(nm.digits as i64)
            }
            b"FORM" => {
                a.count(0, 0)?;
                if nm.engineering {
                    b"ENGINEERING".to_vec()
                } else {
                    b"SCIENTIFIC".to_vec()
                }
            }
            b"FUZZ" => {
                a.count(0, 0)?;
                int_str(nm.fuzz as i64)
            }
            b"DIRECTORY" => {
                a.count(0, 1)?;
                match it.host.directory(a.opt(0)) {
                    Some(d) => d,
                    None => Vec::new(),
                }
            }
            b"D2C" | b"D2X" => {
                a.count(1, 2)?;
                let n = a.num(0)?;
                if n.to_whole(nm.digits).is_none() {
                    let mut t = n.clone();
                    t.round(nm.digits);
                    // must be a whole number
                    if t.exp < 0 && t.dig[(t.dig.len() as i64 + t.exp).max(0) as usize..].iter().any(|&d| d != 0) {
                        return e40();
                    }
                }
                let mut w = n.clone();
                w.round(nm.digits);
                w.strip_all_zeros();
                if w.exp < 0 {
                    return e40();
                }
                let len = a.whole(1)?;
                if len.is_some_and(|l| l < 0) {
                    return e40();
                }
                if name == b"D2C" {
                    if n.neg && len.is_none() {
                        return e40();
                    }
                    int_to_bytes(&w, len.map(|l| l as usize))?
                } else {
                    if n.neg && len.is_none() {
                        return e40();
                    }
                    match len {
                        None => {
                            let b = int_to_bytes(&w, None)?;
                            let mut x = c2x(&b);
                            // no leading zero digit
                            while x.len() > 1 && x[0] == b'0' {
                                x.remove(0);
                            }
                            x
                        }
                        Some(l) => {
                            let l = l as usize;
                            let b = int_to_bytes(&w, Some(l.div_ceil(2)))?;
                            let x = c2x(&b);
                            x[x.len() - l..].to_vec()
                        }
                    }
                }
            }
            b"ENDLOCAL" | b"SETLOCAL" => {
                a.count(0, 0)?;
                bool_v(if name == b"ENDLOCAL" { it.host.endlocal() } else { it.host.setlocal() })
            }
            b"ERRORTEXT" => {
                a.count(1, 1)?;
                let n = a.nonneg(0, 0)?;
                if n > 99 {
                    return e40();
                }
                if n == 0 { Vec::new() } else { it.host.message(n as u32).unwrap_or_default() }
            }
            b"FILESPEC" => {
                a.count(2, 2)?;
                let o = a.letter(0, b'N', b"DPN")?;
                let s = a.s(1);
                let (drive, rest) = if s.len() >= 2 && s[1] == b':' { (&s[..2], &s[2..]) } else { (&s[..0], s) };
                let cut = rest.iter().rposition(|&c| c == b'\\' || c == b'/').map_or(0, |p| p + 1);
                match o {
                    b'D' => drive.to_vec(),
                    b'P' => rest[..cut].to_vec(),
                    _ => rest[cut..].to_vec(),
                }
            }
            b"FORMAT" => {
                a.count(1, 5)?;
                let n = a.num(0)?;
                let b = if a.has(1) { Some(a.nonneg(1, 0)?) } else { None };
                let af = if a.has(2) { Some(a.nonneg(2, 0)?) } else { None };
                let ep = if a.has(3) { Some(a.nonneg(3, 0)?) } else { None };
                let et = if a.has(4) { Some(a.nonneg(4, 0)?) } else { None };
                if b.is_none() && af.is_none() && ep.is_none() && et.is_none() {
                    fmt(&n, &nm)
                } else {
                    format_fn(&n, b, af, ep, et, &nm)?
                }
            }
            b"INSERT" | b"OVERLAY" => {
                a.count(2, 5)?;
                let (new, target) = (a.s(0), a.s(1));
                let n = a.nonneg(2, if name == b"INSERT" { 0 } else { 1 })? as usize;
                if name == b"OVERLAY" && n == 0 {
                    return e40();
                }
                let len = if a.has(3) { a.nonneg(3, 0)? as usize } else { new.len() };
                let pad = a.pad(4)?;
                let mut newp = new.to_vec();
                newp.resize(len, pad);
                if name == b"INSERT" {
                    let mut v = target.to_vec();
                    if v.len() < n {
                        v.resize(n, pad);
                    }
                    let mut out = v[..n].to_vec();
                    out.extend_from_slice(&newp);
                    out.extend_from_slice(&v[n..]);
                    out
                } else {
                    let start = n - 1;
                    let mut v = target.to_vec();
                    if v.len() < start {
                        v.resize(start, pad);
                    }
                    let mut out = v[..start].to_vec();
                    out.extend_from_slice(&newp);
                    if v.len() > start + len {
                        out.extend_from_slice(&v[start + len..]);
                    }
                    out
                }
            }
            b"LASTPOS" => {
                a.count(2, 3)?;
                let (needle, hay) = (a.s(0), a.s(1));
                let start = if a.has(2) { a.pos(2, 1)? as usize } else { hay.len() };
                if needle.is_empty() || needle.len() > hay.len() {
                    return Ok(b"0".to_vec());
                }
                let last = (start.min(hay.len())).saturating_sub(needle.len());
                let mut r = 0;
                if start >= needle.len() {
                    for i in (0..=last).rev() {
                        if &hay[i..i + needle.len()] == needle {
                            r = i + 1;
                            break;
                        }
                    }
                }
                int_str(r as i64)
            }
            b"LEFT" | b"RIGHT" => {
                a.count(2, 3)?;
                let s = a.s(0);
                let l = a.nonneg(1, 0)? as usize;
                let pad = a.pad(2)?;
                if name == b"LEFT" {
                    let mut v: Vec<u8> = s.iter().copied().take(l).collect();
                    v.resize(l, pad);
                    v
                } else if s.len() >= l {
                    s[s.len() - l..].to_vec()
                } else {
                    let mut v = vec![pad; l - s.len()];
                    v.extend_from_slice(s);
                    v
                }
            }
            b"LENGTH" => {
                a.count(1, 1)?;
                int_str(a.s(0).len() as i64)
            }
            b"MAX" | b"MIN" => {
                if args.is_empty() {
                    return e40();
                }
                let mut best: Option<Num> = None;
                for i in 0..args.len() {
                    let n = Num::parse(args[i].as_deref().ok_or(Err::new(40))?).ok_or(Err::new(40))?;
                    best = Some(match best {
                        None => n,
                        Some(b) => {
                            let o = Num::compare(&n, &b, &nm);
                            let better = if name == b"MAX" { o == core::cmp::Ordering::Greater } else { o == core::cmp::Ordering::Less };
                            if better { n } else { b }
                        }
                    });
                }
                fmt(&best.unwrap(), &nm)
            }
            b"POS" => {
                a.count(2, 3)?;
                let (needle, hay) = (a.s(0), a.s(1));
                let start = a.pos(2, 1)? as usize;
                if needle.is_empty() || start > hay.len() {
                    b"0".to_vec()
                } else {
                    int_str(crate::exec::find(&hay[start - 1..], needle).map_or(0, |p| p + start) as i64)
                }
            }
            b"QUEUED" => {
                a.count(0, 0)?;
                int_str(it.queued_count() as i64)
            }
            b"RANDOM" => {
                a.count(0, 3)?;
                let (mut min, mut max) = (0i64, 999i64);
                if a.has(0) && !a.has(1) && args.len() == 1 {
                    max = a.nonneg(0, 0)?;
                } else {
                    if a.has(0) {
                        min = a.nonneg(0, 0)?;
                    }
                    if a.has(1) {
                        max = a.nonneg(1, 0)?;
                    }
                }
                if max < min || max - min > 100000 {
                    return e40();
                }
                if a.has(2) {
                    let seed = a.nonneg(2, 0)?;
                    it.st.rand = seed as u64 ^ 0x5DEECE66D;
                    it.st.rand_seeded = true;
                }
                if !it.st.rand_seeded {
                    let n = it.host.now();
                    it.st.rand = (n.micros as u64) ^ ((n.sec as u64) << 20) ^ ((n.min as u64) << 26) ^ ((n.hour as u64) << 32) ^ 0x5DEECE66D;
                    it.st.rand_seeded = true;
                }
                it.st.rand = it.st.rand.wrapping_mul(0x5DEECE66D).wrapping_add(11) & ((1u64 << 48) - 1);
                let r = (it.st.rand >> 16) as i64;
                int_str(min + r % (max - min + 1))
            }
            b"REVERSE" => {
                a.count(1, 1)?;
                let mut v = a.s(0).to_vec();
                v.reverse();
                v
            }
            b"SIGN" => {
                a.count(1, 1)?;
                let mut n = a.num(0)?;
                n.round(nm.digits);
                if n.is_zero() {
                    b"0".to_vec()
                } else if n.neg {
                    b"-1".to_vec()
                } else {
                    b"1".to_vec()
                }
            }
            b"SOURCELINE" => {
                a.count(0, 1)?;
                let lines = &f.prog.lines;
                // a final empty line (after the last line end) does not count
                let total = if lines.last().is_some_and(|l| l.is_empty()) { lines.len() - 1 } else { lines.len() };
                match a.opt(0) {
                    None => int_str(total as i64),
                    Some(_) => {
                        let n = a.pos(0, 1)? as usize;
                        if n > total {
                            return e40();
                        }
                        lines[n - 1].clone()
                    }
                }
            }
            b"SPACE" => {
                a.count(1, 3)?;
                let n = a.nonneg(1, 1)? as usize;
                let pad = a.pad(2)?;
                let w = words(a.s(0));
                let mut v = Vec::new();
                for (i, x) in w.iter().enumerate() {
                    if i > 0 {
                        v.extend(core::iter::repeat_n(pad, n));
                    }
                    v.extend_from_slice(x);
                }
                v
            }
            b"STRIP" => {
                a.count(1, 3)?;
                let s = a.s(0);
                let o = a.letter(1, b'B', b"BLT")?;
                let ch = a.pad(2)?;
                let mut st = 0;
                let mut en = s.len();
                if o != b'T' {
                    while st < en && s[st] == ch {
                        st += 1;
                    }
                }
                if o != b'L' {
                    while en > st && s[en - 1] == ch {
                        en -= 1;
                    }
                }
                s[st..en].to_vec()
            }
            b"SUBSTR" => {
                a.count(2, 4)?;
                let s = a.s(0);
                let n = a.pos(1, 1)? as usize;
                let pad = a.pad(3)?;
                let len = if a.has(2) { a.nonneg(2, 0)? as usize } else { s.len().saturating_sub(n - 1) };
                let mut v = Vec::with_capacity(len);
                for i in 0..len {
                    v.push(s.get(n - 1 + i).copied().unwrap_or(pad));
                }
                v
            }
            b"SUBWORD" => {
                a.count(2, 3)?;
                let s = a.s(0);
                let n = a.pos(1, 1)? as usize;
                let l = if a.has(2) { Some(a.nonneg(2, 0)? as usize) } else { None };
                let idx = word_starts(s);
                if n > idx.len() || l == Some(0) {
                    Vec::new()
                } else {
                    let last = match l {
                        None => idx.len() - 1,
                        Some(l) => (n - 1 + l - 1).min(idx.len() - 1),
                    };
                    s[idx[n - 1].0..idx[last].1].to_vec()
                }
            }
            b"SYMBOL" => {
                a.count(1, 1)?;
                let s = a.s(0);
                if s.is_empty() || !s.iter().all(|&c| crate::lex::is_sym_char(c)) {
                    b"BAD".to_vec()
                } else if s[0].is_ascii_digit() || s[0] == b'.' {
                    b"LIT".to_vec()
                } else {
                    match it.st.var_get(s, true) {
                        Ok(Some(_)) => b"VAR".to_vec(),
                        _ => b"LIT".to_vec(),
                    }
                }
            }
            b"TIME" => {
                a.count(0, 1)?;
                let o = a.letter(0, b'N', b"CEHLMNRS")?;
                let now = it.host.now();
                let us_of_day = ((now.hour as u64 * 60 + now.min as u64) * 60 + now.sec as u64) * 1_000_000 + now.micros as u64;
                match o {
                    b'N' => alloc::format!("{:02}:{:02}:{:02}", now.hour, now.min, now.sec).into_bytes(),
                    b'C' => {
                        let h = now.hour % 12;
                        let h = if h == 0 { 12 } else { h };
                        alloc::format!("{}:{:02}{}", h, now.min, if now.hour < 12 { "am" } else { "pm" }).into_bytes()
                    }
                    b'H' => int_str(now.hour as i64),
                    b'M' => int_str((now.hour * 60 + now.min) as i64),
                    b'S' => int_str(((now.hour * 60 + now.min) * 60 + now.sec) as i64),
                    b'L' => alloc::format!("{:02}:{:02}:{:02}.{:06}", now.hour, now.min, now.sec, now.micros).into_bytes(),
                    _ => {
                        // E and R: the elapsed-time clock
                        let r = match f.elapsed {
                            None => b"0".to_vec(),
                            Some(start) => {
                                let mut d = us_of_day as i64 - start as i64;
                                if d < 0 {
                                    d += 86_400_000_000;
                                }
                                let n = Num { neg: false, dig: d.to_string().bytes().map(|c| c - b'0').collect(), exp: -6 };
                                let mut n = n;
                                n.round(nm.digits);
                                n.strip_fraction();
                                n.format(&nm)
                            }
                        };
                        if o == b'R' || f.elapsed.is_none() {
                            f.elapsed = Some(us_of_day);
                        }
                        r
                    }
                }
            }
            b"TRACE" => {
                a.count(0, 1)?;
                let old = Interp::trace_str(f);
                if let Some(s) = a.opt(0) {
                    let s = s.to_vec();
                    it.set_trace(f, &s).map_err(|_| Err::new(40))?;
                }
                old
            }
            b"TRANSLATE" => {
                a.count(1, 4)?;
                let s = a.s(0);
                if !a.has(1) && !a.has(2) && !a.has(3) {
                    return Ok(s.to_ascii_uppercase());
                }
                let out = a.s(1);
                let pad = a.pad(3)?;
                let tin: Vec<u8> = match a.opt(2) {
                    Some(t) => t.to_vec(),
                    None => (0..=255u8).collect(),
                };
                s.iter()
                    .map(|&c| match tin.iter().position(|&x| x == c) {
                        Some(p) => out.get(p).copied().unwrap_or(pad),
                        None => c,
                    })
                    .collect()
            }
            b"TRUNC" => {
                a.count(1, 2)?;
                let mut n = a.num(0)?;
                n.round(nm.digits);
                let d = a.nonneg(1, 0)?;
                // truncate (not round) to d decimals
                if n.exp < -d {
                    let cut = (-d - n.exp) as usize;
                    if cut >= n.dig.len() {
                        n = Num::zero();
                    } else {
                        n.dig.truncate(n.dig.len() - cut);
                        n.exp = -d;
                    }
                }
                let n2 = Numeric { digits: 1000, ..nm };
                let s = if n.is_zero() { b"0".to_vec() } else { n.format(&n2) };
                // pad decimals
                let mut s = s;
                let neg_zero = s.as_slice() == b"-0";
                if neg_zero {
                    s = b"0".to_vec();
                }
                if d > 0 {
                    let dot = s.iter().position(|&c| c == b'.');
                    let have = match dot {
                        Some(p) => s.len() - p - 1,
                        None => {
                            s.push(b'.');
                            0
                        }
                    };
                    s.extend(core::iter::repeat_n(b'0', d as usize - have));
                }
                s
            }
            b"VALUE" => {
                a.count(1, 3)?;
                let vname = a.s(0);
                match a.opt(2) {
                    Some(sel) => {
                        let su = sel.to_ascii_uppercase();
                        if su.as_slice() != b"OS2ENVIRONMENT" && su.as_slice() != b"ENVIRONMENT" {
                            return e40();
                        }
                        let old = it.host.env_get(vname).unwrap_or_default();
                        if a.has(1) {
                            it.host.env_set(vname, a.opt(1));
                        }
                        old
                    }
                    None => {
                        let u = vname.to_ascii_uppercase();
                        let cur = match it.st.var_get(&u, true) {
                            Ok(v) => v.unwrap_or_else(|| u.clone()),
                            Err(_) => return e40(),
                        };
                        if let Some(nv) = a.opt(1) {
                            it.st.var_set(&u, nv.to_vec(), true).map_err(|_| Err::new(40))?;
                        }
                        cur
                    }
                }
            }
            b"VERIFY" => {
                a.count(2, 4)?;
                let (s, r) = (a.s(0), a.s(1));
                let o = a.letter(2, b'N', b"MN")?;
                let start = a.pos(3, 1)? as usize;
                let mut res = 0;
                if start <= s.len() {
                    for (i, &c) in s.iter().enumerate().skip(start - 1) {
                        let inr = r.contains(&c);
                        if (o == b'N' && !inr) || (o == b'M' && inr) {
                            res = i + 1;
                            break;
                        }
                    }
                }
                int_str(res as i64)
            }
            b"WORD" => {
                a.count(2, 2)?;
                let n = a.pos(1, 1)? as usize;
                words(a.s(0)).get(n - 1).map(|w| w.to_vec()).unwrap_or_default()
            }
            b"WORDINDEX" => {
                a.count(2, 2)?;
                let n = a.pos(1, 1)? as usize;
                let idx = word_starts(a.s(0));
                int_str(idx.get(n - 1).map_or(0, |w| w.0 + 1) as i64)
            }
            b"WORDLENGTH" => {
                a.count(2, 2)?;
                let n = a.pos(1, 1)? as usize;
                int_str(words(a.s(0)).get(n - 1).map_or(0, |w| w.len()) as i64)
            }
            b"WORDPOS" => {
                a.count(2, 3)?;
                let phrase = words(a.s(0));
                let w = words(a.s(1));
                let start = a.pos(2, 1)? as usize;
                let mut r = 0;
                if !phrase.is_empty() && start <= w.len() {
                    for i in (start - 1)..w.len() {
                        if i + phrase.len() <= w.len() && w[i..i + phrase.len()] == phrase[..] {
                            r = i + 1;
                            break;
                        }
                    }
                }
                int_str(r as i64)
            }
            b"WORDS" => {
                a.count(1, 1)?;
                int_str(words(a.s(0)).len() as i64)
            }
            b"XRANGE" => {
                a.count(0, 2)?;
                let s = match a.opt(0) {
                    None => 0u8,
                    Some(x) if x.len() == 1 => x[0],
                    _ => return e40(),
                };
                let e = match a.opt(1) {
                    None => 255u8,
                    Some(x) if x.len() == 1 => x[0],
                    _ => return e40(),
                };
                let mut v = Vec::new();
                let mut c = s;
                loop {
                    v.push(c);
                    if c == e {
                        break;
                    }
                    c = c.wrapping_add(1);
                }
                v
            }
            b"X2B" => {
                a.count(1, 1)?;
                let d = hex_digits(a.s(0)).ok_or(Err::new(40))?;
                let mut v = Vec::new();
                for x in d {
                    for k in (0..4).rev() {
                        v.push(if x >> k & 1 == 1 { b'1' } else { b'0' });
                    }
                }
                v
            }
            b"X2C" => {
                a.count(1, 1)?;
                x2c(a.s(0))?
            }
            b"X2D" => {
                a.count(1, 2)?;
                let d = hex_digits(a.s(0)).ok_or(Err::new(40))?;
                match a.whole(1)? {
                    None => {
                        let mut digits = d.clone();
                        if digits.len() % 2 == 1 {
                            digits.insert(0, 0);
                        }
                        let b: Vec<u8> = digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect();
                        let dec = bytes_to_dec(&b);
                        if dec.len() > nm.digits {
                            return e40();
                        }
                        dec_str(&dec, false)
                    }
                    Some(n) => {
                        if n < 0 {
                            return e40();
                        }
                        let n = n as usize;
                        if n == 0 {
                            return Ok(b"0".to_vec());
                        }
                        let mut digits: Vec<u8> = if d.len() >= n {
                            d[d.len() - n..].to_vec()
                        } else {
                            let mut v = vec![0u8; n - d.len()];
                            v.extend_from_slice(&d);
                            v
                        };
                        let neg = digits[0] & 8 != 0;
                        // pad to whole bytes with the sign
                        if digits.len() % 2 == 1 {
                            digits.insert(0, if neg { 15 } else { 0 });
                        }
                        let mut b: Vec<u8> = digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect();
                        if neg {
                            negate(&mut b);
                        }
                        let dec = bytes_to_dec(&b);
                        if dec.len() > nm.digits {
                            return e40();
                        }
                        dec_str(&dec, neg)
                    }
                }
            }
            b"RXFUNCADD" => {
                a.count(3, 3)?;
                bool_v(it.host.rxfunc(b'A', a.s(0), a.s(1), a.s(2)) != 0)
            }
            b"RXFUNCDROP" => {
                a.count(1, 1)?;
                bool_v(it.host.rxfunc(b'D', a.s(0), b"", b"") != 0)
            }
            b"RXFUNCQUERY" => {
                a.count(1, 1)?;
                int_str(it.host.rxfunc(b'Q', a.s(0), b"", b"") as i64)
            }
            b"RXQUEUE" => {
                a.count(1, 2)?;
                let op = a.s(0).to_ascii_uppercase();
                if !matches!(op.as_slice(), b"CREATE" | b"DELETE" | b"GET" | b"SET") {
                    return e40();
                }
                if (op.as_slice() == b"GET" && a.has(1)) || (matches!(op.as_slice(), b"SET" | b"DELETE") && !a.has(1)) {
                    return e40();
                }
                let mut st = core::mem::replace(&mut it.st, State::empty());
                let r = it.host.rxqueue(&mut st, &op, a.opt(1));
                it.st = st;
                r.map_err(Err::new)?
            }
            b"DBADJUST" | b"DBBRACKET" | b"DBCENTER" | b"DBLEFT" | b"DBRIGHT" | b"DBRLEFT" | b"DBRRIGHT" | b"DBTODBCS" | b"DBTOSBCS"
            | b"DBUNBRACKET" | b"DBVALIDATE" | b"DBWIDTH" => return dbcs(it, f, name, args),
            _ => return Err(Err::Syntax(0xFFFF_FFFF)),
        })
    })();
    match r {
        Err(Err::Syntax(0xFFFF_FFFF)) => None,
        r => Some(r),
    }
}

/// The DBCS functions on a single-byte system: strings have no
/// double-byte characters.
fn dbcs(_it: &mut Interp, f: &mut Frame, name: &[u8], args: &[Option<Val>]) -> R<Val> {
    let a = A { a: args, digits: f.numeric.digits };
    Ok(match name {
        b"DBADJUST" => {
            a.count(1, 2)?;
            a.s(0).to_vec()
        }
        b"DBBRACKET" | b"DBUNBRACKET" | b"DBTODBCS" | b"DBTOSBCS" => {
            a.count(1, 1)?;
            a.s(0).to_vec()
        }
        b"DBCENTER" | b"DBLEFT" | b"DBRIGHT" => {
            a.count(2, 4)?;
            let l = a.nonneg(1, 0)? as usize;
            let pad = a.pad(2)?;
            let s = a.s(0);
            match name {
                b"DBLEFT" => {
                    let mut v: Vec<u8> = s.iter().copied().take(l).collect();
                    v.resize(l, pad);
                    v
                }
                b"DBRIGHT" => {
                    if s.len() >= l {
                        s[s.len() - l..].to_vec()
                    } else {
                        let mut v = vec![pad; l - s.len()];
                        v.extend_from_slice(s);
                        v
                    }
                }
                _ => {
                    if s.len() >= l {
                        let left = (s.len() - l) / 2;
                        s[left..left + l].to_vec()
                    } else {
                        let total = l - s.len();
                        let mut v = vec![pad; total / 2];
                        v.extend_from_slice(s);
                        v.extend(core::iter::repeat_n(pad, total - total / 2));
                        v
                    }
                }
            }
        }
        b"DBRLEFT" | b"DBRRIGHT" => {
            a.count(2, 3)?;
            let l = a.nonneg(1, 0)? as usize;
            let s = a.s(0);
            if name == b"DBRLEFT" {
                if l >= s.len() { Vec::new() } else { s[l..].to_vec() }
            } else if l >= s.len() {
                Vec::new()
            } else {
                s[..s.len() - l].to_vec()
            }
        }
        b"DBVALIDATE" => {
            a.count(1, 2)?;
            b"1".to_vec()
        }
        _ => {
            a.count(1, 2)?;
            int_str(a.s(0).len() as i64)
        }
    })
}

fn bool_v(b: bool) -> Val {
    if b { b"1".to_vec() } else { b"0".to_vec() }
}

/// (start, end) of each blank-delimited word.
fn word_starts(s: &[u8]) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < s.len() {
        while i < s.len() && s[i] == b' ' {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        let st = i;
        while i < s.len() && s[i] != b' ' {
            i += 1;
        }
        v.push((st, i));
    }
    v
}

#[allow(dead_code)]
fn unused(nm: &Numeric) -> core::cmp::Ordering {
    compare(b"", b"", nm)
}

#[allow(dead_code)]
fn unused2(x: &[u8]) -> &[u8] {
    strip_blanks(x)
}

#[allow(dead_code)]
fn unused3(_c: Cond, y: u32) -> bool {
    is_leap(y)
}
