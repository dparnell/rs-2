//! The samples the notebook's pages show (numbers and amounts in each
//! of the formats the drop-down lists offer) and the format strings the
//! date and time pages make, all in the process code page.

use alloc::vec::Vec;

/// The sample number: one thousand and ones in each decimal place, e.g.
/// `1,000.11` for two digits.
pub fn number(dec: &[u8], th: &[u8], digits: i32) -> Vec<u8> {
    let mut v = b"1".to_vec();
    v.extend_from_slice(th);
    v.extend_from_slice(b"000");
    if digits > 0 {
        v.extend_from_slice(dec);
        for _ in 0..digits.min(9) {
            v.push(b'1');
        }
    }
    v
}

/// Negative numbers (iNegNumber 0-4): `(1.1)`, `-1.1`, `- 1.1`, `1.1-`,
/// `1.1 -`.
pub const NEG_NUMBER_FORMATS: usize = 5;

pub fn neg_number(fmt: usize, sign: &[u8], n: &[u8]) -> Vec<u8> {
    let parts: [&[u8]; 3] = match fmt {
        0 => [b"(", n, b")"],
        1 => [sign, n, b""],
        2 => [sign, b" ", n],
        3 => [n, sign, b""],
        _ => [n, b" ", sign],
    };
    parts.concat()
}

/// Positive amounts (iCurrency 0-3): `$1`, `1$`, `$ 1`, `1 $`.
pub const CURRENCY_FORMATS: usize = 4;

pub fn currency(fmt: usize, sym: &[u8], n: &[u8]) -> Vec<u8> {
    let parts: [&[u8]; 3] = match fmt {
        0 => [sym, n, b""],
        1 => [n, sym, b""],
        2 => [sym, b" ", n],
        _ => [n, b" ", sym],
    };
    parts.concat()
}

/// Negative amounts (iNegCurr 0-15, the sixteen placements of the
/// symbol and the sign).
pub const NEG_CURRENCY_FORMATS: usize = 16;

pub fn neg_currency(fmt: usize, sym: &[u8], sign: &[u8], n: &[u8]) -> Vec<u8> {
    let parts: [&[u8]; 5] = match fmt {
        0 => [b"(", sym, n, b")", b""],
        1 => [sign, sym, n, b"", b""],
        2 => [sym, sign, n, b"", b""],
        3 => [sym, n, sign, b"", b""],
        4 => [b"(", n, sym, b")", b""],
        5 => [sign, n, sym, b"", b""],
        6 => [n, sign, sym, b"", b""],
        7 => [n, sym, sign, b"", b""],
        8 => [sign, n, b" ", sym, b""],
        9 => [sign, sym, b" ", n, b""],
        10 => [n, b" ", sym, sign, b""],
        11 => [sym, b" ", n, sign, b""],
        12 => [sym, b" ", sign, n, b""],
        13 => [n, sign, b" ", sym, b""],
        14 => [b"(", sym, b" ", n, b")"],
        _ => [b"(", n, b" ", sym, b")"],
    };
    parts.concat()
}

/// The short date format of a date order (iDate: 0 month-day-year, 1
/// day-month-year, 2 year-month-day, 3 year-day-month) and separator.
pub fn date_format(order: i32, sep: &[u8]) -> Vec<u8> {
    let f: [&[u8]; 3] = match order {
        1 => [b"%d", b"%m", b"%y"],
        2 => [b"%y", b"%m", b"%d"],
        3 => [b"%y", b"%d", b"%m"],
        _ => [b"%m", b"%d", b"%y"],
    };
    [f[0], sep, f[1], sep, f[2]].concat()
}

/// The time format of a clock (12 or 24 hours) and separator.
pub fn time_format(h24: bool, sep: &[u8]) -> Vec<u8> {
    if h24 {
        [&b"%H"[..], sep, b"%M", sep, b"%S"].concat()
    } else {
        [&b"%I"[..], sep, b"%M", sep, b"%S %p"].concat()
    }
}

/// The date order a short date format follows (by where %d, %m and %y
/// come), if it is one of the four.
pub fn date_order(f: &[u8]) -> Option<i32> {
    let pos = |c: u8| f.windows(2).position(|w| w[0] == b'%' && w[1].to_ascii_lowercase() == c);
    let (d, m, y) = (pos(b'd')?, pos(b'm')?, pos(b'y')?);
    Some(if m < d && d < y {
        0
    } else if d < m && m < y {
        1
    } else if y < m && m < d {
        2
    } else if y < d && d < m {
        3
    } else {
        return None;
    })
}

/// Replace the separator between the fields of a date or time format
/// (the characters between `%x` conversions that are not blanks).
pub fn replace_separator(f: &[u8], sep: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'%' && i + 1 < f.len() {
            out.extend_from_slice(&f[i..i + 2]);
            i += 2;
            // The separator: what stands between this and the next
            // conversion, unless blanks.
            let start = i;
            while i < f.len() && f[i] != b'%' {
                i += 1;
            }
            let between = &f[start..i];
            if i < f.len() && !between.is_empty() && !between.iter().all(|&c| c == b' ') {
                out.extend_from_slice(sep);
            } else {
                out.extend_from_slice(between);
            }
        } else {
            out.push(f[i]);
            i += 1;
        }
    }
    out
}
