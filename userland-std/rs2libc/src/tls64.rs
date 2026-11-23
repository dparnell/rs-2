//! Thread-local storage in 64-bit programs and DLLs (docs/STD64.md).
//!
//! Code reaches thread-local variables in two ways:
//!
//! - in a program, through FS (`mov %fs:offset`, x86-64's "variant II"):
//!   the program's block ends at the thread pointer, where the thread
//!   pointer itself is stored. OS/2 keeps FS for the TIB, which 64-bit code
//!   does not use through FS, so each thread gets an LDT data selector
//!   (RS2API 421) whose base is its thread pointer. A descriptor's base has
//!   32 bits: the block is below 4 GB. The 64-bit PMWIN puts the TIB's
//!   selector back around the 32-bit code it calls, and the kernel keeps FS
//!   for exception handlers.
//! - in a DLL, through `__tls_get_addr`, with the address of the DLL's
//!   [`Module`] (the loader's R_X86_64_DTPMOD64) and the variable's offset.
//!   A thread's block for a DLL is made when it is first asked for.
//!
//! A program's thread pointer has the blocks of the DLLs loaded with it
//! below its own (the kernel's loader lays them out), as the program may
//! use their variables through FS too.
//!
//! rs2libc's own per-thread state, the [`Tcb`], is found through FS when
//! rs2libc is linked into the program. Linked into a DLL it must leave FS
//! alone (the program may have storage of its own there) and finds the TCB
//! in a dword of OS/2's thread-local memory.

use core::ffi::{c_int, c_void};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::sys::{DosAllocThreadLocalMemory, Rs2Win16Selector};

pub const KEYS: usize = 128;

/// A destructor registered by `__cxa_thread_atexit_impl`.
pub struct Dtor {
    pub f: unsafe extern "C" fn(*mut c_void),
    pub arg: *mut c_void,
    pub next: *mut Dtor,
}

/// `RS2_MODULE`, which link64.ld puts in every module: addresses relative
/// to itself.
#[repr(C)]
pub struct Module {
    tdata: isize,
    tdata_size: usize,
    tls_size: usize,
    tls_align: usize,
    text: isize,
    text_end: isize,
    eh_frame_hdr: isize,
    init: isize,
    init_end: isize,
    /// Its place in [`Tcb::dtv`], 0 until a variable is used.
    index: AtomicUsize,
    /// A program's: the DLLs loaded with it that have storage, and their
    /// blocks' distances below the thread pointer (the kernel's loader).
    dlls: [(*const Module, usize); 32],
}

impl Module {
    fn at(&self, rel: isize) -> usize {
        (self as *const Module as usize).wrapping_add_signed(rel)
    }
    pub fn text(&self) -> core::ops::Range<usize> {
        self.at(self.text)..self.at(self.text_end)
    }
    pub fn eh_frame_hdr(&self) -> usize {
        self.at(self.eh_frame_hdr)
    }
    pub fn init_array(&self) -> core::ops::Range<usize> {
        self.at(self.init)..self.at(self.init_end)
    }
    fn align(&self) -> usize {
        self.tls_align.max(1)
    }
    /// The variables' initial values at `block`.
    unsafe fn fill(&self, block: *mut u8) {
        unsafe {
            core::ptr::copy_nonoverlapping(self.at(self.tdata) as *const u8, block, self.tdata_size);
            core::ptr::write_bytes(block.add(self.tdata_size), 0, self.tls_size - self.tdata_size);
        }
    }
}

/// What is at the thread pointer (FS:0).
#[repr(C)]
struct Head {
    self_ptr: *mut Head,
    tcb: *mut Tcb,
}

pub struct Tcb {
    pub errno: c_int,
    pub thread: *mut crate::thread::Thread,
    /// FS's LDT selector (0: none) and the allocation behind it.
    selector: u32,
    fs_block: *mut u8,
    fs_end: *mut u8,
    /// Event semaphore this thread waits on in futex waits (0 until used).
    pub sem: u32,
    pub keys: [*mut c_void; KEYS],
    pub dtors: *mut Dtor,
    /// The thread's blocks for DLLs, by [`Module::index`].
    dtv: *mut *mut u8,
    dtv_len: usize,
}

