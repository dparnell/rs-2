//! HELPMGR's unpublished entry points IBM's VIEW.EXE and VIEWDOC.EXE use
//! (ordinals 26 and 28-47, studied in IBM's HELPMGR, VIEW and VIEWDOC
//! with Ghidra). VIEW opens the books itself to find the topic: it zeroes
//! a 0x164-byte block (IBM's help instance structure, `+0x0C` 1 for
//! books), opens the books in it (HMSetHelpDatabase), loads the TOC and
//! index there (IOLoadAllTOC: count at `+0x76`, an array of entry
//! pointers at `+0x78`, each entry `char *title, u8 level, u16 flags`
//! with 0x800 for entries not in the contents; IOLoadAllIndex: count at
//! `+0x68`, array at `+0x6A`, each `char *text, ..., u16 TOC index at
//! +8, u8 flags at +10` (0x80: an entry to search, 0x01/0x02 its level))
//! and compares with FindPattern and the like (the NLS information they
//! get, `+0x3A` of the block at `+4`, is not used). Then it starts
//! VIEWDOC.EXE, which opens the books with WinCreateBookInstance and
//! sends the instance HM_HELP_CONTENTS, 0x235 (show a TOC entry), 0x236
//! (bring the window up) and 0x1000 (its window, told 0x1003 with the
//! instance when the book's window closes).

use crate::library::{self, Library};
use crate::pm::*;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// What VIEW's block holds for us: the books and the arrays it reads.
struct Db {
    block: usize,
    libs: Vec<Library>,
    /// The block at `+4` (IBM's I/O information), zeroed.
    io: Box<[u8; 0x50]>,
    toc: Option<Arrays>,
    index: Option<Arrays>,
}

/// Entries (fixed-size records with string pointers), their strings and
/// the pointer array (VIEW reads the first two through the pointers).
#[allow(dead_code)]
struct Arrays {
    entries: Vec<[u8; 12]>,
    strings: Vec<Vec<u8>>,
    ptrs: Vec<u32>,
}

struct Registry(core::cell::UnsafeCell<Vec<Box<Db>>>);
unsafe impl Sync for Registry {}
static DBS: Registry = Registry(core::cell::UnsafeCell::new(Vec::new()));

fn dbs() -> &'static mut Vec<Box<Db>> {
    unsafe { &mut *DBS.0.get() }
}

fn db(block: u32) -> Option<&'static mut Db> {
    dbs().iter_mut().find(|d| d.block == block as usize).map(|b| &mut **b)
}

fn put32(block: u32, off: usize, v: u32) {
    unsafe { ((block as usize + off) as *mut u32).write_unaligned(v) }
}

fn put16(block: u32, off: usize, v: u16) {
    unsafe { ((block as usize + off) as *mut u16).write_unaligned(v) }
}

fn byte(block: u32, off: usize) -> *mut u8 {
    (block as usize + off) as *mut u8
}

/// Lay the entries out: the strings NUL-ended, each entry's first
/// dword pointing at its string.
fn arrays(mut entries: Vec<[u8; 12]>, strings: Vec<Vec<u8>>) -> Arrays {
    let mut strings: Vec<Vec<u8>> = strings.into_iter().map(|mut s| {
        s.push(0);
        s
    }).collect();
    for (e, s) in entries.iter_mut().zip(strings.iter_mut()) {
        e[0..4].copy_from_slice(&(s.as_ptr() as u32).to_le_bytes());
    }
    let ptrs = entries.iter().map(|e| e.as_ptr() as u32).collect();
    Arrays { entries, strings, ptrs }
}

/// HMSetHelpDatabase(block, names) (ordinal 37): open the books of
/// `names` (blank- or `+`-separated) in VIEW's block; TRUE if one opened.
#[unsafe(no_mangle)]
pub extern "C" fn HMSetHelpDatabase(block: u32, names: *const u8) -> u32 {
    if block == 0 || names.is_null() {
        return 0;
    }
    IOFreeIOInfoList(block);
    let book = unsafe { (byte(block, 0x0c) as *const u16).read_unaligned() } != 0;
    let mut libs: Vec<Library> = Vec::new();
    for n in library::names(unsafe { cstr(names) }) {
        if let Ok(l) = library::open(&n, true) {
            if book || l.doc().is_help() {
                libs.push(l);
            }
        }
    }
    if libs.is_empty() {
        return 0;
    }
    let mut base = 0;
    for l in libs.iter_mut() {
        l.base = base;
        base += l.doc().header.toc_count as u32;
    }
    let d = Box::new(Db { block: block as usize, libs, io: Box::new([0; 0x50]), toc: None, index: None });
    let io = d.io.as_ptr() as u32;
    dbs().push(d);
    put32(block, 0, io);
    put32(block, 4, io);
    1
}

