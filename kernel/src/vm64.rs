//! Memory above 4 GB, for 64-bit processes (docs/API64.md, "Memory").
//!
//! A 64-bit process has, besides the OS/2 arenas below 4 GB (vm.rs), the
//! 64-bit arena from 4 GB up to 512 GB: private objects only (DosAllocMem
//! from 64-bit code with OBJ_ANY), on 64 KB boundaries, bottom up, with
//! the same page states as below 4 GB (reserved, committed, guard) and
//! demand paging. It is the first PML4 entry's range, so thread-local
//! memory's per-CPU copies of the top table levels (mm::tlm) cover it as
//! they cover high memory. 32-bit code never sees it: nothing below 4 GB
//! changes, and only 64-bit callers get addresses here.

use alloc::vec::Vec;

use crate::mm::paging::{self, PRESENT, USER, WRITABLE};
use crate::mm::phys;
use crate::vm::{pag, AddressSpace, Fault, PageState, VmError, PAGE, TILE};

pub const ARENA64_BASE: u64 = 0x1_0000_0000;
pub const ARENA64_TOP: u64 = 0x80_0000_0000;
/// The largest object (DosAllocMem's size is a ULONG).
pub const MAX_OBJECT: u64 = 0xffff_f000;

const PAGE64: u64 = PAGE as u64;
const TILE64: u64 = TILE as u64;

/// A private object in the 64-bit arena.
#[derive(Clone)]
pub struct Region64 {
    pub base: u64,
    /// Bytes, rounded up to pages.
    pub size: u64,
    /// `pag::READ | WRITE | EXECUTE` and allocation flags.
    pub flags: u32,
    pub pages: Vec<PageState>,
}

impl Region64 {
    pub fn end(&self) -> u64 {
        self.base + self.size
    }
    fn reserved_end(&self) -> u64 {
        self.end().next_multiple_of(TILE64)
    }
}

/// True for an address in the 64-bit arena.
pub fn in_arena(addr: u64) -> bool {
    (ARENA64_BASE..ARENA64_TOP).contains(&addr)
}

impl AddressSpace {
    fn region64(&self, addr: u64) -> Option<&Region64> {
        let (_, r) = self.high64.range(..=addr).next_back()?;
        (addr < r.end()).then_some(r)
    }

    fn region64_mut(&mut self, addr: u64) -> Option<&mut Region64> {
        let (_, r) = self.high64.range_mut(..=addr).next_back()?;
        (addr < r.end()).then_some(r)
    }

    /// DosAllocMem in the 64-bit arena: first fit from 4 GB up.
    pub fn alloc64(&mut self, size: u64, flags: u32) -> Result<u64, VmError> {
        if size == 0 || size > MAX_OBJECT {
            return Err(VmError::InvalidParameter);
        }
        let size = size.next_multiple_of(PAGE64);
        let span = size.next_multiple_of(TILE64);
        let mut base = ARENA64_BASE;
        for r in self.high64.values() {
            if r.base >= base + span {
                break;
            }
            base = base.max(r.reserved_end());
        }
        if base + span > ARENA64_TOP {
            return Err(VmError::NoMemory);
        }
        let state = if flags & pag::COMMIT != 0 {
            if flags & pag::GUARD != 0 { PageState::Guard } else { PageState::Committed }
        } else {
            PageState::Reserved
        };
        let n = (size / PAGE64) as usize;
        let mut pages = Vec::new();
        if pages.try_reserve_exact(n).is_err() {
            return Err(VmError::NoMemory);
        }
        pages.resize(n, state);
        self.high64.insert(base, Region64 { base, size, flags: flags & !(pag::COMMIT | pag::GUARD), pages });
        Ok(base)
    }

    /// DosFreeMem of an object in the 64-bit arena.
    pub fn free64(&mut self, base: u64) -> Result<(), VmError> {
        let r = self.high64.remove(&base).ok_or(VmError::InvalidAddress)?;
        let mut frames = Vec::new();
        for i in 0..r.size / PAGE64 {
            if r.pages[i as usize] == PageState::Reserved {
                // Never mapped while reserved (set_mem64 unmaps on decommit).
                continue;
            }
            if let Some(old) = paging::unmap(self.root, r.base + i * PAGE64) {
                frames.push(old & paging::ADDR_MASK);
            }
        }
        if !frames.is_empty() {
            self.shootdown();
        }
        for f in frames {
            phys::free(f);
        }
        Ok(())
    }

