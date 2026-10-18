//! Uncached mappings for device registers.

use super::paging::{self, NO_CACHE, NO_EXEC, WRITABLE, WRITE_THROUGH, GLOBAL};
use crate::sync::SpinLock;

const MMIO_BASE: u64 = 0xffff_d000_0000_0000;

static NEXT: SpinLock<u64> = SpinLock::new(MMIO_BASE);

/// Map `size` bytes of physical device memory uncached; returns the
/// virtual address of `phys`.
pub fn map(phys: u64, size: u64) -> *mut u8 {
    let start = phys & !0xfff;
    let end = (phys + size + 0xfff) & !0xfff;
    let mut next = NEXT.lock();
    let va = *next;
    for (i, pa) in (start..end).step_by(4096).enumerate() {
        paging::map(
            paging::kernel_root(),
            va + i as u64 * 4096,
            pa,
            WRITABLE | NO_CACHE | WRITE_THROUGH | NO_EXEC | GLOBAL,
        );
    }
    *next = va + (end - start) + 4096; // guard gap
    (va + (phys - start)) as *mut u8
}