/// IOLoadAllTOC(block) (28): the TOC entries of all the books.
#[unsafe(no_mangle)]
pub extern "C" fn IOLoadAllTOC(block: u32) -> u32 {
    let Some(d) = db(block) else { return 0 };
    if d.toc.is_none() {
        let (mut entries, mut strings) = (Vec::new(), Vec::new());
        for l in &d.libs {
            let doc = l.doc();
            for t in 0..doc.header.toc_count {
                let e = doc.toc(t).unwrap_or_default();
                let mut r = [0u8; 12];
                r[4] = e.level();
                let flags: u16 = e.flags as u16 | if e.hidden() || e.level() == 0 { 0x800 } else { 0 };
                r[5..7].copy_from_slice(&flags.to_le_bytes());
                entries.push(r);
                strings.push(e.title);
            }
        }
        d.toc = Some(arrays(entries, strings));
    }
    let a = d.toc.as_ref().unwrap();
    put16(block, 0x76, a.ptrs.len() as u16);
    put32(block, 0x78, a.ptrs.as_ptr() as u32);
    unsafe { *byte(block, 0x84) = 1 };
    1
}

/// IOLoadAllIndex(block, how) (29): the index entries of all the books.
#[unsafe(no_mangle)]
pub extern "C" fn IOLoadAllIndex(block: u32, _how: u32) -> u32 {
    let Some(d) = db(block) else { return 0 };
    if d.index.is_none() {
        let (mut entries, mut strings) = (Vec::new(), Vec::new());
        for l in &d.libs {
            for e in l.doc().index() {
                let mut r = [0u8; 12];
                r[8..10].copy_from_slice(&((e.toc as u32 + l.base) as u16).to_le_bytes());
                r[10] = 0x80 | (e.flags & 0x03);
                entries.push(r);
                strings.push(e.text);
            }
        }
        if entries.is_empty() {
            return 0;
        }
        d.index = Some(arrays(entries, strings));
    }
    let a = d.index.as_ref().unwrap();
    put16(block, 0x68, a.ptrs.len() as u16);
    put32(block, 0x6a, a.ptrs.as_ptr() as u32);
    unsafe { *byte(block, 0x85) = 1 };
    1
}

/// IOFreeAllTOC(block) (30).
#[unsafe(no_mangle)]
pub extern "C" fn IOFreeAllTOC(block: u32) {
    if let Some(d) = db(block) {
        d.toc = None;
        put16(block, 0x76, 0);
        put32(block, 0x78, 0);
        unsafe { *byte(block, 0x84) = 0 };
    }
}

/// IOFreeAllIndex(block) (31).
#[unsafe(no_mangle)]
pub extern "C" fn IOFreeAllIndex(block: u32) {
    if let Some(d) = db(block) {
        d.index = None;
        put16(block, 0x68, 0);
        put32(block, 0x6a, 0);
        unsafe { *byte(block, 0x85) = 0 };
    }
}

/// IOFreeIOInfoList(block) (32): close the books.
#[unsafe(no_mangle)]
pub extern "C" fn IOFreeIOInfoList(block: u32) {
    if db(block).is_some() {
        IOFreeAllTOC(block);
        IOFreeAllIndex(block);
        dbs().retain(|d| d.block != block as usize);
    }
    if block != 0 {
        put32(block, 0, 0);
        put32(block, 4, 0);
    }
}

fn upper(c: u8) -> u8 {
    c.to_ascii_uppercase()
}

fn bytes<'a>(p: *const u8, n: i32) -> &'a [u8] {
    if p.is_null() || n <= 0 { &[] } else { unsafe { core::slice::from_raw_parts(p, n as usize) } }
}

/// MscStrniCmp(a, b, n, nls) (47): 0 if the first `n` characters match,
/// ignoring case.
#[unsafe(no_mangle)]
pub extern "C" fn MscStrniCmp(a: *const u8, b: *const u8, n: i32, _nls: u32) -> i32 {
    for i in 0..n.max(0) as usize {
        let (x, y) = unsafe { (upper(*a.add(i)), upper(*b.add(i))) };
        if x != y {
            return x as i32 - y as i32;
        }
        if x == 0 {
            return 0;
        }
    }
    0
}

/// MscStriCmp(a, b, nls) (46): 0 if equal, ignoring case.
#[unsafe(no_mangle)]
pub extern "C" fn MscStriCmp(a: *const u8, b: *const u8, nls: u32) -> i32 {
    MscStrniCmp(a, b, i32::MAX, nls)
}

/// FindPattern(text, pattern, cchText, cchPattern, nls) (34): TRUE if
/// the pattern is in the text (ignoring case).
#[unsafe(no_mangle)]
pub extern "C" fn FindPattern(text: *const u8, pat: *const u8, n: i32, m: i32, _nls: u32) -> u32 {
    let (t, p) = (bytes(text, n), bytes(pat, m));
    (!p.is_empty() && t.windows(p.len()).any(|w| w.eq_ignore_ascii_case(p))) as u32
}

