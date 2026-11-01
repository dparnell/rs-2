//! String handles (HSZ) and data handles (HDDEDATA), as IBM's PMDDEML
//! keeps them (`DdeCreateHandle`, `DdeCreateStringHandle`,
//! `DdeGetStringHandle`, `DdeQueryString`, `DdeCompareStringHandles`,
//! `DdeMakeDdeStruct`, `DdeCreateDataHandle`): one table, the handle's
//! kind in its top bits (0x10000000 data, 0x20000000 a string,
//! 0x40000000 a string naming one instance of a service: the string
//! followed by ":(hwnd)" when queried) and the entry's index below.
//!
//! IBM's table is shared memory and its strings atoms; rs2's is the
//! process's own (handles are only ever used in the process that made
//! them: what crosses processes are DDEINIT and DDESTRUCT blocks with the
//! strings in them). A data handle stands for a DDESTRUCT in giveable
//! shared memory, ready to be posted with WinDdePostMsg.

use alloc::vec::Vec;
use core::ptr::{null, null_mut};

use os2::{DosAllocSharedMem, DosFreeMem};

use crate::pm::*;

pub const H_DATA: u32 = 0x1000_0000;
pub const H_STR: u32 = 0x2000_0000;
pub const H_INST: u32 = 0x4000_0000;
const H_INDEX: u32 = 0x0fff_ffff;

/// HDATA_APPOWNED: the application frees the handle (it is posted with
/// DDEPM_NOFREE).
pub const HDATA_APPOWNED: u32 = 2;

enum Entry {
    Free,
    Str { s: Vec<u8>, up: Vec<u8>, cp: u32, refs: u32, hwnd: u32 },
    Data { p: *mut u8, refs: u32, flags: u32, locked: bool },
}

static mut TABLE: Vec<Entry> = Vec::new();

fn table() -> &'static mut Vec<Entry> {
    unsafe { &mut *core::ptr::addr_of_mut!(TABLE) }
}

fn insert(e: Entry) -> u32 {
    let t = table();
    if let Some(i) = t.iter().position(|e| matches!(e, Entry::Free)) {
        t[i] = e;
        i as u32
    } else {
        t.push(e);
        t.len() as u32 - 1
    }
}

fn entry(h: u32) -> Option<&'static mut Entry> {
    table().get_mut((h & H_INDEX) as usize)
}

// ---- Strings ----

/// The process's code page (IBM's: the queue's, WinQueryCp).
pub fn process_cp() -> u32 {
    unsafe { WinQueryCp(HMQ_CURRENT) }
}

fn upper(s: &[u8], cp: u32) -> Vec<u8> {
    let mut u: Vec<u8> = s.to_vec();
    u.push(0);
    if unsafe { WinUpper(0, cp, 0, u.as_mut_ptr()) } == 0 {
        for c in u.iter_mut() {
            c.make_ascii_uppercase();
        }
    }
    u.pop();
    u
}

/// A string handle for `s` (IBM's DdeCreateStringHandle and
/// DdeGetStringHandle): an existing handle of the same string (and kind)
/// is kept once more, else a new one is made. Empty strings have none.
pub fn string_handle(s: &[u8], cp: u32, hwnd: u32) -> u32 {
    if s.is_empty() {
        return 0;
    }
    let cp = if cp == 0 { process_cp() } else { cp };
    for (i, e) in table().iter_mut().enumerate() {
        if let Entry::Str { s: es, hwnd: eh, refs, .. } = e
            && es.as_slice() == s
            && *eh == hwnd
        {
            *refs += 1;
            return i as u32 | if hwnd != 0 { H_INST } else { H_STR };
        }
    }
    let up = upper(s, cp);
    let i = insert(Entry::Str { s: s.to_vec(), up, cp, refs: 1, hwnd });
    i | if hwnd != 0 { H_INST } else { H_STR }
}

pub fn is_string(h: u32) -> bool {
    h & (H_STR | H_INST) != 0 && matches!(entry(h), Some(Entry::Str { .. }))
}

/// The string of a handle (without an instance's suffix), its code page
/// and, for an instance-specific one, the instance's window.
pub fn string(h: u32) -> Option<(Vec<u8>, u32, u32)> {
    if h & (H_STR | H_INST) == 0 {
        return None;
    }
    match entry(h)? {
        Entry::Str { s, cp, hwnd, .. } => Some((s.clone(), *cp, *hwnd)),
        _ => None,
    }
}

pub fn keep_string(h: u32) -> bool {
    if h & (H_STR | H_INST) == 0 {
        return false;
    }
    match entry(h) {
        Some(Entry::Str { refs, .. }) => {
            *refs += 1;
            true
        }
        _ => false,
    }
}

pub fn free_string(h: u32) -> bool {
    if h == 0 || h & (H_STR | H_INST) == 0 {
        return false;
    }
    let Some(e) = entry(h) else { return false };
    let Entry::Str { refs, .. } = e else { return false };
    *refs -= 1;
    if *refs == 0 {
        *e = Entry::Free;
    }
    true
}

