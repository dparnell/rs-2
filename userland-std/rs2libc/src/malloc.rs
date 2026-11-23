//! malloc and friends: dlmalloc (the `dlmalloc` crate) taking its memory
//! from DosAllocMem in 1 MB steps (OS/2 memory objects start on 64 KB
//! boundaries, so small steps would waste address space). Large blocks get
//! objects of their own and go back to the system when freed. The memory
//! is asked for with OBJ_ANY: above 512 MB when the system has high
//! memory (VIRTUALADDRESSLIMIT), so programs are not held to the 512 MB
//! of the low arena.
//!
//! In 64-bit programs OBJ_ANY means above 4 GB, where few API calls take
//! pointers (docs/API64.md): the heap fills [`LOW_LIMIT`] below first, so
//! that programs of ordinary size can hand any of their data to any call
//! (PM's, for one), and grows above 4 GB from there. Single blocks of
//! [`BIG`] and more go above 4 GB at once. What must be low (thread-local
//! storage, copies for the API: sys.rs) comes from a heap of its own.

use core::ffi::{c_int, c_void};
use core::ptr::null_mut;

use dlmalloc::{Allocator, Dlmalloc};
use libc::size_t;

use crate::spin::SpinLock;
use crate::sys::*;

#[cfg(target_arch = "x86")]
unsafe fn alloc_heap(p: &mut *mut u8, cb: u32) -> APIRET {
    unsafe { alloc_any(p, cb) }
}

#[cfg(target_arch = "x86_64")]
const LOW_LIMIT: u32 = 96 << 20;
#[cfg(target_arch = "x86_64")]
const BIG: u32 = 16 << 20;
#[cfg(target_arch = "x86_64")]
static LOW_USED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[cfg(target_arch = "x86_64")]
unsafe fn alloc_heap(p: &mut *mut u8, cb: u32) -> APIRET {
    use core::sync::atomic::Ordering::Relaxed;
    const LOW: u32 = PAG_READ | PAG_WRITE | PAG_COMMIT;
    let low = cb < BIG && LOW_USED.load(Relaxed) + cb <= LOW_LIMIT;
    unsafe {
        if low && DosAllocMem(p, cb, LOW) == 0 {
            LOW_USED.fetch_add(cb, Relaxed);
            return 0;
        }
        let rc = DosAllocMem(p, cb, LOW | OBJ_ANY);
        if rc != 0 && !low { DosAllocMem(p, cb, LOW) } else { rc }
    }
}

/// Memory below 4 GB (x86-64): a heap of its own.
#[cfg(target_arch = "x86_64")]
struct LowSystem;

#[cfg(target_arch = "x86_64")]
unsafe impl Allocator for LowSystem {
    fn alloc(&self, size: usize) -> (*mut u8, usize, u32) {
        let size = (size + 0xffff) & !0xffff;
        let mut p = null_mut();
        if size > 1 << 28 || unsafe { DosAllocMem(&mut p, size as u32, PAG_READ | PAG_WRITE | PAG_COMMIT) } != 0 {
            return (null_mut(), 0, 0);
        }
        (p, size, 0)
    }
    fn remap(&self, _ptr: *mut u8, _old: usize, _new: usize, _can_move: bool) -> *mut u8 {
        null_mut()
    }
    fn free_part(&self, _ptr: *mut u8, _old: usize, _new: usize) -> bool {
        false
    }
    fn free(&self, ptr: *mut u8, _size: usize) -> bool {
        unsafe { DosFreeMem(ptr) == 0 }
    }
    fn can_release_part(&self, _flags: u32) -> bool {
        false
    }
    fn allocates_zeros(&self) -> bool {
        true
    }
    fn page_size(&self) -> usize {
        4096
    }
}

#[cfg(target_arch = "x86_64")]
static LOW_HEAP: SpinLock<Dlmalloc<LowSystem>> = SpinLock::new(Dlmalloc::new_with_allocator(LowSystem));

