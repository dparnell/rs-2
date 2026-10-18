//! 4-level page tables.

use super::phys::{self, phys_to_virt};
use crate::arch;

pub const PRESENT: u64 = 1 << 0;
pub const WRITABLE: u64 = 1 << 1;
pub const USER: u64 = 1 << 2;
pub const WRITE_THROUGH: u64 = 1 << 3;
pub const NO_CACHE: u64 = 1 << 4;
pub const ACCESSED: u64 = 1 << 5;
pub const DIRTY: u64 = 1 << 6;
pub const HUGE: u64 = 1 << 7;
pub const GLOBAL: u64 = 1 << 8;
/// Software bits (ignored by the CPU).
pub const SW_COW: u64 = 1 << 9;
pub const SW_SHARED: u64 = 1 << 10;
pub const NO_EXEC: u64 = 1 << 63;

pub const ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;

fn table(phys: u64) -> &'static mut [u64; 512] {
    unsafe { &mut *(phys_to_virt(phys) as *mut [u64; 512]) }
}

fn idx(va: u64, level: u32) -> usize {
    ((va >> (12 + 9 * level)) & 511) as usize
}

/// Walk to the level-1 entry for `va`, creating tables if `create`.
/// Intermediate entries get `inter` flags (USER for user mappings).
fn walk(root: u64, va: u64, create: bool, inter: u64) -> Option<&'static mut u64> {
    let mut t = root;
    for level in (1..4).rev() {
        let e = &mut table(t)[idx(va, level)];
        if *e & PRESENT == 0 {
            if !create {
                return None;
            }
            let p = phys::alloc()?;
            *e = p | PRESENT | WRITABLE | inter;
        } else {
            if *e & HUGE != 0 {
                return None;
            }
            *e |= inter;
        }
        t = *e & ADDR_MASK;
    }
    Some(&mut table(t)[idx(va, 0)])
}

/// Map one 4 KB page. Returns false if out of memory.
pub fn map(root: u64, va: u64, pa: u64, flags: u64) -> bool {
    let inter = flags & USER;
    match walk(root, va, true, inter) {
        Some(e) => {
            *e = (pa & ADDR_MASK) | flags | PRESENT;
            if root == current_root() {
                arch::invlpg(va);
            }
            true
        }
        None => false,
    }
}

/// Remove a mapping; returns the old entry.
pub fn unmap(root: u64, va: u64) -> Option<u64> {
    let e = walk(root, va, false, 0)?;
    let old = *e;
    *e = 0;
    if root == current_root() {
        arch::invlpg(va);
    }
    (old & PRESENT != 0).then_some(old)
}

/// The level-1 entry for `va`, if its tables exist.
pub fn entry(root: u64, va: u64) -> Option<u64> {
    walk(root, va, false, 0).map(|e| *e)
}

/// Update flags of an existing mapping.
pub fn set_entry(root: u64, va: u64, value: u64) -> bool {
    match walk(root, va, false, 0) {
        Some(e) => {
            *e = value;
            if root == current_root() {
                arch::invlpg(va);
            }
            true
        }
        None => false,
    }
}

pub fn translate(root: u64, va: u64) -> Option<u64> {
    let e = entry(root, va)?;
    (e & PRESENT != 0).then(|| (e & ADDR_MASK) | (va & 0xfff))
}

pub fn current_root() -> u64 {
    arch::read_cr3() & ADDR_MASK
}

static mut KERNEL_ROOT: u64 = 0;

pub fn kernel_root() -> u64 {
    unsafe { KERNEL_ROOT }
}

/// Take over the loader's tables: drop the identity map and make sure every
/// kernel-half PML4 slot exists, so address spaces can share them.
pub fn init() {
    let root = current_root();
    unsafe { KERNEL_ROOT = root };
    let t = table(root);
    for e in t[..256].iter_mut() {
        *e = 0;
    }
    for e in t[256..].iter_mut() {
        if *e & PRESENT == 0 {
            *e = phys::alloc().expect("page table") | PRESENT | WRITABLE;
        }
    }
    unsafe { arch::write_cr3(root) };
    // Enable NX (EFER.NXE) and global pages.
    unsafe {
        let efer = arch::rdmsr(0xc000_0080);
        arch::wrmsr(0xc000_0080, efer | 1 << 11);
    }
}

/// Create a new address space sharing the kernel half.
pub fn new_root() -> Option<u64> {
    let p = phys::alloc()?;
    let src = table(kernel_root());
    let dst = table(p);
    dst[256..].copy_from_slice(&src[256..]);
    Some(p)
}

/// Free the user half of an address space (page tables only; the caller
/// releases mapped frames).
pub fn free_user_tables(root: u64) {
    fn free_level(t: u64, level: u32) {
        if level > 0 {
            for &e in table(t).iter() {
                if e & PRESENT != 0 && e & HUGE == 0 {
                    free_level(e & ADDR_MASK, level - 1);
                }
            }
        }
        phys::free(t);
    }
    let t = table(root);
    for i in 0..256 {
        if t[i] & PRESENT != 0 {
            free_level(t[i] & ADDR_MASK, 2);
            t[i] = 0;
        }
    }
}
