// BEEP.COM: the PC speaker through PIT channel 2 and port 61h, as games
// program it: 440 Hz, then 880 Hz, half a second each (timed with the
// BIOS tick count), then a BEL through DOS.
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
    mov bx, 2712            // 1193182 / 440
    call16 tone
    mov bx, 1356            // 880 Hz
    call16 tone
    mov ah, 2
    mov dl, 7
    int 0x21
    mov ah, 9
    mov dx, offset done
    int 0x21
    mov ax, 0x4c00
    int 0x21

// Sound BX (PIT divisor) for 9 ticks (half a second).
tone:
    mov al, 0xb6
    out 0x43, al
    mov al, bl
    out 0x42, al
    mov al, bh
    out 0x42, al
    in al, 0x61
    or al, 3
    out 0x61, al
    xor ax, ax
    int 0x1a
    mov si, dx
1:  xor ax, ax
    int 0x1a
    sub dx, si
    cmp dx, 9
    jb 1b
    in al, 0x61
    and al, 0xfc
    out 0x61, al
    .byte 0xc3
done: .ascii "beep done\r\n$"
