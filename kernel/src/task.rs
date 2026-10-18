//! Processes and threads.

use crate::arch::idt::TrapFrame;

pub fn user_exception(f: &mut TrapFrame) {
    crate::println!("user exception {:#x}", f.vector);
    crate::trap::dump(f);
    crate::arch::halt_forever();
}

pub fn before_user_return(_f: &mut TrapFrame) {}

pub fn timer_tick(_f: &mut TrapFrame) {}

pub fn resolve_user_fault(_addr: u64, _error: u64) -> bool {
    false
}
