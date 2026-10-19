//! The memory PM state lives in: one named shared memory object, at the
//! same address in every PM process, with a heap that is PMWIN's global
//! allocator. Everything PMWIN allocates (the window tree, queues,
//! presentation spaces, bitmaps) is in it, so any process can read and
//! free it; what belongs to one process is tagged with its pid and freed
//! when the process ends.
//!
//! The heap is only used with the PM lock held (the allocator takes it
//! itself when needed), so it needs no lock of its own.

use core::alloc::Layout;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const HEAP_NAME: &[u8] = b"\\SHAREMEM\\PMWIN\\STATE\0";
/// Address space reserved for the state (committed as it is touched).
const HEAP_SIZE: u32 = 64 << 20;
const MAGIC: u32 = 0x4d50_3253; // "S2PM"
const MIN: usize = 16;

unsafe extern "C" {
    #[link_name = "DOSCALLS.300"]
    fn DosAllocSharedMem(ppb: *mut *mut u8, name: *const u8, cb: u32, flags: u32) -> u32;
    #[link_name = "DOSCALLS.301"]
    fn DosGetNamedSharedMem(ppb: *mut *mut u8, name: *const u8, flags: u32) -> u32;
    #[link_name = "DOSCALLS.312"]
    fn DosGetInfoBlocks(tib: *mut *mut os2::Tib, pib: *mut *mut os2::Pib) -> u32;
    #[link_name = "DOSCALLS.229"]
    fn DosSleep(ms: u32) -> u32;
}

/// Start of the shared object.
#[repr(C)]
pub struct Header {
    magic: u32,
    /// Free list (address-ordered).
    free: *mut Free,
    /// The PM state (`state::State`), once initialised.
    pub state: *mut u8,
    /// Bytes in use (for diagnostics).
    pub used: usize,
}

#[repr(C)]
struct Free {
    size: usize,
    next: *mut Free,
}

static BASE: AtomicU32 = AtomicU32::new(0);
static ATTACHING: AtomicBool = AtomicBool::new(false);
static PID: AtomicU32 = AtomicU32::new(0);

/// The calling process's ID.
pub fn pid() -> u32 {
    let p = PID.load(Ordering::Relaxed);
    if p != 0 {
        return p;
    }
    let mut tib: *mut os2::Tib = null_mut();
    let mut pib: *mut os2::Pib = null_mut();
    let p = unsafe {
        DosGetInfoBlocks(&mut tib, &mut pib);
        (*pib).pid
    };
    PID.store(p, Ordering::Relaxed);
    p
}

/// The calling thread's ID (from its TIB: FS:[0x0C] is the TIB2, whose
/// first field is the thread ID).
pub fn tid() -> u32 {
    let t: u32;
    unsafe {
        core::arch::asm!("mov {t}, fs:[0x0c]", "mov {t}, [{t}]", t = out(reg) t, options(nostack, readonly, preserves_flags));
    }
    t
}

/// This process has mapped the shared state.
pub fn attached() -> bool {
    BASE.load(Ordering::Acquire) != 0
}

pub fn header() -> *mut Header {
    let b = BASE.load(Ordering::Acquire);
    if b != 0 {
        return b as *mut Header;
    }
    attach();
    BASE.load(Ordering::Acquire) as *mut Header
}

/// Map the shared object (creating it if this is the first PM process);
/// called with the PM lock held.
fn attach() {
    // Threads of this process race only among themselves here (the lock
    // serialises processes).
    while ATTACHING.swap(true, Ordering::Acquire) {
        unsafe { DosSleep(0) };
    }
    if BASE.load(Ordering::Acquire) == 0 {
        let mut p: *mut u8 = null_mut();
        let rw = os2::PAG_READ | os2::PAG_WRITE;
        if unsafe { DosGetNamedSharedMem(&mut p, HEAP_NAME.as_ptr(), rw) } != 0 {
            if unsafe { DosAllocSharedMem(&mut p, HEAP_NAME.as_ptr(), HEAP_SIZE, rw | os2::PAG_COMMIT) } != 0 {
                panic!("PMWIN: no memory for the shared state");
            }
        }
        let h = p as *mut Header;
        unsafe {
            if (*h).magic != MAGIC {
                // A new object: one free block after the header.
                let first = (p as usize + core::mem::size_of::<Header>()).next_multiple_of(64);
                let f = first as *mut Free;
                (*f).size = p as usize + HEAP_SIZE as usize - first;
                (*f).next = null_mut();
                (*h).free = f;
                (*h).state = null_mut();
                (*h).used = 0;
                (*h).magic = MAGIC;
            }
        }
        BASE.store(p as u32, Ordering::Release);
    }
    ATTACHING.store(false, Ordering::Release);
}

unsafe fn insert(h: *mut Header, addr: usize, size: usize) {
    unsafe {
        let mut prev: *mut Free = null_mut();
        let mut cur = (*h).free;
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
            (*h).free = node;
        } else if prev as usize + (*prev).size == addr {
            (*prev).size += (*node).size;
            (*prev).next = (*node).next;
        } else {
            (*prev).next = node;
        }
    }
}

unsafe fn alloc_in(h: *mut Header, size: usize, align: usize) -> *mut u8 {
    let size = size.max(MIN).next_multiple_of(MIN);
    let align = align.max(MIN);
    unsafe {
        let mut prev: *mut Free = null_mut();
        let mut cur = (*h).free;
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
                    (*h).free = next;
                } else {
                    (*prev).next = next;
                }
                if a > start {
                    insert(h, start, a - start);
                }
                if end - (a + size) >= MIN {
                    insert(h, a + size, end - (a + size));
                }
                (*h).used += size;
                return a as *mut u8;
            }
            prev = cur;
            cur = (*cur).next;
        }
    }
    null_mut()
}

unsafe fn shared_alloc(l: Layout) -> *mut u8 {
    crate::lock::enter_quiet();
    let h = header();
    let p = unsafe { alloc_in(h, l.size(), l.align()) };
    crate::lock::leave_quiet();
    p
}

unsafe fn shared_dealloc(p: *mut u8, l: Layout) {
    crate::lock::enter_quiet();
    let h = header();
    let size = l.size().max(MIN).next_multiple_of(MIN);
    unsafe {
        (*h).used -= size;
        insert(h, p as usize, size);
    }
    crate::lock::leave_quiet();
}

static SHARED_HEAP: os2::heap::Allocator = os2::heap::Allocator { alloc: shared_alloc, dealloc: shared_dealloc };

/// PMWIN's heap is the shared one (from the DLL's initialisation on).
pub fn use_shared_heap() {
    os2::heap::set_allocator(&SHARED_HEAP);
}
