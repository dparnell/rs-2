//! Attribute and transformation objects, UniTransUpper/Lower, UniStrtod
//! and UniQueryAttr. kLIBC builds its ctype tables with UniTransUpper and
//! UniTransLower (found with DosQueryProcAddr) and uses the attribute
//! objects for its wide-character classes.

use alloc::boxed::Box;

use uls::{from_ptr, ULS_BADOBJECT, ULS_INVALID, ULS_NOMATCH, ULS_SUCCESS, ULS_UNSUPPORTED};

/// The attribute names of IBM's unidef.h and their CT_ bits.
const ATTRS: &[(&str, u16)] = &[
    ("alpha", uls::CT_ALPHA),
    ("alnum", uls::CT_ALNUM),
    ("blank", uls::CT_BLANK),
    ("cntrl", uls::CT_CNTRL),
    ("digit", uls::CT_DIGIT),
    ("graph", uls::CT_GRAPH),
    ("lower", uls::CT_LOWER),
    ("print", uls::CT_PRINT),
    ("punct", uls::CT_PUNCT),
    ("space", uls::CT_SPACE),
    ("upper", uls::CT_UPPER),
    ("xdigit", uls::CT_XDIGIT),
    ("number", uls::CT_NUMBER),
    ("symbol", uls::CT_SYMBOL),
    ("ascii", uls::CT_ASCII),
];

/// The CT_ mask of a blank-separated list of attribute names; 0 when one
/// is unknown.
fn attr_mask(name: *const u16) -> u16 {
    if name.is_null() {
        return 0;
    }
    let s = unsafe { from_ptr(name) };
    let mut mask = 0;
    for w in s.split(|&c| c == b' ' as u16 || c == b'\t' as u16).filter(|w| !w.is_empty()) {
        let Some(&(_, m)) = ATTRS.iter().find(|(n, _)| n.len() == w.len() && n.bytes().zip(w.iter()).all(|(a, &b)| a as u16 == uls::to_lower(b))) else {
            return 0;
        };
        mask |= m;
    }
    mask
}

fn has(uc: u16, mask: u16) -> bool {
    uls::char_type(uc).itype & mask != 0
}

/// UniQueryAttr(attrName): the attribute's mask (0: unknown).
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryAttr(name: *const u16) -> u32 {
    attr_mask(name) as u32
}

struct Attr {
    mask: u16,
}

