//! The program heap: an address-ordered free list over memory obtained with
//! DosAllocMem. Large requests get their own memory object.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, null_mut};
use core::sync::atomic::{AtomicBool, Ordering};

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
                let mut p: *mut u8 = null_mut();
                if DosAllocMem(&mut p, CHUNK as u32, PAG_READ | PAG_WRITE | PAG_COMMIT) != 0 {
                    return null_mut();
                }
                self.insert(p as usize, CHUNK);
            }
        }
        null_mut()
    }
}

pub struct Os2Heap;

unsafe impl GlobalAlloc for Os2Heap {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if l.size() >= LARGE && l.align() <= 4096 {
            let mut p: *mut u8 = null_mut();
            let rc = unsafe { DosAllocMem(&mut p, l.size() as u32, PAG_READ | PAG_WRITE | PAG_COMMIT) };
            return if rc == 0 { p } else { null_mut() };
        }
        lock();
        let p = unsafe { (*ptr::addr_of_mut!(HEAP)).alloc(l.size(), l.align()) };
        unlock();
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if l.size() >= LARGE && l.align() <= 4096 {
            unsafe { DosFreeMem(p) };
            return;
        }
        lock();
        unsafe { (*ptr::addr_of_mut!(HEAP)).insert(p as usize, l.size().max(MIN).next_multiple_of(MIN)) };
        unlock();
    }
}
