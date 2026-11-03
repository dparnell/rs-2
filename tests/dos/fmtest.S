// FMTEST.COM: the FM synthesizer as games find and play it: AdLib
// detection through timer 1 at 388h, the OPL3 check (status bits 1-2
// clear), then OPL3 mode and two half-second sine tones on channel 0:
// 440 Hz on the left only, 880 Hz on the right only (timed with the BIOS
// tick count).
.intel_syntax noprefix
.code16
.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
.text
.globl _start
.org 0x100
_start:
    // Reset both timers and the IRQ flag.
    mov ax, 0x6004
    call16 opl_w
    mov ax, 0x8004
    call16 opl_w
    mov dx, 0x388
    in al, dx
    mov [s1], al
    // Timer 1 at FFh (80 us), started.
    mov ax, 0xff02
    call16 opl_w
    mov ax, 0x2104
    call16 opl_w
    mov cx, 200
1:  in al, dx
    loop 1b
    in al, dx
    mov [s2], al
    mov ax, 0x6004
    call16 opl_w
    mov ax, 0x8004
    call16 opl_w
    mov al, [s1]
    and al, 0xe0
    jnz noadlib
    mov al, [s2]
    and al, 0xe0
    cmp al, 0xc0
    jne noadlib
    mov ah, 9
    mov dx, offset s_adlib
    int 0x21
    test byte ptr [s1], 6
    jnz noopl3
    mov ah, 9
    mov dx, offset s_opl3
    int 0x21
    // OPL3 mode (bank 1, register 5).
    mov dx, 0x38a
    mov al, 5
    out dx, al
    inc dx
    mov al, 1
    out dx, al
    // Channel 0: both operators sine, carrier at full level.
    mov si, offset voice
2:  lodsw
    test ax, ax
    jz 3f
    call16 opl_w
    jmp 2b
3:  // 440 Hz, left.
    mov ax, 0x11c0
    call16 opl_w
    mov ax, 0x44a0
    call16 opl_w
    mov ax, 0x32b0
    call16 opl_w
    call16 wait9
    mov ax, 0x12b0
    call16 opl_w
    // 880 Hz, right.
    mov ax, 0x21c0
    call16 opl_w
    mov ax, 0x36b0
    call16 opl_w
    call16 wait9
    mov ax, 0x16b0
    call16 opl_w
    mov ah, 9
    mov dx, offset s_ok
    int 0x21
    mov ax, 0x4c00
    int 0x21
noadlib:
    mov dx, offset s_noadlib
    jmp fail
noopl3:
    mov dx, offset s_noopl3
fail:
    mov ah, 9
    int 0x21
    mov ax, 0x4c01
    int 0x21

// Write AH to FM register AL (bank 0), with the delays the chip needs.
opl_w:
    push cx
    push dx
    mov dx, 0x388
    out dx, al
    mov cx, 6
4:  in al, dx
    loop 4b
    inc dx
    mov al, ah
    out dx, al
    dec dx
    mov cx, 35
5:  in al, dx
    loop 5b
    pop dx
    pop cx
    .byte 0xc3

// Wait 9 BIOS ticks (half a second).
wait9:
    xor ax, ax
    int 0x1a
    mov bp, dx
6:  xor ax, ax
    int 0x1a
    sub dx, bp
    cmp dx, 9
    jb 6b
    .byte 0xc3

// (value << 8 | register) pairs for channel 0's operators.
voice:
    .word 0x0120, 0x0123, 0x3f40, 0x0043, 0xf060, 0xf063, 0x0080, 0x0083, 0x00e0, 0x00e3, 0
s1: .byte 0
s2: .byte 0
s_adlib: .ascii "FM: AdLib found\r\n$"
s_opl3: .ascii "FM: OPL3 found\r\n$"
s_ok: .ascii "FM OK: played 440 Hz left, 880 Hz right\r\n$"
s_noadlib: .ascii "FM FAILED: no AdLib (timer)\r\n$"
s_noopl3: .ascii "FM FAILED: not an OPL3\r\n$"
