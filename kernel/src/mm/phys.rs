//! Physical page frame allocator (one bit per 4 KB frame).

use bootinfo::{BootInfo, MemoryKind, PHYS_MAP_BASE};

use crate::sync::SpinLock;

pub const PAGE: u64 = 4096;

struct Frames {
    /// Bit set = frame in use.
    bitmap: &'static mut [u64],
    frames: u64,
    free: u64,
    /// Search hint: first word that may contain a free bit.
    hint: usize,
}

static FRAMES: SpinLock<Option<Frames>> = SpinLock::new(None);

pub fn phys_to_virt(p: u64) -> *mut u8 {
    (PHYS_MAP_BASE + p) as *mut u8
}

pub fn virt_to_phys(v: *const u8) -> u64 {
    v as u64 - PHYS_MAP_BASE
}

/// A usable page below 640 KB, kept for the AP start-up trampoline.
static LOW_PAGE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn low_page() -> Option<u64> {
    match LOW_PAGE.load(core::sync::atomic::Ordering::Relaxed) {
        0 => None,
        p => Some(p),
    }
}

pub fn init(info: &BootInfo) {
    let top = info
        .memory()
        .iter()
        .filter(|r| r.kind == MemoryKind::Usable)
        .map(|r| r.start + r.len)
        .max()
        .unwrap_or(0);
    let frames = top / PAGE;
    let words = frames.div_ceil(64) as usize;
    let bytes = (words * 8) as u64;
    // Put the bitmap in the first usable region big enough (above 1 MB).
    let region = info
        .memory()
        .iter()
        .find(|r| r.kind == MemoryKind::Usable && r.start >= 0x10_0000 && r.len >= bytes + PAGE)
        .expect("no memory for frame bitmap");
    let bitmap_phys = region.start;
    let bitmap =
        unsafe { core::slice::from_raw_parts_mut(phys_to_virt(bitmap_phys) as *mut u64, words) };
    bitmap.fill(!0);
    let mut f = Frames { bitmap, frames, free: 0, hint: 0 };
    for r in info.memory().iter().filter(|r| r.kind == MemoryKind::Usable) {
        let start = r.start.div_ceil(PAGE);
        let end = (r.start + r.len) / PAGE;
        for fr in start..end {
            f.clear(fr);
        }
    }
    // Keep the real-mode area (trampolines for AP startup) and the bitmap;
    // remember a usable page there for the AP trampoline.
    for fr in 0..(0x10_0000 / PAGE) {
        if fr > 0 && fr < 0x9f && !f.is_set(fr) && LOW_PAGE.load(core::sync::atomic::Ordering::Relaxed) == 0 {
            LOW_PAGE.store(fr * PAGE, core::sync::atomic::Ordering::Relaxed);
        }
        f.set(fr);
    }
    for fr in bitmap_phys / PAGE..(bitmap_phys + bytes).div_ceil(PAGE) {
        f.set(fr);
    }
    crate::println!(
        "memory: {} MB usable of {} MB",
        f.free * PAGE >> 20,
        top >> 20
    );
    *FRAMES.lock() = Some(f);
}

impl Frames {
    fn is_set(&self, fr: u64) -> bool {
        self.bitmap[(fr / 64) as usize] & (1 << (fr % 64)) != 0
    }

    fn set(&mut self, fr: u64) {
        let (w, b) = ((fr / 64) as usize, fr % 64);
        if self.bitmap[w] & (1 << b) == 0 {
            self.bitmap[w] |= 1 << b;
            self.free -= 1;
        }
    }

    fn clear(&mut self, fr: u64) {
        let (w, b) = ((fr / 64) as usize, fr % 64);
        if self.bitmap[w] & (1 << b) != 0 {
            self.bitmap[w] &= !(1 << b);
            self.free += 1;
            self.hint = self.hint.min(w);
        }
    }

    fn alloc(&mut self) -> Option<u64> {
        for w in self.hint..self.bitmap.len() {
            let v = self.bitmap[w];
            if v != !0 {
                let b = (!v).trailing_zeros() as u64;
                let fr = w as u64 * 64 + b;
                if fr >= self.frames {
                    return None;
                }
                self.set(fr);
                self.hint = w;
                return Some(fr * PAGE);
            }
        }
        None
    }

    /// Allocate `n` contiguous frames below `limit` aligned to `align` frames.
    fn alloc_contig(&mut self, n: u64, align: u64, limit: u64) -> Option<u64> {
        let max = (limit / PAGE).min(self.frames);
        let mut fr = 256; // skip the first MB
        while fr + n <= max {
            let base = fr.next_multiple_of(align);
            if base + n > max {
                break;
            }
            match (base..base + n).find(|&x| self.bitmap[(x / 64) as usize] & (1 << (x % 64)) != 0) {
                Some(used) => fr = used + 1,
                None => {
                    for x in base..base + n {
                        self.set(x);
                    }
                    return Some(base * PAGE);
                }
            }
        }
        None
    }
}

/// Allocate one zeroed frame.
pub fn alloc() -> Option<u64> {
    let p = FRAMES.lock().as_mut()?.alloc()?;
    unsafe { core::ptr::write_bytes(phys_to_virt(p), 0, PAGE as usize) };
    Some(p)
}

/// Frames kept back from user memory so the kernel (heap, page tables,
/// DMA buffers) can still allocate when programs have used up the rest.
const USER_RESERVE: u64 = 1024;

/// Allocate one zeroed frame for user memory: fails (the program sees an
/// exception or ERROR_NOT_ENOUGH_MEMORY) once only the kernel's reserve is
/// left.
pub fn alloc_user() -> Option<u64> {
    let p = {
        let mut g = FRAMES.lock();
        let f = g.as_mut()?;
        if f.free <= USER_RESERVE {
            return None;
        }
        f.alloc()?
    };
    unsafe { core::ptr::write_bytes(phys_to_virt(p), 0, PAGE as usize) };
    Some(p)
}

/// Allocate `n` zeroed, physically contiguous frames below `limit`.
pub fn alloc_contig(n: u64, limit: u64) -> Option<u64> {
    let p = FRAMES.lock().as_mut()?.alloc_contig(n, 1, limit)?;
    unsafe { core::ptr::write_bytes(phys_to_virt(p), 0, (n * PAGE) as usize) };
    Some(p)
}

pub fn free(p: u64) {
    if let Some(f) = FRAMES.lock().as_mut() {
        f.clear(p / PAGE);
    }
}

pub fn free_count() -> u64 {
    FRAMES.lock().as_ref().map_or(0, |f| f.free)
}

pub fn total_frames() -> u64 {
    FRAMES.lock().as_ref().map_or(0, |f| f.frames)
}
