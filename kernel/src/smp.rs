//! Multiprocessor support: per-CPU data, starting the application
//! processors (APs), inter-processor interrupts and the big kernel lock.
//!
//! Each CPU finds its [`PerCpu`] through the GS base register. User code
//! uses GS as an ordinary selector (16-bit OS/2 code especially), so the
//! trap path executes `swapgs` on every transition between user and kernel
//! mode: in the kernel GS base is the per-CPU block, in user mode it is
//! whatever the user's GS selector says.
//!
//! Kernel code (API calls, exceptions, kernel threads) runs under one big
//! kernel lock, taken on entry from user mode and dropped when a thread
//! blocks or returns to user mode; user code runs on all CPUs at once.
//! Interrupt handlers do not take it: they only use spin locks.

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};

use crate::arch;
use crate::task::thread::Thread;

pub const MAX_CPUS: usize = 32;

/// Vectors of the inter-processor interrupts.
pub const IPI_RESCHEDULE: u8 = 0xf0;
pub const IPI_TLB_FLUSH: u8 = 0xf1;
pub const IPI_HALT: u8 = 0xf2;

const MSR_GS_BASE: u32 = 0xc000_0101;
const MSR_KERNEL_GS_BASE: u32 = 0xc000_0102;
const MSR_PAT: u32 = 0x277;

/// Priority class index of the thread a CPU runs (`IDLE_CLASS` = idle).
pub const IDLE_CLASS: u8 = 0xff;

#[repr(C)]
pub struct PerCpu {
    /// Address of this block (read through `gs:[0]`).
    self_ptr: UnsafeCell<*const PerCpu>,
    pub apic_id: AtomicU32,
    pub online: AtomicBool,
    /// Spin locks held on this CPU.
    pub held_locks: AtomicU32,
    pub need_resched: AtomicBool,
    pub slice_end: AtomicU64,
    /// Run queue index (class * 32 + level) of the running thread, for
    /// preemption decisions made on other CPUs.
    pub cur_class: AtomicU8,
    /// Page-table root of the address space this CPU runs (the process's
    /// own root even while CR3 holds this CPU's thread-local memory
    /// copies), 0 for the kernel's.
    pub space_root: AtomicU64,
    /// Only touched by this CPU with interrupts disabled.
    current: UnsafeCell<Option<Arc<Thread>>>,
    idle: UnsafeCell<Option<Arc<Thread>>>,
    /// The thread this CPU just switched away from, released once the
    /// switch is complete (see `task::sched::finish_switch`).
    switched_from: UnsafeCell<Option<Arc<Thread>>>,
    /// This CPU's GDT and TSS.
    pub gdt: UnsafeCell<*mut u64>,
    pub tss: UnsafeCell<*mut arch::gdt::Tss>,
}

unsafe impl Sync for PerCpu {}

impl PerCpu {
    const fn new() -> PerCpu {
        PerCpu {
            self_ptr: UnsafeCell::new(core::ptr::null()),
            apic_id: AtomicU32::new(0),
            online: AtomicBool::new(false),
            held_locks: AtomicU32::new(0),
            need_resched: AtomicBool::new(false),
            slice_end: AtomicU64::new(0),
            cur_class: AtomicU8::new(IDLE_CLASS),
            space_root: AtomicU64::new(0),
            current: UnsafeCell::new(None),
            idle: UnsafeCell::new(None),
            switched_from: UnsafeCell::new(None),
            gdt: UnsafeCell::new(core::ptr::null_mut()),
            tss: UnsafeCell::new(core::ptr::null_mut()),
        }
    }

    /// The running thread. Callers on this CPU only.
    pub fn current(&self) -> &mut Option<Arc<Thread>> {
        unsafe { &mut *self.current.get() }
    }

    pub fn idle(&self) -> &mut Option<Arc<Thread>> {
        unsafe { &mut *self.idle.get() }
    }

    pub fn switched_from(&self) -> &mut Option<Arc<Thread>> {
        unsafe { &mut *self.switched_from.get() }
    }

    /// Position in the CPU table (0 = boot processor).
    pub fn index(&self) -> usize {
        (self as *const PerCpu as usize - CPUS.as_ptr() as usize) / core::mem::size_of::<PerCpu>()
    }

    pub fn bit(&self) -> u64 {
        1 << self.index()
    }
}

static CPUS: [PerCpu; MAX_CPUS] = [const { PerCpu::new() }; MAX_CPUS];

/// GS points at the per-CPU block (false only very early on the BSP).
static GS_READY: AtomicBool = AtomicBool::new(false);
static ONLINE: AtomicU32 = AtomicU32::new(1);

