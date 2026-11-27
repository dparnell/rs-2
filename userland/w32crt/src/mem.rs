//! malloc and friends: blocks of the DLL's heap (os2's allocator) with a
//! 16-byte header holding their size, 16-byte aligned.

use core::alloc::Layout;

const HDR: usize = 16;

fn layout(n: usize) -> Layout {
    Layout::from_size_align(n + HDR, 16).unwrap()
}

pub fn heap_alloc(n: usize) -> *mut u8 {
    let p = unsafe { alloc::alloc::alloc(layout(n.max(1))) };
    if p.is_null() {
        crate::set_errno(crate::ENOMEM);
        return p;
    }
    unsafe {
        (p as *mut usize).write(n.max(1));
        p.add(HDR)
    }
}

pub fn size_of(p: *const u8) -> usize {
    unsafe { (p.sub(HDR) as *const usize).read() }
}

pub fn heap_free(p: *mut u8) {
    if p.is_null() {
        return;
    }
    let n = size_of(p);
    unsafe { alloc::alloc::dealloc(p.sub(HDR), layout(n)) };
}

#[unsafe(no_mangle)]
pub extern "C" fn malloc(n: usize) -> *mut u8 {
    heap_alloc(n)
}

#[unsafe(no_mangle)]
pub extern "C" fn free(p: *mut u8) {
    heap_free(p)
}

#[unsafe(no_mangle)]
pub extern "C" fn calloc(n: usize, size: usize) -> *mut u8 {
    let Some(total) = n.checked_mul(size) else {
        crate::set_errno(crate::ENOMEM);
        return core::ptr::null_mut();
    };
    let p = malloc(total);
    if !p.is_null() {
        unsafe { core::ptr::write_bytes(p, 0, total) };
    }
    p
}

#[unsafe(no_mangle)]
pub extern "C" fn realloc(p: *mut u8, n: usize) -> *mut u8 {
    if p.is_null() {
        return malloc(n);
    }
    if n == 0 {
        free(p);
        return core::ptr::null_mut();
    }
    let old = size_of(p);
    let q = malloc(n);
    if !q.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(p, q, old.min(n)) };
        free(p);
    }
    q
}

#[unsafe(no_mangle)]
pub extern "C" fn _msize(p: *mut u8) -> usize {
    if p.is_null() { usize::MAX } else { size_of(p) }
}

#[unsafe(no_mangle)]
pub extern "C" fn _expand(p: *mut u8, n: usize) -> *mut u8 {
    if !p.is_null() && n <= size_of(p) { p } else { core::ptr::null_mut() }
}

#[unsafe(no_mangle)]
pub extern "C" fn _recalloc(p: *mut u8, n: usize, size: usize) -> *mut u8 {
    let old = if p.is_null() { 0 } else { size_of(p) };
    let total = n.saturating_mul(size);
    let q = realloc(p, total);
    if !q.is_null() && total > old {
        unsafe { core::ptr::write_bytes(q.add(old), 0, total - old) };
    }
    q
}

/// _aligned_malloc: the block's start, then the pointer handed out at the
/// alignment, with the start in the word before it.
#[unsafe(no_mangle)]
pub extern "C" fn _aligned_malloc(n: usize, align: usize) -> *mut u8 {
    let align = align.max(4).next_power_of_two();
    let raw = malloc(n + align + 4);
    if raw.is_null() {
        return raw;
    }
    let a = ((raw as usize + 4 + align - 1) & !(align - 1)) as *mut u8;
    unsafe { (a.sub(4) as *mut usize).write(raw as usize) };
    a
}

#[unsafe(no_mangle)]
pub extern "C" fn _aligned_free(p: *mut u8) {
    if !p.is_null() {
        free(unsafe { (p.sub(4) as *const usize).read() } as *mut u8);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _aligned_realloc(p: *mut u8, n: usize, align: usize) -> *mut u8 {
    if p.is_null() {
        return _aligned_malloc(n, align);
    }
    let raw = unsafe { (p.sub(4) as *const usize).read() } as *mut u8;
    let old = size_of(raw) - (p as usize - raw as usize);
    let q = _aligned_malloc(n, align);
    if !q.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(p, q, old.min(n)) };
        _aligned_free(p);
    }
    q
}

static NEW_MODE: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn _set_new_mode(m: i32) -> i32 {
    NEW_MODE.swap(m, core::sync::atomic::Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn _query_new_mode() -> i32 {
    NEW_MODE.load(core::sync::atomic::Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn _heapchk() -> i32 {
    -2 // _HEAPOK
}

#[unsafe(no_mangle)]
pub extern "C" fn _heapmin() -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn _get_heap_handle() -> u32 {
    0
}
