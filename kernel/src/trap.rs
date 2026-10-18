//! Trap dispatch: exceptions, IRQs and the API gate.

use crate::arch::idt::{TrapFrame, API_VECTOR};
use crate::arch::read_cr2;

pub const EXCEPTION_NAMES: [&str; 32] = [
    "divide error", "debug", "NMI", "breakpoint", "overflow", "bound range",
    "invalid opcode", "device not available", "double fault", "coprocessor overrun",
    "invalid TSS", "segment not present", "stack fault", "general protection",
    "page fault", "reserved", "x87 error", "alignment check", "machine check",
    "SIMD error", "virtualization", "control protection", "reserved", "reserved",
    "reserved", "reserved", "reserved", "reserved", "hypervisor injection",
    "VMM communication", "security", "reserved",
];

pub const IRQ_BASE: u64 = 0x20;

#[unsafe(no_mangle)]
pub extern "C" fn dispatch(f: &mut TrapFrame) {
    use crate::smp;
    let user = f.from_user();
    match f.vector {
        v if v < 32 => {
            if user {
                // Kernel code runs under the big kernel lock (smp.rs).
                smp::bkl_acquire();
                crate::task::user_exception(f);
            } else if !crate::mm::fault::kernel_fault(f) {
                kernel_exception(f);
            }
        }
        // Interrupt handlers only use spin locks.
        v if (IRQ_BASE..IRQ_BASE + 16).contains(&v) => {
            crate::irq::handle((v - IRQ_BASE) as u8, f);
        }
        v if v == API_VECTOR as u64 => {
            smp::bkl_acquire();
            crate::api::dispatch(f);
        }
        v if (smp::IPI_RESCHEDULE as u64..=smp::IPI_HALT as u64).contains(&v) => smp::ipi(v as u8),
        v if v == crate::apic::SPURIOUS as u64 => {}
        v => crate::println!("spurious interrupt {v:#x}"),
    }
    if user {
        smp::bkl_acquire();
        if crate::task::before_user_return(f) {
            // No references are held in this frame.
            crate::task::sched::exit_current();
        }
        smp::bkl_release();
    }
}

pub fn dump(f: &TrapFrame) {
    use crate::log::print_unlocked as p;
    p(format_args!(
        "rip {:#018x} cs {:#06x} rflags {:#x} rsp {:#018x} ss {:#06x} err {:#x} cr2 {:#x}\n",
        f.rip, f.cs, f.rflags, f.rsp, f.ss, f.error, read_cr2()
    ));
    p(format_args!(
        "rax {:#018x} rbx {:#018x} rcx {:#018x} rdx {:#018x}\n",
        f.rax, f.rbx, f.rcx, f.rdx
    ));
    p(format_args!(
        "rsi {:#018x} rdi {:#018x} rbp {:#018x} r8  {:#018x}\n",
        f.rsi, f.rdi, f.rbp, f.r8
    ));
    p(format_args!(
        "ds {:#06x} es {:#06x} fs {:#06x} gs {:#06x}\n",
        f.ds, f.es, f.fs, f.gs
    ));
}

fn kernel_exception(f: &TrapFrame) -> ! {
    let name = EXCEPTION_NAMES.get(f.vector as usize).copied().unwrap_or("?");
    crate::log::print_unlocked(format_args!("\nKERNEL TRAP {:#x}: {name}\n", f.vector));
    dump(f);
    crate::backtrace(f.rbp);
    crate::arch::halt_forever();
}
