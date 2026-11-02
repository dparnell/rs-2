//! Updates (Repository::saveIR): the definitions of the last file in
//! SOMIR (the one IBM's SOMIR writes to) that were changed through their
//! `_set_` methods go back into the file, as IBM's saveToIR does: a
//! definition's variable record (its modifiers, types, value, bases...)
//! is written again in space taken from the file's free list (first fit,
//! splitting a larger block) or at its end, and its old record becomes a
//! free block (size, 10, the next free block) at the head of the list in
//! the header; modes are rewritten in the fixed record, and a new name of
//! the same length in the fixed record and its container's contents. The
//! file is then written out whole.

use alloc::vec::Vec;

use somsup::data;

use crate::buf::put_tc;
use crate::file::FILES;
use crate::model::*;
use crate::*;

fn int(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn string(out: &mut Vec<u8>, p: *const u8) {
    if p.is_null() {
        int(out, -1);
        out.push(0);
    } else {
        let b = unsafe { somsup::cstr(p) };
        int(out, b.len() as i32);
        out.extend_from_slice(b);
        out.push(0);
    }
}

fn strings(out: &mut Vec<u8>, s: &Seq) {
    int(out, s.len as i32);
    for &p in s.items() {
        string(out, p as *const u8);
    }
}

/// A definition's variable record as it is now (its size word included).
fn variable_record(o: usize, code: u8, iid: u32) -> Vec<u8> {
    let mut v = Vec::new();
    int(&mut v, 0);
    int(&mut v, 11);
    int(&mut v, iid as i32);
    int(&mut v, 0);
    let c = cd(o);
    // somModifiers: its length counts the { name, value } pairs.
    let pairs: &[usize] = if c.mods.buf.is_null() { &[] } else { unsafe { core::slice::from_raw_parts(c.mods.buf, 2 * c.mods.len as usize) } };
    int(&mut v, (pairs.len() / 2) as i32);
    for p in pairs.chunks(2) {
        string(&mut v, p[0] as *const u8);
        string(&mut v, p.get(1).copied().unwrap_or(0) as *const u8);
    }
    match code {
        CODE_ATTRIBUTE => put_tc(&mut v, data::<[usize; 2]>(p!(AttributeDefCClassData), o)[1]),
        CODE_PARAMETER => put_tc(&mut v, data::<[usize; 2]>(p!(ParameterDefCClassData), o)[1]),
        CODE_TYPE => put_tc(&mut v, data::<[usize; 1]>(p!(TypeDefCClassData), o)[0]),
        CODE_EXCEPTION => put_tc(&mut v, data::<[usize; 1]>(p!(ExceptionDefCClassData), o)[0]),
        CODE_CONSTANT => {
            let d = data::<[usize; 2]>(p!(ConstantDefCClassData), o);
            put_tc(&mut v, d[0]);
            string(&mut v, d[1] as *const u8);
        }
        CODE_INTERFACE => {
            let d = data::<Iface>(p!(InterfaceDefCClassData), o);
            strings(&mut v, &d.bases);
            put_tc(&mut v, d.instance_data);
        }
        CODE_OPERATION => {
            let d = data::<Op>(p!(OperationDefCClassData), o);
            put_tc(&mut v, d.result);
            strings(&mut v, &d.contexts);
            strings(&mut v, &d.raises);
        }
        _ => {}
    }
    let n = v.len() as i32;
    v[..4].copy_from_slice(&n.to_be_bytes());
    v
}

fn get(d: &[u8], at: u32) -> i32 {
    let a = at as usize;
    if a + 4 > d.len() {
        return 0;
    }
    i32::from_be_bytes([d[a], d[a + 1], d[a + 2], d[a + 3]])
}

fn set(d: &mut [u8], at: u32, v: i32) {
    let a = at as usize;
    if a + 4 <= d.len() {
        d[a..a + 4].copy_from_slice(&v.to_be_bytes());
    }
}

/// Space for `n` bytes: the first free block big enough (split when
/// what is left can hold a free block), else the end of the file. The
/// space's size (what the record's size word says) comes back too.
fn allocate(d: &mut Vec<u8>, n: u32) -> (u32, u32) {
    let mut prev = 12u32;
    let mut at = get(d, 12) as u32;
    let mut guard = 0;
    while at != 0 && (at as usize) + 12 <= d.len() && guard < 100_000 {
        guard += 1;
        let size = get(d, at) as u32;
        let next = get(d, at + 8);
        if get(d, at + 4) == 10 && size >= n {
            if size - n >= 12 {
                // The rest stays free, in the block's place in the list.
                let rest = at + n;
                set(d, rest, (size - n) as i32);
                set(d, rest + 4, 10);
                set(d, rest + 8, next);
                set(d, prev, rest as i32);
                return (at, n);
            }
            set(d, prev, next);
            return (at, size);
        }
        prev = at + 8;
        at = next as u32;
    }
    let at = d.len() as u32;
    d.resize(d.len() + n as usize, 0);
    (at, n)
}

/// Make record `at` a free block at the head of the free list.
fn release(d: &mut [u8], at: u32) {
    let size = get(d, at);
    if size < 12 {
        return;
    }
    let head = get(d, 12);
    set(d, at + 4, 10);
    set(d, at + 8, head);
    set(d, 12, at as i32);
}

/// Repository::saveIR: write the changed definitions of the last file.
pub fn save_ir() -> bool {
    #[allow(static_mut_refs)]
    let files = unsafe { &mut FILES };
    let Some(fi) = files.len().checked_sub(1) else { return false };
    let fi = fi as u32;
    let mut changed = false;
    let objs: Vec<(u32, usize)> = cached().into_iter().filter(|&(f, _, _)| f == fi).map(|(_, iid, o)| (iid, o)).collect();
    for (iid, o) in objs {
        let d = &mut files[fi as usize].data;
        let code = get(d, iid + 4) as u8;
        let c = cd(o);
        // The fixed record: the name (its string), then 0, defined_in, the
        // reference list, the variable record, the class's part.
        let name_len = get(d, iid + 8);
        let after = iid + 8 + 4 + (if name_len < 0 { 1 } else { name_len as u32 + 1 });
        let new_name = unsafe { somsup::cstr(c.name) };
        if name_len >= 0 && new_name.len() == name_len as usize && &d[iid as usize + 12..iid as usize + 12 + new_name.len()] != new_name {
            d[iid as usize + 12..iid as usize + 12 + new_name.len()].copy_from_slice(new_name);
            rename_in_contents(d, iid, new_name);
            changed = true;
        }
        let mode_at = match code {
            CODE_ATTRIBUTE | CODE_PARAMETER => Some((after + 16, data::<[usize; 2]>(if code == CODE_ATTRIBUTE { p!(AttributeDefCClassData) } else { p!(ParameterDefCClassData) }, o)[0] as i32)),
            CODE_OPERATION => Some((after + 24, data::<Op>(p!(OperationDefCClassData), o).mode as i32)),
            _ => None,
        };
        if let Some((at, m)) = mode_at {
            if get(d, at) != m {
                set(d, at, m);
                changed = true;
            }
        }
        let rec = variable_record(o, code, iid);
        let old = c.var_iid;
        let same = old != 0 && get(d, old + 4) == 11 && {
            let a = old as usize;
            // Compare all but the size word (the space may be larger).
            a + rec.len() <= d.len() && d[a + 4..a + rec.len()] == rec[4..]
        };
        if same {
            continue;
        }
        let (at, space) = allocate(d, rec.len() as u32);
        d[at as usize..at as usize + rec.len()].copy_from_slice(&rec);
        set(d, at, space as i32);
        set(d, after + 12, at as i32);
        if old != 0 && get(d, old + 4) == 11 {
            release(d, old);
        }
        c.var_iid = at;
        changed = true;
    }
    if changed {
        let f = &files[fi as usize];
        write_file(&f.name, &f.data);
    }
    changed
}

/// A renamed definition's entry in its container's contents record.
fn rename_in_contents(d: &mut [u8], iid: u32, name: &[u8]) {
    let parent = get(d, iid + 8 + 4 + name.len() as u32 + 1 + 4) as u32;
    let contents = if parent == 0 {
        get(d, 8) as u32
    } else {
        let code = get(d, parent + 4) as u8;
        let pl = get(d, parent + 8);
        let after = parent + 12 + if pl < 0 { 1 } else { pl as u32 + 1 };
        match code {
            CODE_INTERFACE | CODE_MODULE | CODE_OPERATION => get(d, after + 16) as u32,
            _ => return,
        }
    };
    if get(d, contents + 4) != 8 {
        return;
    }
    let n = get(d, contents + 8).max(0);
    let mut q = contents + 12;
    for _ in 0..n {
        let e = get(d, q) as u32;
        let len = get(d, q + 8);
        if e == iid && len == name.len() as i32 {
            d[q as usize + 12..q as usize + 12 + name.len()].copy_from_slice(name);
            return;
        }
        q += 12 + if len < 0 { 1 } else { len as u32 + 1 };
    }
}

fn write_file(name: &[u8], data: &[u8]) {
    let mut h = 0u32;
    let mut action = 0u32;
    // OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS;
    // OPEN_SHARE_DENYWRITE | OPEN_ACCESS_WRITEONLY.
    if unsafe { os2::DosOpen(name.as_ptr(), &mut h, &mut action, data.len() as u32, 0, 0x12, 0x21, core::ptr::null_mut()) } != 0 {
        return;
    }
    let mut n = 0u32;
    unsafe {
        os2::DosWrite(h, data.as_ptr(), data.len() as u32, &mut n);
        os2::DosClose(h);
    }
}