#[unsafe(no_mangle)]
pub extern "C" fn UniCreateAttrObject(_lo: u32, name: *const u16, out: *mut u32) -> i32 {
    let mask = attr_mask(name);
    if mask == 0 || out.is_null() {
        return ULS_UNSUPPORTED;
    }
    unsafe { *out = Box::into_raw(Box::new(Attr { mask })) as u32 };
    ULS_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn UniFreeAttrObject(a: u32) -> i32 {
    if a == 0 {
        return ULS_BADOBJECT;
    }
    drop(unsafe { Box::from_raw(a as *mut Attr) });
    ULS_SUCCESS
}

/// UniQueryCharAttr(attrObject, uc): 1 if the character has the
/// attribute(s), else 0.
#[unsafe(no_mangle)]
pub extern "C" fn UniQueryCharAttr(a: u32, uc: u32) -> i32 {
    if a == 0 {
        return 0;
    }
    let m = unsafe { (*(a as *const Attr)).mask };
    has(uc as u16, m) as i32
}

/// UniScanForAttr(attrObject, ucs, count, inverse, &offset): the first
/// character that has (or, inverse, lacks) the attribute.
#[unsafe(no_mangle)]
pub extern "C" fn UniScanForAttr(a: u32, s: *const u16, n: u32, inverse: u32, off: *mut u32) -> i32 {
    if a == 0 || s.is_null() || off.is_null() {
        return ULS_INVALID;
    }
    let m = unsafe { (*(a as *const Attr)).mask };
    for i in 0..n as usize {
        let c = unsafe { *s.add(i) };
        if has(c, m) != (inverse != 0) {
            unsafe { *off = i as u32 };
            return ULS_SUCCESS;
        }
    }
    ULS_NOMATCH
}

/// Transformation kinds.
#[derive(Clone, Copy)]
enum Xform {
    Identity,
    Upper,
    Lower,
}

fn apply(x: Xform, c: u16) -> u16 {
    match x {
        Xform::Identity => c,
        Xform::Upper => uls::to_upper(c),
        Xform::Lower => uls::to_lower(c),
    }
}

/// UniCreateTransformObject(locale, xtype, &xformObject): "upper",
/// "lower" and the transformations that change nothing in the code pages
/// rs2 has ("compose", "decompose", "hiragana", "katakana", "kana").
#[unsafe(no_mangle)]
pub extern "C" fn UniCreateTransformObject(_lo: u32, name: *const u16, out: *mut u32) -> i32 {
    if name.is_null() || out.is_null() {
        return ULS_INVALID;
    }
    let s = unsafe { from_ptr(name) };
    let is = |n: &str| n.len() == s.len() && n.bytes().zip(s.iter()).all(|(a, &b)| a as u16 == uls::to_lower(b));
    let x = if is("upper") {
        Xform::Upper
    } else if is("lower") {
        Xform::Lower
    } else if ["compose", "decompose", "hiragana", "katakana", "kana"].iter().any(|n| is(n)) {
        Xform::Identity
    } else {
        return ULS_UNSUPPORTED;
    };
    unsafe { *out = Box::into_raw(Box::new(x)) as u32 };
    ULS_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn UniFreeTransformObject(x: u32) -> i32 {
    if x == 0 {
        return ULS_BADOBJECT;
    }
    drop(unsafe { Box::from_raw(x as *mut Xform) });
    ULS_SUCCESS
}

/// UniTransformStr(xformObject, in, &inChars, out, &outChars): in and
/// out counts become the characters used and written (-1 in: up to the
/// NUL).
#[unsafe(no_mangle)]
pub extern "C" fn UniTransformStr(x: u32, inp: *const u16, inc: *mut i32, out: *mut u16, outc: *mut i32) -> i32 {
    if x == 0 {
        return ULS_BADOBJECT;
    }
    if inp.is_null() || inc.is_null() || out.is_null() || outc.is_null() {
        return ULS_INVALID;
    }
    let x = unsafe { *(x as *const Xform) };
    let n = unsafe { if *inc < 0 { from_ptr(inp).len() } else { *inc as usize } };
    let room = unsafe { (*outc).max(0) as usize };
    let k = n.min(room);
    for i in 0..k {
        unsafe { out.add(i).write(apply(x, *inp.add(i))) };
    }
    unsafe {
        *inc = k as i32;
        *outc = k as i32;
    }
    if k < n { uls::ULS_BUFFERFULL } else { ULS_SUCCESS }
}

#[unsafe(no_mangle)]
pub extern "C" fn UniTransUpper(_lo: u32, uc: u32) -> u32 {
    uls::to_upper(uc as u16) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniTransLower(_lo: u32, uc: u32) -> u32 {
    uls::to_lower(uc as u16) as u32
}

/// UniStrtod(locale, ucs, &end, &result): a decimal floating point
/// number (leading blanks, sign, digits, '.', exponent) as strtod reads
/// it; end points after it (at ucs when there is none).
#[unsafe(no_mangle)]
pub extern "C" fn UniStrtod(_lo: u32, s: *const u16, end: *mut *const u16, result: *mut f64) -> i32 {
    if s.is_null() || result.is_null() {
        return ULS_INVALID;
    }
    let v = unsafe { from_ptr(s) };
    let mut i = 0;
    while i < v.len() && (v[i] == b' ' as u16 || v[i] == b'\t' as u16) {
        i += 1;
    }

    let mut buf = [0u8; 64];
    let mut n = 0;
    let mut push = |c: u16, n: &mut usize| {
        if *n < buf.len() {
            buf[*n] = c as u8;
            *n += 1;
        }
    };
    let digit = |c: u16| (b'0' as u16..=b'9' as u16).contains(&c);
    if i < v.len() && (v[i] == b'+' as u16 || v[i] == b'-' as u16) {
        push(v[i], &mut n);
        i += 1;
    }
    let mut digits = 0;
    while i < v.len() && digit(v[i]) {
        push(v[i], &mut n);
        i += 1;
        digits += 1;
    }
    if i < v.len() && v[i] == b'.' as u16 {
        push(v[i], &mut n);
        i += 1;
        while i < v.len() && digit(v[i]) {
            push(v[i], &mut n);
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        unsafe {
            *result = 0.0;
            if !end.is_null() {
                *end = s;
            }
        }
        return ULS_SUCCESS;
    }
    if i < v.len() && (v[i] == b'e' as u16 || v[i] == b'E' as u16) {
        let mut j = i + 1;
        let mut e = [0u16; 16];
        let mut en = 0;
        if j < v.len() && (v[j] == b'+' as u16 || v[j] == b'-' as u16) {
            e[en] = v[j];
            en += 1;
            j += 1;
        }
        let ds = j;
        while j < v.len() && digit(v[j]) && en < e.len() {
            e[en] = v[j];
            en += 1;
            j += 1;
        }
        if j > ds {
            push(b'e' as u16, &mut n);
            for &c in &e[..en] {
                push(c, &mut n);
            }
            i = j;
        }
    }
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("0");
    unsafe {
        *result = text.parse::<f64>().unwrap_or(0.0);
        if !end.is_null() {
            *end = s.add(i);
        }
    }

    ULS_SUCCESS
}
