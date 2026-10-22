//! Low-level support: the SOM lock, memory (SOM's replaceable allocator
//! and executable memory for thunks), calls with argument lists, and the
//! stack-aligning trampolines Rust code needs when OS/2 code calls it.

use core::alloc::Layout;
use core::sync::atomic::{AtomicU32, Ordering};

use os2::*;

/// Stack-aligning entry points: OS/2 compilers keep 4-byte stack
/// alignment, Rust code assumes 16. The arguments (`_System`: the caller
/// removes them) are copied to an aligned frame.
#[macro_export]
macro_rules! aligned {
    ($($tramp:ident => $target:path, $copy:literal;)*) => {$(
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $tramp() {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "and esp, -16",
                "sub esp, {pad}",
                "push esi", "push edi", "push ecx",
                "lea esi, [ebp + 8]", "lea edi, [esp + 12]", "mov ecx, {n}", "rep movsd",
                "pop ecx", "pop edi", "pop esi",
                "call {f}", "mov esp, ebp", "pop ebp", "ret",
                pad = const ($copy * 4 + 15) / 16 * 16, n = const $copy, f = sym $target,
            );
        }
    )*};
}

// ---- The SOM lock (recursive: OS/2 mutex semaphores count requests) ----

static SEM: AtomicU32 = AtomicU32::new(0);

fn sem() -> u32 {
    let h = SEM.load(Ordering::Acquire);
    if h != 0 {
        return h;
    }
    let mut new = 0;
    unsafe { DosCreateMutexSem(core::ptr::null(), &mut new, 0, 0) };
    match SEM.compare_exchange(0, new, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => new,
        Err(other) => {
            unsafe { DosCloseMutexSem(new) };
            other
        }
    }
}

pub struct Guard;

pub fn lock() -> Guard {
    unsafe { DosRequestMutexSem(sem(), u32::MAX) };
    Guard
}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe { DosReleaseMutexSem(sem()) };
    }
}

// ---- Memory ----

const HDR: usize = 8;

/// The default SOMMalloc: the process heap, with the size kept in front.
pub extern "C" fn som_malloc(n: u32) -> *mut u8 {
    let size = (n as usize).max(1) + HDR;
    let Ok(l) = Layout::from_size_align(size, 8) else { return core::ptr::null_mut() };
    let p = unsafe { alloc::alloc::alloc(l) };
    if p.is_null() {
        return p;
    }
    unsafe {
        *(p as *mut usize) = size;
        p.add(HDR)
    }
}

pub extern "C" fn som_calloc(count: u32, size: u32) -> *mut u8 {
    let n = count.saturating_mul(size);
    let p = som_malloc(n);
    if !p.is_null() {
        unsafe { core::ptr::write_bytes(p, 0, n as usize) };
    }
    p
}

pub extern "C" fn som_free(p: *mut u8) {
    if p.is_null() {
        return;
    }
    unsafe {
        let base = p.sub(HDR);
        let size = *(base as *const usize);
        alloc::alloc::dealloc(base, Layout::from_size_align_unchecked(size, 8));
    }
}

pub extern "C" fn som_realloc(p: *mut u8, n: u32) -> *mut u8 {
    if p.is_null() {
        return som_malloc(n);
    }
    unsafe {
        let base = p.sub(HDR);
        let old = *(base as *const usize) - HDR;
        let q = som_malloc(n);
        if !q.is_null() {
            core::ptr::copy_nonoverlapping(p, q, old.min(n as usize));
            som_free(p);
        }
        q
    }
}

/// Executable memory for thunks (never freed: classes live as long as
/// the process).
static mut EXEC_AT: usize = 0;
static mut EXEC_END: usize = 0;

pub fn exec_alloc(n: usize) -> *mut u8 {
    let n = (n + 15) & !15;
    let _g = lock();
    unsafe {
        if EXEC_AT + n > EXEC_END {
            let mut p: *mut u8 = core::ptr::null_mut();
            let size = 0x10000;
            if DosAllocMem(&mut p, size, PAG_READ | PAG_WRITE | PAG_EXECUTE | PAG_COMMIT) != 0 {
                return core::ptr::null_mut();
            }
            EXEC_AT = p as usize;
            EXEC_END = EXEC_AT + size as usize;
        }
        let p = EXEC_AT as *mut u8;
        EXEC_AT += n;
        p
    }
}

pub const PAG_READ: u32 = 1;
pub const PAG_WRITE: u32 = 2;
pub const PAG_EXECUTE: u32 = 4;
pub const PAG_COMMIT: u32 = 0x10;

/// Leak a value for the life of the process.
pub fn leak<T>(v: T) -> *mut T {
    alloc::boxed::Box::into_raw(alloc::boxed::Box::new(v))
}

/// Leak a vector's contents (its buffer stays allocated).
pub fn leak_vec<T>(v: alloc::vec::Vec<T>) -> *mut T {
    alloc::boxed::Box::leak(v.into_boxed_slice()).as_mut_ptr()
}

// ---- Calls with argument lists ----

/// Call `f` with `n` 32-bit arguments copied from `args`; the result is
/// edx:eax (or st0 when `f` returns a floating point value).
#[unsafe(naked)]
pub extern "C" fn call_args(f: usize, args: *const u32, n: u32) -> u64 {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "push esi", "push edi",
        "mov ecx, [ebp + 16]", "mov esi, [ebp + 12]",
        "lea eax, [ecx * 4]", "sub esp, eax", "and esp, -16",
        "mov edi, esp", "cld", "rep movsd",
        "call [ebp + 8]",
        "lea esp, [ebp - 8]", "pop edi", "pop esi", "pop ebp", "ret",
    );
}

pub fn call_args_f64(f: usize, args: *const u32, n: u32) -> f64 {
    let g: extern "C" fn(usize, *const u32, u32) -> f64 = unsafe { core::mem::transmute(call_args as usize) };
    g(f, args, n)
}

/// The thread's id (TIB2).
pub extern "C" fn thread_id() -> u32 {
    let mut tib: *mut Tib = core::ptr::null_mut();
    let mut pib: *mut Pib = core::ptr::null_mut();
    unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        if tib.is_null() || (*tib).ptib2.is_null() {
            return 1;
        }
        (*(*tib).ptib2).tid
    }
}

pub extern "C" fn create_mutex(sem: *mut u32) -> u32 {
    unsafe { DosCreateMutexSem(core::ptr::null(), sem, 0, 0) }
}
pub extern "C" fn request_mutex(sem: u32) -> u32 {
    unsafe { DosRequestMutexSem(sem, u32::MAX) }
}
pub extern "C" fn release_mutex(sem: u32) -> u32 {
    unsafe { DosReleaseMutexSem(sem) }
}
pub extern "C" fn destroy_mutex(sem: u32) -> u32 {
    unsafe { DosCloseMutexSem(sem) }
}

/// Length of a C string.
pub unsafe fn strlen(p: *const u8) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    n
}

pub unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(p, strlen(p)) }
}

/// Case-insensitive comparison of ASCII names.
pub fn name_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}
