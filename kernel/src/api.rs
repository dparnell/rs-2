//! The OS/2 API gate.

use crate::arch::idt::TrapFrame;

pub fn dispatch(f: &mut TrapFrame) {
    crate::println!("API call {:#x}", f.rax);
}
