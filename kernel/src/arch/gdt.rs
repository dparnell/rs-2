//! Global descriptor table and TSS.
//!
//! The kernel runs in 64-bit mode; user code runs in compatibility mode
//! through the OS/2 selectors applications expect:
//!
//! | selector | use |
//! |----------|-----|
//! | `0x08`   | kernel 64-bit code |
//! | `0x10`   | kernel data |
//! | `0x20`   | TSS (two slots) |
//! | `0x30`   | current process LDT (two slots) |
//! | `0x40`   | the BIOS data area of Windows programs (infoseg.rs) |
//! | `0x53`   | user flat data (`DS`/`ES`/`SS` of 32-bit code) |
//! | `0x5B`   | user flat 32-bit code |
//! | `0x150B` | thread information block (`FS` of 32-bit code) |
//!
//! Entries from `FIRST_DYNAMIC` up are handed out for global selectors such
//! as the 16-bit info segments.
//!
//! Every CPU has its own copy (the TSS, LDT and TIB entries differ per
//! CPU); [`set_entry`] writes a global entry into all of them.

use core::arch::asm;
use core::mem::size_of;

pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
pub const TSS_SEL: u16 = 0x20;
pub const LDT_SEL: u16 = 0x30;
pub const USER_DS: u16 = 0x53;
pub const USER_CS: u16 = 0x5b;
pub const TIB_SEL: u16 = 0x150b;

pub const ENTRIES: usize = 8192;
pub const FIRST_DYNAMIC: usize = 0x2c0;

#[repr(C, packed)]
pub struct Tss {
    _r0: u32,
    pub rsp: [u64; 3],
    _r1: u64,
    pub ist: [u64; 7],
    _r2: u64,
    _r3: u16,
    pub iomap: u16,
}

#[repr(C, align(4096))]
struct Gdt([u64; ENTRIES]);

static mut GDT: Gdt = Gdt([0; ENTRIES]);
static mut TSS: Tss = Tss { _r0: 0, rsp: [0; 3], _r1: 0, ist: [0; 7], _r2: 0, _r3: 0, iomap: 0 };

/// Stacks for double faults and NMIs, used through the IST.
#[repr(C, align(16))]
struct Stack([u8; 16384]);
static mut DF_STACK: Stack = Stack([0; 16384]);
static mut NMI_STACK: Stack = Stack([0; 16384]);

/// Build a segment descriptor.
///
/// `access` is the access byte (P DPL S type), `flags` the high nibble
/// (G D/B L AVL).
pub const fn descriptor(base: u32, limit: u32, access: u8, flags: u8) -> u64 {
    let limit = limit as u64;
    let base = base as u64;
    (limit & 0xffff)
        | (base & 0xff_ffff) << 16
        | (access as u64) << 40
        | ((limit >> 16) & 0xf) << 48
        | ((flags & 0xf) as u64) << 52
        | ((base >> 24) & 0xff) << 56
}

/// Access bytes.
pub mod access {
    pub const PRESENT: u8 = 0x80;
    pub const DPL3: u8 = 0x60;
    pub const DPL2: u8 = 0x40;
    pub const CODE_DATA: u8 = 0x10;
    pub const CODE: u8 = 0x08;
    pub const CONFORMING: u8 = 0x04;
    pub const EXPAND_DOWN: u8 = 0x04;
    pub const READABLE: u8 = 0x02;
    pub const WRITABLE: u8 = 0x02;
    pub const ACCESSED: u8 = 0x01;
    pub const LDT: u8 = 0x02;
    pub const TSS64: u8 = 0x09;
}

/// Descriptor flags (upper nibble).
pub mod flags {
    pub const GRANULARITY: u8 = 0x8;
    pub const BIG: u8 = 0x4;
    pub const LONG: u8 = 0x2;
}

/// This CPU's GDT.
fn gdt() -> &'static mut [u64; ENTRIES] {
    let p = unsafe { *crate::smp::this().gdt.get() };
    if p.is_null() {
        return unsafe { &mut (*core::ptr::addr_of_mut!(GDT)).0 };
    }
    unsafe { &mut *(p as *mut [u64; ENTRIES]) }
}

pub fn boot_gdt() -> *mut u64 {
    unsafe { (*core::ptr::addr_of_mut!(GDT)).0.as_mut_ptr() }
}

pub fn boot_tss() -> *mut Tss {
    core::ptr::addr_of_mut!(TSS)
}

/// Set a global entry, on every CPU.
pub fn set_entry(index: usize, value: u64) {
    unsafe { (*core::ptr::addr_of_mut!(GDT)).0[index] = value };
    for c in crate::smp::online() {
        let p = unsafe { *c.gdt.get() };
        if !p.is_null() {
            unsafe { *p.add(index) = value };
        }
    }
}

pub fn entry(index: usize) -> u64 {
    gdt()[index]
}

/// Write a 16-byte system descriptor (TSS or LDT).
fn set_system(index: usize, base: u64, limit: u32, ty: u8) {
    let g = gdt();
    g[index] = descriptor(base as u32, limit, access::PRESENT | ty, 0);
    g[index + 1] = base >> 32;
}

