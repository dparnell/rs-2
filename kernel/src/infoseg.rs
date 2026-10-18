//! The 16-bit global information segment (GINFOSEG).
//!
//! One read-only page in the shared arena, mapped into every process and
//! described by a GDT selector, updated from the timer.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU16, Ordering};

use crate::arch::gdt::{self, access, descriptor};
use crate::sync::Once;
use crate::vm::{self, AddressSpace, PageState, SegKind, SharedMem, PAGE};

static OBJ: Once<Arc<SharedMem>> = Once::new();
static SELECTOR: AtomicU16 = AtomicU16::new(0);

pub fn global_selector() -> u16 {
    SELECTOR.load(Ordering::Relaxed)
}

fn page() -> *mut u8 {
    let f = OBJ.get().unwrap().frames.lock()[0];
    crate::mm::phys::phys_to_virt(f)
}

pub fn init() {
    let base = vm::reserve_shared(PAGE, None).expect("infoseg address");
    let obj = SharedMem::new(base, PAGE, vm::pag::READ, None);
    obj.frame(0).expect("infoseg memory");
    obj.pages.lock()[0] = PageState::Committed;
    OBJ.set(obj);
    let idx = gdt::FIRST_DYNAMIC;
    gdt::set_entry(
        idx,
        descriptor(base, 0x7f, access::PRESENT | access::DPL3 | access::CODE_DATA | access::ACCESSED, 0),
    );
    SELECTOR.store((idx * 8 | 3) as u16, Ordering::Relaxed);
    let p = page();
    unsafe {
        let w16 = |o: usize, v: u16| (p.add(o) as *mut u16).write_unaligned(v);
        w16(14, 310); // cusecTimerInterval (tenths of ms * 10)
        *p.add(21) = 20; // major version
        *p.add(22) = 45; // minor version
        *p.add(23) = 0;
        *p.add(24) = 1; // sgCurrent
        *p.add(25) = 16; // sgMax
        *p.add(26) = 3; // cHugeShift
        *p.add(27) = 1; // fProtectModeOnly
        w16(28, 1); // pidForeground
        *p.add(30) = 1; // fDynamicSched
        *p.add(31) = 3; // csecMaxWait
        w16(32, 32); // cmsecMinSlice
        w16(34, 248); // cmsecMaxSlice
        w16(36, 3); // bootdrive C:
        *p.add(70) = 16; // csgWindowableVioMax
        *p.add(71) = 16; // csgPMMax
    }
    update();
}

/// The foreground session changed (sgCurrent, pidForeground).
pub fn set_foreground(sg: u32, pid: u32) {
    if OBJ.get().is_none() {
        return;
    }
    let p = page();
    unsafe {
        *p.add(24) = sg as u8;
        (p.add(28) as *mut u16).write_unaligned(pid as u16);
    }
}

/// Map the infoseg into a new address space.
pub fn attach(space: &mut AddressSpace) {
    if let Some(o) = OBJ.get() {
        space.map_shared(o.clone(), vm::pag::READ, SegKind::None);
    }
}

/// Refresh the time fields.
pub fn update() {
    if OBJ.get().is_none() {
        return;
    }
    let t = crate::rtc::now();
    let ms = crate::irq::uptime_ms();
    let p = page();
    unsafe {
        (p as *mut u32).write_unaligned(crate::rtc::unix_time(&t));
        (p.add(4) as *mut u32).write_unaligned(ms as u32);
        *p.add(8) = t.hour;
        *p.add(9) = t.minute;
        *p.add(10) = t.second;
        *p.add(11) = ((ms / 10) % 100) as u8;
        (p.add(12) as *mut u16).write_unaligned(0xffff);
        *p.add(16) = t.day;
        *p.add(17) = t.month;
        (p.add(18) as *mut u16).write_unaligned(t.year);
        *p.add(20) = t.weekday;
    }
}
