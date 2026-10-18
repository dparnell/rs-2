//! Interrupt descriptor table and the common trap path.
//!
//! Every vector has a 16-byte stub that pushes a dummy error code (when the
//! CPU does not push one) and the vector number, then jumps to
//! `trap_common`, which saves all registers into a [`TrapFrame`] and calls
//! [`crate::trap::dispatch`]. The same path serves exceptions, hardware
//! interrupts and the OS/2 API gate.

use core::arch::{asm, global_asm};

use super::gdt::{self, KERNEL_CS};

/// Saved user or kernel state. Layout matches `trap_common`.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct TrapFrame {
    pub gs: u64,
    pub fs: u64,
    pub es: u64,
    pub ds: u64,
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl TrapFrame {
    pub fn from_user(&self) -> bool {
        self.cs & 3 != 0
    }
}

global_asm!(
    r#"
.section .text
.global trap_stubs
.align 16
trap_stubs:
.set i, 0
.rept 256
    .align 16
    .if (i == 8) || (i == 10) || (i == 11) || (i == 12) || (i == 13) || (i == 14) || (i == 17) || (i == 21) || (i == 29) || (i == 30)
    .else
        push 0
    .endif
    push i
    jmp trap_common
    .set i, i + 1
.endr

trap_common:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    xor eax, eax
    mov ax, ds
    push rax
    mov ax, es
    push rax
    mov ax, fs
    push rax
    mov ax, gs
    push rax
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov rdi, rsp
    cld
    call {dispatch}

.global trap_return
trap_return:
    test qword ptr [rsp + 22*8], 3
    jz 1f
    // Returning to user mode: selectors were validated by the dispatcher.
    mov rax, [rsp + 3*8]
    mov ds, ax
    mov rax, [rsp + 2*8]
    mov es, ax
    mov rax, [rsp + 1*8]
    mov fs, ax
    mov rax, [rsp + 0*8]
    mov gs, ax
1:
    add rsp, 32
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq
"#,
    dispatch = sym crate::trap::dispatch,
);

unsafe extern "C" {
    static trap_stubs: u8;
    pub fn trap_return();
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Gate {
    off_lo: u16,
    sel: u16,
    ist: u8,
    attr: u8,
    off_mid: u16,
    off_hi: u32,
    _r: u32,
}

static mut IDT: [Gate; 256] =
    [Gate { off_lo: 0, sel: 0, ist: 0, attr: 0, off_mid: 0, off_hi: 0, _r: 0 }; 256];

/// Vector of the OS/2 API gate used by the loader-generated thunks.
pub const API_VECTOR: u8 = 0x99;

fn set_gate(v: u8, dpl: u8, ist: u8) {
    let addr = core::ptr::addr_of!(trap_stubs) as u64 + v as u64 * 16;
    unsafe {
        (*core::ptr::addr_of_mut!(IDT))[v as usize] = Gate {
            off_lo: addr as u16,
            sel: KERNEL_CS,
            ist,
            attr: 0x8e | (dpl << 5), // present, 64-bit interrupt gate
            off_mid: (addr >> 16) as u16,
            off_hi: (addr >> 32) as u32,
            _r: 0,
        };
    }
}

pub fn init() {
    for v in 0..=255u8 {
        set_gate(v, 0, 0);
    }
    set_gate(2, 0, gdt::IST_NMI);
    set_gate(8, 0, gdt::IST_DOUBLE_FAULT);
    // Reachable from ring 3: breakpoint, overflow (INTO) and the API gate.
    set_gate(3, 3, 0);
    set_gate(4, 3, 0);
    set_gate(API_VECTOR, 3, 0);
    load();
}

pub fn load() {
    #[repr(C, packed)]
    struct Ptr {
        limit: u16,
        base: u64,
    }
    let ptr = Ptr { limit: 256 * 16 - 1, base: core::ptr::addr_of!(IDT) as u64 };
    unsafe { asm!("lidt [{}]", in(reg) &ptr, options(nostack)) };
}