/// This CPU's block.
#[inline]
pub fn this() -> &'static PerCpu {
    if !GS_READY.load(Ordering::Relaxed) {
        return &CPUS[0];
    }
    let p: *const PerCpu;
    unsafe { core::arch::asm!("mov {}, gs:[0]", out(reg) p, options(nostack, readonly, preserves_flags)) };
    unsafe { &*p }
}

pub fn cpu(i: usize) -> &'static PerCpu {
    &CPUS[i]
}

pub fn online_count() -> u32 {
    ONLINE.load(Ordering::Relaxed)
}

/// Online CPUs.
pub fn online() -> impl Iterator<Item = &'static PerCpu> {
    CPUS.iter().filter(|c| c.online.load(Ordering::Acquire))
}

fn set_gs(c: &'static PerCpu) {
    unsafe {
        *c.self_ptr.get() = c as *const PerCpu;
        arch::wrmsr(MSR_GS_BASE, c as *const PerCpu as u64);
        arch::wrmsr(MSR_KERNEL_GS_BASE, 0);
    }
}

/// Set up CPU 0 (the boot processor); called right after the GDT.
pub fn init_bsp() {
    let c = &CPUS[0];
    unsafe {
        *c.gdt.get() = arch::gdt::boot_gdt();
        *c.tss.get() = arch::gdt::boot_tss();
    }
    set_gs(c);
    c.online.store(true, Ordering::Release);
    GS_READY.store(true, Ordering::Release);
}

// ---- The big kernel lock ----

static BKL: AtomicBool = AtomicBool::new(false);


/// Take the kernel lock for the current thread (no-op if it holds it).
/// Interrupts are enabled while waiting, so this CPU still answers TLB
/// shootdowns from the holder.
pub fn bkl_acquire() {
    let Some(t) = crate::task::sched::current_opt() else { return };
    if t.holds_bkl.load(Ordering::Relaxed) || t.is_idle() {
        return;
    }
    let was = arch::interrupts_enabled();
    if BKL.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        arch::enable_interrupts();
        // Test and test-and-set: wait on plain reads, so waiting CPUs do
        // not keep the holder's cache line busy.
        loop {
            while BKL.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
            if BKL.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                break;
            }
        }
        if !was {
            arch::disable_interrupts();
        }
    }
    t.holds_bkl.store(true, Ordering::Relaxed);
}

/// Drop the kernel lock if the current thread holds it.
pub fn bkl_release() {
    let Some(t) = crate::task::sched::current_opt() else { return };
    bkl_release_for(&t);
}

pub fn bkl_release_for(t: &Thread) -> bool {
    if t.holds_bkl.swap(false, Ordering::Relaxed) {
        BKL.store(false, Ordering::Release);
        true
    } else {
        false
    }
}

// ---- Inter-processor interrupts ----

/// Send `vector` to the CPU with LAPIC id `apic`.
pub fn send_ipi(c: &PerCpu, vector: u8) {
    crate::apic::send_ipi(c.apic_id.load(Ordering::Relaxed), vector);
}

/// Make another CPU look at its run queue (it may be halted).
pub fn kick(c: &PerCpu) {
    if !core::ptr::eq(c, this()) {
        send_ipi(c, IPI_RESCHEDULE);
    }
}

static SHOOTDOWN: crate::sync::SpinLock<()> = crate::sync::SpinLock::new(());
static ACKS: AtomicU32 = AtomicU32::new(0);

/// Flush the TLBs of the CPUs in `mask` (other than this one) and wait
/// until they have.
pub fn tlb_shootdown(mask: u64) {
    let me = this().bit();
    let targets: alloc::vec::Vec<&PerCpu> = online().filter(|c| mask & c.bit() != 0 && c.bit() != me).collect();
    if targets.is_empty() {
        return;
    }
    let _g = SHOOTDOWN.lock();
    ACKS.store(0, Ordering::SeqCst);
    for c in &targets {
        send_ipi(c, IPI_TLB_FLUSH);
    }
    let start = arch::rdtsc();
    while ACKS.load(Ordering::SeqCst) < targets.len() as u32 {
        core::hint::spin_loop();
        if arch::rdtsc().wrapping_sub(start) > 20_000_000_000 {
            crate::log::print_unlocked(format_args!("[smp: TLB shootdown not acknowledged]\n"));
            break;
        }
    }
}

/// IPI handlers (called from the trap dispatcher, no kernel lock).
pub fn ipi(vector: u8) {
    match vector {
        IPI_TLB_FLUSH => {
            unsafe { arch::write_cr3(arch::read_cr3()) };
            ACKS.fetch_add(1, Ordering::SeqCst);
        }
        IPI_HALT => {
            arch::disable_interrupts();
            arch::halt_forever();
        }
        _ => {}
    }
    crate::apic::eoi();
}

