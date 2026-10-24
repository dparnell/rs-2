// Boot sector for running a CPU test payload on QEMU (the oracle): loads
// the sectors after it to 1000:0000 with INT 13h extended reads and jumps
// there. The payload ends by writing to port F4h (isa-debug-exit).
.intel_syntax noprefix
.code16
.text
.globl _start
_start:
    cli
    xor ax, ax
    mov ds, ax
    mov ss, ax
    mov sp, 0x7c00
    sti
    mov si, offset dap + 0x7c00
1:  mov ah, 0x42
    int 0x13
    jc fail
    add word ptr [si + 6], 0x0800      // next 32 KB
    add dword ptr [si + 8], 64
    dec byte ptr [count + 0x7c00]
    jnz 1b
    cli
    ljmp 0x1000, 0
fail:
    mov al, 'E'
    out 0xe9, al
    out 0xf4, al
    hlt
count: .byte 4
.balign 4
dap:
    .byte 16, 0
    .word 64
    .word 0, 0x1000
    .long 1, 0
.org 510
    .byte 0x55, 0xaa
