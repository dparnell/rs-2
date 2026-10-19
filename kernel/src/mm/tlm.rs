//! Thread-local memory (DosAllocThreadLocalMemory) with threads of one
//! process running on several CPUs at once.
//!
//! The memory is at the same address in every thread, `TLM_BASE`, but each
//! thread has its own page there. A 2 MB range is reserved for it in every
//! address space and nothing else is ever mapped in it. Once a process uses
//! thread-local memory, each CPU running it gets private copies of the top
//! three page-table levels (PML4, the PDPT of the low 512 GB, the page
//! directory of the low 1 GB) and its own page table for the reserved
//! range. The copies point at the process's real page tables for
//! everything else, so all other mappings stay shared; the only entry that
//! differs is the one for the reserved range, whose table maps the running
//! thread's page. A context switch between threads of the process just
//! changes that one entry.
//!
//! The process may add page tables later (entries of its page directory
//! for 2 MB ranges it had not used): a CPU running on a copy then takes a
//! page fault there, and [`sync_fault`] copies the entry and retries.
//! Entries are only ever added during the life of a process, never changed
//! or removed, so this is the only synchronisation needed.

use alloc::sync::Arc;

use super::paging::{ADDR_MASK, PRESENT, USER, WRITABLE};
use super::phys::{self, phys_to_virt};
use crate::smp::MAX_CPUS;
use crate::sync::SpinLock;
use crate::task::thread::Thread;
use crate::task::Process;

/// Where thread-local memory lives in every address space.
pub const TLM_BASE: u32 = 0x1fc0_0000;
pub const TLM_SIZE: u32 = 0x20_0000;
/// Page directory index of the reserved range (in the low 1 GB).
const TLM_PD_INDEX: usize = (TLM_BASE >> 21) as usize & 511;

/// One CPU's copies (physical addresses).
#[derive(Clone, Copy)]
struct CpuTables {
    pml4: u64,
    pdpt: u64,
    pd: u64,
    pt: u64,
}

/// A process's per-CPU tables.
pub struct Tlm {
    cpus: SpinLock<[Option<CpuTables>; MAX_CPUS]>,
}

fn table(p: u64) -> &'static mut [u64; 512] {
    unsafe { &mut *(phys_to_virt(p) as *mut [u64; 512]) }
}

impl Tlm {
    pub fn new() -> Tlm {
        Tlm { cpus: SpinLock::new([None; MAX_CPUS]) }
    }

    /// This CPU's copies of `root`, built on first use.
    fn tables(&self, root: u64) -> Option<CpuTables> {
        let cpu = crate::smp::this().index();
        let mut g = self.cpus.lock();
        if let Some(t) = g[cpu] {
            return Some(t);
        }
        let orig_pml4 = table(root);
        let pdpt_e = orig_pml4[0];
        if pdpt_e & PRESENT == 0 {
            return None;
        }
        let pd_e = table(pdpt_e & ADDR_MASK)[0];
        if pd_e & PRESENT == 0 {
            return None;
        }
        let (pml4, pdpt, pd, pt) = (phys::alloc()?, phys::alloc()?, phys::alloc()?, phys::alloc()?);
        table(pml4).copy_from_slice(orig_pml4);
        table(pdpt).copy_from_slice(table(pdpt_e & ADDR_MASK));
        table(pd).copy_from_slice(table(pd_e & ADDR_MASK));
        table(pml4)[0] = pdpt | (pdpt_e & !ADDR_MASK);
        table(pdpt)[0] = pd | (pd_e & !ADDR_MASK);
        table(pd)[TLM_PD_INDEX] = pt | PRESENT | WRITABLE | USER;
        let t = CpuTables { pml4, pdpt, pd, pt };
        g[cpu] = Some(t);
        Some(t)
    }
}

