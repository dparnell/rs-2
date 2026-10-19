//! Memory management.

pub mod fault;
pub mod heap;
pub mod mmio;
pub mod paging;
pub mod tlm;
pub mod phys;

pub fn init(info: &bootinfo::BootInfo) {
    phys::init(info);
    paging::init();
}
