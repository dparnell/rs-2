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
/// The BIOS data area of Windows programs (KERNEL's __0040H selector,
/// docs/WIN16.md): a page with the fields programs read, the timer tick
/// count at 6Ch kept current. Programs may write it, as on Windows (setup
/// programs keep the floppy motor on through 40:40); it is one page for
/// all of them, like the real BIOS data area.
static BIOS: Once<Arc<SharedMem>> = Once::new();
/// Milliseconds of the day at uptime 0 (from the RTC).
static DAY_OFFSET: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
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
    let base = vm::reserve_shared(PAGE, None).expect("BIOS data address");
    let bios = SharedMem::new(base, PAGE, vm::pag::READ | vm::pag::WRITE, None);
    let f = bios.frame(0).expect("BIOS data memory");
    bios.pages.lock()[0] = PageState::Committed;
    let b = crate::mm::phys::phys_to_virt(f);
    unsafe {
        let w16 = |o: usize, v: u16| (b.add(o) as *mut u16).write_unaligned(v);
        w16(0x10, 0x0026); // equipment: FPU, mouse, 80x25 colour
        w16(0x13, 640); // conventional memory (KB)
        *b.add(0x49) = 3; // video mode
        w16(0x4a, 80); // columns
        w16(0x4c, 4000); // page size
        w16(0x63, 0x3d4); // CRT controller
        *b.add(0x84) = 24; // rows - 1
        w16(0x85, 16); // character height
    }
    BIOS.set(bios);
    // Selector 40h: Windows' DPMI host gives programs a GDT selector for
    // real-mode segment 40h (the Delphi compiler reads the tick count
    // through it); it is mapped in Windows programs only.
    gdt::set_entry(8, descriptor(base, 0xfff, access::PRESENT | access::DPL3 | access::CODE_DATA | access::WRITABLE | access::ACCESSED, 0));
    update();
}

/// The BIOS data area's selector (0: none).
pub fn bios_selector() -> u16 {
    BIOS.get().map_or(0, |b| vm::tile_selector(b.base))
}

/// Map the BIOS data area into a Windows program's address space.
pub fn attach_bios(space: &mut AddressSpace) {
    if let Some(b) = BIOS.get() {
        if space.region(b.base).is_none() {
            space.map_shared(b.clone(), vm::pag::READ | vm::pag::WRITE, SegKind::Data);
        }
    }
}

/// The BIOS timer tick count (18.2 a second since midnight).
pub fn bios_tick() {
    let Some(b) = BIOS.get() else { return };
    let ms = (crate::irq::uptime_ms() + DAY_OFFSET.load(Ordering::Relaxed)) % 86_400_000;
    let ticks = (ms * 1_193_180 / 65_536_000) as u32;
    let p = crate::mm::phys::phys_to_virt(b.frames.lock()[0]);
    unsafe { (p.add(0x6c) as *mut u32).write_unaligned(ticks) };
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
    let day = ((t.hour as u64 * 60 + t.minute as u64) * 60 + t.second as u64) * 1000;
    DAY_OFFSET.store((day + 86_400_000 - ms % 86_400_000) % 86_400_000, Ordering::Relaxed);
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