/// Stop the other CPUs (panic).
pub fn halt_others() {
    let me = this().bit();
    for c in online() {
        if c.bit() != me {
            send_ipi(c, IPI_HALT);
        }
    }
}

// ---- Starting the application processors ----

core::arch::global_asm!(
    r#"
.section .text
.global ap_tramp_start
.global ap_tramp_end
.global tr_gdtr
.global tr_pm
.global tr_lm
.global tr_gdt
.global tr_pm32
.global tr_lm64
.global tr_cr3
.global tr_stack
.global tr_arg
.global tr_entry
.code16
ap_tramp_start:
    cli
    cld
    movw %cs, %ax
    movw %ax, %ds
    xorl %ebx, %ebx
    movw %ax, %bx
    shll $4, %ebx
    lgdtl (tr_gdtr - ap_tramp_start)
    movl %cr0, %eax
    orl $1, %eax
    movl %eax, %cr0
    ljmpl *(tr_pm - ap_tramp_start)
.code32
tr_pm32:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movl %cr4, %eax
    orl $0x20, %eax
    movl %eax, %cr4
    movl (tr_cr3 - ap_tramp_start)(%ebx), %eax
    movl %eax, %cr3
    movl $0xC0000080, %ecx
    rdmsr
    orl $0x900, %eax
    wrmsr
    movl %cr0, %eax
    orl $0x80000000, %eax
    movl %eax, %cr0
    ljmpl *(tr_lm - ap_tramp_start)(%ebx)
.code64
tr_lm64:
    movl %ebx, %ebx
    movq (tr_stack - ap_tramp_start)(%rbx), %rsp
    movq (tr_arg - ap_tramp_start)(%rbx), %rdi
    movq (tr_entry - ap_tramp_start)(%rbx), %rax
    jmpq *%rax
.balign 8
tr_gdt:
    .quad 0
    .quad 0x00cf9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x00af9a000000ffff
tr_gdtr:
    .word 31
    .long 0
tr_pm:
    .long 0
    .word 0x08
tr_lm:
    .long 0
    .word 0x18
.balign 8
tr_cr3:
    .quad 0
tr_stack:
    .quad 0
tr_arg:
    .quad 0
tr_entry:
    .quad 0
ap_tramp_end:
"#,
    options(att_syntax)
);

unsafe extern "C" {
    static ap_tramp_start: u8;
    static ap_tramp_end: u8;
    static tr_gdtr: u8;
    static tr_pm: u8;
    static tr_lm: u8;
    static tr_gdt: u8;
    static tr_pm32: u8;
    static tr_lm64: u8;
    static tr_cr3: u8;
    static tr_stack: u8;
    static tr_arg: u8;
    static tr_entry: u8;
}

fn off(sym: *const u8) -> u64 {
    sym as u64 - core::ptr::addr_of!(ap_tramp_start) as u64
}

/// Kernel stack for an AP's idle context.
const AP_STACK: usize = 64 * 1024;

static PAT: AtomicU64 = AtomicU64::new(0);

/// Page tables the trampoline enters long mode with (below 4 GB): the
/// kernel half of the kernel's tables, plus an identity mapping of the low
/// 2 MB where the trampoline runs.
fn trampoline_tables() -> Option<u64> {
    use crate::mm::paging::{kernel_root, ADDR_MASK};
    use crate::mm::phys::{alloc_contig, phys_to_virt};
    const PRESENT: u64 = 1;
    const WRITABLE: u64 = 2;
    const HUGE: u64 = 0x80;
    let pml4 = alloc_contig(1, 1 << 32)?;
    let pdpt = alloc_contig(1, 1 << 32)?;
    let pd = alloc_contig(1, 1 << 32)?;
    unsafe {
        let k = phys_to_virt(kernel_root()) as *const u64;
        let t = phys_to_virt(pml4) as *mut u64;
        for i in 256..512 {
            *t.add(i) = *k.add(i);
        }
        *t = (pdpt & ADDR_MASK) | PRESENT | WRITABLE;
        *(phys_to_virt(pdpt) as *mut u64) = (pd & ADDR_MASK) | PRESENT | WRITABLE;
        *(phys_to_virt(pd) as *mut u64) = PRESENT | WRITABLE | HUGE;
    }
    Some(pml4)
}

fn delay_ms(ms: u64) {
    let end = crate::irq::ticks() + ms * crate::irq::HZ / 1000 + 1;
    while crate::irq::ticks() < end {
        core::hint::spin_loop();
    }
}

