//! Memory: VirtualAlloc and friends on DosAllocMem/DosSetMem/DosQueryMem
//! (and RS2API 434 for a reservation at a given address), and the heaps.
//!
//! Heaps are kept simple: every block comes from the DLL's allocator
//! (os2's heap) with a 16-byte header naming its heap and size; a heap
//! remembers its blocks so that HeapDestroy can free them. GlobalAlloc and
//! LocalAlloc are the process heap's blocks (handles are pointers).

use alloc::vec::Vec;
use core::alloc::Layout;

use os2::{PAG_BASE, PAG_COMMIT, PAG_DECOMMIT, PAG_EXECUTE, PAG_FREE, PAG_GUARD, PAG_READ, PAG_WRITE};

use crate::{err, fail, Global, FALSE, TRUE};

pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_DECOMMIT: u32 = 0x4000;
pub const MEM_RELEASE: u32 = 0x8000;
pub const MEM_FREE: u32 = 0x10000;
pub const MEM_PRIVATE: u32 = 0x20000;
pub const MEM_IMAGE: u32 = 0x100_0000;
pub const MEM_TOP_DOWN: u32 = 0x10_0000;

pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_WRITECOPY: u32 = 0x08;
pub const PAGE_EXECUTE: u32 = 0x10;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;
pub const PAGE_GUARD: u32 = 0x100;

/// Windows page protection to OS/2's PAG_ flags. OS/2 has no "no access"
/// for allocated memory: PAGE_NOACCESS is readable.
fn os2_prot(p: u32) -> u32 {
    let mut f = match p & 0xff {
        PAGE_READONLY => PAG_READ,
        PAGE_READWRITE | PAGE_WRITECOPY => PAG_READ | PAG_WRITE,
        PAGE_EXECUTE | PAGE_EXECUTE_READ => PAG_READ | PAG_EXECUTE,
        PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY => PAG_READ | PAG_WRITE | PAG_EXECUTE,
        _ => PAG_READ,
    };
    if p & PAGE_GUARD != 0 {
        f |= PAG_GUARD;
    }
    f
}

fn win_prot(f: u32) -> u32 {
    let p = match (f & PAG_WRITE != 0, f & PAG_EXECUTE != 0, f & PAG_READ != 0) {
        (true, true, _) => PAGE_EXECUTE_READWRITE,
        (true, false, _) => PAGE_READWRITE,
        (false, true, _) => PAGE_EXECUTE_READ,
        (false, false, true) => PAGE_READONLY,
        _ => PAGE_NOACCESS,
    };
    if f & PAG_GUARD != 0 { p | PAGE_GUARD } else { p }
}

fn query(a: u32) -> Option<(u32, u32)> {
    let (mut size, mut flags) = (u32::MAX, 0u32);
    let rc = unsafe { os2::DosQueryMem(a as *mut u8, &mut size, &mut flags) };
    (rc == 0).then_some((size, flags))
}