/// FindWildPattern(text, pattern, cchText, cchPattern, nls) (33): the
/// pattern with a `*` (at its start, end or one inside) matches the text.
#[unsafe(no_mangle)]
pub extern "C" fn FindWildPattern(text: *const u8, pat: *const u8, n: i32, m: i32, _nls: u32) -> u32 {
    let (t, p) = (bytes(text, n), bytes(pat, m));
    let has = |t: &[u8], p: &[u8]| p.is_empty() || t.windows(p.len()).any(|w| w.eq_ignore_ascii_case(p));
    let parts: Vec<&[u8]> = p.split(|&c| c == b'*').collect();
    match parts.as_slice() {
        [one] => has(t, one),
        [a, b] if a.is_empty() || b.is_empty() => has(t, if a.is_empty() { b } else { a }),
        [a, b] => t.windows(a.len()).enumerate().any(|(i, w)| w.eq_ignore_ascii_case(a) && has(&t[i + a.len()..], b)),
        [a, mid, b] if a.is_empty() && b.is_empty() => has(t, mid),
        _ => has(t, &p.iter().copied().filter(|&c| c != b'*').collect::<Vec<u8>>()),
    }
    .then_some(1)
    .unwrap_or(0)
}

/// GetStrToken(s) (36): the length of the word at `s` (at least 1; 0 at
/// the end).
#[unsafe(no_mangle)]
pub extern "C" fn GetStrToken(s: *const u8) -> u32 {
    let t = unsafe { cstr(s) };
    if t.is_empty() {
        return 0;
    }
    (t.iter().take_while(|&&c| c != b' ').count() as u32).max(1)
}

/// FindTokenPattern(text, pattern, nls) (35): every word of the pattern
/// is a word of the text, in order.
#[unsafe(no_mangle)]
pub extern "C" fn FindTokenPattern(text: *const u8, pat: *const u8, _nls: u32) -> u32 {
    let words = |s: *const u8| -> Vec<Vec<u8>> { unsafe { cstr(s) }.split(|&c| c == b' ').filter(|w| !w.is_empty()).map(|w| w.to_ascii_uppercase()).collect() };
    let (t, p) = (words(text), words(pat));
    let mut i = 0;
    for w in &p {
        match t[i..].iter().position(|x| x == w) {
            Some(k) => i += k + 1,
            None => return 0,
        }
    }
    1
}

/// MscAppendINF(names, out, cb) (45): the books of `names` (an
/// environment variable naming them, or names separated by `+` or
/// blanks), `.INF` added where there is no extension, blank-separated in
/// `out`; FALSE if they do not fit.
#[unsafe(no_mangle)]
pub extern "C" fn MscAppendINF(names: *const u8, out: *mut u8, cb: u32) -> u32 {
    if names.is_null() || out.is_null() || cb == 0 {
        return 0;
    }
    let mut list = unsafe { cstr(names) }.to_vec();
    if !list.contains(&b'\\') {
        let var = z(&list.to_ascii_uppercase());
        let mut p: *const u8 = core::ptr::null();
        if unsafe { os2::dos::DosScanEnv(var.as_ptr(), &mut p) } == 0 {
            list = unsafe { cstr(p) }.to_vec();
        }
    }
    let mut r: Vec<u8> = Vec::new();
    for n in library::names(&list) {
        if !r.is_empty() {
            r.push(b' ');
        }
        r.extend_from_slice(&n);
        let base = n.rsplit(|&c| c == b'\\' || c == b':').next().unwrap_or(&n);
        if !base.contains(&b'.') {
            r.extend_from_slice(b".INF");
        }
    }
    if r.len() + 1 > cb as usize {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(r.as_ptr(), out, r.len());
        *out.add(r.len()) = 0;
    }
    1
}

/// WinCreateBookInstance(hab, phinit) (26): a help instance for books
/// (HELPINIT's pszHelpLibraryName), its window shown by HM_HELP_CONTENTS
/// or message 0x235; HELPINIT's ulReturnCode gets the error.
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateBookInstance(hab: u32, init: *mut u32) -> u32 {
    crate::stack::deep(|| WinCreateBookInstance_(hab, init))
}

fn WinCreateBookInstance_(hab: u32, init: *mut u32) -> u32 {
    if init.is_null() {
        return 0;
    }
    let f = |i: usize| unsafe { init.add(i).read_unaligned() };
    let names = unsafe { cstr(f(10) as *const u8) }.to_vec();
    match crate::create(hab, &names, b"", crate::HelpTable::None, false, true) {
        Ok(h) => {
            let inst = crate::instance(h).unwrap();
            inst.title = inst.libs[0].doc().title().to_vec();
            unsafe { init.add(1).write_unaligned(0) };
            h
        }
        Err(e) => {
            unsafe { init.add(1).write_unaligned(e) };
            0
        }
    }
}
