//! SOMU's functions and variables: SOMUTId/SOMUTStringId (IBM's methods
//! report "not implemented"), UUIDs, the thread procedures, the heap
//! watcher (somwatch*), and the externalization helpers.

use alloc::vec::Vec;

use somsup::{aligned, call_parent, methods, optlink};

use crate::*;

pub fn init() {}

// ---- SOMUTId, SOMUTStringId ----

/// IBM's SOMUTId and SOMUTStringId methods are not implemented: they
/// report SOM error 20189 (fatal) with their source file and line.
fn not_implemented(file: &'static [u8], line: i32) -> usize {
    somsup::error(20189, file, line);
    0
}

extern "C" fn id_set_id(_o: usize, _ev: usize, _other: usize) {
    not_implemented(b"somida.c\0", 0x27);
}
extern "C" fn id_equals(_o: usize, _ev: usize, _other: usize) -> u32 {
    not_implemented(b"somida.c\0", 0x37) as u32
}
extern "C" fn id_compare(_o: usize, _ev: usize, _other: usize) -> i32 {
    not_implemented(b"somida.c\0", 0x4b) as i32
}
extern "C" fn id_hash(_o: usize, _ev: usize) -> u32 {
    not_implemented(b"somida.c\0", 0x5a) as u32
}
extern "C" fn sid_set(_o: usize, _ev: usize, _s: usize) -> i32 {
    not_implemented(b"somsid.c\0", 0x28) as i32
}
extern "C" fn sid_get(_o: usize, _ev: usize, buf: usize) -> usize {
    not_implemented(b"somsid.c\0", 0x3b);
    buf
}
extern "C" fn sid_len(_o: usize, _ev: usize) -> i32 {
    not_implemented(b"somsid.c\0", 0x4b) as i32
}
extern "C" fn sid_equals(_o: usize, _ev: usize, _s: usize) -> u32 {
    not_implemented(b"somsid.c\0", 0x5d) as u32
}
extern "C" fn sid_compare(_o: usize, _ev: usize, _s: usize) -> i32 {
    not_implemented(b"somsid.c\0", 0x71) as i32
}
// SOMUTStringId's overrides of SOMUTId's methods are the parent's.
extern "C" fn sid_set_id(o: usize, ev: usize, other: usize) {
    call_parent(p!(SOMUTStringIdCClassData), p!(SOMUTIdClassData), 0, &[o, ev, other]);
}
extern "C" fn sid_equals_id(o: usize, ev: usize, other: usize) -> u32 {
    call_parent(p!(SOMUTStringIdCClassData), p!(SOMUTIdClassData), 1, &[o, ev, other]) as u32
}
extern "C" fn sid_compare_id(o: usize, ev: usize, other: usize) -> i32 {
    call_parent(p!(SOMUTStringIdCClassData), p!(SOMUTIdClassData), 2, &[o, ev, other]) as i32
}
extern "C" fn sid_hash_id(o: usize, ev: usize) -> u32 {
    call_parent(p!(SOMUTStringIdCClassData), p!(SOMUTIdClassData), 3, &[o, ev]) as u32
}

methods! {
    __m_id_set_id => id_set_id, 3;
    __m_id_equals => id_equals, 3;
    __m_id_compare => id_compare, 3;
    __m_id_hash => id_hash, 2;
    __m_sid_set => sid_set, 3;
    __m_sid_get => sid_get, 3;
    __m_sid_len => sid_len, 2;
    __m_sid_equals => sid_equals, 3;
    __m_sid_compare => sid_compare, 3;
    __m_sid_set_id => sid_set_id, 3;
    __m_sid_equals_id => sid_equals_id, 3;
    __m_sid_compare_id => sid_compare_id, 3;
    __m_sid_hash_id => sid_hash_id, 2;
}

// ---- UUIDs (_Optlink) ----

const HEX: &[u8; 16] = b"0123456789abcdef";