/// The base of the allocation `a` is in: down page by page to PAG_BASE.
fn allocation_base(a: u32) -> Option<u32> {
    let mut p = a & !0xfff;
    for _ in 0..0x10_0000 {
        let (_, f) = query(p)?;
        if f & PAG_FREE != 0 {
            return None;
        }
        if f & PAG_BASE != 0 {
            return Some(p);
        }
        p = p.checked_sub(0x1000)?;
    }
    None
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn VirtualAlloc(addr: u32, size: u32, ty: u32, prot: u32) -> u32 {
    if size == 0 {
        return fail(err::INVALID_PARAMETER, 0);
    }
    let flags = os2_prot(prot);
    if ty & MEM_RESERVE != 0 || addr == 0 {
        let commit = if ty & MEM_COMMIT != 0 { PAG_COMMIT } else { 0 };
        let a = if addr == 0 {
            let mut p: *mut u8 = core::ptr::null_mut();
            // Large reservations may go above 512 MB.
            let any = if size >= 0x100_0000 { os2::OBJ_ANY } else { 0 };
            let mut rc = unsafe { os2::DosAllocMem(&mut p, size, flags | commit | any) };
            if rc != 0 && any != 0 {
                rc = unsafe { os2::DosAllocMem(&mut p, size, flags | commit) };
            }
            if rc != 0 {
                return fail(err::NOT_ENOUGH_MEMORY, 0);
            }
            p as u32
        } else {
            let base = addr & !0xffff;
            let len = (addr + size - base).next_multiple_of(0x1000);
            let rc = unsafe { crate::Rs2W32AllocAt(base, len, flags | commit) };
            if rc != 0 {
                return fail(err::INVALID_ADDRESS, 0);
            }
            base
        };
        return a;
    }
    // Commit pages of a reservation.
    let start = addr & !0xfff;
    let end = (addr + size).next_multiple_of(0x1000);
    let mut p = start;
    while p < end {
        let Some((n, f)) = query(p) else { return fail(err::INVALID_ADDRESS, 0) };
        if f & PAG_FREE != 0 {
            return fail(err::INVALID_ADDRESS, 0);
        }
        let n = n.min(end - p).max(0x1000);
        let rc = if f & PAG_COMMIT != 0 {
            unsafe { os2::DosSetMem(p as *mut u8, n, flags) }
        } else {
            unsafe { os2::DosSetMem(p as *mut u8, n, flags | PAG_COMMIT) }
        };
        if rc != 0 {
            return fail(err::INVALID_ADDRESS, 0);
        }
        p += n;
    }
    start
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn VirtualFree(addr: u32, size: u32, ty: u32) -> i32 {
    if ty & MEM_RELEASE != 0 {
        if size != 0 {
            return fail(err::INVALID_PARAMETER, FALSE);
        }
        if unsafe { os2::DosFreeMem(addr as *mut u8) } != 0 {
            return fail(err::INVALID_ADDRESS, FALSE);
        }
        return TRUE;
    }
    if ty & MEM_DECOMMIT != 0 {
        let start = addr & !0xfff;
        let len = if size == 0 {
            let Some(b) = allocation_base(addr) else { return fail(err::INVALID_ADDRESS, FALSE) };
            let Some((n, _)) = query(b) else { return fail(err::INVALID_ADDRESS, FALSE) };
            b + n - start
        } else {
            (addr + size).next_multiple_of(0x1000) - start
        };
        let mut p = start;
        while p < start + len {
            let Some((n, f)) = query(p) else { break };
            let n = n.min(start + len - p);
            if f & PAG_COMMIT != 0 {
                unsafe { os2::DosSetMem(p as *mut u8, n, PAG_DECOMMIT) };
            }
            p += n.max(0x1000);
        }
        return TRUE;
    }
    fail(err::INVALID_PARAMETER, FALSE)
}

/// MEMORY_BASIC_INFORMATION.
#[repr(C)]
pub struct MemoryBasicInformation {
    pub base: u32,
    pub allocation_base: u32,
    pub allocation_protect: u32,
    pub region_size: u32,
    pub state: u32,
    pub protect: u32,
    pub ty: u32,
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn VirtualQuery(addr: u32, info: *mut MemoryBasicInformation, len: u32) -> u32 {
    if (len as usize) < core::mem::size_of::<MemoryBasicInformation>() {
        return fail(err::INSUFFICIENT_BUFFER, 0);
    }
    let page = addr & !0xfff;
    let Some((size, flags)) = query(page) else { return fail(err::INVALID_PARAMETER, 0) };
    let m = unsafe { &mut *info };
    m.base = page;
    m.region_size = size;
    if flags & PAG_FREE != 0 {
        m.allocation_base = 0;
        m.allocation_protect = 0;
        m.state = MEM_FREE;
        m.protect = PAGE_NOACCESS;
        m.ty = 0;
    } else {
        let base = allocation_base(page).unwrap_or(page);
        m.allocation_base = base;
        m.allocation_protect = query(base).map_or(PAGE_READWRITE, |(_, f)| win_prot(f));
        m.state = if flags & PAG_COMMIT != 0 { MEM_COMMIT } else { MEM_RESERVE };
        m.protect = if flags & PAG_COMMIT != 0 { win_prot(flags) } else { 0 };
        // An image: the loader's region with PE headers (read only if
        // its first page is there).
        let readable = query(base).is_some_and(|(_, f)| f & PAG_COMMIT != 0 && f & PAG_READ != 0);
        m.ty = if readable && unsafe { crate::pe::optional(base) }.is_some() { MEM_IMAGE } else { MEM_PRIVATE };
    }
    core::mem::size_of::<MemoryBasicInformation>() as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn VirtualProtect(addr: u32, size: u32, prot: u32, old: *mut u32) -> i32 {
    let start = addr & !0xfff;
    let end = (addr + size.max(1)).next_multiple_of(0x1000);
    let Some((_, f)) = query(start) else { return fail(err::INVALID_ADDRESS, FALSE) };
    if !old.is_null() {
        unsafe { *old = win_prot(f) };
    }
    if unsafe { os2::DosSetMem(start as *mut u8, end - start, os2_prot(prot)) } != 0 {
        return fail(err::INVALID_ADDRESS, FALSE);
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn VirtualLock(_a: u32, _n: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn VirtualUnlock(_a: u32, _n: u32) -> i32 {
    TRUE
}

/// Is [a, a + n) committed memory with `need` access?
fn accessible(a: u32, n: u32, need: u32) -> bool {
    if n == 0 {
        return true;
    }
    if a == 0 {
        return false;
    }
    let mut p = a & !0xfff;
    let end = a.saturating_add(n);
    while p < end {
        let Some((size, f)) = query(p) else { return false };
        if f & PAG_FREE != 0 || f & PAG_COMMIT == 0 || f & need != need {
            return false;
        }
        p = p.saturating_add(size.max(0x1000));
    }
    true
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsBadReadPtr(a: u32, n: u32) -> i32 {
    (!accessible(a, n, PAG_READ)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsBadWritePtr(a: u32, n: u32) -> i32 {
    (!accessible(a, n, PAG_WRITE)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsBadCodePtr(a: u32) -> i32 {
    (!accessible(a, 1, PAG_READ)) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn IsBadStringPtrA(a: u32, max: u32) -> i32 {
    let mut p = a;
    let mut n = 0;
    while n < max {
        if !accessible(p, 1, PAG_READ) {
            return TRUE;
        }
        if unsafe { *(p as *const u8) } == 0 {
            return FALSE;
        }
        p += 1;
        n += 1;
    }
    FALSE
}

// ---- Heaps ----

const HEAP_NO_SERIALIZE: u32 = 0x01;
const HEAP_GENERATE_EXCEPTIONS: u32 = 0x04;
const HEAP_ZERO_MEMORY: u32 = 0x08;
const HEAP_REALLOC_IN_PLACE_ONLY: u32 = 0x10;
const HDR: usize = 16;
const HEAP_MAGIC: u32 = 0x5048_3257; // "W2HP"

/// A heap: its blocks (for HeapDestroy). Heap handles are 1-based
/// indexes with a tag, the process heap is the first.
static HEAPS: Global<Vec<Option<Vec<u32>>>> = Global::new(Vec::new());

fn heap_handle(i: usize) -> u32 {
    0x4850_0000 | (i as u32 + 1)
}

fn heap_index(h: u32) -> Option<usize> {
    (h & 0xffff_0000 == 0x4850_0000 && h & 0xffff != 0).then(|| (h & 0xffff) as usize - 1)
}

fn layout(n: usize) -> Layout {
    Layout::from_size_align(n + HDR, 16).unwrap()
}

/// Allocate a block of heap `i`.
fn heap_alloc(i: usize, n: usize, zero: bool) -> u32 {
    let lay = layout(n);
    let p = unsafe { if zero { alloc::alloc::alloc_zeroed(lay) } else { alloc::alloc::alloc(lay) } };
    if p.is_null() {
        return 0;
    }
    unsafe {
        (p as *mut u32).write(HEAP_MAGIC);
        (p as *mut u32).add(1).write(i as u32);
        (p as *mut u32).add(2).write(n as u32);
    }
    let user = p as u32 + HDR as u32;
    HEAPS.with(|hs| {
        if let Some(Some(b)) = hs.get_mut(i) {
            b.push(user);
        }
    });
    user
}

/// A block's (heap, size), if `p` is one.
fn block(p: u32) -> Option<(usize, usize)> {
    if p < HDR as u32 {
        return None;
    }
    let h = (p - HDR as u32) as *const u32;
    unsafe { (h.read() == HEAP_MAGIC).then(|| (h.add(1).read() as usize, h.add(2).read() as usize)) }
}

fn heap_free(p: u32) -> bool {
    let Some((i, n)) = block(p) else { return false };
    HEAPS.with(|hs| {
        if let Some(Some(b)) = hs.get_mut(i) {
            if let Some(k) = b.iter().rposition(|&x| x == p) {
                b.swap_remove(k);
            }
        }
    });
    unsafe {
        ((p - HDR as u32) as *mut u32).write(0);
        alloc::alloc::dealloc((p - HDR as u32) as *mut u8, layout(n));
    }
    true
}

static PROCESS_HEAP: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetProcessHeap() -> u32 {
    let h = PROCESS_HEAP.load(core::sync::atomic::Ordering::Acquire);
    if h != 0 {
        return h;
    }
    let h = HeapCreate(0, 0, 0);
    match PROCESS_HEAP.compare_exchange(0, h, core::sync::atomic::Ordering::AcqRel, core::sync::atomic::Ordering::Acquire) {
        Ok(_) => h,
        Err(x) => x,
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GetProcessHeaps(n: u32, out: *mut u32) -> u32 {
    if n >= 1 && !out.is_null() {
        unsafe { *out = GetProcessHeap() };
    }
    1
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapCreate(_flags: u32, _initial: u32, _max: u32) -> u32 {
    HEAPS.with(|hs| {
        let i = match hs.iter().position(|x| x.is_none()) {
            Some(i) => {
                hs[i] = Some(Vec::new());
                i
            }
            None => {
                hs.push(Some(Vec::new()));
                hs.len() - 1
            }
        };
        heap_handle(i)
    })
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapDestroy(h: u32) -> i32 {
    let Some(i) = heap_index(h) else { return fail(err::INVALID_HANDLE, FALSE) };
    if h == PROCESS_HEAP.load(core::sync::atomic::Ordering::Acquire) {
        return fail(err::INVALID_PARAMETER, FALSE);
    }
    let blocks = HEAPS.with(|hs| hs.get_mut(i).and_then(|x| x.take()));
    let Some(blocks) = blocks else { return fail(err::INVALID_HANDLE, FALSE) };
    for p in blocks {
        if let Some((_, n)) = block(p) {
            unsafe {
                ((p - HDR as u32) as *mut u32).write(0);
                alloc::alloc::dealloc((p - HDR as u32) as *mut u8, layout(n));
            }
        }
    }
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapAlloc(h: u32, flags: u32, n: u32) -> u32 {
    let Some(i) = heap_index(h) else { return fail(err::INVALID_HANDLE, 0) };
    let p = heap_alloc(i, n as usize, flags & HEAP_ZERO_MEMORY != 0);
    if p == 0 {
        let _ = HEAP_GENERATE_EXCEPTIONS;
        return fail(err::NOT_ENOUGH_MEMORY, 0);
    }
    p
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapFree(_h: u32, _flags: u32, p: u32) -> i32 {
    if p == 0 {
        return TRUE;
    }
    if heap_free(p) { TRUE } else { fail(err::INVALID_PARAMETER, FALSE) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapReAlloc(h: u32, flags: u32, p: u32, n: u32) -> u32 {
    let Some((_, old)) = block(p) else { return fail(err::INVALID_PARAMETER, 0) };
    if flags & HEAP_REALLOC_IN_PLACE_ONLY != 0 {
        return if n as usize <= old { p } else { fail(err::NOT_ENOUGH_MEMORY, 0) };
    }
    let q = HeapAlloc(h, flags, n);
    if q == 0 {
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(p as *const u8, q as *mut u8, old.min(n as usize)) };
    heap_free(p);
    q
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapSize(_h: u32, _flags: u32, p: u32) -> u32 {
    match block(p) {
        Some((_, n)) => n as u32,
        None => fail(err::INVALID_PARAMETER, 0xffff_ffff),
    }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapValidate(_h: u32, _flags: u32, p: u32) -> i32 {
    (p == 0 || block(p).is_some()) as i32
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapCompact(_h: u32, _flags: u32) -> u32 {
    0x10_0000
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapLock(_h: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn HeapUnlock(_h: u32) -> i32 {
    TRUE
}

// GlobalAlloc / LocalAlloc: GMEM_ZEROINIT (0x40) = LMEM_ZEROINIT.

#[unsafe(no_mangle)]
pub extern "stdcall" fn GlobalAlloc(flags: u32, n: u32) -> u32 {
    HeapAlloc(GetProcessHeap(), if flags & 0x40 != 0 { HEAP_ZERO_MEMORY } else { 0 }, n.max(1))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GlobalReAlloc(h: u32, n: u32, flags: u32) -> u32 {
    // GMEM_MODIFY: only the flags change.
    if flags & 0x80 != 0 {
        return h;
    }
    HeapReAlloc(GetProcessHeap(), if flags & 0x40 != 0 { HEAP_ZERO_MEMORY } else { 0 }, h, n.max(1))
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GlobalFree(h: u32) -> u32 {
    if h == 0 || heap_free(h) { 0 } else { fail(err::INVALID_HANDLE, h) }
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GlobalLock(h: u32) -> u32 {
    h
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GlobalUnlock(_h: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GlobalHandle(p: u32) -> u32 {
    p
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GlobalSize(h: u32) -> u32 {
    block(h).map_or(0, |(_, n)| n as u32)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn GlobalFlags(_h: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LocalAlloc(flags: u32, n: u32) -> u32 {
    GlobalAlloc(flags, n)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LocalReAlloc(h: u32, n: u32, flags: u32) -> u32 {
    GlobalReAlloc(h, n, flags)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LocalFree(h: u32) -> u32 {
    GlobalFree(h)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LocalLock(h: u32) -> u32 {
    h
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LocalUnlock(_h: u32) -> i32 {
    TRUE
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LocalSize(h: u32) -> u32 {
    GlobalSize(h)
}

#[unsafe(no_mangle)]
pub extern "stdcall" fn LocalHandle(p: u32) -> u32 {
    p
}

/// MEMORYSTATUS.
#[unsafe(no_mangle)]
pub unsafe extern "stdcall" fn GlobalMemoryStatus(m: *mut u32) {
    let mut v = [0u32; 3];
    // QSV_TOTPHYSMEM 17, QSV_TOTRESMEM 18, QSV_TOTAVAILMEM 19.
    unsafe { os2::DosQuerySysInfo(17, 19, v.as_mut_ptr(), 12) };
    let total = v[0];
    let avail = v[2].min(total);
    unsafe {
        *m = 32;
        *m.add(1) = if total != 0 { 100 - (avail as u64 * 100 / total as u64) as u32 } else { 0 };
        *m.add(2) = total;
        *m.add(3) = avail;
        *m.add(4) = total;
        *m.add(5) = avail;
        *m.add(6) = 0x7ffe_0000;
        *m.add(7) = 0x6000_0000;
    }
    let _ = (HEAP_NO_SERIALIZE, PAG_COMMIT);
}
