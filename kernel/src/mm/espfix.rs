//! Returning to a 16-bit stack segment ("espfix", as Linux's espfix64).
//!
//! `iretq` to a 16-bit stack segment loads only SP: bits 16..31 of ESP
//! keep those of the kernel's RSP at the `iretq`. 16-bit code that goes on
//! to 32-bit code with that ESP (Borland's C runtime thunks, after a page
//! fault in them: Resource Workshop) faulted on a garbage stack address.
//!
//! So such a return runs its `iretq` on a stack whose address has the
//! program's bits 16..31: each CPU has a 4 GB window of the kernel half in
//! which one page appears in every 64 KB, so whatever the program's ESP,
//! the frame can be put at `window + (ESP & 0xffff0000) + FRAME`. Nothing is
//! written to the program's own stack: IBM's thunks (VisualAge C++'s, in
//! IBM Works' database engine) keep the 16:16 address they call below SP,
//! where an earlier version of this put its frame.
//!
//! Each CPU's window takes 4 entries of the page directory pointer table
//! of PML4 slot 509; they share one page directory, whose entries all
//! point at one page table, which maps the CPU's page at every 16th entry.

use super::paging::{self, ADDR_MASK, NO_EXEC, PRESENT, WRITABLE};
use super::phys::{self, phys_to_virt};
use crate::smp::MAX_CPUS;

const SLOT: usize = 509;
const BASE: u64 = 0xffff_0000_0000_0000 | (SLOT as u64) << 39;
/// Offset of the frame in each 64 KB: in the mapped (last) page, with room
/// below it.
const FRAME: u64 = 0xff00;

fn table(p: u64) -> &'static mut [u64; 512] {
    unsafe { &mut *(phys_to_virt(p) as *mut [u64; 512]) }
}

/// Map every CPU's window (after paging::init, before address spaces are
/// made: they share the kernel half's PML4 entries).
pub fn init() {
    let pml4 = table(paging::kernel_root());
    let pdpt = pml4[SLOT] & ADDR_MASK;
    for cpu in 0..MAX_CPUS {
        let (Some(page), Some(pt), Some(pd)) = (phys::alloc(), phys::alloc(), phys::alloc()) else {
            panic!("espfix: out of memory");
        };
        for (i, e) in table(pt).iter_mut().enumerate() {
            if i % 16 == 15 {
                *e = page | PRESENT | WRITABLE | NO_EXEC;
            }
        }
        for e in table(pd).iter_mut() {
            *e = pt | PRESENT | WRITABLE;
        }
        for g in 0..4 {
            table(pdpt)[cpu * 4 + g] = pd | PRESENT | WRITABLE;
        }
    }
}

/// Where this CPU returns to a 16-bit stack with ESP `esp`: the program's
/// RAX, then the `iretq` frame (RIP, CS, RFLAGS, RSP, SS) are put there,
/// and trap_return loads RSP from the trap frame's error slot. Interrupts
/// must stay off until then (the window is this CPU's).
pub fn frame(esp: u32, rax: u64, iret: [u64; 5]) -> u64 {
    let cpu = crate::smp::this().index() as u64;
    let at = BASE + (cpu << 32) + (esp as u64 & 0xffff_0000) + FRAME;
    let p = at as *mut u64;
    unsafe {
        p.write(rax);
        for (i, v) in iret.iter().enumerate() {
            p.add(1 + i).write(*v);
        }
    }
    at
}