/// The program's module, when it is this rs2libc that serves it.
static PROGRAM: AtomicPtr<Module> = AtomicPtr::new(null_mut());
/// In a DLL: the dword of thread-local memory with the thread's TCB.
static SLOT: AtomicPtr<u32> = AtomicPtr::new(null_mut());

/// The TIB's selector (gdt::TIB_SEL): FS without a thread pointer.
const TIB: u32 = 0x150b;

fn fs() -> u32 {
    let v: u32;
    unsafe { core::arch::asm!("mov {0:e}, fs", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v & 0xffff
}

fn existing() -> *mut Tcb {
    let slot = SLOT.load(Ordering::Relaxed);
    if !slot.is_null() {
        return unsafe { slot.read_volatile() as usize as *mut Tcb };
    }
    if fs() == TIB {
        return null_mut();
    }
    let p: *mut Tcb;
    unsafe { core::arch::asm!("mov {0}, fs:[8]", out(reg) p, options(nostack, readonly, preserves_flags)) };
    p
}

/// The current thread's control block.
#[inline]
pub fn tcb() -> *mut Tcb {
    let t = existing();
    if t.is_null() { unsafe { setup(null_mut()) } } else { t }
}

/// rs2libc is in a DLL (before anything uses the TCB).
pub unsafe fn in_dll() {
    let mut p: *mut u32 = null_mut();
    if unsafe { DosAllocThreadLocalMemory(1, &mut p) } != 0 {
        crate::start::fatal("rs2libc: no thread-local memory\r\n");
    }
    SLOT.store(p, Ordering::Relaxed);
}

/// This rs2libc serves the program, whose module is `m`.
pub unsafe fn set_program(m: *mut Module) {
    PROGRAM.store(m, Ordering::Release);
}

/// Create the calling thread's control block if it has none yet, and its
/// storage behind FS if there is a program to serve.
pub unsafe fn setup(thread: *mut crate::thread::Thread) -> *mut Tcb {
    unsafe {
        let mut t = existing();
        if t.is_null() {
            t = crate::malloc::low_memalign(16, core::mem::size_of::<Tcb>()) as *mut Tcb;
            if t.is_null() {
                crate::start::fatal("rs2libc: no memory for thread-local storage\r\n");
            }
            core::ptr::write_bytes(t, 0, 1);
            let slot = SLOT.load(Ordering::Relaxed);
            if !slot.is_null() {
                slot.write_volatile(t as usize as u32);
            }
        }
        if !thread.is_null() {
            (*t).thread = thread;
        }
        let m = PROGRAM.load(Ordering::Acquire);
        if !m.is_null() && (*t).selector == 0 {
            setup_fs(t, &*m);
        } else if (*t).selector != 0 && fs() == TIB {
            // Made in a DLL's initialisation, whose FS does not last.
            core::arch::asm!("mov fs, {0:e}", in(reg) (*t).selector, options(nostack, preserves_flags));
        }
        t
    }
}

unsafe fn setup_fs(t: *mut Tcb, m: &Module) {
    // The thread pointer is the block's start plus its size rounded up to
    // its alignment (as the linker computed the offsets).
    let off = (m.tls_size + m.align() - 1) & !(m.align() - 1);
    let align = m.align().max(16);
    // The blocks of DLLs loaded with the program are below its own.
    let dlls = || m.dlls.iter().take_while(|d| !d.0.is_null()).map(|d| (unsafe { &*d.0 }, d.1));
    let tp = dlls().fold(off, |o, d| o.max(d.1)).next_multiple_of(16);
    let align = dlls().fold(align, |a, d| a.max(d.0.align()));
    let tp = tp.next_multiple_of(align);
    unsafe {
        let block = crate::malloc::low_memalign(align, tp + core::mem::size_of::<Head>());
        if block.is_null() {
            crate::start::fatal("rs2libc: no memory for thread-local storage\r\n");
        }
        m.fill(block.add(tp - off));
        for (d, below) in dlls() {
            let b = block.add(tp - below);
            d.fill(b);
            *dtv_slot(t, d) = b;
        }
        (*t).fs_end = block.add(tp);
        let head = block.add(tp) as *mut Head;
        *head = Head { self_ptr: head, tcb: t };
        let sel = Rs2Win16Selector(0, 0, 1);
        if sel == 0 || Rs2Win16Selector(3, sel, 0xffff_ffff) == 0 || Rs2Win16Selector(2, sel, head as usize as u32) == 0 {
            crate::start::fatal("rs2libc: no selector for thread-local storage\r\n");
        }
        (*t).selector = sel;
        (*t).fs_block = block;
        core::arch::asm!("mov fs, {0:e}", in(reg) sel, options(nostack, preserves_flags));
    }
}

/// Free the calling thread's storage (it must not be used afterwards).
pub unsafe fn teardown() {
    unsafe {
        let t = existing();
        if t.is_null() {
            return;
        }
        if (*t).sem != 0 {
            crate::sys::DosCloseEventSem((*t).sem);
        }
        let slot = SLOT.load(Ordering::Relaxed);
        if !slot.is_null() {
            slot.write_volatile(0);
        }
        if (*t).selector != 0 {
            core::arch::asm!("mov fs, {0:e}", in(reg) TIB, options(nostack, preserves_flags));
            Rs2Win16Selector(4, (*t).selector, 0);
            crate::malloc::low_free((*t).fs_block);
        }
        for i in 0..(*t).dtv_len {
            let b = *(*t).dtv.add(i);
            if !b.is_null() && !((*t).fs_block..(*t).fs_end).contains(&b) {
                crate::malloc::free_raw(b);
            }
        }
        if !(*t).dtv.is_null() {
            crate::malloc::free_raw((*t).dtv as *mut u8);
        }
        crate::malloc::low_free(t as *mut u8);
    }
}

// ---- Variables of DLLs ----

/// A variable as the linker describes it (two GOT entries).
#[repr(C)]
pub struct TlsIndex {
    module: *const Module,
    offset: usize,
}

static NEXT: AtomicUsize = AtomicUsize::new(1);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __tls_get_addr(ti: *const TlsIndex) -> *mut c_void {
    unsafe {
        let (m, off) = (&*(*ti).module, (*ti).offset);
        let t = tcb();
        let i = m.index.load(Ordering::Acquire);
        if i != 0 && i < (*t).dtv_len {
            let b = *(*t).dtv.add(i);
            if !b.is_null() {
                return b.add(off) as *mut c_void;
            }
        }
        first_use(t, m).add(off) as *mut c_void
    }
}

/// The entry for `m` in the thread's dtv.
unsafe fn dtv_slot(t: *mut Tcb, m: &Module) -> *mut *mut u8 {
    let mut i = m.index.load(Ordering::Acquire);
    if i == 0 {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        i = m.index.compare_exchange(0, n, Ordering::AcqRel, Ordering::Acquire).map_or_else(|x| x, |_| n);
    }
    unsafe {
        if i >= (*t).dtv_len {
            let len = (i + 8).next_power_of_two();
            let v = crate::malloc::memalign_raw(16, len * 8) as *mut *mut u8;
            if v.is_null() {
                crate::start::fatal("rs2libc: no memory for thread-local storage\r\n");
            }
            core::ptr::write_bytes(v, 0, len);
            if !(*t).dtv.is_null() {
                core::ptr::copy_nonoverlapping((*t).dtv, v, (*t).dtv_len);
                crate::malloc::free_raw((*t).dtv as *mut u8);
            }
            (*t).dtv = v;
            (*t).dtv_len = len;
        }
        (*t).dtv.add(i)
    }
}

#[cold]
unsafe fn first_use(t: *mut Tcb, m: &Module) -> *mut u8 {
    unsafe {
        let slot = dtv_slot(t, m);
        let b = crate::malloc::memalign_raw(m.align().max(16), m.tls_size.max(1));
        if b.is_null() {
            crate::start::fatal("rs2libc: no memory for thread-local storage\r\n");
        }
        m.fill(b);
        *slot = b;
        b
    }
}
