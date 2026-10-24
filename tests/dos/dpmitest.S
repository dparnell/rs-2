// DPMITEST.COM: a DPMI 0.9 client as DOS extenders are. It enters
// protected mode as a 32-bit client and tests: output through INT 31h
// 0300h (simulated real-mode INT 21h), LDT descriptors (a flat 4 GB
// selector writing to video memory), a 1 MB memory block (filled and
// checked with 32-bit string instructions), a 32-bit code segment
// (far call, 32-bit arithmetic), an exception handler (#DE, skipping the
// faulting DIV), a protected-mode interrupt handler (INT 60h), a
// real-mode callback reached through 0301h, DOS memory (0100h), the
// version (0400h); then it ends with INT 21h 4Ch from protected mode.
// Each test prints "ok" or "FAILED"; the exit code is the failures.
.intel_syntax noprefix
.code16
.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
.macro ret16
    .byte 0xc3
.endm
// Print the $-string at label through INT 31h 0300h (protected mode).
.macro PRINT label
    mov dx, offset \label
    call16 pm_print
.endm
.macro RESULT ok
    jz 1f
    inc byte ptr [failures]
    PRINT s_failed
    jmp 2f
1:  PRINT s_ok
2:
.endm
.text
.globl _start
.org 0x100
_start:
    // Keep 32 KB, the stack inside it.
    mov sp, 0x7ffe
    mov ah, 0x4a
    mov bx, 0x800
    int 0x21
    mov [rm_seg], cs
    mov ah, 9
    mov dx, offset s_title
    int 0x21
    mov ax, 0x1687
    int 0x2f
    test ax, ax
    jz 1f
    mov ah, 9
    mov dx, offset s_nodpmi
    int 0x21
    mov ax, 0x4c01
    int 0x21
1:  mov [entry], di
    mov [entry + 2], es
    mov [host_paras], si
    // Host data, if it wants some.
    test si, si
    jz 2f
    mov ah, 0x48
    mov bx, si
    int 0x21
    mov es, ax
2:  mov ax, 1
    .byte 0xff, 0x1e
    .word entry
    jnc in_pm
    mov ah, 9
    mov dx, offset s_noenter
    int 0x21
    mov ax, 0x4c02
    int 0x21

in_pm:
    mov [data_sel], ds
    mov [cs_data_sel], ds
    PRINT s_pm
    // Version.
    mov ax, 0x0400
    int 0x31
    cmp ax, 0x005a
    PRINT s_version
    cmp ax, 0x005a
    RESULT

    // A flat selector: base 0, limit 4 GB; a character into video memory.
    PRINT s_flat
    xor ax, ax
    mov cx, 1
    int 0x31
    jc 3f
    mov [flat_sel], ax
    mov bx, ax
    mov ax, 0x0007
    xor cx, cx
    xor dx, dx
    int 0x31
    jc 3f
    mov ax, 0x0008
    mov cx, 0xffff
    mov dx, 0xffff
    int 0x31
    jc 3f
    push es
    mov es, [flat_sel]
    mov edi, 0xb8000 + 2 * 79
    mov word ptr es:[edi], 0x2f2a
    mov ax, es:[edi]
    pop es
    cmp ax, 0x2f2a
3:  RESULT

    // A 1 MB block, filled and read back through a selector for it.
    PRINT s_mem
    mov ax, 0x0501
    mov bx, 0x0010
    xor cx, cx
    int 0x31
    jc 4f
    mov [blk_hi], bx
    mov [blk_lo], cx
    xor ax, ax
    mov cx, 1
    int 0x31
    jc 4f
    mov [blk_sel], ax
    mov bx, ax
    mov ax, 0x0007
    mov cx, [blk_hi]
    mov dx, [blk_lo]
    int 0x31
    mov ax, 0x0008
    mov cx, 0x000f
    mov dx, 0xffff
    int 0x31
    jc 4f
    push es
    mov es, [blk_sel]
    xor edi, edi
    mov ecx, 0x40000
    mov eax, 0x12345678
    cld
    .byte 0x67
    rep stosd
    mov ebx, es:[0xffffc]
    mov edx, es:[0x80000]
    pop es
    cmp ebx, 0x12345678
    jne 4f
    cmp edx, 0x12345678
4:  RESULT

    // A 32-bit code segment: an alias of ours with the D bit.
    PRINT s_code32
    xor ax, ax
    mov cx, 1
    int 0x31
    jc 5f
    mov [code32_sel], ax
    mov bx, cs
    mov ax, 0x0006
    int 0x31
    mov bx, [code32_sel]
    mov ax, 0x0007
    int 0x31
    mov ax, 0x0008
    xor cx, cx
    mov dx, 0xffff
    int 0x31
    mov ax, 0x0009
    mov cl, 0xfa
    mov ch, 0x40
    int 0x31
    jc 5f
    mov ax, [code32_sel]
    mov [far32 + 4], ax
    mov dword ptr [far32], offset code32
    mov ebx, 1000
    // call fword [far32]
    .byte 0x66, 0xff, 0x1e
    .word far32
    cmp eax, 1000 * 1000 + 7
