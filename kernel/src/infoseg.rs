//! The 16-bit global information segment (GINFOSEG).
//!
//! One read-only page in the shared arena, mapped into every process and
//! described by a GDT selector, updated from the timer.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU8, Ordering};

use crate::arch::gdt::{self, access, descriptor};
use crate::sync::{Once, SpinLock};
use crate::vm::{self, AddressSpace, PageState, SegKind, SharedMem, PAGE};

static OBJ: Once<Arc<SharedMem>> = Once::new();
/// The first megabyte as Windows programs see it through KERNEL's
/// __0000H, __0040H ... __F000H selectors (docs/WIN16.md): the BIOS data
/// area at 400h with the fields programs read, the timer tick count at
/// 46Ch kept current, and the top of the ROM BIOS (its date at
/// F000:FFF5, the machine model at F000:FFFE: Open Watcom's 16-bit
/// Windows runtime reads them). Programs may write it, as on Windows
/// (setup programs keep the floppy motor on through 40:40); it is one
/// object for all of them, like the real memory, its pages zero until
/// touched.
static BIOS: Once<Arc<SharedMem>> = Once::new();
/// The size of that image.
const LOW_MEM: u32 = 0x10_0000;
/// Where the BIOS data area is in it.
const BDA: usize = 0x400;
/// KERNEL's real-mode segments and the WIN16 ordinals of their selectors
/// (__0000H 183, __0040H 193, __A000H 174, __B000H 181, __B800H 182,
/// __C000H 195, __D000H 179, __E000H 190, __F000H 194): GDT entries from
/// FIRST_DYNAMIC + 1 on, in this order.
const REAL_SEGMENTS: [(u16, u16); 9] =
    [(183, 0x0000), (193, 0x0040), (174, 0xa000), (181, 0xb000), (182, 0xb800), (195, 0xc000), (179, 0xd000), (190, 0xe000), (194, 0xf000)];
/// Milliseconds of the day at uptime 0 (from the RTC).
static DAY_OFFSET: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static SELECTOR: AtomicU16 = AtomicU16::new(0);

/// The global information segment's flat address.
pub fn base() -> u32 {
    OBJ.get().map_or(0, |o| o.base)
}

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
        w16(12, 0xffff); // timezone: unknown
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
    let base = vm::reserve_shared(LOW_MEM, None).expect("BIOS data address");
    let bios = SharedMem::new(base, LOW_MEM, vm::pag::READ | vm::pag::WRITE, None);
    bios.pages.lock().fill(PageState::Committed);
    let b = crate::mm::phys::phys_to_virt(bios.frame(0).expect("BIOS data memory"));
    unsafe {
        let b = b.add(BDA);
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
    let rom = crate::mm::phys::phys_to_virt(bios.frame(0xff).expect("BIOS ROM memory"));
    unsafe {
        // F000:FFF0: a far jump to the reset code, the BIOS date, the
        // model byte (FCh: an AT).
        let top = rom.add(0xff0);
        core::ptr::copy_nonoverlapping([0xea, 0x5b, 0xe0, 0x00, 0xf0].as_ptr(), top, 5);
        core::ptr::copy_nonoverlapping(b"01/01/26".as_ptr(), top.add(5), 8);
        *top.add(0xe) = 0xfc;
    }
    let data = access::PRESENT | access::DPL3 | access::CODE_DATA | access::WRITABLE | access::ACCESSED;
    for (i, &(_, seg)) in REAL_SEGMENTS.iter().enumerate() {
        gdt::set_entry(gdt::FIRST_DYNAMIC + 1 + i, descriptor(base + seg as u32 * 16, 0xffff, data, 0));
    }
    BIOS.set(bios);
    // Selector 40h: Windows' DPMI host gives programs a GDT selector for
    // real-mode segment 40h (the Delphi compiler reads the tick count
    // through it); it is mapped in Windows programs only.
    gdt::set_entry(8, descriptor(base + BDA as u32, 0xffff, data, 0));
    update();
}

/// The selector of KERNEL's real-mode segment export with WIN16 ordinal
/// `ordinal` (__0040H ... __F000H), if it is one.
pub fn real_mode_selector(ordinal: u16) -> Option<u16> {
    BIOS.get()?;
    let i = REAL_SEGMENTS.iter().position(|&(o, _)| o == ordinal)?;
    Some(((gdt::FIRST_DYNAMIC + 1 + i) * 8 | 3) as u16)
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
    unsafe { (p.add(BDA + 0x6c) as *mut u32).write_unaligned(ticks) };
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

/// Offset of the time fields' sequence count in the page: past IBM's
/// GINFOSEG (0x48 bytes), inside the selector's limit. Odd while the
/// timer (or DosSetDateTime) rewrites the time fields.
pub const SEQ_OFFSET: usize = 0x7c;

/// One writer of the time fields at a time: the timer on CPU 0 and
/// DosSetDateTime on any CPU.
static WRITER: SpinLock<()> = SpinLock::new(());

/// Refresh the time fields.
///
/// Programs read them with no lock, as IBM's DOSCALL1 does (DosGetDateTime
/// copies the hour..hundredths dword at 8 and the day..year dword at
/// 0x10). Each group is therefore stored as one aligned dword, never byte
/// by byte, so such a read sees a whole time or a whole date; the groups
/// go in the order of IBM's clock driver (seconds since 1970, then the
/// time of day, then the date and weekday). A reader that wants all of
/// them consistent (rs2's own) brackets its reads with the sequence count
/// at [`SEQ_OFFSET`]: even and unchanged means no update ran in between.
pub fn update() {
    if OBJ.get().is_none() {
        return;
    }
    if crate::irq::ticks() % 256 == 0 {
        crate::rtc::resync();
    }
    let _w = WRITER.lock();
    let ms = crate::irq::uptime_ms();
    let now = crate::rtc::now_ms();
    let t = crate::rtc::from_unix((now / 1000) as u32);
    let day = now % 86_400_000;
    DAY_OFFSET.store((day + 86_400_000 - ms % 86_400_000) % 86_400_000, Ordering::Relaxed);
    let p = page();
    // The page is mapped into every process; the fields are aligned
    // dwords (GINFOSEG's layout), stored whole.
    let field = |o: usize| unsafe { &*(p.add(o) as *const AtomicU32) };
    let seq = field(SEQ_OFFSET);
    let n = seq.load(Ordering::Relaxed).wrapping_add(1);
    seq.store(n, Ordering::Relaxed);
    core::sync::atomic::fence(Ordering::Release);
    field(0).store(crate::rtc::unix_time(&t), Ordering::Relaxed);
    field(4).store(ms as u32, Ordering::Relaxed);
    let hund = ((now % 1000) / 10) as u8;
    field(8).store(u32::from_le_bytes([t.hour, t.minute, t.second, hund]), Ordering::Relaxed);
    let y = t.year.to_le_bytes();
    field(16).store(u32::from_le_bytes([t.day, t.month, y[0], y[1]]), Ordering::Relaxed);
    unsafe { &*(p.add(20) as *const AtomicU8) }.store(t.weekday, Ordering::Relaxed);
    seq.store(n.wrapping_add(1), Ordering::Release);
}