/// somUUIDToString(uuid, buffer): IBM's text form, bytes 3-0, 7-4, 8, 9,
/// 10-15: `xxxxxxxx-xxxxxxxx-xx-xx-xxxxxxxxxxxx`.
pub extern "C" fn somUUIDToString(uuid: *const u8, out: *mut u8) -> *mut u8 {
    let order: [usize; 16] = [3, 2, 1, 0, 7, 6, 5, 4, 8, 9, 10, 11, 12, 13, 14, 15];
    let mut s = Vec::with_capacity(37);
    for (k, &i) in order.iter().enumerate() {
        if matches!(k, 4 | 8 | 9 | 10) {
            s.push(b'-');
        }
        let b = unsafe { *uuid.add(i) };
        s.push(HEX[(b >> 4) as usize]);
        s.push(HEX[(b & 15) as usize]);
    }
    s.push(0);
    unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), out, s.len()) };
    out
}

fn hexval(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// somStringToUUID(text, uuid): the hex digit pairs in order (the dashes
/// skipped), as IBM's (which does not undo somUUIDToString's byte order).
pub extern "C" fn somStringToUUID(text: *const u8, uuid: *mut u8) -> i32 {
    unsafe { core::ptr::write_bytes(uuid, 0, 16) };
    let t = unsafe { somsup::cstr(text) };
    let digits: Vec<u8> = t.iter().copied().filter(|&c| c != b'-').collect();
    for (i, pair) in digits.chunks(2).take(16).enumerate() {
        let v = (hexval(pair[0]) << 4) | pair.get(1).map_or(0, |&c| hexval(c));
        unsafe { *uuid.add(i) = v };
    }
    0
}

pub extern "C" fn somCompareUUIDStrings(a: *const u8, b: *const u8) -> i32 {
    let (x, y) = unsafe { (somsup::cstr(a), somsup::cstr(b)) };
    for i in 0..=x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q || q == 0 {
            return p as i32 - q as i32;
        }
    }
    0
}

static mut UUID_COUNT: u32 = 0;
static mut SEED: u32 = 0;

fn rand16() -> u16 {
    unsafe {
        SEED = SEED.wrapping_mul(1103515245).wrapping_add(12345);
        (SEED >> 16) as u16
    }
}

/// somCreateUUID(uuid): the time and a counter, 0x7f, then random bytes
/// (IBM's takes the last six from the network adapter's address when
/// NetBIOS can tell it).
pub extern "C" fn somCreateUUID(uuid: *mut u8) -> i32 {
    let mut t = [0u32; 2];
    unsafe { os2::DosQuerySysInfo(15, 16, t.as_mut_ptr(), 8) };
    let mut ms = 0u32;
    unsafe { os2::DosQuerySysInfo(14, 14, &mut ms, 4) };
    unsafe {
        if SEED == 0 {
            SEED = t[0] ^ ms ^ 0x5eed;
        }
        let n = UUID_COUNT;
        UUID_COUNT += 1;
        core::ptr::write_bytes(uuid, 0, 16);
        core::ptr::write_unaligned(uuid as *mut u32, t[0].wrapping_add(ms).wrapping_add(n));
        core::ptr::write_unaligned(uuid.add(4) as *mut u32, t[1]);
        *uuid.add(8) = 0x7f;
        *uuid.add(9) = 0;
        for k in [10usize, 12, 14] {
            let r = rand16();
            *uuid.add(k) = (r >> 8) as u8;
            *uuid.add(k + 1) = r as u8;
        }
    }
    0
}

// ---- Threads (variables holding the procedures, as IBM's) ----

/// SOMStartThread(thrd, proc, data, datasz, stacksz).
extern "C" fn start_thread(thrd: *mut *mut u32, proc_: usize, data: usize, _datasz: u32, stacksz: u32) -> u32 {
    let mut tid = 0u32;
    let f: os2::ThreadFn = unsafe { core::mem::transmute(proc_) };
    let stack = if stacksz == 0 { 0x10000 } else { stacksz };
    if unsafe { os2::DosCreateThread(&mut tid, f, data as u32, 2, stack) } != 0 {
        return 1;
    }
    if !thrd.is_null() {
        let p = somsup::malloc(4) as *mut u32;
        unsafe {
            *p = tid;
            *thrd = p;
        }
    }
    0
}

extern "C" fn end_thread() -> u32 {
    unsafe { os2::DosExit(0, 0) };
    0
}

unsafe extern "C" {
    #[link_name = "DOSCALLS.111"]
    fn DosKillThread(tid: u32) -> u32;
}

extern "C" fn kill_thread(thrd: *const u32) -> u32 {
    if thrd.is_null() { 1 } else { unsafe { DosKillThread(*thrd) } }
}