5:  RESULT

    // Exception handler for #DE: it skips the DIV.
    PRINT s_exc
    mov ax, 0x0203
    mov bl, 0
    mov cx, cs
    mov edx, offset de_handler
    int 0x31
    jc 6f
    mov byte ptr [exc_seen], 0
    xor bl, bl
    mov ax, 100
    div bl
    cmp byte ptr [exc_seen], 1
6:  RESULT

    // A protected-mode interrupt handler.
    PRINT s_int
    mov ax, 0x0205
    mov bl, 0x60
    mov cx, cs
    mov edx, offset int60
    int 0x31
    jc 7f
    mov word ptr [int_count], 0
    int 0x60
    int 0x60
    cmp word ptr [int_count], 2
7:  RESULT

    // A real-mode callback, called from real mode through 0301h.
    PRINT s_rmcb
    push ds
    pop es
    mov ax, 0x0303
    push cs
    pop ds
    mov esi, offset rmcb_proc
    mov edi, offset cb_regs
    int 0x31
    mov ds, [cs_data_sel]
    jc 8f
    mov [rm_regs + 42], dx
    mov [rm_regs + 44], cx
    mov word ptr [rm_regs + 46], 0
    mov word ptr [rm_regs + 48], 0
    mov byte ptr [rmcb_seen], 0
    push ds
    pop es
    mov ax, 0x0301
    xor bx, bx
    xor cx, cx
    mov edi, offset rm_regs
    int 0x31
    jc 8f
    cmp byte ptr [rmcb_seen], 1
8:  RESULT

    // DOS memory.
    PRINT s_dosmem
    mov ax, 0x0100
    mov bx, 0x100
    int 0x31
    jc 9f
    push es
    mov es, dx
    mov word ptr es:[0xffe], 0x55aa
    mov si, es:[0xffe]
    pop es
    push dx
    mov ax, 0x0101
    int 0x31
    pop dx
    cmp si, 0x55aa
9:  RESULT

    PRINT s_done
    mov al, [failures]
    mov ah, 0x4c
    int 0x21

// Print the $-string at DS:DX (protected mode): INT 21h AH=09h in real
// mode with DS = our segment.
pm_print:
    push es
    push di
    push ax
    push bx
    push cx
    mov [rm_regs + 20], dx
    mov word ptr [rm_regs + 22], 0
    mov byte ptr [rm_regs + 29], 9
    mov ax, [rm_seg]
    mov [rm_regs + 36], ax
    mov word ptr [rm_regs + 46], 0
    mov word ptr [rm_regs + 48], 0
    mov word ptr [rm_regs + 32], 0
    push ds
    pop es
    mov edi, offset rm_regs
    mov ax, 0x0300
    mov bl, 0x21
    xor bh, bh
    xor cx, cx
    int 0x31
    pop cx
    pop bx
    pop ax
    pop di
    pop es
    ret16

// #DE handler (32-bit client frame on the locked stack): EIP += 2.
de_handler:
    push ds
    mov ds, cs:[cs_data_sel]
    mov byte ptr [exc_seen], 1
    pop ds
    add dword ptr ss:[esp + 12], 2
    .byte 0x66, 0xcb

int60:
    push ds
    mov ds, cs:[cs_data_sel]
    inc word ptr [int_count]
    pop ds
    .byte 0x66, 0xcf

// The real-mode callback: DS:SI is the real-mode stack (its far return
// address), ES:DI the register structure. Return as RETF would.
rmcb_proc:
    mov ax, [si]
    mov es:[di + 42], ax
    mov ax, [si + 2]
    mov es:[di + 44], ax
    add word ptr es:[di + 46], 4
    push ds
    mov ds, cs:[cs_data_sel]
    mov byte ptr [rmcb_seen], 1
    pop ds
    .byte 0x66, 0xcf

.code32
code32:
    mov eax, ebx
    imul eax, ebx
    add eax, 7
    retf
.code16

cs_data_sel: .word 0
s_title: .ascii "rs2 DPMI test\r\n$"
s_nodpmi: .ascii "No DPMI host.\r\n$"
s_noenter: .ascii "Cannot enter protected mode.\r\n$"
s_pm: .ascii "In protected mode (32-bit client).\r\n$"
s_version: .ascii "Version 0.90: $"
s_flat: .ascii "Flat 4 GB selector, video memory: $"
s_mem: .ascii "1 MB memory block: $"
s_code32: .ascii "32-bit code segment: $"
s_exc: .ascii "Exception handler (#DE): $"
s_int: .ascii "Protected-mode INT 60h handler: $"
s_rmcb: .ascii "Real-mode callback: $"
s_dosmem: .ascii "DOS memory block: $"
s_ok: .ascii "ok\r\n$"
s_failed: .ascii "FAILED\r\n$"
s_done: .ascii "DPMI test done.\r\n$"
.balign 4
entry: .long 0
host_paras: .word 0
rm_seg: .word 0
data_sel: .word 0
flat_sel: .word 0
blk_sel: .word 0
blk_hi: .word 0
blk_lo: .word 0
code32_sel: .word 0
far32: .long 0
    .word 0
failures: .byte 0
exc_seen: .byte 0
rmcb_seen: .byte 0
.balign 2
int_count: .word 0
rm_regs: .fill 50, 1, 0
cb_regs: .fill 50, 1, 0
