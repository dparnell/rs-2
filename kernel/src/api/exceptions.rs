//! Delivery of processor exceptions to OS/2 programs.
//!
//! Until exception handler chains are walked, an exception ends the process
//! with the message OS/2 would put in its popup.

use alloc::sync::Arc;

use crate::arch::idt::TrapFrame;
use crate::task::thread::Thread;
use crate::task::Process;

pub fn deliver(f: &mut TrapFrame, p: &Arc<Process>, t: &Arc<Thread>) {
    let (sys, what) = match f.vector {
        0 => ("SYS3172", "divide by zero"),
        6 => ("SYS3176", "illegal instruction"),
        13 | 12 | 11 | 14 => ("SYS3175", "access violation"),
        16 | 19 => ("SYS3173", "floating point error"),
        _ => ("SYS3170", "unexpected exception"),
    };
    crate::println!(
        "\n{sys}: The system detected a {what} in {} (process {}, thread {}).",
        p.exe.name,
        p.pid,
        t.tid
    );
    crate::println!(
        "TRAP {:04x} ERRCD={:04x} CS:EIP={:04x}:{:08x} SS:ESP={:04x}:{:08x} CR2={:08x}",
        f.vector,
        f.error,
        f.cs,
        f.rip,
        f.ss,
        f.rsp,
        crate::arch::read_cr2()
    );
    crate::println!(
        "EAX={:08x} EBX={:08x} ECX={:08x} EDX={:08x} ESI={:08x} EDI={:08x} EBP={:08x}",
        f.rax as u32,
        f.rbx as u32,
        f.rcx as u32,
        f.rdx as u32,
        f.rsi as u32,
        f.rdi as u32,
        f.rbp as u32
    );
    crate::println!("DS={:04x} ES={:04x} FS={:04x} GS={:04x}", f.ds, f.es, f.fs, f.gs);
    p.exit(2, 0xffff);
}
