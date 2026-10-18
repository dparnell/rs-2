//! OS/2 process address spaces.
//!
//! Layout of the 512 MB compatibility region (identical to OS/2 2.x+):
//!
//! ```text
//! 0x00000000  unused first 64 KB (null pointer trap)
//! 0x00010000  private arena, allocated bottom up
//!     ...
//!             shared arena, allocated top down, same addresses in every
//!             process (system DLLs, shared memory, the API gate)
//! 0x20000000  end of the tiled region
//! ```
//!
//! Every memory object starts on a 64 KB boundary. LDT entry `n` describes
//! the 64 KB tile at linear `n << 16`, so a 16:16 pointer `sel:off` and the
//! flat address `(sel >> 3) << 16 | off` are interchangeable.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use crate::arch::gdt::{self, access, descriptor};
use crate::mm::paging::{self, PRESENT, USER, WRITABLE};
use crate::mm::phys;
use crate::sync::SpinLock;

pub const PAGE: u32 = 4096;
pub const TILE: u32 = 0x1_0000;
pub const ARENA_START: u32 = 0x0001_0000;
pub const TILED_TOP: u32 = 0x2000_0000;
/// The API gate occupies the top 128 KB of the shared arena.
pub const GATE_BASE: u32 = 0x1ffe_0000;
pub const GATE_SIZE: u32 = 0x2_0000;
pub const LDT_ENTRIES: usize = 8192;

/// `DosAllocMem` and friends: protection and allocation flags.
pub mod pag {
    pub const READ: u32 = 0x0001;
    pub const WRITE: u32 = 0x0002;
    pub const EXECUTE: u32 = 0x0004;
    pub const GUARD: u32 = 0x0008;
    pub const COMMIT: u32 = 0x0010;
    pub const DECOMMIT: u32 = 0x0020;
    pub const OBJ_TILE: u32 = 0x0040;
    pub const OBJ_PROTECTED: u32 = 0x0080;
    pub const OBJ_GETTABLE: u32 = 0x0100;
    pub const OBJ_GIVEABLE: u32 = 0x0200;
    pub const OBJ_ANY: u32 = 0x0400;
    pub const SHARED: u32 = 0x2000;
    pub const FREE: u32 = 0x4000;
    pub const BASE: u32 = 0x10000;
}

/// How the tiles of an object are described in the LDT.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegKind {
    /// No LDT descriptors (not tiled).
    None,
    Data,
    /// 16-bit code (execute/read).
    Code16,
    /// 32-bit code, for 16:32 far pointers into flat code.
    Code32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    Reserved,
    Committed,
    /// Committed, but the first touch raises a guard page exception.
    Guard,
}

/// Memory shared between processes (shared memory, DLL shared objects).
pub struct SharedMem {
    pub base: u32,
    pub size: u32,
    pub name: Option<alloc::string::String>,
    pub flags: u32,
    /// Physical frame per page, 0 until first touched.
    pub frames: SpinLock<Vec<u64>>,
    /// Commit state shared by all mappings.
    pub pages: SpinLock<Vec<PageState>>,
}

impl SharedMem {
    pub fn new(base: u32, size: u32, flags: u32, name: Option<alloc::string::String>) -> Arc<Self> {
        let n = (size / PAGE) as usize;
        Arc::new(SharedMem {
            base,
            size,
            name,
            flags,
            frames: SpinLock::new(vec![0; n]),
            pages: SpinLock::new(vec![PageState::Reserved; n]),
        })
    }

    /// Frame for page `i`, allocating a zeroed one on first use.
    pub fn frame(&self, i: usize) -> Option<u64> {
        let mut f = self.frames.lock();
        if f[i] == 0 {
            f[i] = phys::alloc()?;
        }
        Some(f[i])
    }
}

impl Drop for SharedMem {
    fn drop(&mut self) {
        for &f in self.frames.lock().iter() {
            if f != 0 {
                phys::free(f);
            }
        }
        shared_arena().lock().remove(&self.base);
    }
}

#[derive(Clone)]
pub enum Backing {
    Private,
    Shared(Arc<SharedMem>),
}

