// SBTEST.COM: the Sound Blaster at 220h as games drive it: DSP reset and
// version, IRQ 7 hooked and unmasked, DMA channel 1 set up for a block
// of 11025 samples (a 441 Hz square wave, about a second at ~11 kHz),
// the single-cycle 8-bit DMA command, and the end-of-block interrupt.
.intel_syntax noprefix
.code16
.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
.equ LEN, 11025
.text
.globl _start
.org 0x100
_start:
    // Reset the DSP.
    mov dx, 0x226
    mov al, 1
    out dx, al
    in al, dx
    in al, dx
    xor al, al
    out dx, al
    mov cx, 1000
1:  mov dx, 0x22e
    in al, dx
    test al, 0x80
    jnz 2f
    loop 1b
    jmp nodsp
2:  mov dx, 0x22a
    in al, dx
    cmp al, 0xaa
    jne nodsp
    // Version.
    mov al, 0xe1
    call16 dsp_write
    call16 dsp_read
    mov [ver_major], al
    call16 dsp_read
    // IRQ 7 (INT 0Fh).
    mov ax, 0x250f
    mov dx, offset irq7
    int 0x21
    in al, 0x21
    and al, 0x7f
    out 0x21, al
    // The samples: a buffer not crossing a 64 KB page.
    mov ax, cs
    mov bx, offset buf
    call16 lin
    mov si, offset buf
    // If it would cross, start at the next page boundary.
    mov cx, dx
    add ax, LEN
    jnc 3f
    neg cx
    add si, cx
3:  mov di, si
    xor bx, bx
    mov cx, LEN
4:  mov al, 0xc0
    cmp bl, 12
    jb 5f
    mov al, 0x40
5:  stosb
    inc bl
    cmp bl, 25
    jb 6f
    xor bl, bl
6:  loop 4b
    // DMA channel 1: single cycle, memory to device.
    mov ax, cs
    mov bx, si
    call16 lin
    mov [page], dl
    mov [addr], ax
    mov al, 5
    out 0x0a, al
    xor al, al
    out 0x0c, al
    mov al, 0x49
    out 0x0b, al
    mov ax, [addr]
    out 0x02, al
    mov al, ah
    out 0x02, al
    mov al, [page]
    out 0x83, al
    mov ax, LEN - 1
    out 0x03, al
    mov al, ah
    out 0x03, al
    mov al, 1
    out 0x0a, al
    // Speaker on, ~11 kHz, play LEN bytes.
    mov al, 0xd1
    call16 dsp_write
    mov al, 0x40
    call16 dsp_write
    mov al, 165
    call16 dsp_write
    mov al, 0x14
    call16 dsp_write
    mov al, (LEN - 1) & 0xff
    call16 dsp_write
    mov al, (LEN - 1) >> 8
    call16 dsp_write
    // Wait for the interrupt (at most 3 s).
    xor ax, ax
    int 0x1a
    mov bp, dx
7:  cmp byte ptr [done_flag], 0
    jne 8f
    xor ax, ax
    int 0x1a
    sub dx, bp
    cmp dx, 55
    jb 7b
    mov dx, offset s_noirq
    jmp out
8:  mov dx, offset s_ok
out:
    mov ah, 9
    int 0x21
    mov al, 0xd3
    call16 dsp_write
    mov ax, 0x4c00
    int 0x21
nodsp:
    mov ah, 9
    mov dx, offset s_nodsp
    int 0x21
    mov ax, 0x4c01
    int 0x21

// AX:BX -> DX:AX linear (DL page, AX offset in page).
lin:
    mov dx, ax
    shl ax, 4
    shr dx, 12
    add ax, bx
    adc dx, 0
    .byte 0xc3

dsp_write:
    push dx
    push ax
    mov dx, 0x22c
9:  in al, dx
    test al, 0x80
    jnz 9b
    pop ax
    out dx, al
    pop dx
    .byte 0xc3

dsp_read:
    push dx
    mov dx, 0x22e
10: in al, dx
    test al, 0x80
    jz 10b
    mov dx, 0x22a
    in al, dx
    pop dx
    .byte 0xc3

irq7:
    push ax
    push dx
    mov dx, 0x22e
    in al, dx
    mov byte ptr cs:[done_flag], 1
    mov al, 0x20
    out 0x20, al
    pop dx
    pop ax
    iret

ver_major: .byte 0
done_flag: .byte 0
page: .byte 0
addr: .word 0
s_ok: .ascii "SB OK: played a block, got the interrupt\r\n$"
s_noirq: .ascii "SB: no end-of-block interrupt\r\n$"
s_nodsp: .ascii "SB: no DSP\r\n$"
buf: .fill 24000, 1, 0