/// Start every processor the MADT lists. Called on the BSP with interrupts
/// enabled and the LAPIC timer running.
pub fn start_aps() {
    let Some(info) = crate::acpi::info() else { return };
    if !crate::apic::active() {
        return;
    }
    let bsp_id = crate::apic::id() as u32;
    CPUS[0].apic_id.store(bsp_id, Ordering::Relaxed);
    let ids: alloc::vec::Vec<u32> = info.cpus.iter().map(|&a| a as u32).filter(|&a| a != bsp_id).collect();
    if ids.is_empty() {
        return;
    }
    if cmdline_has("nosmp") {
        crate::println!("smp: disabled (nosmp), {} processor(s) not started", ids.len());
        return;
    }
    // The trampoline page: below 1 MB, as a SIPI vector names it.
    let Some(page) = crate::mm::phys::low_page() else {
        crate::println!("smp: no low memory for the trampoline");
        return;
    };
    let Some(pml4) = trampoline_tables() else { return };
    PAT.store(unsafe { arch::rdmsr(MSR_PAT) }, Ordering::Relaxed);
    let len = off(core::ptr::addr_of!(ap_tramp_end)) as usize;
    let dst = crate::mm::phys::phys_to_virt(page);
    unsafe {
        core::ptr::copy_nonoverlapping(core::ptr::addr_of!(ap_tramp_start), dst, len);
        let w32 = |o: u64, v: u32| (dst.add(o as usize) as *mut u32).write_unaligned(v);
        w32(off(core::ptr::addr_of!(tr_gdtr)) + 2, (page + off(core::ptr::addr_of!(tr_gdt))) as u32);
        w32(off(core::ptr::addr_of!(tr_pm)), (page + off(core::ptr::addr_of!(tr_pm32))) as u32);
        w32(off(core::ptr::addr_of!(tr_lm)), (page + off(core::ptr::addr_of!(tr_lm64))) as u32);
        (dst.add(off(core::ptr::addr_of!(tr_cr3)) as usize) as *mut u64).write_unaligned(pml4);
        (dst.add(off(core::ptr::addr_of!(tr_entry)) as usize) as *mut u64).write_unaligned(ap_main as *const () as u64);
    }
    let mut started = 0;
    for (n, &apic) in ids.iter().enumerate() {
        let index = n + 1;
        if index >= MAX_CPUS {
            break;
        }
        let c = &CPUS[index];
        c.apic_id.store(apic, Ordering::Relaxed);
        let stack = Box::leak(alloc::vec![0u8; AP_STACK].into_boxed_slice());
        let top = (stack.as_ptr() as u64 + AP_STACK as u64) & !15;
        unsafe {
            (dst.add(off(core::ptr::addr_of!(tr_stack)) as usize) as *mut u64).write_unaligned(top);
            (dst.add(off(core::ptr::addr_of!(tr_arg)) as usize) as *mut u64).write_unaligned(c as *const PerCpu as u64);
        }
        // INIT, then two STARTUPs (the vector is the page number).
        crate::apic::send_init(apic);
        delay_ms(10);
        for _ in 0..2 {
            crate::apic::send_startup(apic, (page >> 12) as u8);
            delay_ms(1);
            if c.online.load(Ordering::Acquire) {
                break;
            }
        }
        // Wait for it to come up before reusing the trampoline.
        let end = crate::irq::ticks() + 200;
        while !c.online.load(Ordering::Acquire) && crate::irq::ticks() < end {
            core::hint::spin_loop();
        }
        if c.online.load(Ordering::Acquire) {
            started += 1;
        } else {
            crate::println!("smp: processor {apic} did not start");
        }
    }
    crate::println!("smp: {} processor(s) online", started + 1);
}

fn cmdline_has(w: &str) -> bool {
    crate::boot_info().cmdline().split_whitespace().any(|x| x.eq_ignore_ascii_case(w))
}

/// First Rust code on an AP (from the trampoline, on its own stack).
extern "C" fn ap_main(c: &'static PerCpu) -> ! {
    unsafe { arch::write_cr3(crate::mm::paging::kernel_root()) };
    set_gs(c);
    arch::gdt::init_ap(c);
    arch::idt::load();
    arch::init_fpu();
    unsafe { arch::wrmsr(MSR_PAT, PAT.load(Ordering::Relaxed)) };
    crate::apic::init_ap();
    let idle = Thread::new_idle();
    *c.idle() = Some(idle.clone());
    *c.current() = Some(idle);
    ONLINE.fetch_add(1, Ordering::SeqCst);
    c.online.store(true, Ordering::Release);
    arch::enable_interrupts();
    crate::task::sched::idle_loop();
}