#[derive(Clone)]
pub struct Region {
    pub base: u32,
    /// Size in bytes as requested, rounded up to pages.
    pub size: u32,
    /// `pag::READ | WRITE | EXECUTE` and allocation flags.
    pub flags: u32,
    /// Per-page state for private memory (shared memory keeps its own).
    pub pages: Vec<PageState>,
    pub backing: Backing,
    pub seg: SegKind,
    /// Explicit 16-bit segment limit for the first tile (DosAllocSeg).
    pub seg_limit: Option<u32>,
}

impl Region {
    pub fn end(&self) -> u32 {
        self.base + self.size
    }

    pub fn reserved_end(&self) -> u32 {
        (self.base + self.size).next_multiple_of(TILE)
    }

    fn page_state(&self, i: usize) -> PageState {
        match &self.backing {
            Backing::Private => self.pages[i],
            Backing::Shared(s) => s.pages.lock()[i],
        }
    }

    fn set_page_state(&mut self, i: usize, st: PageState) {
        match &self.backing {
            Backing::Private => self.pages[i] = st,
            Backing::Shared(s) => s.pages.lock()[i] = st,
        }
    }
}

/// Address ranges reserved in the shared arena (by base), shared by all
/// processes so a shared object has one address everywhere.
fn shared_arena() -> &'static SpinLock<BTreeMap<u32, u32>> {
    static ARENA: SpinLock<BTreeMap<u32, u32>> = SpinLock::new(BTreeMap::new());
    &ARENA
}

/// Reserve `size` bytes (rounded to 64 KB) in the shared arena. With
/// `at = Some(addr)` the exact address is required.
pub fn reserve_shared(size: u32, at: Option<u32>) -> Option<u32> {
    let size = size.next_multiple_of(TILE);
    let mut a = shared_arena().lock();
    let overlaps =
        |a: &BTreeMap<u32, u32>, base: u32| a.iter().any(|(&b, &s)| base < b + s && b < base + size);
    if let Some(base) = at {
        if base % TILE != 0 || base + size > TILED_TOP || overlaps(&a, base) {
            return None;
        }
        a.insert(base, size);
        return Some(base);
    }
    let mut top = GATE_BASE;
    loop {
        let base = top.checked_sub(size)?;
        if base < 0x0400_0000 {
            return None;
        }
        match a.iter().find(|&(&b, &s)| base < b + s && b < base + size) {
            Some((&b, _)) => top = b,
            None => {
                a.insert(base, size);
                return Some(base);
            }
        }
    }
}

pub fn release_shared(base: u32) {
    shared_arena().lock().remove(&base);
}

fn shared_overlaps(base: u32, size: u32) -> bool {
    shared_arena().lock().iter().any(|(&b, &s)| base < b + s && b < base + size)
}

pub struct AddressSpace {
    /// Physical address of the PML4.
    pub root: u64,
    pub ldt: Box<[u64; LDT_ENTRIES]>,
    pub regions: BTreeMap<u32, Region>,
}

/// Errors in OS/2 terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    NoMemory,
    InvalidAddress,
    AccessDenied,
    InvalidParameter,
}

impl VmError {
    pub fn code(self) -> u32 {
        match self {
            VmError::NoMemory => 8,          // ERROR_NOT_ENOUGH_MEMORY
            VmError::InvalidAddress => 487,  // ERROR_INVALID_ADDRESS
            VmError::AccessDenied => 5,      // ERROR_ACCESS_DENIED
            VmError::InvalidParameter => 87, // ERROR_INVALID_PARAMETER
        }
    }
}

impl AddressSpace {
    pub fn new() -> Option<Self> {
        let root = paging::new_root()?;
        let ldt: Box<[u64; LDT_ENTRIES]> = unsafe { Box::new_zeroed().assume_init() };
        let mut s = AddressSpace { root, ldt, regions: BTreeMap::new() };
        s.map_gate();
        crate::infoseg::attach(&mut s);
        Some(s)
    }

    fn map_gate(&mut self) {
        let gate = crate::loader::gate::shared();
        self.insert(Region {
            base: GATE_BASE,
            size: GATE_SIZE,
            flags: pag::READ | pag::EXECUTE,
            pages: Vec::new(),
            backing: Backing::Shared(gate),
            seg: SegKind::Code16,
            seg_limit: None,
        });
    }