/// Point the LDT slot at `base` (a kernel virtual address) and load it.
pub fn load_ldt(base: u64, entries: usize) {
    if entries == 0 {
        unsafe { asm!("lldt {0:x}", in(reg) 0u16, options(nostack)) };
        return;
    }
    set_system(LDT_SEL as usize / 8, base, (entries * 8 - 1) as u32, access::LDT);
    unsafe { asm!("lldt {0:x}", in(reg) LDT_SEL, options(nostack)) };
}

/// Set the base of the TIB selector (done on every thread switch).
pub fn set_tib_base(base: u32, limit: u32) {
    use access::*;
    gdt()[TIB_SEL as usize / 8] =
        descriptor(base, limit, PRESENT | DPL3 | CODE_DATA | WRITABLE | ACCESSED, flags::BIG);
}

/// Kernel stack used when an interrupt arrives from user mode (this CPU).
pub fn set_kernel_stack(rsp0: u64) {
    let t = unsafe { *crate::smp::this().tss.get() };
    let t = if t.is_null() { core::ptr::addr_of_mut!(TSS) } else { t };
    unsafe { (*t).rsp[0] = rsp0 };
}

pub const IST_DOUBLE_FAULT: u8 = 1;
pub const IST_NMI: u8 = 2;

#[repr(C, align(4096))]
struct ApTables {
    gdt: Gdt,
    tss: Tss,
    df: Stack,
    nmi: Stack,
}

/// Give an application processor its own GDT (a copy of the boot one),
/// TSS and exception stacks, and load them.
pub fn init_ap(c: &'static crate::smp::PerCpu) {
    let t: &'static mut ApTables = alloc::boxed::Box::leak(unsafe { alloc::boxed::Box::new_zeroed().assume_init() });
    unsafe {
        t.gdt.0.copy_from_slice(&(*core::ptr::addr_of!(GDT)).0);
        t.tss.ist[IST_DOUBLE_FAULT as usize - 1] = core::ptr::addr_of!(t.df) as u64 + size_of::<Stack>() as u64;
        t.tss.ist[IST_NMI as usize - 1] = core::ptr::addr_of!(t.nmi) as u64 + size_of::<Stack>() as u64;
        t.tss.iomap = size_of::<Tss>() as u16;
        *c.gdt.get() = t.gdt.0.as_mut_ptr();
        *c.tss.get() = core::ptr::addr_of_mut!(t.tss);
    }
    let tss = core::ptr::addr_of!(t.tss) as u64;
    set_system(TSS_SEL as usize / 8, tss, size_of::<Tss>() as u32 - 1, access::TSS64);
    load(t.gdt.0.as_ptr());
}

/// Load a GDT, reload the segment registers and the task register.
fn load(base: *const u64) {
    #[repr(C, packed)]
    struct Ptr {
        limit: u16,
        base: u64,
    }
    let ptr = Ptr { limit: (ENTRIES * 8 - 1) as u16, base: base as u64 };
    unsafe {
        asm!(
            "lgdt [{ptr}]",
            "push {cs}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ds, {ds:x}",
            "mov es, {ds:x}",
            "mov ss, {ds:x}",
            "xor {tmp:e}, {tmp:e}",
            "mov fs, {tmp:x}",
            // GS is left alone: its base is the per-CPU block (smp.rs).
            "ltr {tss:x}",
            ptr = in(reg) &ptr,
            cs = in(reg) KERNEL_CS as u64,
            ds = in(reg) KERNEL_DS,
            tss = in(reg) TSS_SEL,
            tmp = out(reg) _,
        );
    }
}

pub fn init() {
    use access::*;
    let g = gdt();
    g[1] = descriptor(0, 0xfffff, PRESENT | CODE_DATA | CODE | READABLE | ACCESSED, flags::LONG | flags::GRANULARITY);
    g[2] = descriptor(0, 0xfffff, PRESENT | CODE_DATA | WRITABLE | ACCESSED, flags::BIG | flags::GRANULARITY);
    g[USER_DS as usize / 8] = descriptor(
        0,
        0xfffff,
        PRESENT | DPL3 | CODE_DATA | WRITABLE | ACCESSED,
        flags::BIG | flags::GRANULARITY,
    );
    g[USER_CS as usize / 8] = descriptor(
        0,
        0xfffff,
        PRESENT | DPL3 | CODE_DATA | CODE | READABLE | ACCESSED,
        flags::BIG | flags::GRANULARITY,
    );
    set_tib_base(0, 0xfff);

    unsafe {
        let tss = &mut *core::ptr::addr_of_mut!(TSS);
        tss.ist[IST_DOUBLE_FAULT as usize - 1] =
            core::ptr::addr_of!(DF_STACK) as u64 + size_of::<Stack>() as u64;
        tss.ist[IST_NMI as usize - 1] = core::ptr::addr_of!(NMI_STACK) as u64 + size_of::<Stack>() as u64;
        tss.iomap = size_of::<Tss>() as u16;
        set_system(TSS_SEL as usize / 8, core::ptr::addr_of!(TSS) as u64, size_of::<Tss>() as u32 - 1, TSS64);
    }
    load(g.as_ptr());
}