extern "C" fn yield_thread() -> u32 {
    unsafe { os2::DosSleep(0) };
    0
}

extern "C" fn get_thread_handle(thrd: *mut *mut u32) -> u32 {
    if thrd.is_null() {
        return 1;
    }
    let p = somsup::malloc(4) as *mut u32;
    let tid: u32;
    unsafe { core::arch::asm!("mov {0}, fs:[0x0c]", "mov {0}, [{0}]", out(reg) tid) };
    unsafe {
        *p = tid;
        *thrd = p;
    }
    0
}

aligned! {
    __a_start_thread => start_thread, 5;
    __a_end_thread => end_thread, 0;
    __a_kill_thread => kill_thread, 1;
    __a_yield_thread => yield_thread, 0;
    __a_get_thread_handle => get_thread_handle, 1;
}

#[unsafe(no_mangle)]
pub static mut SOMStartThread: extern "C" fn() = __a_start_thread;
#[unsafe(no_mangle)]
pub static mut SOMEndThread: extern "C" fn() = __a_end_thread;
#[unsafe(no_mangle)]
pub static mut SOMKillThread: extern "C" fn() = __a_kill_thread;
#[unsafe(no_mangle)]
pub static mut SOMYieldThread: extern "C" fn() = __a_yield_thread;
#[unsafe(no_mangle)]
pub static mut SOMGetThreadHandle: extern "C" fn() = __a_get_thread_handle;

// ---- The heap watcher ----
//
// somWatchStart puts somwatchMalloc & co. in SOM's allocator variables:
// each block gets a header (its size, a check word, the live list) and a
// trailing check word; somCheckHeap walks the live blocks and reports a
// damaged one (SOM error 9 when the level asks for it).

#[unsafe(no_mangle)]
pub static mut SOM_WatchLevel: i32 = 0;
#[unsafe(no_mangle)]
pub static mut SOM_WatchFrequency: i32 = 0;
#[unsafe(no_mangle)]
pub static mut somwatchTrace: i32 = 0;

const HEAD: usize = 0x1c;
const MAGIC: u32 = 0x5741_5443; // "CTAW"
const TAIL: u32 = 0xfdfd_fdfd;

#[repr(C)]
struct Block {
    size: usize,
    magic: u32,
    next: *mut Block,
    prev: *mut Block,
    serial: u32,
    file: usize,
    line: u32,
}

static mut LIVE: *mut Block = core::ptr::null_mut();
static mut STATS: [u32; 4] = [0; 4]; // allocations, frees, bytes allocated, bytes freed
static mut OLD: [usize; 4] = [0; 4];
static mut WATCHING: bool = false;
static mut CALLS: i32 = 0;

unsafe extern "C" {
    #[link_name = "SOM.12"]
    static mut SOMMalloc: usize;
    #[link_name = "SOM.1"]
    static mut SOMCalloc: usize;
    #[link_name = "SOM.16"]
    static mut SOMRealloc: usize;
    #[link_name = "SOM.10"]
    static mut SOMFree: usize;
}

fn raw_malloc(n: usize) -> *mut u8 {
    let f: extern "C" fn(u32) -> *mut u8 = unsafe { core::mem::transmute(if OLD[0] != 0 { OLD[0] } else { SOMMalloc }) };
    f(n as u32)
}
fn raw_free(p: *mut u8) {
    let f: extern "C" fn(*mut u8) = unsafe { core::mem::transmute(if OLD[3] != 0 { OLD[3] } else { SOMFree }) };
    f(p)
}

fn tick() {
    unsafe {
        CALLS += 1;
        if SOM_WatchFrequency > 0 && CALLS % SOM_WatchFrequency == 0 {
            check_heap();
        }
    }
}

extern "C" fn watch_malloc(n: u32) -> *mut u8 {
    tick();
    let n = n as usize;
    let b = raw_malloc(HEAD + n + 4) as *mut Block;
    if b.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        *b = Block { size: n, magic: MAGIC, next: LIVE, prev: core::ptr::null_mut(), serial: STATS[0], file: 0, line: 0 };
        if !LIVE.is_null() {
            (*LIVE).prev = b;
        }
        LIVE = b;
        let user = (b as *mut u8).add(HEAD);
        core::ptr::write_unaligned(user.add(n) as *mut u32, TAIL);
        STATS[0] += 1;
        STATS[2] += n as u32;
        if somwatchTrace != 0 {
            somsup::printf(b"somwatch: malloc %d at %08lX\n\0", &[n as u32, user as u32]);
        }
        user
    }
}