    /// Make this address space current (CR3 and LDT).
    pub fn activate(&self) {
        if paging::current_root() != self.root {
            unsafe { crate::arch::write_cr3(self.root) };
        }
        gdt::load_ldt(self.ldt.as_ptr() as u64, LDT_ENTRIES);
    }

    /// Region containing `addr`.
    pub fn region(&self, addr: u32) -> Option<&Region> {
        let (_, r) = self.regions.range(..=addr).next_back()?;
        (addr < r.end()).then_some(r)
    }

    pub fn region_mut(&mut self, addr: u32) -> Option<&mut Region> {
        let (_, r) = self.regions.range_mut(..=addr).next_back()?;
        (addr < r.end()).then_some(r)
    }

    /// True if `[base, base+size)` (rounded to 64 KB) is free for private use.
    pub fn is_free(&self, base: u32, size: u32) -> bool {
        self.free_in_private(base, size)
    }

    fn free_in_private(&self, base: u32, size: u32) -> bool {
        let end = base + size.next_multiple_of(TILE);
        if base < ARENA_START || end > TILED_TOP {
            return false;
        }
        let clash = self.regions.values().any(|r| base < r.reserved_end() && r.base < end);
        !clash && !shared_overlaps(base, size.next_multiple_of(TILE))
    }

    /// Find room for `size` bytes in the private arena.
    pub fn find_private(&self, size: u32) -> Option<u32> {
        let size = size.next_multiple_of(TILE);
        let mut base = ARENA_START;
        while base + size <= TILED_TOP {
            if self.free_in_private(base, size) {
                return Some(base);
            }
            base += TILE;
        }
        None
    }

    /// Insert a region and write its LDT descriptors.
    pub fn insert(&mut self, r: Region) {
        self.write_descriptors(&r);
        self.regions.insert(r.base, r);
    }

    fn write_descriptors(&mut self, r: &Region) {
        if r.seg == SegKind::None || r.base >= TILED_TOP {
            return;
        }
        let mut tile = r.base;
        let mut first = true;
        while tile < r.end() {
            let remaining = r.end() - tile;
            let mut limit = remaining.min(TILE) - 1;
            if first {
                if let Some(l) = r.seg_limit {
                    limit = l;
                }
            }
            first = false;
            self.ldt[(tile >> 16) as usize] = seg_descriptor(tile, limit, r.seg, r.flags);
            tile += TILE;
        }
    }

    fn clear_descriptors(&mut self, base: u32, end: u32) {
        let mut tile = base;
        while tile < end && tile < TILED_TOP {
            self.ldt[(tile >> 16) as usize] = 0;
            tile += TILE;
        }
    }

    /// Allocate a private object (DosAllocMem semantics).
    pub fn alloc(&mut self, size: u32, flags: u32, seg: SegKind, at: Option<u32>) -> Result<u32, VmError> {
        if size == 0 {
            return Err(VmError::InvalidParameter);
        }
        let size = size.next_multiple_of(PAGE);
        let base = match at {
            Some(a) => {
                if !self.free_in_private(a, size) {
                    return Err(VmError::InvalidAddress);
                }
                a
            }
            None => self.find_private(size).ok_or(VmError::NoMemory)?,
        };
        let state = if flags & pag::COMMIT != 0 {
            if flags & pag::GUARD != 0 { PageState::Guard } else { PageState::Committed }
        } else {
            PageState::Reserved
        };
        self.insert(Region {
            base,
            size,
            flags,
            pages: vec![state; (size / PAGE) as usize],
            backing: Backing::Private,
            seg,
            seg_limit: None,
        });
        Ok(base)
    }

    /// Map a shared object (it already has an address in the shared arena).
    pub fn map_shared(&mut self, obj: Arc<SharedMem>, flags: u32, seg: SegKind) {
        self.insert(Region {
            base: obj.base,
            size: obj.size,
            flags,
            pages: Vec::new(),
            backing: Backing::Shared(obj),
            seg,
            seg_limit: None,
        });
    }

