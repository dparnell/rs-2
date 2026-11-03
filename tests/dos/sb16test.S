// SB16TEST.COM: the Sound Blaster 16 as games drive it: DSP version 4,
// the mixer's IRQ and DMA registers (80h/81h), then a second of 16-bit
// signed stereo at 22050 Hz by auto-init DMA on channel 5 (a 1 kHz square
// wave left, 500 Hz right), with the interrupt at each half of the
// buffer acknowledged at 22Fh, and the end of auto-init (D9h).
.intel_syntax noprefix
.code16
.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
// Frames in the whole buffer (two blocks).
.equ FRAMES, 4096
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
    mov al, 0xe1
    call16 dsp_write
    call16 dsp_read
    cmp al, 4
    jne notsb16
    call16 dsp_read
    // Mixer: IRQ 7 (bit 2), DMA 1 and 5.
    mov dx, 0x224
    mov al, 0x80
    out dx, al
    inc dx
    in al, dx
    cmp al, 4
    jne badmixer
    dec dx
    mov al, 0x81
    out dx, al
    inc dx
    in al, dx
    cmp al, 0x22
    jne badmixer
    // IRQ 7 (INT 0Fh).
    mov ax, 0x250f
    mov dx, offset irq7
    int 0x21
    in al, 0x21
    and al, 0x7f
    out 0x21, al
    // The buffer: 16 KB not crossing a 128 KB boundary.
    mov ax, cs
    mov dx, ax
    shl ax, 4
    shr dx, 12
    add ax, offset buf
    adc dx, 0
    // DX:AX linear. Crossing if (low 17 bits) + 16384 > 128 KB.
    mov si, offset buf
    test dl, 1
    jz 3f
    cmp ax, 0x10000 - FRAMES * 4
    jbe 3f
    neg ax
    add si, ax
    inc dx
    xor ax, ax
3:  mov [lin_lo], ax
    mov [lin_hi], dl
    // Fill: left toggles every 11 frames, right every 22.
    mov di, si
    xor bx, bx
    xor bp, bp
    mov cx, FRAMES
4:  mov ax, 8000
    cmp bl, 11
    jb 5f
    neg ax
5:  stosw
    mov ax, 8000
    cmp bp, 22
    jb 6f
    neg ax
6:  stosw
    inc bl
    cmp bl, 22
    jb 7f
    xor bl, bl
7:  inc bp
    cmp bp, 44
    jb 8f
    xor bp, bp
8:  loop 4b
    // DMA channel 5: auto-init, memory to device, word address.
    mov al, 5
    out 0xd4, al
    xor al, al
    out 0xd8, al
    mov al, 0x59
    out 0xd6, al
    mov ax, [lin_lo]
    mov dl, [lin_hi]
    shr dl, 1
    rcr ax, 1
    out 0xc4, al
    mov al, ah
    out 0xc4, al
    mov al, [lin_hi]
    and al, 0xfe
    out 0x8b, al
    mov ax, FRAMES * 2 - 1
    out 0xc6, al
    mov al, ah
    out 0xc6, al
    mov al, 1
    out 0xd4, al
    // 22050 Hz, 16-bit auto-init, signed stereo, half the buffer a block.
    mov al, 0x41
    call16 dsp_write
    mov al, 0x56
    call16 dsp_write
    mov al, 0x22
    call16 dsp_write
    mov al, 0xb6
    call16 dsp_write
    mov al, 0x30
    call16 dsp_write
    mov al, (FRAMES - 1) & 0xff
    call16 dsp_write
    mov al, (FRAMES - 1) >> 8
    call16 dsp_write
    // About a second: 10 blocks of 2048 frames (at most 4 s).
    xor ax, ax
    int 0x1a
    mov bp, dx
9:  cmp byte ptr [irqs], 10
    jae 10f
    xor ax, ax
    int 0x1a
    sub dx, bp
    cmp dx, 73
    jb 9b
    mov dx, offset s_noirq
    jmp out
10: // End auto-init: one more block, then it stops.
    mov al, 0xd9
    call16 dsp_write
    xor ax, ax
    int 0x1a
    mov bp, dx
11: cmp byte ptr [irqs], 11
    jae 12f
    xor ax, ax
    int 0x1a
    sub dx, bp
    cmp dx, 18
    jb 11b
    mov dx, offset s_noend
    jmp out
12: mov dx, offset s_ok
out:
    mov ah, 9
    int 0x21
    mov ax, 0x4c00
    int 0x21
nodsp:
    mov dx, offset s_nodsp
    jmp fail
notsb16:
    mov dx, offset s_notsb16
    jmp fail
badmixer:
    mov dx, offset s_mixer
fail:
    mov ah, 9
    int 0x21
    mov ax, 0x4c01
    int 0x21

dsp_write:
    push dx
    push ax
    mov dx, 0x22c
13: in al, dx
    test al, 0x80
    jnz 13b
    pop ax
    out dx, al
    pop dx
    .byte 0xc3

dsp_read:
    push dx
    mov dx, 0x22e
14: in al, dx
    test al, 0x80
    jz 14b
    mov dx, 0x22a
    in al, dx
    pop dx
    .byte 0xc3

irq7:
    push ax
    push dx
    // Which interrupt: mixer register 82h bit 1 is 16-bit DMA.
    mov dx, 0x224
    mov al, 0x82
    out dx, al
    inc dx
    in al, dx
    test al, 2
    jz 15f
    inc byte ptr cs:[irqs]
    mov dx, 0x22f
    in al, dx
15: mov al, 0x20
    out 0x20, al
    pop dx
    pop ax
    iret

irqs: .byte 0
lin_hi: .byte 0
lin_lo: .word 0
s_ok: .ascii "SB16 OK: 16-bit stereo auto-init DMA, 11 interrupts\r\n$"
s_noirq: .ascii "SB16 FAILED: interrupts missing\r\n$"
s_noend: .ascii "SB16 FAILED: no interrupt after D9h\r\n$"
s_nodsp: .ascii "SB16 FAILED: no DSP\r\n$"
s_notsb16: .ascii "SB16 FAILED: DSP version is not 4\r\n$"
s_mixer: .ascii "SB16 FAILED: mixer IRQ/DMA registers\r\n$"
buf: .fill 32768, 1, 0