#[cfg(target_arch = "x86_64")]
pub fn low_memalign(align: usize, size: usize) -> *mut u8 {
    unsafe { LOW_HEAP.lock().c_memalign(align, size) }
}
#[cfg(target_arch = "x86_64")]
pub unsafe fn low_free(p: *mut u8) {
    unsafe { LOW_HEAP.lock().c_free(p) }
}
#[cfg(target_arch = "x86")]
pub fn low_memalign(align: usize, size: usize) -> *mut u8 {
    unsafe { memalign_raw(align, size) }
}
#[cfg(target_arch = "x86")]
pub unsafe fn low_free(p: *mut u8) {
    unsafe { free_raw(p) }
}

pub struct Os2System;

unsafe impl Allocator for Os2System {
    fn alloc(&self, size: usize) -> (*mut u8, usize, u32) {
        let size = (size + 0xffff) & !0xffff;
        let Ok(cb) = u32::try_from(size) else { return (null_mut(), 0, 0) };
        let mut p = null_mut();
        let rc = unsafe { alloc_heap(&mut p, cb) };
        if rc != 0 {
            return (null_mut(), 0, 0);
        }
        (p, size, 0)
    }
    fn remap(&self, _ptr: *mut u8, _old: usize, _new: usize, _can_move: bool) -> *mut u8 {
        null_mut()
    }
    fn free_part(&self, _ptr: *mut u8, _old: usize, _new: usize) -> bool {
        false
    }
    fn free(&self, ptr: *mut u8, _size: usize) -> bool {
        unsafe { DosFreeMem(ptr) == 0 }
    }
    fn can_release_part(&self, _flags: u32) -> bool {
        false
    }
    fn allocates_zeros(&self) -> bool {
        true
    }
    fn page_size(&self) -> usize {
        4096
    }
}

static HEAP: SpinLock<Dlmalloc<Os2System>> = SpinLock::new({
    let mut d = Dlmalloc::new_with_allocator(Os2System);
    d.set_granularity(1 << 20);
    d
});

pub unsafe fn memalign_raw(align: usize, size: usize) -> *mut u8 {
    unsafe { HEAP.lock().c_memalign(align, size) }
}

pub unsafe fn free_raw(p: *mut u8) {
    unsafe { HEAP.lock().c_free(p) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn malloc(size: size_t) -> *mut c_void {
    let p = unsafe { HEAP.lock().c_malloc(size) };
    if p.is_null() {
        crate::errno::set_errno(libc::ENOMEM);
    }
    p as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn calloc(n: size_t, size: size_t) -> *mut c_void {
    let Some(total) = n.checked_mul(size) else {
        crate::errno::set_errno(libc::ENOMEM);
        return null_mut();
    };
    let p = unsafe { malloc(total) };
    if !p.is_null() {
        unsafe { core::ptr::write_bytes(p as *mut u8, 0, total) };
    }
    p
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn realloc(p: *mut c_void, size: size_t) -> *mut c_void {
    let r = unsafe { HEAP.lock().c_realloc(p as *mut u8, size) };
    if r.is_null() && size != 0 {
        crate::errno::set_errno(libc::ENOMEM);
    }
    r as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(p: *mut c_void) {
    unsafe { HEAP.lock().c_free(p as *mut u8) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_memalign(out: *mut *mut c_void, align: size_t, size: size_t) -> c_int {
    if !align.is_power_of_two() || align < 4 {
        return libc::EINVAL;
    }
    let p = unsafe { memalign_raw(align, size) };
    if p.is_null() {
        return libc::ENOMEM;
    }
    unsafe { *out = p as *mut c_void };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memalign(align: size_t, size: size_t) -> *mut c_void {
    unsafe { memalign_raw(align.next_power_of_two(), size) as *mut c_void }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aligned_alloc(align: size_t, size: size_t) -> *mut c_void {
    unsafe { memalign(align, size) }
}
