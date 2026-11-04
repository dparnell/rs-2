//! ELF thread-local storage on rs2.
//!
//! std (built for i686 Linux) reaches its `thread_local!`s through the
//! thread pointer in GS's base, using the i386 "variant II" layout: the
//! TLS block (a copy of `.tdata` followed by zeroed `.tbss`) ends at the
//! thread pointer, `%gs:0` holds the thread pointer itself, and variables
//! are at negative offsets from it. The linker relaxes every access in the
//! static executable to that local-exec form.
//!
//! OS/2 keeps FS for the TIB and leaves GS alone, so each thread gets an
//! LDT data selector of its own (an alias selector from RS2API 421, base =
//! its thread pointer, limit 4 GB so that negative offsets wrap) loaded
//! into GS. The kernel saves and restores GS with the thread's other
//! segment registers. rs2libc's per-thread state (errno, pthread keys,
//! the futex semaphore) is in the control block at the thread pointer.

use core::ffi::{c_int, c_void};
use core::ptr::null_mut;

use crate::sys::Rs2Win16Selector;

pub const KEYS: usize = 128;

/// A destructor registered by `__cxa_thread_atexit_impl`.
pub struct Dtor {
    pub f: unsafe extern "C" fn(*mut c_void),
    pub arg: *mut c_void,
    pub next: *mut Dtor,
}

/// The thread control block at the thread pointer.
#[repr(C)]
pub struct Tcb {
    /// `%gs:0`: the thread pointer (the TCB's own address).
    pub self_ptr: *mut Tcb,
    pub errno: c_int,
    pub thread: *mut crate::thread::Thread,
    /// GS's LDT selector.
    pub selector: u32,
    /// The allocation holding the TLS block and this TCB.
    pub block: *mut u8,
    /// Event semaphore this thread waits on in futex waits (0 until used).
    pub sem: u32,
    pub keys: [*mut c_void; KEYS],
    pub dtors: *mut Dtor,
}

unsafe extern "C" {
    static __tdata_start: u8;
    static __tdata_end: u8;
    static __tbss_end: u8;
    static __tls_align: u8;
}

fn tls_layout() -> (usize, usize, usize) {
    let start = &raw const __tdata_start as usize;
    let filesz = &raw const __tdata_end as usize - start;
    let memsz = &raw const __tbss_end as usize - start;
    let align = (&raw const __tls_align as usize).max(1);
    (filesz, memsz, align)
}

/// The current thread's control block.
#[inline]
pub fn tcb() -> *mut Tcb {
    let gs: u32;
    unsafe { core::arch::asm!("mov {0:e}, gs", out(reg) gs, options(nomem, nostack, preserves_flags)) };
    if gs & 0xfff8 == 0 {
        // A thread rs2libc did not start (a DLL's): give it TLS now.
        return unsafe { setup(null_mut()) };
    }
    let p: *mut Tcb;
    unsafe { core::arch::asm!("mov {0}, gs:[0]", out(reg) p, options(nostack, readonly, preserves_flags)) };
    p
}

/// Create the calling thread's TLS block and control block and point GS at
/// them.
pub unsafe fn setup(thread: *mut crate::thread::Thread) -> *mut Tcb {
    let (filesz, memsz, align) = tls_layout();
    // The thread pointer is the block's start plus memsz rounded up to the
    // TLS segment's alignment (as the linker computed the offsets).
    let off = (memsz + align - 1) & !(align - 1);
    let align = align.max(16);
    let total = off + core::mem::size_of::<Tcb>();
    unsafe {
        let block = crate::malloc::memalign_raw(align, total);
        if block.is_null() {
            crate::start::fatal("rs2libc: no memory for thread-local storage\r\n");
        }
        core::ptr::copy_nonoverlapping(&raw const __tdata_start, block, filesz);
        core::ptr::write_bytes(block.add(filesz), 0, total - filesz);
        let tcb = block.add(off) as *mut Tcb;
        (*tcb).self_ptr = tcb;
        (*tcb).block = block;
        (*tcb).thread = thread;
        let sel = Rs2Win16Selector(0, 0, 1);
        if sel == 0
            || Rs2Win16Selector(3, sel, 0xffff_ffff) == 0
            || Rs2Win16Selector(2, sel, tcb as u32) == 0
        {
            crate::start::fatal("rs2libc: no selector for thread-local storage\r\n");
        }
        (*tcb).selector = sel;
        core::arch::asm!("mov gs, {0:e}", in(reg) sel, options(nostack, preserves_flags));
        tcb
    }
}

/// Free the calling thread's TLS (it must not be used afterwards).
pub unsafe fn teardown() {
    unsafe {
        let t = tcb();
        let sel = (*t).selector;
        let block = (*t).block;
        if (*t).sem != 0 {
            crate::sys::DosCloseEventSem((*t).sem);
        }
        core::arch::asm!("mov gs, {0:e}", in(reg) 0u32, options(nostack, preserves_flags));
        Rs2Win16Selector(4, sel, 0);
        crate::malloc::free_raw(block);
    }
}