/// DdeQueryString: the string (an instance's with ":(hwnd)") in code
/// page `cp`, at most cch - 1 characters and a NUL; the length it has
/// when `buf` is null.
pub fn query_string(h: u32, buf: *mut u8, cch: u32, cp: u32) -> u32 {
    let Some((mut s, scp, hwnd)) = string(h) else { return 0 };
    if hwnd != 0 {
        s.extend_from_slice(alloc::format!(":({hwnd})").as_bytes());
    }
    if buf.is_null() {
        return s.len() as u32;
    }
    if cch == 0 {
        return 0;
    }
    let n = s.len().min(cch as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    let cp = if cp == 0 { process_cp() } else { cp };
    if cp != scp && scp != 0 {
        unsafe { WinCpTranslateString(0, scp, buf, cp, n as u32 + 1, buf) };
    }
    n as u32
}

/// DdeCompareStringHandles: WCS_EQ (1), WCS_LT (2) or WCS_GT (3);
/// case-insensitive unless `case` (DDECTXT_CASESENSITIVE).
pub fn compare(h1: u32, h2: u32, case: bool) -> u32 {
    if h1 == h2 {
        return 1;
    }
    let get = |h: u32| -> Option<Vec<u8>> {
        if h == 0 {
            return Some(Vec::new());
        }
        match entry(h)? {
            Entry::Str { s, up, .. } => Some(if case { s.clone() } else { up.clone() }),
            _ => None,
        }
    };
    let (Some(a), Some(b)) = (get(h1), get(h2)) else { return 0 };
    match a.cmp(&b) {
        core::cmp::Ordering::Equal => 1,
        core::cmp::Ordering::Less => 2,
        core::cmp::Ordering::Greater => 3,
    }
}

pub fn equal(h1: u32, h2: u32, case: bool) -> bool {
    compare(h1, h2, case) == 1
}

// ---- DDESTRUCT ----

/// A DDESTRUCT (cbData, fsStatus, usFormat, offszItemName, offabData)
/// with the item's name and room for `cb` bytes (`data` copied in), in
/// giveable shared memory (IBM's DdeMakeDdeStruct).
pub fn make_struct(item: u32, data: *const u8, ncopy: u32, cb: u32, fmt: u16, status: u16) -> *mut u8 {
    let name = string(item).map(|s| s.0).unwrap_or_default();
    let off = 12 + name.len() as u32 + 1;
    let mut p: *mut u8 = null_mut();
    if unsafe { DosAllocSharedMem(&mut p, null(), off + cb.max(1), GIVEABLE) } != 0 {
        return null_mut();
    }
    unsafe {
        (p as *mut u32).write_unaligned(cb);
        (p.add(4) as *mut u16).write_unaligned(status);
        (p.add(6) as *mut u16).write_unaligned(fmt);
        (p.add(8) as *mut u16).write_unaligned(12);
        (p.add(10) as *mut u16).write_unaligned(off as u16);
        core::ptr::copy_nonoverlapping(name.as_ptr(), p.add(12), name.len());
        *p.add(12 + name.len()) = 0;
        if !data.is_null() && ncopy != 0 {
            core::ptr::copy_nonoverlapping(data, p.add(off as usize), ncopy.min(cb) as usize);
        }
    }
    p
}

pub struct Dde {
    pub cb: u32,
    pub status: u16,
    pub fmt: u16,
    pub item: Vec<u8>,
    pub data: *mut u8,
}

pub fn parse(p: *const u8) -> Dde {
    unsafe {
        let oi = (p.add(8) as *const u16).read_unaligned() as usize;
        let od = (p.add(10) as *const u16).read_unaligned() as usize;
        let item = if oi == 0 { Vec::new() } else { (0..).map(|i| *p.add(oi + i)).take_while(|&b| b != 0).collect() };
        Dde {
            cb: (p as *const u32).read_unaligned(),
            status: (p.add(4) as *const u16).read_unaligned(),
            fmt: (p.add(6) as *const u16).read_unaligned(),
            item,
            data: if od == 0 { null_mut() } else { p.add(od) as *mut u8 },
        }
    }
}

pub fn set_status(p: *mut u8, status: u16) {
    unsafe { (p.add(4) as *mut u16).write_unaligned(status) };
}

pub fn set_format(p: *mut u8, fmt: u16) {
    unsafe { (p.add(6) as *mut u16).write_unaligned(fmt) };
}

// ---- Data ----

/// A data handle for the DDESTRUCT `p` (handed over to the handle).
pub fn data_handle(p: *mut u8, flags: u32) -> u32 {
    if p.is_null() {
        return 0;
    }
    insert(Entry::Data { p, refs: 1, flags, locked: false }) | H_DATA
}

pub fn is_data(h: u32) -> bool {
    h & H_DATA != 0 && matches!(entry(h), Some(Entry::Data { .. }))
}

/// The handle's DDESTRUCT and its afCmd flags.
pub fn data(h: u32) -> Option<(*mut u8, u32)> {
    if h & H_DATA == 0 {
        return None;
    }
    match entry(h)? {
        Entry::Data { p, flags, .. } => Some((*p, *flags)),
        _ => None,
    }
}

pub fn set_data(h: u32, np: *mut u8) {
    if let Some(Entry::Data { p, .. }) = entry(h) {
        *p = np;
    }
}

/// While a callback has the handle, freeing it does nothing (IBM's
/// DdeLockDataHandle).
pub fn lock_data(h: u32, on: bool) {
    if let Some(Entry::Data { locked, .. }) = entry(h) {
        *locked = on;
    }
}

/// DdeFreeDataHandle: the DDESTRUCT goes with the last reference.
pub fn free_data(h: u32) -> bool {
    if h & H_DATA == 0 {
        return false;
    }
    let Some(e) = entry(h) else { return false };
    let Entry::Data { p, refs, locked, .. } = e else { return false };
    if *locked {
        return true;
    }
    *refs -= 1;
    if *refs == 0 {
        unsafe { DosFreeMem(*p) };
        *e = Entry::Free;
    }
    true
}

/// The handle goes, its DDESTRUCT stays (it was posted or freed
/// elsewhere: IBM's DdeRemoveDataHandle).
pub fn remove_data(h: u32) {
    if h & H_DATA == 0 {
        return;
    }
    if let Some(e) = entry(h)
        && matches!(e, Entry::Data { .. })
    {
        *e = Entry::Free;
    }
}