impl Drop for Tlm {
    fn drop(&mut self) {
        // The shared lower levels belong to the address space.
        for t in self.cpus.lock().iter().flatten() {
            for p in [t.pml4, t.pdpt, t.pd, t.pt] {
                phys::free(p);
            }
        }
    }
}

/// The thread's own page, allocated (zeroed) on first use.
pub fn thread_page(t: &Thread) -> Option<u64> {
    let p = t.tlm_page.load(core::sync::atomic::Ordering::Relaxed);
    if p != 0 {
        return Some(p);
    }
    let p = phys::alloc()?;
    t.tlm_page.store(p, core::sync::atomic::Ordering::Relaxed);
    Some(p)
}

/// Switching to `t`, a thread of `p`, which uses thread-local memory: map
/// its page in this CPU's table and return the page-table root to load
/// (None: keep the process's own tables).
pub fn switch_in(p: &Process, t: &Thread) -> Option<u64> {
    let tlm = p.tlm.get()?;
    let tables = tlm.tables(p.cr3)?;
    let page = thread_page(t)?;
    let pte = &mut table(tables.pt)[0];
    let want = page | PRESENT | WRITABLE | USER | super::paging::NO_EXEC;
    if *pte != want {
        *pte = want;
        crate::arch::invlpg(TLM_BASE as u64);
    }
    Some(tables.pml4)
}

/// Start using thread-local memory in `p` (DosAllocThreadLocalMemory).
pub fn enable(p: &Arc<Process>) {
    // Callers hold the kernel lock.
    if p.tlm.get().is_none() {
        p.tlm.set(Tlm::new());
    }
}

/// A page fault at `addr` on this CPU: if it runs on a copy that lacks a
/// page directory entry the process has since added, copy it. Returns true
/// if the access can be retried.
pub fn sync_fault(addr: u64) -> bool {
    if addr >= 1 << 30 {
        return false;
    }
    let Some(t) = crate::task::sched::current_opt() else { return false };
    let Some(p) = t.process() else { return false };
    let Some(tlm) = p.tlm.get() else { return false };
    let cr3 = crate::arch::read_cr3() & ADDR_MASK;
    let cpu = crate::smp::this().index();
    let Some(tables) = tlm.cpus.lock()[cpu] else { return false };
    if cr3 != tables.pml4 {
        return false;
    }
    let i = (addr >> 21) as usize & 511;
    if i == TLM_PD_INDEX {
        return false;
    }
    let pdpt_e = table(p.cr3)[0];
    let orig = table(table(pdpt_e & ADDR_MASK)[0] & ADDR_MASK)[i];
    let copy = &mut table(tables.pd)[i];
    // The CPU sets the accessed and dirty bits in the copy itself.
    let same = (*copy ^ orig) & !(super::paging::ACCESSED | super::paging::DIRTY) == 0;
    if orig & PRESENT != 0 && !same {
        *copy = orig;
        return true;
    }
    false
}

/// A fault in the reserved range by a thread of a process using
/// thread-local memory, on a CPU still on the process's own tables (the
/// memory was enabled by a thread on another CPU): move this CPU to its
/// copies. Returns true if the access can be retried.
pub fn activate_fault(addr: u64) -> bool {
    if !(TLM_BASE as u64..(TLM_BASE + TLM_SIZE) as u64).contains(&addr) {
        return false;
    }
    let Some(t) = crate::task::sched::current_opt() else { return false };
    let Some(p) = t.process() else { return false };
    let Some(tlm) = p.tlm.get() else { return false };
    // Already on this CPU's copies: a real fault (only the first page of
    // the range is ever mapped).
    let cpu = crate::smp::this().index();
    if tlm.cpus.lock()[cpu].is_some_and(|c| c.pml4 == crate::arch::read_cr3() & ADDR_MASK) {
        return false;
    }
    match switch_in(&p, &t) {
        Some(root) => {
            crate::vm::activate(p.cr3, root, p.ldt_base, &p.cpus);
            true
        }
        None => false,
    }
}