fn block_of(p: *mut u8) -> *mut Block {
    unsafe { p.sub(HEAD) as *mut Block }
}

fn block_ok(b: *mut Block) -> bool {
    unsafe {
        (*b).magic == MAGIC && core::ptr::read_unaligned((b as *const u8).add(HEAD + (*b).size) as *const u32) == TAIL
    }
}

extern "C" fn watch_free(p: *mut u8) {
    tick();
    if p.is_null() {
        return;
    }
    let b = block_of(p);
    unsafe {
        if (*b).magic != MAGIC {
            somsup::printf(b"somwatch: free of %08lX, not a watched block\n\0", &[p as u32]);
            if SOM_WatchLevel > 1 {
                somsup::error(9, b"somwatch.c\0", 0x240);
            }
            return;
        }
        if !block_ok(b) {
            show_block(p);
            somsup::error(9, b"somwatch.c\0", 0x246);
        }
        if !(*b).prev.is_null() {
            (*(*b).prev).next = (*b).next;
        } else {
            LIVE = (*b).next;
        }
        if !(*b).next.is_null() {
            (*(*b).next).prev = (*b).prev;
        }
        STATS[1] += 1;
        STATS[3] += (*b).size as u32;
        (*b).magic = 0;
        if somwatchTrace != 0 {
            somsup::printf(b"somwatch: free %08lX\n\0", &[p as u32]);
        }
    }
    raw_free(b as *mut u8);
}

extern "C" fn watch_calloc(count: u32, size: u32) -> *mut u8 {
    let n = count.saturating_mul(size);
    let p = watch_malloc(n);
    if !p.is_null() {
        unsafe { core::ptr::write_bytes(p, 0, n as usize) };
    }
    p
}

extern "C" fn watch_realloc(p: *mut u8, n: u32) -> *mut u8 {
    if p.is_null() {
        return watch_malloc(n);
    }
    if n == 0 {
        watch_free(p);
        return core::ptr::null_mut();
    }
    let old = unsafe { (*block_of(p)).size };
    let q = watch_malloc(n);
    if !q.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(p, q, old.min(n as usize)) };
        watch_free(p);
    }
    q
}

fn check_heap() -> bool {
    let mut b = unsafe { LIVE };
    let mut n = 0u32;
    let mut ok = true;
    while !b.is_null() {
        n += 1;
        if !block_ok(b) {
            show_block(unsafe { (b as *mut u8).add(HEAD) });
            ok = false;
            somsup::error(9, b"somwatch.c\0", 0x240);
        }
        b = unsafe { (*b).next };
    }
    unsafe {
        if n != STATS[0] - STATS[1] {
            somsup::printf(b"somwatch: %d blocks in the list, %d allocated\n\0", &[n, STATS[0] - STATS[1]]);
            somsup::error(9, b"somwatch.c\0", 0x231);
            ok = false;
        }
    }
    ok
}

fn show_block(p: *mut u8) {
    let b = block_of(p);
    unsafe {
        somsup::printf(b"somwatch: block %08lX (header %08lX): size %d, serial %d%s\n\0", &[
            p as u32,
            b as u32,
            (*b).size as u32,
            (*b).serial,
            if block_ok(b) { b"\0".as_ptr() } else { b", damaged\0".as_ptr() } as u32,
        ]);
    }
}

/// somWatchStart(frequency, level) (_Optlink).
pub extern "C" fn somWatchStart(freq: i32, level: i32) {
    unsafe {
        SOM_WatchLevel = if (1..=2).contains(&level) { level } else { 2 };
        SOM_WatchFrequency = freq;
        if WATCHING {
            return;
        }
        WATCHING = true;
        OLD = [SOMMalloc, SOMCalloc, SOMRealloc, SOMFree];
        SOMMalloc = __a_somwatchMalloc as usize;
        SOMCalloc = __a_somwatchCalloc as usize;
        SOMRealloc = __a_somwatchRealloc as usize;
        SOMFree = __a_somwatchFree as usize;
    }
}

