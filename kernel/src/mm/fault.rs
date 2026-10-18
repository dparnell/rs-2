//! Page faults taken in kernel mode.

use crate::arch::idt::TrapFrame;

/// Try to resolve a fault raised by kernel code (for example while touching
/// user memory). Returns true if execution can continue.
pub fn kernel_fault(f: &mut TrapFrame) -> bool {
    if f.vector == 14 {
        let addr = crate::arch::read_cr2();
        if addr < 0x8000_0000_0000 {
            return crate::task::resolve_user_fault(addr, f.error);
        }
    }
    false
}
