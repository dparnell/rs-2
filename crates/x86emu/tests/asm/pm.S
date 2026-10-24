// Protected mode tests: entering and leaving protected mode, segment
// loads and their faults, LAR/LSL/VERR/VERW,
// interrupt gates, ring 3 through IRET, call gates with parameters, the
// TSS stack switch, the LDT, 16-bit code segments, unreal mode. Every
// exception goes to one handler that writes the vector, the error code,
// the faulting IP and CS to port E9h and resumes at `recover`.
//
// Linked at 10000h (the payload address), so flat 32-bit code can use
// labels as they are; real-mode code (CS = 1000h) subtracts BASE.
.intel_syntax noprefix
.equ BASE, 0x10000
.equ STACK0, 0x1fff0
.equ STACK3, 0x18000

.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
.macro ret16
    .byte 0xc3
.endm
// Mark the start of a test: a tag byte, and where a fault resumes.
.macro TRY tag, resume
    mov al, \tag
    call out8
    mov dword ptr [recover], offset \resume
    mov [saved_esp], esp
.endm

.code16
.text
.globl _start
_start:
    cli
    mov ax, cs
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0xfff0
    mov al, 'S'
    out 0xe9, al
    lgdt [R_gdtr]
    mov eax, cr0
    or al, 1
    mov cr0, eax
    .byte 0x66, 0xea
    .long pm32
    .word 0x08

.code32
pm32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov esp, STACK0
    mov al, 'P'
    call out8
    // The IDT: exception stubs, then the test gates.
    xor ecx, ecx
1:  lea eax, [exc_stubs + ecx * 8]
    sub eax, ecx
    mov edx, 0x8e00
    call set_gate
    inc ecx
    cmp ecx, 32
    jb 1b
    mov ecx, 0x30
    mov eax, offset handler_31
    mov edx, 0x8e00
    call set_gate
    mov ecx, 0x31
    mov edx, 0xee00
    call set_gate
    mov ecx, 0x32
    mov eax, offset handler_32
    call set_gate
    mov ecx, 0x33
    mov eax, offset handler_31
    mov edx, 0xef00
    call set_gate
    lidt [idtr]
    // TSS, LDT and the call gate.
    mov edi, offset gdt + 0x38
    mov eax, offset tss
    call set_base
    mov edi, offset gdt + 0x70
    mov eax, offset ldt
    call set_base
    mov eax, offset call_target
    mov word ptr [gdt + 0x68], ax
    shr eax, 16
    mov word ptr [gdt + 0x6e], ax
    mov dword ptr [tss + 4], STACK0
    mov dword ptr [tss + 8], 0x10
    mov ax, 0x38
    ltr ax
    mov ax, 0x70
    lldt ax
    mov al, [gdt + 0x3d]
    call out8

    // Segment loads that fault.
    // (A null DS loads; using it faults on real CPUs, not in QEMU's TCG.)
    TRY 'a', t_b
    mov eax, 3
    mov ds, ax
    mov ax, ds
    mov bx, 0x10
    mov ds, bx
    call out16
t_b:
    TRY 'b', t_c
    mov ax, 0x90
    mov ds, ax
t_c:
    TRY 'c', t_d
    mov ax, 0x50
    mov es, ax
t_d:
    TRY 'd', t_e
    mov ax, 0x58
    mov ds, ax
t_e:
    TRY 'e', t_f
    mov ax, 0x60
    mov ss, ax
t_f:
    TRY 'f', t_g
    xor eax, eax
    mov ss, ax
t_g:
    // Limits and access rights on memory accesses are not checked by
    // QEMU's TCG, so they are tested in tests/pm.rs instead; here only
    // the load (the accessed bit) and a jump to execute-only code.
    TRY 'g', t_m
    mov ax, 0x40
    mov es, ax
    .byte 0xea
    .long exec_only
    .word 0x58
exec_only:
    mov al, 'x'
    call out8
    .byte 0xea
    .long t_m
    .word 0x08
t_m:
    // The accessed bit of 40h, set by the loads above.
    mov al, [gdt + 0x45]
    call out8
    // LAR, LSL, VERR, VERW.
    mov esi, offset sels
2:  movzx ebx, word ptr [esi]
    cmp bx, 0xffff
    je t_n
    mov eax, 0x5a5a5a5a
    lar eax, ebx
    call out_zf
    call out32
    mov eax, 0x5a5a5a5a
    lsl eax, ebx
    call out_zf
    call out32
    verr bx
    call out_zf
    verw bx
    call out_zf
    add esi, 2
    jmp 2b
t_n:
    // Exceptions without and with error codes, traps.
    TRY 'n', t_o
    xor ecx, ecx
    div ecx
