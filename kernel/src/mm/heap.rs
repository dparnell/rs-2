//! Kernel heap: an address-ordered free list with coalescing over a region
//! that grows on demand.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use super::paging::{self, NO_EXEC, WRITABLE, GLOBAL};
use super::phys;
use crate::sync::SpinLock;

pub const HEAP_BASE: u64 = 0xffff_c000_0000_0000;
pub const HEAP_MAX: u64 = 64 << 30;

#[repr(C)]
struct Free {
    size: usize,
    next: *mut Free,
}

struct Heap {
    head: *mut Free,
    top: u64,
}

unsafe impl Send for Heap {}

static HEAP: SpinLock<Heap> = SpinLock::new(Heap { head: ptr::null_mut(), top: HEAP_BASE });

const MIN: usize = core::mem::size_of::<Free>();

impl Heap {
    /// Insert a free block, keeping address order and merging neighbours.
    unsafe fn insert(&mut self, addr: usize, size: usize) {
        unsafe {
            let mut prev: *mut Free = ptr::null_mut();
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

    fn grow(&mut self, bytes: usize) -> bool {
        let pages = bytes.div_ceil(4096).max(64);
        let start = self.top;
        for i in 0..pages as u64 {
            if self.top >= HEAP_BASE + HEAP_MAX {
                return false;
            }
            let Some(f) = phys::alloc() else { return false };
            if !paging::map(paging::kernel_root(), self.top, f, WRITABLE | NO_EXEC | GLOBAL) {
                return false;
            }
            self.top += 4096;
            let _ = i;
        }
        unsafe { self.insert(start as usize, (self.top - start) as usize) };
        true
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(MIN).next_multiple_of(MIN);
        let align = layout.align().max(MIN);
        loop {
            unsafe {
                let mut prev: *mut Free = ptr::null_mut();
                let mut cur = self.head;
                while !cur.is_null() {
                    let start = cur as usize;
                    let end = start + (*cur).size;
                    let aligned = start.next_multiple_of(align);
                    // A leading gap must be big enough to stay a free block.
                    let aligned = if aligned != start && aligned - start < MIN {
                        (start + MIN).next_multiple_of(align)
                    } else {
                        aligned
                    };
                    if aligned + size <= end {
                        let next = (*cur).next;
                        if prev.is_null() {
                            self.head = next;
                        } else {
                            (*prev).next = next;
                        }
                        if aligned > start {
                            self.insert(start, aligned - start);
                        }
                        let tail = end - (aligned + size);
                        if tail >= MIN {
                            self.insert(aligned + size, tail);
                        } else if tail > 0 {
                            // Too small to track: hand it out with the block.
                            return self.finish(aligned, size + tail);
                        }
                        return self.finish(aligned, size);
                    }
                    prev = cur;
                    cur = (*cur).next;
                }
            }
            if !self.grow(size + align) {
                return ptr::null_mut();
            }
        }
    }

    fn finish(&mut self, addr: usize, _size: usize) -> *mut u8 {
        addr as *mut u8
    }
}

pub struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { HEAP.lock().alloc(layout) }
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        let size = layout.size().max(MIN).next_multiple_of(MIN);
        // Blocks that absorbed a tiny tail are freed without it; the few
        // bytes are recovered when neighbours coalesce.
        unsafe { HEAP.lock().insert(p as usize, size) };
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;
