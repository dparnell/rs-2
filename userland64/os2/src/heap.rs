//! The program heap: an address-ordered free list over memory obtained with
//! DosAllocMem. Large requests get their own memory object.
//!
//! Memory comes from below 4 GB while there is room there (the 32-bit
//! PMWIN can take pointers to it), then from the 64-bit arena above 4 GB
//! (DosAllocMem with OBJ_ANY, docs/API64.md).

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, null_mut};
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::dos::*;

const CHUNK: usize = 1 << 20;
const LARGE: usize = 256 * 1024;
const MIN: usize = 16;

#[repr(C)]
struct Free {
    size: usize,
    next: *mut Free,
}

struct Heap {
    head: *mut Free,
}

static LOCK: AtomicBool = AtomicBool::new(false);
static mut HEAP: Heap = Heap { head: null_mut() };

fn lock() {
    while LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        unsafe { DosSleep(0) };
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

impl Heap {
    unsafe fn insert(&mut self, addr: usize, size: usize) {
        unsafe {
            let mut prev: *mut Free = null_mut();
            let mut cur = self.head;
            while !cur.is_null() && (cur as usize) < addr {
                prev = cur;
                cur = (*cur).next;
            }
            let node = addr as *mut Free;
            (*node).size = size;
            (*node).next = cur;
            if !cur.is_null() && addr + size == cur as usize {
                (*node).size += (*cur).size;
                (*node).next = (*cur).next;
            }
            if prev.is_null() {
                self.head = node;
            } else if prev as usize + (*prev).size == addr {
                (*prev).size += (*node).size;
                (*prev).next = (*node).next;
            } else {
                (*prev).next = node;
            }
        }
    }

    unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        let size = size.max(MIN).next_multiple_of(MIN);
        let align = align.max(MIN);
        for _ in 0..2 {
            unsafe {
                let mut prev: *mut Free = null_mut();
                let mut cur = self.head;
                while !cur.is_null() {
                    let start = cur as usize;
                    let end = start + (*cur).size;
                    let mut a = start.next_multiple_of(align);
                    if a != start && a - start < MIN {
                        a = (start + MIN).next_multiple_of(align);
                    }
                    if a + size <= end {
                        let next = (*cur).next;
                        if prev.is_null() {
                            self.head = next;
                        } else {
                            (*prev).next = next;
                        }
                        if a > start {
                            self.insert(start, a - start);
                        }
                        if end - (a + size) >= MIN {
                            self.insert(a + size, end - (a + size));
                        }
                        return a as *mut u8;
                    }
                    prev = cur;
                    cur = (*cur).next;
                }
                // Grow by one chunk.
                let p = alloc_mem(CHUNK);
                if p.is_null() {
                    return null_mut();
                }
                self.insert(p as usize, CHUNK);
            }
        }
        null_mut()
    }
}

/// A committed memory object of `size` bytes: below 4 GB if there is
/// room, else above (OBJ_ANY). Null if neither has room (or `size` is
/// beyond a ULONG).
fn alloc_mem(size: usize) -> *mut u8 {
    let Ok(cb) = u32::try_from(size) else { return null_mut() };
    let mut p: *mut u8 = null_mut();
    let fl = PAG_READ | PAG_WRITE | PAG_COMMIT;
    if unsafe { DosAllocMem(&mut p, cb, fl) } == 0 {
        return p;
    }
    if unsafe { DosAllocMem(&mut p, cb, fl | OBJ_ANY) } == 0 { p } else { null_mut() }
}

/// A replacement for the heap (PMWIN keeps all it allocates in the
/// shared PM state).
pub struct Allocator {
    pub alloc: unsafe fn(Layout) -> *mut u8,
    pub dealloc: unsafe fn(*mut u8, Layout),
}

static REPLACEMENT: AtomicPtr<Allocator> = AtomicPtr::new(null_mut());

/// Send every allocation of this module (program or DLL) to `a`; call it
/// before anything is allocated.
pub fn set_allocator(a: &'static Allocator) {
    REPLACEMENT.store(a as *const Allocator as *mut Allocator, Ordering::Release);
}

/// Memory from the process's own heap even when the module's allocations
/// go elsewhere (PMWIN allocates container records, which belong to the
/// application, with it).
///
/// # Safety
/// Free it with [`free_private`] and the same size.
pub unsafe fn alloc_private(size: usize) -> *mut u8 {
    if size >= LARGE {
        return alloc_mem(size);
    }
    lock();
    let p = unsafe { (*ptr::addr_of_mut!(HEAP)).alloc(size, 16) };
    unlock();
    p
}

/// # Safety
/// `p` and `size` as from [`alloc_private`].
pub unsafe fn free_private(p: *mut u8, size: usize) {
    if size >= LARGE {
        unsafe { DosFreeMem(p) };
        return;
    }
    lock();
    unsafe { (*ptr::addr_of_mut!(HEAP)).insert(p as usize, size.max(MIN).next_multiple_of(MIN)) };
    unlock();
}

pub struct Os2Heap;

unsafe impl GlobalAlloc for Os2Heap {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let r = REPLACEMENT.load(Ordering::Acquire);
        if !r.is_null() {
            return unsafe { ((*r).alloc)(l) };
        }
        if l.size() >= LARGE && l.align() <= 4096 {
            return alloc_mem(l.size());
        }
        lock();
        let p = unsafe { (*ptr::addr_of_mut!(HEAP)).alloc(l.size(), l.align()) };
        unlock();
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let r = REPLACEMENT.load(Ordering::Acquire);
        if !r.is_null() {
            return unsafe { ((*r).dealloc)(p, l) };
        }
        if l.size() >= LARGE && l.align() <= 4096 {
            unsafe { DosFreeMem(p) };
            return;
        }
        lock();
        unsafe { (*ptr::addr_of_mut!(HEAP)).insert(p as usize, l.size().max(MIN).next_multiple_of(MIN)) };
        unlock();
    }
}

/// Walk the free list (debugging): false if a node is not where a heap
/// node can be (unaligned, or its size or next pointer broken).
pub fn check() -> bool {
    lock();
    let mut ok = true;
    unsafe {
        let mut cur = (*ptr::addr_of!(HEAP)).head;
        let mut n = 0;
        while !cur.is_null() {
            let a = cur as usize;
            if a % MIN != 0 || (*cur).size % MIN != 0 || (*cur).size == 0 || (*cur).size > 1 << 28 || n > 1_000_000 {
                ok = false;
                break;
            }
            let next = (*cur).next as usize;
            if next != 0 && (next % MIN != 0 || next <= a) {
                ok = false;
                break;
            }
            cur = (*cur).next;
            n += 1;
        }
    }
    unlock();
    ok
}