t_o:
    TRY 'o', t_p
    ud2
t_p:
    TRY 'p', t_q
    mov eax, 10
    bound eax, [bounds]
    mov al, '+'
    call out8
    mov eax, 30
    bound eax, [bounds]
t_q:
    TRY 'q', t_r
    mov al, 0x7f
    add al, 1
    into
t_r:
    TRY 'r', t_s
    int3
t_s:
    TRY 's', t_t
    mov ebx, 0x12345678
    int 0x30
    mov al, '='
    call out8
t_t:
    // A 16-bit code segment called from 32-bit code.
    TRY 't', t_u
    .byte 0x9a
    .long R_code16
    .word 0x18
    call out32
t_u:
    // Ring 3.
    TRY 'u', t_v
    mov eax, offset r3_a
    call to_ring3
t_v:
    TRY 'v', t_w
    mov eax, offset r3_b
    call to_ring3
t_w:
    TRY 'w', t_x
    mov eax, offset r3_c
    call to_ring3
t_x:
    TRY 'x', t_y
    mov eax, offset r3_d
    call to_ring3
t_y:
    TRY 'y', t_z
    mov eax, offset r3_e
    call to_ring3
t_z:
    TRY 'z', t_end
    mov eax, offset r3_f
    xor edx, edx
    call to_ring3_fl
t_end:
    // Back to real mode, with FS a 4 GB data segment (unreal mode).
    mov ax, 0x80
    mov fs, ax
    .byte 0xea
    .long R_real16
    .word 0x18

// ecx = vector, eax = handler, dx = type word.
set_gate:
    mov [idt + ecx * 8], ax
    mov word ptr [idt + ecx * 8 + 2], 0x08
    mov [idt + ecx * 8 + 4], dx
    push eax
    shr eax, 16
    mov [idt + ecx * 8 + 6], ax
    pop eax
    ret

// edi = descriptor, eax = base.
set_base:
    mov [edi + 2], ax
    shr eax, 16
    mov [edi + 4], al
    mov [edi + 7], ah
    ret

out8:
    out 0xe9, al
    ret
out16:
    push eax
    out 0xe9, al
    mov al, ah
    out 0xe9, al
    pop eax
    ret
out32:
    push eax
    call out16
    shr eax, 16
    call out16
    pop eax
    ret
// The zero flag as '1' or '0'.
out_zf:
    push eax
    setz al
    add al, '0'
    call out8
    pop eax
    ret

// Enter ring 3 at eax with IOPL 3 (to_ring3_fl: flags in edx).
to_ring3:
    mov edx, 0x3002
to_ring3_fl:
    push 0x33
    push STACK3
    push edx
    push 0x2b
    push eax
    iretd

exc_common:
    pop eax
    call out8
    mov ecx, 0x27d00
    bt ecx, eax
    jnc 1f
    pop ecx
    jmp 2f
1:  xor ecx, ecx
2:  mov eax, ecx
    call out16
    pop eax
    call out16
    pop eax
    call out8
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov esp, [saved_esp]
    jmp dword ptr [recover]

// INT 30h (ring 0 gate) and 31h/33h (ring 3 gates): what the frame holds.
handler_31:
    push eax
    mov ax, cs
    call out8
    mov eax, [esp + 8]
    call out16
    mov eax, [esp + 12]
    and eax, 0x3200
    call out16
    test byte ptr [esp + 8], 3
    jz 1f
    mov eax, [esp + 16]
    call out16
    mov eax, [esp + 20]
    call out16
1:  mov eax, ebx
    call out32
    pop eax
    iretd

// INT 32h: back from ring 3 to the next test.
handler_32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov esp, [saved_esp]
    jmp dword ptr [recover]

// Call gate 68h target (ring 0) with two dword parameters.
call_target:
    mov ax, cs
    call out8
    mov eax, [esp + 4]
    call out16
    mov eax, [esp + 8]
    call out32
    mov eax, [esp + 12]
    call out32
    mov eax, [esp + 16]
    call out16
    mov eax, [esp + 20]
    call out16
    mov ax, 0x10
    mov ds, ax
    retf 8

r3_a:
    mov ax, 0x33
    mov ds, ax
    mov es, ax
    mov al, 'R'
    call out8
    mov ax, cs
    call out8
    mov ebx, 0xcafe0003
    int 0x31
    mov ebx, 0xcafe0033
    int 0x33
    mov al, 'r'
    call out8
    hlt
r3_b:
    int 0x30
r3_c:
    mov ax, 0x33
    mov ds, ax
    mov eax, esp
    call out32
    push 0x11111111
    push 0x22222222
    .byte 0x9a
    .long 0
    .word 0x6b
    mov ax, ds
    call out16
    mov eax, esp
    call out32
    int 0x32
