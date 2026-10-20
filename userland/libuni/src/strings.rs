//! The UniStr* functions: C string functions on NUL-terminated UCS-2.

use uls::from_ptr;

unsafe fn len(p: *const u16) -> usize {
    unsafe { from_ptr(p) }.len()
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrlen(s: *const u16) -> u32 {
    if s.is_null() { 0 } else { unsafe { len(s) as u32 } }
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrcpy(d: *mut u16, s: *const u16) -> *mut u16 {
    unsafe { core::ptr::copy(s, d, len(s) + 1) };
    d
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrncpy(d: *mut u16, s: *const u16, n: u32) -> *mut u16 {
    let n = n as usize;
    let k = unsafe { from_ptr(s) }.len().min(n);
    unsafe {
        core::ptr::copy(s, d, k);
        for i in k..n {
            d.add(i).write(0);
        }
    }
    d
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrcat(d: *mut u16, s: *const u16) -> *mut u16 {
    unsafe { UniStrcpy(d.add(len(d)), s) };
    d
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrncat(d: *mut u16, s: *const u16, n: u32) -> *mut u16 {
    unsafe {
        let e = d.add(len(d));
        let k = from_ptr(s).len().min(n as usize);
        core::ptr::copy(s, e, k);
        e.add(k).write(0);
    }
    d
}

fn cmp(a: &[u16], b: &[u16], map: fn(u16) -> u16) -> i32 {
    for i in 0.. {
        let x = map(a.get(i).copied().unwrap_or(0));
        let y = map(b.get(i).copied().unwrap_or(0));
        if x != y || x == 0 {
            return x as i32 - y as i32;
        }
    }
    0
}

fn same(c: u16) -> u16 {
    c
}

fn trunc(s: &[u16], n: u32) -> &[u16] {
    &s[..s.len().min(n as usize)]
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrcmp(a: *const u16, b: *const u16) -> i32 {
    unsafe { cmp(from_ptr(a), from_ptr(b), same) }
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrncmp(a: *const u16, b: *const u16, n: u32) -> i32 {
    unsafe { cmp(trunc(from_ptr(a), n), trunc(from_ptr(b), n), same) }
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrcmpi(_lo: u32, a: *const u16, b: *const u16) -> i32 {
    unsafe { cmp(from_ptr(a), from_ptr(b), uls::to_lower) }
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrncmpi(_lo: u32, a: *const u16, b: *const u16, n: u32) -> i32 {
    unsafe { cmp(trunc(from_ptr(a), n), trunc(from_ptr(b), n), uls::to_lower) }
}

/// UniStrcoll: by the case-folded characters, then exactly.
#[unsafe(no_mangle)]
pub extern "C" fn UniStrcoll(_lo: u32, a: *const u16, b: *const u16) -> i32 {
    let (a, b) = unsafe { (from_ptr(a), from_ptr(b)) };
    match cmp(a, b, uls::to_lower) {
        0 => cmp(a, b, same),
        r => r,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrchr(s: *const u16, c: u32) -> *mut u16 {
    let v = unsafe { from_ptr(s) };
    if c as u16 == 0 {
        return unsafe { s.add(v.len()) as *mut u16 };
    }
    v.iter().position(|&x| x == c as u16).map_or(core::ptr::null_mut(), |i| unsafe { s.add(i) as *mut u16 })
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrrchr(s: *const u16, c: u32) -> *mut u16 {
    let v = unsafe { from_ptr(s) };
    if c as u16 == 0 {
        return unsafe { s.add(v.len()) as *mut u16 };
    }
    v.iter().rposition(|&x| x == c as u16).map_or(core::ptr::null_mut(), |i| unsafe { s.add(i) as *mut u16 })
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrspn(s: *const u16, set: *const u16) -> u32 {
    let (v, set) = unsafe { (from_ptr(s), from_ptr(set)) };
    v.iter().take_while(|c| set.contains(c)).count() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrcspn(s: *const u16, set: *const u16) -> u32 {
    let (v, set) = unsafe { (from_ptr(s), from_ptr(set)) };
    v.iter().take_while(|c| !set.contains(c)).count() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrpbrk(s: *const u16, set: *const u16) -> *mut u16 {
    let (v, set) = unsafe { (from_ptr(s), from_ptr(set)) };
    v.iter().position(|c| set.contains(c)).map_or(core::ptr::null_mut(), |i| unsafe { s.add(i) as *mut u16 })
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrstr(s: *const u16, sub: *const u16) -> *mut u16 {
    let (v, w) = unsafe { (from_ptr(s), from_ptr(sub)) };
    if w.is_empty() {
        return s as *mut u16;
    }
    v.windows(w.len()).position(|x| x == w).map_or(core::ptr::null_mut(), |i| unsafe { s.add(i) as *mut u16 })
}

static mut TOK: *mut u16 = core::ptr::null_mut();

/// UniStrtok: the next token, splitting at characters of `set`.
#[unsafe(no_mangle)]
pub extern "C" fn UniStrtok(s: *mut u16, set: *const u16) -> *mut u16 {
    unsafe {
        let p = if s.is_null() { TOK } else { s };
        if p.is_null() {
            return p;
        }
        let set = from_ptr(set);
        let start = p.add(from_ptr(p).iter().take_while(|c| set.contains(c)).count());
        if *start == 0 {
            TOK = core::ptr::null_mut();
            return core::ptr::null_mut();
        }
        let n = from_ptr(start).iter().take_while(|c| !set.contains(c)).count();
        let end = start.add(n);
        if *end == 0 {
            TOK = core::ptr::null_mut();
        } else {
            end.write(0);
            TOK = end.add(1);
        }
        start
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrupr(s: *mut u16) -> *mut u16 {
    let n = unsafe { len(s) };
    for i in 0..n {
        unsafe { s.add(i).write(uls::to_upper(*s.add(i))) };
    }
    s
}

#[unsafe(no_mangle)]
pub extern "C" fn UniStrlwr(s: *mut u16) -> *mut u16 {
    let n = unsafe { len(s) };
    for i in 0..n {
        unsafe { s.add(i).write(uls::to_lower(*s.add(i))) };
    }
    s
}

/// Parse an integer as strtol/strtoul do: (value, characters used, overflow).
fn parse_int(v: &[u16], mut base: u32, signed: bool) -> (i64, usize, bool) {
    let mut i = v.iter().take_while(|&&c| uls::char_type(c).itype & uls::CT_SPACE != 0).count();
    let mut neg = false;
    if let Some(&c) = v.get(i)
        && (c == b'+' as u16 || c == b'-' as u16)
    {
        neg = c == b'-' as u16;
        i += 1;
    }
    let hex_prefix = v.get(i) == Some(&(b'0' as u16)) && matches!(v.get(i + 1), Some(&c) if c == b'x' as u16 || c == b'X' as u16);
    if (base == 0 || base == 16) && hex_prefix && v.get(i + 2).and_then(|&c| char::from_u32(c as u32)?.to_digit(16)).is_some() {
        i += 2;
        base = 16;
    } else if base == 0 {
        base = if v.get(i) == Some(&(b'0' as u16)) { 8 } else { 10 };
    }
    let start = i;
    let mut n: i64 = 0;
    let mut over = false;
    while let Some(d) = v.get(i).and_then(|&c| char::from_u32(c as u32)?.to_digit(base)) {
        n = n.saturating_mul(base as i64).saturating_add(d as i64);
        if n > u32::MAX as i64 {
            over = true;
        }
        i += 1;
    }
    if i == start {
        return (0, 0, false);
    }
    if neg {
        n = -n;
    }
    if signed && !(i32::MIN as i64..=i32::MAX as i64).contains(&n) {
        over = true;
    }
    (n, i, over)
}

/// UniStrtol(object, string, &end, base, &result): 0 or ULS_RANGE.
#[unsafe(no_mangle)]
pub extern "C" fn UniStrtol(_lo: u32, s: *const u16, end: *mut *const u16, base: u32, out: *mut i32) -> i32 {
    let (n, used, over) = parse_int(unsafe { from_ptr(s) }, base, true);
    unsafe {
        if !end.is_null() {
            end.write_unaligned(s.add(used));
        }
        if !out.is_null() {
            out.write_unaligned(if over { if n < 0 { i32::MIN } else { i32::MAX } } else { n as i32 });
        }
    }
    if over { 0x20413 } else { 0 }
}

/// UniStrtoul(object, string, &end, base, &result).
#[unsafe(no_mangle)]
pub extern "C" fn UniStrtoul(_lo: u32, s: *const u16, end: *mut *const u16, base: u32, out: *mut u32) -> i32 {
    let (n, used, over) = parse_int(unsafe { from_ptr(s) }, base, false);
    unsafe {
        if !end.is_null() {
            end.write_unaligned(s.add(used));
        }
        if !out.is_null() {
            out.write_unaligned(if over { u32::MAX } else { n as u32 });
        }
    }
    if over { 0x20413 } else { 0 }
}
