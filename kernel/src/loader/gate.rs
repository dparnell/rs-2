//! The API gate: a shared code object of `int 0x99` stubs.
//!
//! Slot `module * ORDINALS + ordinal` lives at `GATE_BASE + slot * 4`. The
//! same bytes serve 32-bit callers (flat address) and 16-bit callers (16:16
//! through the gate's tiled code selector); the kernel identifies the API
//! from the address of the trapping instruction.

use alloc::sync::Arc;

use crate::sync::Once;
use crate::vm::{self, PageState, SharedMem, GATE_BASE, GATE_SIZE, PAGE};

pub const ORDINALS: u32 = 2048;
pub const SLOT_SIZE: u32 = 4;

/// Kernel-implemented modules, in gate slot order.
pub const MODULES: &[&str] =
    &["DOSCALLS", "VIOCALLS", "KBDCALLS", "MOUCALLS", "MSG", "NLS", "SESMGR", "QUECALLS"];

pub fn module_index(name: &str) -> Option<u32> {
    MODULES.iter().position(|m| m.eq_ignore_ascii_case(name)).map(|i| i as u32)
}

pub fn slot_address(module: u32, ordinal: u32) -> u32 {
    GATE_BASE + (module * ORDINALS + ordinal) * SLOT_SIZE
}

/// Decode a gate address back into (module index, ordinal).
pub fn decode(linear: u32) -> Option<(u32, u32)> {
    if !(GATE_BASE..GATE_BASE + GATE_SIZE).contains(&linear) || (linear - GATE_BASE) % SLOT_SIZE != 0 {
        return None;
    }
    let slot = (linear - GATE_BASE) / SLOT_SIZE;
    let m = slot / ORDINALS;
    ((m as usize) < MODULES.len()).then_some((m, slot % ORDINALS))
}

static GATE: Once<Arc<SharedMem>> = Once::new();

pub fn shared() -> Arc<SharedMem> {
    GATE.get().expect("gate not initialised").clone()
}

pub fn init() {
    vm::reserve_shared(GATE_SIZE, Some(GATE_BASE)).expect("gate address");
    let obj = SharedMem::new(GATE_BASE, GATE_SIZE, vm::pag::READ | vm::pag::EXECUTE, None);
    for i in 0..(GATE_SIZE / PAGE) as usize {
        let frame = obj.frame(i).expect("gate memory");
        let page = crate::mm::phys::phys_to_virt(frame);
        for s in 0..(PAGE / SLOT_SIZE) as usize {
            unsafe {
                // int 0x99 ; int3 ; int3
                page.add(s * 4).copy_from_nonoverlapping([0xcd, 0x99, 0xcc, 0xcc].as_ptr(), 4);
            }
        }
        obj.pages.lock()[i] = PageState::Committed;
    }
    GATE.set(obj);
}