r3_d:
    mov ax, 0x13
    mov ds, ax
r3_e:
    mov ax, 0x07
    mov es, ax
    mov eax, es:[0]
    call out32
    mov ax, 0x0f
    mov es, ax
r3_f:
    // IOPL 0: POPFD cannot change IOPL or IF; CLI faults.
    pushfd
    pop eax
    or eax, 0x3200
    push eax
    popfd
    pushfd
    pop ebx
    int 0x31
    cli

.code16
code16:
    mov ax, 0x20
    mov es, ax
    mov eax, es:[R_magic]
    .byte 0x66, 0xcb
real16:
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov eax, cr0
    and al, 0xfe
    mov cr0, eax
    .byte 0xea
    .word R_rm_back
    .word 0x1000
rm_back:
    mov ax, cs
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0xfff0
    lidt [R_rm_idtr]
    mov al, 'Q'
    out 0xe9, al
    mov ebx, 0x10fff0
    mov dword ptr fs:[ebx], 0x13579bdf
    mov eax, fs:[ebx]
    call16 rout32
    xor ax, ax
    mov es, ax
    mov word ptr es:[0x80 * 4], offset R_rm_int
    mov es:[0x80 * 4 + 2], cs
    mov ax, 0x4321
    int 0x80
    call16 rout32
    mov al, 'E'
    out 0xe9, al
    out 0xf4, al
1:  hlt
    jmp 1b
rm_int:
    push bp
    mov bp, sp
    mov ax, [bp + 6]
    call16 rout32
    mov eax, 0x87654321
    pop bp
    iret
rout32:
    push eax
    out 0xe9, al
    mov al, ah
    out 0xe9, al
    shr eax, 16
    out 0xe9, al
    mov al, ah
    out 0xe9, al
    pop eax
    ret16

.balign 8
exc_stubs:
.set i, 0
.rept 32
    .byte 0x6a, i
    .byte 0xe9
    .long exc_common - . - 4
.set i, i + 1
.endr

// Code and data descriptors used here come with the accessed bit set:
// QEMU does not set it when it loads CS, real processors do.
.balign 8
gdt:
    .quad 0
    .quad 0x00cf9b000000ffff        // 08 code32 ring 0
    .quad 0x00cf93000000ffff        // 10 data32 ring 0
    .quad 0x00009b010000ffff        // 18 code16 ring 0, base 10000h
    .quad 0x000093010000ffff        // 20 data16 ring 0, base 10000h
    .quad 0x00cffb000000ffff        // 28 code32 ring 3
    .quad 0x00cff3000000ffff        // 30 data32 ring 3
    .quad 0x0000890000000067        // 38 TSS (base set at run time)
    .quad 0x0000920200000fff        // 40 data, base 20000h, limit FFFh
    .quad 0x0000960200000fff        // 48 expand down data16, limit FFFh
    .quad 0x00cf12000000ffff        // 50 not present
    .quad 0x00cf99000000ffff        // 58 execute-only code32
    .quad 0x00cf90000000ffff        // 60 read-only data
    .quad 0x0000ec0200080000        // 68 call gate DPL 3, 2 params (offset at run time)
    .quad 0x000082000000000f        // 70 LDT (base at run time)
    .quad 0x00cff3000000ffff        // 78 data32 ring 3
    .quad 0x008f93000000ffff        // 80 data16, 4 GB (unreal mode)
gdt_end:
gdtr:
    .word gdt_end - gdt - 1
    .long gdt
idtr:
    .word 0x34 * 8 - 1
    .long idt
rm_idtr:
    .word 0x3ff
    .long 0
ldt:
    .quad 0x0040f3030000ffff        // 07 data32 ring 3, base 30000h
    .quad 0x0040720300000fff        // 0F not present
sels:
    .word 0x08, 0x10, 0x18, 0x2b, 0x38, 0x40, 0x48, 0x50, 0x58, 0x60, 0x68, 0x70, 0x07, 0x0f, 0x90, 0x00, 0x13, 0xffff
bounds:
    .long 0, 20
magic:
    .long 0xfeedface
recover:
    .long 0
saved_esp:
    .long 0
.balign 8
tss:
    .fill 0x68, 1, 0
idt:
    .fill 0x34 * 8, 1, 0

// Offsets from the payload start for real-mode code.
.set R_code16, code16 - _start
.set R_gdtr, gdtr - _start
.set R_magic, magic - _start
.set R_real16, real16 - _start
.set R_rm_back, rm_back - _start
.set R_rm_idtr, rm_idtr - _start
.set R_rm_int, rm_int - _start