/// somWatchInit(argc, argv) (_Optlink): the -somwatch option ("as", "fc",
/// "nsn": how often to check, check level, trace) or the SOMWATCH
/// environment variable, then somWatchStart.
pub extern "C" fn somWatchInit(argc: i32, argv: *const *const u8) {
    let mut opt: Option<&[u8]> = None;
    let mut v: *const u8 = core::ptr::null();
    if unsafe { os2::DosScanEnv(b"SOMWATCH\0".as_ptr(), &mut v) } == 0 && !v.is_null() {
        opt = Some(unsafe { somsup::cstr(v) });
    }
    for i in 1..argc.max(0) as usize {
        let a = unsafe { somsup::cstr(*argv.add(i)) };
        if a == b"-somwatch" && i + 1 < argc as usize {
            opt = Some(unsafe { somsup::cstr(*argv.add(i + 1)) });
        }
    }
    let (mut freq, mut level) = (20, 1);
    if let Some(o) = opt {
        match o.first().map(|c| c.to_ascii_lowercase()) {
            Some(b'a') => freq = 1,
            Some(b'n') => freq = 0,
            _ => freq = 20,
        }
        if o.get(1).map(|c| c.to_ascii_lowercase()) == Some(b'c') {
            level = 2;
        }
        match o.get(2).map(|c| c.to_ascii_lowercase()) {
            Some(b'n') => unsafe { somwatchTrace = 1 },
            Some(b'b') => unsafe { somwatchTrace = 2 },
            _ => {}
        }
    }
    somWatchStart(freq, level);
}

/// somWatchGetStats(stats): live blocks, live bytes, allocations, bytes
/// allocated, frees, bytes freed.
extern "C" fn watch_get_stats(out: *mut u32) {
    unsafe {
        let s = [STATS[0] - STATS[1], STATS[2] - STATS[3], STATS[0], STATS[2], STATS[1], STATS[3]];
        core::ptr::copy_nonoverlapping(s.as_ptr(), out, 6);
    }
}

extern "C" fn check_heap_api() -> u32 {
    check_heap() as u32
}

extern "C" fn check_heap_all() -> u32 {
    unsafe {
        let l = SOM_WatchLevel;
        SOM_WatchLevel = 2;
        let r = check_heap();
        SOM_WatchLevel = l;
        r as u32
    }
}

extern "C" fn show_heap() {
    unsafe {
        somsup::printf(b"somwatch: %d allocations (%d bytes), %d frees (%d bytes)\n\0", &[STATS[0], STATS[2], STATS[1], STATS[3]]);
        let mut b = LIVE;
        while !b.is_null() {
            show_block((b as *mut u8).add(HEAD));
            b = (*b).next;
        }
    }
    check_heap();
}

/// somCheckMemBlock(p) (_Optlink): TRUE when the block is intact (or
/// nothing is watched).
pub extern "C" fn somCheckMemBlock(p: *mut u8) -> u32 {
    if unsafe { !WATCHING } || p.is_null() {
        return 1;
    }
    let b = block_of(p);
    (unsafe { (*b).magic } == MAGIC && block_ok(b)) as u32
}

pub extern "C" fn somShowMemBlock(p: *mut u8) {
    if !p.is_null() && unsafe { WATCHING } {
        show_block(p);
    }
}

aligned! {
    __a_somwatchMalloc => watch_malloc, 1;
    __a_somwatchCalloc => watch_calloc, 2;
    __a_somwatchRealloc => watch_realloc, 2;
    __a_somwatchFree => watch_free, 1;
    __a_somWatchGetStats => watch_get_stats, 1;
    __a_somCheckHeap => check_heap_api, 0;
    __a_somCheckHeapAll => check_heap_all, 0;
    __a_somShowHeap => show_heap, 0;
}

optlink! {
    __o_somUUIDToString => somUUIDToString, 2;
    __o_somStringToUUID => somStringToUUID, 2;
    __o_somCompareUUIDStrings => somCompareUUIDStrings, 2;
    __o_somCreateUUID => somCreateUUID, 1;
    __o_somWatchStart => somWatchStart, 2;
    __o_somWatchInit => somWatchInit, 2;
    __o_somCheckMemBlock => somCheckMemBlock, 1;
    __o_somShowMemBlock => somShowMemBlock, 1;
}