    /// DosSetMem in the 64-bit arena.
    pub fn set_mem64(&mut self, addr: u64, size: u64, flags: u32) -> Result<(), VmError> {
        if size == 0 {
            return Err(VmError::InvalidParameter);
        }
        let root = self.root;
        let r = self.region64_mut(addr).ok_or(VmError::InvalidAddress)?;
        let first = (addr - r.base) / PAGE64;
        let last = (addr + size - 1 - r.base) / PAGE64;
        if last >= r.size / PAGE64 {
            return Err(VmError::InvalidAddress);
        }
        let mut frames = Vec::new();
        let mut changed = false;
        for i in first..=last {
            let va = r.base + i * PAGE64;
            let st = &mut r.pages[i as usize];
            if flags & pag::DECOMMIT != 0 {
                *st = PageState::Reserved;
                if let Some(old) = paging::unmap(root, va) {
                    changed = true;
                    frames.push(old & paging::ADDR_MASK);
                }
            } else if flags & pag::COMMIT != 0 {
                *st = if flags & pag::GUARD != 0 { PageState::Guard } else { PageState::Committed };
                if *st == PageState::Guard {
                    if let Some(old) = paging::entry(root, va) {
                        if old & PRESENT != 0 && old & USER != 0 {
                            paging::set_entry(root, va, old & !USER);
                            changed = true;
                        }
                    }
                }
            } else if flags & pag::GUARD != 0 && *st == PageState::Committed {
                *st = PageState::Guard;
                if let Some(old) = paging::entry(root, va) {
                    if old & PRESENT != 0 && old & USER != 0 {
                        paging::set_entry(root, va, old & !USER);
                        changed = true;
                    }
                }
            }
        }
        if flags & (pag::READ | pag::WRITE | pag::EXECUTE) != 0 && flags & pag::DECOMMIT == 0 {
            r.flags = (r.flags & !7) | (flags & 7);
            for i in first..=last {
                let va = r.base + i * PAGE64;
                if let Some(old) = paging::entry(root, va) {
                    if old & PRESENT != 0 {
                        let e = if flags & pag::WRITE != 0 { old | WRITABLE } else { old & !WRITABLE };
                        paging::set_entry(root, va, e);
                        if old & WRITABLE != 0 && e & WRITABLE == 0 {
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            self.shootdown();
        }
        for f in frames {
            phys::free(f);
        }
        Ok(())
    }

    /// DosQueryMem in the 64-bit arena: the run of pages from `addr` (up
    /// to `max` bytes) with the same attributes, and those attributes.
    /// None outside the arena.
    pub fn query64(&self, addr: u64, max: u64) -> Option<(u64, u32)> {
        let addr = addr & !(PAGE64 - 1);
        if !in_arena(addr) {
            return None;
        }
        let Some(r) = self.region64(addr) else {
            let next = self.high64.range(addr..).next().map_or(ARENA64_TOP, |(&b, _)| b);
            return Some(((next - addr).min(max.max(PAGE64)), pag::FREE));
        };
        let first = ((addr - r.base) / PAGE64) as usize;
        let st = r.pages[first];
        let total = r.pages.len();
        let mut n = 1;
        while first + n < total && r.pages[first + n] == st && (n as u64 + 1) * PAGE64 <= max {
            n += 1;
        }
        let mut flags = r.flags & (pag::READ | pag::WRITE | pag::EXECUTE);
        match st {
            PageState::Committed => flags |= pag::COMMIT,
            PageState::Guard => flags |= pag::COMMIT | pag::GUARD,
            PageState::Reserved => {}
        }
        if addr == r.base {
            flags |= pag::BASE;
        }
        Some((n as u64 * PAGE64, flags))
    }

    /// A page fault (or kernel access) at `addr`, any user address: below
    /// 4 GB as `fault_in`, in the 64-bit arena its objects' pages.
    pub fn fault_in64(&mut self, addr: u64, write: bool) -> Fault {
        if addr <= u32::MAX as u64 {
            return self.fault_in(addr as u32, write);
        }
        let root = self.root;
        let Some(r) = self.region64_mut(addr) else { return Fault::Invalid };
        let i = ((addr - r.base) / PAGE64) as usize;
        let va = r.base + i as u64 * PAGE64;
        match r.pages[i] {
            PageState::Reserved => return Fault::Invalid,
            PageState::Guard => {
                r.pages[i] = PageState::Committed;
                if let Some(e) = paging::entry(root, va) {
                    if e & PRESENT != 0 && e & USER == 0 {
                        paging::set_entry(root, va, e | USER);
                    }
                }
                return Fault::Guard;
            }
            PageState::Committed => {}
        }
        if (write && r.flags & pag::WRITE == 0) || r.flags & (pag::READ | pag::WRITE | pag::EXECUTE) == 0 {
            return Fault::Protection;
        }
        if let Some(e) = paging::entry(root, va) {
            if e & PRESENT != 0 {
                return if write && e & WRITABLE == 0 { Fault::Protection } else { Fault::Resolved };
            }
        }
        let Some(frame) = phys::alloc_user() else { return Fault::NoMemory };
        let flags = if r.flags & pag::WRITE != 0 { USER | WRITABLE } else { USER };
        if !paging::map(root, va, frame, flags) {
            phys::free(frame);
            return Fault::NoMemory;
        }
        Fault::Resolved
    }

    /// The guard page at `page` was committed: the page below becomes the
    /// guard page (as `guard_below`), any user address.
    pub fn guard_below64(&mut self, page: u64) {
        if page <= u32::MAX as u64 {
            return self.guard_below(page as u32);
        }
        let below = page - PAGE64;
        let Some(r) = self.region64_mut(below) else { return };
        let i = ((below - r.base) / PAGE64) as usize;
        if r.pages[i] == PageState::Reserved {
            r.pages[i] = PageState::Guard;
        }
    }

    /// `check` for any user address: `[addr, addr+len)` accessible (and
    /// faulted in), entirely below 4 GB or entirely in the 64-bit arena.
    pub fn check64(&mut self, addr: u64, len: u64, write: bool) -> bool {
        if len == 0 {
            return true;
        }
        let Some(end) = addr.checked_add(len) else { return false };
        if end <= 1 << 32 {
            return len <= u32::MAX as u64 && self.check(addr as u32, len as u32, write);
        }
        if !in_arena(addr) || end > ARENA64_TOP {
            return false;
        }
        let mut p = addr & !(PAGE64 - 1);
        while p < end {
            match self.fault_in64(p, write) {
                Fault::Resolved => {}
                Fault::Guard => {
                    self.guard_below64(p);
                    if self.fault_in64(p, write) != Fault::Resolved {
                        return false;
                    }
                }
                _ => return false,
            }
            p += PAGE64;
        }
        true
    }

    /// Copy bytes out of user memory at any user address.
    pub fn read_bytes64(&mut self, addr: u64, out: &mut [u8]) -> bool {
        if addr + (out.len() as u64) <= 1 << 32 {
            return self.read_bytes(addr as u32, out);
        }
        if !self.check64(addr, out.len() as u64, false) {
            return false;
        }
        self.copy64(addr, out.len(), |pa, done, n| unsafe {
            core::ptr::copy_nonoverlapping(phys::phys_to_virt(pa), out[done..].as_mut_ptr(), n)
        })
    }

    /// Copy bytes into user memory at any user address.
    pub fn write_bytes64(&mut self, addr: u64, data: &[u8]) -> bool {
        if addr + (data.len() as u64) <= 1 << 32 {
            return self.write_bytes(addr as u32, data);
        }
        if !self.check64(addr, data.len() as u64, true) {
            return false;
        }
        self.copy64(addr, data.len(), |pa, done, n| unsafe {
            core::ptr::copy_nonoverlapping(data[done..].as_ptr(), phys::phys_to_virt(pa), n)
        })
    }

    /// Walk the present pages of `[addr, addr+len)`, page by page.
    fn copy64(&self, addr: u64, len: usize, mut f: impl FnMut(u64, usize, usize)) -> bool {
        let mut done = 0usize;
        while done < len {
            let va = addr + done as u64;
            let Some(pa) = paging::translate(self.root, va) else { return false };
            let n = (PAGE as usize - (va as usize & 0xfff)).min(len - done);
            f(pa, done, n);
            done += n;
        }
        true
    }

    /// Release everything in the 64-bit arena (the process ends).
    pub(crate) fn free_all64(&mut self) {
        let bases: Vec<u64> = self.high64.keys().copied().collect();
        for b in bases {
            let _ = self.free64(b);
        }
    }

    /// Bytes committed in the 64-bit arena (for the log and tests).
    pub fn committed64(&self) -> u64 {
        self.high64.values().map(|r| r.pages.iter().filter(|&&s| s != PageState::Reserved).count() as u64 * PAGE64).sum()
    }
}