    /// Release the object starting at `base`.
    pub fn free(&mut self, base: u32) -> Result<(), VmError> {
        let r = self.regions.remove(&base).ok_or(VmError::InvalidAddress)?;
        for i in 0..r.size / PAGE {
            let va = (r.base + i * PAGE) as u64;
            if let Some(old) = paging::unmap(self.root, va) {
                if matches!(r.backing, Backing::Private) {
                    phys::free(old & paging::ADDR_MASK);
                }
            }
        }
        self.clear_descriptors(r.base, r.reserved_end());
        Ok(())
    }

    /// Commit or decommit pages (DosSetMem).
    pub fn set_mem(&mut self, addr: u32, size: u32, flags: u32) -> Result<(), VmError> {
        let root = self.root;
        let r = self.region_mut(addr).ok_or(VmError::InvalidAddress)?;
        let first = (addr - r.base) / PAGE;
        let last = (addr + size - 1 - r.base) / PAGE;
        if r.base + (last + 1) * PAGE > r.base + r.size {
            return Err(VmError::InvalidAddress);
        }
        for i in first..=last {
            let va = (r.base + i * PAGE) as u64;
            if flags & pag::DECOMMIT != 0 {
                r.set_page_state(i as usize, PageState::Reserved);
                if let Some(old) = paging::unmap(root, va) {
                    if matches!(r.backing, Backing::Private) {
                        phys::free(old & paging::ADDR_MASK);
                    }
                }
            } else if flags & pag::COMMIT != 0 {
                let st = if flags & pag::GUARD != 0 { PageState::Guard } else { PageState::Committed };
                r.set_page_state(i as usize, st);
            }
        }
        if flags & (pag::READ | pag::WRITE | pag::EXECUTE) != 0 && flags & pag::DECOMMIT == 0 {
            r.flags = (r.flags & !7) | (flags & 7);
            // Re-protect present pages.
            for i in first..=last {
                let va = (r.base + i * PAGE) as u64;
                if let Some(e) = paging::entry(root, va) {
                    if e & PRESENT != 0 {
                        let mut e = e & !WRITABLE;
                        if flags & pag::WRITE != 0 {
                            e |= WRITABLE;
                        }
                        paging::set_entry(root, va, e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Bring the page containing `addr` in. `write` requests write access.
    /// Returns the result for the fault handler.
    pub fn fault_in(&mut self, addr: u32, write: bool) -> Fault {
        let root = self.root;
        let Some(r) = self.region_mut(addr) else { return Fault::Invalid };
        let i = ((addr - r.base) / PAGE) as usize;
        let va = (r.base + i as u32 * PAGE) as u64;
        match r.page_state(i) {
            PageState::Reserved => return Fault::Invalid,
            PageState::Guard => {
                r.set_page_state(i, PageState::Committed);
                return Fault::Guard;
            }
            PageState::Committed => {}
        }
        if write && r.flags & pag::WRITE == 0 {
            return Fault::Protection;
        }
        if r.flags & (pag::READ | pag::WRITE | pag::EXECUTE) == 0 {
            return Fault::Protection;
        }
        if let Some(e) = paging::entry(root, va) {
            if e & PRESENT != 0 {
                // Present but faulted: a protection problem.
                return if write && e & WRITABLE == 0 { Fault::Protection } else { Fault::Resolved };
            }
        }
        let frame = match &r.backing {
            Backing::Private => phys::alloc(),
            Backing::Shared(s) => s.frame(i),
        };
        let Some(frame) = frame else { return Fault::NoMemory };
        let mut flags = USER;
        if r.flags & pag::WRITE != 0 {
            flags |= WRITABLE;
        }
        if !paging::map(root, va, frame, flags) {
            return Fault::NoMemory;
        }
        Fault::Resolved
    }

    /// Check that `[addr, addr+len)` is accessible and fault it in, so the
    /// kernel can then touch it directly.
    pub fn check(&mut self, addr: u32, len: u32, write: bool) -> bool {
        if len == 0 {
            return true;
        }
        let Some(end) = addr.checked_add(len) else { return false };
        let mut p = addr & !(PAGE - 1);
        while p < end {
            match self.fault_in(p, write) {
                Fault::Resolved => {}
                // Kernel touching a guard page commits it silently.
                Fault::Guard => {
                    if self.fault_in(p, write) != Fault::Resolved {
                        return false;
                    }
                }
                _ => return false,
            }
            p += PAGE;
        }
        true
    }

    /// Copy bytes into user memory (already-validated or not).
    pub fn write_bytes(&mut self, addr: u32, data: &[u8]) -> bool {
        if !self.check(addr, data.len() as u32, true) {
            return false;
        }
        // The pages may be read-only for the user (loader writes): go through
        // the physical frames.
        let mut done = 0usize;
        while done < data.len() {
            let va = addr as u64 + done as u64;
            let Some(pa) = paging::translate(self.root, va) else { return false };
            let n = (PAGE as usize - (va as usize & 0xfff)).min(data.len() - done);
            unsafe { core::ptr::copy_nonoverlapping(data[done..].as_ptr(), phys::phys_to_virt(pa), n) };
            done += n;
        }
        true
    }

    /// Kernel-privileged write (loader): commits the pages and ignores the
    /// user protection.
    pub fn poke_bytes(&mut self, addr: u32, data: &[u8]) -> bool {
        if data.is_empty() {
            return true;
        }
        let first = addr & !(PAGE - 1);
        let end = addr + data.len() as u32;
        let mut p = first;
        while p < end {
            let Some(r) = self.region(p) else { return false };
            let run_end = r.end().min(end);
            let base = p;
            if !self.populate(base, run_end - base) {
                return false;
            }
            p = run_end.next_multiple_of(PAGE);
        }
        let mut done = 0usize;
        while done < data.len() {
            let va = addr as u64 + done as u64;
            let Some(pa) = paging::translate(self.root, va) else { return false };
            let n = (PAGE as usize - (va as usize & 0xfff)).min(data.len() - done);
            unsafe { core::ptr::copy_nonoverlapping(data[done..].as_ptr(), phys::phys_to_virt(pa), n) };
            done += n;
        }
        true
    }

    pub fn read_bytes(&mut self, addr: u32, out: &mut [u8]) -> bool {
        if !self.check(addr, out.len() as u32, false) {
            return false;
        }
        let mut done = 0usize;
        while done < out.len() {
            let va = addr as u64 + done as u64;
            let Some(pa) = paging::translate(self.root, va) else { return false };
            let n = (PAGE as usize - (va as usize & 0xfff)).min(out.len() - done);
            unsafe { core::ptr::copy_nonoverlapping(phys::phys_to_virt(pa), out[done..].as_mut_ptr(), n) };
            done += n;
        }
        true
    }

    /// Commit and fault in every page of a region with write access for the
    /// loader, regardless of user protection.
    pub fn populate(&mut self, base: u32, size: u32) -> bool {
        let root = self.root;
        let Some(r) = self.region_mut(base) else { return false };
        let first = ((base - r.base) / PAGE) as usize;
        let n = size.div_ceil(PAGE) as usize;
        for i in first..first + n {
            if r.page_state(i) == PageState::Reserved {
                r.set_page_state(i, PageState::Committed);
            }
            let va = (r.base + i as u32 * PAGE) as u64;
            if paging::translate(root, va).is_some() {
                continue;
            }
            let frame = match &r.backing {
                Backing::Private => phys::alloc(),
                Backing::Shared(s) => s.frame(i),
            };
            let Some(frame) = frame else { return false };
            let mut flags = USER;
            if r.flags & pag::WRITE != 0 {
                flags |= WRITABLE;
            }
            if !paging::map(root, va, frame, flags) {
                return false;
            }
        }
        true
    }

    /// Grow a private object in place (within its 64 KB reservation or into
    /// free address space after it).
    pub fn resize(&mut self, base: u32, new_size: u32) -> Result<(), VmError> {
        let new_size = new_size.next_multiple_of(PAGE);
        let (old_size, old_resv) = {
            let r = self.regions.get(&base).ok_or(VmError::InvalidAddress)?;
            (r.size, r.reserved_end())
        };
        if new_size <= old_size {
            return Ok(());
        }
        let new_end = (base + new_size).next_multiple_of(TILE);
        if new_end > old_resv && !self.is_free(old_resv, new_end - old_resv) {
            return Err(VmError::NoMemory);
        }
        let r = self.regions.get_mut(&base).unwrap();
        if !matches!(r.backing, Backing::Private) {
            return Err(VmError::AccessDenied);
        }
        r.size = new_size;
        r.pages.resize((new_size / PAGE) as usize, PageState::Committed);
        let r = r.clone();
        self.write_descriptors(&r);
        Ok(())
    }

    /// DosQueryMem: size of the run of pages starting at `addr` (up to
    /// `max` bytes) that share attributes, and those attributes.
    pub fn query(&self, addr: u32, max: u32) -> Option<(u32, u32)> {
        let addr = addr & !(PAGE - 1);
        let Some(r) = self.region(addr) else {
            // Free memory: report up to the next region.
            let next = self.regions.range(addr..).next().map_or(TILED_TOP, |(&b, _)| b);
            return Some(((next - addr).min(max.max(PAGE)), pag::FREE));
        };
        let first = ((addr - r.base) / PAGE) as usize;
        let st = r.page_state(first);
        let mut n = 1;
        let total = (r.size / PAGE) as usize;
        while first + n < total && r.page_state(first + n) == st && (n as u32 + 1) * PAGE <= max {
            n += 1;
        }
        let mut flags = r.flags & (pag::READ | pag::WRITE | pag::EXECUTE);
        match st {
            PageState::Committed => flags |= pag::COMMIT,
            PageState::Guard => flags |= pag::COMMIT | pag::GUARD,
            PageState::Reserved => {}
        }
        if matches!(r.backing, Backing::Shared(_)) {
            flags |= pag::SHARED;
        }
        if addr == r.base {
            flags |= pag::BASE;
        }
        Some((n as u32 * PAGE, flags))
    }

    /// Change a 16-bit segment's size (DosReallocSeg).
    pub fn set_seg_limit(&mut self, base: u32, limit: u32) {
        if let Some(r) = self.regions.get_mut(&base) {
            r.seg_limit = Some(limit);
            let r = r.clone();
            self.write_descriptors(&r);
        }
    }

    /// Descriptor table entry for a selector (LDT or GDT).
    pub fn descriptor(&self, sel: u16) -> u64 {
        let idx = (sel >> 3) as usize;
        if sel & 4 != 0 { self.ldt[idx] } else { gdt::entry(idx) }
    }

    /// Linear address of `sel:off`, or None for an invalid selector.
    pub fn linear(&self, sel: u16, off: u32) -> Option<u32> {
        if sel < 4 {
            return None;
        }
        let d = self.descriptor(sel);
        if d & (1 << 47) == 0 {
            return None;
        }
        Some(desc_base(d).wrapping_add(off))
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        let bases: Vec<u32> = self.regions.keys().copied().collect();
        for b in bases {
            let _ = self.free(b);
        }
        paging::free_user_tables(self.root);
        phys::free(self.root);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    Resolved,
    Guard,
    Invalid,
    Protection,
    NoMemory,
}

pub fn desc_base(d: u64) -> u32 {
    (((d >> 16) & 0xff_ffff) | ((d >> 56) & 0xff) << 24) as u32
}

pub fn desc_limit(d: u64) -> u32 {
    let l = ((d & 0xffff) | ((d >> 48) & 0xf) << 16) as u32;
    if d & (1 << 55) != 0 { l << 12 | 0xfff } else { l }
}

/// True if the descriptor is a 32-bit (B/D set) segment.
pub fn desc_big(d: u64) -> bool {
    d & (1 << 54) != 0
}

/// Build a user descriptor for a tile.
pub fn seg_descriptor(base: u32, limit: u32, kind: SegKind, flags: u32) -> u64 {
    use access::*;
    let a = PRESENT | DPL3 | CODE_DATA | ACCESSED;
    let (a, f) = match kind {
        SegKind::None => return 0,
        SegKind::Data => {
            let w = if flags & pag::WRITE != 0 { WRITABLE } else { 0 };
            (a | w, 0)
        }
        SegKind::Code16 => (a | CODE | READABLE, 0),
        SegKind::Code32 => (a | CODE | READABLE, gdt::flags::BIG),
    };
    descriptor(base, limit, a, f)
}

/// The tiled selector for a linear address in the compatibility region.
pub fn tile_selector(addr: u32) -> u16 {
    ((addr >> 13) | 7) as u16
}

/// Flat address of a tiled 16:16 pointer.
pub fn tiled_flat(sel: u16, off: u16) -> u32 {
    ((sel as u32 >> 3) << 16) | off as u32
}
