//! Stem functions (SysStemSort, SysStemCopy, SysStemDelete,
//! SysStemInsert) and SysDumpVariables. The stem functions name variables
//! symbolically (RXSHV_SYSET/SYFET/SYDRO), so the stem may be given in
//! any case; they return "0", or "-1" when the variable pool fails.

use alloc::vec::Vec;
use core::cmp::Ordering;
use core::ptr::null_mut;

use os2::*;

use crate::rx::*;

/// The stem argument with a period added.
fn stem_of(s: &[u8]) -> Vec<u8> {
    let mut v = s.to_vec();
    if v.last() != Some(&b'.') {
        v.push(b'.');
    }
    v
}

fn el(stem: &[u8], i: u32) -> Vec<u8> {
    tail(stem, &dec(i as i64))
}

/// `stem.0` as a number: None if the pool failed (the call returns
/// "-1"); Some(None) if it is not a number (REXX error 40).
fn count(stem: &[u8]) -> (u32, Option<u32>) {
    let (rc, v) = fetch_small(SHV_SYFET, &tail(stem, b"0"));
    (rc, scan_long(&v).map(|n| n as u32))
}

/// Move `stem.from` to `stem2.to`: false if the pool fails.
fn copy_el(from: &[u8], to: &[u8]) -> bool {
    let (rc, v) = fetch_var(SHV_SYFET, from);
    if rc != 0 {
        return false;
    }
    let rc = set_var(SHV_SYSET, to, &v);
    rc == 0 || rc == 1
}

/// The string up to its first zero (the sort compares C strings).
fn cs(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

fn ord(n: i32) -> Ordering {
    n.cmp(&0)
}

/// SysStemSort(stem [, 'A'|'D' [, 'C'|'I' [, first [, last [, firstcol
/// [, lastcol]]]]]]).
pub fn sys_stem_sort(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 1 || a.n() > 7 || !a.valid(0) {
        return INVALID;
    }
    let stem = stem_of(a.cstr(0));
    let mut ascending = true;
    if a.n() > 1 && a.valid(1) {
        match a.first(1) {
            b'A' | b'a' => ascending = true,
            b'D' | b'd' => ascending = false,
            _ => return INVALID,
        }
    }
    let mut insensitive = false;
    if a.n() > 2 && a.valid(2) {
        match a.first(2) {
            b'C' | b'c' => insensitive = false,
            b'I' | b'i' => insensitive = true,
            _ => return INVALID,
        }
    }
    let mut first: u32 = 1;
    let mut last: u32 = u32::MAX;
    let mut col0: u32 = 0;
    let mut col1: u32 = u32::MAX;
    if a.n() > 3 && a.valid(3) {
        match scan_long(a.cstr(3)) {
            Some(n) if n != 0 => first = n as u32,
            _ => return INVALID,
        }
    }
    if a.n() > 4 && a.valid(4) {
        match scan_long(a.cstr(4)) {
            Some(n) => last = n as u32,
            None => return INVALID,
        }
        if last < first {
            return INVALID;
        }
    }
    if a.n() > 5 && a.valid(5) {
        match scan_long(a.cstr(5)) {
            Some(n) => col0 = (n as u32).wrapping_sub(1),
            None => return INVALID,
        }
    }
    if a.n() == 7 && a.valid(6) {
        match scan_long(a.cstr(6)) {
            Some(n) => col1 = (n as u32).wrapping_sub(1),
            None => return INVALID,
        }
        if col1 < col0 {
            return INVALID;
        }
    }
    let (rc, n) = count(&stem);
    let mut ok = rc == 0;
    if ok {
        let Some(n) = n else {
            return INVALID;
        };
        if n < first {
            return INVALID;
        }
        if last == u32::MAX || n < last {
            last = n;
        }
        // The values (with their lengths, which the column sort uses).
        let mut items: Vec<Vec<u8>> = Vec::new();
        for i in first..=last {
            let (rc, v) = fetch_var(SHV_SYFET, &el(&stem, i));
            if rc != 0 {
                ok = false;
                break;
            }
            items.push(v);
        }
        if ok {
            let cols = col0 != 0 || col1 != u32::MAX;
            let width = col1.wrapping_sub(col0).wrapping_add(1);
            items.sort_by(|x, y| {
                let c = if !cols {
                    if insensitive { stricmp(x, y) } else { cs(x).cmp(cs(y)) as i32 }
                } else if x.len() as u32 > col0 && y.len() as u32 > col0 {
                    let (xs, ys) = (&x[col0 as usize..], &y[col0 as usize..]);
                    if insensitive {
                        strnicmp(xs, ys, width as i32)
                    } else {
                        let (xs, ys) = (cs(xs), cs(ys));
                        let w = width as usize;
                        xs[..xs.len().min(w)].cmp(&ys[..ys.len().min(w)]) as i32
                    }
                } else {
                    x.len().cmp(&y.len()) as i32
                };
                if ascending { ord(c) } else { ord(c).reverse() }
            });
            for (k, v) in items.iter().enumerate() {
                let rc = set_var(SHV_SYSET, &el(&stem, first + k as u32), v);
                if rc != 0 && rc != 1 {
                    ok = false;
                    break;
                }
            }
        }
    }
    r.ok_or_minus1(ok);
    0
}

/// SysStemDelete(stem, start [, count]).
pub fn sys_stem_delete(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 2 || a.n() > 3 || !a.valid(0) || !a.valid(1) || (a.n() == 3 && !a.valid(2)) {
        return INVALID;
    }
    let stem = stem_of(a.cstr(0));
    let Some(start) = scan_long(a.cstr(1)) else {
        return INVALID;
    };
    let start = start as u32;
    let mut del: u32 = 1;
    if a.n() == 3 {
        match scan_long(a.cstr(2)) {
            Some(n) if n != 0 => del = n as u32,
            _ => return INVALID,
        }
    }
    let (rc, n) = count(&stem);
    let mut ok = rc == 0;
    if ok {
        let Some(n) = n else {
            return INVALID;
        };
        if n < del.wrapping_add(start).wrapping_sub(1) {
            return INVALID;
        }
        let mut i = start;
        while ok && del.wrapping_add(i) <= n {
            ok = copy_el(&el(&stem, i + del), &el(&stem, i));
            i += 1;
        }
        if ok {
            for i in n.wrapping_sub(del).wrapping_add(1)..=n {
                if pool(SHV_SYDRO, &el(&stem, i), null_mut(), 0, 0).0 != 0 {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            let rc = set_var(SHV_SYSET, &tail(&stem, b"0"), &dec(n.wrapping_sub(del) as i32 as i64));
            ok = rc == 0 || rc == 1;
        }
    }
    r.ok_or_minus1(ok);
    0
}

/// SysStemInsert(stem, position, value).
pub fn sys_stem_insert(a: &Args, r: &mut Ret) -> u32 {
    if a.n() != 3 || !a.valid(0) || !a.valid(1) || !a.valid(2) {
        return INVALID;
    }
    let stem = stem_of(a.cstr(0));
    let Some(pos) = scan_long(a.cstr(1)) else {
        return INVALID;
    };
    let pos = pos as u32;
    let (rc, n) = count(&stem);
    let mut ok = rc == 0;
    if ok {
        let Some(n) = n else {
            return INVALID;
        };
        if pos == 0 || n.wrapping_add(1) < pos {
            return INVALID;
        }
        let mut i = n;
        while ok && pos <= i && i != 0 {
            ok = copy_el(&el(&stem, i), &el(&stem, i + 1));
            i -= 1;
        }
        if ok {
            let rc = set_var(SHV_SYSET, &el(&stem, pos), a.bytes(2));
            ok = rc == 0 || rc == 1;
        }
        if ok {
            let rc = set_var(SHV_SYSET, &tail(&stem, b"0"), &dec(n as i64 + 1));
            ok = rc == 0 || rc == 1;
        }
    }
    r.ok_or_minus1(ok);
    0
}

/// SysStemCopy(from, to [, fromstart [, tostart [, count [, 'I'|'O']]]]).
pub fn sys_stem_copy(a: &Args, r: &mut Ret) -> u32 {
    if a.n() < 2 || a.n() > 6 || !a.valid(0) || !a.valid(1) || (a.n() == 6 && !a.valid(5)) {
        return INVALID;
    }
    let from = stem_of(a.cstr(0));
    let to = stem_of(a.cstr(1));
    let (mut from_start, mut to_start, mut num) = (1u32, 1u32, 0u32);
    for (i, v) in [(2, &mut from_start), (3, &mut to_start), (4, &mut num)] {
        if a.n() > i && a.valid(i) {
            match scan_long(a.cstr(i)) {
                Some(n) => *v = n as u32,
                None => return INVALID,
            }
        }
    }
    let mut insert = false;
    if a.n() > 5 {
        match a.first(5) {
            b'I' | b'i' => insert = true,
            b'O' | b'o' => insert = false,
            _ => return INVALID,
        }
    }
    let (rc, n) = count(&from);
    let mut ok = rc == 0;
    let mut n_from = 0;
    if ok {
        let Some(n) = n else {
            return INVALID;
        };
        if n.wrapping_sub(from_start).wrapping_add(1) < num || n == 0 {
            return INVALID;
        }
        n_from = n;
    }
    let mut n_to = 0u32;
    if ok {
        let (rc, v) = fetch_small(SHV_SYFET, &tail(&to, b"0"));
        if rc == 0 {
            match scan_long(&v) {
                Some(n) => n_to = n as u32,
                None => return INVALID,
            }
        } else if rc != 1 {
            ok = false;
        }
        if ok && n_to.wrapping_add(1) < to_start {
            return INVALID;
        }
    }
    if num == 0 {
        num = n_from.wrapping_sub(from_start).wrapping_add(1);
    }
    if ok && insert {
        let mut i = n_to;
        while ok && to_start <= i && i != 0 {
            ok = copy_el(&el(&to, i), &el(&to, i + num));
            i -= 1;
        }
        if ok {
            n_to = n_to.wrapping_add(num);
            let rc = set_var(SHV_SYSET, &tail(&to, b"0"), &dec(n_to as i64));
            ok = rc == 0 || rc == 1;
        }
    }
    if ok && num != 0 {
        for k in 0..num {
            ok = copy_el(&el(&from, from_start.wrapping_add(k)), &el(&to, to_start.wrapping_add(k)));
            if !ok {
                break;
            }
        }
    }
    if ok && n_to < num.wrapping_add(to_start).wrapping_sub(1) {
        let rc = set_var(SHV_SYSET, &tail(&to, b"0"), &dec(num.wrapping_add(to_start).wrapping_sub(1) as i64));
        ok = rc == 0 || rc == 1;
    }
    r.ok_or_minus1(ok);
    0
}

fn write(h: u32, b: &[u8]) {
    let mut n = 0;
    unsafe { DosWrite(h, b.as_ptr(), b.len() as u32, &mut n) };
}

/// SysDumpVariables([file]): every variable as "Name=x, Value='y'",
/// to the file (appended) or standard output.
pub fn sys_dump_variables(a: &Args, r: &mut Ret) -> u32 {
    if a.n() > 1 || (a.n() == 1 && !a.valid(0)) {
        return INVALID;
    }
    let mut h = 0u32;
    let mut opened = false;
    let mut rc;
    if a.n() < 1 {
        h = 1;
        rc = 0;
    } else {
        let f = cz(a.cstr(0));
        let mut act = 0u32;
        rc = unsafe { DosOpen(f.as_ptr(), &mut h, &mut act, 0, 0, 0x11, 0x7022, null_mut()) };
        if rc == 0 {
            opened = true;
            let mut pos = 0u32;
            rc = unsafe { DosSetFilePtr(h, 0, FILE_END, &mut pos) };
        }
    }
    let _ = rc;
    loop {
        let mut b = ShvBlock::new(SHV_NEXTV);
        rc = unsafe { crate::api::RexxVariablePool(&mut b) };
        if rc == 0 {
            let name = unsafe { core::slice::from_raw_parts(b.name.ptr, b.name.len as usize) };
            let value = unsafe { core::slice::from_raw_parts(b.value.ptr, b.value.len as usize) };
            write(h, b"Name=");
            write(h, name);
            write(h, b", Value='");
            write(h, value);
            write(h, b"'\r\n");
            unsafe { DosFreeMem(b.name.ptr) };
            rc = unsafe { DosFreeMem(b.value.ptr) };
            if b.ret & SHV_LVAR != 0 {
                break;
            }
        }
        if rc != 0 {
            break;
        }
    }
    if opened {
        rc = unsafe { DosClose(h) };
    }
    r.ok_or_minus1(rc == 0);
    0
}
